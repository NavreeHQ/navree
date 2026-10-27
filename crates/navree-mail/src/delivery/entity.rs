//! The mail tables as SeaORM entities (migration set `mail`).
//!
//! - `mail_transports(id, tier, workspace_id, organization_id, domain,
//!   kind, config, enabled, checked_at, created_at, updated_at)`: tenant
//!   transports, `config` encrypted with the app key.
//! - `mail_deliveries(id, workspace_id, organization_id, status, transport,
//!   tier, provider_id, template, subject, sender, recipient_hash,
//!   recipients, error, opened_at, clicked_at, created_at, updated_at)`:
//!   one row per message sent.
//! - `mail_events(id, delivery_id → mail_deliveries ON DELETE CASCADE,
//!   workspace_id, provider, kind, recipient_hash, bounce_type, url, detail,
//!   occurred_at, created_at)`: what providers and tracking reported.
//! - `mail_suppressions(id, workspace_id, email, reason, detail,
//!   delivery_id, created_at)`: addresses mail is not sent to.
//! - `mail_outbox(id, workspace_id, organization_id, reason, status,
//!   subject, recipients, message, via, template, delivery_id, decided_by,
//!   decided_at, created_at, updated_at)`: mail held for approval,
//!   `message` encrypted with the app key.
//!
//! Change them through [`TransportResolver`](crate::TransportResolver),
//! [`Suppressions`](crate::Suppressions), [`Outbox`](crate::Outbox) and
//! [`Deliveries`](crate::Deliveries); read them freely.

/// `mail_transports`: transports stored for a domain, a workspace, an
/// organization or the platform.
pub mod transport {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    /// A stored transport.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "mail_transports")]
    pub struct Model {
        /// The id (UUID v7).
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// `domain`, `workspace`, `organization` or `platform`.
        pub tier: String,
        /// The owning workspace (`workspace` tier, or a workspace's domain).
        pub workspace_id: Option<Uuid>,
        /// The owning organization (`organization` tier, or an
        /// organization's domain).
        pub organization_id: Option<Uuid>,
        /// The sending domain (`domain` tier), lower case.
        pub domain: Option<String>,
        /// The transport type (`smtp`, `ses`, ...).
        pub kind: String,
        /// The settings as JSON, encrypted with the app key. Never
        /// serialized.
        #[serde(skip_serializing)]
        #[sea_orm(column_type = "Text")]
        pub config: String,
        /// Whether it is used.
        pub enabled: bool,
        /// When its `usable` check last passed.
        pub checked_at: Option<DateTimeUtc>,
        /// When it was created.
        pub created_at: DateTimeUtc,
        /// When it last changed.
        pub updated_at: DateTimeUtc,
    }

    /// No relations.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `mail_deliveries`: the delivery log.
pub mod delivery {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    /// A delivery.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "mail_deliveries")]
    pub struct Model {
        /// The id (UUID v7).
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// The workspace it was sent for.
        pub workspace_id: Option<Uuid>,
        /// The organization it was sent for.
        pub organization_id: Option<Uuid>,
        /// `sending`, `sent`, `delivered`, `bounced`, `complained`,
        /// `failed`, `suppressed`, `held` or `discarded`.
        pub status: String,
        /// The transport (`ses`, `smtp`, ...).
        pub transport: String,
        /// Where the transport came from (`workspace`, `config`, ...).
        pub tier: String,
        /// The provider's message id.
        pub provider_id: Option<String>,
        /// The template it was rendered from.
        pub template: Option<String>,
        /// The subject line.
        #[sea_orm(column_type = "Text")]
        pub subject: String,
        /// The sender's address.
        pub sender: String,
        /// SHA-256 (hex) of the first recipient's address, lower-cased.
        pub recipient_hash: String,
        /// How many recipients it had.
        pub recipients: i32,
        /// Why the transport refused it.
        #[sea_orm(column_type = "Text", nullable)]
        pub error: Option<String>,
        /// When it was first opened (tracking).
        pub opened_at: Option<DateTimeUtc>,
        /// When a link was first clicked (tracking).
        pub clicked_at: Option<DateTimeUtc>,
        /// When it was handed to the transport.
        pub created_at: DateTimeUtc,
        /// When it last changed.
        pub updated_at: DateTimeUtc,
    }

    /// A delivery has many events.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        /// `mail_events.delivery_id`.
        #[sea_orm(has_many = "super::event::Entity")]
        Events,
    }

    impl Related<super::event::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Events.def()
        }
    }

    impl ActiveModelBehavior for ActiveModel {}
}

/// `mail_events`: what happened to a delivery.
pub mod event {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    /// An event.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "mail_events")]
    pub struct Model {
        /// The id (UUID v7).
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// The delivery, when it is known.
        pub delivery_id: Option<Uuid>,
        /// The delivery's workspace.
        pub workspace_id: Option<Uuid>,
        /// Who reported it: `ses`, `mailgun`, `postmark`, `resend`,
        /// `tracking`, `fake`.
        pub provider: String,
        /// `delivered`, `bounced`, `deferred`, `complained`, `opened` or
        /// `clicked`.
        pub kind: String,
        /// SHA-256 (hex) of the recipient's address, lower-cased.
        pub recipient_hash: Option<String>,
        /// `hard` or `soft`, for bounces.
        pub bounce_type: Option<String>,
        /// The link, for clicks.
        #[sea_orm(column_type = "Text", nullable)]
        pub url: Option<String>,
        /// The provider's diagnostic.
        #[sea_orm(column_type = "Text", nullable)]
        pub detail: Option<String>,
        /// When it happened, by the provider's clock.
        pub occurred_at: DateTimeUtc,
        /// When it was recorded.
        pub created_at: DateTimeUtc,
    }

    /// An event belongs to a delivery.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        /// `mail_deliveries.id`.
        #[sea_orm(
            belongs_to = "super::delivery::Entity",
            from = "Column::DeliveryId",
            to = "super::delivery::Column::Id",
            on_delete = "Cascade"
        )]
        Delivery,
    }

    impl Related<super::delivery::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Delivery.def()
        }
    }

    impl ActiveModelBehavior for ActiveModel {}
}

/// `mail_suppressions`: addresses mail is not sent to.
pub mod suppression {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    /// A suppressed address.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "mail_suppressions")]
    pub struct Model {
        /// The id (UUID v7).
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// The workspace it applies to; `None` for every workspace.
        pub workspace_id: Option<Uuid>,
        /// The address, lower case.
        pub email: String,
        /// `hard_bounce`, `complaint`, `unsubscribed` or `manual`.
        pub reason: String,
        /// The provider's diagnostic or an admin's note.
        #[sea_orm(column_type = "Text", nullable)]
        pub detail: Option<String>,
        /// The delivery that caused it.
        pub delivery_id: Option<Uuid>,
        /// When it was added.
        pub created_at: DateTimeUtc,
    }

    /// No relations.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `mail_outbox`: mail held for approval.
pub mod outbox {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    /// A held message.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "mail_outbox")]
    pub struct Model {
        /// The id (UUID v7).
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// The workspace it was sent for.
        pub workspace_id: Option<Uuid>,
        /// The organization it was sent for.
        pub organization_id: Option<Uuid>,
        /// Why it is held.
        #[sea_orm(column_type = "Text")]
        pub reason: String,
        /// `held`, `released` or `discarded`.
        pub status: String,
        /// The subject line.
        #[sea_orm(column_type = "Text")]
        pub subject: String,
        /// The recipients (a JSON array of addresses).
        pub recipients: Json,
        /// The rendered message as JSON, encrypted with the app key. Never
        /// serialized.
        #[serde(skip_serializing)]
        #[sea_orm(column_type = "Text")]
        pub message: String,
        /// The named transport it goes through.
        pub via: Option<String>,
        /// The template it was rendered from.
        pub template: Option<String>,
        /// The delivery, once released.
        pub delivery_id: Option<Uuid>,
        /// Who released or discarded it.
        pub decided_by: Option<Uuid>,
        /// When.
        pub decided_at: Option<DateTimeUtc>,
        /// When it was held.
        pub created_at: DateTimeUtc,
        /// When it last changed.
        pub updated_at: DateTimeUtc,
    }

    /// No relations.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
