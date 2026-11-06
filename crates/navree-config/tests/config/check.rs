//! CONFIG-007 and the production profile: `config:check` finds
//! every problem at once, names unknown keys and sections, lists hidden
//! `.env` values, and fails (exit code 1) on errors and unknown keys.

use navree_config::check::{CheckOptions, CheckReport, Finding, production_profile};
use navree_config::{
    Config, ConfigModule, ConfigSection, ConfigSources, Environment, ModuleBuilderConfigExt,
    SectionRegistration,
};
use navree_kernel::{Module, ModuleBuilder, Navree};
use serde::Deserialize;
use tempfile::TempDir;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
#[allow(dead_code)]
struct Database {
    url: String,
    max_connections: u32,
}

impl ConfigSection for Database {
    const KEY: &'static str = "database";

    fn validate(&self) -> Result<(), String> {
        if self.max_connections == 0 {
            return Err("max_connections must be at least 1".to_owned());
        }
        Ok(())
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
#[allow(dead_code)]
struct Queue {
    workers: u32,
}

impl ConfigSection for Queue {
    const KEY: &'static str = "queue";

    fn validate(&self) -> Result<(), String> {
        if self.workers == 0 {
            return Err("workers must be at least 1".to_owned());
        }
        Ok(())
    }
}

fn registrations() -> Vec<SectionRegistration> {
    vec![
        SectionRegistration::of::<navree_config::AppOptions>(),
        SectionRegistration::of::<Database>(),
        SectionRegistration::of::<Queue>(),
    ]
}

fn check(sources: ConfigSources) -> CheckReport {
    CheckReport::run(&sources, &registrations(), &CheckOptions::default()).expect("load")
}

fn good() -> ConfigSources {
    ConfigSources::new()
        .set("database.url", "sqlite://app.db")
        .set("database.max_connections", 5)
        .set("queue.workers", 2)
}

#[test]
fn a_clean_configuration_passes_with_exit_code_0() {
    let report = check(good());
    assert!(report.is_ok(), "{}", report.render());
    assert_eq!(report.exit_code(), 0);
    assert_eq!(report.problems(), 0);
    assert_eq!(report.sections, 3);
    assert!(
        report
            .render()
            .contains("config:check: ok (3 sections read)")
    );
}

#[test]
fn every_invalid_section_is_listed_at_once_and_fails_with_exit_code_1() {
    let report = check(
        ConfigSources::new()
            .set("database.max_connections", 0)
            .set("queue.workers", 0)
            .set("app.url", "nowhere"),
    );
    assert_eq!(report.errors.len(), 3, "{}", report.render());
    assert_eq!(report.exit_code(), 1);
    let text = report.render();
    assert!(
        text.contains("`database`") && text.contains("max_connections must be at least 1"),
        "{text}"
    );
    assert!(
        text.contains("`queue`") && text.contains("workers must be at least 1"),
        "{text}"
    );
    assert!(
        text.contains("app.url must be an absolute http or https address"),
        "{text}"
    );
    assert!(text.contains("config:check: 3 problems"), "{text}");
}

#[test]
fn unknown_keys_and_sections_fail_the_check() {
    let report = check(
        good()
            .set("database.max_conections", 9)
            .set("queu.workers", 1),
    );
    assert!(report.errors.is_empty());
    let keys: Vec<&str> = report.unknown.iter().map(|u| u.key.as_str()).collect();
    assert_eq!(keys, ["database.max_conections", "queu"]);
    assert_eq!(report.exit_code(), 1);
    let text = report.render();
    assert!(
        text.contains("unknown  unknown configuration key `database.max_conections`"),
        "{text}"
    );
    assert!(
        text.contains("Did you mean `database.max_connections`?"),
        "{text}"
    );
    assert!(text.contains("Did you mean `queue`?"), "{text}");
    assert!(text.contains("config:check: 2 problems"), "{text}");
}

#[test]
fn a_framework_section_is_never_reported_as_unknown() {
    // The check has no booted app to ask which sections it uses: the names
    // the framework reads are known.
    let report = check(
        good()
            .set("mail.transport", "log")
            .set("hashing.memory_kib", 1024)
            .set("ui.debug", false)
            .set("validation.phone.default_region", "IN")
            .set("observe.tracing.trace_url", "http://t")
            .set("database.mongodb.url", "mongodb://localhost"),
    );
    assert!(report.unknown.is_empty(), "{}", report.render());
    assert!(report.is_ok());
}

#[test]
fn a_section_with_a_registration_still_finds_typos_inside_it() {
    let report = check(good().set("queue.worker", 1));
    assert_eq!(report.unknown.len(), 1, "{}", report.render());
    assert_eq!(report.unknown[0].key, "queue.worker");
}

#[test]
fn a_key_set_in_dotenv_and_in_a_file_is_a_warning_not_a_failure() {
    let dir: TempDir = tempfile::tempdir().expect("tempdir");
    let env = dir.path().join(".env");
    let file = dir.path().join("app.toml");
    std::fs::write(&env, "APP_QUEUE__WORKERS=8\n").expect("write");
    std::fs::write(
        &file,
        "[queue]\nworkers = 2\n[database]\nmax_connections = 5\n",
    )
    .expect("write");
    // `.env` ranks above the file, as in the default sources.
    let report = check(ConfigSources::new().file(&file).dotenv(&env));
    assert_eq!(report.shadowed.len(), 1);
    assert!(report.is_ok(), "{}", report.render());
    assert_eq!(report.exit_code(), 0, "a warning does not fail the check");
    let text = report.render();
    assert!(
        text.contains("warning  `queue.workers` is set in both places"),
        "{text}"
    );
    assert!(text.contains("the `.env` value is used"), "{text}");
    assert!(!text.contains("config:check: 1 problem"), "{text}");

    // The other order reports the same key, the other way round.
    let report = check(ConfigSources::new().dotenv(&env).file(&file));
    assert_eq!(report.shadowed.len(), 1);
    assert_eq!(report.exit_code(), 0);
    assert!(
        report.render().contains("the config file value is used"),
        "{}",
        report.render()
    );
}

#[test]
fn the_report_never_prints_a_value() {
    let report = check(
        good()
            .set("database.url", "postgres://u:hunter2@db/x")
            .set("databse.password", "hunter2")
            .set("queue.workers", 0),
    );
    let text = report.render();
    assert!(!text.contains("hunter2"), "{text}");
    assert!(!text.contains("postgres://"), "{text}");
}

#[test]
fn a_file_that_cannot_be_loaded_is_an_error_not_a_report() {
    let dir: TempDir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("app.toml");
    std::fs::write(&file, "[queue\nworkers = 2\n").expect("write");
    let error = CheckReport::run(
        &ConfigSources::new().file(&file),
        &registrations(),
        &CheckOptions::default(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("invalid TOML"), "{error}");
}

// ---------------------------------------------------------------------------
// The production profile
// ---------------------------------------------------------------------------

fn production() -> CheckOptions {
    CheckOptions {
        environment: Some("production".to_owned()),
    }
}

fn findings(report: &CheckReport) -> Vec<(&'static str, bool)> {
    report
        .profile
        .iter()
        .map(|f: &Finding| (f.key, f.fail))
        .collect()
}

#[test]
fn without_production_there_is_no_profile() {
    let report = check(good());
    assert!(report.profile.is_empty());
    assert!(!report.render().contains("production profile"));
    let report = CheckReport::run(
        &good().set("app.env", "qa"),
        &registrations(),
        &CheckOptions::default(),
    )
    .expect("load");
    assert!(
        report.profile.is_empty(),
        "only production and staging take the production profile"
    );
    let report = CheckReport::run(
        &good().set("app.env", "staging"),
        &registrations(),
        &CheckOptions::default(),
    )
    .expect("load");
    assert!(
        !report.profile.is_empty(),
        "staging takes the production profile"
    );
}

#[test]
fn env_production_applies_the_profile_and_fails_on_unsafe_settings() {
    let report = CheckReport::run(
        &good().set("mail.transport", "log"),
        &registrations(),
        &production(),
    )
    .expect("load");
    assert_eq!(report.environment, Some(Environment::Production));
    let found = findings(&report);
    assert!(found.contains(&("app.key", true)), "{found:?}");
    assert!(
        found.contains(&("database.url", true)),
        "SQLite file: {found:?}"
    );
    assert!(found.contains(&("mail.transport", true)), "{found:?}");
    assert!(found.contains(&("app.url", false)), "a note: {found:?}");
    assert_eq!(report.exit_code(), 1);
    let text = report.render();
    assert!(
        text.contains("production profile (production): FAIL"),
        "{text}"
    );
    assert!(text.contains("fail  app.key:"), "{text}");
    assert!(text.contains("note  app.url:"), "{text}");
}

#[test]
fn a_safe_production_configuration_passes() {
    let report = CheckReport::run(
        &ConfigSources::new()
            .set("app.key", "base64:abc")
            .set("app.url", "https://acme.example.com")
            .set("database.url", "postgres://u:p@db/acme")
            .set("database.max_connections", 10)
            .set("mail.transport", "smtp://mail.acme.example.com:587")
            .set("queue.workers", 4),
        &registrations(),
        &production(),
    )
    .expect("load");
    assert!(report.profile.is_empty(), "{:?}", report.profile);
    assert!(report.is_ok(), "{}", report.render());
    assert!(
        report
            .render()
            .contains("production profile (production): ok")
    );
}

#[test]
fn tracing_everything_to_an_exporter_and_a_public_api_reference_are_notes() {
    let safe = || {
        ConfigSources::new()
            .set("app.key", "base64:abc")
            .set("app.url", "https://acme.example.com")
            .set("database.url", "postgres://u:p@db/acme")
            .set("mail.transport", "smtp://mail.acme.example.com:587")
    };
    let report = CheckReport::run(
        &safe()
            .set("observe.tracing.endpoint", "http://collector:4317")
            .set("api.docs_ui", true),
        &registrations(),
        &production(),
    )
    .expect("load");
    let found = findings(&report);
    assert!(
        found.contains(&("observe.tracing.sample_ratio", false)),
        "{found:?}"
    );
    assert!(found.contains(&("api.docs_ui", false)), "{found:?}");
    assert!(report.profile.iter().all(|finding| !finding.fail));

    // A ratio below 1, or no exporter, is quiet.
    let report = CheckReport::run(
        &safe()
            .set("observe.tracing.endpoint", "http://collector:4317")
            .set("observe.tracing.sample_ratio", "0.1"),
        &registrations(),
        &production(),
    )
    .expect("load");
    assert!(report.profile.is_empty(), "{:?}", report.profile);
    let report = CheckReport::run(&safe(), &registrations(), &production()).expect("load");
    assert!(report.profile.is_empty(), "{:?}", report.profile);
}

#[test]
fn the_environment_can_come_from_the_sources() {
    let report = CheckReport::run(
        &good().set("app.env", "production"),
        &registrations(),
        &CheckOptions::default(),
    )
    .expect("load");
    assert!(!report.profile.is_empty());
    // `--env` replaces what the sources say.
    let report = CheckReport::run(
        &good().set("app.env", "production"),
        &registrations(),
        &CheckOptions {
            environment: Some("local".to_owned()),
        },
    )
    .expect("load");
    assert!(report.profile.is_empty());
}

#[test]
fn a_production_database_url_and_blank_settings() {
    let config = Config::load(&ConfigSources::new().set("database.url", "SQLite://x.db?mode=rwc"))
        .expect("load");
    assert!(
        production_profile(&config)
            .iter()
            .any(|f| f.key == "database.url" && f.fail)
    );
    let config = Config::load(
        &ConfigSources::new()
            .set("app.key", "  ")
            .set("mail.transport", " LOG "),
    )
    .expect("load");
    let found = production_profile(&config);
    assert!(
        found.iter().any(|f| f.key == "app.key" && f.fail),
        "blank key is not set"
    );
    assert!(found.iter().any(|f| f.key == "mail.transport" && f.fail));
}

#[test]
fn the_options_parse_env_in_both_spellings() {
    let parse = |args: &[&str]| {
        CheckOptions::parse(&args.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>())
    };
    assert_eq!(parse(&[]).unwrap(), CheckOptions::default());
    assert_eq!(parse(&["--env", "production"]).unwrap(), production());
    assert_eq!(parse(&["--env=production"]).unwrap(), production());
    assert!(parse(&["--env"]).is_err());
    assert!(parse(&["--env="]).is_err());
    assert!(parse(&["--strict"]).is_err());
    assert!(parse(&["production"]).is_err());
}

// ---------------------------------------------------------------------------
// The command, through the app
// ---------------------------------------------------------------------------

struct Sections;

impl Module for Sections {
    fn register(&self, m: &mut ModuleBuilder) {
        m.config::<Database>();
        m.config::<Queue>();
    }
}

async fn run(sources: ConfigSources, args: &[&str]) -> Result<(), navree_kernel::BoxError> {
    let mut command = vec!["config:check".to_owned()];
    command.extend(args.iter().map(|a| (*a).to_owned()));
    Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(Sections)
        .run_with_args(command)
        .await
}

#[tokio::test]
async fn the_command_exits_ok_on_a_clean_configuration() {
    run(good(), &[]).await.expect("clean");
}

#[tokio::test]
async fn the_command_fails_on_an_invalid_section_without_booting() {
    // `Queue` fails validation; the command still reports instead of
    // stopping the boot at the first bad section.
    let error = run(
        good()
            .set("queue.workers", 0)
            .set("database.max_connections", 0),
        &[],
    )
    .await
    .expect_err("two invalid sections");
    assert_eq!(error.to_string(), "config:check found 2 problem(s)");
}

#[tokio::test]
async fn the_command_fails_on_an_unknown_key() {
    let error = run(good().set("queue.workrs", 3), &[])
        .await
        .expect_err("unknown key");
    assert!(error.to_string().contains("1 problem"), "{error}");
}

#[tokio::test]
async fn the_command_applies_the_production_profile_for_env_production() {
    run(good(), &[]).await.expect("not production");
    let error = run(good(), &["--env", "production"])
        .await
        .expect_err("SQLite and no key in production");
    assert!(error.to_string().contains("problem"), "{error}");
}

#[tokio::test]
async fn the_command_rejects_unknown_arguments() {
    let error = run(good(), &["--strict"])
        .await
        .expect_err("unknown argument");
    assert!(
        error.to_string().contains("unknown argument `--strict`"),
        "{error}"
    );
}
