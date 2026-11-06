//! [`WorkspaceConfig`]: the `workspace` configuration section.

use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use navree_config::{ConfigSection, ConfigTable};
use serde::{Deserialize, Serialize};

use crate::role::{RoleTable, check_permission, check_role_name};

/// Where the workspace of a request comes from. The resolvers run in the
/// order of [`WorkspaceConfig::resolvers`]; the first that finds an
/// identifier (a slug or a workspace id) wins.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolverKind {
    /// A request header ([`WorkspaceConfig::header`], `X-Workspace: agra`).
    Header,
    /// The subdomain under [`WorkspaceConfig::base_domain`]
    /// (`agra.app.example.com`). Finds nothing while `base_domain` is unset.
    Subdomain,
    /// The path segment after [`WorkspaceConfig::path_prefix`]
    /// (`/api/w/agra/deals`).
    Path,
    /// A claim of the request's bearer JWT ([`WorkspaceConfig::claim`]).
    Claim,
    /// The workspace's own verified domain (`crm.agra-corp.com`), matched
    /// by navree-domains' `DomainsModule`, which marks the request with a
    /// [`DomainWorkspace`](crate::DomainWorkspace). Finds nothing without
    /// that module, or for a host that is not a verified custom domain.
    /// Workspaces only: organizations and partitions can't use it.
    Domain,
    /// The workspace the signed-in user last switched to in this browser,
    /// kept in their navree-session session (the switch route,
    /// [`WorkspaceConfig::switch_path`], stores it). For server-rendered
    /// pages, whose plain links can't carry a header. A choice made by
    /// another user, or one the user may no longer enter, is ignored and the
    /// next resolver answers. Finds nothing without the crate's `session`
    /// feature or outside `SessionModule`. Workspaces only.
    Session,
}

impl ResolverKind {
    /// `header`, `subdomain`, `path`, `claim`, `domain` or `session`.
    pub fn as_str(self) -> &'static str {
        match self {
            ResolverKind::Header => "header",
            ResolverKind::Subdomain => "subdomain",
            ResolverKind::Path => "path",
            ResolverKind::Claim => "claim",
            ResolverKind::Domain => "domain",
            ResolverKind::Session => "session",
        }
    }
}

impl fmt::Display for ResolverKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ResolverKind {
    type Err = String;

    /// The kind named by [`as_str`](ResolverKind::as_str); the message does
    /// not repeat the text.
    fn from_str(name: &str) -> Result<Self, Self::Err> {
        Ok(match name {
            "header" => ResolverKind::Header,
            "subdomain" => ResolverKind::Subdomain,
            "path" => ResolverKind::Path,
            "claim" => ResolverKind::Claim,
            "domain" => ResolverKind::Domain,
            "session" => ResolverKind::Session,
            _ => {
                return Err(
                    "a resolver is header, subdomain, path, claim, domain or session".to_owned(),
                );
            }
        })
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
    #[serde(deserialize_with = "navree_config::comma_list")]
    pub permissions: Vec<String>,
}

/// An organization role's settings, under
/// `workspace.organizations.roles.<name>`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct OrgRoleOptions {
    /// Permission strings the role grants inside the organization (on the
    /// organization routes and behind [`InOrganization`](crate::InOrganization)),
    /// on top of the built-in ones. A list, or a comma-separated string.
    #[serde(deserialize_with = "navree_config::comma_list")]
    pub permissions: Vec<String>,
    /// The workspace role the organization role gives in the workspaces its
    /// members may enter. Default: the workspace role of the same name
    /// (`owner` → `owner`, `admin` → `admin`, `partner` → `partner`), which
    /// must be a built-in or configured workspace role.
    pub workspace_role: Option<String>,
}

/// Organizations (`workspace.organizations`): an optional level above
/// workspaces. An organization groups workspaces; its members get a role
/// there, and through it a workspace role in all or some of its
/// workspaces.
///
/// ```toml
/// [workspace.organizations]
/// enabled = true
/// resolvers = ["header", "path", "subdomain", "claim"]
/// header = "X-Organization"
/// # base_domain = "partners.example.com"   # agra.partners.example.com -> agra
/// path_prefix = "/o"                       # /api/o/agra/reports -> agra
/// claim = "organization"
///
/// [workspace.organizations.roles.partner]  # an app-defined role
/// permissions = ["organization.read", "reports.read"]
/// workspace_role = "viewer"
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, ConfigTable)]
#[serde(default)]
pub struct OrganizationOptions {
    /// Whether organizations are on: the services work, the routes are
    /// mounted (with `workspace.routes`) and workspace resolution accepts
    /// organization grants. Off by default.
    pub enabled: bool,
    /// The resolvers [`CurrentOrganization`](crate::CurrentOrganization)
    /// tries, in order.
    #[serde(deserialize_with = "navree_config::comma_list_of")]
    pub resolvers: Vec<ResolverKind>,
    /// The header the `header` resolver reads.
    pub header: String,
    /// The domain whose subdomains name organizations; the `subdomain`
    /// resolver finds nothing while it is unset.
    pub base_domain: Option<String>,
    /// The path segment(s) before the organization in a URL (`/o`).
    pub path_prefix: String,
    /// The bearer JWT claim the `claim` resolver reads.
    pub claim: String,
    /// Extra permissions per organization role, and app-defined roles.
    pub roles: BTreeMap<String, OrgRoleOptions>,
}

impl Default for OrganizationOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            resolvers: vec![
                ResolverKind::Header,
                ResolverKind::Path,
                ResolverKind::Subdomain,
                ResolverKind::Claim,
            ],
            header: "X-Organization".to_owned(),
            base_domain: None,
            path_prefix: "/o".to_owned(),
            claim: "organization".to_owned(),
            roles: BTreeMap::new(),
        }
    }
}

impl OrganizationOptions {
    /// The segments of [`path_prefix`](Self::path_prefix): `/o` → `["o"]`.
    pub fn path_segments(&self) -> Vec<&str> {
        segments(&self.path_prefix)
    }
}

/// Partitions (`workspace.partitions`): named slices of a workspace's data
/// (a `partition_id` column), with members limited to all or some of them.
///
/// ```toml
/// [workspace.partitions]
/// enabled = true
/// resolvers = ["header", "claim"]   # header, path or claim
/// header = "X-Partition"            # X-Partition: agra-co-uk (a slug or an id)
/// path_prefix = "/p"
/// claim = "partition"
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, ConfigTable)]
#[serde(default)]
pub struct PartitionOptions {
    /// Whether partitions are on. Off by default.
    pub enabled: bool,
    /// The resolvers that find the request's partition, in order (`header`,
    /// `path` or `claim`).
    #[serde(deserialize_with = "navree_config::comma_list_of")]
    pub resolvers: Vec<ResolverKind>,
    /// The header the `header` resolver reads.
    pub header: String,
    /// The path segment(s) before the partition in a URL (`/p`).
    pub path_prefix: String,
    /// The bearer JWT claim the `claim` resolver reads.
    pub claim: String,
}

impl Default for PartitionOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            resolvers: vec![ResolverKind::Header, ResolverKind::Claim],
            header: "X-Partition".to_owned(),
            path_prefix: "/p".to_owned(),
            claim: "partition".to_owned(),
        }
    }
}

impl PartitionOptions {
    /// The segments of [`path_prefix`](Self::path_prefix): `/p` → `["p"]`.
    pub fn path_segments(&self) -> Vec<&str> {
        segments(&self.path_prefix)
    }
}

/// Invitation settings (`workspace.invitations`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, ConfigTable)]
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

/// What a demo workspace does with writes (`workspace.demo.mode`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DemoMode {
    /// Requests that would write (`POST`, `PUT`, `PATCH`, `DELETE`) are
    /// refused with a 403 (code `demo_read_only`), except for
    /// [`DemoOptions::editor_roles`] and the routes in
    /// [`DemoOptions::allow`].
    #[default]
    ReadOnly,
    /// Writes are allowed; every night at [`DemoOptions::reset_at`] the
    /// demo seeders clear and seed the demo workspaces again
    /// (`workspace:demo-reset`).
    Reset,
}

impl DemoMode {
    /// `read_only` or `reset`: the value of the `X-Navree-Demo` header.
    pub fn as_str(self) -> &'static str {
        match self {
            DemoMode::ReadOnly => "read_only",
            DemoMode::Reset => "reset",
        }
    }
}

impl fmt::Display for DemoMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Demo workspaces (`workspace.demo`): workspaces seeded from fixtures for
/// sales and onboarding, read-only or reset every night.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, ConfigTable)]
#[serde(default)]
pub struct DemoOptions {
    /// `read_only` (the default) or `reset`.
    #[config(one_of = "read_only, reset")]
    pub mode: DemoMode,
    /// When the nightly reset runs in `reset` mode, as 24-hour `HH:MM`
    /// UTC (default `03:00`). Needs `ScheduleModule` in the app.
    pub reset_at: String,
    /// The role signed-in users who are not members get in a demo
    /// workspace (`viewer`, say): they enter it, and `GET /workspaces`
    /// lists it for them. Unset (the default): demos are for their members.
    pub guest_role: Option<String>,
    /// Roles whose members still write to a read-only demo (the people who
    /// curate it). A list, or a comma-separated string.
    #[serde(deserialize_with = "navree_config::comma_list")]
    pub editor_roles: Vec<String>,
    /// Routes that write but stay open in a read-only demo: a route name
    /// (`reports.preview`) or a method and the mounted path
    /// (`POST /api/v1/reports/preview`). A list, or a comma-separated
    /// string.
    #[serde(deserialize_with = "navree_config::comma_list")]
    pub allow: Vec<String>,
    /// The directory [`DemoSeed::fixture`](crate::DemoSeed::fixture) reads
    /// `<name>.json` files from (default `fixtures/demo`).
    pub fixtures: String,
}

impl Default for DemoOptions {
    fn default() -> Self {
        Self {
            mode: DemoMode::ReadOnly,
            reset_at: "03:00".to_owned(),
            guest_role: None,
            editor_roles: Vec::new(),
            allow: Vec::new(),
            fixtures: "fixtures/demo".to_owned(),
        }
    }
}

impl DemoOptions {
    /// [`reset_at`](Self::reset_at) as (hour, minute), when it is a valid
    /// `HH:MM` time.
    pub fn reset_time(&self) -> Option<(u32, u32)> {
        parse_hh_mm(&self.reset_at)
    }

    /// Whether a request that writes, with `method` to the route named
    /// `name` mounted at `path`, may write to a demo workspace in this mode
    /// for a member with `role`: always in `reset` mode; in `read_only`
    /// mode for [`editor_roles`](Self::editor_roles) and the routes in
    /// [`allow`](Self::allow).
    pub fn allows_write(
        &self,
        method: &http::Method,
        name: Option<&str>,
        path: Option<&str>,
        role: &str,
    ) -> bool {
        if self.mode == DemoMode::Reset {
            return true;
        }
        if self.editor_roles.iter().any(|editor| editor == role) {
            return true;
        }
        self.allow
            .iter()
            .any(|entry| match entry.trim().split_once(' ') {
                Some((verb, route)) => {
                    verb.eq_ignore_ascii_case(method.as_str()) && Some(route.trim()) == path
                }
                None => Some(entry.trim()) == name,
            })
    }

    /// The problems with the options, naming their keys; `roles` checks
    /// the role names.
    fn check(&self, roles: &RoleTable, errors: &mut Vec<String>) {
        if self.reset_time().is_none() {
            errors.push(
                "workspace.demo.reset_at must be a 24-hour `HH:MM` time (UTC), such as `03:00`"
                    .to_owned(),
            );
        }
        if let Some(role) = &self.guest_role {
            if role == crate::role::OWNER {
                errors.push("workspace.demo.guest_role can't be `owner`".to_owned());
            } else if !roles.contains(&crate::WorkspaceRole::parse(role)) {
                errors.push(format!(
                    "workspace.demo.guest_role: `{role}` is not a workspace role"
                ));
            }
        }
        for role in &self.editor_roles {
            if !roles.contains(&crate::WorkspaceRole::parse(role)) {
                errors.push(format!(
                    "workspace.demo.editor_roles: `{role}` is not a workspace role"
                ));
            }
        }
        for entry in &self.allow {
            let valid = match entry.trim().split_once(' ') {
                Some((verb, path)) => {
                    ["GET", "POST", "PUT", "PATCH", "DELETE"]
                        .iter()
                        .any(|m| m.eq_ignore_ascii_case(verb))
                        && path.trim().starts_with('/')
                }
                None => !entry.trim().is_empty(),
            };
            if !valid {
                errors.push(format!(
                    "workspace.demo.allow: `{entry}` must be a route name or `METHOD /path`"
                ));
            }
        }
        if self.fixtures.trim().is_empty() {
            errors.push("workspace.demo.fixtures must name a directory".to_owned());
        }
    }
}

/// Parses `"HH:MM"` (hour 0–23 with one or two digits, minute 00–59).
pub(crate) fn parse_hh_mm(time: &str) -> Option<(u32, u32)> {
    let (hour, minute) = time.trim().split_once(':')?;
    let number = |s: &str| {
        (!s.is_empty() && s.len() <= 2 && s.bytes().all(|b| b.is_ascii_digit()))
            .then(|| s.parse::<u32>().ok())
            .flatten()
    };
    let hour = number(hour)?;
    let minute = number(minute).filter(|_| minute.len() == 2)?;
    (hour < 24 && minute < 60).then_some((hour, minute))
}

/// Options of the workspace module (configuration section `workspace`).
///
/// ```toml
/// [workspace]
/// resolvers = ["header", "path", "subdomain", "claim"]  # order; or "header,path"
/// # "domain" (first, usually) names a workspace by its custom domain (navree-domains)
/// # "session" names the workspace a browser switched to (Navree UI pages)
/// header = "X-Workspace"          # X-Workspace: agra (a slug or an id)
/// # base_domain = "app.example.com"  # agra.app.example.com -> agra
/// path_prefix = "/w"              # /api/w/agra/deals -> agra
/// claim = "workspace"             # a claim of the bearer JWT
/// isolation = "row"               # row | schema | database
/// routes = true                   # /api/workspaces and /api/invitations
/// switch_path = "/workspace/switch"  # where a browser switches workspace (feature `session`)
/// session_default = false         # a browser that chose none is in its first workspace
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
///
/// [workspace.organizations]       # organizations above workspaces
/// enabled = false
///
/// [workspace.partitions]          # partitions inside workspaces
/// enabled = false
///
/// [workspace.demo]                # demo workspaces
/// mode = "read_only"              # read_only | reset
/// reset_at = "03:00"              # the nightly reset (UTC), mode = "reset"
/// # guest_role = "viewer"         # signed-in non-members enter demos
/// editor_roles = []               # roles that still write to read-only demos
/// allow = []                      # route names or "METHOD /path" left writable
/// fixtures = "fixtures/demo"      # where DemoSeed::fixture reads <name>.json
/// ```
///
/// Environment variables map as usual: `APP_WORKSPACE__HEADER` →
/// `workspace.header`; lists also accept a comma-separated string
/// (`APP_WORKSPACE__RESOLVERS=header,path`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, ConfigSection)]
#[config(
    key = "workspace",
    docs = "/docs/workspaces/introduction/",
    validate = Self::check_values
)]
#[serde(default)]
pub struct WorkspaceConfig {
    /// The resolvers to try, in order.
    #[serde(deserialize_with = "navree_config::comma_list_of")]
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
    #[config(one_of = "row, schema, database")]
    pub isolation: Isolation,
    /// Whether the module mounts its routes (`/api/workspaces/...` and
    /// `/api/invitations/accept`).
    pub routes: bool,
    /// Where a browser switches workspace: the absolute path (no API
    /// prefix) of the `POST` route that stores the choice the `session`
    /// resolver reads, mounted with [`routes`](Self::routes) when the
    /// crate's `session` feature is on (default `/workspace/switch`).
    pub switch_path: String,
    /// Whether a browser signed in on its session that has not switched
    /// workspace yet (and that no other resolver places) is put in the first
    /// workspace its user belongs to, the oldest, which it then keeps until it
    /// switches. Off by default: such a request names no workspace (400
    /// `workspace_required`). Navree UI apps with the `session` resolver turn
    /// it on, so a page opens in a workspace right after signing in.
    pub session_default: bool,
    /// The table of user accounts (`id`, `name`, `email`; navree-auth's
    /// `users`) that member lists and invitation checks read; empty when
    /// the app has none.
    pub users_table: String,
    /// Invitations.
    #[config(table)]
    pub invitations: InvitationOptions,
    /// Extra permissions per role, and app-defined roles.
    pub roles: BTreeMap<String, RoleOptions>,
    /// Schema isolation: the prefix of each workspace's schema name
    /// (`workspace_` gives `workspace_0199...`).
    pub schema_prefix: String,
    /// Database isolation: the URL of each workspace's database, with
    /// `{id}` standing for the workspace id (simple form, 32 hex digits).
    pub database_url: Option<String>,
    /// Organizations above workspaces (off by default).
    #[config(table)]
    pub organizations: OrganizationOptions,
    /// Partitions inside workspaces (off by default).
    #[config(table)]
    pub partitions: PartitionOptions,
    /// Demo workspaces.
    #[config(table)]
    pub demo: DemoOptions,
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
            switch_path: "/workspace/switch".to_owned(),
            session_default: false,
            users_table: "users".to_owned(),
            invitations: InvitationOptions::default(),
            roles: BTreeMap::new(),
            schema_prefix: "workspace_".to_owned(),
            database_url: None,
            organizations: OrganizationOptions::default(),
            partitions: PartitionOptions::default(),
            demo: DemoOptions::default(),
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
        if !is_switch_path(&self.switch_path) {
            errors.push(
                "workspace.switch_path must be a path such as `/workspace/switch` (a `/`, then letters, digits, `-`, `_`, `.` and `/`)"
                    .to_owned(),
            );
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
        self.check_organizations(&mut errors);
        self.check_partitions(&mut errors);
        self.demo.check(&RoleTable::from_config(self), &mut errors);
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// The segments of [`path_prefix`](Self::path_prefix): `/w` → `["w"]`.
    pub fn path_segments(&self) -> Vec<&str> {
        segments(&self.path_prefix)
    }

    /// Whether `name` is a built-in or configured workspace role.
    fn has_workspace_role(&self, name: &str) -> bool {
        matches!(name, "owner" | "admin" | "member" | "viewer") || self.roles.contains_key(name)
    }

    fn check_organizations(&self, errors: &mut Vec<String>) {
        let orgs = &self.organizations;
        let key = "workspace.organizations";
        check_resolvers(key, &orgs.resolvers, true, errors);
        check_header(key, &orgs.header, errors);
        if let Some(domain) = &orgs.base_domain
            && !is_domain(domain)
        {
            errors.push(format!(
                "{key}.base_domain must be a host name such as `partners.example.com`"
            ));
        }
        check_prefix(key, &orgs.path_prefix, errors);
        check_claim(key, &orgs.claim, errors);
        for (name, role) in &orgs.roles {
            if let Err(reason) = check_role_name(name) {
                errors.push(format!("{key}.roles: `{name}` {reason}"));
            }
            for permission in &role.permissions {
                if let Err(reason) = check_permission(permission) {
                    errors.push(format!(
                        "{key}.roles.{name}.permissions: `{permission}` {reason}"
                    ));
                }
            }
            let mapped = role.workspace_role.as_deref().unwrap_or(name);
            if !self.has_workspace_role(mapped) {
                errors.push(format!(
                    "{key}.roles.{name}.workspace_role must name a workspace role (built in or under workspace.roles), not `{mapped}`"
                ));
            }
        }
    }

    fn check_partitions(&self, errors: &mut Vec<String>) {
        let partitions = &self.partitions;
        let key = "workspace.partitions";
        check_resolvers(key, &partitions.resolvers, false, errors);
        check_header(key, &partitions.header, errors);
        check_prefix(key, &partitions.path_prefix, errors);
        check_claim(key, &partitions.claim, errors);
    }

    /// The users table, or `None` when [`users_table`](Self::users_table)
    /// is empty.
    pub fn users_table(&self) -> Option<&str> {
        Some(self.users_table.trim()).filter(|table| !table.is_empty())
    }
}

impl WorkspaceConfig {
    /// Checks the values for the configuration section: every problem, in
    /// one message (the boot calls this).
    fn check_values(&self) -> Result<(), String> {
        self.check().map_err(|errors| errors.join("; "))
    }
}

/// The non-empty segments of a path prefix.
fn segments(prefix: &str) -> Vec<&str> {
    prefix
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect()
}

/// A resolver list of organizations or partitions: at least one, no
/// repeats, no `domain` (custom domains name workspaces) and, for
/// partitions, no `subdomain`.
fn check_resolvers(
    key: &str,
    resolvers: &[ResolverKind],
    subdomain: bool,
    errors: &mut Vec<String>,
) {
    if resolvers.is_empty() {
        errors.push(format!("{key}.resolvers must name at least one resolver"));
    }
    for (index, kind) in resolvers.iter().enumerate() {
        if resolvers[..index].contains(kind) {
            errors.push(format!("{key}.resolvers lists `{kind}` twice"));
        }
        if !subdomain && *kind == ResolverKind::Subdomain {
            errors.push(format!(
                "{key}.resolvers can't use `subdomain` (use header, path or claim)"
            ));
        }
        if *kind == ResolverKind::Domain {
            errors.push(format!(
                "{key}.resolvers can't use `domain` (custom domains name workspaces)"
            ));
        }
        if *kind == ResolverKind::Session {
            errors.push(format!(
                "{key}.resolvers can't use `session` (a browser switches workspaces)"
            ));
        }
    }
}

/// Whether `path` can be the switch route's path: absolute, one or more
/// segments of letters, digits, `-`, `_` and `.`, and no `//`, `..` or
/// route captures.
fn is_switch_path(path: &str) -> bool {
    path.starts_with('/')
        && path.len() <= 255
        && !segments(path).is_empty()
        && !path.contains("//")
        && !path.contains("..")
        && path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'_' | b'.'))
}

fn check_header(key: &str, header: &str, errors: &mut Vec<String>) {
    if http::HeaderName::from_bytes(header.as_bytes()).is_err() {
        errors.push(format!("{key}.header must be a valid header name"));
    }
}

fn check_prefix(key: &str, prefix: &str, errors: &mut Vec<String>) {
    if segments(prefix).is_empty() || !prefix.starts_with('/') {
        errors.push(format!(
            "{key}.path_prefix must start with `/` and name a segment"
        ));
    }
}

fn check_claim(key: &str, claim: &str, errors: &mut Vec<String>) {
    if claim.trim().is_empty() {
        errors.push(format!("{key}.claim must not be empty"));
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

#[cfg(test)]
mod tests {
    use navree_config::{Config, ConfigSources, FieldType};
    use serde_json::json;

    use super::*;

    fn load(sources: ConfigSources) -> Result<WorkspaceConfig, navree_config::ConfigError> {
        Config::load(&sources).unwrap().typed::<WorkspaceConfig>()
    }

    #[test]
    fn the_section_describes_itself() {
        let schema = WorkspaceConfig::schema().expect("schema");
        assert_eq!(schema.key, "workspace");
        assert_eq!(
            schema.docs.as_deref(),
            Some("/docs/workspaces/introduction/")
        );
        assert!(
            schema.unmarked_secrets().is_empty(),
            "{:?}",
            schema.unmarked_secrets()
        );
        assert!(schema.secret_paths().is_empty());
        let field = |name: &str| {
            schema
                .fields
                .iter()
                .find(|f| f.name == name)
                .unwrap_or_else(|| panic!("{name}"))
        };
        assert_eq!(
            field("isolation").ty,
            FieldType::choice(&["row", "schema", "database"])
        );
        for table in ["invitations", "organizations", "partitions", "demo"] {
            assert_eq!(field(table).ty, FieldType::Table, "{table}");
        }
        let leaves = schema.leaves();
        let leaf = |path: &str| {
            leaves
                .iter()
                .find(|(key, _)| key == path)
                .map(|(_, field)| *field)
                .unwrap_or_else(|| panic!("{path}"))
        };
        assert_eq!(
            leaf("workspace.demo.mode").ty,
            FieldType::choice(&["read_only", "reset"])
        );
        leaf("workspace.organizations.resolvers");
        leaf("workspace.partitions.path_prefix");
        leaf("workspace.invitations.ttl_secs");
        let defaults = WorkspaceConfig::defaults().expect("defaults");
        assert_eq!(defaults["header"], "X-Workspace");
        assert_eq!(defaults["isolation"], "row");
        assert_eq!(defaults["invitations"]["ttl_secs"], 604_800);
        assert_eq!(defaults["demo"]["mode"], "read_only");
    }

    #[test]
    fn resolver_names_parse_and_a_bad_one_is_refused() {
        for kind in [
            ResolverKind::Header,
            ResolverKind::Subdomain,
            ResolverKind::Path,
            ResolverKind::Claim,
            ResolverKind::Domain,
            ResolverKind::Session,
        ] {
            assert_eq!(kind.as_str().parse::<ResolverKind>(), Ok(kind));
        }
        assert!("cookie".parse::<ResolverKind>().is_err());
        let result = load(
            ConfigSources::new().env_vars("APP_", [("APP_WORKSPACE__RESOLVERS", "header, cookie")]),
        );
        assert!(result.is_err());
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

    // The `session` resolver and the switch route's path.
    #[test]
    fn the_session_resolver_names_workspaces_only() {
        let config = load(
            ConfigSources::new()
                .set("workspace.resolvers", json!(["session", "header"]))
                .set("workspace.switch_path", "/app/workspace/switch"),
        )
        .unwrap();
        assert_eq!(
            config.resolvers,
            [ResolverKind::Session, ResolverKind::Header]
        );
        assert_eq!(ResolverKind::Session.as_str(), "session");
        assert_eq!(config.switch_path, "/app/workspace/switch");
        assert_eq!(WorkspaceConfig::default().switch_path, "/workspace/switch");
        let from_env = load(
            ConfigSources::new().env_vars("APP_", [("APP_WORKSPACE__RESOLVERS", "session,path")]),
        )
        .unwrap();
        assert_eq!(
            from_env.resolvers,
            [ResolverKind::Session, ResolverKind::Path]
        );
        for (key, value) in [
            ("workspace.organizations.resolvers", json!(["session"])),
            (
                "workspace.partitions.resolvers",
                json!(["session", "header"]),
            ),
            ("workspace.switch_path", json!("workspace/switch")),
            ("workspace.switch_path", json!("/")),
            ("workspace.switch_path", json!("//evil.test/switch")),
            ("workspace.switch_path", json!("/workspace/{id}")),
            ("workspace.switch_path", json!("/a/../b")),
        ] {
            let result = load(ConfigSources::new().set(key, value.clone()));
            assert!(result.is_err(), "{key} = {value} should be rejected");
        }
    }

    #[test]
    fn custom_domains_name_workspaces_only() {
        let config = load(
            ConfigSources::new().env_vars("APP_", [("APP_WORKSPACE__RESOLVERS", "domain,header")]),
        )
        .unwrap();
        assert_eq!(
            config.resolvers,
            [ResolverKind::Domain, ResolverKind::Header]
        );
        assert_eq!(ResolverKind::Domain.as_str(), "domain");
        assert_eq!(ResolverKind::Domain.to_string(), "domain");
        for key in [
            "workspace.organizations.resolvers",
            "workspace.partitions.resolvers",
        ] {
            let error = load(ConfigSources::new().set(key, json!(["domain", "header"])))
                .unwrap_err()
                .to_string();
            assert!(
                error.contains(&format!("{key} can't use `domain`")),
                "{error}"
            );
        }
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
    fn organizations_and_partitions_are_off_by_default() {
        let config = load(ConfigSources::new()).unwrap();
        let orgs = &config.organizations;
        assert!(!orgs.enabled);
        assert_eq!(
            orgs.resolvers,
            [
                ResolverKind::Header,
                ResolverKind::Path,
                ResolverKind::Subdomain,
                ResolverKind::Claim
            ]
        );
        assert_eq!(orgs.header, "X-Organization");
        assert_eq!(orgs.base_domain, None);
        assert_eq!(orgs.path_prefix, "/o");
        assert_eq!(orgs.path_segments(), ["o"]);
        assert_eq!(orgs.claim, "organization");
        assert!(orgs.roles.is_empty());
        let partitions = &config.partitions;
        assert!(!partitions.enabled);
        assert_eq!(
            partitions.resolvers,
            [ResolverKind::Header, ResolverKind::Claim]
        );
        assert_eq!(partitions.header, "X-Partition");
        assert_eq!(partitions.path_prefix, "/p");
        assert_eq!(partitions.claim, "partition");
    }

    #[test]
    fn organization_and_partition_options_are_read_from_config_and_environment() {
        let config = load(
            ConfigSources::new()
                .set(
                    "workspace.roles.auditor.permissions",
                    json!(["reports.read"]),
                )
                .set(
                    "workspace.organizations.roles.partner.permissions",
                    json!(["organization.read"]),
                )
                .set(
                    "workspace.organizations.roles.partner.workspace_role",
                    "auditor",
                )
                .env_vars(
                    "APP_",
                    [
                        ("APP_WORKSPACE__ORGANIZATIONS__ENABLED", "true"),
                        (
                            "APP_WORKSPACE__ORGANIZATIONS__RESOLVERS",
                            "subdomain,header",
                        ),
                        ("APP_WORKSPACE__ORGANIZATIONS__HEADER", "X-Company"),
                        (
                            "APP_WORKSPACE__ORGANIZATIONS__BASE_DOMAIN",
                            "partners.example.com",
                        ),
                        (
                            "APP_WORKSPACE__ORGANIZATIONS__ROLES__ADMIN__PERMISSIONS",
                            "reports.*,billing.read",
                        ),
                        ("APP_WORKSPACE__PARTITIONS__ENABLED", "true"),
                        ("APP_WORKSPACE__PARTITIONS__RESOLVERS", "path,header"),
                        ("APP_WORKSPACE__PARTITIONS__HEADER", "X-Domain"),
                    ],
                ),
        )
        .unwrap();
        let orgs = &config.organizations;
        assert!(orgs.enabled);
        assert_eq!(
            orgs.resolvers,
            [ResolverKind::Subdomain, ResolverKind::Header]
        );
        assert_eq!(orgs.header, "X-Company");
        assert_eq!(orgs.base_domain.as_deref(), Some("partners.example.com"));
        assert_eq!(
            orgs.roles["admin"].permissions,
            ["reports.*", "billing.read"]
        );
        assert_eq!(orgs.roles["admin"].workspace_role, None);
        assert_eq!(orgs.roles["partner"].permissions, ["organization.read"]);
        assert_eq!(
            orgs.roles["partner"].workspace_role.as_deref(),
            Some("auditor")
        );
        let partitions = &config.partitions;
        assert!(partitions.enabled);
        assert_eq!(
            partitions.resolvers,
            [ResolverKind::Path, ResolverKind::Header]
        );
        assert_eq!(partitions.header, "X-Domain");
    }

    #[test]
    fn invalid_organization_and_partition_options_fail_validation() {
        for (key, value) in [
            ("workspace.organizations.resolvers", json!([])),
            (
                "workspace.organizations.resolvers",
                json!(["claim", "claim"]),
            ),
            ("workspace.organizations.header", json!("bad header")),
            ("workspace.organizations.base_domain", json!("https://x")),
            ("workspace.organizations.path_prefix", json!("o")),
            ("workspace.organizations.claim", json!("")),
            (
                "workspace.organizations.roles.Partner.permissions",
                json!(["organization.read"]),
            ),
            (
                "workspace.organizations.roles.partner.permissions",
                json!(["organization"]),
            ),
            // An app-defined organization role maps to a workspace role of
            // the same name unless it names one, and that role must exist.
            (
                "workspace.organizations.roles.partner.permissions",
                json!(["organization.read"]),
            ),
            (
                "workspace.organizations.roles.admin.workspace_role",
                json!("auditor"),
            ),
            ("workspace.partitions.resolvers", json!([])),
            ("workspace.partitions.resolvers", json!(["subdomain"])),
            ("workspace.partitions.header", json!("")),
            ("workspace.partitions.path_prefix", json!("/")),
            ("workspace.partitions.claim", json!("  ")),
        ] {
            let result = load(ConfigSources::new().set(key, value.clone()));
            assert!(result.is_err(), "{key} = {value} should be rejected");
        }
        let mapped = load(ConfigSources::new().set(
            "workspace.organizations.roles.partner.workspace_role",
            "viewer",
        ));
        assert!(mapped.is_ok(), "{mapped:?}");
        let same_name = load(
            ConfigSources::new()
                .set("workspace.roles.partner.permissions", json!(["deals.read"]))
                .set(
                    "workspace.organizations.roles.partner.permissions",
                    json!(["organization.read"]),
                ),
        );
        assert!(same_name.is_ok(), "{same_name:?}");
    }

    #[test]
    fn demo_options_default_to_read_only() {
        let demo = load(ConfigSources::new()).unwrap().demo;
        assert_eq!(demo.mode, DemoMode::ReadOnly);
        assert_eq!(demo.reset_at, "03:00");
        assert_eq!(demo.reset_time(), Some((3, 0)));
        assert_eq!(demo.guest_role, None);
        assert!(demo.editor_roles.is_empty());
        assert!(demo.allow.is_empty());
        assert_eq!(demo.fixtures, "fixtures/demo");
        assert_eq!(DemoMode::ReadOnly.as_str(), "read_only");
        assert_eq!(DemoMode::Reset.to_string(), "reset");
    }

    #[test]
    fn demo_options_are_read_from_the_environment() {
        let demo = load(ConfigSources::new().env_vars(
            "APP_",
            [
                ("APP_WORKSPACE__DEMO__MODE", "reset"),
                ("APP_WORKSPACE__DEMO__RESET_AT", "4:15"),
                ("APP_WORKSPACE__DEMO__GUEST_ROLE", "viewer"),
                ("APP_WORKSPACE__DEMO__EDITOR_ROLES", "owner, admin"),
                (
                    "APP_WORKSPACE__DEMO__ALLOW",
                    "reports.preview,POST /api/search",
                ),
                ("APP_WORKSPACE__DEMO__FIXTURES", "storage/demo"),
            ],
        ))
        .unwrap()
        .demo;
        assert_eq!(demo.mode, DemoMode::Reset);
        assert_eq!(demo.reset_time(), Some((4, 15)));
        assert_eq!(demo.guest_role.as_deref(), Some("viewer"));
        assert_eq!(demo.editor_roles, ["owner", "admin"]);
        assert_eq!(demo.allow, ["reports.preview", "POST /api/search"]);
        assert_eq!(demo.fixtures, "storage/demo");
    }

    #[test]
    fn read_only_demos_allow_editors_and_listed_routes_to_write() {
        use http::Method;
        let demo = DemoOptions {
            editor_roles: vec!["admin".into()],
            allow: vec!["reports.preview".into(), "post /api/search".into()],
            ..DemoOptions::default()
        };
        let path = Some("/api/deals");
        for method in [Method::POST, Method::PUT, Method::PATCH, Method::DELETE] {
            assert!(!demo.allows_write(&method, Some("deals.store"), path, "member"));
            assert!(demo.allows_write(&method, Some("deals.store"), path, "admin"));
        }
        assert!(demo.allows_write(&Method::POST, Some("reports.preview"), path, "member"));
        assert!(demo.allows_write(&Method::POST, None, Some("/api/search"), "member"));
        assert!(!demo.allows_write(&Method::PUT, None, Some("/api/search"), "member"));
        let reset = DemoOptions {
            mode: DemoMode::Reset,
            ..DemoOptions::default()
        };
        assert!(reset.allows_write(&Method::DELETE, None, path, "viewer"));
    }

    #[test]
    fn invalid_demo_options_name_their_keys() {
        for (key, value) in [
            ("workspace.demo.reset_at", json!("3am")),
            ("workspace.demo.reset_at", json!("03:60")),
            ("workspace.demo.guest_role", json!("owner")),
            ("workspace.demo.guest_role", json!("auditor")),
            ("workspace.demo.editor_roles", json!(["auditor"])),
            ("workspace.demo.allow", json!(["PURGE /api/x"])),
            ("workspace.demo.allow", json!(["POST api/x"])),
            ("workspace.demo.fixtures", json!("")),
        ] {
            let error = load(ConfigSources::new().set(key, value.clone()))
                .unwrap_err()
                .to_string();
            assert!(error.contains(key), "{key} = {value}: {error}");
        }
        assert!(load(ConfigSources::new().set("workspace.demo.mode", "sometimes")).is_err());
        // An app-defined role may be the guest role or an editor role.
        let config = load(
            ConfigSources::new()
                .set("workspace.roles.auditor.permissions", json!(["deals.read"]))
                .set("workspace.demo.guest_role", "auditor")
                .set("workspace.demo.editor_roles", json!(["auditor"])),
        )
        .unwrap();
        assert_eq!(config.demo.guest_role.as_deref(), Some("auditor"));
    }

    #[test]
    fn hh_mm_times_are_parsed() {
        assert_eq!(parse_hh_mm("03:00"), Some((3, 0)));
        assert_eq!(parse_hh_mm(" 23:59 "), Some((23, 59)));
        assert_eq!(parse_hh_mm("7:05"), Some((7, 5)));
        assert_eq!(parse_hh_mm("24:00"), None);
        assert_eq!(parse_hh_mm("12:5"), None);
        assert_eq!(parse_hh_mm("noon"), None);
    }

    #[test]
    fn domains_are_checked() {
        assert!(is_domain("app.example.com"));
        assert!(is_domain("localhost:3000"));
        assert!(!is_domain("app..example.com"));
        assert!(!is_domain("https://x"));
    }
}
