//! `harbor.toml`: the servers and their roles, the processes, the build, the
//! deploy settings, the environment and the encrypted secrets.
//!
//! [`HarborConfig::parse`] reads the file (unknown keys are errors, so a
//! typo never goes unnoticed) and [`HarborConfig::validate`] lists
//! everything Harbor could not run. Defaults follow the docs' reference
//! table; the derived names (paths, units, the database) are methods.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::net::IpAddr;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{HarborError, Result};

/// What a server runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Caddy and the app's `web` process.
    Web,
    /// The `worker` process (`queue:work`).
    Worker,
    /// The `scheduler` process (`schedule:work`).
    Scheduler,
    /// PostgreSQL with the app's database.
    Postgres,
    /// Redis.
    Redis,
}

impl Role {
    /// Every role, in display order.
    pub const ALL: [Role; 5] = [
        Role::Web,
        Role::Worker,
        Role::Scheduler,
        Role::Postgres,
        Role::Redis,
    ];

    /// The role's name in `harbor.toml`.
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Web => "web",
            Role::Worker => "worker",
            Role::Scheduler => "scheduler",
            Role::Postgres => "postgres",
            Role::Redis => "redis",
        }
    }

    /// Whether the role runs the app's binary.
    pub fn runs_app(self) -> bool {
        matches!(self, Role::Web | Role::Worker | Role::Scheduler)
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How `navree deploy` builds the release binary (`build.method`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BuildMethod {
    /// Natively on Linux with the servers' architecture and a glibc no newer
    /// than theirs; in a container otherwise.
    #[default]
    Auto,
    /// Always with the local Cargo.
    Native,
    /// Always in a container (Docker or Podman).
    Container,
}

/// The whole `harbor.toml`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarborConfig {
    /// The package to build and run; empty means the package in the
    /// directory of `harbor.toml` (filled in by [`crate::Project::load`]).
    #[serde(default)]
    pub app: String,
    /// The domain Caddy serves and gets a certificate for.
    #[serde(default)]
    pub domain: Option<String>,
    /// More domains, redirected to `domain`.
    #[serde(default)]
    pub aliases: Vec<String>,
    /// With `[frontend]`, a domain that serves the API publicly.
    #[serde(default)]
    pub api_domain: Option<String>,
    /// A web front end deployed in the same release.
    #[serde(default)]
    pub frontend: Option<FrontendConfig>,
    /// `[build]`.
    #[serde(default)]
    pub build: BuildConfig,
    /// `[deploy]`.
    #[serde(default)]
    pub deploy: DeployConfig,
    /// `[processes]`.
    #[serde(default)]
    pub processes: Processes,
    /// `[servers.<name>]`, in the order of the file.
    #[serde(default, deserialize_with = "servers_in_any_order")]
    pub servers: Vec<Server>,
    /// `[ssh]`.
    #[serde(default)]
    pub ssh: SshConfig,
    /// `[web]`.
    #[serde(default)]
    pub web: WebConfig,
    /// `[postgres]`.
    #[serde(default)]
    pub postgres: PostgresConfig,
    /// `[redis]`.
    #[serde(default)]
    pub redis: RedisConfig,
    /// `[env]`: settings for every process.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// `[secrets]`: encrypted settings for every process (`harbor:v1:...`).
    #[serde(default)]
    pub secrets: BTreeMap<String, String>,
}

/// `[frontend]`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrontendConfig {
    /// The front end's directory, relative to the app.
    pub path: String,
    /// How to build it; the default depends on the lockfile.
    #[serde(default)]
    pub build: Option<String>,
    /// How to run it; the default depends on the lockfile.
    #[serde(default)]
    pub command: Option<String>,
    /// The path that must answer 200 before a switch.
    #[serde(default = "default_frontend_health")]
    pub health_check: String,
    /// The ports of its blue and green slots.
    #[serde(default = "default_frontend_ports")]
    pub ports: [u16; 2],
    /// The Node.js major version installed on `web` servers.
    #[serde(default = "default_node_version")]
    pub node_version: u32,
}

/// `[build]`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildConfig {
    /// `auto`, `native` or `container`.
    #[serde(default)]
    pub method: BuildMethod,
    /// Cargo features of the release build.
    #[serde(default)]
    pub features: Vec<String>,
    /// A Rust target triple, to override the servers' architecture.
    #[serde(default)]
    pub target: Option<String>,
    /// The image container builds run in; default: Harbor's builder image
    /// (Ubuntu 22.04 with rustup), whose glibc runs on 22.04 and 24.04.
    #[serde(default)]
    pub image: Option<String>,
}

/// `[deploy]`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeployConfig {
    /// Where releases live on the servers; default `/srv/<app>`.
    #[serde(default)]
    pub path: Option<String>,
    /// Releases kept on each server for rollbacks.
    #[serde(default = "default_keep_releases")]
    pub keep_releases: u32,
    /// The path that must answer 200 before a switch.
    #[serde(default = "default_health_check")]
    pub health_check: String,
    /// How long the health check may take.
    #[serde(default = "default_health_timeout")]
    pub health_timeout_secs: u64,
    /// Run `migrate` on deploy.
    #[serde(default = "default_true")]
    pub migrate: bool,
    /// Directories shipped with the binary, when present.
    #[serde(default = "default_include")]
    pub include: Vec<String>,
    /// Directories kept across releases (links to `shared/`).
    #[serde(default = "default_shared")]
    pub shared: Vec<String>,
    /// The ports of the blue and green slots.
    #[serde(default = "default_ports")]
    pub ports: [u16; 2],
    /// The port of Caddy's internal entry (always the live slot).
    #[serde(default = "default_internal_port")]
    pub internal_port: u16,
    /// The load balancers in front of Caddy (addresses or CIDR ranges).
    /// Caddy's `trusted_proxies` takes them, so an HTTP balancer's
    /// `X-Forwarded-For` survives; the app's `http.trusted_proxies` lists
    /// them after loopback; and `web.proxy_protocol` accepts the PROXY
    /// protocol from them.
    #[serde(default)]
    pub trusted_proxies: Vec<String>,
}

impl Default for DeployConfig {
    fn default() -> Self {
        Self {
            path: None,
            keep_releases: default_keep_releases(),
            health_check: default_health_check(),
            health_timeout_secs: default_health_timeout(),
            migrate: true,
            include: default_include(),
            shared: default_shared(),
            ports: default_ports(),
            internal_port: default_internal_port(),
            trusted_proxies: Vec::new(),
        }
    }
}

/// `[processes]`: what the `web`, `worker` and `scheduler` roles run.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Processes {
    /// The `web` process.
    #[serde(default = "default_web_process")]
    pub web: Process,
    /// The `worker` process.
    #[serde(default = "default_worker_process")]
    pub worker: WorkerProcess,
    /// The `scheduler` process.
    #[serde(default = "default_scheduler_process")]
    pub scheduler: Process,
}

impl Default for Processes {
    fn default() -> Self {
        Self {
            web: default_web_process(),
            worker: default_worker_process(),
            scheduler: default_scheduler_process(),
        }
    }
}

/// A process: the app command it runs.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Process {
    /// The app command and its arguments, e.g. `serve`.
    pub command: String,
}

/// The worker process: its command and how many run on each server.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerProcess {
    /// The app command, e.g. `queue:work` or `fleet`.
    #[serde(default = "default_worker_command")]
    pub command: String,
    /// Worker processes per `worker` server.
    #[serde(default = "default_instances")]
    pub instances: u32,
}

/// One `[servers.<name>]` table.
#[derive(Debug, Clone, PartialEq)]
pub struct Server {
    /// The table's name (`app-1`).
    pub name: String,
    /// The public address or host name.
    pub host: String,
    /// The address on a private network.
    pub private_ip: Option<String>,
    /// What it runs.
    pub roles: Vec<Role>,
    /// The pinned SSH host key (`ssh-ed25519 AAAA...`).
    pub host_key: Option<String>,
}

impl Server {
    /// Whether the server has `role`.
    pub fn has(&self, role: Role) -> bool {
        self.roles.contains(&role)
    }

    /// Whether it runs the app's binary (web, worker or scheduler).
    pub fn runs_app(&self) -> bool {
        self.roles.iter().any(|r| r.runs_app())
    }

    /// The address other servers reach it at: the private address when it
    /// has one.
    pub fn internal_address(&self) -> &str {
        self.private_ip.as_deref().unwrap_or(&self.host)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawServer {
    host: String,
    #[serde(default)]
    private_ip: Option<String>,
    #[serde(default = "default_roles")]
    roles: Vec<Role>,
    #[serde(default)]
    host_key: Option<String>,
}

fn servers_in_any_order<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Vec<Server>, D::Error> {
    let map = BTreeMap::<String, RawServer>::deserialize(deserializer)?;
    Ok(map
        .into_iter()
        .map(|(name, raw)| Server {
            name,
            host: raw.host,
            private_ip: raw.private_ip,
            roles: raw.roles,
            host_key: raw.host_key,
        })
        .collect())
}

/// `[ssh]`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SshConfig {
    /// The deploy user, who owns the app and runs its processes.
    #[serde(default = "default_deploy_user")]
    pub user: String,
    /// The user `harbor:provision` connects as.
    #[serde(default = "default_provision_user")]
    pub provision_user: String,
    /// The SSH port.
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    /// Your private key; default `~/.ssh/id_ed25519`.
    #[serde(default)]
    pub key: Option<String>,
    /// Public keys allowed to log in as the deploy user; default the public
    /// half of `key`.
    #[serde(default)]
    pub authorized_keys: Option<Vec<String>>,
}

impl Default for SshConfig {
    fn default() -> Self {
        Self {
            user: default_deploy_user(),
            provision_user: default_provision_user(),
            port: default_ssh_port(),
            key: None,
            authorized_keys: None,
        }
    }
}

/// `[web]`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebConfig {
    /// Addresses allowed to fetch `/metrics` through Caddy.
    #[serde(default)]
    pub metrics_allow: Vec<String>,
    /// Accept the PROXY protocol from `deploy.trusted_proxies`.
    #[serde(default)]
    pub proxy_protocol: bool,
    /// `[web.custom_domains]`: workspaces' own domains.
    #[serde(default)]
    pub custom_domains: CustomDomainsConfig,
}

/// Where Caddy sends requests to workspaces' own domains.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CustomDomainsTo {
    /// The app's `web` process.
    #[default]
    App,
    /// The `[frontend]`.
    Frontend,
}

/// `[web.custom_domains]`: Caddy serves any domain the app's ask endpoint
/// allows (navree-domains' `domains.ask_path`), with a certificate it gets
/// on the domain's first visit (on-demand TLS).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomDomainsConfig {
    /// Serve workspaces' own domains.
    #[serde(default)]
    pub enabled: bool,
    /// The app's ask endpoint, called through Caddy's internal entry
    /// (`domains.ask_path`).
    #[serde(default = "default_ask_path")]
    pub ask_path: String,
    /// `app` (default) or `frontend`.
    #[serde(default)]
    pub to: CustomDomainsTo,
}

impl Default for CustomDomainsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            ask_path: default_ask_path(),
            to: CustomDomainsTo::App,
        }
    }
}

fn default_ask_path() -> String {
    "/_domains/allow".to_owned()
}

/// `[postgres]`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostgresConfig {
    /// The PostgreSQL major version.
    #[serde(default = "default_postgres_version")]
    pub version: u32,
    /// The database and user name; default the app's name with `_`.
    #[serde(default)]
    pub database: Option<String>,
    /// Daily backups kept.
    #[serde(default = "default_backup_days")]
    pub backup_days: u32,
}

impl Default for PostgresConfig {
    fn default() -> Self {
        Self {
            version: default_postgres_version(),
            database: None,
            backup_days: default_backup_days(),
        }
    }
}

/// `[redis]`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedisConfig {
    /// Redis's memory limit.
    #[serde(default = "default_maxmemory")]
    pub maxmemory_mb: u32,
}

impl Default for RedisConfig {
    fn default() -> Self {
        Self {
            maxmemory_mb: default_maxmemory(),
        }
    }
}

fn default_true() -> bool {
    true
}
fn default_frontend_health() -> String {
    "/".to_owned()
}
fn default_frontend_ports() -> [u16; 2] {
    [3001, 3002]
}
fn default_node_version() -> u32 {
    24
}
fn default_keep_releases() -> u32 {
    5
}
fn default_health_check() -> String {
    "/health/ready".to_owned()
}
fn default_health_timeout() -> u64 {
    60
}
fn default_include() -> Vec<String> {
    vec!["config".to_owned(), "resources".to_owned()]
}
fn default_shared() -> Vec<String> {
    vec!["storage".to_owned()]
}
fn default_ports() -> [u16; 2] {
    [8081, 8082]
}
fn default_internal_port() -> u16 {
    8079
}
fn default_web_process() -> Process {
    Process {
        command: "serve".to_owned(),
    }
}
fn default_worker_command() -> String {
    "queue:work".to_owned()
}
fn default_instances() -> u32 {
    1
}
fn default_worker_process() -> WorkerProcess {
    WorkerProcess {
        command: default_worker_command(),
        instances: default_instances(),
    }
}
fn default_scheduler_process() -> Process {
    Process {
        command: "schedule:work".to_owned(),
    }
}
fn default_roles() -> Vec<Role> {
    vec![Role::Web, Role::Worker, Role::Scheduler]
}
fn default_deploy_user() -> String {
    "navree".to_owned()
}
fn default_provision_user() -> String {
    "root".to_owned()
}
fn default_ssh_port() -> u16 {
    22
}
fn default_postgres_version() -> u32 {
    17
}
fn default_backup_days() -> u32 {
    7
}
fn default_maxmemory() -> u32 {
    256
}

impl HarborConfig {
    /// Parses `text` (the contents of `path`). Unknown keys are errors.
    /// Servers keep the order of the file. Nothing is validated yet: see
    /// [`validate`](HarborConfig::validate).
    pub fn parse(text: &str, path: &Path) -> Result<HarborConfig> {
        let mut config: HarborConfig = toml::from_str(text).map_err(|e| HarborError::Parse {
            path: path.to_path_buf(),
            message: e.to_string().trim_end().to_owned(),
        })?;
        // serde sees tables sorted by name; the document knows their order.
        if let Ok(document) = text.parse::<toml_edit::DocumentMut>()
            && let Some(servers) = document.get("servers").and_then(|s| s.as_table_like())
        {
            let order: Vec<&str> = servers.iter().map(|(name, _)| name).collect();
            config
                .servers
                .sort_by_key(|s| order.iter().position(|n| *n == s.name));
        }
        Ok(config)
    }

    /// Every problem that stops Harbor from running this configuration, one
    /// line each; empty when it is valid.
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        let mut problem = |p: String| problems.push(p);

        if self.app.is_empty() {
            problem("`app` is not set and there is no Cargo package next to harbor.toml".into());
        } else if !is_app_name(&self.app) {
            problem(format!(
                "`app` must be a package name of letters, digits, `-` and `_`, starting with a letter (got `{}`)",
                self.app
            ));
        }

        // Servers.
        if self.servers.is_empty() {
            problem("no servers: add a [servers.<name>] table with `host` and `roles`".into());
        }
        for server in &self.servers {
            let name = &server.name;
            if !is_server_name(name) {
                problem(format!(
                    "servers.{name}: a server's name must be letters, digits, `-` and `_`"
                ));
            }
            if server.host.trim().is_empty() || server.host.contains(char::is_whitespace) {
                problem(format!(
                    "servers.{name}.host must be an address or host name"
                ));
            }
            if server.roles.is_empty() {
                problem(format!("servers.{name}.roles is empty"));
            }
            let unique: BTreeSet<Role> = server.roles.iter().copied().collect();
            if unique.len() != server.roles.len() {
                problem(format!("servers.{name}.roles lists a role twice"));
            }
            if let Some(ip) = &server.private_ip
                && ip.parse::<IpAddr>().is_err()
            {
                problem(format!(
                    "servers.{name}.private_ip must be an IP address (got `{ip}`)"
                ));
            }
            if let Some(key) = &server.host_key
                && !is_public_key(key)
            {
                problem(format!(
                    "servers.{name}.host_key must look like `ssh-ed25519 AAAA...` (written by harbor:provision)"
                ));
            }
        }
        let names: BTreeSet<&str> = self.servers.iter().map(|s| s.name.as_str()).collect();
        if names.len() != self.servers.len() {
            problem("two servers have the same name".into());
        }

        // Domains.
        let web = self.servers.iter().any(|s| s.has(Role::Web));
        match &self.domain {
            None if web => problem("`domain` is required with a `web` server".into()),
            Some(domain) if !is_domain(domain) => problem(format!(
                "`domain` must be a host name like `app.example.com` (got `{domain}`)"
            )),
            _ => {}
        }
        for alias in &self.aliases {
            if !is_domain(alias) {
                problem(format!("`aliases`: `{alias}` is not a host name"));
            }
        }
        if let Some(api) = &self.api_domain {
            if self.frontend.is_none() {
                problem(
                    "`api_domain` needs a [frontend] (without one, `domain` serves the API)".into(),
                );
            }
            if !is_domain(api) {
                problem(format!("`api_domain` must be a host name (got `{api}`)"));
            }
        }
        let custom = &self.web.custom_domains;
        if custom.enabled {
            if !custom.ask_path.starts_with('/')
                || custom.ask_path.len() < 2
                || custom.ask_path.contains(char::is_whitespace)
            {
                problem(format!(
                    "`web.custom_domains.ask_path` must be a path such as `/_domains/allow` (got `{}`)",
                    custom.ask_path
                ));
            }
            if custom.to == CustomDomainsTo::Frontend && self.frontend.is_none() {
                problem("`web.custom_domains.to = \"frontend\"` needs a [frontend]".into());
            }
        }

        // Data servers.
        let postgres: Vec<&Server> = self
            .servers
            .iter()
            .filter(|s| s.has(Role::Postgres))
            .collect();
        let redis: Vec<&Server> = self.servers.iter().filter(|s| s.has(Role::Redis)).collect();
        if postgres.len() > 1 {
            problem("only one server may have the `postgres` role".into());
        }
        if redis.len() > 1 {
            problem("only one server may have the `redis` role".into());
        }
        if let [db] = postgres[..] {
            for client in self.remote_clients(db) {
                if db.private_ip.is_some() {
                    if client.private_ip.is_none() {
                        problem(format!(
                            "servers.{} needs a `private_ip`: PostgreSQL on {} listens on its private network",
                            client.name, db.name
                        ));
                    }
                } else if client.host.parse::<IpAddr>().is_err() {
                    problem(format!(
                        "servers.{}.host must be an IP address (or give both servers a `private_ip`): PostgreSQL on {} admits other servers by address",
                        client.name, db.name
                    ));
                }
            }
        }
        if let [cache] = redis[..] {
            let clients = self.remote_clients(cache);
            if !clients.is_empty() && cache.private_ip.is_none() {
                problem(format!(
                    "servers.{} needs a `private_ip`: Redis is reached from other servers over a private network only",
                    cache.name
                ));
            }
            for client in clients {
                if client.private_ip.is_none() {
                    problem(format!(
                        "servers.{} needs a `private_ip`: Redis on {} is reached over a private network only",
                        client.name, cache.name
                    ));
                }
            }
        }

        // Deploy.
        let d = &self.deploy;
        if let Some(path) = &d.path
            && (!path.starts_with('/') || !is_safe_path(path))
        {
            problem(format!(
                "deploy.path must be an absolute path of letters, digits, `-`, `_`, `.` and `/` (got `{path}`)"
            ));
        }
        if d.keep_releases == 0 {
            problem("deploy.keep_releases must be at least 1".into());
        }
        if d.health_timeout_secs == 0 {
            problem("deploy.health_timeout_secs must be at least 1".into());
        }
        if !is_url_path(&d.health_check) {
            problem(format!(
                "deploy.health_check must be a path like /health/ready (got `{}`)",
                d.health_check
            ));
        }
        for dir in d.include.iter().chain(&d.shared) {
            if !is_relative_dir(dir) {
                problem(format!(
                    "deploy.include and deploy.shared take directory names relative to the app (got `{dir}`)"
                ));
            }
        }
        let mut ports = vec![d.ports[0], d.ports[1], d.internal_port];
        if let Some(frontend) = &self.frontend {
            ports.extend(frontend.ports);
        }
        let distinct: BTreeSet<u16> = ports.iter().copied().collect();
        if distinct.len() != ports.len() || distinct.contains(&0) {
            problem(format!(
                "deploy.ports, deploy.internal_port and frontend.ports must be distinct, non-zero ports (got {ports:?})"
            ));
        }
        for proxy in &d.trusted_proxies {
            if !is_ip_or_cidr(proxy) {
                problem(format!(
                    "deploy.trusted_proxies: `{proxy}` is not an address or CIDR range"
                ));
            }
        }

        // Processes.
        for (role, command) in [
            ("web", &self.processes.web.command),
            ("worker", &self.processes.worker.command),
            ("scheduler", &self.processes.scheduler.command),
        ] {
            if !is_app_command(command) {
                problem(format!(
                    "processes.{role}.command must be an app command with plain arguments (got `{command}`)"
                ));
            }
        }
        if !(1..=64).contains(&self.processes.worker.instances) {
            problem("processes.worker.instances must be between 1 and 64".into());
        }

        // SSH, web, data.
        for (key, user) in [
            ("ssh.user", &self.ssh.user),
            ("ssh.provision_user", &self.ssh.provision_user),
        ] {
            if !is_user_name(user) {
                problem(format!("{key} must be a Linux user name (got `{user}`)"));
            }
        }
        if self.ssh.user == "root" {
            problem("ssh.user must not be root: the app runs as this user".into());
        }
        if self.ssh.port == 0 {
            problem("ssh.port must not be 0".into());
        }
        if let Some(keys) = &self.ssh.authorized_keys {
            for key in keys {
                if !is_public_key(key) {
                    problem(format!(
                        "ssh.authorized_keys: `{}` is not a public key line",
                        key.chars().take(24).collect::<String>()
                    ));
                }
            }
        }
        for address in &self.web.metrics_allow {
            if !is_ip_or_cidr(address) {
                problem(format!(
                    "web.metrics_allow: `{address}` is not an address or CIDR range"
                ));
            }
        }
        if self.web.proxy_protocol && d.trusted_proxies.is_empty() {
            problem(
                "web.proxy_protocol needs deploy.trusted_proxies: the load balancer's addresses"
                    .into(),
            );
        }
        if !(13..=30).contains(&self.postgres.version) {
            problem(format!(
                "postgres.version must be a PostgreSQL major version such as 17 (got {})",
                self.postgres.version
            ));
        }
        if self.postgres.backup_days == 0 {
            problem("postgres.backup_days must be at least 1".into());
        }
        if let Some(db) = &self.postgres.database
            && !is_identifier(db)
        {
            problem(format!(
                "postgres.database must be lower-case letters, digits and `_` (got `{db}`)"
            ));
        }
        if self.redis.maxmemory_mb < 16 {
            problem("redis.maxmemory_mb must be at least 16".into());
        }

        // Front end.
        if let Some(frontend) = &self.frontend {
            if !is_relative_dir(&frontend.path) {
                problem(format!(
                    "frontend.path must be a directory relative to the app (got `{}`)",
                    frontend.path
                ));
            }
            if !is_url_path(&frontend.health_check) {
                problem(format!(
                    "frontend.health_check must be a path like / (got `{}`)",
                    frontend.health_check
                ));
            }
            if frontend.node_version < 18 {
                problem("frontend.node_version must be 18 or later".into());
            }
        }

        // Environment.
        for name in self.env.keys().chain(self.secrets.keys()) {
            if !is_env_name(name) {
                problem(format!(
                    "`{name}` is not an environment variable name (letters, digits and `_`)"
                ));
            }
        }
        for (name, value) in &self.env {
            if value.contains('\0') {
                problem(format!("env.{name} contains a NUL byte"));
            }
        }
        for (name, value) in &self.secrets {
            if !value.starts_with(crate::secrets::SECRET_PREFIX) {
                problem(format!(
                    "secrets.{name} is not encrypted: set it with `navree harbor:secrets set {name}`"
                ));
            }
        }
        problems
    }

    /// The servers that run the app's processes and reach `data` (a
    /// `postgres` or `redis` server) from another machine.
    fn remote_clients(&self, data: &Server) -> Vec<&Server> {
        self.servers
            .iter()
            .filter(|s| s.name != data.name && s.runs_app())
            .collect()
    }

    /// The server named `name`.
    pub fn server(&self, name: &str) -> Result<&Server> {
        self.servers
            .iter()
            .find(|s| s.name == name)
            .ok_or_else(|| HarborError::UnknownServer(name.to_owned()))
    }

    /// The servers with `role`, in file order.
    pub fn servers_with(&self, role: Role) -> impl Iterator<Item = &Server> {
        self.servers.iter().filter(move |s| s.has(role))
    }

    /// The server with the `postgres` role, if any.
    pub fn postgres_server(&self) -> Option<&Server> {
        self.servers_with(Role::Postgres).next()
    }

    /// The server with the `redis` role, if any.
    pub fn redis_server(&self) -> Option<&Server> {
        self.servers_with(Role::Redis).next()
    }

    /// Where releases live on the servers (`/srv/<app>`).
    pub fn deploy_path(&self) -> String {
        self.deploy
            .path
            .clone()
            .unwrap_or_else(|| format!("/srv/{}", self.app))
    }

    /// The database and user name (`crm_api` for `crm-api`).
    pub fn database(&self) -> String {
        self.postgres
            .database
            .clone()
            .unwrap_or_else(|| self.app.replace('-', "_").to_lowercase())
    }

    /// Whether every server that runs the app is `server` itself, so the
    /// app reaches its data on loopback.
    pub fn only_local_clients(&self, server: &Server) -> bool {
        self.remote_clients(server).is_empty()
    }
}

/// `harbor.toml` and `harbor.key`, or `harbor.<env>.toml` and
/// `harbor.<env>.key` with `--env <env>` (or `NAVREE_HARBOR_ENV`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Environment {
    name: Option<String>,
}

impl Environment {
    /// Production: `harbor.toml`.
    pub fn production() -> Self {
        Self { name: None }
    }

    /// The environment called `name`; `production` (or an empty name) is
    /// `harbor.toml`.
    pub fn named(name: &str) -> Result<Self> {
        let name = name.trim();
        if name.is_empty() || name == "production" {
            return Ok(Self::production());
        }
        if !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(HarborError::msg(format!(
                "environment names are lower-case letters, digits and `-` (got `{name}`)"
            )));
        }
        Ok(Self {
            name: Some(name.to_owned()),
        })
    }

    /// `--env`, else `NAVREE_HARBOR_ENV`, else production.
    pub fn select(flag: Option<&str>, var: Option<&str>) -> Result<Self> {
        match flag.or(var) {
            Some(name) => Self::named(name),
            None => Ok(Self::production()),
        }
    }

    /// The name shown in output: `production`, `staging`, ...
    pub fn name(&self) -> &str {
        self.name.as_deref().unwrap_or("production")
    }

    /// `harbor.toml` or `harbor.<env>.toml`.
    pub fn config_file(&self) -> String {
        match &self.name {
            None => "harbor.toml".to_owned(),
            Some(name) => format!("harbor.{name}.toml"),
        }
    }

    /// `harbor.key` or `harbor.<env>.key`.
    pub fn key_file(&self) -> String {
        match &self.name {
            None => "harbor.key".to_owned(),
            Some(name) => format!("harbor.{name}.key"),
        }
    }

    /// The `--env` flag that selects it again (empty for production).
    pub fn flag(&self) -> String {
        match &self.name {
            None => String::new(),
            Some(name) => format!(" --env {name}"),
        }
    }
}

// --- Checks -----------------------------------------------------------------

fn is_app_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_alphabetic())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        && name.len() <= 48
}

fn is_server_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn is_user_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_lowercase() || c == '_')
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        && name.len() <= 32
}

fn is_identifier(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_lowercase() || c == '_')
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && name.len() <= 63
}

/// An environment variable name.
pub(crate) fn is_env_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn is_domain(domain: &str) -> bool {
    !domain.is_empty()
        && domain.len() <= 253
        && domain.contains('.')
        && domain
            .split('.')
            .all(|label| !label.is_empty() && !label.starts_with('-') && !label.ends_with('-'))
        && domain
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == '*')
}

fn is_safe_path(path: &str) -> bool {
    path.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'))
        && !path.split('/').any(|part| part == "..")
}

fn is_relative_dir(dir: &str) -> bool {
    !dir.is_empty() && !dir.starts_with('/') && !dir.contains('\\') && is_safe_path(dir)
}

fn is_url_path(path: &str) -> bool {
    path.starts_with('/')
        && path
            .chars()
            .all(|c| c.is_ascii_graphic() && !matches!(c, '\'' | '"' | '\\' | '`' | '$'))
}

fn is_app_command(command: &str) -> bool {
    !command.trim().is_empty()
        && command.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, ' ' | ':' | '-' | '_' | '.' | ',' | '=' | '/')
        })
}

/// `ssh-ed25519 AAAA... [comment]`.
pub(crate) fn is_public_key(line: &str) -> bool {
    let mut parts = line.split_whitespace();
    let (Some(kind), Some(data)) = (parts.next(), parts.next()) else {
        return false;
    };
    (kind.starts_with("ssh-") || kind.starts_with("ecdsa-") || kind.starts_with("sk-"))
        && data.len() >= 16
        && data
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '='))
}

/// An IP address or a CIDR range (`10.0.0.0/24`).
pub(crate) fn is_ip_or_cidr(value: &str) -> bool {
    match value.split_once('/') {
        None => value.parse::<IpAddr>().is_ok(),
        Some((ip, bits)) => match (ip.parse::<IpAddr>(), bits.parse::<u8>()) {
            (Ok(IpAddr::V4(_)), Ok(bits)) => bits <= 32,
            (Ok(IpAddr::V6(_)), Ok(bits)) => bits <= 128,
            _ => false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> HarborConfig {
        HarborConfig::parse(text, Path::new("harbor.toml")).unwrap()
    }

    fn problems(text: &str) -> Vec<String> {
        parse(text).validate()
    }

    const MINIMAL: &str = r#"
app = "crm-api"
domain = "crm.example.com"

[servers.app-1]
host = "203.0.113.10"
"#;

    #[test]
    fn the_minimal_file_takes_every_default() {
        let config = parse(MINIMAL);
        assert_eq!(config.validate(), Vec::<String>::new());
        let server = &config.servers[0];
        assert_eq!(server.name, "app-1");
        assert_eq!(server.roles, [Role::Web, Role::Worker, Role::Scheduler]);
        assert_eq!(config.build.method, BuildMethod::Auto);
        assert_eq!(config.deploy.keep_releases, 5);
        assert_eq!(config.deploy.health_check, "/health/ready");
        assert_eq!(config.deploy.health_timeout_secs, 60);
        assert!(config.deploy.migrate);
        assert_eq!(config.deploy.include, ["config", "resources"]);
        assert_eq!(config.deploy.shared, ["storage"]);
        assert_eq!(config.deploy.ports, [8081, 8082]);
        assert_eq!(config.deploy.internal_port, 8079);
        assert_eq!(config.processes.web.command, "serve");
        assert_eq!(config.processes.worker.command, "queue:work");
        assert_eq!(config.processes.worker.instances, 1);
        assert_eq!(config.processes.scheduler.command, "schedule:work");
        assert_eq!(config.ssh.user, "navree");
        assert_eq!(config.ssh.provision_user, "root");
        assert_eq!(config.ssh.port, 22);
        assert_eq!(config.postgres.version, 17);
        assert_eq!(config.postgres.backup_days, 7);
        assert_eq!(config.redis.maxmemory_mb, 256);
        assert_eq!(config.deploy_path(), "/srv/crm-api");
        assert_eq!(config.database(), "crm_api");
    }

    #[test]
    fn servers_keep_the_order_of_the_file() {
        let config = parse(
            r#"
app = "a"
domain = "a.example.com"
[servers.web-2]
host = "10.0.0.2"
[servers.web-1]
host = "10.0.0.1"
[servers.data]
host = "10.0.0.3"
roles = ["postgres"]
"#,
        );
        let names: Vec<&str> = config.servers.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["web-2", "web-1", "data"]);
    }

    #[test]
    fn unknown_keys_are_errors() {
        let error = HarborConfig::parse(
            "app = \"a\"\n[deploy]\nkeep_relases = 3\n",
            Path::new("harbor.toml"),
        )
        .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("keep_relases"), "{message}");
        let error = HarborConfig::parse(
            "[servers.a]\nhost = \"x\"\nrole = [\"web\"]\n",
            Path::new("harbor.toml"),
        )
        .unwrap_err();
        assert!(error.to_string().contains("role"), "{error}");
        let error = HarborConfig::parse(
            "[servers.a]\nhost = \"x\"\nroles = [\"cache\"]\n",
            Path::new("h"),
        )
        .unwrap_err();
        assert!(error.to_string().contains("cache"), "{error}");
    }

    #[test]
    fn a_web_server_needs_a_domain() {
        let found = problems("app = \"a\"\n[servers.a]\nhost = \"10.0.0.1\"\n");
        assert_eq!(found, ["`domain` is required with a `web` server"]);
        let found =
            problems("app = \"a\"\n[servers.a]\nhost = \"10.0.0.1\"\nroles = [\"worker\"]\n");
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn at_least_one_server() {
        let found = problems("app = \"a\"\n");
        assert_eq!(
            found,
            ["no servers: add a [servers.<name>] table with `host` and `roles`"]
        );
    }

    #[test]
    fn one_postgres_and_one_redis_server_at_most() {
        let found = problems(
            r#"
app = "a"
[servers.a]
host = "10.0.0.1"
private_ip = "10.1.0.1"
roles = ["worker", "postgres", "redis"]
[servers.b]
host = "10.0.0.2"
private_ip = "10.1.0.2"
roles = ["postgres", "redis"]
"#,
        );
        assert!(found.contains(&"only one server may have the `postgres` role".to_owned()));
        assert!(found.contains(&"only one server may have the `redis` role".to_owned()));
    }

    #[test]
    fn remote_postgres_needs_private_addresses_or_ip_hosts() {
        let found = problems(
            r#"
app = "a"
domain = "a.example.com"
[servers.web]
host = "web.example.com"
roles = ["web"]
[servers.data]
host = "203.0.113.31"
roles = ["postgres"]
"#,
        );
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("servers.web.host must be an IP address"));

        let found = problems(
            r#"
app = "a"
domain = "a.example.com"
[servers.web]
host = "203.0.113.11"
roles = ["web"]
[servers.data]
host = "203.0.113.31"
private_ip = "10.0.0.31"
roles = ["postgres"]
"#,
        );
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("servers.web needs a `private_ip`"));
    }

    #[test]
    fn remote_redis_needs_a_private_network() {
        let found = problems(
            r#"
app = "a"
domain = "a.example.com"
[servers.web]
host = "203.0.113.11"
roles = ["web"]
[servers.data]
host = "203.0.113.31"
roles = ["redis"]
"#,
        );
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found.iter().all(|p| p.contains("private")), "{found:?}");
    }

    #[test]
    fn the_multi_server_example_is_valid() {
        let config = parse(
            r#"
app = "crm-api"
domain = "crm.example.com"

[deploy]
trusted_proxies = ["10.0.0.0/24"]

[servers.web-1]
host = "203.0.113.11"
private_ip = "10.0.0.11"
roles = ["web", "scheduler"]

[servers.web-2]
host = "203.0.113.12"
private_ip = "10.0.0.12"
roles = ["web"]

[servers.worker-1]
host = "203.0.113.21"
private_ip = "10.0.0.21"
roles = ["worker"]

[servers.data-1]
host = "203.0.113.31"
private_ip = "10.0.0.31"
roles = ["postgres", "redis"]
"#,
        );
        assert_eq!(config.validate(), Vec::<String>::new());
        assert_eq!(config.servers_with(Role::Web).count(), 2);
        assert_eq!(config.postgres_server().unwrap().name, "data-1");
        assert!(!config.only_local_clients(config.postgres_server().unwrap()));
    }

    #[test]
    fn values_are_checked() {
        let found = problems(
            r#"
app = "crm api"
domain = "https://crm.example.com"
aliases = ["www crm"]
api_domain = "api.example.com"
[deploy]
path = "srv/x"
keep_releases = 0
health_check = "health"
health_timeout_secs = 0
include = ["../etc"]
ports = [8081, 8081]
trusted_proxies = ["10.0.0.0/40"]
[processes]
worker = { command = "queue:work; rm -rf /", instances = 0 }
[ssh]
user = "root"
port = 0
authorized_keys = ["not a key"]
[web]
metrics_allow = ["everyone"]
proxy_protocol = true
[postgres]
version = 9
database = "Crm-Api"
backup_days = 0
[redis]
maxmemory_mb = 1
[env]
"BAD NAME" = "x"
[secrets]
APP_KEY = "plain text"
[servers.a]
host = "10.0.0.1"
private_ip = "not-an-ip"
roles = []
host_key = "nope"
"#,
        );
        let joined = found.join("\n");
        for needle in [
            "`app` must be a package name",
            "`domain` must be a host name",
            "`aliases`: `www crm`",
            "`api_domain` needs a [frontend]",
            "deploy.path must be an absolute path",
            "deploy.keep_releases must be at least 1",
            "deploy.health_timeout_secs must be at least 1",
            "deploy.health_check must be a path",
            "deploy.include and deploy.shared",
            "must be distinct, non-zero ports",
            "deploy.trusted_proxies: `10.0.0.0/40`",
            "processes.worker.command must be an app command",
            "processes.worker.instances must be between 1 and 64",
            "ssh.user must not be root",
            "ssh.port must not be 0",
            "ssh.authorized_keys",
            "web.metrics_allow: `everyone`",
            "postgres.version must be",
            "postgres.database must be",
            "postgres.backup_days must be at least 1",
            "redis.maxmemory_mb must be at least 16",
            "`BAD NAME` is not an environment variable name",
            "secrets.APP_KEY is not encrypted",
            "servers.a.private_ip must be an IP address",
            "servers.a.roles is empty",
            "servers.a.host_key must look like",
        ] {
            assert!(joined.contains(needle), "missing `{needle}` in:\n{joined}");
        }
        // The proxy-protocol check needs trusted proxies; they are set (if
        // invalid) above, so it does not fire.
        assert!(!joined.contains("web.proxy_protocol needs"), "{joined}");
    }

    #[test]
    fn custom_domains_are_off_by_default_and_checked_when_on() {
        let config = HarborConfig::parse(MINIMAL, Path::new("harbor.toml")).unwrap();
        assert_eq!(config.web.custom_domains, CustomDomainsConfig::default());
        assert!(!config.web.custom_domains.enabled);
        assert_eq!(config.web.custom_domains.ask_path, "/_domains/allow");
        assert_eq!(config.web.custom_domains.to, CustomDomainsTo::App);
        assert!(
            problems(&format!(
                "{MINIMAL}\n[web.custom_domains]\nenabled = true\n"
            ))
            .is_empty()
        );
        let found = problems(&format!(
            "{MINIMAL}\n[web.custom_domains]\nenabled = true\nask_path = \"allow\"\nto = \"frontend\"\n"
        ));
        assert_eq!(
            found,
            [
                "`web.custom_domains.ask_path` must be a path such as `/_domains/allow` (got `allow`)",
                "`web.custom_domains.to = \"frontend\"` needs a [frontend]",
            ]
        );
        assert!(
            HarborConfig::parse(
                &format!("{MINIMAL}\n[web.custom_domains]\nenable = true\n"),
                Path::new("harbor.toml")
            )
            .is_err()
        );
    }

    #[test]
    fn proxy_protocol_needs_trusted_proxies() {
        let found = problems(&format!("{MINIMAL}\n[web]\nproxy_protocol = true\n"));
        assert_eq!(
            found,
            ["web.proxy_protocol needs deploy.trusted_proxies: the load balancer's addresses"]
        );
    }

    #[test]
    fn frontend_defaults() {
        let config = parse(&format!("{MINIMAL}\n[frontend]\npath = \"web\"\n"));
        let frontend = config.frontend.as_ref().unwrap();
        assert_eq!(frontend.path, "web");
        assert_eq!(frontend.build, None);
        assert_eq!(frontend.command, None);
        assert_eq!(frontend.health_check, "/");
        assert_eq!(frontend.ports, [3001, 3002]);
        assert_eq!(frontend.node_version, 24);
        assert!(config.validate().is_empty());
    }

    #[test]
    fn environments_name_their_files() {
        let production = Environment::select(None, None).unwrap();
        assert_eq!(production.name(), "production");
        assert_eq!(production.config_file(), "harbor.toml");
        assert_eq!(production.key_file(), "harbor.key");
        assert_eq!(production.flag(), "");
        let staging = Environment::select(Some("staging"), Some("qa")).unwrap();
        assert_eq!(staging.name(), "staging");
        assert_eq!(staging.config_file(), "harbor.staging.toml");
        assert_eq!(staging.key_file(), "harbor.staging.key");
        assert_eq!(staging.flag(), " --env staging");
        assert_eq!(Environment::select(None, Some("qa")).unwrap().name(), "qa");
        assert_eq!(Environment::named("production").unwrap(), production);
        assert!(Environment::named("../x").is_err());
    }

    #[test]
    fn address_checks() {
        assert!(is_ip_or_cidr("10.0.0.1"));
        assert!(is_ip_or_cidr("10.0.0.0/24"));
        assert!(is_ip_or_cidr("::1"));
        assert!(is_ip_or_cidr("fd00::/8"));
        assert!(!is_ip_or_cidr("10.0.0.0/33"));
        assert!(!is_ip_or_cidr("example.com"));
        assert!(is_public_key(
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJz me@laptop"
        ));
        assert!(is_public_key(
            "ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTY="
        ));
        assert!(!is_public_key("ssh-ed25519"));
        assert!(!is_public_key("hello world"));
        assert!(is_domain("crm.example.com"));
        assert!(!is_domain("localhost"));
        assert!(!is_domain("-a.example.com"));
    }
}
