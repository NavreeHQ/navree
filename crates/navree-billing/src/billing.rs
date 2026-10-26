//! [`Billing`]: the service. Checkout, subscriptions, trials, usage,
//! invoices, the portal and the webhook pipeline.

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use http::HeaderMap;
use http::request::Parts;
use navree_contracts::{Clock, Dispatcher, JobEnvelope};
use navree_db::{Db, Uow};
use navree_events::EventBus;
use navree_http::AppError;
use navree_kernel::WeakAppContext;
use navree_workspace::entity::member;
use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DbErr, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder, Set, SqlErr,
};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::billable::{AsBillable, BillableRef, BillableRegistration, WORKSPACE_BILLABLE};
use crate::checkout::{Checkout, CheckoutBuilder};
use crate::config::BillingOptions;
use crate::entity::{
    BillingCustomer, BillingEventRecord, customer, event, subscription, subscription_item, usage,
};
use crate::error::BillingError;
use crate::events::{
    InvoicePaid, PaymentFailed, Pending, SubscriptionCanceled, SubscriptionCreated,
    SubscriptionUpdated, TrialEnding,
};
use crate::invoice::Invoice;
use crate::plan::{Interval, Plan, Plans, PriceRole};
use crate::provider::fake::BillingFake;
use crate::provider::{
    BillingProvider, CheckoutRequest, NewCustomer, ProviderInvoice, ProviderSubscription,
    SubscriptionChange, UsageReport, WebhookAction, WebhookEvent,
};
use crate::subscription::{Proration, Subscription, SubscriptionStatus};
use crate::webhook::{ProcessBillingEvent, WebhookReceipt};

/// Cached invoice lists: billable → (expiry, invoices).
type InvoiceCache = HashMap<(String, Uuid), (DateTime<Utc>, Vec<Invoice>)>;

/// What `billing:report-usage` did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UsageReportSummary {
    /// Batches (one per billable and meter) sent to the provider.
    pub reported: usize,
    /// The units they carried.
    pub units: u64,
    /// Batches the provider refused or did not answer (kept for the next run).
    pub failed: usize,
    /// Rows left unreported because the billable has no valid subscription
    /// with that meter any more.
    pub skipped: usize,
}

/// Billing: make a billable pay through Stripe, Paddle or the fake, and
/// read its state from the local copy.
///
/// Provided by [`BillingModule`](crate::BillingModule); inject it as
/// `Arc<Billing>`. Every method takes the billable as `&B` where `B` is a
/// [`Billable`](crate::Billable) (a workspace, a user, `CurrentWorkspace`,
/// `CurrentUser`, ...) or a [`BillableRef`].
///
/// Writes join the task's unit of work when there is an unfinished one (a
/// request's, `Db::transaction`), and use their own otherwise. Calls that
/// change the provider's state update the provider first and the local copy
/// after it; the webhook that follows is then a no-op.
pub struct Billing {
    db: Arc<Db>,
    options: Arc<BillingOptions>,
    plans: Plans,
    provider: Arc<dyn BillingProvider>,
    fake: Option<Arc<BillingFake>>,
    clock: Arc<dyn Clock>,
    app: WeakAppContext,
    return_url: Option<String>,
    invoice_cache: Mutex<InvoiceCache>,
    route_billable: OnceLock<Result<BillableRegistration, String>>,
}

impl std::fmt::Debug for Billing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Billing")
            .field("provider", &self.provider.name())
            .finish_non_exhaustive()
    }
}

/// The parts of a provider subscription Navree derives from the plans.
struct Classified {
    plan: String,
    interval: Interval,
    quantity: u32,
    /// Each item with its meter key (for metered prices).
    items: Vec<(crate::provider::ProviderItem, Option<String>)>,
}

impl Billing {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        db: Arc<Db>,
        options: Arc<BillingOptions>,
        provider: Arc<dyn BillingProvider>,
        fake: Option<Arc<BillingFake>>,
        clock: Arc<dyn Clock>,
        app: WeakAppContext,
        app_url: Option<String>,
    ) -> Billing {
        let return_url = options.return_url.clone().or(app_url);
        Billing {
            plans: Plans::new(&options.plans),
            db,
            options,
            provider,
            fake,
            clock,
            app,
            return_url,
            invoice_cache: Mutex::new(HashMap::new()),
            route_billable: OnceLock::new(),
        }
    }

    // ---------------------------------------------------------- reading

    /// The options the module runs with.
    pub fn options(&self) -> &BillingOptions {
        &self.options
    }

    /// The provider client.
    pub fn provider(&self) -> &dyn BillingProvider {
        &*self.provider
    }

    /// The fake provider, when `billing.provider = "fake"`.
    pub fn fake(&self) -> Option<Arc<BillingFake>> {
        self.fake.clone()
    }

    /// Every configured plan, sorted by key.
    pub fn plans(&self) -> &[Plan] {
        self.plans.all()
    }

    /// The plan with key `key`.
    pub fn plan(&self, key: &str) -> Option<&Plan> {
        self.plans.find(key)
    }

    /// The plan a trial without a subscription grants
    /// (`billing.trial_plan`, default the first plan by key).
    pub fn trial_plan(&self) -> Option<&Plan> {
        match &self.options.trial_plan {
            Some(key) => self.plans.find(key),
            None => self.plans.all().first(),
        }
    }

    /// Where checkout and the portal return by default
    /// (`billing.return_url`, else `app.url`).
    pub fn return_url(&self) -> Option<&str> {
        self.return_url.as_deref()
    }

    pub(crate) fn now(&self) -> DateTime<Utc> {
        self.clock.now()
    }

    fn snapshot(&self, row: &subscription::Model) -> Subscription {
        Subscription::from_row(row, self.now(), self.options.past_due_grace_days)
    }

    /// Every subscription of the billable, newest first.
    pub async fn subscriptions(
        &self,
        billable: &impl AsBillable,
    ) -> Result<Vec<Subscription>, BillingError> {
        let billable = billable.billable_ref();
        let rows = subscription::Entity::find()
            .filter(subscription::Column::BillableType.eq(&billable.billable_type))
            .filter(subscription::Column::BillableId.eq(billable.billable_id))
            .order_by_desc(subscription::Column::CreatedAt)
            .order_by_desc(subscription::Column::Id)
            .all(&self.db.reader().await)
            .await?;
        Ok(rows.iter().map(|row| self.snapshot(row)).collect())
    }

    /// The billable's subscription: its valid one, else its latest one that
    /// has not ended, else its latest one. One indexed query.
    pub async fn subscription(
        &self,
        billable: &impl AsBillable,
    ) -> Result<Option<Subscription>, BillingError> {
        let all = self.subscriptions(billable).await?;
        let pick = all
            .iter()
            .position(Subscription::valid)
            .or_else(|| all.iter().position(|s| !s.ended()))
            .or(if all.is_empty() { None } else { Some(0) });
        Ok(pick.map(|index| all[index].clone()))
    }

    /// Whether the billable has a valid subscription on plan `plan`.
    pub async fn subscribed(
        &self,
        billable: &impl AsBillable,
        plan: &str,
    ) -> Result<bool, BillingError> {
        Ok(self
            .subscription(billable)
            .await?
            .is_some_and(|sub| sub.valid() && sub.on_plan(plan)))
    }

    /// The billable's customer record, if it has one.
    pub async fn customer(
        &self,
        billable: &impl AsBillable,
    ) -> Result<Option<BillingCustomer>, BillingError> {
        let billable = billable.billable_ref();
        Ok(self
            .customer_row(
                &self.db.reader().await,
                &billable.billable_type,
                billable.billable_id,
            )
            .await?)
    }

    async fn customer_row<C: ConnectionTrait>(
        &self,
        conn: &C,
        billable_type: &str,
        billable_id: Uuid,
    ) -> Result<Option<customer::Model>, DbErr> {
        customer::Entity::find()
            .filter(customer::Column::BillableType.eq(billable_type))
            .filter(customer::Column::BillableId.eq(billable_id))
            .one(conn)
            .await
    }

    /// The provider customer id of the billable, at this provider.
    async fn customer_id(&self, billable: &BillableRef) -> Result<Option<String>, BillingError> {
        Ok(self
            .customer(billable)
            .await?
            .filter(|row| row.provider.as_deref() == Some(self.provider.name()))
            .and_then(|row| row.provider_customer_id))
    }

    /// What grants the billable access: its valid subscription, else its
    /// trial without a subscription (as a `Trialing` subscription on
    /// `billing.trial_plan`), else nothing.
    pub async fn access(
        &self,
        billable: &impl AsBillable,
    ) -> Result<Option<Subscription>, BillingError> {
        let billable = billable.billable_ref();
        if let Some(sub) = self.subscription(&billable).await?
            && sub.valid()
        {
            return Ok(Some(sub));
        }
        self.generic_trial(&billable).await
    }

    /// The trial without a subscription, as a subscription, while it lasts.
    async fn generic_trial(
        &self,
        billable: &BillableRef,
    ) -> Result<Option<Subscription>, BillingError> {
        let Some(row) = self.customer(billable).await? else {
            return Ok(None);
        };
        let now = self.now();
        let Some(ends) = row.trial_ends_at.filter(|ends| *ends > now) else {
            return Ok(None);
        };
        let plan = self
            .trial_plan()
            .map(|plan| plan.key.clone())
            .unwrap_or_default();
        Ok(Some(Subscription {
            id: Uuid::nil(),
            billable_type: billable.billable_type.clone(),
            billable_id: billable.billable_id,
            plan,
            interval: Interval::Monthly,
            status: SubscriptionStatus::Trialing,
            quantity: 1,
            trial_ends_at: Some(ends),
            ends_at: None,
            current_period_start: Some(row.created_at),
            current_period_end: Some(ends),
            past_due_since: None,
            provider: self.provider.name().to_owned(),
            provider_id: None,
            provider_customer_id: row.provider_customer_id.clone(),
            created_at: row.created_at,
            evaluated_at: now,
            past_due_grace_days: self.options.past_due_grace_days,
        }))
    }

    // ---------------------------------------------------------- units of work

    /// Runs `f` in the task's unfinished unit of work, or in a new one that
    /// commits when `f` succeeds.
    async fn work<T, F, Fut>(&self, f: F) -> Result<T, BillingError>
    where
        F: FnOnce(Uow) -> Fut,
        Fut: Future<Output = Result<T, BillingError>>,
    {
        if let Some(uow) = Uow::current()
            && !uow.is_finished().await
        {
            return f(uow).await;
        }
        let uow = self.db.uow(None, None).await;
        match uow.scope(f(uow.clone())).await {
            Ok(value) => {
                uow.commit().await?;
                Ok(value)
            }
            Err(error) => {
                if let Err(rollback) = uow.rollback().await {
                    tracing::warn!(target: "navree::billing", error = %rollback, "rollback failed");
                }
                Err(error)
            }
        }
    }

    /// Emits `pending` once `uow` commits, when the app has an `EventBus`.
    fn emit_after_commit(&self, uow: &Uow, pending: Vec<Pending>) {
        if pending.is_empty() {
            return;
        }
        let app = self.app.clone();
        uow.after_commit(move || {
            Box::pin(async move {
                let Some(app) = app.upgrade() else {
                    return;
                };
                let Some(bus) = app.try_get::<EventBus>().await else {
                    return;
                };
                for event in pending {
                    let name = event.name();
                    if let Err(error) = event.emit(&bus).await {
                        tracing::warn!(target: "navree::billing", event = name, %error, "a billing event listener failed");
                    }
                }
            })
        });
    }

    // ---------------------------------------------------------- checkout

    /// A hosted checkout for `billable` on plan `plan`; configure it and
    /// call [`create`](CheckoutBuilder::create).
    pub fn checkout(&self, billable: &impl AsBillable, plan: &str) -> CheckoutBuilder<'_> {
        CheckoutBuilder::new(self, billable.billable_ref(), plan)
    }

    pub(crate) async fn create_checkout(
        &self,
        builder: CheckoutBuilder<'_>,
    ) -> Result<Checkout, BillingError> {
        let parts = builder.into_parts();
        let plan = self
            .plans
            .find(&parts.plan)
            .ok_or_else(|| BillingError::UnknownPlan(parts.plan.clone()))?
            .clone();
        let price = plan
            .price(parts.interval)
            .ok_or_else(|| BillingError::NoPrice {
                plan: plan.key.clone(),
                interval: parts.interval.as_str(),
            })?
            .to_owned();
        if parts.quantity == Some(0) {
            return Err(BillingError::invalid(
                "quantity",
                "quantity must be at least 1",
            ));
        }
        if let Some(sub) = self.subscription(&parts.billable).await?
            && sub.valid()
        {
            return Err(BillingError::AlreadySubscribed);
        }
        let missing = |key: &str| {
            BillingError::Config(format!(
                "no {key}: pass it to the checkout, or set billing.return_url or app.url"
            ))
        };
        let success_url = parts
            .success_url
            .or_else(|| self.return_url.clone())
            .ok_or_else(|| missing("success_url"))?;
        let cancel_url = parts
            .cancel_url
            .or_else(|| self.return_url.clone())
            .ok_or_else(|| missing("cancel_url"))?;
        let email = parts
            .billable
            .email
            .clone()
            .or(parts.customer_email.clone());
        let customer_id = self.ensure_customer(&parts.billable, email.clone()).await?;
        let quantity = match parts.quantity {
            Some(quantity) => quantity,
            None if plan.seats => self.seat_count(&parts.billable).await?,
            None => 1,
        };
        let trial_days = match parts.trial_days {
            Some(0) => None,
            Some(days) => Some(days),
            None => (plan.trial_days > 0).then_some(plan.trial_days),
        };
        let request = CheckoutRequest {
            billable_type: parts.billable.billable_type.clone(),
            billable_id: parts.billable.billable_id,
            customer_id,
            customer_email: email,
            plan: plan.key.clone(),
            interval: parts.interval,
            price_id: price,
            metered_prices: plan.meter_prices(),
            quantity,
            trial_days,
            coupon: parts.coupon,
            allow_promotion_codes: parts.allow_promotion_codes,
            success_url,
            cancel_url,
            metadata: parts.metadata,
        };
        let created = self.provider.create_checkout(&request).await?;
        Ok(Checkout {
            id: created.id,
            url: created.url,
            plan: plan.key,
            interval: parts.interval,
            quantity,
            trial_days,
        })
    }

    /// The billable's provider customer, created at the provider (and linked
    /// in `billing_customers`) when it has none. `None` when the provider
    /// needs more to create one (Paddle without an email).
    async fn ensure_customer(
        &self,
        billable: &BillableRef,
        email: Option<String>,
    ) -> Result<Option<String>, BillingError> {
        if let Some(existing) = self.customer_id(billable).await? {
            return Ok(Some(existing));
        }
        let created = self
            .provider
            .create_customer(&NewCustomer {
                billable_type: billable.billable_type.clone(),
                billable_id: billable.billable_id,
                name: billable.name.clone(),
                email: email.clone(),
            })
            .await?;
        if let Some(id) = &created {
            let (key, id, email) = (
                (billable.billable_type.clone(), billable.billable_id),
                id.clone(),
                email,
            );
            self.work(|uow| async move {
                let tx = uow.tx().await?;
                self.link_customer(&*tx, &key, &id, email).await?;
                Ok(())
            })
            .await?;
        }
        Ok(created)
    }

    /// Links provider customer `customer_id` to the billable.
    async fn link_customer<C: ConnectionTrait>(
        &self,
        conn: &C,
        billable: &(String, Uuid),
        customer_id: &str,
        email: Option<String>,
    ) -> Result<(), DbErr> {
        let now = self.now();
        let provider = self.provider.name().to_owned();
        match self.customer_row(conn, &billable.0, billable.1).await? {
            Some(row) => {
                if row.provider_customer_id.as_deref() == Some(customer_id)
                    && row.provider.as_deref() == Some(provider.as_str())
                {
                    return Ok(());
                }
                let mut active: customer::ActiveModel = row.clone().into();
                active.provider = Set(Some(provider));
                active.provider_customer_id = Set(Some(customer_id.to_owned()));
                if row.email.is_none() {
                    active.email = Set(email);
                }
                active.updated_at = Set(now);
                active.update(conn).await?;
            }
            None => {
                customer::ActiveModel {
                    id: Set(Uuid::now_v7()),
                    billable_type: Set(billable.0.clone()),
                    billable_id: Set(billable.1),
                    provider: Set(Some(provider)),
                    provider_customer_id: Set(Some(customer_id.to_owned())),
                    email: Set(email),
                    trial_ends_at: Set(None),
                    created_at: Set(now),
                    updated_at: Set(now),
                }
                .insert(conn)
                .await?;
            }
        }
        Ok(())
    }

    /// The seats of a billable: a workspace's member count (at least 1),
    /// 1 for other billables.
    pub(crate) async fn seat_count(&self, billable: &BillableRef) -> Result<u32, BillingError> {
        if billable.billable_type != WORKSPACE_BILLABLE {
            return Ok(1);
        }
        let members = member::Entity::find()
            .filter(member::Column::WorkspaceId.eq(billable.billable_id))
            .count(&self.db.reader().await)
            .await?;
        Ok(u32::try_from(members).unwrap_or(u32::MAX).max(1))
    }

    // ---------------------------------------------------------- changes

    /// The billable's subscription to change: its valid one, else its
    /// latest one that has not ended.
    async fn changeable(
        &self,
        billable: &BillableRef,
    ) -> Result<(Subscription, Vec<subscription_item::Model>), BillingError> {
        let sub = self
            .subscription(billable)
            .await?
            .filter(|sub| !sub.ended())
            .ok_or(BillingError::NoSubscription)?;
        let items = subscription_item::Entity::find()
            .filter(subscription_item::Column::SubscriptionId.eq(sub.id))
            .order_by_asc(subscription_item::Column::CreatedAt)
            .order_by_asc(subscription_item::Column::Id)
            .all(&self.db.reader().await)
            .await?;
        Ok((sub, items))
    }

    fn provider_id(sub: &Subscription) -> Result<&str, BillingError> {
        sub.provider_id
            .as_deref()
            .ok_or(BillingError::NoSubscription)
    }

    /// Swaps to plan `plan` at the current interval, prorated and effective
    /// now.
    pub async fn swap(
        &self,
        billable: &impl AsBillable,
        plan: &str,
    ) -> Result<Subscription, BillingError> {
        self.swap_to(billable, plan, None, Proration::NextInvoice)
            .await
    }

    /// Swaps to plan `plan`'s yearly price.
    pub async fn swap_yearly(
        &self,
        billable: &impl AsBillable,
        plan: &str,
    ) -> Result<Subscription, BillingError> {
        self.swap_to(
            billable,
            plan,
            Some(Interval::Yearly),
            Proration::NextInvoice,
        )
        .await
    }

    /// Swaps to plan `plan`'s monthly price.
    pub async fn swap_monthly(
        &self,
        billable: &impl AsBillable,
        plan: &str,
    ) -> Result<Subscription, BillingError> {
        self.swap_to(
            billable,
            plan,
            Some(Interval::Monthly),
            Proration::NextInvoice,
        )
        .await
    }

    /// Swaps to plan `plan` at the current interval, charged as `proration`
    /// says.
    pub async fn swap_with(
        &self,
        billable: &impl AsBillable,
        plan: &str,
        proration: Proration,
    ) -> Result<Subscription, BillingError> {
        self.swap_to(billable, plan, None, proration).await
    }

    /// Swaps to plan `plan` at `interval` (the current one when `None`).
    /// Metered prices follow the new plan's meters; for a plan with
    /// `seats = true` the quantity becomes the member count.
    pub async fn swap_to(
        &self,
        billable: &impl AsBillable,
        plan: &str,
        interval: Option<Interval>,
        proration: Proration,
    ) -> Result<Subscription, BillingError> {
        let billable = billable.billable_ref();
        let plan = self
            .plans
            .find(plan)
            .ok_or_else(|| BillingError::UnknownPlan(plan.to_owned()))?
            .clone();
        let (sub, items) = self.changeable(&billable).await?;
        let interval = interval.unwrap_or(sub.interval);
        let price = plan
            .price(interval)
            .ok_or_else(|| BillingError::NoPrice {
                plan: plan.key.clone(),
                interval: interval.as_str(),
            })?
            .to_owned();
        let old_seats = self.plans.find(&sub.plan).is_some_and(|old| old.seats);
        let quantity = if plan.seats {
            self.seat_count(&billable).await?
        } else if old_seats {
            1
        } else {
            sub.quantity.max(1)
        };
        let base = self.base_item(&items);
        let new_meters = plan.meter_prices();
        let current_meters: Vec<&str> = items
            .iter()
            .filter(|item| item.meter.is_some())
            .map(|item| item.price_id.as_str())
            .collect();
        let change = SubscriptionChange {
            base_item_id: base.and_then(|item| item.provider_item_id.clone()),
            price_id: price,
            quantity,
            add_prices: new_meters
                .iter()
                .filter(|price| !current_meters.contains(&price.as_str()))
                .cloned()
                .collect(),
            remove_item_ids: items
                .iter()
                .filter(|item| item.meter.is_some() && !new_meters.contains(&item.price_id))
                .filter_map(|item| item.provider_item_id.clone())
                .collect(),
            proration,
        };
        let updated = self
            .provider
            .update_subscription(Self::provider_id(&sub)?, &change)
            .await?;
        self.store_snapshot(&updated).await
    }

    /// The base (licensed) item among `items`.
    fn base_item<'a>(
        &self,
        items: &'a [subscription_item::Model],
    ) -> Option<&'a subscription_item::Model> {
        items
            .iter()
            .find(|item| {
                matches!(
                    self.plans.role_of(&item.price_id),
                    Some(PriceRole::Base { .. })
                )
            })
            .or_else(|| items.iter().find(|item| item.meter.is_none()))
    }

    /// Changes the seat count (prorated on the next invoice).
    pub async fn update_quantity(
        &self,
        billable: &impl AsBillable,
        quantity: u32,
    ) -> Result<Subscription, BillingError> {
        if quantity == 0 {
            return Err(BillingError::invalid(
                "quantity",
                "quantity must be at least 1",
            ));
        }
        let billable = billable.billable_ref();
        let (sub, items) = self.changeable(&billable).await?;
        let base = self.base_item(&items);
        let price = base
            .map(|item| item.price_id.clone())
            .or_else(|| {
                self.plans
                    .find(&sub.plan)
                    .and_then(|plan| plan.price(sub.interval).map(str::to_owned))
            })
            .ok_or(BillingError::NoSubscription)?;
        let change = SubscriptionChange {
            base_item_id: base.and_then(|item| item.provider_item_id.clone()),
            price_id: price,
            quantity,
            add_prices: Vec::new(),
            remove_item_ids: Vec::new(),
            proration: Proration::NextInvoice,
        };
        let updated = self
            .provider
            .update_subscription(Self::provider_id(&sub)?, &change)
            .await?;
        self.store_snapshot(&updated).await
    }

    /// Cancels at the end of the period: [`valid`](Subscription::valid)
    /// stays true until `ends_at`, and [`resume`](Self::resume) undoes it.
    pub async fn cancel(&self, billable: &impl AsBillable) -> Result<Subscription, BillingError> {
        let (sub, _) = self.changeable(&billable.billable_ref()).await?;
        let updated = self
            .provider
            .cancel_subscription(Self::provider_id(&sub)?, true)
            .await?;
        self.store_snapshot(&updated).await
    }

    /// Cancels at once, without a refund.
    pub async fn cancel_now(
        &self,
        billable: &impl AsBillable,
    ) -> Result<Subscription, BillingError> {
        let (sub, _) = self.changeable(&billable.billable_ref()).await?;
        let updated = self
            .provider
            .cancel_subscription(Self::provider_id(&sub)?, false)
            .await?;
        self.store_snapshot(&updated).await
    }

    /// Undoes a [`cancel`](Self::cancel) before the period ends.
    pub async fn resume(&self, billable: &impl AsBillable) -> Result<Subscription, BillingError> {
        let (sub, _) = self.changeable(&billable.billable_ref()).await?;
        if !sub.on_grace_period() {
            return Err(BillingError::invalid(
                "subscription",
                "only a subscription canceled at the end of its period can be resumed",
            ));
        }
        let updated = self
            .provider
            .resume_subscription(Self::provider_id(&sub)?)
            .await?;
        self.store_snapshot(&updated).await
    }

    /// Stops charging, keeping the subscription.
    pub async fn pause(&self, billable: &impl AsBillable) -> Result<Subscription, BillingError> {
        let (sub, _) = self.changeable(&billable.billable_ref()).await?;
        let updated = self
            .provider
            .pause_subscription(Self::provider_id(&sub)?)
            .await?;
        self.store_snapshot(&updated).await
    }

    /// Starts charging a paused subscription again.
    pub async fn unpause(&self, billable: &impl AsBillable) -> Result<Subscription, BillingError> {
        let (sub, _) = self.changeable(&billable.billable_ref()).await?;
        let updated = self
            .provider
            .unpause_subscription(Self::provider_id(&sub)?)
            .await?;
        self.store_snapshot(&updated).await
    }

    /// Applies a provider subscription to the local copy (in the task's
    /// unit of work or a new one) and returns it.
    async fn store_snapshot(
        &self,
        snapshot: &ProviderSubscription,
    ) -> Result<Subscription, BillingError> {
        let at = self.now();
        let row = self
            .work(|uow| async move {
                let mut pending = Vec::new();
                let row = self
                    .apply_snapshot(&uow, snapshot, at, &mut pending)
                    .await?;
                self.emit_after_commit(&uow, pending);
                Ok(row)
            })
            .await?;
        row.map(|row| self.snapshot(&row))
            .ok_or(BillingError::NoSubscription)
    }

    // ---------------------------------------------------------- trials

    /// Starts a trial without a subscription: `days` from now, stored in
    /// `billing_customers.trial_ends_at`, never sent to the provider.
    /// Returns when it ends.
    pub async fn start_trial(
        &self,
        billable: &impl AsBillable,
        days: u32,
    ) -> Result<DateTime<Utc>, BillingError> {
        if days == 0 {
            return Err(BillingError::invalid("days", "days must be at least 1"));
        }
        let billable = billable.billable_ref();
        let now = self.now();
        let ends = now + TimeDelta::days(i64::from(days));
        self.work(|uow| async move {
            let tx = uow.tx().await?;
            match self
                .customer_row(&*tx, &billable.billable_type, billable.billable_id)
                .await?
            {
                Some(row) => {
                    let mut active: customer::ActiveModel = row.into();
                    active.trial_ends_at = Set(Some(ends));
                    active.updated_at = Set(now);
                    active.update(&*tx).await?;
                }
                None => {
                    customer::ActiveModel {
                        id: Set(Uuid::now_v7()),
                        billable_type: Set(billable.billable_type.clone()),
                        billable_id: Set(billable.billable_id),
                        provider: Set(None),
                        provider_customer_id: Set(None),
                        email: Set(billable.email.clone()),
                        trial_ends_at: Set(Some(ends)),
                        created_at: Set(now),
                        updated_at: Set(now),
                    }
                    .insert(&*tx)
                    .await?;
                }
            }
            Ok(())
        })
        .await?;
        Ok(ends)
    }

    /// Whether the billable is on a trial: a trial without a subscription,
    /// or a subscription's trial.
    pub async fn on_trial(&self, billable: &impl AsBillable) -> Result<bool, BillingError> {
        let billable = billable.billable_ref();
        if self.generic_trial(&billable).await?.is_some() {
            return Ok(true);
        }
        Ok(self
            .subscription(&billable)
            .await?
            .is_some_and(|sub| sub.on_trial()))
    }

    /// When the billable's trial ends: its subscription's trial while it is
    /// on one, else the trial without a subscription (also once it has
    /// passed).
    pub async fn trial_ends_at(
        &self,
        billable: &impl AsBillable,
    ) -> Result<Option<DateTime<Utc>>, BillingError> {
        let billable = billable.billable_ref();
        if let Some(sub) = self.subscription(&billable).await?
            && sub.on_trial()
        {
            return Ok(sub.trial_ends_at);
        }
        Ok(self
            .customer(&billable)
            .await?
            .and_then(|row| row.trial_ends_at))
    }

    // ---------------------------------------------------------- usage

    /// Records `quantity` units of meter `meter` for the billable: one row
    /// in `billing_usage`, in the task's unit of work (so a failing request
    /// records nothing), with no network round trip.
    /// `billing:report-usage` sends it to the provider.
    pub async fn report_usage(
        &self,
        billable: &impl AsBillable,
        meter: &str,
        quantity: u64,
    ) -> Result<(), BillingError> {
        if quantity == 0 {
            return Err(BillingError::invalid(
                "quantity",
                "quantity must be at least 1",
            ));
        }
        if !self.plans.has_meter(meter) {
            return Err(BillingError::UnknownMeter(meter.to_owned()));
        }
        let billable = billable.billable_ref();
        let sub = self
            .subscription(&billable)
            .await?
            .filter(Subscription::valid)
            .ok_or(BillingError::NoSubscription)?;
        if self
            .plans
            .find(&sub.plan)
            .and_then(|plan| plan.meter_price(meter))
            .is_none()
        {
            return Err(BillingError::UnknownMeter(meter.to_owned()));
        }
        let quantity = i64::try_from(quantity)
            .map_err(|_| BillingError::invalid("quantity", "quantity is too large"))?;
        let now = self.now();
        let meter = meter.to_owned();
        self.work(|uow| async move {
            let tx = uow.tx().await?;
            usage::ActiveModel {
                id: Set(Uuid::now_v7()),
                billable_type: Set(billable.billable_type.clone()),
                billable_id: Set(billable.billable_id),
                meter: Set(meter),
                quantity: Set(quantity),
                recorded_at: Set(now),
                reported_at: Set(None),
            }
            .insert(&*tx)
            .await?;
            Ok(())
        })
        .await
    }

    /// This period's usage of `meter` (reported or not): rows since the
    /// valid subscription's current period started (every row without one).
    pub async fn usage(
        &self,
        billable: &impl AsBillable,
        meter: &str,
    ) -> Result<u64, BillingError> {
        if !self.plans.has_meter(meter) {
            return Err(BillingError::UnknownMeter(meter.to_owned()));
        }
        let billable = billable.billable_ref();
        let since = self
            .subscription(&billable)
            .await?
            .filter(Subscription::valid)
            .and_then(|sub| sub.current_period_start);
        let mut select = usage::Entity::find()
            .filter(usage::Column::BillableType.eq(&billable.billable_type))
            .filter(usage::Column::BillableId.eq(billable.billable_id))
            .filter(usage::Column::Meter.eq(meter));
        if let Some(since) = since {
            select = select.filter(usage::Column::RecordedAt.gte(since));
        }
        let rows = select.all(&self.db.reader().await).await?;
        Ok(rows
            .iter()
            .map(|row| u64::try_from(row.quantity).unwrap_or_default())
            .sum())
    }

    /// Sends every unreported usage row to the provider, summed per
    /// billable and meter, and marks them reported (what
    /// `billing:report-usage` runs).
    pub async fn report_pending_usage(&self) -> Result<UsageReportSummary, BillingError> {
        let rows = usage::Entity::find()
            .filter(usage::Column::ReportedAt.is_null())
            .order_by_asc(usage::Column::RecordedAt)
            .order_by_asc(usage::Column::Id)
            .all(self.db.conn())
            .await?;
        let mut groups: BTreeMap<(String, Uuid, String), Vec<usage::Model>> = BTreeMap::new();
        for row in rows {
            groups
                .entry((
                    row.billable_type.clone(),
                    row.billable_id,
                    row.meter.clone(),
                ))
                .or_default()
                .push(row);
        }
        let mut summary = UsageReportSummary::default();
        for ((billable_type, billable_id, meter), rows) in groups {
            let billable = BillableRef::new(billable_type, billable_id);
            let sub = self
                .subscription(&billable)
                .await?
                .filter(Subscription::valid);
            let target = sub.as_ref().and_then(|sub| {
                let price = self.plans.find(&sub.plan)?.meter_price(&meter)?.to_owned();
                Some((
                    sub.provider_id.clone()?,
                    sub.provider_customer_id.clone()?,
                    price,
                ))
            });
            let Some((subscription_id, customer_id, price_id)) = target else {
                summary.skipped += rows.len();
                continue;
            };
            let quantity: u64 = rows
                .iter()
                .map(|row| u64::try_from(row.quantity).unwrap_or_default())
                .sum();
            let ids: Vec<Uuid> = rows.iter().map(|row| row.id).collect();
            let mut digest = Sha256::new();
            for id in &ids {
                digest.update(id.as_bytes());
            }
            let report = UsageReport {
                customer_id,
                subscription_id,
                meter: meter.clone(),
                price_id,
                quantity,
                idempotency_key: format!("navree-usage-{}", &hex::encode(digest.finalize())[..32]),
                timestamp: rows
                    .last()
                    .map_or_else(|| self.now(), |row| row.recorded_at),
            };
            match self.provider.report_usage(&report).await {
                Ok(()) => {
                    usage::Entity::update_many()
                        .col_expr(usage::Column::ReportedAt, Expr::value(self.now()))
                        .filter(usage::Column::Id.is_in(ids))
                        .exec(self.db.conn())
                        .await?;
                    summary.reported += 1;
                    summary.units += quantity;
                }
                Err(error) => {
                    tracing::warn!(target: "navree::billing", meter = %meter, %error, "reporting usage failed; it stays queued");
                    summary.failed += 1;
                }
            }
        }
        Ok(summary)
    }

    // ---------------------------------------------------------- invoices, portal

    /// The billable's invoices from the provider, newest first, cached for
    /// `billing.invoice_cache_secs`. Empty when it has no customer yet.
    pub async fn invoices(&self, billable: &impl AsBillable) -> Result<Vec<Invoice>, BillingError> {
        let billable = billable.billable_ref();
        let key = (billable.billable_type.clone(), billable.billable_id);
        let now = self.now();
        if let Some((expires, invoices)) = self.cache().get(&key)
            && *expires > now
        {
            return Ok(invoices.clone());
        }
        let Some(customer) = self.customer_id(&billable).await? else {
            return Ok(Vec::new());
        };
        let invoices = self.provider.invoices(&customer).await?;
        if self.options.invoice_cache_secs > 0 {
            let secs = i64::try_from(self.options.invoice_cache_secs).unwrap_or(i64::MAX);
            self.cache()
                .insert(key, (now + TimeDelta::seconds(secs), invoices.clone()));
        }
        Ok(invoices)
    }

    fn cache(&self) -> std::sync::MutexGuard<'_, InvoiceCache> {
        self.invoice_cache
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn forget_invoices(&self, billable_type: &str, billable_id: Uuid) {
        self.cache()
            .remove(&(billable_type.to_owned(), billable_id));
    }

    /// A customer portal session URL, returning to `return_url`.
    /// [`BillingError::NoCustomer`] before the first checkout.
    pub async fn portal_url(
        &self,
        billable: &impl AsBillable,
        return_url: &str,
    ) -> Result<String, BillingError> {
        let billable = billable.billable_ref();
        let customer = self
            .customer_id(&billable)
            .await?
            .ok_or(BillingError::NoCustomer)?;
        let subscriptions: Vec<String> = self
            .subscriptions(&billable)
            .await?
            .into_iter()
            .filter(|sub| !sub.ended())
            .filter_map(|sub| sub.provider_id)
            .collect();
        Ok(self
            .provider
            .portal_url(&customer, return_url, &subscriptions)
            .await?)
    }

    /// Pulls the billable's subscriptions from the provider into the local
    /// copy (what `billing:sync` runs) and forgets its cached invoices.
    pub async fn sync(
        &self,
        billable: &impl AsBillable,
    ) -> Result<Vec<Subscription>, BillingError> {
        let billable = billable.billable_ref();
        let customer = self
            .customer_id(&billable)
            .await?
            .ok_or(BillingError::NoCustomer)?;
        let remote = self.provider.subscriptions(&customer).await?;
        let mut synced = Vec::with_capacity(remote.len());
        for snapshot in &remote {
            synced.push(self.store_snapshot(snapshot).await?);
        }
        self.forget_invoices(&billable.billable_type, billable.billable_id);
        Ok(synced)
    }

    // ---------------------------------------------------------- seats

    /// Brings a workspace's seat count in line with its member count, for a
    /// valid subscription on a plan with `seats = true`. Returns the
    /// updated subscription, or `None` when nothing changed.
    pub async fn sync_seats(
        &self,
        workspace_id: Uuid,
    ) -> Result<Option<Subscription>, BillingError> {
        let billable = BillableRef::new(WORKSPACE_BILLABLE, workspace_id);
        let Some(sub) = self
            .subscription(&billable)
            .await?
            .filter(|sub| sub.valid() && !sub.is_trial_without_subscription())
        else {
            return Ok(None);
        };
        if !self.plans.find(&sub.plan).is_some_and(|plan| plan.seats) {
            return Ok(None);
        }
        let seats = self.seat_count(&billable).await?;
        if seats == sub.quantity {
            return Ok(None);
        }
        Ok(Some(self.update_quantity(&billable, seats).await?))
    }

    // ---------------------------------------------------------- webhooks

    /// Handles a webhook: verifies its signature (400 when it fails),
    /// stores it in `billing_events` once (a redelivery is answered as a
    /// duplicate) and dispatches the `billing.process_event` job, or runs
    /// it at once when the app has no job dispatcher.
    pub async fn receive_webhook(
        &self,
        headers: &HeaderMap,
        body: &[u8],
    ) -> Result<WebhookReceipt, BillingError> {
        let tolerance = Duration::from_secs(self.options.webhook_tolerance_secs);
        self.provider
            .verify_webhook(headers, body, self.now(), tolerance)?;
        let event = self
            .provider
            .parse_webhook(body)
            .map_err(|error| BillingError::InvalidPayload(error.to_string()))?;
        let payload = std::str::from_utf8(body)
            .map_err(|_| BillingError::InvalidPayload("the body is not UTF-8".into()))?
            .to_owned();
        let provider = self.provider.name();
        let dispatcher = match self.app.upgrade() {
            Some(app) => app.try_get::<dyn Dispatcher>().await,
            None => None,
        };
        let id = Uuid::now_v7();
        let now = self.now();
        let uow = self.db.uow(None, None).await;
        let stored: Result<Option<Uuid>, BillingError> = uow
            .scope(async {
                let tx = uow.tx().await?;
                if let Some(existing) = event::Entity::find()
                    .filter(event::Column::Provider.eq(provider))
                    .filter(event::Column::ProviderEventId.eq(&event.id))
                    .one(&*tx)
                    .await?
                {
                    return Ok(Some(existing.id));
                }
                event::ActiveModel {
                    id: Set(id),
                    provider: Set(provider.to_owned()),
                    provider_event_id: Set(event.id.clone()),
                    event_type: Set(event.event_type.clone()),
                    payload: Set(payload),
                    received_at: Set(now),
                    processed_at: Set(None),
                    attempts: Set(0),
                    error: Set(None),
                }
                .insert(&*tx)
                .await?;
                drop(tx);
                if let Some(dispatcher) = &dispatcher {
                    let mut envelope = JobEnvelope::of(&ProcessBillingEvent { event_id: id })
                        .map_err(BillingError::Other)?;
                    envelope.options.queue = self.options.queue.clone();
                    dispatcher
                        .dispatch_envelope(envelope)
                        .await
                        .map_err(BillingError::Other)?;
                }
                Ok(None)
            })
            .await;
        let duplicate_of = match stored {
            Ok(duplicate) => {
                uow.commit().await?;
                duplicate
            }
            Err(error) => {
                let _ = uow.rollback().await;
                match &error {
                    BillingError::Db(db)
                        if matches!(db.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) =>
                    {
                        Some(Uuid::nil())
                    }
                    _ => return Err(error),
                }
            }
        };
        if let Some(existing) = duplicate_of {
            return Ok(WebhookReceipt {
                event_id: existing,
                provider_event_id: event.id,
                event_type: event.event_type,
                duplicate: true,
            });
        }
        if dispatcher.is_none()
            && let Err(error) = self.process_event(id).await
        {
            tracing::warn!(target: "navree::billing", event = %event.id, %error, "processing a billing webhook failed; see billing:events --failed");
        }
        Ok(WebhookReceipt {
            event_id: id,
            provider_event_id: event.id,
            event_type: event.event_type,
            duplicate: false,
        })
    }

    /// Processes stored event `id` (the `billing.process_event` job):
    /// updates the local customer and subscription rows and emits the
    /// billing events after the commit. Idempotent: a processed event is a
    /// no-op. A failure is recorded on the row (`attempts`, `error`) and
    /// returned, so the queue retries it.
    pub async fn process_event(&self, id: Uuid) -> Result<(), BillingError> {
        let Some(row) = event::Entity::find_by_id(id).one(self.db.conn()).await? else {
            return Ok(());
        };
        if row.processed_at.is_some() {
            return Ok(());
        }
        match self.process_row(&row).await {
            Ok(()) => Ok(()),
            Err(error) => {
                event::Entity::update_many()
                    .col_expr(event::Column::Attempts, Expr::value(row.attempts + 1))
                    .col_expr(event::Column::Error, Expr::value(Some(error.to_string())))
                    .filter(event::Column::Id.eq(row.id))
                    .exec(self.db.conn())
                    .await?;
                Err(error)
            }
        }
    }

    async fn process_row(&self, row: &event::Model) -> Result<(), BillingError> {
        let event = self.provider.parse_webhook(row.payload.as_bytes())?;
        let uow = self.db.uow(None, None).await;
        let result: Result<(), BillingError> = uow
            .scope(async {
                let pending = self.apply_event(&uow, &event).await?;
                let tx = uow.tx().await?;
                event::Entity::update_many()
                    .col_expr(event::Column::ProcessedAt, Expr::value(Some(self.now())))
                    .col_expr(event::Column::Attempts, Expr::value(row.attempts + 1))
                    .col_expr(event::Column::Error, Expr::value(None::<String>))
                    .filter(event::Column::Id.eq(row.id))
                    .exec(&*tx)
                    .await?;
                drop(tx);
                self.emit_after_commit(&uow, pending);
                Ok(())
            })
            .await;
        match result {
            Ok(()) => {
                uow.commit().await?;
                Ok(())
            }
            Err(error) => {
                let _ = uow.rollback().await;
                Err(error)
            }
        }
    }

    /// Applies a webhook event's actions in `uow`; returns the events to
    /// emit.
    async fn apply_event(
        &self,
        uow: &Uow,
        event: &WebhookEvent,
    ) -> Result<Vec<Pending>, BillingError> {
        let mut pending = Vec::new();
        for action in &event.actions {
            match action {
                WebhookAction::CustomerLinked {
                    customer_id,
                    billable,
                    email,
                } => {
                    if let Some(billable) = billable {
                        let tx = uow.tx().await?;
                        self.link_customer(&*tx, billable, customer_id, email.clone())
                            .await?;
                    }
                }
                WebhookAction::Subscription(snapshot) => {
                    self.apply_snapshot(uow, snapshot, event.occurred_at, &mut pending)
                        .await?;
                }
                WebhookAction::TrialEnding(snapshot) => {
                    if let Some(row) = self
                        .apply_snapshot(uow, snapshot, event.occurred_at, &mut pending)
                        .await?
                    {
                        pending.push(Pending::TrialEnding(TrialEnding {
                            billable_type: row.billable_type.clone(),
                            billable_id: row.billable_id,
                            subscription_id: row.id,
                            plan: row.plan.clone(),
                            trial_ends_at: row.trial_ends_at,
                        }));
                    }
                }
                WebhookAction::InvoicePaid(invoice) => {
                    if let Some(event) = self.invoice_event(uow, invoice, true).await? {
                        pending.push(event);
                    }
                }
                WebhookAction::PaymentFailed(invoice) => {
                    if let Some(event) = self.invoice_event(uow, invoice, false).await? {
                        pending.push(event);
                    }
                }
            }
        }
        Ok(pending)
    }

    /// The `InvoicePaid` or `PaymentFailed` event of an invoice, when its
    /// billable is known; forgets the billable's cached invoices.
    async fn invoice_event(
        &self,
        uow: &Uow,
        invoice: &ProviderInvoice,
        paid: bool,
    ) -> Result<Option<Pending>, BillingError> {
        let tx = uow.tx().await?;
        let sub = match &invoice.subscription_id {
            Some(id) => {
                subscription::Entity::find()
                    .filter(subscription::Column::Provider.eq(self.provider.name()))
                    .filter(subscription::Column::ProviderId.eq(id))
                    .one(&*tx)
                    .await?
            }
            None => None,
        };
        let billable = match (&sub, &invoice.customer_id) {
            (Some(sub), _) => Some((sub.billable_type.clone(), sub.billable_id)),
            (None, Some(customer)) => self.billable_of_customer(&*tx, customer).await?,
            (None, None) => None,
        };
        let Some((billable_type, billable_id)) = billable else {
            tracing::warn!(target: "navree::billing", invoice = %invoice.id, "an invoice for an unknown customer; no event emitted");
            return Ok(None);
        };
        self.forget_invoices(&billable_type, billable_id);
        let (subscription_id, plan) = match sub {
            Some(sub) => (Some(sub.id), Some(sub.plan)),
            None => (None, None),
        };
        Ok(Some(if paid {
            Pending::InvoicePaid(InvoicePaid {
                billable_type,
                billable_id,
                subscription_id,
                plan,
                invoice_id: invoice.id.clone(),
                amount_cents: invoice.amount_cents,
                currency: invoice.currency.clone(),
            })
        } else {
            Pending::PaymentFailed(PaymentFailed {
                billable_type,
                billable_id,
                subscription_id,
                plan,
                invoice_id: invoice.id.clone(),
                amount_cents: invoice.amount_cents,
                currency: invoice.currency.clone(),
            })
        }))
    }

    /// The billable linked to provider customer `customer_id`.
    async fn billable_of_customer<C: ConnectionTrait>(
        &self,
        conn: &C,
        customer_id: &str,
    ) -> Result<Option<(String, Uuid)>, DbErr> {
        Ok(customer::Entity::find()
            .filter(customer::Column::Provider.eq(self.provider.name()))
            .filter(customer::Column::ProviderCustomerId.eq(customer_id))
            .one(conn)
            .await?
            .map(|row| (row.billable_type, row.billable_id)))
    }

    /// Plan, interval, quantity and meters of a provider subscription.
    fn classify(&self, snapshot: &ProviderSubscription) -> Classified {
        let mut base: Option<(String, Interval, u32)> = None;
        let mut items = Vec::with_capacity(snapshot.items.len());
        for item in &snapshot.items {
            match self.plans.role_of(&item.price_id) {
                Some(PriceRole::Base { plan, interval }) => {
                    if base.is_none() {
                        let quantity = item
                            .quantity
                            .map_or(1, |q| u32::try_from(q).unwrap_or(u32::MAX));
                        base = Some((plan, interval, quantity));
                    }
                    items.push((item.clone(), None));
                }
                Some(PriceRole::Meter { meter }) => items.push((item.clone(), Some(meter))),
                None => items.push((item.clone(), None)),
            }
        }
        let (plan, interval, quantity) = base.unwrap_or_else(|| {
            let first = snapshot.items.first();
            let plan = snapshot
                .metadata
                .get("plan")
                .cloned()
                .or_else(|| first.map(|item| item.price_id.clone()))
                .unwrap_or_else(|| "unknown".to_owned());
            tracing::warn!(target: "navree::billing", subscription = %snapshot.id, %plan, "a subscription whose price is not under billing.plans");
            let quantity = first
                .and_then(|item| item.quantity)
                .map_or(1, |q| u32::try_from(q).unwrap_or(u32::MAX));
            (plan, Interval::Monthly, quantity)
        });
        Classified {
            plan,
            interval,
            quantity,
            items,
        }
    }

    /// Applies a provider subscription seen at `at` to the local copy.
    /// Older states than the stored one are ignored; the events of the
    /// change are pushed to `pending`. `None` when the subscription's
    /// billable is unknown.
    async fn apply_snapshot(
        &self,
        uow: &Uow,
        snapshot: &ProviderSubscription,
        at: DateTime<Utc>,
        pending: &mut Vec<Pending>,
    ) -> Result<Option<subscription::Model>, BillingError> {
        let tx = uow.tx().await?;
        let provider = self.provider.name();
        let existing = subscription::Entity::find()
            .filter(subscription::Column::Provider.eq(provider))
            .filter(subscription::Column::ProviderId.eq(&snapshot.id))
            .one(&*tx)
            .await?;
        if let Some(row) = &existing
            && row.provider_updated_at.is_some_and(|stored| at < stored)
        {
            return Ok(existing);
        }
        let billable = match &existing {
            Some(row) => Some((row.billable_type.clone(), row.billable_id)),
            None => match self
                .billable_of_customer(&*tx, &snapshot.customer_id)
                .await?
            {
                Some(billable) => Some(billable),
                None => snapshot.billable(),
            },
        };
        let Some(billable) = billable else {
            tracing::warn!(target: "navree::billing", subscription = %snapshot.id, "a subscription of an unknown customer, without billable metadata; ignored");
            return Ok(None);
        };
        self.link_customer(&*tx, &billable, &snapshot.customer_id, None)
            .await?;
        let classified = self.classify(snapshot);
        let now = self.now();
        let status = snapshot.status;
        let quantity = i64::from(classified.quantity);
        let row = match existing {
            None => {
                let row = subscription::ActiveModel {
                    id: Set(Uuid::now_v7()),
                    billable_type: Set(billable.0.clone()),
                    billable_id: Set(billable.1),
                    provider: Set(provider.to_owned()),
                    provider_id: Set(snapshot.id.clone()),
                    provider_customer_id: Set(snapshot.customer_id.clone()),
                    plan: Set(classified.plan.clone()),
                    interval: Set(classified.interval.as_str().to_owned()),
                    status: Set(status.as_str().to_owned()),
                    quantity: Set(quantity),
                    trial_ends_at: Set(snapshot.trial_ends_at),
                    ends_at: Set(snapshot.ends_at),
                    current_period_start: Set(snapshot.current_period_start),
                    current_period_end: Set(snapshot.current_period_end),
                    past_due_since: Set((status == SubscriptionStatus::PastDue).then_some(now)),
                    provider_updated_at: Set(Some(at)),
                    created_at: Set(now),
                    updated_at: Set(now),
                }
                .insert(&*tx)
                .await?;
                if status != SubscriptionStatus::Canceled {
                    pending.push(Pending::Created(SubscriptionCreated {
                        billable_type: row.billable_type.clone(),
                        billable_id: row.billable_id,
                        subscription_id: row.id,
                        plan: row.plan.clone(),
                        interval: classified.interval,
                        status,
                        quantity: classified.quantity,
                    }));
                }
                row
            }
            Some(old) => {
                let past_due_since = match (status, old.past_due_since) {
                    (SubscriptionStatus::PastDue, Some(since)) => Some(since),
                    (SubscriptionStatus::PastDue, None) => Some(now),
                    _ => None,
                };
                let changed = old.plan != classified.plan
                    || old.interval != classified.interval.as_str()
                    || old.status != status.as_str()
                    || old.quantity != quantity
                    || old.trial_ends_at != snapshot.trial_ends_at
                    || old.ends_at != snapshot.ends_at
                    || old.current_period_end != snapshot.current_period_end;
                let newly_canceled = (old.ends_at.is_none() && snapshot.ends_at.is_some())
                    || (old.status != SubscriptionStatus::Canceled.as_str()
                        && status == SubscriptionStatus::Canceled);
                let mut active: subscription::ActiveModel = old.clone().into();
                active.provider_customer_id = Set(snapshot.customer_id.clone());
                active.plan = Set(classified.plan.clone());
                active.interval = Set(classified.interval.as_str().to_owned());
                active.status = Set(status.as_str().to_owned());
                active.quantity = Set(quantity);
                active.trial_ends_at = Set(snapshot.trial_ends_at);
                active.ends_at = Set(snapshot.ends_at);
                active.current_period_start = Set(snapshot.current_period_start);
                active.current_period_end = Set(snapshot.current_period_end);
                active.past_due_since = Set(past_due_since);
                active.provider_updated_at = Set(Some(at));
                active.updated_at = Set(now);
                let row = active.update(&*tx).await?;
                if newly_canceled {
                    pending.push(Pending::Canceled(SubscriptionCanceled {
                        billable_type: row.billable_type.clone(),
                        billable_id: row.billable_id,
                        subscription_id: row.id,
                        plan: row.plan.clone(),
                        ends_at: row.ends_at,
                    }));
                } else if changed {
                    pending.push(Pending::Updated(SubscriptionUpdated {
                        billable_type: row.billable_type.clone(),
                        billable_id: row.billable_id,
                        subscription_id: row.id,
                        plan: row.plan.clone(),
                        interval: classified.interval,
                        status,
                        quantity: classified.quantity,
                    }));
                }
                row
            }
        };
        subscription_item::Entity::delete_many()
            .filter(subscription_item::Column::SubscriptionId.eq(row.id))
            .exec(&*tx)
            .await?;
        for (item, meter) in &classified.items {
            subscription_item::ActiveModel {
                id: Set(Uuid::now_v7()),
                subscription_id: Set(row.id),
                provider_item_id: Set(item.id.clone()),
                price_id: Set(item.price_id.clone()),
                meter: Set(meter.clone()),
                quantity: Set(item.quantity.map(|q| i64::try_from(q).unwrap_or(i64::MAX))),
                created_at: Set(now),
            }
            .insert(&*tx)
            .await?;
        }
        Ok(Some(row))
    }

    // ---------------------------------------------------------- events log

    /// The received webhook events, newest first; with `failed_only`, those
    /// whose processing failed and has not succeeded since.
    pub async fn events(&self, failed_only: bool) -> Result<Vec<BillingEventRecord>, BillingError> {
        let mut select = event::Entity::find().order_by_desc(event::Column::ReceivedAt);
        if failed_only {
            select = select
                .filter(event::Column::ProcessedAt.is_null())
                .filter(event::Column::Error.is_not_null());
        }
        Ok(select.all(&self.db.reader().await).await?)
    }

    /// Deletes processed events older than `days` days; returns how many.
    pub async fn prune_events(&self, days: u32) -> Result<u64, BillingError> {
        let before = self.now() - TimeDelta::days(i64::from(days));
        let deleted = event::Entity::delete_many()
            .filter(event::Column::ProcessedAt.is_not_null())
            .filter(event::Column::ProcessedAt.lt(before))
            .exec(self.db.conn())
            .await?;
        Ok(deleted.rows_affected)
    }

    // ---------------------------------------------------------- requests

    /// The billable type the routes and `Subscribed` act on.
    pub(crate) fn route_registration(&self) -> Result<BillableRegistration, String> {
        self.route_billable
            .get_or_init(|| {
                let Some(app) = self.app.upgrade() else {
                    return Err("the app has shut down".to_owned());
                };
                let mut registered: Vec<BillableRegistration> = Vec::new();
                for contributed in app.contributions::<BillableRegistration>() {
                    if !registered
                        .iter()
                        .any(|r| r.billable_type == contributed.value.billable_type)
                    {
                        registered.push(*contributed.value);
                    }
                }
                choose_route_billable(self.options.routes_billable.as_deref(), &registered)
            })
            .clone()
    }

    /// The billable of a request (see `billing.routes_billable`).
    pub(crate) async fn request_billable(
        &self,
        parts: &mut Parts,
    ) -> Result<BillableRef, AppError> {
        let registration = self.route_registration().map_err(AppError::internal)?;
        (registration.resolve)(parts).await
    }
}

/// Picks the billable the routes act on from the registered ones.
pub(crate) fn choose_route_billable(
    configured: Option<&str>,
    registered: &[BillableRegistration],
) -> Result<BillableRegistration, String> {
    let names = || {
        registered
            .iter()
            .map(|r| format!("`{}`", r.billable_type))
            .collect::<Vec<_>>()
            .join(", ")
    };
    match configured {
        Some(kind) => registered
            .iter()
            .find(|r| r.billable_type == kind)
            .copied()
            .ok_or_else(|| {
                format!(
                    "billing.routes_billable is `{kind}`, but no module registers that billable with BillingModule::for_feature (registered: {})",
                    names()
                )
            }),
        None => match registered {
            [] => Err(
                "no billable is registered: import BillingModule::for_feature::<Workspace>() (or another Billable) in the module that owns billing"
                    .to_owned(),
            ),
            [only] => Ok(*only),
            _ => Err(format!(
                "several billables are registered ({}): set billing.routes_billable to the one the routes act on",
                names()
            )),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::billable::USER_BILLABLE;

    fn registration(kind: &'static str) -> BillableRegistration {
        BillableRegistration {
            billable_type: kind,
            resolve: |_| Box::pin(async { Err(AppError::internal("unused")) }),
        }
    }

    #[test]
    fn the_route_billable_is_the_only_one_or_the_configured_one() {
        let workspace = registration(WORKSPACE_BILLABLE);
        let user = registration(USER_BILLABLE);
        assert_eq!(
            choose_route_billable(None, &[workspace])
                .unwrap()
                .billable_type,
            "workspace"
        );
        let both = [workspace, user];
        let error = choose_route_billable(None, &both).unwrap_err();
        assert!(error.contains("billing.routes_billable"), "{error}");
        assert_eq!(
            choose_route_billable(Some("user"), &both)
                .unwrap()
                .billable_type,
            "user"
        );
        assert!(choose_route_billable(Some("organization"), &both).is_err());
        assert!(
            choose_route_billable(None, &[])
                .unwrap_err()
                .contains("for_feature")
        );
    }
}
