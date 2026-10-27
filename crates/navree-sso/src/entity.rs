//! The module's tables as SeaORM entities.
//!
//! - `sso_connections(id, owner_type, owner_id, slug UNIQUE, name,
//!   protocol, enabled, enforce, settings, provisioning, created_at,
//!   updated_at)`: `settings` and `provisioning` are JSON; an OpenID
//!   Connect client secret is a navree-crypto ciphertext inside `settings`.
//! - `sso_domains(id, connection_id → sso_connections ON DELETE CASCADE,
//!   domain, token, verified_at, created_at, updated_at)`: unique per
//!   (connection_id, domain).
//! - `sso_identities(id, connection_id → sso_connections ON DELETE CASCADE,
//!   user_id → users ON DELETE CASCADE, subject, email, attributes,
//!   last_login_at, created_at, updated_at)`: unique per (connection_id,
//!   subject).
//!
//! Changes go through `navree_db::Repo`, so audit entries carry the rows:
//! client secrets only as ciphertext.

/// `sso_connections`.
pub mod sso_connection {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    /// A connection to an identity provider.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "sso_connections")]
    pub struct Model {
        /// The id (UUID v7).
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// `app`, `organization` or `workspace`.
        pub owner_type: String,
        /// The organization or workspace (none for `app`).
        pub owner_id: Option<Uuid>,
        /// The URL segment.
        pub slug: String,
        /// The display name.
        pub name: String,
        /// `saml` or `oidc`.
        pub protocol: String,
        /// Whether people can sign in with it.
        pub enabled: bool,
        /// Refuse passwords for its verified domains (`None`: the app's
        /// default).
        pub enforce: Option<bool>,
        /// The identity provider settings (JSON; the client secret
        /// encrypted).
        #[sea_orm(column_type = "Text")]
        pub settings: String,
        /// The provisioning rules (JSON).
        #[sea_orm(column_type = "Text")]
        pub provisioning: String,
        /// When it was created.
        pub created_at: DateTimeUtc,
        /// When it last changed.
        pub updated_at: DateTimeUtc,
    }

    /// No relations are declared.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `sso_domains`.
pub mod sso_domain {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    /// An email domain of a connection.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "sso_domains")]
    pub struct Model {
        /// The id (UUID v7).
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// The connection.
        pub connection_id: Uuid,
        /// The domain, lower-cased.
        pub domain: String,
        /// The verification token (the TXT record's value after the
        /// prefix).
        pub token: String,
        /// When the TXT record was found.
        pub verified_at: Option<DateTimeUtc>,
        /// When it was added.
        pub created_at: DateTimeUtc,
        /// When it last changed.
        pub updated_at: DateTimeUtc,
    }

    /// No relations are declared.
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

/// `sso_identities`.
pub mod sso_identity {
    use sea_orm::entity::prelude::*;
    use serde::Serialize;

    /// A person at an identity provider, linked to a user.
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
    #[sea_orm(table_name = "sso_identities")]
    pub struct Model {
        /// The id (UUID v7).
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        /// The connection.
        pub connection_id: Uuid,
        /// The user.
        pub user_id: Uuid,
        /// The SAML name id or the OpenID Connect `sub`.
        pub subject: String,
        /// The email the identity provider sent last.
        pub email: Option<String>,
        /// The attributes or claims of the last sign-in (JSON).
        #[sea_orm(column_type = "Text")]
        pub attributes: String,
        /// The last sign-in.
        pub last_login_at: DateTimeUtc,
        /// When it was linked.
        pub created_at: DateTimeUtc,
        /// When it last changed.
        pub updated_at: DateTimeUtc,
    }

    /// No relations are declared (the user is navree-auth's).
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
