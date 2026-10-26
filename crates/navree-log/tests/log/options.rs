//! `LogOptions` as the `log` configuration section.

use std::collections::BTreeMap;
use std::path::PathBuf;

use navree_config::{Config, ConfigError, ConfigSources};
use navree_log::{FileLog, LogFormat, LogOptions, Rotation};
use serde_json::json;

fn load(sources: ConfigSources) -> Result<LogOptions, ConfigError> {
    Config::load(&sources)
        .expect("load config")
        .typed::<LogOptions>()
}

#[test]
fn defaults_match_documentation() {
    let options = LogOptions::default();
    let expected_format = if cfg!(debug_assertions) {
        LogFormat::Pretty
    } else {
        LogFormat::Json
    };
    assert_eq!(options.format, expected_format);
    assert_eq!(options.level, "info");
    assert!(options.levels.is_empty());
    assert_eq!(options.file, None);
    assert_eq!(
        options.redact,
        ["password", "token", "authorization", "cookie", "secret"]
    );
    assert_eq!(options.slow_query_ms, 200);
    assert_eq!(
        FileLog::default(),
        FileLog {
            path: PathBuf::from("storage/logs/navree.log"),
            rotation: Rotation::Daily,
            keep_days: 14,
        }
    );
}

#[test]
fn missing_section_uses_defaults() {
    assert_eq!(
        load(ConfigSources::new()).expect("options"),
        LogOptions::default()
    );
}

#[test]
fn reads_log_section_from_config() {
    let options = load(ConfigSources::new().set(
        "log",
        json!({
            "format": "json",
            "level": "debug",
            "levels": { "navree_queue": "debug", "sqlx": "warn" },
            "file": { "path": "storage/logs/app.log", "rotation": "hourly", "keep_days": 3 },
            "redact": ["password", "ssn"],
            "slow_query_ms": 500,
        }),
    ))
    .expect("options");
    assert_eq!(
        options,
        LogOptions {
            format: LogFormat::Json,
            level: "debug".to_owned(),
            levels: BTreeMap::from([
                ("navree_queue".to_owned(), "debug".to_owned()),
                ("sqlx".to_owned(), "warn".to_owned()),
            ]),
            file: Some(FileLog {
                path: PathBuf::from("storage/logs/app.log"),
                rotation: Rotation::Hourly,
                keep_days: 3,
            }),
            redact: vec!["password".to_owned(), "ssn".to_owned()],
            slow_query_ms: 500,
        }
    );
}

#[test]
fn partial_file_section_uses_file_defaults() {
    let options = load(ConfigSources::new().set("log", json!({ "file": { "rotation": "never" } })))
        .expect("options");
    assert_eq!(
        options.file,
        Some(FileLog {
            rotation: Rotation::Never,
            ..FileLog::default()
        })
    );
}

#[test]
fn environment_strings_are_accepted() {
    let options = load(ConfigSources::new().env_vars(
        "APP_",
        [
            ("APP_LOG__FORMAT", "JSON"),
            ("APP_LOG__LEVEL", "Warn"),
            ("APP_LOG__LEVELS", "navree::request=debug, sqlx=error"),
            ("APP_LOG__REDACT", "password, ssn,,"),
            ("APP_LOG__SLOW_QUERY_MS", "50"),
            ("APP_LOG__FILE__PATH", "logs/app.log"),
            ("APP_LOG__FILE__ROTATION", "Daily"),
            ("APP_LOG__FILE__KEEP_DAYS", "7"),
        ],
    ))
    .expect("options");
    assert_eq!(options.format, LogFormat::Json);
    assert_eq!(options.level, "Warn");
    assert_eq!(
        options.levels,
        BTreeMap::from([
            ("navree::request".to_owned(), "debug".to_owned()),
            ("sqlx".to_owned(), "error".to_owned()),
        ])
    );
    assert_eq!(options.redact, ["password", "ssn"]);
    assert_eq!(options.slow_query_ms, 50);
    assert_eq!(
        options.file,
        Some(FileLog {
            path: PathBuf::from("logs/app.log"),
            rotation: Rotation::Daily,
            keep_days: 7,
        })
    );
}

#[test]
fn nested_level_variables_set_one_target() {
    let options =
        load(ConfigSources::new().env_vars("APP_", [("APP_LOG__LEVELS__NAVREE_QUEUE", "debug")]))
            .expect("options");
    assert_eq!(
        options.levels,
        BTreeMap::from([("navree_queue".to_owned(), "debug".to_owned())])
    );
}

#[test]
fn invalid_level_fails_validation() {
    let error = load(ConfigSources::new().set("log", json!({ "level": "loud" })))
        .expect_err("invalid level");
    let message = error.to_string();
    assert!(matches!(error, ConfigError::Validation { .. }), "{message}");
    assert!(
        message.contains("`log`") && message.contains("loud"),
        "{message}"
    );
}

#[test]
fn invalid_target_level_fails_validation() {
    let error = load(ConfigSources::new().set("log", json!({ "levels": { "sqlx": "chatty" } })))
        .expect_err("invalid level");
    assert!(error.to_string().contains("chatty"), "{error}");
}

#[test]
fn malformed_level_string_is_rejected() {
    let error = load(ConfigSources::new().env_vars("APP_", [("APP_LOG__LEVELS", "sqlx")]))
        .expect_err("missing `=level`");
    assert!(matches!(error, ConfigError::Section { .. }), "{error}");
}

#[test]
fn unknown_format_is_rejected() {
    let error = load(ConfigSources::new().set("log", json!({ "format": "xml" })))
        .expect_err("unknown format");
    assert!(error.to_string().contains("xml"), "{error}");
}

#[test]
fn file_settings_are_validated() {
    let error = load(ConfigSources::new().set("log", json!({ "file": { "keep_days": 0 } })))
        .expect_err("keep_days 0");
    assert!(error.to_string().contains("keep_days"), "{error}");
    let error = load(ConfigSources::new().set("log", json!({ "file": { "path": "" } })))
        .expect_err("empty path");
    assert!(error.to_string().contains("path"), "{error}");
}
