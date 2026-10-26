//! [`Subscribed`]: a route that needs a valid subscription.

use std::ops::Deref;

use axum::extract::FromRequestParts;
use http::StatusCode;
use http::request::Parts;
use navree_http::AppError;
use navree_kernel::AppContext;

use crate::billing::Billing;
use crate::subscription::Subscription;

/// The request's billable has a valid subscription (or a trial without a
/// subscription, as a `Trialing` subscription on `billing.trial_plan`);
/// otherwise the extractor answers 402. It derefs to the [`Subscription`].
///
/// The billable is the one the routes act on: the `CurrentWorkspace`, or
/// the `CurrentUser` when you bill users (`billing.routes_billable`).
///
/// ```json
/// { "statusCode": 402, "message": "This workspace needs an active subscription", "error": "Payment Required" }
/// ```
#[derive(Clone, Debug)]
pub struct Subscribed(pub Subscription);

impl Subscribed {
    /// The subscription.
    pub fn into_inner(self) -> Subscription {
        self.0
    }
}

impl Deref for Subscribed {
    type Target = Subscription;

    fn deref(&self) -> &Subscription {
        &self.0
    }
}

impl<S: Send + Sync> FromRequestParts<S> for Subscribed {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        if let Some(subscribed) = parts.extensions.get::<Subscribed>() {
            return Ok(subscribed.clone());
        }
        let ctx = parts
            .extensions
            .get::<AppContext>()
            .cloned()
            .ok_or_else(|| AppError::internal("Subscribed needs the navree request pipeline"))?;
        let billing = ctx
            .get::<Billing>()
            .await
            .map_err(|_| AppError::internal("Subscribed needs BillingModule in the app"))?;
        let billable = billing.request_billable(parts).await?;
        let access = billing
            .access(&billable)
            .await
            .map_err(|error| error.to_app_error())?;
        match access {
            Some(subscription) => {
                let subscribed = Subscribed(subscription);
                parts.extensions.insert(subscribed.clone());
                Ok(subscribed)
            }
            None => Err(payment_required(&billable.billable_type)),
        }
    }
}

/// The 402 of a billable without a valid subscription.
pub(crate) fn payment_required(billable_type: &str) -> AppError {
    AppError::from_status(StatusCode::PAYMENT_REQUIRED)
        .with_detail(format!("This {billable_type} needs an active subscription"))
}
