//! [`BillingError`] and its HTTP mapping.

use std::collections::BTreeMap;

use http::StatusCode;
use navree_http::AppError;
use navree_kernel::BoxError;
use sea_orm::DbErr;

use crate::provider::{ProviderError, WebhookError};

/// A billing error. Returned from a handler with `?`, it becomes an error
/// response in Navree's JSON error shape:
///
/// | Error | Status | Message |
/// |---|---|---|
/// | `UnknownPlan` | 400 | `Unknown plan \`<key>\`.` (under `errors.plan`) |
/// | `NoPrice` | 400 | `Plan \`<key>\` has no <interval> price.` (under `errors.interval`) |
/// | `UnknownMeter` | 400 | `Unknown meter \`<key>\` for this plan.` (under `errors.meter`) |
/// | `Invalid` | 400 | the field's message, under `errors.<field>` |
/// | `AlreadySubscribed` | 409 | `Already subscribed; change the plan with swap instead.` |
/// | `NoSubscription` | 404 | `No subscription.` |
/// | `NoCustomer` | 404 | `No billing customer yet.` |
/// | `InvalidWebhook` | 400 | `Invalid webhook signature.` |
/// | `InvalidPayload` | 400 | `Invalid webhook payload.` |
/// | `Provider` | 502 | `The payment provider refused the request: ...` |
/// | `Db`, `Fake`, `Config`, `Other` | 500 | |
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BillingError {
    /// No plan with this key under `billing.plans`.
    #[error("unknown plan `{0}`")]
    UnknownPlan(String),
    /// The plan has no price for the interval.
    #[error("plan `{plan}` has no {interval} price")]
    NoPrice {
        /// The plan key.
        plan: String,
        /// `monthly` or `yearly`.
        interval: &'static str,
    },
    /// No plan has this meter, or the billable's plan does not.
    #[error("unknown meter `{0}`")]
    UnknownMeter(String),
    /// An input value is invalid.
    #[error("invalid {field}: {message}")]
    Invalid {
        /// The field.
        field: &'static str,
        /// What is wrong.
        message: String,
    },
    /// The billable already has a valid subscription.
    #[error("the billable already has a valid subscription")]
    AlreadySubscribed,
    /// The billable has no subscription to change.
    #[error("the billable has no subscription")]
    NoSubscription,
    /// The billable has no customer at the provider yet.
    #[error("the billable has no customer at the provider")]
    NoCustomer,
    /// The webhook's signature is missing, wrong or too old.
    #[error("invalid webhook: {0}")]
    InvalidWebhook(#[from] WebhookError),
    /// The webhook's body cannot be read.
    #[error("invalid webhook payload: {0}")]
    InvalidPayload(String),
    /// The provider refused the request or could not be reached.
    #[error(transparent)]
    Provider(#[from] ProviderError),
    /// A database error.
    #[error("billing database error: {0}")]
    Db(#[from] DbErr),
    /// A misuse of the fake provider.
    #[error("billing fake: {0}")]
    Fake(String),
    /// The module is misconfigured.
    #[error("billing configuration: {0}")]
    Config(String),
    /// Anything else.
    #[error(transparent)]
    Other(BoxError),
}

impl BillingError {
    /// An [`Invalid`](BillingError::Invalid) error for `field`.
    pub fn invalid(field: &'static str, message: impl Into<String>) -> Self {
        BillingError::Invalid {
            field,
            message: message.into(),
        }
    }

    /// The error response (also what the module's `map_error` registers).
    pub fn to_app_error(&self) -> AppError {
        map_billing_error(self).unwrap_or_else(|| AppError::internal(self.to_string()))
    }
}

fn field_error(field: &str, message: String) -> AppError {
    AppError::validation(BTreeMap::from([(field.to_owned(), vec![message])]))
}

/// The error response of a [`BillingError`]; `None` keeps a 500.
pub(crate) fn map_billing_error(error: &BillingError) -> Option<AppError> {
    Some(match error {
        BillingError::UnknownPlan(plan) => field_error("plan", format!("Unknown plan `{plan}`.")),
        BillingError::NoPrice { plan, interval } => field_error(
            "interval",
            format!("Plan `{plan}` has no {interval} price."),
        ),
        BillingError::UnknownMeter(meter) => {
            field_error("meter", format!("Unknown meter `{meter}` for this plan."))
        }
        BillingError::Invalid { field, message } => field_error(field, message.clone()),
        BillingError::AlreadySubscribed => {
            AppError::conflict("Already subscribed; change the plan with swap instead.")
        }
        BillingError::NoSubscription => AppError::not_found().with_detail("No subscription."),
        BillingError::NoCustomer => AppError::not_found().with_detail("No billing customer yet."),
        BillingError::InvalidWebhook(_) => AppError::bad_request("Invalid webhook signature."),
        BillingError::InvalidPayload(_) => AppError::bad_request("Invalid webhook payload."),
        BillingError::Provider(provider) => AppError::from_status(StatusCode::BAD_GATEWAY)
            .with_detail(format!(
                "The payment provider refused the request: {}",
                provider_message(provider)
            )),
        BillingError::Db(db) => return navree_db::map_db_error(db),
        BillingError::Fake(_) | BillingError::Config(_) | BillingError::Other(_) => return None,
    })
}

/// What a client may see of a provider error (no URLs, no keys).
fn provider_message(error: &ProviderError) -> String {
    match error {
        ProviderError::Api { message, .. } => message.clone(),
        ProviderError::Transport { .. } => "it could not be reached".to_owned(),
        ProviderError::Decode { .. } => "its answer was unexpected".to_owned(),
        ProviderError::Unsupported { operation, .. } => format!("{operation} is not supported"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_map_to_statuses() {
        let cases = [
            (BillingError::UnknownPlan("gold".into()), 400),
            (
                BillingError::NoPrice {
                    plan: "pro".into(),
                    interval: "yearly",
                },
                400,
            ),
            (BillingError::UnknownMeter("ai".into()), 400),
            (
                BillingError::invalid("quantity", "quantity must be at least 1"),
                400,
            ),
            (BillingError::AlreadySubscribed, 409),
            (BillingError::NoSubscription, 404),
            (BillingError::NoCustomer, 404),
            (
                BillingError::InvalidWebhook(WebhookError::SignatureMismatch),
                400,
            ),
            (BillingError::InvalidPayload("x".into()), 400),
            (
                BillingError::Provider(ProviderError::Api {
                    provider: "stripe",
                    status: 402,
                    message: "Your card was declined.".into(),
                }),
                502,
            ),
            (
                BillingError::Db(DbErr::RecordNotFound("subscriptions".into())),
                404,
            ),
            (BillingError::Fake("x".into()), 500),
            (BillingError::Config("x".into()), 500),
            (BillingError::Other("boom".into()), 500),
        ];
        for (error, status) in cases {
            assert_eq!(error.to_app_error().status.as_u16(), status, "{error}");
        }
        let body = BillingError::UnknownPlan("gold".into())
            .to_app_error()
            .body();
        assert_eq!(body["errors"]["plan"][0], "Unknown plan `gold`.");
        let body = BillingError::Provider(ProviderError::Transport {
            provider: "stripe",
            message: "https://api.stripe.com refused sk_live_x".into(),
        })
        .to_app_error()
        .body();
        assert!(!body["message"].as_str().unwrap().contains("sk_live"));
    }
}
