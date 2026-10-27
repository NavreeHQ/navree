//! The billing tables as SeaORM entities (migration set `billing`).
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
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    /// A billable's customer record.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "billing_customers")]
    pub struct Model {
        /// The id (UUID v7).
        #[sea_orm(primary_key, auto_increment = false)]
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
        pub trial_ends_at: Option<DateTimeUtc>,
        /// When the row was created.
        pub created_at: DateTimeUtc,
        /// When the row last changed.
        pub updated_at: DateTimeUtc,
    }

    /// No relations.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// The access subject of subscriptions (`Subscription`), for admin panels
/// (Helm) and `#[can]` checks; permissions `subscriptions.<action>`.
pub const SUBSCRIPTION_SUBJECT: &str = "Subscription";

/// Subscriptions are the access subject [`SUBSCRIPTION_SUBJECT`] with the
/// permission prefix `subscriptions`, so an app can list them in Helm or
/// check `#[can(Read, Subscription)]`.
impl navree_access::Subject for subscription::Entity {
    const SUBJECT: &'static str = SUBSCRIPTION_SUBJECT;
    const PERMISSION_PREFIX: &'static str = "subscriptions";
}

/// `subscriptions`: the local copy of the provider's subscriptions.
pub mod subscription {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    /// A subscription row.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "subscriptions")]
    pub struct Model {
        /// The id (UUID v7).
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// `Billable::BILLABLE_TYPE`.
        pub billable_type: String,
        /// `Billable::billable_id`.
        pub billable_id: Uuid,
        /// `stripe`, `paddle` or `fake`.
        pub provider: String,
        /// The provider's subscription id.
        #[sea_orm(unique)]
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
        pub trial_ends_at: Option<DateTimeUtc>,
        /// When a canceled subscription ends (or ended).
        pub ends_at: Option<DateTimeUtc>,
        /// The start of the current period.
        pub current_period_start: Option<DateTimeUtc>,
        /// The end of the current period.
        pub current_period_end: Option<DateTimeUtc>,
        /// Since when the subscription is past due.
        pub past_due_since: Option<DateTimeUtc>,
        /// The time of the provider state this row reflects (older webhook
        /// events are ignored).
        pub provider_updated_at: Option<DateTimeUtc>,
        /// When the row was created.
        pub created_at: DateTimeUtc,
        /// When the row last changed.
        pub updated_at: DateTimeUtc,
    }

    /// A subscription has items.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        /// `subscription_items.subscription_id`.
        #[sea_orm(has_many = "super::subscription_item::Entity")]
        Items,
    }

    impl Related<super::subscription_item::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Items.def()
        }
    }

    impl ActiveModelBehavior for ActiveModel {}
}

/// `subscription_items`: each price of a subscription, metered prices
/// included.
pub mod subscription_item {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    /// A subscription item.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "subscription_items")]
    pub struct Model {
        /// The id (UUID v7).
        #[sea_orm(primary_key, auto_increment = false)]
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
        pub created_at: DateTimeUtc,
    }

    /// An item belongs to a subscription.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        /// `subscriptions.id`.
        #[sea_orm(
            belongs_to = "super::subscription::Entity",
            from = "Column::SubscriptionId",
            to = "super::subscription::Column::Id",
            on_delete = "Cascade"
        )]
        Subscription,
    }

    impl Related<super::subscription::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Subscription.def()
        }
    }

    impl ActiveModelBehavior for ActiveModel {}
}

/// `billing_usage`: recorded metered usage.
pub mod usage {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    /// One recorded use.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "billing_usage")]
    pub struct Model {
        /// The id (UUID v7).
        #[sea_orm(primary_key, auto_increment = false)]
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
        pub recorded_at: DateTimeUtc,
        /// When `billing:report-usage` sent it to the provider.
        pub reported_at: Option<DateTimeUtc>,
    }

    /// No relations.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `billing_events`: every webhook received.
pub mod event {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    /// A received webhook event.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "billing_events")]
    pub struct Model {
        /// The id (UUID v7).
        #[sea_orm(primary_key, auto_increment = false)]
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
        pub received_at: DateTimeUtc,
        /// When the `billing.process_event` job finished it.
        pub processed_at: Option<DateTimeUtc>,
        /// Processing attempts so far.
        pub attempts: i32,
        /// The error of the last failed attempt.
        pub error: Option<String>,
    }

    /// No relations.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
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
    use navree_access::Subject;

    use super::*;

    #[test]
    fn subscriptions_are_an_access_subject() {
        assert_eq!(<subscription::Entity as Subject>::SUBJECT, "Subscription");
        assert_eq!(SUBSCRIPTION_SUBJECT, "Subscription");
        assert_eq!(
            <subscription::Entity as Subject>::PERMISSION_PREFIX,
            "subscriptions"
        );
    }
}
