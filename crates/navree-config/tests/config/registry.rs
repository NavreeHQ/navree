//! Sections that describe themselves with `#[derive(ConfigSection)]`, and the
//! registry built from them.

use navree_config::{
    Config, ConfigRegistry, ConfigSection, ConfigSources, ConfigTable, EnvFallback, FieldType,
    SectionRegistration,
};
use navree_contracts::Secret;
use serde::Deserialize;
use serde_json::{Value, json};

/// Where entries are kept.
#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Store {
    #[default]
    Memory,
    Redis,
}

/// CORS settings.
#[derive(Clone, Debug, Default, Deserialize, ConfigTable)]
#[serde(default)]
struct Cors {
    /// The origins that may call the API.
    allowed_origins: Vec<String>,
    /// Allow cookies.
    credentials: bool,
}

/// Cache settings.
///
/// A second paragraph the schema leaves out.
#[derive(Clone, Debug, Default, Deserialize, ConfigSection)]
#[config(key = "cachetest", docs = "/docs/digging-deeper/cache/", validate = Self::check)]
#[serde(default)]
struct CacheTest {
    /// Where entries live.
    #[config(one_of = "memory, redis")]
    store: Store,
    /// Prepended to every key.
    prefix: String,
    /// How long an entry lives, in seconds.
    default_ttl_secs: u64,
    /// The password of Redis.
    redis_password: Option<Secret<String>>,
    /// A token kept as plain text.
    #[config(secret)]
    api_token: String,
    /// The Redis address.
    #[config(env_fallback = "REDIS_URL")]
    url: Option<String>,
    /// Cross-origin settings.
    #[config(table)]
    cors: Cors,
    #[serde(skip)]
    #[allow(dead_code)]
    runtime_only: u8,
}

impl CacheTest {
    fn check(&self) -> Result<(), String> {
        if self.prefix.contains(' ') {
            return Err("prefix must not contain a space".to_owned());
        }
        Ok(())
    }
}

/// A section nested below another.
#[derive(Clone, Debug, Default, Deserialize, ConfigSection)]
#[config(key = "cachetest.inner")]
#[serde(default)]
struct Inner {
    /// Switches it on.
    enabled: bool,
}

/// A section written by hand: it registers, and describes nothing.
#[derive(Deserialize)]
struct ByHand {
    #[allow(dead_code)]
    x: u8,
}

impl ConfigSection for ByHand {
    const KEY: &'static str = "byhand";
}

/// A section with no default and a credential that nobody marked.
#[allow(dead_code)]
#[derive(Debug, Deserialize, ConfigSection)]
#[config(key = "needs")]
struct Needs {
    /// The address (required).
    address: String,
    /// The password.
    password: String,
    /// Left alone.
    note: Option<String>,
}

fn names(fields: &[navree_config::FieldSchema]) -> Vec<&str> {
    fields.iter().map(|field| field.name.as_str()).collect()
}

#[test]
fn the_derive_writes_the_key_and_the_schema() {
    assert_eq!(CacheTest::KEY, "cachetest");
    let schema = CacheTest::schema().expect("schema");
    assert_eq!(schema.key, "cachetest");
    assert_eq!(schema.doc, "Cache settings.");
    assert_eq!(schema.docs.as_deref(), Some("/docs/digging-deeper/cache/"));
    assert_eq!(
        names(&schema.fields),
        [
            "store",
            "prefix",
            "default_ttl_secs",
            "redis_password",
            "api_token",
            "url",
            "cors"
        ]
    );

    let by_name = |name: &str| {
        schema
            .fields
            .iter()
            .find(|field| field.name == name)
            .unwrap_or_else(|| panic!("{name}"))
    };
    assert_eq!(by_name("store").ty, FieldType::choice(&["memory", "redis"]));
    assert_eq!(by_name("store").doc, "Where entries live.");
    assert_eq!(by_name("default_ttl_secs").ty, FieldType::Integer);
    assert!(by_name("redis_password").secret, "a Secret is a credential");
    assert!(by_name("api_token").secret, "so is a marked field");
    assert!(!by_name("prefix").secret);
    assert_eq!(by_name("url").env_fallback.as_deref(), Some("REDIS_URL"));
    assert!(
        !by_name("prefix").required,
        "serde(default) leaves nothing required"
    );

    let cors = by_name("cors");
    assert_eq!(cors.ty, FieldType::Table);
    assert_eq!(names(&cors.fields), ["allowed_origins", "credentials"]);
    assert_eq!(cors.fields[0].ty, FieldType::list(FieldType::String));
    assert_eq!(cors.fields[1].ty, FieldType::Bool);
    assert_eq!(cors.fields[0].doc, "The origins that may call the API.");
}

#[test]
fn defaults_come_from_default_and_credentials_are_null() {
    let defaults = CacheTest::defaults().expect("defaults");
    assert_eq!(
        defaults,
        json!({
            "store": "memory",
            "prefix": "",
            "default_ttl_secs": 0,
            "redis_password": null,
            "api_token": null,
            "url": null,
            "cors": { "allowed_origins": [], "credentials": false },
        })
    );
    let table = Cors::default().to_json();
    assert_eq!(table["credentials"], json!(false));
}

#[test]
fn a_secret_never_shows_its_value_in_the_defaults() {
    let table = CacheTest {
        redis_password: Some(Secret::new("hunter2".to_owned())),
        api_token: "hunter3".to_owned(),
        ..CacheTest::default()
    }
    .to_json();
    let text = table.to_string();
    assert!(!text.contains("hunter"), "{text}");
    assert_eq!(table["redis_password"], Value::Null);
    assert_eq!(table["api_token"], Value::Null);
}

#[test]
fn a_section_without_a_default_publishes_none_and_marks_what_is_required() {
    assert_eq!(Needs::defaults(), None);
    let schema = Needs::schema().expect("schema");
    let required: Vec<&str> = schema
        .fields
        .iter()
        .filter(|field| field.required)
        .map(|field| field.name.as_str())
        .collect();
    assert_eq!(required, ["address", "password"]);
}

#[test]
fn the_validate_function_runs_at_boot() {
    let config = Config::load(&ConfigSources::new().set("cachetest.prefix", "a b")).expect("load");
    let error = config.typed::<CacheTest>().expect_err("invalid");
    assert!(
        error
            .to_string()
            .contains("prefix must not contain a space"),
        "{error}"
    );
    let config = Config::load(&ConfigSources::new().set("cachetest.prefix", "ab")).expect("load");
    assert_eq!(config.typed::<CacheTest>().expect("valid").prefix, "ab");
}

fn registry() -> ConfigRegistry {
    ConfigRegistry::new(&[
        SectionRegistration::of::<Needs>(),
        SectionRegistration::of::<CacheTest>(),
        SectionRegistration::of::<Inner>(),
        // Registered twice: counted once.
        SectionRegistration::of::<CacheTest>(),
        SectionRegistration::of::<ByHand>(),
    ])
}

#[test]
fn the_registry_lists_each_section_once_in_key_order() {
    let registry = registry();
    let keys: Vec<&str> = registry.sections().iter().map(|s| s.key).collect();
    assert_eq!(keys, ["byhand", "cachetest", "cachetest.inner", "needs"]);
    assert!(registry.section("cachetest").is_some());
    assert!(registry.section("nothing").is_none());
}

#[test]
fn the_registry_collects_defaults_secrets_and_fallbacks() {
    let registry = registry();
    let defaults = registry.defaults_tree();
    assert_eq!(defaults["cachetest"]["store"], json!("memory"));
    assert_eq!(defaults["cachetest"]["cors"]["credentials"], json!(false));
    assert_eq!(defaults["cachetest"]["inner"], json!({ "enabled": false }));
    assert!(defaults.get("byhand").is_none());
    assert!(defaults.get("needs").is_none());

    assert_eq!(
        registry.secret_paths(),
        ["cachetest.redis_password", "cachetest.api_token"]
    );
    assert_eq!(
        registry.env_fallbacks(),
        [EnvFallback {
            key: "cachetest.url".to_owned(),
            variable: "REDIS_URL".to_owned()
        }]
    );
    assert_eq!(registry.unmarked_secrets(), ["needs.password"]);
}

#[test]
fn the_json_schema_nests_sections_and_tables() {
    let document = registry().schema_document();
    assert_eq!(document["type"], "object");
    let cache = &document["properties"]["cachetest"];
    assert_eq!(cache["description"], "Cache settings.");
    assert_eq!(
        cache["properties"]["store"]["enum"],
        json!(["memory", "redis"])
    );
    assert_eq!(cache["properties"]["api_token"]["x-secret"], json!(true));
    assert_eq!(
        cache["properties"]["cors"]["properties"]["credentials"]["type"],
        "boolean"
    );
    // A section below another sits inside it.
    assert_eq!(
        cache["properties"]["inner"]["properties"]["enabled"]["type"],
        "boolean"
    );
    assert_eq!(
        document["properties"]["needs"]["required"],
        json!(["address", "password"])
    );
}

#[test]
fn a_hand_written_section_has_no_schema_and_still_registers() {
    let registry = ConfigRegistry::new(&[SectionRegistration::of::<ByHand>()]);
    let section = registry.section("byhand").expect("byhand");
    assert!(section.schema.is_none());
    assert!(section.defaults.is_none());
    assert!(
        registry.schema_document()["properties"]
            .as_object()
            .expect("object")
            .is_empty()
    );
}
