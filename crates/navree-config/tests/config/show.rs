//! CONFIG-004: `config:show`. The effective configuration with the origin of
//! every value, secrets masked, and no way to reveal them.

use navree_config::show::{ShowOptions, defaults_tree, render};
use navree_config::{
    AppOptions, Config, ConfigModule, ConfigSection, ConfigSources, SectionRegistration,
    defaults_of,
};
use navree_kernel::{Module, ModuleBuilder, Navree};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tempfile::TempDir;

const PASSWORD: &str = "hunter2-very-secret";

fn write(dir: &TempDir, name: &str, contents: &str) -> std::path::PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, contents).expect("write fixture");
    path
}

fn show(config: &Config, options: &ShowOptions) -> String {
    render(config, &json!({}), options)
}

fn options(args: &[&str]) -> ShowOptions {
    ShowOptions::parse(&args.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>()).expect("options")
}

fn config() -> Config {
    Config::load(
        &ConfigSources::new()
            .set("http.addr", "127.0.0.1:8080")
            .set(
                "database.url",
                format!("postgres://app:{PASSWORD}@db.internal/acme"),
            )
            .set("auth.jwt.secret", PASSWORD)
            .set("mail.webhooks.token", PASSWORD)
            .set("app.key", format!("base64:{PASSWORD}"))
            .set("queue.workers", 4)
            .set("log.levels", json!(["info", "debug"])),
    )
    .expect("load")
}

#[test]
fn the_table_shows_every_key_with_its_value() {
    let text = show(&config(), &options(&[]));
    assert!(text.contains("http.addr"), "{text}");
    assert!(text.contains("\"127.0.0.1:8080\""), "{text}");
    assert!(text.contains("queue.workers"), "{text}");
    assert!(text.contains(" 4"), "numbers are not quoted: {text}");
    assert!(text.contains(r#"["info","debug"]"#), "{text}");
    assert!(
        !text.contains("ConfigSources::set"),
        "origins only with --origin: {text}"
    );
}

#[test]
fn secrets_are_masked_and_there_is_no_way_to_reveal_them() {
    let text = show(&config(), &options(&["--origin", "--defaults"]));
    assert!(!text.contains(PASSWORD), "{text}");
    assert!(text.contains("auth.jwt.secret"), "{text}");
    assert!(text.contains("[redacted] (set)"), "{text}");
    // The password of a URL is masked, the rest of the URL is not.
    assert!(
        text.contains("postgres://app:[redacted]@db.internal/acme"),
        "{text}"
    );
    for flag in [
        "--reveal",
        "--show-secrets",
        "--secrets",
        "--unmask",
        "-s",
        "--raw",
    ] {
        let error = ShowOptions::parse(&[flag.to_owned()]).unwrap_err();
        assert!(error.contains("unknown option"), "{flag}: {error}");
    }
}

#[test]
fn the_json_view_masks_too() {
    let plain = show(&config(), &options(&["--json"]));
    let origin = show(&config(), &options(&["--json", "--origin"]));
    for text in [&plain, &origin] {
        assert!(!text.contains(PASSWORD), "{text}");
        let parsed: serde_json::Value = serde_json::from_str(text).expect("json");
        assert!(parsed.is_object());
    }
    let plain: serde_json::Value = serde_json::from_str(&plain).unwrap();
    assert_eq!(plain["http"]["addr"], "127.0.0.1:8080");
    assert_eq!(plain["auth"]["jwt"]["secret"], "[redacted]");
    assert_eq!(plain["queue"]["workers"], "4".parse::<u64>().unwrap());
    let origin: serde_json::Value = serde_json::from_str(&origin).unwrap();
    assert_eq!(origin["http.addr"]["value"], "127.0.0.1:8080");
    assert_eq!(origin["http.addr"]["origin"], "ConfigSources::set");
    assert_eq!(origin["app.key"]["value"], "[redacted]");
}

#[test]
fn origins_name_the_source_of_each_value() {
    let dir = tempfile::tempdir().expect("tempdir");
    let env = write(&dir, ".env", "APP_MAIL__FROM=\"Acme <hello@acme.test>\"\n");
    let file = write(&dir, "app.toml", "[http]\naddr = \"0.0.0.0:80\"\n");
    let config = Config::load(
        &ConfigSources::new()
            .dotenv(&env)
            .file(&file)
            .env_vars("APP_", [("APP_MAIL__TRANSPORT", "log")]),
    )
    .expect("load");
    let text = show(&config, &options(&["--origin"]));
    let line = |key: &str| {
        text.lines()
            .find(|l| l.starts_with(key))
            .unwrap_or_else(|| panic!("{key} in {text}"))
            .to_owned()
    };
    assert!(line("mail.from").ends_with(&format!("{}:1 (APP_MAIL__FROM)", env.display())));
    assert!(line("http.addr").ends_with(&file.display().to_string()));
    assert!(line("mail.transport").ends_with("environment (APP_MAIL__TRANSPORT)"));
}

#[test]
fn a_section_limits_the_keys() {
    let text = show(&config(), &options(&["database"]));
    assert!(text.contains("database.url"), "{text}");
    assert!(!text.contains("http.addr"), "{text}");
    // A section name is a whole path segment: `data` is not `database`.
    let text = show(&config(), &options(&["data"]));
    assert!(text.contains("nothing is set under `data`"), "{text}");
    let text = show(&config(), &options(&["auth.jwt"]));
    assert!(
        text.contains("auth.jwt.secret") && !text.contains("queue"),
        "{text}"
    );
}

#[test]
fn the_defaults_fill_in_what_no_source_sets() {
    let config = Config::load(&ConfigSources::new().set("app.name", "acme")).expect("load");
    let defaults = defaults_tree(&[SectionRegistration::of::<AppOptions>()]);
    let without = render(&config, &defaults, &options(&["app"]));
    assert!(
        !without.contains("app.url"),
        "defaults only with --defaults: {without}"
    );

    let with = render(
        &config,
        &defaults,
        &options(&["app", "--defaults", "--origin"]),
    );
    assert!(
        with.contains("app.name") && with.contains("\"acme\""),
        "{with}"
    );
    let url = with
        .lines()
        .find(|l| l.starts_with("app.url"))
        .expect("app.url");
    assert!(url.contains("(unset)") && url.ends_with("default"), "{url}");
    // A secret default that is unset says so; it never invents a value.
    let key = with
        .lines()
        .find(|l| l.starts_with("app.key"))
        .expect("app.key");
    assert!(key.contains("(unset)"), "{key}");
    // A value a source sets wins over its default.
    let name = with
        .lines()
        .find(|l| l.starts_with("app.name"))
        .expect("app.name");
    assert!(name.ends_with("ConfigSources::set"), "{name}");
}

#[test]
fn a_secret_that_is_set_shows_as_set_not_as_its_value() {
    let config = Config::load(&ConfigSources::new().set("app.key", "base64:xyz")).expect("load");
    let text = render(
        &config,
        &defaults_tree(&[SectionRegistration::of::<AppOptions>()]),
        &options(&["app", "--defaults"]),
    );
    let key = text
        .lines()
        .find(|l| l.starts_with("app.key"))
        .expect("app.key");
    assert!(key.contains("[redacted] (set)"), "{key}");
    assert!(!text.contains("base64:xyz"));
}

#[test]
fn the_options_reject_what_they_do_not_know() {
    assert!(ShowOptions::parse(&["a".to_owned(), "b".to_owned()]).is_err());
    assert!(ShowOptions::parse(&["--bogus".to_owned()]).is_err());
    let parsed = options(&["mail", "--json", "--defaults", "--origin"]);
    assert_eq!(
        parsed,
        ShowOptions {
            section: Some("mail".to_owned()),
            defaults: true,
            origin: true,
            json: true
        }
    );
}

#[test]
fn a_section_can_publish_its_defaults_from_default_and_serialize() {
    #[derive(Default, Deserialize, Serialize)]
    #[serde(default)]
    struct Pool {
        size: u32,
        name: String,
    }
    impl ConfigSection for Pool {
        const KEY: &'static str = "pool";

        fn defaults() -> Option<serde_json::Value> {
            defaults_of::<Self>()
        }
    }
    let config = Config::load(&ConfigSources::new().set("pool.size", 8)).expect("load");
    let defaults = defaults_tree(&[SectionRegistration::of::<Pool>()]);
    let text = render(&config, &defaults, &options(&["--defaults"]));
    assert!(text.contains("pool.size") && text.contains(" 8"), "{text}");
    assert!(
        text.contains("pool.name") && text.contains("\"\""),
        "{text}"
    );
}

// `config:show` through the app: an offline command that reads the app's
// sources and sections without booting it.
#[tokio::test]
async fn the_command_runs_without_booting_the_app() {
    // A provider that would fail if the app booted proves it does not.
    struct Boom;
    impl Module for Boom {
        fn register(&self, m: &mut ModuleBuilder) {
            m.factory(navree_kernel::Factory::<u8>::new(|_| async {
                Err::<std::sync::Arc<u8>, navree_kernel::BoxError>("booted".into())
            }));
        }
    }
    let sources = ConfigSources::new().set("http.addr", "127.0.0.1:1");
    Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(Boom)
        .run_with_args(vec!["config:show".to_owned(), "--origin".to_owned()])
        .await
        .expect("config:show runs without a boot");
}

#[tokio::test]
async fn the_command_rejects_an_unknown_option() {
    let error = Navree::app()
        .module(ConfigModule::for_root(ConfigSources::new()))
        .run_with_args(vec!["config:show".to_owned(), "--reveal".to_owned()])
        .await
        .expect_err("no such option");
    assert!(
        error.to_string().contains("unknown option `--reveal`"),
        "{error}"
    );
}

#[test]
fn the_short_names_of_the_app_settings_are_listed_under_app() {
    let config = Config::load(&ConfigSources::new().env_vars(
        "APP_",
        [("APP_ENV", "production"), ("APP_KEY", "base64:abc")],
    ))
    .expect("load");
    let text = show(&config, &options(&["--origin"]));
    let env = text
        .lines()
        .find(|l| l.starts_with("app.env"))
        .unwrap_or_else(|| panic!("app.env in {text}"));
    assert!(env.ends_with("environment (APP_ENV)"), "{env}");
    assert!(
        !text
            .lines()
            .any(|l| l.starts_with("env ") || l.starts_with("key ")),
        "no top-level duplicates: {text}"
    );
    assert!(
        text.contains("app.key") && text.contains("[redacted] (set)"),
        "{text}"
    );
    assert!(!text.contains("base64:abc"), "{text}");
}
