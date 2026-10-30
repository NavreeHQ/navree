//! The billing docs page's examples (`packages/billing.mdx`), compiled
//! through the facade with the macros, and the testing example run end to
//! end. Billing is opt-in: `cargo test -p navree --features billing --test billing`.
#![cfg(feature = "sqlite")]
#![cfg(feature = "billing")]

use std::sync::Mutex;

use navree::BoxFuture;
use navree::billing::{Billable, Billing, CheckoutUrl, PaymentFailed, Proration, Subscribed};
use navree::db::Db;
use navree::events::{Listener, ModuleBuilderEventsExt};
use navree::prelude::*;
use navree::queue::Queue;
use navree::testing::{TestApp, principal};
use navree::workspace::{CurrentWorkspace, NewWorkspace, Workspace, Workspaces};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

// ------------------------------------------------------------ installation

/// "Installation": the app root.
#[allow(dead_code)]
fn app(sources: ConfigSources) -> AppBuilder {
    Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(CryptoModule::from_config())
        .module(DatabaseModule::from_config())
        .module(EventsModule::for_root())
        .module(QueueModule::from_config())
        .module(ScheduleModule::for_root()) // reports metered usage every minute
        .module(HttpClientModule::from_config())
        .module(AuthModule::from_config())
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(WorkspaceModule::from_config())
        .module(BillingModule::from_config()) // section `billing`
        .module(SettingsModule)
}

// ------------------------------------------------------------ billables

/// "Billable models": a type of your own.
mod organizations {
    use navree::billing::Billable;
    use uuid::Uuid;

    pub struct Organization {
        pub id: Uuid,
        pub legal_name: String,
        pub billing_email: Option<String>,
    }

    impl Billable for Organization {
        /// Stored with customers and subscriptions; keep it stable.
        const BILLABLE_TYPE: &'static str = "organization";

        fn billable_id(&self) -> Uuid {
            self.id
        }

        fn billing_name(&self) -> String {
            self.legal_name.clone()
        }

        fn billing_email(&self) -> Option<String> {
            self.billing_email.clone()
        }
    }
}

// ------------------------------------------------------------ checkout

/// The body of `POST /api/v1/billing/upgrade`.
#[derive(Debug, Deserialize, garde::Validate)]
pub struct Upgrade {
    /// The plan key.
    #[garde(length(min = 1))]
    pub plan: String,
    /// The yearly price instead of the monthly one.
    #[garde(skip)]
    pub yearly: bool,
}

/// The upgrade route.
#[injectable]
pub struct BillingController {
    billing: Arc<Billing>,
}

#[controller("/billing/upgrade", version = "1")]
#[guard(InWorkspace::guard())]
impl BillingController {
    /// `POST /api/v1/billing/upgrade`: a checkout URL for the current workspace.
    #[post("/")]
    #[permissions("billing.manage")]
    async fn upgrade(
        &self,
        workspace: CurrentWorkspace,
        CurrentUser(user): CurrentUser,
        Valid(Json(input)): Valid<Json<Upgrade>>,
    ) -> Result<Json<CheckoutUrl>> {
        let mut checkout = self
            .billing
            .checkout(&workspace, &input.plan)
            .customer_email(&user.email)
            .success_url("https://app.acme.test/settings/billing?upgraded=1")
            .cancel_url("https://app.acme.test/settings/billing");
        if input.yearly {
            checkout = checkout.yearly();
        }
        Ok(Json(checkout.create().await?.into()))
    }
}

/// "Requiring a subscription on a route".
#[injectable]
pub struct SummaryController;

#[controller("/deals", version = "1")]
impl SummaryController {
    #[post("/{id}/summary")]
    #[authenticated]
    async fn summarize(
        &self,
        subscription: Subscribed,
        Path(id): Path<Uuid>,
    ) -> Result<Json<Uuid>> {
        if !subscription.on_plan("pro") {
            return Err(AppError::forbidden().with_detail("AI summaries are part of the Pro plan"));
        }
        Ok(Json(id))
    }
}

/// The module that owns billing.
#[module(
    imports = [BillingModule::for_feature::<Workspace>()],
    controllers = [BillingController, SummaryController],
    register = Self::wire,
)]
pub struct SettingsModule;

impl SettingsModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.value(Warnings::default())
            .listen::<PaymentFailed, WarnOwnerOfFailedPayment>();
    }
}

// ------------------------------------------------------------ events

/// Who was warned (the docs' listener sends a notification).
#[derive(Default)]
pub struct Warnings(Mutex<Vec<(String, i64)>>);

/// Tells a workspace's owner that a payment failed.
#[injectable]
pub struct WarnOwnerOfFailedPayment {
    workspaces: Arc<Workspaces>,
    auth: Arc<AuthService>,
    warnings: Arc<Warnings>,
}

impl Listener<PaymentFailed> for WarnOwnerOfFailedPayment {
    fn handle<'a>(
        &'a self,
        event: &'a PaymentFailed,
        _ctx: &'a AppContext,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async move {
            let Some(workspace) = self.workspaces.find(event.billable_id).await? else {
                return Ok(()); // deleted since
            };
            if let Some(owner) = self.auth.find_user(workspace.owner_id).await? {
                self.warnings
                    .0
                    .lock()
                    .unwrap()
                    .push((owner.email, event.amount_cents));
            }
            Ok(())
        })
    }

    fn queued(&self) -> bool {
        true
    }
}

// ------------------------------------------------------------ the service

/// "Checking subscriptions", "Changing subscriptions", "Trials", "Metered
/// usage", "Invoices and the customer portal": the calls as documented.
#[allow(dead_code)]
async fn service_calls(billing: &Billing, workspace: &Workspace) -> Result<(), BoxError> {
    let subscription = billing.subscription(workspace).await?;
    match subscription {
        Some(sub) if sub.valid() => {
            println!(
                "{} on {} until {:?}",
                workspace.name, sub.plan, sub.current_period_end
            );
        }
        Some(sub) if sub.past_due() => { /* ask for a new card */ }
        _ => { /* free tier */ }
    }
    if billing.subscribed(workspace, "pro").await? {
        // Pro features
    }

    billing.swap(workspace, "business").await?; // prorated, effective now
    billing.swap_yearly(workspace, "pro").await?; // the plan's yearly price
    billing.update_quantity(workspace, 12).await?; // seats
    billing.cancel(workspace).await?; // at the end of the period
    billing.cancel_now(workspace).await?; // at once, no refund
    billing.resume(workspace).await?; // undo a cancel before it ends
    billing.pause(workspace).await?; // stop charging, keep it
    billing.unpause(workspace).await?;
    billing
        .swap_with(workspace, "business", Proration::InvoiceNow)
        .await?;

    billing.start_trial(workspace, 14).await?;
    billing.on_trial(workspace).await?;
    billing.trial_ends_at(workspace).await?;

    billing.report_usage(workspace, "ai_summaries", 1).await?;
    let _used = billing.usage(workspace, "ai_summaries").await?;

    for invoice in billing.invoices(workspace).await? {
        println!(
            "{} {} {} {}",
            invoice.number, invoice.status, invoice.total_cents, invoice.currency
        );
    }
    let _url = billing
        .portal_url(workspace, "https://app.acme.test/settings/billing")
        .await?;
    let organization = organizations::Organization {
        id: Uuid::now_v7(),
        legal_name: "Acme Holdings".into(),
        billing_email: None,
    };
    assert_eq!(organization.billable_id(), organization.id);
    billing.subscription(&organization).await?;
    Ok(())
}

// ------------------------------------------------------------ testing

#[tokio::test]
async fn upgrading_starts_a_pro_trial() {
    let app = TestApp::builder()
        .temp_database()
        .fake_auth()
        .config("app.key", navree::crypto::generate_key())
        .config("billing.provider", "fake")
        .config("billing.plans.pro.prices.monthly", "price_pro")
        .config("billing.plans.pro.trial_days", 14)
        .config("queue.driver", "memory")
        .module(CryptoModule::from_config())
        .module(DatabaseModule::from_config())
        .module(EventsModule::for_root())
        .module(QueueModule::from_config())
        .module(AuthModule::from_config())
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(WorkspaceModule::from_config())
        .module(BillingModule::from_config())
        .module(SettingsModule)
        .build()
        .await;
    let owner = app
        .get::<AuthService>()
        .await
        .create_user("Ada Lovelace", "ada@example.com", "password123")
        .await
        .unwrap();
    let uow = app.get::<Db>().await.uow(None, None).await;
    let workspace = app
        .get::<Workspaces>()
        .await
        .create(&uow, owner.id, NewWorkspace::named("Acme"))
        .await
        .unwrap();
    uow.commit().await.unwrap();

    let response = app
        .acting_as(principal(owner.id))
        .post("/api/v1/billing/upgrade")
        .header("X-Workspace", "acme")
        .json(&json!({ "plan": "pro", "yearly": false }))
        .send()
        .await
        .assert_status(200);

    let billing = app.get::<Billing>().await;
    let fake = billing.fake().expect("the fake provider");
    let checkout = fake.assert_checkout(|c| c.plan == "pro" && c.trial_days == Some(14));
    assert_eq!(response.json_value()["url"], json!(checkout.url));
    assert_eq!(checkout.customer_email.as_deref(), Some("ada@example.com"));

    fake.complete_checkout(&checkout.id).await.unwrap();
    app.get::<Queue>().await.run_pending().await.unwrap();

    let sub = billing.subscription(&workspace).await.unwrap().unwrap();
    assert!(sub.on_trial() && sub.on_plan("pro"));

    // The subscribed route lets the Pro workspace through.
    let deal = Uuid::now_v7();
    app.acting_as(principal(owner.id))
        .post(&format!("/api/v1/deals/{deal}/summary"))
        .header("X-Workspace", "acme")
        .send()
        .await
        .assert_status(200);
    // Unknown plans are a 400 naming the field; a second checkout a 409.
    app.acting_as(principal(owner.id))
        .post("/api/v1/billing/upgrade")
        .header("X-Workspace", "acme")
        .json(&json!({ "plan": "pro", "yearly": false }))
        .send()
        .await
        .assert_status(409);

    // The trial ends with a failed payment: the owner is warned.
    fake.fail_next_payment(&workspace);
    fake.end_trial(&workspace).await.unwrap();
    app.get::<Queue>().await.run_pending().await.unwrap();
    let warnings = app.get::<Warnings>().await;
    assert_eq!(
        *warnings.0.lock().unwrap(),
        [("ada@example.com".to_owned(), 1000)]
    );
}

#[tokio::test]
async fn a_workspace_without_a_subscription_gets_a_402() {
    let app = TestApp::builder()
        .temp_database()
        .fake_auth()
        .config("app.key", navree::crypto::generate_key())
        .config("billing.provider", "fake")
        .config("billing.plans.pro.prices.monthly", "price_pro")
        .module(CryptoModule::from_config())
        .module(DatabaseModule::from_config())
        .module(AuthModule::from_config())
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(WorkspaceModule::from_config())
        .module(BillingModule::from_config())
        .module(SettingsModule)
        .build()
        .await;
    let owner = app
        .get::<AuthService>()
        .await
        .create_user("Ada Lovelace", "ada@example.com", "password123")
        .await
        .unwrap();
    let uow = app.get::<Db>().await.uow(None, None).await;
    app.get::<Workspaces>()
        .await
        .create(&uow, owner.id, NewWorkspace::named("Acme"))
        .await
        .unwrap();
    uow.commit().await.unwrap();
    app.acting_as(principal(owner.id))
        .post(&format!("/api/v1/deals/{}/summary", Uuid::now_v7()))
        .header("X-Workspace", "acme")
        .send()
        .await
        .assert_error(402, "This workspace needs an active subscription")
        .assert_error_code(402, "subscription_required");
}
