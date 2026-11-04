//! CONFIG-006: a key or section that nothing reads is named, with the
//! closest name that is read.

use std::collections::HashMap;

use navree_config::{Config, ConfigSection, ConfigSources, UnusedKey, ValueOrigin};
use serde::Deserialize;
use tempfile::TempDir;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
#[allow(dead_code)]
struct Database {
    url: String,
    max_connections: u32,
    pool: Pool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
#[allow(dead_code)]
struct Pool {
    idle_secs: u64,
}

impl ConfigSection for Database {
    const KEY: &'static str = "database";
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
#[allow(dead_code)]
struct Mongo {
    url: String,
}

impl ConfigSection for Mongo {
    const KEY: &'static str = "database.mongodb";
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
#[allow(dead_code)]
struct Queue {
    workers: u32,
}

impl ConfigSection for Queue {
    const KEY: &'static str = "queue";
}

fn load(sources: ConfigSources) -> Config {
    Config::load(&sources).expect("load")
}

fn keys(config: &Config) -> Vec<String> {
    config.unused().into_iter().map(|u| u.key).collect()
}

#[test]
fn a_typo_in_a_nested_key_is_named_with_a_suggestion() {
    let config = load(
        ConfigSources::new()
            .set("database.url", "sqlite://app.db")
            .set("database.max_conections", 50),
    );
    config.typed::<Database>().expect("database");
    let unused = config.unused();
    assert_eq!(unused.len(), 1, "{unused:?}");
    assert_eq!(unused[0].key, "database.max_conections");
    assert_eq!(
        unused[0].suggestion.as_deref(),
        Some("database.max_connections")
    );
    assert!(!unused[0].is_section());
    let message = unused[0].message();
    assert!(
        message.contains("unknown configuration key `database.max_conections`"),
        "{message}"
    );
    assert!(
        message.contains("Did you mean `database.max_connections`?"),
        "{message}"
    );
}

#[test]
fn a_typo_in_a_top_level_section_is_named() {
    let config = load(
        ConfigSources::new()
            .set("database.url", "sqlite://app.db")
            .set("databse.url", "sqlite://typo.db"),
    );
    config.typed::<Database>().expect("database");
    let unused = config.unused();
    assert_eq!(
        keys(&config),
        ["databse"],
        "the whole table is reported once"
    );
    assert!(unused[0].is_section());
    assert_eq!(unused[0].suggestion.as_deref(), Some("database"));
    assert!(
        unused[0]
            .message()
            .contains("unknown configuration section `databse`")
    );
}

#[test]
fn a_typo_in_an_environment_variable_is_named_with_its_variable() {
    let config = load(ConfigSources::new().env_vars(
        "APP_",
        [("APP_QUEUE__WORKER", "4"), ("APP_QUEUE__WORKERS", "2")],
    ));
    config.typed::<Queue>().expect("queue");
    let unused = config.unused();
    assert_eq!(unused.len(), 1, "{unused:?}");
    assert_eq!(unused[0].key, "queue.worker");
    assert_eq!(
        unused[0].origin,
        ValueOrigin::Environment {
            variable: "APP_QUEUE__WORKER".to_owned()
        }
    );
    assert_eq!(unused[0].suggestion.as_deref(), Some("queue.workers"));
}

#[test]
fn a_key_in_a_dotenv_file_names_its_line() {
    let dir: TempDir = tempfile::tempdir().expect("tempdir");
    let env = dir.path().join(".env");
    std::fs::write(&env, "APP_QUEUE__WORKERS=2\nAPP_QUEUE__WORKRES=3\n").expect("write");
    let config = load(ConfigSources::new().dotenv(&env));
    config.typed::<Queue>().expect("queue");
    let unused = config.unused();
    assert_eq!(unused.len(), 1);
    assert!(
        matches!(&unused[0].origin, ValueOrigin::DotEnv { line: 2, .. }),
        "{:?}",
        unused[0].origin
    );
    assert_eq!(unused[0].suggestion.as_deref(), Some("queue.workers"));
}

#[test]
fn keys_that_are_read_are_not_reported() {
    let config = load(
        ConfigSources::new()
            .set("database.url", "sqlite://app.db")
            .set("database.pool.idle_secs", 30)
            .set("queue.workers", 2),
    );
    config.typed::<Database>().expect("database");
    config.typed::<Queue>().expect("queue");
    assert!(config.unused().is_empty(), "{:?}", config.unused());
}

#[test]
fn a_nested_struct_names_its_own_typos() {
    let config = load(ConfigSources::new().set("database.pool.idle_sec", 30));
    config.typed::<Database>().expect("database");
    assert_eq!(keys(&config), ["database.pool.idle_sec"]);
    assert_eq!(
        config.unused()[0].suggestion.as_deref(),
        Some("database.pool.idle_secs")
    );
}

#[test]
fn nothing_is_reported_until_something_has_read_the_tree() {
    // With no reads at all, every key is unread; the boot calls `unused`
    // after the modules built their sections.
    let config = load(ConfigSources::new().set("database.url", "sqlite://app.db"));
    assert_eq!(keys(&config), ["database"]);
    config.typed::<Database>().expect("database");
    assert!(config.unused().is_empty());
}

#[test]
fn two_sections_that_read_one_table_do_not_report_each_other() {
    // `database` has no field `mongodb`; the section `database.mongodb`
    // reads it.
    let config = load(
        ConfigSources::new()
            .set("database.url", "sqlite://app.db")
            .set("database.mongodb.url", "mongodb://localhost"),
    );
    config.typed::<Database>().expect("database");
    assert_eq!(keys(&config), ["database.mongodb"], "not read yet");
    config.typed::<Mongo>().expect("mongo");
    assert!(config.unused().is_empty(), "{:?}", config.unused());
}

#[test]
fn a_typo_inside_a_nested_section_is_still_found() {
    let config = load(
        ConfigSources::new()
            .set("database.url", "sqlite://app.db")
            .set("database.mongodb.urll", "mongodb://localhost"),
    );
    config.typed::<Database>().expect("database");
    config.typed::<Mongo>().expect("mongo");
    assert_eq!(keys(&config), ["database.mongodb.urll"]);
}

#[test]
fn a_key_read_with_get_string_is_read() {
    let config = load(
        ConfigSources::new()
            .set("tracing.trace_url", "http://t")
            .set("tracing.other", 1),
    );
    assert_eq!(
        config.get_string("tracing.trace_url").as_deref(),
        Some("http://t")
    );
    assert_eq!(keys(&config), ["tracing.other"]);
    assert_eq!(config.get_bool("tracing.other"), Some(true));
    assert!(config.unused().is_empty());
}

#[test]
fn a_map_reads_every_key_it_holds() {
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Disks {
        disks: HashMap<String, Disk>,
    }
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Disk {
        root: String,
    }
    impl ConfigSection for Disks {
        const KEY: &'static str = "storage";
    }
    let config = load(
        ConfigSources::new()
            .set("storage.disks.local.root", "storage")
            .set("storage.disks.s3.root", "bucket"),
    );
    config.typed::<Disks>().expect("storage");
    assert!(config.unused().is_empty(), "{:?}", config.unused());

    // The struct inside the map still names its own typos.
    let config = load(
        ConfigSources::new()
            .set("storage.disks.local.rot", "storage")
            .set("storage.disks.local.root", "x"),
    );
    config.typed::<Disks>().expect("storage");
    assert_eq!(keys(&config), ["storage.disks.local.rot"]);
}

#[test]
fn flatten_and_json_values_read_every_key() {
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Open {
        name: String,
        #[serde(flatten)]
        rest: HashMap<String, serde_json::Value>,
    }
    impl ConfigSection for Open {
        const KEY: &'static str = "open";
    }
    let config = load(
        ConfigSources::new()
            .set("open.name", "x")
            .set("open.anything", 1)
            .set("open.nested.deep", true),
    );
    config.typed::<Open>().expect("open");
    assert!(config.unused().is_empty(), "{:?}", config.unused());

    let config = load(ConfigSources::new().set("raw.a", 1).set("raw.b.c", 2));
    config.section::<serde_json::Value>("raw").expect("raw");
    assert!(config.unused().is_empty());
}

#[test]
fn keys_inside_a_list_of_tables_are_checked() {
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Servers {
        servers: Vec<Server>,
    }
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Server {
        host: String,
        port: u16,
    }
    impl ConfigSection for Servers {
        const KEY: &'static str = "cluster";
    }
    let config = load(ConfigSources::new().set(
        "cluster.servers",
        serde_json::json!([{ "host": "a", "port": 1 }, { "host": "b", "prot": 2, "port": 3 }]),
    ));
    config.typed::<Servers>().expect("cluster");
    assert_eq!(keys(&config), ["cluster.servers[1].prot"]);
}

#[test]
fn a_failed_read_notes_nothing() {
    let config = load(
        ConfigSources::new()
            .set("queue.workers", "many")
            .set("queue.typo", 1),
    );
    assert!(config.typed::<Queue>().is_err());
    // The section was not read, so it is unread as a whole.
    assert_eq!(keys(&config), ["queue"]);
}

#[test]
fn claims_cover_what_the_notes_cannot_see() {
    let config = load(
        ConfigSources::new()
            .set("hashing.memory_kib", 1024)
            .set("queue.workers", 1)
            .set("queue.typo", 1),
    );
    config.typed::<Queue>().expect("queue");
    assert_eq!(keys(&config), ["hashing", "queue.typo"]);
    // A claim on a section applies where no section read covers a key.
    let claimed: Vec<String> = config
        .unused_with(&["hashing", "queue"])
        .into_iter()
        .map(|u| u.key)
        .collect();
    assert_eq!(claimed, ["queue.typo"], "the read of `queue` still decides");
    // A claim on a key below a section applies everywhere.
    let nested: Vec<String> = config
        .unused_with(&["hashing", "queue.typo"])
        .into_iter()
        .map(|u| u.key)
        .collect();
    assert!(nested.is_empty(), "{nested:?}");
}

#[test]
fn the_short_names_of_the_app_settings_are_never_reported() {
    let config = load(ConfigSources::new().env_vars(
        "APP_",
        [
            ("APP_KEY", "base64:abc"),
            ("APP_ENV", "production"),
            ("APP_URL", "https://acme.test"),
            ("APP_NAME", "acme"),
            ("APP_VERSION", "1.0.0"),
            ("APP_PREVIOUS_KEYS", "a,b"),
        ],
    ));
    config.app().expect("app");
    assert!(config.unused().is_empty(), "{:?}", config.unused());
}

#[test]
fn a_typo_in_the_app_section_is_named() {
    let config = load(
        ConfigSources::new()
            .set("app.nmae", "acme")
            .set("app.name", "acme"),
    );
    config.app().expect("app");
    let unused = config.unused();
    assert_eq!(unused.len(), 1);
    assert_eq!(unused[0].key, "app.nmae");
    assert_eq!(unused[0].suggestion.as_deref(), Some("app.name"));
}

#[test]
fn a_value_is_never_part_of_the_message() {
    let config = load(
        ConfigSources::new()
            .set("databse.password", "hunter2")
            .set("databse.url", "postgres://u:hunter2@h/d"),
    );
    let message: UnusedKey = config.unused().remove(0);
    let text = message.message();
    assert!(!text.contains("hunter2"), "{text}");
    assert!(text.contains("`databse`"), "{text}");
}

#[test]
fn distant_names_get_no_suggestion() {
    let config = load(
        ConfigSources::new()
            .set("queue.workers", 1)
            .set("queue.zzzzzz", 1),
    );
    config.typed::<Queue>().expect("queue");
    assert_eq!(config.unused()[0].suggestion, None);
}

#[test]
fn clones_share_the_notes() {
    let config = load(ConfigSources::new().set("queue.workers", 1));
    let clone = config.clone();
    clone.typed::<Queue>().expect("queue");
    assert!(config.unused().is_empty());
}

#[test]
fn an_empty_section_header_is_not_a_key() {
    let dir: TempDir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("app.toml");
    std::fs::write(&file, "[never_read]\n\n[queue]\nworkers = 1\n").expect("write");
    let config = load(ConfigSources::new().file(&file));
    config.typed::<Queue>().expect("queue");
    assert!(config.unused().is_empty());
}
