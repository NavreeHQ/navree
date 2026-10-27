//! `DatabaseModule` wiring: configuration, health check, commands, shutdown.

use navree_config::{ConfigModule, ConfigSources};
use navree_contracts::{HealthCheck, HealthStatus};
use navree_db::{DatabaseModule, Db, DbOptions};
use navree_kernel::Navree;
use serde_json::json;

use crate::support::TempDb;

#[tokio::test]
async fn from_config_reads_the_database_section() {
    let temp = TempDb::new();
    let app = Navree::app()
        .module(ConfigModule::for_root(ConfigSources::new().set(
            "database",
            json!({ "url": temp.url, "max_connections": 3, "slow_query_ms": 50 }),
        )))
        .module(DatabaseModule::from_config())
        .boot()
        .await
        .expect("boots");
    let options = app.ctx().get::<DbOptions>().await.unwrap();
    assert_eq!(options.url, temp.url);
    assert_eq!(options.max_connections, 3);
    assert_eq!(options.slow_query_ms, 50);
}

#[tokio::test]
async fn slow_query_threshold_falls_back_to_the_log_section() {
    let temp = TempDb::new();
    let app = Navree::app()
        .module(ConfigModule::for_root(
            ConfigSources::new()
                .set("database", json!({ "url": temp.url }))
                .set("log", json!({ "slow_query_ms": 75 })),
        ))
        .module(DatabaseModule::from_config())
        .boot()
        .await
        .expect("boots");
    assert_eq!(
        app.ctx().get::<DbOptions>().await.unwrap().slow_query_ms,
        75
    );
}

#[tokio::test]
async fn plain_module_reads_environment_variables() {
    let temp = TempDb::new();
    let app = Navree::app()
        .module(ConfigModule::for_root(ConfigSources::new().env_vars(
            "APP_",
            [
                ("APP_DATABASE__URL", temp.url.as_str()),
                ("APP_DATABASE__MIGRATE_ON_BOOT", "true"),
                ("APP_DATABASE__MAX_CONNECTIONS", "4"),
            ],
        )))
        .module(DatabaseModule)
        .boot()
        .await
        .expect("boots");
    let options = app.ctx().get::<DbOptions>().await.unwrap();
    assert_eq!(options.url, temp.url);
    assert!(options.migrate_on_boot);
    assert_eq!(options.max_connections, 4);
}

#[tokio::test]
async fn mysql_settings_come_from_the_database_mysql_section() {
    let temp = TempDb::new();
    let from_file = Navree::app()
        .module(ConfigModule::for_root(ConfigSources::new().set(
            "database",
            json!({ "url": temp.url, "mysql": { "strict": false } }),
        )))
        .module(DatabaseModule::from_config())
        .boot()
        .await
        .expect("boots");
    let options = from_file.ctx().get::<DbOptions>().await.unwrap();
    assert!(!options.mysql.strict);

    let from_env = Navree::app()
        .module(ConfigModule::for_root(ConfigSources::new().env_vars(
            "APP_",
            [
                ("APP_DATABASE__URL", temp.url.as_str()),
                ("APP_DATABASE__MYSQL__STRICT", "false"),
            ],
        )))
        .module(DatabaseModule)
        .boot()
        .await
        .expect("boots");
    assert!(
        !from_env
            .ctx()
            .get::<DbOptions>()
            .await
            .unwrap()
            .mysql
            .strict
    );

    let defaults = Navree::app()
        .module(ConfigModule::for_root(
            ConfigSources::new().set("database", json!({ "url": temp.url })),
        ))
        .module(DatabaseModule::from_config())
        .boot()
        .await
        .expect("boots");
    assert!(
        defaults
            .ctx()
            .get::<DbOptions>()
            .await
            .unwrap()
            .mysql
            .strict
    );
}

#[tokio::test]
async fn invalid_database_config_fails_the_boot() {
    let error = Navree::app()
        .module(ConfigModule::for_root(
            ConfigSources::new().set("database", json!({ "max_connections": 0 })),
        ))
        .module(DatabaseModule::from_config())
        .boot()
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("database"), "{error}");
    assert!(error.contains("max_connections"), "{error}");
}

#[tokio::test]
async fn connection_errors_do_not_leak_passwords() {
    let options = DbOptions {
        url: "postgres://navree:hunter2@127.0.0.1:1/app".to_owned(),
        connect_timeout_secs: 1,
        ..DbOptions::default()
    };
    let error = Navree::app()
        .module(DatabaseModule::for_root(options))
        .boot()
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("navree_db::db::Db"), "{error}");
    assert!(error.contains("navree:***@127.0.0.1"), "{error}");
    assert!(!error.contains("hunter2"), "{error}");
}

#[tokio::test]
async fn health_check_up() {
    let temp = TempDb::new();
    let app = Navree::app()
        .module(DatabaseModule::for_root(temp.options()))
        .boot()
        .await
        .unwrap();
    let checks = app.ctx().get_all::<dyn HealthCheck>().await.unwrap();
    let database = checks
        .iter()
        .find(|c| c.name() == "database")
        .expect("database health check registered");
    assert_eq!(database.check().await, HealthStatus::Up);

    let db = app.ctx().get::<Db>().await.unwrap();
    db.conn().close_by_ref().await.unwrap();
    assert!(matches!(database.check().await, HealthStatus::Down(_)));
}

#[tokio::test]
async fn database_commands_are_registered() {
    let temp = TempDb::new();
    let app = Navree::app()
        .module(DatabaseModule::for_root(temp.options()))
        .boot()
        .await
        .unwrap();
    let names: Vec<&str> = app.ctx().commands().iter().map(|c| c.name).collect();
    for name in [
        "migrate",
        "migrate:rollback",
        "migrate:fresh",
        "migrate:status",
        "db:seed",
    ] {
        assert!(names.contains(&name), "{name} in {names:?}");
    }
}

#[tokio::test]
async fn pool_is_closed_on_shutdown() {
    let temp = TempDb::new();
    let app = Navree::app()
        .module(DatabaseModule::for_root(temp.options()))
        .boot()
        .await
        .unwrap();
    let db = app.ctx().get::<Db>().await.unwrap();
    db.conn().ping().await.expect("open while running");
    app.shutdown().await.expect("shuts down");
    assert!(db.conn().ping().await.is_err(), "closed after shutdown");
}
