//! Every name the Dock docs page and skill use exists: a rename fails here.

use std::path::Path;
use std::time::Duration;

use navree_dock::compose::{COMPOSE_FILE, PROJECT_LABEL, SERVICE_LABEL};
use navree_dock::{
    AddReport, DOCK_FILE, Dock, DockError, DockFile, FakeExecutor, POSTGRES_VERSION,
    ProcessExecutor, READY_TIMEOUT, RUNTIME_VARIABLE, Runtime, RuntimeChoice, Service,
    TIMESCALEDB_IMAGE, TIMESCALEDB_POSTGRES_VERSIONS, TIMESCALEDB_VERSION,
};

#[test]
fn files_variables_labels_and_limits() {
    assert_eq!(DOCK_FILE, "dock.toml");
    assert_eq!(COMPOSE_FILE, ".navree/dock/compose.yaml");
    assert_eq!(RUNTIME_VARIABLE, "NAVREE_DOCK_RUNTIME");
    assert_eq!(PROJECT_LABEL, "dev.navree.dock.project");
    assert_eq!(SERVICE_LABEL, "dev.navree.dock.service");
    assert_eq!(READY_TIMEOUT, Duration::from_secs(60));
    assert_eq!(Runtime::Docker.program(), "docker");
    assert_eq!(Runtime::Podman.program(), "podman");
    assert_eq!(RuntimeChoice::default(), RuntimeChoice::Auto);
}

/// "The services": TimescaleDB 2.31.0 on PostgreSQL 17 by default, and the
/// majors `version` may name while TimescaleDB is on.
#[test]
fn the_timescaledb_image_and_versions() {
    assert_eq!(TIMESCALEDB_IMAGE, "timescale/timescaledb");
    assert_eq!(TIMESCALEDB_VERSION, "2.31.0");
    assert_eq!(POSTGRES_VERSION, "17");
    assert_eq!(TIMESCALEDB_POSTGRES_VERSIONS, [16, 17, 18]);
}

#[test]
fn the_six_services_and_their_keys() {
    let names: Vec<&str> = Service::ALL.iter().map(|s| s.name()).collect();
    assert_eq!(
        names,
        [
            "postgres",
            "redis",
            "meilisearch",
            "mailpit",
            "minio",
            "nats"
        ]
    );
    assert_eq!(
        Service::Postgres.keys(),
        ["database", "user", "password", "extensions", "timescaledb"]
    );
    assert_eq!(Service::Redis.keys(), ["maxmemory_mb"]);
    assert_eq!(Service::Meilisearch.keys(), ["master_key"]);
    assert_eq!(Service::Mailpit.keys(), ["smtp_port", "ui_port"]);
    assert_eq!(
        Service::Minio.keys(),
        ["bucket", "access_key", "secret_key", "console_port"]
    );
    assert_eq!(Service::Nats.keys(), ["jetstream", "monitor_port"]);
    // `{host}`, `{port}`, `{user}`, `{password}`, `{database}`, `{bucket}`, `{key}`.
    let mut placeholders: Vec<&str> = Service::ALL
        .iter()
        .flat_map(|s| s.placeholders().iter().copied())
        .collect();
    placeholders.sort_unstable();
    placeholders.dedup();
    assert_eq!(
        placeholders,
        [
            "bucket",
            "console_port",
            "database",
            "host",
            "key",
            "monitor_port",
            "password",
            "port",
            "ui_port",
            "user"
        ]
    );
}

/// Environment variables: `(name, value)` pairs.
type Settings = Vec<(String, String)>;

#[test]
fn the_documented_api_exists() {
    let _: fn(&Path) -> Result<DockFile, DockError> = DockFile::load;
    let _: fn(&str, &str) -> Result<DockFile, DockError> = DockFile::parse;
    let _: fn(&DockFile) -> Settings = DockFile::env;
    let _: fn(&Path) -> Result<Settings, DockError> = navree_dock::app_env;
    let _: fn(&[(String, String)], bool) -> String = navree_dock::env_lines;
    let _: fn(&Path, &str) -> Result<AddReport, DockError> = navree_dock::add_service;
    let _: fn(&Path, &str) -> Result<Service, DockError> = navree_dock::remove_service;
    let _ = |root: &Path| -> Result<Dock<ProcessExecutor>, DockError> {
        Dock::open(root, ProcessExecutor::new(root))
    };
    let _ = Dock::<FakeExecutor>::up;
    let _ = Dock::<FakeExecutor>::down;
    let _ = Dock::<FakeExecutor>::status;
    let _ = Dock::<FakeExecutor>::logs;
    let _ = Dock::<FakeExecutor>::shell;
    let _ = Dock::<FakeExecutor>::remove_container;
    let _ = navree_dock::status_table;
    let _ = navree_dock::write_dotenv;
    let _ = navree_dock::compose::render;
}
