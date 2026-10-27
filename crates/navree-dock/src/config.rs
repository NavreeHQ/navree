//! `dock.toml`: which services an app runs, read and checked.

use std::collections::BTreeMap;
use std::path::Path;

use toml::Value;

use crate::error::DockError;
use crate::runtime::RuntimeChoice;
use crate::service::{
    self, HOST, POSTGRES_VERSION, Port, Service, ServiceOptions, ServiceSpec, TIMESCALEDB_IMAGE,
    TIMESCALEDB_POSTGRES_VERSIONS, TIMESCALEDB_VERSION,
};
use crate::template;

/// The file's name, next to the app's `Cargo.toml`.
pub const DOCK_FILE: &str = "dock.toml";

/// The top-level keys of `dock.toml`.
const TOP_KEYS: &[&str] = &["project", "runtime", "write_env", "services"];

/// The keys every service takes.
const COMMON_KEYS: &[&str] = &["image", "version", "port", "env"];

/// A parsed `dock.toml`, with every default filled in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DockFile {
    /// The prefix of container and volume names (default: the package name).
    pub project: String,
    /// Which runtime to drive.
    pub runtime: RuntimeChoice,
    /// Whether `navree dock up` writes the settings into `.env`.
    pub write_env: bool,
    /// The services, in file order.
    pub services: Vec<ServiceSpec>,
}

impl DockFile {
    /// Reads `<root>/dock.toml`; the package name comes from
    /// `<root>/Cargo.toml` (or the directory's name).
    pub fn load(root: &Path) -> Result<DockFile, DockError> {
        let path = root.join(DOCK_FILE);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(DockError::NoDockFile(root.to_path_buf()));
            }
            Err(error) => return Err(DockError::io(path, error)),
        };
        DockFile::parse(&text, &package_name(root))
    }

    /// Parses the text of a `dock.toml` for the package `package`.
    pub fn parse(text: &str, package: &str) -> Result<DockFile, DockError> {
        let document: toml_edit::DocumentMut =
            text.parse().map_err(|e: toml_edit::TomlError| {
                DockError::config(e.to_string().trim().to_owned())
            })?;
        let table: toml::Table =
            toml::from_str(text).map_err(|e| DockError::config(e.to_string().trim().to_owned()))?;

        for key in table.keys() {
            if !TOP_KEYS.contains(&key.as_str()) {
                return Err(DockError::config(format!(
                    "unknown key `{key}`; the top-level keys are project, runtime, write_env and services"
                )));
            }
        }
        let project = match table.get("project") {
            Some(value) => {
                let project =
                    string(value).ok_or_else(|| DockError::config("`project` must be a string"))?;
                if !is_project_name(&project) {
                    return Err(DockError::config(format!(
                        "`project` must use lower-case letters, digits, `-` and `_`, starting with a letter or digit, not \"{project}\""
                    )));
                }
                project
            }
            None => default_project(package),
        };
        let runtime = match table.get("runtime") {
            Some(value) => string(value)
                .ok_or_else(|| DockError::config("`runtime` must be a string"))?
                .parse()
                .map_err(DockError::config)?,
            None => RuntimeChoice::Auto,
        };
        let write_env = match table.get("write_env") {
            Some(Value::Boolean(write)) => *write,
            Some(_) => return Err(DockError::config("`write_env` must be true or false")),
            None => true,
        };

        let services_table = match table.get("services") {
            None => toml::Table::new(),
            Some(Value::Table(services)) => services.clone(),
            Some(_) => {
                return Err(DockError::config(
                    "`services` must be a table of services, such as [services.postgres]",
                ));
            }
        };
        // toml::Table is sorted; toml_edit keeps the file's order.
        let order: Vec<String> = document
            .get("services")
            .and_then(toml_edit::Item::as_table_like)
            .map(|services| services.iter().map(|(name, _)| name.to_owned()).collect())
            .unwrap_or_default();

        let mut services = Vec::new();
        for name in order {
            let service: Service = name.parse().map_err(|_| {
                DockError::config(format!(
                    "[services.{name}]: unknown service; Dock runs postgres, redis, meilisearch, mailpit, minio and nats"
                ))
            })?;
            let settings = match &services_table[&name] {
                Value::Table(settings) => settings,
                _ => {
                    return Err(DockError::config(format!(
                        "`services.{name}` must be a table ([services.{name}])"
                    )));
                }
            };
            let env_order: Vec<String> = document
                .get("services")
                .and_then(|services| services.get(&name))
                .and_then(|item| item.get("env"))
                .and_then(toml_edit::Item::as_table_like)
                .map(|env| env.iter().map(|(key, _)| key.to_owned()).collect())
                .unwrap_or_default();
            services.push(
                build(service, settings, &env_order, &project, package).map_err(|message| {
                    DockError::config(format!("[services.{name}]: {message}"))
                })?,
            );
        }
        check_unique_ports(&services)?;
        check_unique_env(&services)?;
        Ok(DockFile {
            project,
            runtime,
            write_env,
            services,
        })
    }

    /// The service `name`, which must be in the file.
    pub fn service(&self, name: &str) -> Result<&ServiceSpec, DockError> {
        let service: Service = name.parse()?;
        self.services
            .iter()
            .find(|spec| spec.service == service)
            .ok_or_else(|| DockError::NotInDockFile(name.to_owned()))
    }

    /// The services named (each once, in file order), or every service
    /// when `names` is empty.
    pub fn select(&self, names: &[String]) -> Result<Vec<&ServiceSpec>, DockError> {
        if names.is_empty() {
            return Ok(self.services.iter().collect());
        }
        let mut wanted = Vec::new();
        for name in names {
            wanted.push(self.service(name)?.service);
        }
        Ok(self
            .services
            .iter()
            .filter(|spec| wanted.contains(&spec.service))
            .collect())
    }

    /// Every setting the app gets, in file order.
    pub fn env(&self) -> Vec<(String, String)> {
        self.services
            .iter()
            .flat_map(|spec| spec.env.iter().cloned())
            .collect()
    }

    /// The name of the compose project's network (`<project>_default`).
    pub fn network(&self) -> String {
        format!("{}_default", self.project)
    }
}

/// The package name in `<root>/Cargo.toml`, else the directory's name.
pub fn package_name(root: &Path) -> String {
    let from_manifest = std::fs::read_to_string(root.join("Cargo.toml"))
        .ok()
        .and_then(|text| text.parse::<toml::Table>().ok())
        .and_then(|manifest| {
            manifest
                .get("package")?
                .get("name")?
                .as_str()
                .map(str::to_owned)
        });
    from_manifest
        .or_else(|| {
            std::path::absolute(root)
                .ok()?
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "app".to_owned())
}

fn string(value: &Value) -> Option<String> {
    value.as_str().map(str::to_owned)
}

/// Lower-case letters, digits, `-` and `_`, starting with a letter or digit:
/// what both container and compose project names accept.
fn is_project_name(name: &str) -> bool {
    name.chars()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// The package name, lower-cased, with other characters replaced by `-`.
fn default_project(package: &str) -> String {
    let mut project: String = package
        .to_ascii_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    project = project
        .trim_start_matches(|c: char| !c.is_ascii_alphanumeric())
        .to_owned();
    if project.is_empty() {
        "app".to_owned()
    } else {
        project
    }
}

/// The package name as a PostgreSQL database name: `crm-api` -> `crm_api`.
fn default_database(package: &str) -> String {
    let name: String = package
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if name.starts_with(|c: char| c.is_ascii_digit()) || name.is_empty() {
        format!("_{name}")
    } else {
        name
    }
}

/// The package name as a bucket name: `crm_api` -> `crm-api`.
fn default_bucket(package: &str) -> String {
    let mut name: String = package
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    name.truncate(63);
    let name = name.trim_matches('-').to_owned();
    if name.len() < 3 {
        format!("{name}-bucket").trim_start_matches('-').to_owned()
    } else {
        name
    }
}

fn is_credential(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '~' | '-'))
}

fn is_identifier(value: &str, max: usize) -> bool {
    value.len() <= max
        && value
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn is_bucket(value: &str) -> bool {
    (3..=63).contains(&value.len())
        && value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.')
        && value.starts_with(|c: char| c.is_ascii_alphanumeric())
        && value.ends_with(|c: char| c.is_ascii_alphanumeric())
}

fn is_image_part(value: &str, extra: &[char]) -> bool {
    value
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric())
        && value.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') || extra.contains(&c)
        })
}

fn is_env_name(name: &str) -> bool {
    name.chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The service's keys, read and checked; errors are messages for
/// `[services.<name>]`.
struct Keys<'a> {
    table: &'a toml::Table,
}

impl Keys<'_> {
    fn string(&self, key: &str) -> Result<Option<String>, String> {
        match self.table.get(key) {
            None => Ok(None),
            Some(Value::String(value)) => Ok(Some(value.clone())),
            Some(_) => Err(format!("`{key}` must be a string")),
        }
    }

    fn credential(&self, key: &str, default: &str) -> Result<String, String> {
        let value = self.string(key)?.unwrap_or_else(|| default.to_owned());
        if is_credential(&value) {
            Ok(value)
        } else {
            Err(format!(
                "`{key}` may use letters, digits, `.`, `_`, `~` and `-`, and may not be empty"
            ))
        }
    }

    fn port(&self, key: &str) -> Result<Option<u16>, String> {
        match self.table.get(key) {
            None => Ok(None),
            Some(Value::Integer(port)) => u16::try_from(*port)
                .ok()
                .filter(|port| *port > 0)
                .map(Some)
                .ok_or_else(|| format!("`{key}` must be a port number from 1 to 65535")),
            Some(_) => Err(format!("`{key}` must be a port number from 1 to 65535")),
        }
    }

    fn bool(&self, key: &str, default: bool) -> Result<bool, String> {
        match self.table.get(key) {
            None => Ok(default),
            Some(Value::Boolean(value)) => Ok(*value),
            Some(_) => Err(format!("`{key}` must be true or false")),
        }
    }

    fn strings(&self, key: &str) -> Result<Vec<String>, String> {
        match self.table.get(key) {
            None => Ok(Vec::new()),
            Some(Value::Array(items)) => items
                .iter()
                .map(|item| {
                    item.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| format!("`{key}` must be a list of strings"))
                })
                .collect(),
            Some(_) => Err(format!("`{key}` must be a list of strings")),
        }
    }
}

/// Resolves one service of `dock.toml`.
fn build(
    service: Service,
    table: &toml::Table,
    env_order: &[String],
    project: &str,
    package: &str,
) -> Result<ServiceSpec, String> {
    for key in table.keys() {
        if !COMMON_KEYS.contains(&key.as_str()) && !service.keys().contains(&key.as_str()) {
            let mut known: Vec<&str> = COMMON_KEYS.to_vec();
            known.extend(service.keys());
            return Err(format!(
                "unknown key `{key}`; {service} takes {}",
                known.join(", ")
            ));
        }
    }
    let keys = Keys { table };

    let image = keys.string("image")?;
    let version = keys.string("version")?;
    if let Some(image) = &image
        && !is_image_part(image, &['/', ':', '@'])
    {
        return Err(format!("`image` is not an image reference: \"{image}\""));
    }
    if let Some(version) = &version
        && !is_image_part(version, &[])
    {
        return Err(format!("`version` is not an image tag: \"{version}\""));
    }

    let mut port = keys.port("port")?;
    if service == Service::Mailpit
        && let Some(smtp) = keys.port("smtp_port")?
    {
        if port.is_some_and(|port| port != smtp) {
            return Err("set either `port` or `smtp_port` (the same SMTP port), not both".into());
        }
        port = Some(smtp);
    }
    let main = Port {
        key: "port",
        host: port.unwrap_or(service.main_port()),
        container: service.main_port(),
    };
    let second = |key: &'static str, container: u16| -> Result<Port, String> {
        Ok(Port {
            key,
            host: keys.port(key)?.unwrap_or(container),
            container,
        })
    };

    let (options, ports) = match service {
        Service::Postgres => {
            let database = keys
                .string("database")?
                .unwrap_or_else(|| default_database(package));
            if !is_identifier(&database, 63) {
                return Err(format!(
                    "`database` must be letters, digits and `_`, starting with a letter or `_`, not \"{database}\""
                ));
            }
            let extensions = keys.strings("extensions")?;
            if let Some(bad) = extensions.iter().find(|e| !is_identifier(e, 63)) {
                return Err(format!("`extensions` has an invalid name: \"{bad}\""));
            }
            // TimescaleDB is the default database; an image of your own
            // runs it only when it is a TimescaleDB image, unless the file
            // says otherwise.
            let timescaledb = keys.bool(
                "timescaledb",
                image.as_deref().is_none_or(service::has_timescaledb),
            )?;
            let options = ServiceOptions::Postgres {
                database,
                user: keys.credential("user", "navree")?,
                password: keys.credential("password", "navree")?,
                extensions,
                timescaledb,
            };
            (options, vec![main])
        }
        Service::Redis => {
            let maxmemory_mb = match table.get("maxmemory_mb") {
                None => 256,
                Some(Value::Integer(mb)) if *mb > 0 => {
                    u32::try_from(*mb).map_err(|_| "`maxmemory_mb` is too large".to_owned())?
                }
                Some(_) => return Err("`maxmemory_mb` must be a positive number".into()),
            };
            (ServiceOptions::Redis { maxmemory_mb }, vec![main])
        }
        Service::Meilisearch => (
            ServiceOptions::Meilisearch {
                master_key: keys.credential("master_key", "dock-meilisearch-master-key")?,
            },
            vec![main],
        ),
        Service::Mailpit => (
            ServiceOptions::Mailpit,
            vec![main, second("ui_port", 8025)?],
        ),
        Service::Minio => {
            let bucket = keys
                .string("bucket")?
                .unwrap_or_else(|| default_bucket(package));
            if !is_bucket(&bucket) {
                return Err(format!(
                    "`bucket` must be 3 to 63 lower-case letters, digits, `-` and `.`, not \"{bucket}\""
                ));
            }
            let access_key = keys.credential("access_key", "navree")?;
            let secret_key = keys.credential("secret_key", "navree-secret")?;
            if access_key.len() < 3 {
                return Err("`access_key` must be at least 3 characters".into());
            }
            if secret_key.len() < 8 {
                return Err("`secret_key` must be at least 8 characters".into());
            }
            let options = ServiceOptions::Minio {
                bucket,
                access_key,
                secret_key,
            };
            (options, vec![main, second("console_port", 9001)?])
        }
        Service::Nats => (
            ServiceOptions::Nats {
                jetstream: keys.bool("jetstream", true)?,
            },
            vec![main, second("monitor_port", 8222)?],
        ),
    };

    let image = resolve_image(service, image, version, &options)?;
    let mut spec = ServiceSpec {
        service,
        image,
        container: format!("{project}-{}", service.name()),
        volume: format!("{project}-{}-data", service.name()),
        ports,
        env: Vec::new(),
        options,
    };
    spec.env = settings(&spec, table.get("env"))?;
    // In the file's order (toml::Table is sorted by key).
    spec.env.sort_by_key(|(name, _)| {
        env_order
            .iter()
            .position(|key| key == name)
            .unwrap_or(usize::MAX)
    });
    Ok(spec)
}

/// The image reference: `image` (with `version` as its tag), or the
/// service's default repository with `version` or the default tag.
fn resolve_image(
    service: Service,
    image: Option<String>,
    version: Option<String>,
    options: &ServiceOptions,
) -> Result<String, String> {
    if let Some(image) = image {
        return match version {
            None => Ok(image),
            Some(_) if service::image_tag(&image).is_some() || image.contains('@') => Err(
                "`image` already has a tag; set the tag in `image` or in `version`, not both"
                    .into(),
            ),
            Some(version) => Ok(format!("{image}:{version}")),
        };
    }
    if let ServiceOptions::Postgres {
        extensions,
        timescaledb,
        ..
    } = options
    {
        return postgres_image(version, extensions, *timescaledb);
    }
    let (repository, default_tag) = service.default_image();
    let tag = version.unwrap_or_else(|| default_tag.to_owned());
    Ok(format!("{repository}:{tag}"))
}

/// PostgreSQL's image for `version`: TimescaleDB's for that major version
/// (it has pgvector too), or, without TimescaleDB, `postgres` (or
/// `pgvector/pgvector` with the `vector` extension).
fn postgres_image(
    version: Option<String>,
    extensions: &[String],
    timescaledb: bool,
) -> Result<String, String> {
    let version = version.unwrap_or_else(|| POSTGRES_VERSION.to_owned());
    if timescaledb {
        let supported = TIMESCALEDB_POSTGRES_VERSIONS.map(|major| major.to_string());
        return match version.parse::<u32>() {
            Ok(major) if TIMESCALEDB_POSTGRES_VERSIONS.contains(&major) => Ok(format!(
                "{TIMESCALEDB_IMAGE}:{}",
                service::timescaledb_tag(major)
            )),
            _ => Err(format!(
                "with TimescaleDB, `version` is the PostgreSQL major version: {} (the versions TimescaleDB {TIMESCALEDB_VERSION} runs on), not \"{version}\"; set `timescaledb = false` for plain PostgreSQL {version}",
                join_or(&supported)
            )),
        };
    }
    if extensions.iter().any(|e| e == "vector") {
        let major: String = version.chars().take_while(char::is_ascii_digit).collect();
        if major.is_empty() {
            return Err(format!(
                "with the `vector` extension, `version` must start with the PostgreSQL major version, not \"{version}\""
            ));
        }
        return Ok(format!("pgvector/pgvector:pg{major}"));
    }
    Ok(format!("postgres:{version}"))
}

/// `a, b or c`.
fn join_or(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} or {last}", init.join(", ")),
    }
}

/// The settings the app gets: the service's `env` table rendered, or its
/// defaults.
fn settings(spec: &ServiceSpec, env: Option<&Value>) -> Result<Vec<(String, String)>, String> {
    let entries: Vec<(String, String)> = match env {
        None => spec
            .service
            .default_env()
            .iter()
            .map(|(name, template)| ((*name).to_owned(), (*template).to_owned()))
            .collect(),
        Some(Value::Table(table)) => {
            let mut entries = Vec::new();
            for (name, value) in table {
                if !is_env_name(name) {
                    return Err(format!(
                        "`env` key \"{name}\" is not an environment variable name"
                    ));
                }
                let template = value
                    .as_str()
                    .ok_or_else(|| format!("`env.{name}` must be a string"))?;
                if template.contains(['\n', '\r', '\0']) {
                    return Err(format!("`env.{name}` may not contain line breaks"));
                }
                entries.push((name.clone(), template.to_owned()));
            }
            entries
        }
        Some(_) => {
            return Err(
                "`env` must be a table of variables, such as env = { NAME = \"{host}:{port}\" }"
                    .into(),
            );
        }
    };
    let allowed = spec.service.placeholders();
    entries
        .into_iter()
        .map(|(name, value)| {
            let rendered = template::render(&value, |placeholder| {
                if allowed.contains(&placeholder) {
                    placeholder_value(spec, placeholder)
                } else {
                    None
                }
            })
            .map_err(|unknown| {
                let known: Vec<String> = allowed.iter().map(|p| format!("{{{p}}}")).collect();
                format!(
                    "`env.{name}` uses {{{unknown}}}, which {} does not have; it has {}",
                    spec.service,
                    known.join(", ")
                )
            })?;
            Ok((name, rendered))
        })
        .collect()
}

fn placeholder_value(spec: &ServiceSpec, name: &str) -> Option<String> {
    let value = match (name, &spec.options) {
        ("host", _) => HOST.to_owned(),
        ("port", _) => spec.port().to_string(),
        ("user", ServiceOptions::Postgres { user, .. }) => user.clone(),
        ("password", ServiceOptions::Postgres { password, .. }) => password.clone(),
        ("database", ServiceOptions::Postgres { database, .. }) => database.clone(),
        ("key", ServiceOptions::Meilisearch { master_key }) => master_key.clone(),
        ("user", ServiceOptions::Minio { access_key, .. }) => access_key.clone(),
        ("password", ServiceOptions::Minio { secret_key, .. }) => secret_key.clone(),
        ("bucket", ServiceOptions::Minio { bucket, .. }) => bucket.clone(),
        ("ui_port" | "console_port" | "monitor_port", _) => spec.port_of(name)?.to_string(),
        _ => return None,
    };
    Some(value)
}

fn check_unique_ports(services: &[ServiceSpec]) -> Result<(), DockError> {
    let mut seen: BTreeMap<u16, String> = BTreeMap::new();
    for spec in services {
        for port in &spec.ports {
            let owner = format!("{}'s `{}`", spec.service, port.key);
            if let Some(first) = seen.insert(port.host, owner.clone()) {
                return Err(DockError::config(format!(
                    "port {} is used twice: by {first} and by {owner}",
                    port.host
                )));
            }
        }
    }
    Ok(())
}

fn check_unique_env(services: &[ServiceSpec]) -> Result<(), DockError> {
    let mut seen: BTreeMap<&str, &str> = BTreeMap::new();
    for spec in services {
        for (name, _) in &spec.env {
            if let Some(first) = seen.insert(name, spec.name()) {
                return Err(DockError::config(format!(
                    "{name} is set by both {first} and {}; keep it in one service's `env`",
                    spec.name()
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> DockFile {
        DockFile::parse(text, "crm-api").unwrap_or_else(|e| panic!("{e}"))
    }

    fn error(text: &str) -> String {
        DockFile::parse(text, "crm-api").unwrap_err().to_string()
    }

    fn env(spec: &ServiceSpec) -> Vec<(&str, &str)> {
        spec.env
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect()
    }

    #[test]
    fn empty_tables_run_with_their_defaults_in_file_order() {
        let file = parse(
            "[services.redis]\n[services.postgres]\n[services.mailpit]\n[services.meilisearch]\n[services.minio]\n[services.nats]\n",
        );
        assert_eq!(file.project, "crm-api");
        assert_eq!(file.runtime, RuntimeChoice::Auto);
        assert!(file.write_env);
        let names: Vec<&str> = file.services.iter().map(ServiceSpec::name).collect();
        assert_eq!(
            names,
            [
                "redis",
                "postgres",
                "mailpit",
                "meilisearch",
                "minio",
                "nats"
            ]
        );
        let postgres = file.service("postgres").unwrap();
        assert_eq!(postgres.image, "timescale/timescaledb:2.31.0-pg17");
        assert_eq!(postgres.container, "crm-api-postgres");
        assert_eq!(postgres.volume, "crm-api-postgres-data");
        assert_eq!(
            env(postgres),
            [(
                "APP_DATABASE__URL",
                "postgres://navree:navree@127.0.0.1:5432/crm_api"
            )]
        );
        assert_eq!(
            env(file.service("redis").unwrap()),
            [("REDIS_URL", "redis://127.0.0.1:6379")]
        );
        assert_eq!(
            env(file.service("meilisearch").unwrap()),
            [
                ("APP_SEARCH__ENGINE", "meilisearch"),
                ("APP_SEARCH__URL", "http://127.0.0.1:7700"),
                ("APP_SEARCH__KEY", "dock-meilisearch-master-key"),
            ]
        );
        assert_eq!(
            env(file.service("mailpit").unwrap()),
            [("APP_MAIL__TRANSPORT", "smtp://127.0.0.1:1025")]
        );
        assert_eq!(
            env(file.service("minio").unwrap()),
            [
                ("AWS_ENDPOINT_URL", "http://127.0.0.1:9000"),
                ("AWS_ACCESS_KEY_ID", "navree"),
                ("AWS_SECRET_ACCESS_KEY", "navree-secret"),
                ("AWS_REGION", "us-east-1"),
            ]
        );
        assert_eq!(
            env(file.service("nats").unwrap()),
            [
                ("APP_MICRO__TRANSPORT", "nats"),
                ("APP_MICRO__URL", "nats://127.0.0.1:4222"),
            ]
        );
        let minio = file.service("minio").unwrap();
        assert_eq!(
            minio.options,
            ServiceOptions::Minio {
                bucket: "crm-api".into(),
                access_key: "navree".into(),
                secret_key: "navree-secret".into(),
            }
        );
        assert_eq!(minio.addresses(), ["127.0.0.1:9000", "127.0.0.1:9001"]);
    }

    #[test]
    fn top_level_keys() {
        let file = parse("project = \"shop\"\nruntime = \"podman\"\nwrite_env = false\n");
        assert_eq!(file.project, "shop");
        assert_eq!(file.runtime, RuntimeChoice::Podman);
        assert!(!file.write_env);
        assert!(file.services.is_empty());
        assert!(error("projects = \"x\"").contains("unknown key `projects`"));
        assert!(error("project = \"Shop App\"").contains("`project` must use lower-case"));
        assert!(error("runtime = \"lxc\"").contains("`runtime` must be"));
        assert!(error("write_env = \"yes\"").contains("`write_env` must be true or false"));
        assert!(error("services = 3").contains("`services` must be a table"));
    }

    #[test]
    fn the_project_prefixes_names() {
        let file = parse("project = \"shop\"\n[services.postgres]\n");
        let postgres = file.service("postgres").unwrap();
        assert_eq!(postgres.container, "shop-postgres");
        assert_eq!(postgres.volume, "shop-postgres-data");
        // The database still follows the package.
        assert!(postgres.env[0].1.ends_with("/crm_api"));
        assert_eq!(file.network(), "shop_default");
    }

    #[test]
    fn package_names_become_valid_defaults() {
        let file = DockFile::parse("[services.postgres]\n[services.minio]\n", "My_App").unwrap();
        assert_eq!(file.project, "my_app");
        assert!(file.services[0].env[0].1.ends_with("/my_app"));
        assert!(matches!(
            &file.services[1].options,
            ServiceOptions::Minio { bucket, .. } if bucket == "my-app"
        ));
    }

    #[test]
    fn postgres_runs_timescaledb_by_default() {
        let file = parse("[services.postgres]\n");
        let postgres = &file.services[0];
        assert_eq!(postgres.image, "timescale/timescaledb:2.31.0-pg17");
        assert_eq!(
            postgres.options,
            ServiceOptions::Postgres {
                database: "crm_api".into(),
                user: "navree".into(),
                password: "navree".into(),
                extensions: vec![],
                timescaledb: true,
            }
        );
        assert_eq!(postgres.data_path(), "/var/lib/postgresql/data");
        // `version` is the PostgreSQL major version under TimescaleDB.
        let pg16 = parse("[services.postgres]\nversion = \"16\"\n");
        assert_eq!(pg16.services[0].image, "timescale/timescaledb:2.31.0-pg16");
        let pg18 = parse("[services.postgres]\nversion = \"18\"\n");
        assert_eq!(pg18.services[0].image, "timescale/timescaledb:2.31.0-pg18");
        assert_eq!(pg18.services[0].data_path(), "/var/lib/postgresql");
        // The TimescaleDB image has pgvector too.
        let vector = parse("[services.postgres]\nextensions = [\"vector\"]\n");
        assert_eq!(
            vector.services[0].image,
            "timescale/timescaledb:2.31.0-pg17"
        );
        assert_eq!(
            error("[services.postgres]\nversion = \"15\"\n"),
            "dock.toml: [services.postgres]: with TimescaleDB, `version` is the PostgreSQL major version: 16, 17 or 18 (the versions TimescaleDB 2.31.0 runs on), not \"15\"; set `timescaledb = false` for plain PostgreSQL 15"
        );
        assert!(
            error("[services.postgres]\nversion = \"17.6\"\n")
                .contains("`version` is the PostgreSQL major version"),
        );
        assert!(
            error("[services.postgres]\ntimescaledb = \"yes\"\n")
                .contains("`timescaledb` must be true or false")
        );
    }

    #[test]
    fn plain_postgres_is_an_option() {
        let file = parse("[services.postgres]\ntimescaledb = false\n");
        let postgres = &file.services[0];
        assert_eq!(postgres.image, "postgres:17");
        assert!(matches!(
            postgres.options,
            ServiceOptions::Postgres {
                timescaledb: false,
                ..
            }
        ));
        let tagged = parse("[services.postgres]\ntimescaledb = false\nversion = \"16.4-alpine\"\n");
        assert_eq!(tagged.services[0].image, "postgres:16.4-alpine");
        assert_eq!(
            parse("[services.postgres]\ntimescaledb = false\nversion = \"15\"\n").services[0].image,
            "postgres:15"
        );
        // An image of your own runs as named; TimescaleDB is on for
        // TimescaleDB images only, unless the file says otherwise.
        let postgis = parse("[services.postgres]\nimage = \"postgis/postgis:17-3.5\"\n");
        assert_eq!(postgis.services[0].image, "postgis/postgis:17-3.5");
        assert!(matches!(
            postgis.services[0].options,
            ServiceOptions::Postgres {
                timescaledb: false,
                ..
            }
        ));
        let ha = parse("[services.postgres]\nimage = \"timescale/timescaledb-ha:pg17\"\n");
        assert!(matches!(
            ha.services[0].options,
            ServiceOptions::Postgres {
                timescaledb: true,
                ..
            }
        ));
        let declared = parse(
            "[services.postgres]\nimage = \"registry.example.com/db:17\"\ntimescaledb = true\n",
        );
        assert!(matches!(
            declared.services[0].options,
            ServiceOptions::Postgres {
                timescaledb: true,
                ..
            }
        ));
    }

    #[test]
    fn postgres_settings_and_the_vector_image() {
        let file = parse(
            "[services.postgres]\ntimescaledb = false\nversion = \"16\"\ndatabase = \"crm\"\nuser = \"crm\"\npassword = \"s3cret\"\nport = 5433\nextensions = [\"vector\", \"pg_trgm\"]\n",
        );
        let postgres = &file.services[0];
        assert_eq!(postgres.image, "pgvector/pgvector:pg16");
        assert_eq!(postgres.port(), 5433);
        assert_eq!(
            postgres.env[0].1,
            "postgres://crm:s3cret@127.0.0.1:5433/crm"
        );
        assert_eq!(postgres.data_path(), "/var/lib/postgresql/data");
        let latest = parse("[services.postgres]\ntimescaledb = false\nversion = \"18\"\n");
        assert_eq!(latest.services[0].image, "postgres:18");
        assert_eq!(latest.services[0].data_path(), "/var/lib/postgresql");
        assert!(
            error("[services.postgres]\ntimescaledb = false\nversion = \"latest\"\nextensions = [\"vector\"]\n")
                .contains("must start with the PostgreSQL major version")
        );
        assert!(
            error("[services.postgres]\ndatabase = \"crm-api\"\n").contains("`database` must be")
        );
        assert!(error("[services.postgres]\npassword = \"p@ss\"\n").contains("`password` may use"));
        assert!(
            error("[services.postgres]\nextensions = [\"vector; drop\"]\n")
                .contains("invalid name")
        );
    }

    #[test]
    fn image_and_version() {
        let custom = parse("[services.redis]\nimage = \"valkey/valkey:8\"\n");
        assert_eq!(custom.services[0].image, "valkey/valkey:8");
        let tagged = parse("[services.redis]\nimage = \"valkey/valkey\"\nversion = \"8\"\n");
        assert_eq!(tagged.services[0].image, "valkey/valkey:8");
        let registry =
            parse("[services.redis]\nimage = \"localhost:5000/redis\"\nversion = \"7\"\n");
        assert_eq!(registry.services[0].image, "localhost:5000/redis:7");
        let version = parse("[services.meilisearch]\nversion = \"v1.20\"\n");
        assert_eq!(version.services[0].image, "getmeili/meilisearch:v1.20");
        assert!(
            error("[services.redis]\nimage = \"valkey/valkey:8\"\nversion = \"8\"\n")
                .contains("already has a tag")
        );
        assert!(
            error("[services.redis]\nimage = \"redis 7\"\n").contains("not an image reference")
        );
        assert!(error("[services.redis]\nversion = \"7;rm\"\n").contains("not an image tag"));
    }

    #[test]
    fn service_specific_keys() {
        let file = parse(
            "[services.redis]\nmaxmemory_mb = 64\n[services.meilisearch]\nmaster_key = \"abc-123\"\n[services.mailpit]\nsmtp_port = 2525\nui_port = 8026\n[services.minio]\nbucket = \"crm-uploads\"\naccess_key = \"admin\"\nsecret_key = \"admin-secret\"\nconsole_port = 9091\n[services.nats]\njetstream = false\nmonitor_port = 8223\n",
        );
        assert_eq!(
            file.services[0].options,
            ServiceOptions::Redis { maxmemory_mb: 64 }
        );
        assert_eq!(file.services[1].env[2].1, "abc-123");
        assert_eq!(
            file.services[2].addresses(),
            ["127.0.0.1:2525", "127.0.0.1:8026"]
        );
        assert_eq!(file.services[2].env[0].1, "smtp://127.0.0.1:2525");
        assert_eq!(file.services[3].port_of("console_port"), Some(9091));
        assert_eq!(file.services[3].env[1].1, "admin");
        assert_eq!(
            file.services[4].options,
            ServiceOptions::Nats { jetstream: false }
        );
        assert_eq!(file.services[4].port_of("monitor_port"), Some(8223));
        assert!(error("[services.mailpit]\nport = 2525\nsmtp_port = 2526\n").contains("not both"));
        assert_eq!(
            parse("[services.mailpit]\nport = 2525\nsmtp_port = 2525\n").services[0].port(),
            2525
        );
        assert!(error("[services.minio]\nsecret_key = \"short\"\n").contains("at least 8"));
        assert!(error("[services.minio]\nbucket = \"Crm_Uploads\"\n").contains("`bucket` must be"));
        assert!(error("[services.redis]\nmaxmemory_mb = 0\n").contains("positive number"));
        assert!(error("[services.nats]\njetstream = \"yes\"\n").contains("true or false"));
    }

    #[test]
    fn unknown_services_and_keys_are_named() {
        assert_eq!(
            error("[services.mysql]\n"),
            "dock.toml: [services.mysql]: unknown service; Dock runs postgres, redis, meilisearch, mailpit, minio and nats"
        );
        assert_eq!(
            error("[services.redis]\nbucket = \"x\"\n"),
            "dock.toml: [services.redis]: unknown key `bucket`; redis takes image, version, port, env, maxmemory_mb"
        );
        assert!(error("[services]\nredis = 1\n").contains("`services.redis` must be a table"));
        assert!(error("[services.redis]\nport = 70000\n").contains("from 1 to 65535"));
        assert!(error("[services.redis]\nport = \"6379\"\n").contains("from 1 to 65535"));
        assert!(error("[services.redis\n").starts_with("dock.toml: "));
    }

    #[test]
    fn ports_and_settings_must_be_unique() {
        assert_eq!(
            error("[services.mailpit]\n[services.nats]\nport = 8025\n"),
            "dock.toml: port 8025 is used twice: by mailpit's `ui_port` and by nats's `port`"
        );
        assert!(
            error(
                "[services.postgres]\n[services.redis]\nenv = { APP_DATABASE__URL = \"redis://{host}\" }\n"
            )
            .contains("APP_DATABASE__URL is set by both postgres and redis")
        );
    }

    #[test]
    fn env_tables_replace_the_settings() {
        let file = parse(
            r#"
[services.postgres]
database = "crm_api"
env = { APP_DATABASE__URL = "postgres://{user}:{password}@{host}:{port}/{database}?application_name=crm-dev" }

[services.minio]
bucket = "crm-uploads"
env = { CRM_UPLOADS_ENDPOINT = "http://{host}:{port}", CRM_UPLOADS_BUCKET = "{bucket}" }
"#,
        );
        assert_eq!(
            env(&file.services[0]),
            [(
                "APP_DATABASE__URL",
                "postgres://navree:navree@127.0.0.1:5432/crm_api?application_name=crm-dev"
            )]
        );
        assert_eq!(
            env(&file.services[1]),
            [
                ("CRM_UPLOADS_ENDPOINT", "http://127.0.0.1:9000"),
                ("CRM_UPLOADS_BUCKET", "crm-uploads"),
            ]
        );
        assert_eq!(
            error("[services.redis]\nenv = { X = \"{database}\" }\n"),
            "dock.toml: [services.redis]: `env.X` uses {database}, which redis does not have; it has {host}, {port}"
        );
        assert!(
            error("[services.redis]\nenv = { \"1X\" = \"a\" }\n")
                .contains("not an environment variable name")
        );
        assert!(error("[services.redis]\nenv = { X = 1 }\n").contains("`env.X` must be a string"));
        assert!(error("[services.redis]\nenv = \"X=1\"\n").contains("`env` must be a table"));
        assert!(error("[services.redis]\nenv = { X = \"a\\nb\" }\n").contains("line breaks"));
        let meili = parse("[services.meilisearch]\nenv = { SEARCH = \"{host}:{port}/{key}\" }\n");
        assert_eq!(
            meili.services[0].env[0].1,
            "127.0.0.1:7700/dock-meilisearch-master-key"
        );
    }

    #[test]
    fn selecting_services() {
        let file = parse("[services.postgres]\n[services.redis]\n[services.nats]\n");
        let names = |selected: Vec<&ServiceSpec>| -> Vec<&'static str> {
            selected.iter().map(|s| s.name()).collect()
        };
        assert_eq!(
            names(file.select(&[]).unwrap()),
            ["postgres", "redis", "nats"]
        );
        assert_eq!(
            names(
                file.select(&["nats".into(), "postgres".into(), "nats".into()])
                    .unwrap()
            ),
            ["postgres", "nats"]
        );
        assert_eq!(
            file.select(&["mailpit".into()]).unwrap_err().to_string(),
            "mailpit is not in dock.toml; add it with `navree dock:add mailpit`"
        );
        assert!(
            file.select(&["mysql".into()])
                .unwrap_err()
                .to_string()
                .contains("unknown service")
        );
        assert_eq!(file.env().len(), 4);
    }

    #[test]
    fn load_reads_the_package_name_and_reports_a_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let error = DockFile::load(dir.path()).unwrap_err();
        assert!(matches!(error, DockError::NoDockFile(_)), "{error}");
        assert!(error.to_string().contains("navree dock:add <service>"));
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"billing-api\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(dir.path().join(DOCK_FILE), "[services.postgres]\n").unwrap();
        let file = DockFile::load(dir.path()).unwrap();
        assert_eq!(file.project, "billing-api");
        assert!(file.services[0].env[0].1.ends_with("/billing_api"));
    }
}
