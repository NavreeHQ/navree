//! [`WorkspaceConfig`]: the `workspace` configuration section.

use std::collections::BTreeMap;
use std::fmt;

use navree_config::ConfigSection;
use serde::de::{self, Deserializer, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};

use crate::role::{check_permission, check_role_name};

/// Where the workspace of a request comes from. The resolvers run in the
/// order of [`WorkspaceConfig::resolvers`]; the first that finds an
/// identifier (a slug or a workspace id) wins.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolverKind {
    /// A request header ([`WorkspaceConfig::header`], `X-Workspace: acme`).
    Header,
    /// The subdomain under [`WorkspaceConfig::base_domain`]
    /// (`acme.app.example.com`). Finds nothing while `base_domain` is unset.
    Subdomain,
    /// The path segment after [`WorkspaceConfig::path_prefix`]
    /// (`/api/w/acme/deals`).
    Path,
    /// A claim of the request's bearer JWT ([`WorkspaceConfig::claim`]).
    Claim,
}

impl ResolverKind {
    /// `header`, `subdomain`, `path` or `claim`.
    pub fn as_str(self) -> &'static str {
        match self {
            ResolverKind::Header => "header",
            ResolverKind::Subdomain => "subdomain",
            ResolverKind::Path => "path",
            ResolverKind::Claim => "claim",
        }
    }
}

impl fmt::Display for ResolverKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How one workspace's data is kept apart from another's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Isolation {
    /// One set of tables with a `workspace_id` column: queries are scoped
    /// by [`WorkspaceRepo`](crate::WorkspaceRepo), and on PostgreSQL every
    /// workspace request sets `navree.workspace` for its transaction, so
    /// row-level security policies ([`rls_policy_sql`](crate::rls_policy_sql))
    /// enforce the scope in the database.
    #[default]
    Row,
    /// One PostgreSQL schema per workspace: each workspace request's
    /// transaction sets `search_path` to the workspace's schema.
    Schema,
    /// One database per workspace ([`WorkspaceConfig::database_url`]);
    /// [`WorkspaceUow`](crate::WorkspaceUow) is a unit of work on it.
    Database,
}

impl Isolation {
    /// `row`, `schema` or `database`.
    pub fn as_str(self) -> &'static str {
        match self {
            Isolation::Row => "row",
            Isolation::Schema => "schema",
            Isolation::Database => "database",
        }
    }
}

/// The permissions of a workspace role, set under `workspace.roles.<name>`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct RoleOptions {
    /// Permission strings (`deals.*`, `invoices.read`) the role grants
    /// inside a workspace, on top of the built-in ones of `owner`, `admin`,
    /// `member` and `viewer`. A list, or a comma-separated string (from an
    /// environment variable).
    #[serde(deserialize_with = "list_or_csv")]
    pub permissions: Vec<String>,
}

/// Invitation settings (`workspace.invitations`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct InvitationOptions {
    /// How long an invitation can be accepted, in seconds (default seven
    /// days).
    pub ttl_secs: u64,
    /// The front-end page the invitation mail links to, as
    /// `{url}?token=..`; the page posts the token to
    /// `POST /api/invitations/accept`. Without it the mail carries the
    /// token alone.
    pub url: Option<String>,
    /// A navree-view template pair (`{template}.html` / `.txt`) for the
    /// mail, rendered with `{workspace, email, role, inviter, token, url,
    /// expires_in_days}`; built-in text when unset.
    pub mail_template: Option<String>,
    /// Whether only the account whose email address the invitation was
    /// sent to may accept it (default true).
    pub require_matching_email: bool,
}

impl Default for InvitationOptions {
    fn default() -> Self {
        Self {
            ttl_secs: 7 * 24 * 60 * 60,
            url: None,
            mail_template: None,
            require_matching_email: true,
        }
    }
}

/// Options of the workspace module (configuration section `workspace`).
///
/// ```toml
/// [workspace]
/// resolvers = ["header", "path", "subdomain", "claim"]  # order; or "header,path"
/// header = "X-Workspace"          # X-Workspace: acme (a slug or an id)
/// # base_domain = "app.example.com"  # acme.app.example.com -> acme
/// path_prefix = "/w"              # /api/w/acme/deals -> acme
/// claim = "workspace"             # a claim of the bearer JWT
/// isolation = "row"               # row | schema | database
/// routes = true                   # /api/workspaces and /api/invitations
/// users_table = "users"           # names and emails of members; "" = none
/// schema_prefix = "workspace_"    # schema isolation: workspace_<id>
/// # database_url = "sqlite://storage/workspaces/{id}.db?mode=rwc"  # database isolation
///
/// [workspace.invitations]
/// ttl_secs = 604800               # seven days
/// # url = "https://app.example.com/invitations/accept"  # mail link {url}?token=..
/// # mail_template = "mail/workspace-invitation"
/// require_matching_email = true
///
/// [workspace.roles.member]
/// permissions = ["deals.*", "companies.read"]
///
/// [workspace.roles.billing]       # an app-defined role
/// permissions = ["invoices.*"]
/// ```
///
/// Environment variables map as usual: `APP_WORKSPACE__HEADER` →
/// `workspace.header`; lists also accept a comma-separated string
/// (`APP_WORKSPACE__RESOLVERS=header,path`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct WorkspaceConfig {
    /// The resolvers to try, in order.
    #[serde(deserialize_with = "list_or_csv")]
    pub resolvers: Vec<ResolverKind>,
    /// The header the `header` resolver reads.
    pub header: String,
    /// The domain whose subdomains name workspaces (`app.example.com`); the
    /// `subdomain` resolver finds nothing while it is unset.
    pub base_domain: Option<String>,
    /// The path segment(s) before the workspace in a URL (`/w`).
    pub path_prefix: String,
    /// The bearer JWT claim the `claim` resolver reads.
    pub claim: String,
    /// How workspaces' data is kept apart.
    pub isolation: Isolation,
    /// Whether the module mounts its routes (`/api/workspaces/...` and
    /// `/api/invitations/accept`).
    pub routes: bool,
    /// The table of user accounts (`id`, `name`, `email`; navree-auth's
    /// `users`) that member lists and invitation checks read; empty when
    /// the app has none.
    pub users_table: String,
    /// Invitations.
    pub invitations: InvitationOptions,
    /// Extra permissions per role, and app-defined roles.
    pub roles: BTreeMap<String, RoleOptions>,
    /// Schema isolation: the prefix of each workspace's schema name
    /// (`workspace_` gives `workspace_0199...`).
    pub schema_prefix: String,
    /// Database isolation: the URL of each workspace's database, with
    /// `{id}` standing for the workspace id (simple form, 32 hex digits).
    pub database_url: Option<String>,
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            resolvers: vec![
                ResolverKind::Header,
                ResolverKind::Path,
                ResolverKind::Subdomain,
                ResolverKind::Claim,
            ],
            header: "X-Workspace".to_owned(),
            base_domain: None,
            path_prefix: "/w".to_owned(),
            claim: "workspace".to_owned(),
            isolation: Isolation::Row,
            routes: true,
            users_table: "users".to_owned(),
            invitations: InvitationOptions::default(),
            roles: BTreeMap::new(),
            schema_prefix: "workspace_".to_owned(),
            database_url: None,
        }
    }
}

impl WorkspaceConfig {
    /// Every problem with the options, naming the configuration keys.
    pub fn check(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        if self.resolvers.is_empty() {
            errors.push("workspace.resolvers must name at least one resolver".to_owned());
        }
        for (index, kind) in self.resolvers.iter().enumerate() {
            if self.resolvers[..index].contains(kind) {
                errors.push(format!("workspace.resolvers lists `{kind}` twice"));
            }
        }
        if http::HeaderName::from_bytes(self.header.as_bytes()).is_err() {
            errors.push("workspace.header must be a valid header name".to_owned());
        }
        if let Some(domain) = &self.base_domain
            && !is_domain(domain)
        {
            errors.push(
                "workspace.base_domain must be a host name such as `app.example.com`".to_owned(),
            );
        }
        if self.path_segments().is_empty() || !self.path_prefix.starts_with('/') {
            errors.push("workspace.path_prefix must start with `/` and name a segment".to_owned());
        }
        if self.claim.trim().is_empty() {
            errors.push("workspace.claim must not be empty".to_owned());
        }
        let invitations = &self.invitations;
        if invitations.ttl_secs == 0 {
            errors.push("workspace.invitations.ttl_secs must be at least 1".to_owned());
        }
        if invitations
            .url
            .as_deref()
            .is_some_and(|url| !is_http_url(url))
        {
            errors.push("workspace.invitations.url must be an absolute http(s) URL".to_owned());
        }
        for (name, role) in &self.roles {
            if let Err(reason) = check_role_name(name) {
                errors.push(format!("workspace.roles: `{name}` {reason}"));
            }
            for permission in &role.permissions {
                if let Err(reason) = check_permission(permission) {
                    errors.push(format!(
                        "workspace.roles.{name}.permissions: `{permission}` {reason}"
                    ));
                }
            }
        }
        if !is_identifier(&self.schema_prefix) {
            errors.push(
                "workspace.schema_prefix must be 1 to 30 lower-case letters, digits or `_`, starting with a letter"
                    .to_owned(),
            );
        }
        match (&self.isolation, &self.database_url) {
            (Isolation::Database, None) => errors.push(
                "workspace.database_url is required with workspace.isolation = \"database\""
                    .to_owned(),
            ),
            (_, Some(url)) if !url.contains("{id}") => errors
                .push("workspace.database_url must contain `{id}` (the workspace id)".to_owned()),
            _ => {}
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// The segments of [`path_prefix`](Self::path_prefix): `/w` → `["w"]`.
    pub fn path_segments(&self) -> Vec<&str> {
        self.path_prefix
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect()
    }

    /// The users table, or `None` when [`users_table`](Self::users_table)
    /// is empty.
    pub fn users_table(&self) -> Option<&str> {
        Some(self.users_table.trim()).filter(|table| !table.is_empty())
    }
}

impl ConfigSection for WorkspaceConfig {
    const KEY: &'static str = "workspace";

    fn validate(&self) -> Result<(), String> {
        self.check().map_err(|errors| errors.join("; "))
    }
}

/// Whether `url` is an absolute `http://` or `https://` URL with a host.
fn is_http_url(url: &str) -> bool {
    ["https://", "http://"]
        .iter()
        .find_map(|scheme| url.strip_prefix(scheme))
        .is_some_and(|rest| !rest.is_empty() && !rest.starts_with('/'))
}

/// Whether `domain` looks like a host name: dot-separated labels of ASCII
/// letters, digits and `-`, optionally with a port.
fn is_domain(domain: &str) -> bool {
    let host = domain.rsplit_once(':').map_or(domain, |(host, port)| {
        if port.chars().all(|c| c.is_ascii_digit()) {
            host
        } else {
            domain
        }
    });
    !host.is_empty()
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
}

/// A PostgreSQL identifier prefix: lower-case letters, digits and `_`,
/// starting with a letter, at most 30 bytes (room for a 32-digit id within
/// the 63-byte limit).
fn is_identifier(prefix: &str) -> bool {
    !prefix.is_empty()
        && prefix.len() <= 30
        && prefix.starts_with(|c: char| c.is_ascii_lowercase())
        && prefix
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Deserializes a list from a sequence or from a comma-separated string
/// (environment variables are strings).
fn list_or_csv<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct ListOrCsv<T>(std::marker::PhantomData<T>);

    impl<'de, T: Deserialize<'de>> Visitor<'de> for ListOrCsv<T> {
        type Value = Vec<T>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a list or a comma-separated string")
        }

        fn visit_str<E: de::Error>(self, value: &str) -> Result<Vec<T>, E> {
            value
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(|item| T::deserialize(de::value::StrDeserializer::<E>::new(item)))
                .collect()
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<T>, A::Error> {
            let mut items = Vec::new();
            while let Some(item) = seq.next_element()? {
                items.push(item);
            }
            Ok(items)
        }
    }

    deserializer.deserialize_any(ListOrCsv(std::marker::PhantomData))
}

#[cfg(test)]
mod tests {
    use navree_config::{Config, ConfigSources};
    use serde_json::json;

    use super::*;

    fn load(sources: ConfigSources) -> Result<WorkspaceConfig, navree_config::ConfigError> {
        Config::load(&sources).unwrap().typed::<WorkspaceConfig>()
    }

    #[test]
    fn defaults() {
        let config = load(ConfigSources::new()).unwrap();
        assert_eq!(
            config.resolvers,
            [
                ResolverKind::Header,
                ResolverKind::Path,
                ResolverKind::Subdomain,
                ResolverKind::Claim
            ]
        );
        assert_eq!(config.header, "X-Workspace");
        assert_eq!(config.base_domain, None);
        assert_eq!(config.path_prefix, "/w");
        assert_eq!(config.path_segments(), ["w"]);
        assert_eq!(config.claim, "workspace");
        assert_eq!(config.isolation, Isolation::Row);
        assert!(config.routes);
        assert_eq!(config.users_table(), Some("users"));
        assert_eq!(config.invitations.ttl_secs, 604_800);
        assert!(config.invitations.require_matching_email);
        assert!(config.roles.is_empty());
        assert_eq!(config.schema_prefix, "workspace_");
        assert_eq!(config.database_url, None);
    }

    #[test]
    fn section_is_read_from_config_and_environment() {
        let config = load(
            ConfigSources::new()
                .set("workspace.base_domain", "app.example.com")
                .set(
                    "workspace.roles.billing.permissions",
                    json!(["invoices.*", "workspace.read"]),
                )
                .env_vars(
                    "APP_",
                    [
                        ("APP_WORKSPACE__RESOLVERS", "subdomain, header"),
                        ("APP_WORKSPACE__HEADER", "X-Tenant"),
                        ("APP_WORKSPACE__ISOLATION", "schema"),
                        ("APP_WORKSPACE__ROUTES", "false"),
                        ("APP_WORKSPACE__INVITATIONS__TTL_SECS", "3600"),
                        (
                            "APP_WORKSPACE__ROLES__MEMBER__PERMISSIONS",
                            "deals.*,deals.read",
                        ),
                    ],
                ),
        )
        .unwrap();
        assert_eq!(
            config.resolvers,
            [ResolverKind::Subdomain, ResolverKind::Header]
        );
        assert_eq!(config.header, "X-Tenant");
        assert_eq!(config.base_domain.as_deref(), Some("app.example.com"));
        assert_eq!(config.isolation, Isolation::Schema);
        assert!(!config.routes);
        assert_eq!(config.invitations.ttl_secs, 3600);
        assert_eq!(
            config.roles["billing"].permissions,
            ["invoices.*", "workspace.read"]
        );
        assert_eq!(
            config.roles["member"].permissions,
            ["deals.*", "deals.read"]
        );
    }

    #[test]
    fn invalid_values_fail_validation() {
        for (key, value) in [
            ("workspace.resolvers", json!([])),
            ("workspace.resolvers", json!(["header", "header"])),
            ("workspace.header", json!("not a header")),
            ("workspace.base_domain", json!("https://app.example.com")),
            ("workspace.path_prefix", json!("/")),
            ("workspace.path_prefix", json!("w")),
            ("workspace.claim", json!(" ")),
            ("workspace.invitations.ttl_secs", json!(0)),
            ("workspace.invitations.url", json!("accept")),
            ("workspace.roles.has space.permissions", json!(["a.b"])),
            ("workspace.roles.billing.permissions", json!(["invoices"])),
            ("workspace.schema_prefix", json!("Workspace")),
            ("workspace.isolation", json!("database")),
            ("workspace.database_url", json!("sqlite://x.db")),
        ] {
            let result = load(ConfigSources::new().set(key, value.clone()));
            assert!(result.is_err(), "{key} = {value} should be rejected");
        }
        let unknown = load(ConfigSources::new().set("workspace.resolvers", json!(["cookie"])));
        assert!(unknown.is_err());
    }

    #[test]
    fn database_isolation_takes_a_url_template() {
        let config = load(
            ConfigSources::new()
                .set("workspace.isolation", "database")
                .set(
                    "workspace.database_url",
                    "sqlite://storage/workspaces/{id}.db?mode=rwc",
                ),
        )
        .unwrap();
        assert_eq!(config.isolation, Isolation::Database);
    }

    #[test]
    fn an_empty_users_table_means_none() {
        let config = load(ConfigSources::new().set("workspace.users_table", "")).unwrap();
        assert_eq!(config.users_table(), None);
    }

    #[test]
    fn domains_are_checked() {
        assert!(is_domain("app.example.com"));
        assert!(is_domain("localhost:3000"));
        assert!(!is_domain("app..example.com"));
        assert!(!is_domain("https://x"));
    }
}
