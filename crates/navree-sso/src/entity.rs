//! The module's tables, as navree-orm models.
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
//! The module writes them through its `store` functions: ORM statements in
//! the unit of work that record change events, so audit trails carry the
//! rows. A connection's `settings` hold the OpenID Connect client secret
//! (encrypted) and the identity provider's metadata document (with its
//! certificates); the field is never serialized, and a connection's change
//! events carry a summary of the settings instead (the protocol, the URLs,
//! the client id and scopes, whether a client secret and a metadata
//! document are set, and a digest of the document). No secret, certificate
//! or key reaches an observer or an audit trail.

use std::fmt;

use serde_json::{Value, json};

use crate::SsoError;

/// A table of the module, as its change events show it.
pub(crate) trait Recorded: navree_orm::Model<Key = uuid::Uuid> + Clone + Sync {
    /// The table.
    const TABLE: &'static str;

    /// The row's id.
    fn id(&self) -> uuid::Uuid;

    /// The row as a change event carries it: its `Serialize` form (which
    /// leaves secrets out), plus summaries of what is skipped.
    fn event_form(&self) -> Result<Value, SsoError>;
}

fn serialized<T: serde::Serialize>(row: &T) -> Result<Value, SsoError> {
    serde_json::to_value(row).map_err(|error| SsoError::Config(error.to_string()))
}

/// `sso_connections`.
pub mod sso_connection {
    use chrono::{DateTime, Utc};
    use serde::Serialize;
    use uuid::Uuid;

    /// A connection to an identity provider.
    ///
    /// `settings` (JSON, with the client secret encrypted) is never
    /// serialized and never printed: it holds secrets and certificates.
    #[derive(Clone, PartialEq, Eq, navree_orm::Model, Serialize)]
    #[model(table = "sso_connections")]
    pub struct Model {
        /// The id (UUID v7).
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
        /// encrypted). Never serialized.
        #[serde(skip_serializing)]
        pub settings: String,
        /// The provisioning rules (JSON).
        pub provisioning: String,
        /// When it was created.
        pub created_at: DateTime<Utc>,
        /// When it last changed.
        pub updated_at: DateTime<Utc>,
    }
}

impl fmt::Debug for sso_connection::Model {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SsoConnectionRow")
            .field("id", &self.id)
            .field("owner_type", &self.owner_type)
            .field("owner_id", &self.owner_id)
            .field("slug", &self.slug)
            .field("name", &self.name)
            .field("protocol", &self.protocol)
            .field("enabled", &self.enabled)
            .field("enforce", &self.enforce)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish_non_exhaustive()
    }
}

impl Recorded for sso_connection::Model {
    const TABLE: &'static str = "sso_connections";

    fn id(&self) -> uuid::Uuid {
        self.id
    }

    fn event_form(&self) -> Result<Value, SsoError> {
        let mut form = serialized(self)?;
        if let Value::Object(fields) = &mut form {
            fields.insert("settings".to_owned(), settings_summary(&self.settings));
        }
        Ok(form)
    }
}

/// What a connection's stored settings show in a change event: only the
/// fields listed here are copied. The client secret and the metadata
/// document (certificates) are reduced to whether they are set and, for the
/// document, a digest, so a change to either still shows.
pub(crate) fn settings_summary(stored: &str) -> Value {
    let Ok(Value::Object(settings)) = serde_json::from_str::<Value>(stored) else {
        return Value::Null;
    };
    let mut summary = serde_json::Map::new();
    for field in [
        "protocol",
        "issuer",
        "client_id",
        "scopes",
        "metadata_url",
        "name_id_format",
        "force_authn",
        "require_signed_assertion",
    ] {
        if let Some(value) = settings.get(field) {
            summary.insert(field.to_owned(), value.clone());
        }
    }
    if let Some(secret) = settings.get("client_secret") {
        summary.insert("has_client_secret".to_owned(), json!(!secret.is_null()));
    }
    if let Some(Value::String(xml)) = settings.get("metadata_xml") {
        summary.insert("has_metadata_xml".to_owned(), json!(true));
        summary.insert(
            "metadata_xml_sha256".to_owned(),
            json!(navree_crypto::sha256_hex(xml.as_bytes())),
        );
    }
    Value::Object(summary)
}

/// `sso_domains`.
pub mod sso_domain {
    use chrono::{DateTime, Utc};
    use serde::Serialize;
    use uuid::Uuid;

    /// An email domain of a connection.
    #[derive(Clone, Debug, PartialEq, Eq, navree_orm::Model, Serialize)]
    #[model(table = "sso_domains")]
    pub struct Model {
        /// The id (UUID v7).
        pub id: Uuid,
        /// The connection.
        pub connection_id: Uuid,
        /// The domain, lower-cased.
        pub domain: String,
        /// The verification token (the TXT record's value after the
        /// prefix).
        pub token: String,
        /// When the TXT record was found.
        pub verified_at: Option<DateTime<Utc>>,
        /// When it was added.
        pub created_at: DateTime<Utc>,
        /// When it last changed.
        pub updated_at: DateTime<Utc>,
    }
}

impl Recorded for sso_domain::Model {
    const TABLE: &'static str = "sso_domains";

    fn id(&self) -> uuid::Uuid {
        self.id
    }

    fn event_form(&self) -> Result<Value, SsoError> {
        serialized(self)
    }
}

/// `sso_identities`.
pub mod sso_identity {
    use chrono::{DateTime, Utc};
    use serde::Serialize;
    use uuid::Uuid;

    /// A person at an identity provider, linked to a user.
    #[derive(Clone, Debug, PartialEq, Eq, navree_orm::Model, Serialize)]
    #[model(table = "sso_identities")]
    pub struct Model {
        /// The id (UUID v7).
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
        pub attributes: String,
        /// The last sign-in.
        pub last_login_at: DateTime<Utc>,
        /// When it was linked.
        pub created_at: DateTime<Utc>,
        /// When it last changed.
        pub updated_at: DateTime<Utc>,
    }
}

impl Recorded for sso_identity::Model {
    const TABLE: &'static str = "sso_identities";

    fn id(&self) -> uuid::Uuid {
        self.id
    }

    fn event_form(&self) -> Result<Value, SsoError> {
        serialized(self)
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use uuid::Uuid;

    use super::*;

    fn connection(settings: Value) -> sso_connection::Model {
        let now = Utc::now();
        sso_connection::Model {
            id: Uuid::now_v7(),
            owner_type: "app".into(),
            owner_id: None,
            slug: "acme".into(),
            name: "Acme".into(),
            protocol: "oidc".into(),
            enabled: true,
            enforce: None,
            settings: settings.to_string(),
            provisioning: "{}".into(),
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn a_connections_event_form_leaves_secrets_and_certificates_out() {
        let oidc = connection(json!({
            "protocol": "oidc",
            "issuer": "https://login.acme.test",
            "client_id": "c1",
            "client_secret": "CIPHERTEXT-OF-THE-SECRET",
            "scopes": ["openid"],
        }));
        let form = oidc.event_form().unwrap().to_string();
        assert!(!form.contains("CIPHERTEXT-OF-THE-SECRET"), "{form}");
        assert!(form.contains("\"has_client_secret\":true"), "{form}");
        assert!(form.contains("\"client_id\":\"c1\""), "{form}");

        let saml = connection(json!({
            "protocol": "saml",
            "metadata_xml": "<EntityDescriptor>MIICERTIFICATE</EntityDescriptor>",
        }));
        let form = saml.event_form().unwrap().to_string();
        assert!(!form.contains("MIICERTIFICATE"), "{form}");
        assert!(form.contains("\"has_metadata_xml\":true"), "{form}");
        assert!(form.contains("metadata_xml_sha256"), "{form}");
    }

    #[test]
    fn a_connections_row_is_never_serialized_or_printed_with_its_settings() {
        let row = connection(json!({ "protocol": "oidc", "client_secret": "CIPHERTEXT" }));
        assert!(!serde_json::to_string(&row).unwrap().contains("CIPHERTEXT"));
        assert!(!format!("{row:?}").contains("CIPHERTEXT"));
    }

    #[test]
    fn unknown_settings_fields_are_not_copied() {
        let summary = settings_summary(r#"{"protocol":"saml","private_key":"KEY","extra":1}"#);
        assert_eq!(summary, json!({ "protocol": "saml" }));
        assert_eq!(settings_summary("not json"), Value::Null);
    }
}
