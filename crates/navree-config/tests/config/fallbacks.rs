//! Environment fallbacks read through the declaration: the variable a module
//! reads is the one `config:show --fallbacks` and the reference list.

use navree_config::{
    Config, ConfigRegistry, ConfigSection, ConfigSources, ConfigTable, SectionRegistration,
    env_fallback, table_env_fallback,
};
use serde::Deserialize;

/// A provider's settings, one entry of a map.
#[derive(Clone, Debug, Default, Deserialize, ConfigTable)]
#[serde(default)]
#[allow(dead_code)]
struct Provider {
    /// The region.
    #[config(env_fallback = "NAVREE_FALLBACK_TEST_REGION")]
    region: Option<String>,
    /// The model.
    model: Option<String>,
}

/// The engine's settings.
#[derive(Clone, Debug, Default, Deserialize, ConfigTable)]
#[serde(default)]
#[allow(dead_code)]
struct Engine {
    /// The executable.
    #[config(env_fallback = "NAVREE_FALLBACK_TEST_CHROME")]
    path: Option<String>,
}

/// A section with fallbacks at the top and in a table.
#[derive(Clone, Debug, Default, Deserialize, ConfigSection)]
#[config(key = "fallbacktest")]
#[serde(default)]
#[allow(dead_code)]
struct FallbackTest {
    /// The server.
    #[config(env_fallback = "NAVREE_FALLBACK_TEST_URL")]
    url: Option<String>,
    /// A key without one.
    prefix: String,
    /// The engine.
    #[config(table)]
    engine: Engine,
}

/// An environment of `pairs`.
fn env(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
    move |name| {
        pairs
            .iter()
            .find(|(variable, _)| *variable == name)
            .map(|(_, value)| (*value).to_owned())
    }
}

#[test]
fn the_declared_variable_is_found_by_the_fields_key() {
    assert_eq!(
        env_fallback::<FallbackTest>("url").as_deref(),
        Some("NAVREE_FALLBACK_TEST_URL")
    );
    assert_eq!(
        env_fallback::<FallbackTest>("engine.path").as_deref(),
        Some("NAVREE_FALLBACK_TEST_CHROME")
    );
    assert_eq!(env_fallback::<FallbackTest>("prefix"), None);
    assert_eq!(env_fallback::<FallbackTest>("nope"), None);
    assert_eq!(env_fallback::<FallbackTest>("engine.nope"), None);
    assert_eq!(
        table_env_fallback::<Provider>("region").as_deref(),
        Some("NAVREE_FALLBACK_TEST_REGION")
    );
    assert_eq!(table_env_fallback::<Provider>("model"), None);
}

#[test]
fn a_fallback_fills_only_a_key_the_configuration_leaves_out() {
    let vars = env(&[
        ("NAVREE_FALLBACK_TEST_URL", "redis://cache:6379"),
        ("NAVREE_FALLBACK_TEST_CHROME", "/usr/bin/chromium"),
    ]);
    let config = Config::load(&ConfigSources::new()).unwrap();
    let url = config
        .fallback_with::<FallbackTest>("url", &vars)
        .expect("the variable is set");
    assert_eq!(url.variable, "NAVREE_FALLBACK_TEST_URL");
    assert_eq!(url.value, "redis://cache:6379");
    let path = config
        .fallback_with::<FallbackTest>("engine.path", &vars)
        .expect("a table's field falls back too");
    assert_eq!(path.value, "/usr/bin/chromium");

    // A file or an APP_ variable wins.
    let config = Config::load(
        &ConfigSources::new()
            .set("fallbacktest.url", "redis://main:6379")
            .env_vars("APP_", [("APP_FALLBACKTEST__ENGINE__PATH", "/opt/chrome")]),
    )
    .unwrap();
    assert_eq!(config.fallback_with::<FallbackTest>("url", &vars), None);
    assert_eq!(
        config.fallback_with::<FallbackTest>("engine.path", &vars),
        None
    );
}

#[test]
fn a_blank_or_missing_variable_and_an_undeclared_field_give_nothing() {
    let config = Config::load(&ConfigSources::new()).unwrap();
    let blank = env(&[("NAVREE_FALLBACK_TEST_URL", "  ")]);
    assert_eq!(config.fallback_with::<FallbackTest>("url", blank), None);
    assert_eq!(config.fallback_with::<FallbackTest>("url", env(&[])), None);
    // `prefix` declares no variable, so none is read, whatever is set.
    let read = std::cell::Cell::new(false);
    let watching = |_: &str| {
        read.set(true);
        Some("x".to_owned())
    };
    assert_eq!(
        config.fallback_with::<FallbackTest>("prefix", watching),
        None
    );
    assert!(!read.get());
}

#[test]
fn a_fallback_never_shows_its_value() {
    let config = Config::load(&ConfigSources::new()).unwrap();
    let fallback = config
        .fallback_with::<FallbackTest>("url", env(&[("NAVREE_FALLBACK_TEST_URL", "s3cret")]))
        .unwrap();
    let debug = format!("{fallback:?}");
    assert!(debug.contains("NAVREE_FALLBACK_TEST_URL"), "{debug}");
    assert!(!debug.contains("s3cret"), "{debug}");
}

/// What a module reads is what `config:show --fallbacks` lists.
#[test]
fn the_variables_read_are_the_ones_listed() {
    let registry = ConfigRegistry::new(&[SectionRegistration::of::<FallbackTest>()]);
    let listed: Vec<(String, String)> = registry
        .env_fallbacks()
        .into_iter()
        .map(|fallback| (fallback.key, fallback.variable))
        .collect();
    for (key, variable) in &listed {
        let field = key
            .strip_prefix("fallbacktest.")
            .expect("a key of the section");
        assert_eq!(
            env_fallback::<FallbackTest>(field).as_ref(),
            Some(variable),
            "{key}"
        );
    }
    assert_eq!(listed.len(), 2, "{listed:?}");
}
