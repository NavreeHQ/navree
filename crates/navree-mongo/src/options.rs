//! [`MongoOptions`]: the `database.mongodb` configuration section.

use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use mongodb::options::{
    Acknowledgment, ClientOptions, ReadConcern, ReadPreference, SelectionCriteria,
};
use navree_config::ConfigSection;
use serde::Deserialize;

use crate::error::MongoError;

/// The name of the connection [`MongoOptions::url`] describes.
pub const DEFAULT_CONNECTION: &str = "default";

/// The settings of `MongoModule`: the `database.mongodb` configuration
/// section. Every key is optional.
///
/// ```toml
/// [database.mongodb]
/// url = "mongodb://127.0.0.1:27017/crm?replicaSet=rs0"   # mongodb+srv:// too
/// database = "crm"                  # else the URL's path, else `navree`
/// app_name = "crm-api"              # in the server's logs
/// min_pool_size = 0
/// max_pool_size = 10
/// connect_timeout_ms = 10000
/// server_selection_timeout_ms = 30000
/// read_preference = "primary"       # primaryPreferred, secondary, secondaryPreferred, nearest
/// read_concern = "majority"         # local, available, linearizable, snapshot
/// write_concern = "majority"        # or a number of nodes, or a tag set
/// journal = true
/// slow_query_ms = 500               # warn about commands slower than this (0: off)
/// migrate_on_boot = false
///
/// [database.mongodb.connections.analytics]   # another server or database
/// url = "mongodb://10.0.0.9:27017/analytics"
/// ```
///
/// From the environment: `APP_DATABASE__MONGODB__URL`,
/// `APP_DATABASE__MONGODB__DATABASE`, ... The URL's own options
/// (`?replicaSet=rs0&tls=true&authSource=admin`) apply too; the keys above
/// win over them. `Debug` masks URL passwords.
#[derive(Clone, PartialEq, Eq, Deserialize, ConfigSection)]
#[config(key = "database.mongodb", docs = "/docs/orm/mongodb/", validate = Self::check_values)]
#[serde(default)]
pub struct MongoOptions {
    /// The connection string (default `mongodb://127.0.0.1:27017/navree`):
    /// `mongodb://[user:password@]host[:port][,host...]/[database][?options]`
    /// or `mongodb+srv://...` (DNS seed lists). TLS goes through rustls.
    pub url: String,
    /// The database; the URL's path when unset, else `navree`.
    pub database: Option<String>,
    /// The application name the server logs (default: the app's name).
    pub app_name: Option<String>,
    /// Connections each server's pool keeps open.
    pub min_pool_size: Option<u32>,
    /// The most connections per server (the driver's default is 10).
    pub max_pool_size: Option<u32>,
    /// How long an idle pooled connection lives, in milliseconds.
    pub max_idle_time_ms: Option<u64>,
    /// How long opening a connection may take, in milliseconds (default
    /// 10000).
    pub connect_timeout_ms: u64,
    /// How long an operation waits for a suitable server, in milliseconds
    /// (default 30000).
    pub server_selection_timeout_ms: u64,
    /// Which members serve reads: `primary` (the default),
    /// `primaryPreferred`, `secondary`, `secondaryPreferred` or `nearest`.
    pub read_preference: Option<String>,
    /// The read concern: `local`, `available`, `majority`, `linearizable`
    /// or `snapshot`.
    pub read_concern: Option<String>,
    /// The write concern: `majority`, a number of members (`1`), or a
    /// custom tag set's name.
    pub write_concern: Option<String>,
    /// Whether writes wait for the journal.
    pub journal: Option<bool>,
    /// Whether writes are retried once on a network error (the driver's
    /// default is yes).
    pub retry_writes: Option<bool>,
    /// Whether reads are retried once on a network error.
    pub retry_reads: Option<bool>,
    /// Commands slower than this many milliseconds are logged as warnings;
    /// 0 (the default) turns that off.
    pub slow_query_ms: u64,
    /// Whether every command's shape (no values) is logged at debug level.
    pub log_queries: bool,
    /// The collection holding the counters of integer keys (models whose
    /// key the database assigns): `navree_sequences`.
    pub sequences: String,
    /// The collection recording applied migrations: `navree_migrations`.
    pub migrations: String,
    /// Whether pending MongoDB migrations run when the module boots.
    pub migrate_on_boot: bool,
    /// More connections, by name.
    pub connections: BTreeMap<String, MongoConnectionOptions>,
}

/// A named connection: `database.mongodb.connections.<name>`, opened on
/// first use by [`MongoDb::connection`](crate::MongoDb::connection).
///
/// ```no_run
/// # async fn demo(mongo: navree_mongo::MongoDb) -> navree_mongo::MongoResult {
/// // [database.mongodb.connections.analytics]
/// // url = "mongodb://10.0.0.9:27017/analytics"
/// let analytics = mongo.connection("analytics").await?;
/// analytics.ping().await?;
/// # Ok(()) }
/// ```
#[derive(Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct MongoConnectionOptions {
    /// The connection string, like [`MongoOptions::url`].
    pub url: String,
    /// The database; the URL's path when unset.
    pub database: Option<String>,
}

impl Default for MongoOptions {
    fn default() -> Self {
        MongoOptions {
            url: "mongodb://127.0.0.1:27017/navree".to_owned(),
            database: None,
            app_name: None,
            min_pool_size: None,
            max_pool_size: None,
            max_idle_time_ms: None,
            connect_timeout_ms: 10_000,
            server_selection_timeout_ms: 30_000,
            read_preference: None,
            read_concern: None,
            write_concern: None,
            journal: None,
            retry_writes: None,
            retry_reads: None,
            slow_query_ms: 0,
            log_queries: false,
            sequences: "navree_sequences".to_owned(),
            migrations: "navree_migrations".to_owned(),
            migrate_on_boot: false,
            connections: BTreeMap::new(),
        }
    }
}

impl MongoOptions {
    /// Options for the server and database at `url`, everything else
    /// default.
    pub fn new(url: impl Into<String>) -> Self {
        MongoOptions {
            url: url.into(),
            ..MongoOptions::default()
        }
    }

    /// The URL with its password (if any) replaced by `***`, safe to log.
    pub fn redacted_url(&self) -> String {
        redact_url(&self.url)
    }

    /// Every problem with these options (empty when valid). Messages name
    /// the key and never echo a URL, which may hold a password.
    pub fn check(&self) -> Result<(), Vec<String>> {
        let mut problems = Vec::new();
        if let Some(problem) = url_problem(&self.url) {
            problems.push(format!("database.mongodb.url {problem}"));
        }
        if let Some(database) = &self.database
            && let Some(problem) = database_problem(database)
        {
            problems.push(format!("database.mongodb.database {problem}"));
        }
        if self.connect_timeout_ms == 0 {
            problems.push("database.mongodb.connect_timeout_ms must be at least 1".to_owned());
        }
        if self.server_selection_timeout_ms == 0 {
            problems
                .push("database.mongodb.server_selection_timeout_ms must be at least 1".to_owned());
        }
        if let (Some(min), Some(max)) = (self.min_pool_size, self.max_pool_size)
            && min > max
        {
            problems
                .push("database.mongodb.min_pool_size must not exceed max_pool_size".to_owned());
        }
        if self.max_pool_size == Some(0) {
            problems.push("database.mongodb.max_pool_size must be at least 1".to_owned());
        }
        if let Some(preference) = &self.read_preference
            && read_preference(preference).is_none()
        {
            problems.push(format!(
                "database.mongodb.read_preference `{preference}` is not one of primary, primaryPreferred, secondary, secondaryPreferred, nearest"
            ));
        }
        if let Some(concern) = &self.read_concern
            && read_concern(concern).is_none()
        {
            problems.push(format!(
                "database.mongodb.read_concern `{concern}` is not one of local, available, majority, linearizable, snapshot"
            ));
        }
        if let Some(concern) = &self.write_concern
            && concern.trim().is_empty()
        {
            problems.push(
                "database.mongodb.write_concern is empty (majority, a number of members, or a tag set)"
                    .to_owned(),
            );
        }
        for (key, name) in [
            ("sequences", &self.sequences),
            ("migrations", &self.migrations),
        ] {
            if let Some(problem) = collection_problem(name) {
                problems.push(format!("database.mongodb.{key} {problem}"));
            }
        }
        for (name, connection) in &self.connections {
            if name == DEFAULT_CONNECTION {
                problems.push(
                    "database.mongodb.connections.default: `default` is the connection of database.mongodb.url; pick another name"
                        .to_owned(),
                );
            } else if name.is_empty()
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
            {
                problems.push(format!(
                    "database.mongodb.connections: `{name}` is not a connection name (lowercase letters, digits, `_` and `-`)"
                ));
            }
            if let Some(problem) = url_problem(&connection.url) {
                problems.push(format!("database.mongodb.connections.{name}.url {problem}"));
            }
            if let Some(database) = &connection.database
                && let Some(problem) = database_problem(database)
            {
                problems.push(format!(
                    "database.mongodb.connections.{name}.database {problem}"
                ));
            }
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems)
        }
    }

    /// The driver's options for connection string `url`, with these
    /// options applied over the URL's own.
    pub(crate) async fn client_options(&self, url: &str) -> Result<ClientOptions, MongoError> {
        let mut client = ClientOptions::parse(url)
            .await
            .map_err(|error| MongoError::Options {
                problems: vec![format!(
                    "the MongoDB URL {} does not parse: {error}",
                    redact_url(url)
                )],
            })?;
        if let Some(app_name) = &self.app_name {
            client.app_name = Some(app_name.clone());
        }
        if self.min_pool_size.is_some() {
            client.min_pool_size = self.min_pool_size;
        }
        if self.max_pool_size.is_some() {
            client.max_pool_size = self.max_pool_size;
        }
        if let Some(ms) = self.max_idle_time_ms {
            client.max_idle_time = Some(Duration::from_millis(ms));
        }
        client.connect_timeout = Some(Duration::from_millis(self.connect_timeout_ms));
        client.server_selection_timeout =
            Some(Duration::from_millis(self.server_selection_timeout_ms));
        if let Some(preference) = self.read_preference.as_deref().and_then(read_preference) {
            client.selection_criteria = Some(SelectionCriteria::ReadPreference(preference));
        }
        if let Some(concern) = self.read_concern.as_deref().and_then(read_concern) {
            client.read_concern = Some(concern);
        }
        if self.write_concern.is_some() || self.journal.is_some() {
            let mut concern = client.write_concern.take().unwrap_or_default();
            if let Some(w) = &self.write_concern {
                concern.w = Some(acknowledgment(w));
            }
            if let Some(journal) = self.journal {
                concern.journal = Some(journal);
            }
            client.write_concern = Some(concern);
        }
        if self.retry_writes.is_some() {
            client.retry_writes = self.retry_writes;
        }
        if self.retry_reads.is_some() {
            client.retry_reads = self.retry_reads;
        }
        Ok(client)
    }

    /// The database of a connection: `database`, else the URL's path,
    /// else `navree`.
    pub(crate) fn database_of(database: Option<&String>, client: &ClientOptions) -> String {
        database
            .cloned()
            .or_else(|| client.default_database.clone())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "navree".to_owned())
    }
}

impl MongoOptions {
    /// Checks the values and joins the problems (the boot calls this).
    fn check_values(&self) -> Result<(), String> {
        self.check().map_err(|problems| problems.join("; "))
    }
}

impl fmt::Debug for MongoOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MongoOptions")
            .field("url", &self.redacted_url())
            .field("database", &self.database)
            .field("app_name", &self.app_name)
            .field("min_pool_size", &self.min_pool_size)
            .field("max_pool_size", &self.max_pool_size)
            .field("read_preference", &self.read_preference)
            .field("read_concern", &self.read_concern)
            .field("write_concern", &self.write_concern)
            .field("migrate_on_boot", &self.migrate_on_boot)
            .field("connections", &self.connections)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for MongoConnectionOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MongoConnectionOptions")
            .field("url", &redact_url(&self.url))
            .field("database", &self.database)
            .finish()
    }
}

/// `url` with the password of its credentials replaced by `***`.
pub(crate) fn redact_url(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_owned();
    };
    let host_end = rest.find('/').unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(host_end);
    match authority.rsplit_once('@') {
        Some((credentials, hosts)) => {
            let user = credentials
                .split_once(':')
                .map_or(credentials, |(user, _)| user);
            format!("{scheme}://{user}:***@{hosts}{tail}")
        }
        None => url.to_owned(),
    }
}

/// Why `url` is not a MongoDB connection string, without echoing it.
fn url_problem(url: &str) -> Option<&'static str> {
    let url = url.trim();
    if url.is_empty() {
        return Some("is empty (mongodb://host:port/database)");
    }
    if !(url.starts_with("mongodb://") || url.starts_with("mongodb+srv://")) {
        return Some("must start with mongodb:// or mongodb+srv://");
    }
    None
}

/// Why `name` is not a MongoDB database name.
fn database_problem(name: &str) -> Option<String> {
    if name.is_empty() {
        return Some("is empty".to_owned());
    }
    if name.len() > 63 {
        return Some("is longer than 63 bytes".to_owned());
    }
    if let Some(c) = name.chars().find(|c| "/\\. \"$*<>:|?\0".contains(*c)) {
        return Some(format!("`{name}` holds `{c}`, which database names cannot"));
    }
    None
}

/// Why `name` is not a collection name the module can own.
fn collection_problem(name: &str) -> Option<String> {
    if name.is_empty() {
        return Some("is empty".to_owned());
    }
    if name.starts_with("system.") || name.contains('$') || name.contains('\0') {
        return Some(format!(
            "`{name}` cannot name a collection (no `system.` prefix, `$` or NUL)"
        ));
    }
    None
}

/// The read preference named `name` (camelCase or snake_case).
fn read_preference(name: &str) -> Option<ReadPreference> {
    Some(
        match name.trim().to_ascii_lowercase().replace('_', "").as_str() {
            "primary" => ReadPreference::Primary,
            "primarypreferred" => ReadPreference::PrimaryPreferred { options: None },
            "secondary" => ReadPreference::Secondary { options: None },
            "secondarypreferred" => ReadPreference::SecondaryPreferred { options: None },
            "nearest" => ReadPreference::Nearest { options: None },
            _ => return None,
        },
    )
}

/// The read concern named `name`.
fn read_concern(name: &str) -> Option<ReadConcern> {
    Some(match name.trim().to_ascii_lowercase().as_str() {
        "local" => ReadConcern::local(),
        "available" => ReadConcern::available(),
        "majority" => ReadConcern::majority(),
        "linearizable" => ReadConcern::linearizable(),
        "snapshot" => ReadConcern::snapshot(),
        _ => return None,
    })
}

/// The write acknowledgment `w`: `majority`, a number of members, or a
/// tag set.
fn acknowledgment(w: &str) -> Acknowledgment {
    let w = w.trim();
    if w.eq_ignore_ascii_case("majority") {
        Acknowledgment::Majority
    } else if let Ok(nodes) = w.parse::<u32>() {
        Acknowledgment::Nodes(nodes)
    } else {
        Acknowledgment::Custom(w.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use navree_config::FieldType;

    use super::*;

    #[test]
    fn defaults_are_valid() {
        assert!(MongoOptions::default().check().is_ok());
        assert_eq!(MongoOptions::new("mongodb://h/x").url, "mongodb://h/x");
    }

    #[test]
    fn the_section_describes_itself() {
        let schema = MongoOptions::schema().expect("schema");
        assert_eq!(schema.key, "database.mongodb");
        assert_eq!(schema.docs.as_deref(), Some("/docs/orm/mongodb/"));
        assert!(
            schema.unmarked_secrets().is_empty(),
            "{:?}",
            schema.unmarked_secrets()
        );
        let field = |name: &str| {
            schema
                .fields
                .iter()
                .find(|field| field.name == name)
                .unwrap_or_else(|| panic!("no field {name}"))
        };
        assert_eq!(field("url").ty, FieldType::String);
        assert_eq!(field("max_pool_size").ty, FieldType::Integer);
        assert_eq!(field("journal").ty, FieldType::Bool);
        assert_eq!(
            field("connections").ty,
            FieldType::map(FieldType::other("MongoConnectionOptions"))
        );
        let defaults = MongoOptions::defaults().expect("defaults");
        assert_eq!(defaults["url"], "mongodb://127.0.0.1:27017/navree");
        assert_eq!(defaults["connect_timeout_ms"], 10_000);
        assert_eq!(defaults["sequences"], "navree_sequences");
        // The check the boot runs is the one the section names.
        let bad = MongoOptions {
            connect_timeout_ms: 0,
            ..MongoOptions::default()
        };
        assert!(bad.validate().unwrap_err().contains("connect_timeout_ms"));
    }

    #[test]
    fn problems_name_their_key_without_echoing_secrets() {
        let mut options = MongoOptions::new("postgres://user:hunter2@host/db");
        options.connect_timeout_ms = 0;
        options.read_preference = Some("closest".into());
        options.min_pool_size = Some(5);
        options.max_pool_size = Some(2);
        options.migrations = "system.migrations".into();
        options.connections.insert(
            "Bad Name".into(),
            MongoConnectionOptions {
                url: "mongodb://h".into(),
                database: Some("a.b".into()),
            },
        );
        let problems = options.check().unwrap_err();
        let text = problems.join("\n");
        assert!(
            text.contains("database.mongodb.url must start with mongodb://"),
            "{text}"
        );
        assert!(text.contains("connect_timeout_ms"), "{text}");
        assert!(text.contains("`closest`"), "{text}");
        assert!(text.contains("min_pool_size"), "{text}");
        assert!(text.contains("`system.migrations`"), "{text}");
        assert!(
            text.contains("`Bad Name` is not a connection name"),
            "{text}"
        );
        assert!(text.contains("connections.Bad Name.database"), "{text}");
        assert!(!text.contains("hunter2"), "{text}");
    }

    #[test]
    fn urls_are_redacted() {
        assert_eq!(
            redact_url("mongodb://anu:secret@db1:27017,db2:27017/crm?authSource=admin"),
            "mongodb://anu:***@db1:27017,db2:27017/crm?authSource=admin"
        );
        assert_eq!(redact_url("mongodb://db/crm"), "mongodb://db/crm");
        let options = MongoOptions::new("mongodb+srv://anu:secret@cluster.example.net/crm");
        assert!(!format!("{options:?}").contains("secret"));
    }

    #[test]
    fn preferences_concerns_and_acknowledgments_parse() {
        assert!(matches!(
            read_preference("secondary_preferred"),
            Some(ReadPreference::SecondaryPreferred { .. })
        ));
        assert!(matches!(
            read_preference("primaryPreferred"),
            Some(ReadPreference::PrimaryPreferred { .. })
        ));
        assert!(read_concern("MAJORITY").is_some());
        assert!(read_concern("eventual").is_none());
        assert_eq!(acknowledgment("majority"), Acknowledgment::Majority);
        assert_eq!(acknowledgment("2"), Acknowledgment::Nodes(2));
        assert_eq!(
            acknowledgment("dc-east"),
            Acknowledgment::Custom("dc-east".into())
        );
    }

    #[tokio::test]
    async fn client_options_apply_over_the_url() {
        let mut options = MongoOptions::new("mongodb://127.0.0.1:27017/crm?appName=url-name");
        options.app_name = Some("crm-api".into());
        options.max_pool_size = Some(20);
        options.write_concern = Some("majority".into());
        options.journal = Some(true);
        let client = options.client_options(&options.url).await.unwrap();
        assert_eq!(client.app_name.as_deref(), Some("crm-api"));
        assert_eq!(client.max_pool_size, Some(20));
        assert_eq!(client.default_database.as_deref(), Some("crm"));
        let concern = client.write_concern.clone().unwrap();
        assert_eq!(concern.w, Some(Acknowledgment::Majority));
        assert_eq!(concern.journal, Some(true));
        assert_eq!(MongoOptions::database_of(None, &client), "crm");
        assert_eq!(
            MongoOptions::database_of(Some(&"other".to_owned()), &client),
            "other"
        );
        let error = options
            .client_options("mongodb://anu:pw@")
            .await
            .unwrap_err();
        assert!(!error.to_string().contains(":pw@"), "{error}");
    }
}
