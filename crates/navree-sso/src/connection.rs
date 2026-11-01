//! Connections: who owns them, how they talk to the identity provider and
//! how signed-in people become users and members.

use chrono::{DateTime, Utc};
use navree_openapi::ToSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Who a connection belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", content = "id", rename_all = "snake_case")]
pub enum SsoOwner {
    /// The whole app (a single-tenant app's company directory).
    App,
    /// An organization (navree-workspace): members join it, reaching all
    /// its workspaces.
    Organization(Uuid),
    /// A workspace (navree-workspace): members join it.
    Workspace(Uuid),
}

impl SsoOwner {
    /// `app`, `organization` or `workspace`.
    pub fn kind(&self) -> &'static str {
        match self {
            SsoOwner::App => "app",
            SsoOwner::Organization(_) => "organization",
            SsoOwner::Workspace(_) => "workspace",
        }
    }

    /// The organization's or workspace's id.
    pub fn id(&self) -> Option<Uuid> {
        match self {
            SsoOwner::App => None,
            SsoOwner::Organization(id) | SsoOwner::Workspace(id) => Some(*id),
        }
    }

    /// The owner stored as `(kind, id)`.
    pub(crate) fn from_parts(kind: &str, id: Option<Uuid>) -> Option<SsoOwner> {
        match (kind, id) {
            ("app", None) => Some(SsoOwner::App),
            ("organization", Some(id)) => Some(SsoOwner::Organization(id)),
            ("workspace", Some(id)) => Some(SsoOwner::Workspace(id)),
            _ => None,
        }
    }
}

/// The protocol of a connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    /// SAML 2.0.
    Saml,
    /// OpenID Connect.
    Oidc,
}

impl Protocol {
    /// `saml` or `oidc`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Protocol::Saml => "saml",
            Protocol::Oidc => "oidc",
        }
    }
}

/// How a connection reaches its identity provider.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "protocol", rename_all = "snake_case")]
pub enum ConnectionSettings {
    /// SAML 2.0: the identity provider's metadata, by URL or pasted.
    Saml(SamlSettings),
    /// OpenID Connect: the issuer (discovery) and the client.
    Oidc(OidcSettings),
}

impl ConnectionSettings {
    /// The protocol.
    pub fn protocol(&self) -> Protocol {
        match self {
            ConnectionSettings::Saml(_) => Protocol::Saml,
            ConnectionSettings::Oidc(_) => Protocol::Oidc,
        }
    }

    /// The settings without secrets (what the API returns).
    pub(crate) fn redacted(&self) -> ConnectionSettings {
        match self {
            ConnectionSettings::Saml(saml) => ConnectionSettings::Saml(saml.clone()),
            ConnectionSettings::Oidc(oidc) => ConnectionSettings::Oidc(OidcSettings {
                client_secret: None,
                ..oidc.clone()
            }),
        }
    }
}

/// A SAML connection's settings. Give `metadata_url` (fetched over HTTPS
/// and refreshed every `sso.saml.metadata_ttl_secs`) or `metadata_xml`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct SamlSettings {
    /// The identity provider's metadata URL.
    pub metadata_url: Option<String>,
    /// The identity provider's metadata document.
    pub metadata_xml: Option<String>,
    /// The name id format to ask for (the identity provider's default when
    /// unset). Prefer `persistent` or `emailAddress`: a `transient` name id
    /// changes at every sign-in.
    pub name_id_format: Option<String>,
    /// Ask the identity provider to authenticate again even with a session.
    pub force_authn: bool,
    /// Refuse responses whose assertion is not itself signed.
    pub require_signed_assertion: bool,
}

/// An OpenID Connect connection's settings.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct OidcSettings {
    /// The issuer; discovery reads `{issuer}/.well-known/openid-configuration`.
    pub issuer: String,
    /// The client id.
    pub client_id: String,
    /// The client secret: stored encrypted with the app key, never
    /// returned.
    #[serde(default, skip_serializing)]
    pub client_secret: Option<String>,
    /// The scopes to ask for (`sso.oidc.scopes` when empty).
    #[serde(default)]
    pub scopes: Vec<String>,
}

/// How people who sign in become users and members.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct Provisioning {
    /// Create users at their first sign-in (just-in-time provisioning).
    /// Default `true`.
    pub jit: bool,
    /// The role of members whose groups match no rule (`member` by
    /// default); `null` refuses them ([`SsoError::NotAssigned`](crate::SsoError::NotAssigned)).
    pub default_role: Option<String>,
    /// Group to role rules, the first matching one wins.
    pub roles: Vec<RoleRule>,
    /// Update a member's role from their groups at every sign-in (the
    /// owner's role never changes). Default `true`.
    pub sync_roles: bool,
    /// Create users whose email domain is not a verified domain of the
    /// connection. Such emails never link to an existing user. Default
    /// `false`.
    pub allow_unverified_emails: bool,
    /// Where the profile is read from.
    pub attributes: AttributeMapping,
}

impl Default for Provisioning {
    fn default() -> Self {
        Provisioning {
            jit: true,
            default_role: Some("member".to_owned()),
            roles: Vec::new(),
            sync_roles: true,
            allow_unverified_emails: false,
            attributes: AttributeMapping::default(),
        }
    }
}

impl Provisioning {
    /// The role for `groups`: the first rule whose group the user has,
    /// else the default role.
    pub fn role_for(&self, groups: &[String]) -> Option<String> {
        self.roles
            .iter()
            .find(|rule| groups.iter().any(|g| g.eq_ignore_ascii_case(&rule.group)))
            .map(|rule| rule.role.clone())
            .or_else(|| self.default_role.clone())
    }
}

/// A group to role rule.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RoleRule {
    /// The group, as the identity provider names it (compared ignoring
    /// ASCII case). Entra ID sends group object ids.
    pub group: String,
    /// The workspace or organization role.
    pub role: String,
}

/// The SAML attribute names or OpenID Connect claims each profile field is
/// read from, the first present one winning. The defaults cover Okta,
/// Entra ID, Google Workspace, AD FS and the standard OpenID Connect
/// claims.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct AttributeMapping {
    /// The email address (a SAML name id that is an email address is used
    /// when none is present).
    pub email: Vec<String>,
    /// The display name.
    pub name: Vec<String>,
    /// The given name (joined with the family name when there is no
    /// display name).
    pub first_name: Vec<String>,
    /// The family name.
    pub last_name: Vec<String>,
    /// The groups (role mapping).
    pub groups: Vec<String>,
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

impl Default for AttributeMapping {
    fn default() -> Self {
        AttributeMapping {
            email: names(&[
                "email",
                "mail",
                "Email",
                "emailAddress",
                "http://schemas.xmlsoap.org/ws/2005/05/identity/claims/emailaddress",
                "urn:oid:0.9.2342.19200300.100.1.3",
            ]),
            name: names(&[
                "name",
                "displayName",
                "http://schemas.microsoft.com/identity/claims/displayname",
                "http://schemas.xmlsoap.org/ws/2005/05/identity/claims/name",
                "urn:oid:2.16.840.1.113730.3.1.241",
            ]),
            first_name: names(&[
                "given_name",
                "firstName",
                "first_name",
                "givenName",
                "http://schemas.xmlsoap.org/ws/2005/05/identity/claims/givenname",
                "urn:oid:2.5.4.42",
            ]),
            last_name: names(&[
                "family_name",
                "lastName",
                "last_name",
                "surname",
                "sn",
                "http://schemas.xmlsoap.org/ws/2005/05/identity/claims/surname",
                "urn:oid:2.5.4.4",
            ]),
            groups: names(&[
                "groups",
                "memberOf",
                "http://schemas.microsoft.com/ws/2008/06/identity/claims/groups",
                "Group",
            ]),
        }
    }
}

/// A connection, as the service and the routes return it (secrets left
/// out).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct SsoConnection {
    /// Always `"SsoConnection"`.
    #[serde(rename = "__type")]
    #[schema(example = "SsoConnection")]
    pub resource_type: &'static str,
    /// The id.
    pub id: Uuid,
    /// Who owns it.
    pub owner: SsoOwner,
    /// The URL segment (`/api/sso/{slug}/login`): lower-case letters,
    /// digits and dashes.
    pub slug: String,
    /// The display name (`Acme Okta`).
    pub name: String,
    /// The protocol.
    pub protocol: Protocol,
    /// Whether people can sign in with it.
    pub enabled: bool,
    /// Refuse passwords for its verified domains: `null` follows
    /// `sso.enforce_for_verified_domains`.
    pub enforce: Option<bool>,
    /// The identity provider settings (without the client secret).
    pub settings: ConnectionSettings,
    /// Whether an OpenID Connect client secret is stored.
    pub has_client_secret: bool,
    /// Provisioning rules.
    pub provisioning: Provisioning,
    /// What to give the identity provider: the service provider's entity
    /// id, assertion consumer service and metadata URLs (SAML), or the
    /// redirect URI (OpenID Connect).
    pub service_provider: ServiceProviderInfo,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

/// The URLs to give an identity provider for a connection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct ServiceProviderInfo {
    /// The sign-in URL to send people to.
    pub login_url: String,
    /// SAML: the entity id (audience).
    pub entity_id: Option<String>,
    /// SAML: the assertion consumer service URL (HTTP-POST).
    pub acs_url: Option<String>,
    /// SAML: the metadata URL.
    pub metadata_url: Option<String>,
    /// OpenID Connect: the redirect URI.
    pub redirect_uri: Option<String>,
}

/// A new connection for [`Sso::create_connection`](crate::Sso::create_connection).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, ToSchema)]
pub struct NewConnection {
    /// Who owns it.
    pub owner: SsoOwner,
    /// The display name.
    pub name: String,
    /// The URL segment; derived from the name when absent.
    #[serde(default)]
    pub slug: Option<String>,
    /// The identity provider settings.
    pub settings: ConnectionSettings,
    /// Provisioning rules.
    #[serde(default)]
    pub provisioning: Provisioning,
    /// Refuse passwords for its verified domains (`null`: the app's
    /// default).
    #[serde(default)]
    pub enforce: Option<bool>,
    /// Whether people can sign in with it (default `true`).
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
}

fn enabled_by_default() -> bool {
    true
}

impl NewConnection {
    /// A SAML connection with metadata XML.
    pub fn saml(owner: SsoOwner, name: impl Into<String>, metadata_xml: impl Into<String>) -> Self {
        NewConnection {
            owner,
            name: name.into(),
            slug: None,
            settings: ConnectionSettings::Saml(SamlSettings {
                metadata_xml: Some(metadata_xml.into()),
                ..SamlSettings::default()
            }),
            provisioning: Provisioning::default(),
            enforce: None,
            enabled: true,
        }
    }

    /// An OpenID Connect connection.
    pub fn oidc(
        owner: SsoOwner,
        name: impl Into<String>,
        issuer: impl Into<String>,
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
    ) -> Self {
        NewConnection {
            owner,
            name: name.into(),
            slug: None,
            settings: ConnectionSettings::Oidc(OidcSettings {
                issuer: issuer.into(),
                client_id: client_id.into(),
                client_secret: Some(client_secret.into()),
                scopes: Vec::new(),
            }),
            provisioning: Provisioning::default(),
            enforce: None,
            enabled: true,
        }
    }

    /// With this slug.
    #[must_use]
    pub fn slug(mut self, slug: impl Into<String>) -> Self {
        self.slug = Some(slug.into());
        self
    }

    /// With these provisioning rules.
    #[must_use]
    pub fn provisioning(mut self, provisioning: Provisioning) -> Self {
        self.provisioning = provisioning;
        self
    }

    /// Refusing passwords for its verified domains (or not).
    #[must_use]
    pub fn enforce(mut self, enforce: bool) -> Self {
        self.enforce = Some(enforce);
        self
    }
}

/// Changes to a connection for [`Sso::update_connection`](crate::Sso::update_connection);
/// absent fields stay. A new OpenID Connect `client_secret` replaces the
/// stored one; leaving it out keeps it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(default)]
pub struct ConnectionUpdate {
    /// The display name.
    pub name: Option<String>,
    /// The URL segment.
    pub slug: Option<String>,
    /// The identity provider settings (same protocol).
    pub settings: Option<ConnectionSettings>,
    /// Provisioning rules.
    pub provisioning: Option<Provisioning>,
    /// Whether people can sign in with it.
    pub enabled: Option<bool>,
    /// Refuse passwords for its verified domains; `null` is not a change,
    /// use [`follow_default_enforcement`](ConnectionUpdate::follow_default_enforcement).
    pub enforce: Option<bool>,
    /// Follow `sso.enforce_for_verified_domains` again.
    pub follow_default_enforcement: bool,
}

/// A verified (or pending) email domain of a connection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct SsoDomain {
    /// Always `"SsoDomain"`.
    #[serde(rename = "__type")]
    #[schema(example = "SsoDomain")]
    pub resource_type: &'static str,
    /// The id.
    pub id: Uuid,
    /// The connection.
    pub connection_id: Uuid,
    /// The domain (`acme.com`).
    pub domain: String,
    /// Whether the TXT record was found (or an operator verified it).
    pub verified: bool,
    /// When it was verified.
    pub verified_at: Option<DateTime<Utc>>,
    /// The TXT record to create: its name (the domain itself)...
    pub txt_name: String,
    /// ...and its value.
    pub txt_value: String,
    /// When it was added.
    pub created_at: DateTime<Utc>,
}

/// What `POST /sso/discover` answers for an email.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct SsoDiscovery {
    /// Whether the email's domain signs in with single sign-on.
    pub sso: bool,
    /// The connection's slug.
    pub connection: Option<String>,
    /// The connection's display name.
    pub name: Option<String>,
    /// The connection's protocol.
    pub protocol: Option<Protocol>,
    /// The login route to send the browser to.
    pub login_url: Option<String>,
    /// Whether the email may still use a password.
    pub password_allowed: bool,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn owners_serialize_with_their_kind() {
        let id = Uuid::nil();
        assert_eq!(
            serde_json::to_value(SsoOwner::Workspace(id)).unwrap(),
            json!({ "type": "workspace", "id": id })
        );
        assert_eq!(
            serde_json::to_value(SsoOwner::App).unwrap(),
            json!({ "type": "app" })
        );
        assert_eq!(
            SsoOwner::from_parts("organization", Some(id)),
            Some(SsoOwner::Organization(id))
        );
        assert_eq!(SsoOwner::from_parts("app", Some(id)), None);
    }

    #[test]
    fn settings_are_tagged_by_protocol_and_hide_the_secret() {
        let settings: ConnectionSettings = serde_json::from_value(json!({
            "protocol": "oidc",
            "issuer": "https://login.acme.test",
            "client_id": "app",
            "client_secret": "s3cret",
        }))
        .unwrap();
        assert_eq!(settings.protocol(), Protocol::Oidc);
        let shown = serde_json::to_value(&settings).unwrap();
        assert!(shown.get("client_secret").is_none(), "{shown}");
        let saml: ConnectionSettings = serde_json::from_value(json!({
            "protocol": "saml",
            "metadata_url": "https://idp.acme.test/metadata",
        }))
        .unwrap();
        assert_eq!(saml.protocol(), Protocol::Saml);
    }

    #[test]
    fn roles_come_from_the_first_matching_rule() {
        let provisioning = Provisioning {
            roles: vec![
                RoleRule {
                    group: "Acme Admins".to_owned(),
                    role: "admin".to_owned(),
                },
                RoleRule {
                    group: "Engineering".to_owned(),
                    role: "member".to_owned(),
                },
            ],
            default_role: Some("viewer".to_owned()),
            ..Provisioning::default()
        };
        let groups = |g: &[&str]| g.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            provisioning.role_for(&groups(&["engineering", "acme admins"])),
            Some("admin".to_owned())
        );
        assert_eq!(
            provisioning.role_for(&groups(&["Sales"])),
            Some("viewer".to_owned())
        );
        let strict = Provisioning {
            default_role: None,
            ..provisioning
        };
        assert_eq!(strict.role_for(&groups(&["Sales"])), None);
    }
}
