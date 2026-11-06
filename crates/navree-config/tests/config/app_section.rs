//! The `app` section is typed. The short names (`APP_URL`,
//! `APP_ENV`, `APP_KEY`, ...) resolve into it, the section is validated, and
//! the readers need no string lookups.

use navree_config::{
    AppOptions, Config, ConfigModule, ConfigSection, ConfigSources, Environment, ValueOrigin,
};
use navree_kernel::Navree;
use serde_json::json;
use tempfile::TempDir;

fn env(pairs: &[(&str, &str)]) -> Config {
    Config::load(&ConfigSources::new().env_vars("APP_", pairs.iter().copied())).expect("load")
}

fn write(dir: &TempDir, name: &str, contents: &str) -> std::path::PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, contents).expect("write fixture");
    path
}

#[test]
fn the_section_reads_every_setting() {
    let config = Config::load(&ConfigSources::new().set(
        "app",
        json!({
            "name": "agra",
            "url": "https://agra.example.com",
            "version": "1.4.0",
            "env": "Production",
            "key": "base64:abc",
            "previous_keys": ["base64:old1", "base64:old2"],
        }),
    ))
    .expect("load");
    let app = config.app().expect("app");
    assert_eq!(app.name.as_deref(), Some("agra"));
    assert_eq!(app.url.as_deref(), Some("https://agra.example.com"));
    assert_eq!(app.version.as_deref(), Some("1.4.0"));
    assert_eq!(app.env, Some(Environment::Production));
    assert_eq!(
        app.key.as_ref().map(|k| k.expose().as_str()),
        Some("base64:abc")
    );
    let previous: Vec<&str> = app
        .previous_keys
        .iter()
        .map(|k| k.expose().as_str())
        .collect();
    assert_eq!(previous, ["base64:old1", "base64:old2"]);
}

#[test]
fn an_absent_section_is_all_unset() {
    let app = Config::load(&ConfigSources::new())
        .expect("load")
        .app()
        .expect("app");
    assert!(app.name.is_none() && app.url.is_none() && app.version.is_none());
    assert!(app.env.is_none() && app.key.is_none() && app.previous_keys.is_empty());
}

#[test]
fn the_short_environment_variables_set_the_app_keys() {
    let config = env(&[
        ("APP_NAME", "agra"),
        ("APP_URL", "https://acme.test"),
        ("APP_VERSION", "2.0.0"),
        ("APP_ENV", "staging"),
        ("APP_KEY", "base64:abc"),
        ("APP_PREVIOUS_KEYS", " base64:a , base64:b ,"),
    ]);
    // The aliases show up under `app.*`, with the origin of the variable.
    assert_eq!(
        config.get_string("app.url").as_deref(),
        Some("https://acme.test")
    );
    assert_eq!(
        config.origin("app.env"),
        Some(ValueOrigin::Environment {
            variable: "APP_ENV".to_owned()
        })
    );
    let app = config.app().expect("app");
    assert_eq!(app.name.as_deref(), Some("agra"));
    assert_eq!(app.env, Some(Environment::Staging));
    let previous: Vec<&str> = app
        .previous_keys
        .iter()
        .map(|k| k.expose().as_str())
        .collect();
    assert_eq!(
        previous,
        ["base64:a", "base64:b"],
        "comma-separated, trimmed"
    );
}

#[test]
fn the_full_spelling_works_too() {
    let config = env(&[
        ("APP_APP__URL", "https://full.test"),
        ("APP_APP__ENV", "local"),
    ]);
    let app = config.app().expect("app");
    assert_eq!(app.url.as_deref(), Some("https://full.test"));
    assert_eq!(app.env, Some(Environment::Local));
}

#[test]
fn app_key_wins_over_the_short_key_from_the_same_source() {
    // The same pin as the crypto module's: `app.key` beats `key`.
    let config = env(&[("APP_KEY", "short"), ("APP_APP__KEY", "full")]);
    assert_eq!(config.app().unwrap().key.unwrap().expose().as_str(), "full");
}

#[test]
fn the_short_environment_wins_over_the_app_env_from_the_same_source() {
    // Documented since the first release: `env` (`APP_ENV`), else `app.env`.
    let config = env(&[("APP_ENV", "development"), ("APP_APP__ENV", "production")]);
    assert_eq!(
        config.environment(),
        Some(Environment::Other("development".to_owned()))
    );
    let config = env(&[("APP_ENV", "production"), ("APP_APP__ENV", "development")]);
    assert_eq!(config.environment(), Some(Environment::Production));
}

#[test]
fn the_source_that_ranks_higher_wins_whatever_the_spelling() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = write(
        &dir,
        "app.toml",
        "[app]\nenv = \"staging\"\nurl = \"https://file.test\"\n",
    );
    // The real environment ranks above the file: `APP_ENV` beats `[app] env`.
    let config = Config::load(
        &ConfigSources::new()
            .file(&file)
            .env_vars("APP_", [("APP_ENV", "production")]),
    )
    .expect("load");
    assert_eq!(config.environment(), Some(Environment::Production));
    assert_eq!(
        config.get_string("app.url").as_deref(),
        Some("https://file.test")
    );
    // A short name in a file that ranks below another file loses to it.
    let env = write(&dir, ".env", "APP_URL=https://dotenv.test\nAPP_ENV=local\n");
    let config = Config::load(&ConfigSources::new().dotenv(&env).file(&file)).expect("load");
    assert_eq!(
        config.environment(),
        Some(Environment::Staging),
        "file over .env"
    );
    assert_eq!(
        config.get_string("app.url").as_deref(),
        Some("https://file.test")
    );
    // With only `.env`, its short names apply.
    let config = Config::load(&ConfigSources::new().dotenv(&env)).expect("load");
    assert_eq!(config.environment(), Some(Environment::Local));
    assert_eq!(
        config.get_string("app.url").as_deref(),
        Some("https://dotenv.test")
    );
}

#[test]
fn the_top_level_short_keys_stay_in_the_tree() {
    // Readers that still look at `key` and `env` keep working.
    let config = env(&[("APP_KEY", "base64:abc"), ("APP_ENV", "production")]);
    assert_eq!(config.get_string("key").as_deref(), Some("base64:abc"));
    assert_eq!(config.get_string("env").as_deref(), Some("production"));
    assert_eq!(config.get_string("app.key").as_deref(), Some("base64:abc"));
}

#[test]
fn a_blank_value_is_not_set() {
    let config = env(&[
        ("APP_KEY", ""),
        ("APP_ENV", " "),
        ("APP_URL", ""),
        ("APP_NAME", "  "),
        ("APP_PREVIOUS_KEYS", ""),
    ]);
    let app = config.app().expect("blank values validate");
    assert!(app.key.is_none() && app.env.is_none() && app.url.is_none() && app.name.is_none());
    assert!(app.previous_keys.is_empty());
    assert_eq!(config.environment(), None);
}

#[test]
fn a_bad_url_is_rejected_naming_the_key() {
    for bad in [
        "acme.test",
        "ftp://acme.test",
        "https://",
        "https://a b.test",
        "/relative",
    ] {
        let config = Config::load(&ConfigSources::new().set("app.url", bad)).expect("load");
        let text = config.app().unwrap_err().to_string();
        assert!(text.contains("`app`"), "{bad}: {text}");
        assert!(
            text.contains("app.url must be an absolute http or https address"),
            "{bad}: {text}"
        );
    }
    for good in [
        "http://localhost:3000",
        "https://agra.example.com/",
        "http://127.0.0.1:8080",
    ] {
        let config = Config::load(&ConfigSources::new().set("app.url", good)).expect("load");
        assert!(config.app().is_ok(), "{good}");
    }
}

#[test]
fn a_bad_environment_name_is_rejected() {
    let config = Config::load(&ConfigSources::new().set("app.env", "not a name")).expect("load");
    let text = config.app().unwrap_err().to_string();
    assert!(text.contains("app.env must be a name"), "{text}");
    for good in ["local", "qa-2", "preview_7", "Production"] {
        let config = Config::load(&ConfigSources::new().set("app.env", good)).expect("load");
        assert!(config.app().is_ok(), "{good}");
    }
}

#[test]
fn the_readers_that_must_not_fail_return_none_for_an_invalid_section() {
    let config = Config::load(
        &ConfigSources::new()
            .set("app.url", "bad")
            .set("app.name", "agra"),
    )
    .expect("load");
    assert_eq!(config.app_url(), None);
    assert_eq!(config.app_name(), None, "an invalid section reads as unset");
    assert_eq!(config.environment(), None);
}

#[test]
fn the_readers_return_the_settings() {
    let config = env(&[
        ("APP_NAME", "agra"),
        ("APP_URL", "https://acme.test"),
        ("APP_VERSION", "1.2.3"),
    ]);
    assert_eq!(config.app_name().as_deref(), Some("agra"));
    assert_eq!(config.app_url().as_deref(), Some("https://acme.test"));
    assert_eq!(config.app_version().as_deref(), Some("1.2.3"));
}

#[test]
fn a_secret_key_never_prints() {
    let config = env(&[
        ("APP_KEY", "base64:SECRETSECRET"),
        ("APP_PREVIOUS_KEYS", "base64:OLDOLD"),
    ]);
    let app = config.app().expect("app");
    let debug = format!("{app:?}");
    assert!(
        !debug.contains("SECRETSECRET") && !debug.contains("OLDOLD"),
        "{debug}"
    );
    assert!(debug.contains("[redacted]"), "{debug}");
    let redacted = config.redacted().to_string();
    assert!(!redacted.contains("SECRETSECRET"), "{redacted}");
}

#[test]
fn a_wrong_shape_names_the_key_and_not_the_value() {
    let config = Config::load(&ConfigSources::new().set("app.previous_keys", json!({"a": 1})))
        .expect("load");
    let text = config.app().unwrap_err().to_string();
    assert!(text.contains("`app`"), "{text}");
    assert!(text.contains("previous_keys"), "{text}");
}

#[test]
fn the_section_key_is_app() {
    assert_eq!(AppOptions::KEY, "app");
    assert!(AppOptions::defaults().is_some());
}

#[tokio::test]
async fn config_module_provides_the_app_options_and_validates_them_at_boot() {
    let app = Navree::app()
        .module(ConfigModule::for_root(ConfigSources::new().env_vars(
            "APP_",
            [("APP_URL", "https://acme.test"), ("APP_ENV", "qa")],
        )))
        .boot()
        .await
        .expect("boot");
    let options = app.ctx().get::<AppOptions>().await.expect("app options");
    assert_eq!(options.url.as_deref(), Some("https://acme.test"));
    assert_eq!(options.env, Some(Environment::Other("qa".to_owned())));

    let errors = Navree::app()
        .module(ConfigModule::for_root(
            ConfigSources::new().env_vars("APP_", [("APP_URL", "acme.test")]),
        ))
        .boot()
        .await
        .expect_err("a bad url fails the boot");
    let text = errors.to_string();
    assert!(
        text.contains("app.url must be an absolute http or https address"),
        "{text}"
    );
    assert!(
        !text.contains("acme.test"),
        "the value is not echoed: {text}"
    );
}
