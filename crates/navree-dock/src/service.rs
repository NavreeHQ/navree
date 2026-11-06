//! The six services Dock runs, and how each one becomes a container.

use std::fmt;
use std::str::FromStr;

use crate::error::DockError;

/// The address every port is bound to and every setting points at.
pub const HOST: &str = "127.0.0.1";

/// The image `[services.postgres]` runs by default: PostgreSQL with the
/// TimescaleDB extension (and pgvector), Navree's default database.
pub const TIMESCALEDB_IMAGE: &str = "timescale/timescaledb";

/// The TimescaleDB release of [`TIMESCALEDB_IMAGE`] Dock runs.
pub const TIMESCALEDB_VERSION: &str = "2.31.0";

/// The PostgreSQL major versions [`TIMESCALEDB_VERSION`]'s images exist
/// for: what `version` may be while TimescaleDB is on.
pub const TIMESCALEDB_POSTGRES_VERSIONS: [u32; 3] = [16, 17, 18];

/// The PostgreSQL major version Dock runs when `dock.toml` sets none.
pub const POSTGRES_VERSION: &str = "17";

/// The tag of the default image: [`TIMESCALEDB_VERSION`] on
/// [`POSTGRES_VERSION`] (a test keeps them in step).
const DEFAULT_TIMESCALEDB_TAG: &str = "2.31.0-pg17";

/// The tag of [`TIMESCALEDB_IMAGE`] for PostgreSQL `major`.
pub(crate) fn timescaledb_tag(major: u32) -> String {
    format!("{TIMESCALEDB_VERSION}-pg{major}")
}

/// A service Dock can run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Service {
    /// PostgreSQL with TimescaleDB ([`TIMESCALEDB_IMAGE`]), Navree's default
    /// database; with `timescaledb = false`, plain PostgreSQL (`postgres`,
    /// or `pgvector/pgvector` with the `vector` extension).
    Postgres,
    /// Redis.
    Redis,
    /// Meilisearch.
    Meilisearch,
    /// Mailpit: an SMTP server with a web inbox.
    Mailpit,
    /// MinIO: S3-compatible object storage (the maintained `pgsty/minio` build).
    Minio,
    /// NATS, with JetStream.
    Nats,
}

impl Service {
    /// Every service, in the order of the docs.
    pub const ALL: [Service; 6] = [
        Service::Postgres,
        Service::Redis,
        Service::Meilisearch,
        Service::Mailpit,
        Service::Minio,
        Service::Nats,
    ];

    /// Its name in `dock.toml` and on the command line.
    pub fn name(self) -> &'static str {
        match self {
            Service::Postgres => "postgres",
            Service::Redis => "redis",
            Service::Meilisearch => "meilisearch",
            Service::Mailpit => "mailpit",
            Service::Minio => "minio",
            Service::Nats => "nats",
        }
    }

    /// The image repository and tag Dock uses when `dock.toml` sets neither
    /// `image` nor `version` (for PostgreSQL, TimescaleDB on PostgreSQL
    /// [`POSTGRES_VERSION`]).
    pub fn default_image(self) -> (&'static str, &'static str) {
        match self {
            Service::Postgres => (TIMESCALEDB_IMAGE, DEFAULT_TIMESCALEDB_TAG),
            Service::Redis => ("redis", "7"),
            Service::Meilisearch => ("getmeili/meilisearch", "v1.55"),
            Service::Mailpit => ("axllent/mailpit", "v1.31"),
            Service::Minio => ("pgsty/minio", "RELEASE.2026-08-04T00-00-00Z"),
            Service::Nats => ("nats", "2.14"),
        }
    }

    /// Its keys in `dock.toml`, besides `image`, `version`, `port` and `env`.
    pub fn keys(self) -> &'static [&'static str] {
        match self {
            Service::Postgres => &["database", "user", "password", "extensions", "timescaledb"],
            Service::Redis => &["maxmemory_mb"],
            Service::Meilisearch => &["master_key"],
            Service::Mailpit => &["smtp_port", "ui_port"],
            Service::Minio => &["bucket", "access_key", "secret_key", "console_port"],
            Service::Nats => &["jetstream", "monitor_port"],
        }
    }

    /// The placeholders its `env` values may use.
    pub fn placeholders(self) -> &'static [&'static str] {
        match self {
            Service::Postgres => &["host", "port", "user", "password", "database"],
            Service::Redis => &["host", "port"],
            Service::Meilisearch => &["host", "port", "key"],
            Service::Mailpit => &["host", "port", "ui_port"],
            Service::Minio => &["host", "port", "user", "password", "bucket", "console_port"],
            Service::Nats => &["host", "port", "monitor_port"],
        }
    }

    /// The settings the app gets when the service has no `env` table.
    pub fn default_env(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Service::Postgres => &[(
                "APP_DATABASE__URL",
                "postgres://{user}:{password}@{host}:{port}/{database}",
            )],
            Service::Redis => &[("REDIS_URL", "redis://{host}:{port}")],
            Service::Meilisearch => &[
                ("APP_SEARCH__ENGINE", "meilisearch"),
                ("APP_SEARCH__URL", "http://{host}:{port}"),
                ("APP_SEARCH__KEY", "{key}"),
            ],
            Service::Mailpit => &[("APP_MAIL__TRANSPORT", "smtp://{host}:{port}")],
            Service::Minio => &[
                ("AWS_ENDPOINT_URL", "http://{host}:{port}"),
                ("AWS_ACCESS_KEY_ID", "{user}"),
                ("AWS_SECRET_ACCESS_KEY", "{password}"),
                ("AWS_REGION", "us-east-1"),
            ],
            // The broker's address only: the transport kind is the app's shape and stays in
            // config/app.toml.
            Service::Nats => &[("APP_MICRO__URL", "nats://{host}:{port}")],
        }
    }

    /// Its main port inside the container, which is also the default host port.
    pub fn main_port(self) -> u16 {
        match self {
            Service::Postgres => 5432,
            Service::Redis => 6379,
            Service::Meilisearch => 7700,
            Service::Mailpit => 1025,
            Service::Minio => 9000,
            Service::Nats => 4222,
        }
    }

    /// A line `navree dock:add` prints after adding it, when the app needs
    /// something more.
    pub fn add_hint(self) -> Option<&'static str> {
        match self {
            Service::Postgres => Some("Your app needs navree's `postgres` feature."),
            _ => None,
        }
    }
}

impl fmt::Display for Service {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for Service {
    type Err = DockError;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        Service::ALL
            .into_iter()
            .find(|service| service.name() == name)
            .ok_or_else(|| DockError::UnknownService(name.to_owned()))
    }
}

/// A port published on 127.0.0.1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Port {
    /// Its `dock.toml` key: `port`, `ui_port`, `console_port` or `monitor_port`.
    pub key: &'static str,
    /// The port on 127.0.0.1.
    pub host: u16,
    /// The port inside the container.
    pub container: u16,
}

/// A service's own settings, with defaults filled in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServiceOptions {
    /// PostgreSQL.
    Postgres {
        /// The database created on first start.
        database: String,
        /// The role it is created with.
        user: String,
        /// The role's password.
        password: String,
        /// Extensions created in the database.
        extensions: Vec<String>,
        /// Whether the server runs TimescaleDB: its extension is created in
        /// the database once it is ready (on by default; off for plain
        /// PostgreSQL and for images of your own that are not TimescaleDB
        /// images).
        timescaledb: bool,
    },
    /// Redis.
    Redis {
        /// The memory limit, in MiB.
        maxmemory_mb: u32,
    },
    /// Meilisearch.
    Meilisearch {
        /// The master key.
        master_key: String,
    },
    /// Mailpit.
    Mailpit,
    /// MinIO.
    Minio {
        /// A bucket created on first start.
        bucket: String,
        /// The access key (root user).
        access_key: String,
        /// The secret key (root password).
        secret_key: String,
    },
    /// NATS.
    Nats {
        /// Whether JetStream is on.
        jetstream: bool,
    },
}

/// How Dock knows a service is ready.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Readiness {
    /// A command run in the container exits with 0 (and prints `expect`, when set).
    Exec {
        /// The command and its arguments.
        args: Vec<String>,
        /// What it must print.
        expect: Option<&'static str>,
    },
    /// `GET http://127.0.0.1:{port}{path}` answers 200.
    Http {
        /// The host port.
        port: u16,
        /// The path.
        path: &'static str,
    },
}

impl fmt::Display for Readiness {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Readiness::Exec { args, .. } => write!(f, "`{}`", args.join(" ")),
            Readiness::Http { port, path } => write!(f, "GET http://{HOST}:{port}{path}"),
        }
    }
}

/// How `navree dock shell` opens a service's client.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shell {
    /// A command run in the service's container, with extra environment variables.
    Exec {
        /// Variables set for the command.
        env: Vec<(String, String)>,
        /// The command and its arguments.
        args: Vec<String>,
    },
    /// A throwaway container of `image` on the project's network.
    Run {
        /// The image.
        image: &'static str,
        /// Variables set in the container.
        env: Vec<(String, String)>,
        /// The command and its arguments.
        args: Vec<String>,
    },
}

/// A command run in a service's container once it is ready.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetupStep {
    /// Variables set for the command.
    pub env: Vec<(String, String)>,
    /// The command and its arguments.
    pub args: Vec<String>,
}

/// The nats-box image `navree dock shell nats` runs the `nats` CLI from.
pub const NATS_BOX_IMAGE: &str = "natsio/nats-box:0.20.1";

/// One service of `dock.toml`, resolved: its image, container, volume,
/// ports, settings and options.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceSpec {
    /// Which service.
    pub service: Service,
    /// The image reference, tag included.
    pub image: String,
    /// The container's name: `<project>-<service>`.
    pub container: String,
    /// The data volume's name: `<project>-<service>-data`.
    pub volume: String,
    /// Its published ports, the main one first.
    pub ports: Vec<Port>,
    /// The settings the app gets (environment variables), in order.
    pub env: Vec<(String, String)>,
    /// Its own settings.
    pub options: ServiceOptions,
}

impl ServiceSpec {
    /// Its name.
    pub fn name(&self) -> &'static str {
        self.service.name()
    }

    /// The host port of its main port.
    pub fn port(&self) -> u16 {
        self.ports[0].host
    }

    /// The host port of `key`.
    pub fn port_of(&self, key: &str) -> Option<u16> {
        self.ports.iter().find(|p| p.key == key).map(|p| p.host)
    }

    /// Its addresses: `127.0.0.1:<port>` for every published port.
    pub fn addresses(&self) -> Vec<String> {
        self.ports
            .iter()
            .map(|p| format!("{HOST}:{}", p.host))
            .collect()
    }

    /// Where its data lives in the container (the volume's mount point).
    pub fn data_path(&self) -> &'static str {
        match self.service {
            Service::Postgres if postgres_major(&self.image).is_none_or(|major| major >= 18) => {
                "/var/lib/postgresql"
            }
            Service::Postgres => "/var/lib/postgresql/data",
            Service::Meilisearch => "/meili_data",
            Service::Redis | Service::Mailpit | Service::Minio | Service::Nats => "/data",
        }
    }

    /// The container's environment variables.
    pub fn container_env(&self) -> Vec<(String, String)> {
        let pairs: Vec<(&str, String)> = match &self.options {
            ServiceOptions::Postgres {
                database,
                user,
                password,
                timescaledb,
                ..
            } => {
                let mut pairs = vec![
                    ("POSTGRES_USER", user.clone()),
                    ("POSTGRES_PASSWORD", password.clone()),
                    ("POSTGRES_DB", database.clone()),
                ];
                if *timescaledb {
                    // The TimescaleDB images' first start turns its
                    // telemetry off (and unschedules the telemetry job).
                    pairs.push(("TIMESCALEDB_TELEMETRY", "off".to_owned()));
                }
                pairs
            }
            ServiceOptions::Redis { .. } | ServiceOptions::Nats { .. } => vec![],
            ServiceOptions::Meilisearch { master_key } => vec![
                ("MEILI_MASTER_KEY", master_key.clone()),
                ("MEILI_ENV", "development".to_owned()),
                ("MEILI_NO_ANALYTICS", "true".to_owned()),
            ],
            ServiceOptions::Mailpit => vec![
                ("MP_DATABASE", "/data/mailpit.db".to_owned()),
                ("MP_SMTP_AUTH_ACCEPT_ANY", "1".to_owned()),
                ("MP_SMTP_AUTH_ALLOW_INSECURE", "1".to_owned()),
            ],
            ServiceOptions::Minio {
                access_key,
                secret_key,
                ..
            } => vec![
                ("MINIO_ROOT_USER", access_key.clone()),
                ("MINIO_ROOT_PASSWORD", secret_key.clone()),
            ],
        };
        pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
    }

    /// The container's command, when it is not the image's default.
    ///
    /// A [`TIMESCALEDB_IMAGE`] server starts with TimescaleDB preloaded and
    /// its telemetry off, also on a data volume a plain PostgreSQL image
    /// created (whose configuration does not preload it).
    pub fn command(&self) -> Option<Vec<String>> {
        let words: Vec<String> = match &self.options {
            ServiceOptions::Postgres {
                timescaledb: true, ..
            } if is_timescaledb_image(&self.image) => vec![
                "postgres".into(),
                "-c".into(),
                "shared_preload_libraries=timescaledb".into(),
                "-c".into(),
                "timescaledb.telemetry_level=off".into(),
            ],
            // Options only: the image's entrypoint puts its server in front
            // (`redis-server`, or `valkey-server` in a Valkey image).
            ServiceOptions::Redis { maxmemory_mb } => vec![
                "--maxmemory".into(),
                format!("{maxmemory_mb}mb"),
                "--appendonly".into(),
                "yes".into(),
            ],
            ServiceOptions::Minio { .. } => vec![
                "server".into(),
                "/data".into(),
                "--console-address".into(),
                ":9001".into(),
            ],
            ServiceOptions::Nats { jetstream } => {
                let mut words = Vec::new();
                if *jetstream {
                    words.extend(["--jetstream".into(), "--store_dir".into(), "/data".into()]);
                }
                words.extend(["--http_port".into(), "8222".into()]);
                words
            }
            _ => return None,
        };
        Some(words)
    }

    /// Its readiness check.
    pub fn readiness(&self) -> Readiness {
        let exec = |args: &[&str], expect| Readiness::Exec {
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            expect,
        };
        match &self.options {
            ServiceOptions::Postgres { database, user, .. } => exec(
                // Over TCP: during its first start the image runs a
                // temporary server on the Unix socket only.
                &[
                    "pg_isready",
                    "-h",
                    HOST,
                    "-p",
                    "5432",
                    "-U",
                    user,
                    "-d",
                    database,
                ],
                None,
            ),
            ServiceOptions::Redis { .. } => exec(&["redis-cli", "ping"], Some("PONG")),
            ServiceOptions::Meilisearch { .. } => Readiness::Http {
                port: self.port(),
                path: "/health",
            },
            ServiceOptions::Mailpit => Readiness::Http {
                port: self.port_of("ui_port").unwrap_or(8025),
                path: "/readyz",
            },
            ServiceOptions::Minio { .. } => Readiness::Http {
                port: self.port(),
                path: "/minio/health/live",
            },
            ServiceOptions::Nats { .. } => Readiness::Http {
                port: self.port_of("monitor_port").unwrap_or(8222),
                path: "/healthz",
            },
        }
    }

    /// Commands run in the container once it is ready (idempotent): the
    /// PostgreSQL extensions (TimescaleDB's first, when it is on) and the
    /// MinIO bucket.
    pub fn setup(&self) -> Vec<SetupStep> {
        let words = |words: &[&str]| words.iter().map(|w| (*w).to_owned()).collect();
        match &self.options {
            ServiceOptions::Postgres {
                database,
                user,
                extensions,
                timescaledb,
                ..
            } => timescaledb
                .then_some("timescaledb")
                .into_iter()
                .chain(
                    extensions
                        .iter()
                        .map(String::as_str)
                        .filter(|extension| !(*timescaledb && *extension == "timescaledb")),
                )
                .map(|extension| SetupStep {
                    env: vec![],
                    args: words(&[
                        "psql",
                        "-v",
                        "ON_ERROR_STOP=1",
                        "-U",
                        user,
                        "-d",
                        database,
                        "-c",
                        &format!("CREATE EXTENSION IF NOT EXISTS \"{extension}\""),
                    ]),
                })
                .collect(),
            ServiceOptions::Minio { bucket, .. } => vec![SetupStep {
                env: self.minio_client_env(),
                args: words(&["mcli", "mb", "--ignore-existing", &format!("dock/{bucket}")]),
            }],
            _ => vec![],
        }
    }

    /// `MC_HOST_dock`, which points the bundled `mcli` (or `mc`) at the server.
    fn minio_client_env(&self) -> Vec<(String, String)> {
        match &self.options {
            ServiceOptions::Minio {
                access_key,
                secret_key,
                ..
            } => vec![(
                "MC_HOST_dock".to_owned(),
                format!("http://{access_key}:{secret_key}@127.0.0.1:9000"),
            )],
            _ => vec![],
        }
    }

    /// How `navree dock shell` opens its client.
    pub fn shell(&self) -> Shell {
        let words = |words: &[&str]| words.iter().map(|w| (*w).to_owned()).collect();
        match &self.options {
            ServiceOptions::Postgres { database, user, .. } => Shell::Exec {
                env: vec![],
                args: words(&["psql", "-U", user, "-d", database]),
            },
            ServiceOptions::Redis { .. } => Shell::Exec {
                env: vec![],
                args: words(&["redis-cli"]),
            },
            ServiceOptions::Nats { .. } => Shell::Run {
                image: NATS_BOX_IMAGE,
                env: vec![("NATS_URL".to_owned(), "nats://nats:4222".to_owned())],
                args: words(&["sh"]),
            },
            ServiceOptions::Minio { .. } => Shell::Exec {
                env: self.minio_client_env(),
                args: words(&["sh"]),
            },
            ServiceOptions::Meilisearch { .. } | ServiceOptions::Mailpit => Shell::Exec {
                env: vec![],
                args: words(&["sh"]),
            },
        }
    }

    /// Where to open its web UI, with a label, when it has one.
    pub fn web_ui(&self) -> Option<String> {
        match &self.options {
            ServiceOptions::Mailpit => Some(format!(
                "Mailpit's inbox: http://{HOST}:{}",
                self.port_of("ui_port").unwrap_or(8025)
            )),
            ServiceOptions::Minio {
                access_key,
                secret_key,
                ..
            } => Some(format!(
                "MinIO's console: http://{HOST}:{} ({access_key} / {secret_key})",
                self.port_of("console_port").unwrap_or(9001)
            )),
            _ => None,
        }
    }
}

/// The PostgreSQL major version of an image reference, from its tag
/// (`postgres:17`, `postgres:16.4-alpine`, `pgvector/pgvector:pg17`,
/// `pgvector/pgvector:0.8.0-pg16`); `None` for tags such as `latest`.
pub(crate) fn postgres_major(image: &str) -> Option<u32> {
    let tag = image_tag(image)?;
    let candidate = match tag.find("pg") {
        Some(at) => &tag[at + 2..],
        None => tag,
    };
    let digits: String = candidate.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// The tag of an image reference: what follows the last `:` after the last `/`.
pub(crate) fn image_tag(image: &str) -> Option<&str> {
    let name = image.rsplit('/').next().unwrap_or(image);
    let name = name.split('@').next().unwrap_or(name);
    name.split_once(':').map(|(_, tag)| tag)
}

/// The repository of an image reference, without its tag or digest
/// (`localhost:5000/postgres:16` gives `localhost:5000/postgres`).
pub(crate) fn image_repository(image: &str) -> &str {
    let image = image.split('@').next().unwrap_or(image);
    let name_start = image.rfind('/').map_or(0, |at| at + 1);
    match image[name_start..].find(':') {
        Some(colon) => &image[..name_start + colon],
        None => image,
    }
}

/// Whether `image` is a TimescaleDB image (`timescale/timescaledb`,
/// `timescale/timescaledb-ha`, from any registry): what turns
/// `timescaledb` on by default for an `image` of your own.
pub(crate) fn has_timescaledb(image: &str) -> bool {
    image_repository(image)
        .rsplit('/')
        .next()
        .is_some_and(|name| name.starts_with("timescaledb"))
}

/// Whether `image` is [`TIMESCALEDB_IMAGE`] (from any registry), whose
/// configuration Dock knows.
fn is_timescaledb_image(image: &str) -> bool {
    let repository = image_repository(image);
    repository == TIMESCALEDB_IMAGE || repository.ends_with(&format!("/{TIMESCALEDB_IMAGE}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn services_parse_by_name() {
        for service in Service::ALL {
            assert_eq!(service.name().parse::<Service>().unwrap(), service);
        }
        let error = "mysql".parse::<Service>().unwrap_err();
        assert_eq!(
            error.to_string(),
            "unknown service `mysql`; Dock runs postgres, redis, meilisearch, mailpit, minio and nats"
        );
    }

    #[test]
    fn image_tags_and_postgres_majors() {
        assert_eq!(image_tag("postgres:17"), Some("17"));
        assert_eq!(image_tag("localhost:5000/postgres"), None);
        assert_eq!(image_tag("localhost:5000/postgres:16"), Some("16"));
        assert_eq!(image_tag("redis@sha256:abc"), None);
        assert_eq!(postgres_major("postgres:17"), Some(17));
        assert_eq!(postgres_major("postgres:16.4-alpine"), Some(16));
        assert_eq!(postgres_major("pgvector/pgvector:pg18"), Some(18));
        assert_eq!(postgres_major("pgvector/pgvector:0.8.0-pg16"), Some(16));
        assert_eq!(postgres_major("postgres:latest"), None);
        assert_eq!(postgres_major("postgres"), None);
        assert_eq!(
            postgres_major("timescale/timescaledb:2.31.0-pg17"),
            Some(17)
        );
        assert_eq!(
            postgres_major("timescale/timescaledb:latest-pg18"),
            Some(18)
        );
        assert_eq!(postgres_major("timescale/timescaledb-ha:pg16"), Some(16));
    }

    #[test]
    fn image_repositories_and_timescaledb_images() {
        assert_eq!(image_repository("postgres:17"), "postgres");
        assert_eq!(
            image_repository("localhost:5000/postgres:16"),
            "localhost:5000/postgres"
        );
        assert_eq!(
            image_repository("localhost:5000/postgres"),
            "localhost:5000/postgres"
        );
        assert_eq!(image_repository("redis@sha256:abc"), "redis");
        assert!(has_timescaledb("timescale/timescaledb:2.31.0-pg17"));
        assert!(has_timescaledb("timescale/timescaledb-ha:pg17"));
        assert!(has_timescaledb("ghcr.io/agra/timescaledb:17"));
        assert!(!has_timescaledb("postgres:17"));
        assert!(!has_timescaledb("postgis/postgis:17-3.5"));
        assert!(is_timescaledb_image("timescale/timescaledb:2.31.0-pg17"));
        assert!(is_timescaledb_image(
            "docker.io/timescale/timescaledb:latest-pg17"
        ));
        assert!(!is_timescaledb_image("timescale/timescaledb-ha:pg17"));
    }

    #[test]
    fn the_default_image_is_timescaledb_on_the_default_postgres_version() {
        let major: u32 = POSTGRES_VERSION.parse().unwrap();
        assert_eq!(DEFAULT_TIMESCALEDB_TAG, timescaledb_tag(major));
        assert!(TIMESCALEDB_POSTGRES_VERSIONS.contains(&major));
        assert_eq!(
            Service::Postgres.default_image(),
            ("timescale/timescaledb", "2.31.0-pg17")
        );
    }

    fn postgres(dock_toml: &str) -> ServiceSpec {
        crate::DockFile::parse(dock_toml, "crm-api")
            .unwrap_or_else(|e| panic!("{e}"))
            .services
            .remove(0)
    }

    fn words(args: &[&str]) -> Vec<String> {
        args.iter().map(|w| (*w).to_owned()).collect()
    }

    #[test]
    fn timescaledb_is_preloaded_without_telemetry_and_created_once_ready() {
        let spec = postgres("[services.postgres]\nextensions = [\"vector\", \"timescaledb\"]\n");
        assert_eq!(
            spec.container_env(),
            [
                ("POSTGRES_USER".to_owned(), "navree".to_owned()),
                ("POSTGRES_PASSWORD".to_owned(), "navree".to_owned()),
                ("POSTGRES_DB".to_owned(), "crm_api".to_owned()),
                ("TIMESCALEDB_TELEMETRY".to_owned(), "off".to_owned()),
            ]
        );
        // Also for a volume a plain PostgreSQL image created first.
        assert_eq!(
            spec.command(),
            Some(words(&[
                "postgres",
                "-c",
                "shared_preload_libraries=timescaledb",
                "-c",
                "timescaledb.telemetry_level=off",
            ]))
        );
        let created: Vec<String> = spec
            .setup()
            .into_iter()
            .map(|step| step.args.last().unwrap().clone())
            .collect();
        assert_eq!(
            created,
            [
                "CREATE EXTENSION IF NOT EXISTS \"timescaledb\"",
                "CREATE EXTENSION IF NOT EXISTS \"vector\"",
            ],
            "TimescaleDB first, and once"
        );
        assert_eq!(
            spec.setup()[0].args,
            words(&[
                "psql",
                "-v",
                "ON_ERROR_STOP=1",
                "-U",
                "navree",
                "-d",
                "crm_api",
                "-c",
                "CREATE EXTENSION IF NOT EXISTS \"timescaledb\"",
            ])
        );
        assert_eq!(
            spec.readiness().to_string(),
            "`pg_isready -h 127.0.0.1 -p 5432 -U navree -d crm_api`"
        );
    }

    #[test]
    fn plain_postgres_and_images_of_your_own_keep_their_configuration() {
        let plain = postgres("[services.postgres]\ntimescaledb = false\n");
        assert_eq!(plain.command(), None);
        assert!(plain.setup().is_empty());
        assert!(
            !plain
                .container_env()
                .iter()
                .any(|(name, _)| name == "TIMESCALEDB_TELEMETRY")
        );
        // Dock knows how the `timescale/timescaledb` images are configured,
        // not other images: it only creates the extension in those.
        let ha = postgres("[services.postgres]\nimage = \"timescale/timescaledb-ha:pg17\"\n");
        assert_eq!(ha.command(), None);
        assert_eq!(ha.setup().len(), 1);
        let pinned = postgres(
            "[services.postgres]\nimage = \"timescale/timescaledb\"\nversion = \"2.30.2-pg17\"\n",
        );
        assert_eq!(pinned.image, "timescale/timescaledb:2.30.2-pg17");
        assert!(pinned.command().is_some());
    }

    #[test]
    fn every_default_setting_uses_only_known_placeholders() {
        for service in Service::ALL {
            for (_, template) in service.default_env() {
                for name in crate::template::placeholders_in(template) {
                    assert!(
                        service.placeholders().contains(&name.as_str()),
                        "{service}: {{{name}}}"
                    );
                }
            }
        }
    }
}
