//! The billing routes under `{http.api_prefix}/billing` (default
//! `/api/billing`).

use std::collections::BTreeMap;
use std::sync::Arc;

use bytes::Bytes;
use chrono::{DateTime, Utc};
use http::request::Parts;
use http::{HeaderMap, Method};
use navree_auth::AuthService;
use navree_contracts::AuthState;
use navree_http::extract::FromRequestParts;
use navree_http::{ApiMeta, AppError, Controller, Json, RouteDef, schema_fn};
use navree_kernel::{AppContext, BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree_openapi::ToSchema;
use navree_workspace::InWorkspace;
use serde::{Deserialize, Serialize};

use crate::BillingOptions;
use crate::billable::{BillableRef, USER_BILLABLE, WORKSPACE_BILLABLE};
use crate::billing::Billing;
use crate::checkout::CheckoutUrl;
use crate::config::is_http_url;
use crate::error::BillingError;
use crate::invoice::Invoice;
use crate::plan::{Interval, Plan};
use crate::subscription::{Proration, Subscription, SubscriptionStatus};
use crate::webhook::WebhookReceipt;

/// A subscription as the routes return it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct SubscriptionResource {
    /// Always `Subscription`.
    #[serde(rename = "__type")]
    #[schema(rename = "__type", example = "Subscription")]
    pub kind: String,
    /// The local id (nil for a trial without a subscription).
    pub id: uuid::Uuid,
    /// The plan key.
    pub plan: String,
    /// Monthly or yearly.
    pub interval: Interval,
    /// The provider's status.
    pub status: SubscriptionStatus,
    /// Seats.
    pub quantity: u32,
    /// When the trial ends.
    pub trial_ends_at: Option<DateTime<Utc>>,
    /// When a canceled subscription ends.
    pub ends_at: Option<DateTime<Utc>>,
    /// The end of the current period.
    pub current_period_end: Option<DateTime<Utc>>,
    /// `Subscription::valid`.
    pub valid: bool,
    /// `Subscription::on_trial`.
    pub on_trial: bool,
    /// `Subscription::on_grace_period`.
    pub on_grace_period: bool,
    /// `Subscription::canceled`.
    pub canceled: bool,
    /// `Subscription::past_due`.
    pub past_due: bool,
    /// `Subscription::paused`.
    pub paused: bool,
}

impl From<&Subscription> for SubscriptionResource {
    fn from(sub: &Subscription) -> Self {
        SubscriptionResource {
            kind: "Subscription".to_owned(),
            id: sub.id,
            plan: sub.plan.clone(),
            interval: sub.interval,
            status: sub.status,
            quantity: sub.quantity,
            trial_ends_at: sub.trial_ends_at,
            ends_at: sub.ends_at,
            current_period_end: sub.current_period_end,
            valid: sub.valid(),
            on_trial: sub.on_trial(),
            on_grace_period: sub.on_grace_period(),
            canceled: sub.canceled(),
            past_due: sub.past_due(),
            paused: sub.paused(),
        }
    }
}

/// A plan's price ids.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct PlanPricesResource {
    /// The monthly price id.
    pub monthly: Option<String>,
    /// The yearly price id.
    pub yearly: Option<String>,
}

/// A configured plan as the routes return it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct PlanResource {
    /// The plan key.
    pub key: String,
    /// The display name.
    pub name: String,
    /// The intervals it has a price for.
    pub intervals: Vec<Interval>,
    /// The price ids.
    pub prices: PlanPricesResource,
    /// The trial on checkout, in days.
    pub trial_days: u32,
    /// Whether the quantity follows the member count.
    pub seats: bool,
    /// The meter keys.
    pub meters: Vec<String>,
}

impl From<&Plan> for PlanResource {
    fn from(plan: &Plan) -> Self {
        PlanResource {
            key: plan.key.clone(),
            name: plan.name.clone(),
            intervals: plan.intervals(),
            prices: PlanPricesResource {
                monthly: plan.monthly_price.clone(),
                yearly: plan.yearly_price.clone(),
            },
            trial_days: plan.trial_days,
            seats: plan.seats,
            meters: plan.meters.keys().cloned().collect(),
        }
    }
}

/// The trial part of `GET /billing`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct TrialResource {
    /// Whether the billable is on a trial (with or without a subscription).
    pub on_trial: bool,
    /// When it ends.
    pub ends_at: Option<DateTime<Utc>>,
}

/// `GET /billing`: everything a billing settings page shows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct BillingOverview {
    /// The billable's subscription (see `Billing::subscription`).
    pub subscription: Option<SubscriptionResource>,
    /// The trial.
    pub trial: TrialResource,
    /// This period's usage per meter of the current plan.
    pub usage: BTreeMap<String, u64>,
    /// The configured plans.
    pub plans: Vec<PlanResource>,
}

/// The body of `POST /billing/checkout`.
#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct CheckoutInput {
    /// The plan key.
    pub plan: String,
    /// `monthly` (default) or `yearly`.
    pub interval: Option<Interval>,
}

/// The body of `POST /billing/swap`.
#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct SwapInput {
    /// The plan key.
    pub plan: String,
    /// `monthly` or `yearly`; the current interval when absent.
    pub interval: Option<Interval>,
}

/// The body of `POST /billing/portal`.
#[derive(Clone, Debug, Default, Deserialize, ToSchema)]
pub struct PortalInput {
    /// Where the portal returns; default `billing.return_url`. Must share
    /// its origin when `billing.return_url` is set.
    pub return_url: Option<String>,
}

/// A list response: `{"data": [...]}`.
#[derive(Clone, Debug, Serialize)]
struct DataList<T> {
    data: Vec<T>,
}

/// The webhook route's answer.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct WebhookAnswer {
    /// Always true: the event is stored (or was before).
    pub received: bool,
    /// Whether it had been received before.
    pub duplicate: bool,
}

impl From<WebhookReceipt> for WebhookAnswer {
    fn from(receipt: WebhookReceipt) -> Self {
        WebhookAnswer {
            received: true,
            duplicate: receipt.duplicate,
        }
    }
}

/// The request's billable (see `billing.routes_billable`).
struct RouteBillable(BillableRef);

impl FromRequestParts for RouteBillable {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        let billing = billing_of(parts).await?;
        Ok(RouteBillable(billing.request_billable(parts).await?))
    }
}

async fn billing_of(parts: &Parts) -> Result<Arc<Billing>, AppError> {
    let ctx = parts
        .extensions
        .get::<AppContext>()
        .cloned()
        .ok_or_else(|| AppError::internal("the billing routes need the navree request pipeline"))?;
    ctx.get::<Billing>().await.map_err(AppError::internal)
}

/// The signed-in user's email, to prefill a checkout.
struct SignedInEmail(Option<String>);

impl FromRequestParts for SignedInEmail {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        let Some(user_id) = parts
            .extensions
            .get::<AuthState>()
            .and_then(|state| state.0.as_ref())
            .map(|principal| principal.user_id)
        else {
            return Ok(SignedInEmail(None));
        };
        let Some(ctx) = parts.extensions.get::<AppContext>().cloned() else {
            return Ok(SignedInEmail(None));
        };
        let Some(auth) = ctx.try_get::<AuthService>().await else {
            return Ok(SignedInEmail(None));
        };
        let user = auth.find_user(user_id).await.ok().flatten();
        Ok(SignedInEmail(user.map(|user| user.email)))
    }
}

fn billing_error(error: BillingError) -> AppError {
    error.to_app_error()
}

/// The origin (`scheme://host[:port]`) of an absolute URL.
fn origin(url: &str) -> Option<&str> {
    let after_scheme = url.find("://")? + 3;
    let end = url[after_scheme..]
        .find(['/', '?', '#'])
        .map_or(url.len(), |index| after_scheme + index);
    Some(&url[..end])
}

/// The billing routes, mounted under `{http.api_prefix}/billing`. The
/// management routes act on the request's billable (`billing.routes_billable`)
/// and need, for a workspace, the `billing.manage` permission in it (behind
/// the `InWorkspace` guard); for a user, a signed-in user; for a billable of
/// your own, the `billing.manage` permission.
///
/// | Route | Answer |
/// |---|---|
/// | `GET /billing` | [`BillingOverview`]: subscription, trial, usage, plans |
/// | `GET /billing/plans` | `{data: [PlanResource]}` |
/// | `POST /billing/checkout` `{plan, interval?}` | `{url}` of a checkout |
/// | `POST /billing/swap` `{plan, interval?}` | the updated [`SubscriptionResource`] |
/// | `POST /billing/cancel`, `POST /billing/resume` | the updated [`SubscriptionResource`] |
/// | `GET /billing/invoices` | `{data: [Invoice]}` |
/// | `POST /billing/portal` `{return_url?}` | `{url}` of the customer portal |
/// | `POST /billing/webhook` | the provider's webhooks; public and verified |
///
/// `billing.routes = false` drops the management routes;
/// `billing.webhook_route = false` drops the webhook.
pub struct BillingController {
    billing: Arc<Billing>,
    routes: bool,
    webhook: bool,
    billable_type: Option<&'static str>,
}

impl Injectable for BillingController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Billing>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            let billing = r.get::<Billing>().await?;
            let options: &BillingOptions = billing.options();
            let (routes, webhook) = (options.routes, options.webhook_route);
            let billable_type = if routes {
                let registration =
                    billing
                        .route_registration()
                        .map_err(|message| KernelError::Construction {
                            provider: "navree_billing::BillingController".to_owned(),
                            source: format!("{message} (or set billing.routes = false)").into(),
                        })?;
                Some(registration.billable_type)
            } else {
                None
            };
            Ok(BillingController {
                billing,
                routes,
                webhook,
                billable_type,
            })
        })
    }
}

impl BillingController {
    /// The access of a management route.
    fn guard(&self, route: RouteDef) -> RouteDef {
        match self.billable_type {
            Some(USER_BILLABLE) => route.authenticated(),
            Some(WORKSPACE_BILLABLE) => route
                .permissions(&["billing.manage"])
                .guard(InWorkspace::guard()),
            _ => route.permissions(&["billing.manage"]),
        }
    }
}

impl Controller for BillingController {
    fn prefix(&self) -> &'static str {
        "/billing"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let mut routes = Vec::new();
        if self.routes {
            routes.extend(self.management_routes());
        }
        if self.webhook {
            let billing = self.billing.clone();
            let webhook = move |headers: HeaderMap, body: Bytes| {
                let billing = billing.clone();
                async move {
                    let receipt = billing
                        .receive_webhook(&headers, &body)
                        .await
                        .map_err(billing_error)?;
                    Ok::<_, AppError>(Json(WebhookAnswer::from(receipt)))
                }
            };
            routes.push(
                RouteDef::on(Method::POST, "/webhook", webhook)
                    .public()
                    .name("billing.webhook")
                    .api(api(
                        "Receive the payment provider's webhooks",
                        &[400],
                        None,
                        Some(schema_fn::<WebhookAnswer>),
                    )),
            );
        }
        routes
    }
}

impl BillingController {
    fn management_routes(self: &Arc<Self>) -> Vec<RouteDef> {
        let billing = self.billing.clone();
        let overview = move |RouteBillable(billable): RouteBillable| {
            let billing = billing.clone();
            async move {
                let subscription = billing
                    .subscription(&billable)
                    .await
                    .map_err(billing_error)?;
                let current = billing.access(&billable).await.map_err(billing_error)?;
                let mut usage = BTreeMap::new();
                if let Some(plan) = current.as_ref().and_then(|sub| billing.plan(&sub.plan)) {
                    for meter in plan.meters.keys() {
                        let used = billing
                            .usage(&billable, meter)
                            .await
                            .map_err(billing_error)?;
                        usage.insert(meter.clone(), used);
                    }
                }
                let trial = TrialResource {
                    on_trial: billing.on_trial(&billable).await.map_err(billing_error)?,
                    ends_at: billing
                        .trial_ends_at(&billable)
                        .await
                        .map_err(billing_error)?,
                };
                Ok::<_, AppError>(Json(BillingOverview {
                    subscription: subscription.as_ref().map(SubscriptionResource::from),
                    trial,
                    usage,
                    plans: billing.plans().iter().map(PlanResource::from).collect(),
                }))
            }
        };
        let billing = self.billing.clone();
        let plans = move || {
            let billing = billing.clone();
            async move {
                Json(DataList::<PlanResource> {
                    data: billing.plans().iter().map(PlanResource::from).collect(),
                })
            }
        };
        let billing = self.billing.clone();
        let checkout = move |RouteBillable(billable): RouteBillable,
                             SignedInEmail(email): SignedInEmail,
                             Json(input): Json<CheckoutInput>| {
            let billing = billing.clone();
            async move {
                let mut checkout = billing
                    .checkout(&billable, &input.plan)
                    .interval(input.interval.unwrap_or_default());
                if let Some(email) = email {
                    checkout = checkout.customer_email(email);
                }
                let created = checkout.create().await.map_err(billing_error)?;
                Ok::<_, AppError>(Json(CheckoutUrl::from(created)))
            }
        };
        let billing = self.billing.clone();
        let swap = move |RouteBillable(billable): RouteBillable, Json(input): Json<SwapInput>| {
            let billing = billing.clone();
            async move {
                let sub = billing
                    .swap_to(
                        &billable,
                        &input.plan,
                        input.interval,
                        Proration::NextInvoice,
                    )
                    .await
                    .map_err(billing_error)?;
                Ok::<_, AppError>(Json(SubscriptionResource::from(&sub)))
            }
        };
        let billing = self.billing.clone();
        let cancel = move |RouteBillable(billable): RouteBillable| {
            let billing = billing.clone();
            async move {
                let sub = billing.cancel(&billable).await.map_err(billing_error)?;
                Ok::<_, AppError>(Json(SubscriptionResource::from(&sub)))
            }
        };
        let billing = self.billing.clone();
        let resume = move |RouteBillable(billable): RouteBillable| {
            let billing = billing.clone();
            async move {
                let sub = billing.resume(&billable).await.map_err(billing_error)?;
                Ok::<_, AppError>(Json(SubscriptionResource::from(&sub)))
            }
        };
        let billing = self.billing.clone();
        let invoices = move |RouteBillable(billable): RouteBillable| {
            let billing = billing.clone();
            async move {
                let data = billing.invoices(&billable).await.map_err(billing_error)?;
                Ok::<_, AppError>(Json(DataList::<Invoice> { data }))
            }
        };
        let billing = self.billing.clone();
        let portal = move |RouteBillable(billable): RouteBillable, body: Bytes| {
            let billing = billing.clone();
            async move {
                let input: PortalInput = if body.is_empty() {
                    PortalInput::default()
                } else {
                    serde_json::from_slice(&body).map_err(|error| {
                        AppError::bad_request(format!("Invalid JSON body: {error}"))
                    })?
                };
                let return_url = match (input.return_url, billing.return_url()) {
                    (Some(url), Some(default)) => {
                        if !is_http_url(&url) || origin(&url) != origin(default) {
                            return Err(BillingError::invalid(
                                "return_url",
                                "return_url must be on the same origin as billing.return_url",
                            )
                            .to_app_error());
                        }
                        url
                    }
                    (Some(url), None) => {
                        if !is_http_url(&url) {
                            return Err(BillingError::invalid(
                                "return_url",
                                "return_url must be an absolute http(s) URL",
                            )
                            .to_app_error());
                        }
                        url
                    }
                    (None, Some(default)) => default.to_owned(),
                    (None, None) => {
                        return Err(BillingError::invalid(
                            "return_url",
                            "return_url is required when billing.return_url and app.url are unset",
                        )
                        .to_app_error());
                    }
                };
                let url = billing
                    .portal_url(&billable, &return_url)
                    .await
                    .map_err(billing_error)?;
                Ok::<_, AppError>(Json(CheckoutUrl { url }))
            }
        };
        vec![
            self.guard(RouteDef::on(Method::GET, "/", overview))
                .name("billing.show")
                .api(api(
                    "The billable's subscription, trial, usage and the plans",
                    &[401, 403],
                    None,
                    Some(schema_fn::<BillingOverview>),
                )),
            self.guard(RouteDef::on(Method::GET, "/plans", plans))
                .name("billing.plans")
                .api(api("The configured plans", &[401, 403], None, None)),
            self.guard(RouteDef::on(Method::POST, "/checkout", checkout))
                .name("billing.checkout")
                .api(api(
                    "Start a hosted checkout",
                    &[400, 401, 403, 409, 502],
                    Some(schema_fn::<CheckoutInput>),
                    Some(schema_fn::<CheckoutUrl>),
                )),
            self.guard(RouteDef::on(Method::POST, "/swap", swap))
                .name("billing.swap")
                .api(api(
                    "Change the plan",
                    &[400, 401, 403, 404, 502],
                    Some(schema_fn::<SwapInput>),
                    Some(schema_fn::<SubscriptionResource>),
                )),
            self.guard(RouteDef::on(Method::POST, "/cancel", cancel))
                .name("billing.cancel")
                .api(api(
                    "Cancel at the end of the period",
                    &[401, 403, 404, 502],
                    None,
                    Some(schema_fn::<SubscriptionResource>),
                )),
            self.guard(RouteDef::on(Method::POST, "/resume", resume))
                .name("billing.resume")
                .api(api(
                    "Undo a cancel before the period ends",
                    &[400, 401, 403, 404, 502],
                    None,
                    Some(schema_fn::<SubscriptionResource>),
                )),
            self.guard(RouteDef::on(Method::GET, "/invoices", invoices))
                .name("billing.invoices")
                .api(api("The invoices", &[401, 403, 502], None, None)),
            self.guard(RouteDef::on(Method::POST, "/portal", portal))
                .name("billing.portal")
                .api(api(
                    "Open the customer portal",
                    &[400, 401, 403, 404, 502],
                    Some(schema_fn::<PortalInput>),
                    Some(schema_fn::<CheckoutUrl>),
                )),
        ]
    }
}

fn api(
    summary: &str,
    responses: &[u16],
    request: Option<navree_http::SchemaFn>,
    response: Option<navree_http::SchemaFn>,
) -> ApiMeta {
    ApiMeta {
        summary: Some(summary.to_owned()),
        tags: vec!["Billing".to_owned()],
        responses: responses.to_vec(),
        request,
        response,
        ..ApiMeta::default()
    }
}

#[cfg(test)]
mod tests {
    use navree_http::handler::Endpoint;

    use super::*;

    #[test]
    fn handlers_on_the_billing_extractors_are_native() {
        async fn handler(_: RouteBillable, _: SignedInEmail, _: crate::Subscribed) {}
        let _ = Endpoint::new(handler);
    }

    #[test]
    fn origins_are_scheme_and_host() {
        assert_eq!(
            origin("https://app.test/settings?x=1"),
            Some("https://app.test")
        );
        assert_eq!(
            origin("http://localhost:3000"),
            Some("http://localhost:3000")
        );
        assert_eq!(origin("https://app.test#x"), Some("https://app.test"));
        assert_eq!(origin("app.test"), None);
    }
}
