//! The billing tables (migration set `billing`) as navree-orm models
//! (`subscription::Model::query()`, typed columns such as
//! `subscription::Model::STATUS`).
//!
//! - `billing_customers(id, billable_type, billable_id, provider,
//!   provider_customer_id, email, trial_ends_at, created_at, updated_at)`,
//!   one row per billable;
//! - `subscriptions(id, billable_type, billable_id, provider, provider_id
//!   UNIQUE, provider_customer_id, plan, interval, status, quantity,
//!   trial_ends_at, ends_at, current_period_start, current_period_end,
//!   past_due_since, provider_updated_at, created_at, updated_at)`;
//! - `subscription_items(id, subscription_id → subscriptions ON DELETE
//!   CASCADE, provider_item_id, price_id, meter, quantity, created_at)`;
//! - `billing_usage(id, billable_type, billable_id, meter, quantity,
//!   recorded_at, reported_at)`;
//! - `billing_events(id, provider, provider_event_id, event_type, payload,
//!   received_at, processed_at, attempts, error)`, unique per provider and
//!   event id.
//!
//! Read and change them through [`Billing`](crate::Billing): a subscription
//! row is only as fresh as the last webhook, and the service knows which
//! fields the provider owns.

/// `billing_customers`: the provider's customer of each billable, and the
/// trial without a subscription.
pub mod customer {
    use chrono::{DateTime, Utc};
    use serde::Serialize;
    use uuid::Uuid;

    /// A billable's customer record.
    #[derive(Clone, Debug, PartialEq, Eq, navree_orm::Model, Serialize)]
    #[model(table = "billing_customers")]
    pub struct Model {
        /// The id (UUID v7).
        pub id: Uuid,
        /// `Billable::BILLABLE_TYPE` (`workspace`, `user`, ...).
        pub billable_type: String,
        /// `Billable::billable_id`.
        pub billable_id: Uuid,
        /// The provider the customer lives at (`stripe`, `paddle`, `fake`).
        pub provider: Option<String>,
        /// The provider's customer id, once one exists.
        pub provider_customer_id: Option<String>,
        /// The billing email the customer was created with.
        pub email: Option<String>,
        /// The end of a trial without a subscription.
        pub trial_ends_at: Option<DateTime<Utc>>,
        /// When the row was created.
        pub created_at: DateTime<Utc>,
        /// When the row last changed.
        pub updated_at: DateTime<Utc>,
    }
}

/// The access subject of subscriptions (`Subscription`), for admin panels
/// (Helm) and `#[can]` checks; permissions `subscriptions.<action>`.
pub const SUBSCRIPTION_SUBJECT: &str = "Subscription";

/// `subscriptions`: the local copy of the provider's subscriptions.
///
/// The model is the access subject [`SUBSCRIPTION_SUBJECT`] with the
/// permission prefix `subscriptions`, so an app can list subscriptions in
/// Helm or check `#[can(Read, Subscription)]`.
pub mod subscription {
    use chrono::{DateTime, Utc};
    use serde::Serialize;
    use uuid::Uuid;

    /// A subscription row.
    #[derive(Clone, Debug, PartialEq, Eq, navree_orm::Model, Serialize)]
    #[model(
        table = "subscriptions",
        subject = "Subscription",
        permission_prefix = "subscriptions"
    )]
    pub struct Model {
        /// The id (UUID v7).
        pub id: Uuid,
        /// `Billable::BILLABLE_TYPE`.
        pub billable_type: String,
        /// `Billable::billable_id`.
        pub billable_id: Uuid,
        /// `stripe`, `paddle` or `fake`.
        pub provider: String,
        /// The provider's subscription id.
        pub provider_id: String,
        /// The provider's customer id.
        pub provider_customer_id: String,
        /// The plan key (`pro`).
        pub plan: String,
        /// `monthly` or `yearly`.
        pub interval: String,
        /// `trialing`, `active`, `past_due`, `paused`, `canceled` or
        /// `incomplete`.
        pub status: String,
        /// Seats.
        pub quantity: i64,
        /// When the trial ends.
        pub trial_ends_at: Option<DateTime<Utc>>,
        /// When a canceled subscription ends (or ended).
        pub ends_at: Option<DateTime<Utc>>,
        /// The start of the current period.
        pub current_period_start: Option<DateTime<Utc>>,
        /// The end of the current period.
        pub current_period_end: Option<DateTime<Utc>>,
        /// Since when the subscription is past due.
        pub past_due_since: Option<DateTime<Utc>>,
        /// The time of the provider state this row reflects (older webhook
        /// events are ignored).
        pub provider_updated_at: Option<DateTime<Utc>>,
        /// When the row was created.
        pub created_at: DateTime<Utc>,
        /// When the row last changed.
        pub updated_at: DateTime<Utc>,
    }
}

/// `subscription_items`: each price of a subscription, metered prices
/// included.
pub mod subscription_item {
    use chrono::{DateTime, Utc};
    use serde::Serialize;
    use uuid::Uuid;

    /// A subscription item.
    #[derive(Clone, Debug, PartialEq, Eq, navree_orm::Model, Serialize)]
    #[model(table = "subscription_items")]
    pub struct Model {
        /// The id (UUID v7).
        pub id: Uuid,
        /// The subscription.
        pub subscription_id: Uuid,
        /// The provider's item id (Stripe `si_...`), when it has one.
        pub provider_item_id: Option<String>,
        /// The price id.
        pub price_id: String,
        /// The meter key, for a metered price.
        pub meter: Option<String>,
        /// The quantity (none for metered prices).
        pub quantity: Option<i64>,
        /// When the row was created.
        pub created_at: DateTime<Utc>,
    }
}

/// `billing_usage`: recorded metered usage.
pub mod usage {
    use chrono::{DateTime, Utc};
    use serde::Serialize;
    use uuid::Uuid;

    /// One recorded use.
    #[derive(Clone, Debug, PartialEq, Eq, navree_orm::Model, Serialize)]
    #[model(table = "billing_usage")]
    pub struct Model {
        /// The id (UUID v7).
        pub id: Uuid,
        /// `Billable::BILLABLE_TYPE`.
        pub billable_type: String,
        /// `Billable::billable_id`.
        pub billable_id: Uuid,
        /// The meter key.
        pub meter: String,
        /// How much was used.
        pub quantity: i64,
        /// When it was recorded.
        pub recorded_at: DateTime<Utc>,
        /// When `billing:report-usage` sent it to the provider.
        pub reported_at: Option<DateTime<Utc>>,
    }
}

/// `billing_events`: every webhook received.
pub mod event {
    use chrono::{DateTime, Utc};
    use serde::Serialize;
    use uuid::Uuid;

    /// A received webhook event.
    #[derive(Clone, Debug, PartialEq, Eq, navree_orm::Model, Serialize)]
    #[model(table = "billing_events")]
    pub struct Model {
        /// The id (UUID v7).
        pub id: Uuid,
        /// `stripe`, `paddle` or `fake`.
        pub provider: String,
        /// The provider's event id (`evt_...`, `evt_01h...`).
        pub provider_event_id: String,
        /// The provider's event type (`customer.subscription.updated`).
        pub event_type: String,
        /// The raw JSON body.
        pub payload: String,
        /// When it arrived.
        pub received_at: DateTime<Utc>,
        /// When the `billing.process_event` job finished it.
        pub processed_at: Option<DateTime<Utc>>,
        /// Processing attempts so far.
        pub attempts: i32,
        /// The error of the last failed attempt.
        pub error: Option<String>,
    }
}

/// A row of `billing_customers`.
pub type BillingCustomer = customer::Model;
/// A row of `billing_events`.
pub type BillingEventRecord = event::Model;
/// A row of `billing_usage`.
pub type UsageRecord = usage::Model;
/// A row of `subscription_items`.
pub type SubscriptionItem = subscription_item::Model;

#[cfg(test)]
mod tests {
    use navree_access::SubjectRecords;

    use super::*;

    fn subject<S: SubjectRecords>() -> (&'static str, &'static str) {
        (S::SUBJECT, S::PERMISSION_PREFIX)
    }

    #[test]
    fn subscriptions_are_an_access_subject() {
        assert_eq!(SUBSCRIPTION_SUBJECT, "Subscription");
        assert_eq!(
            subject::<subscription::Model>(),
            (SUBSCRIPTION_SUBJECT, "subscriptions")
        );
    }
}
