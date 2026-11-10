//! The Compose files Dock generates (snapshots).

use navree_dock::DockFile;
use navree_dock::compose::render;

fn compose(dock_toml: &str) -> String {
    render(&DockFile::parse(dock_toml, "crm-api").unwrap_or_else(|e| panic!("{e}")))
}

#[test]
fn every_service_with_its_defaults() {
    insta::assert_snapshot!(
        "every_service",
        compose(
            "[services.postgres]\n[services.redis]\n[services.meilisearch]\n[services.mailpit]\n[services.minio]\n[services.nats]\n"
        )
    );
}

#[test]
fn settings_change_images_ports_and_commands() {
    insta::assert_snapshot!(
        "customised",
        compose(
            r#"
project = "crm"

[services.postgres]
version = "16"
database = "crm_dev"
user = "crm"
password = "s3cret-pass"
port = 5433
extensions = ["vector"]

[services.redis]
image = "valkey/valkey:8"
maxmemory_mb = 64
port = 6380

[services.mailpit]
port = 2525
ui_port = 8026

[services.minio]
access_key = "admin"
secret_key = "admin-secret"
port = 9100
console_port = 9101

[services.nats]
jetstream = false
monitor_port = 8223
"#
        )
    );
}

#[test]
fn timescaledb_starts_preloaded_and_plain_postgres_as_its_image_does() {
    let yaml = compose("[services.postgres]\n");
    assert!(
        yaml.contains(
            "    image: \"timescale/timescaledb:2.31.0-pg17\"\n    container_name: \"crm-api-postgres\"\n"
        ),
        "{yaml}"
    );
    assert!(
        yaml.contains("      TIMESCALEDB_TELEMETRY: \"off\"\n"),
        "{yaml}"
    );
    assert!(
        yaml.contains(
            "    command: [\"postgres\", \"-c\", \"shared_preload_libraries=timescaledb\", \"-c\", \"timescaledb.telemetry_level=off\"]\n"
        ),
        "{yaml}"
    );
    let plain = compose("[services.postgres]\ntimescaledb = false\n");
    assert!(plain.contains("    image: \"postgres:17\"\n"), "{plain}");
    assert!(!plain.contains("command:"), "{plain}");
    assert!(!plain.contains("TIMESCALEDB"), "{plain}");
}

#[test]
fn postgres_18_mounts_its_new_data_directory() {
    let yaml = compose("[services.postgres]\nversion = \"18\"\n");
    assert!(
        yaml.contains("- \"crm-api-postgres-data:/var/lib/postgresql\"\n"),
        "{yaml}"
    );
    let yaml = compose("[services.postgres]\nversion = \"17\"\n");
    assert!(
        yaml.contains("- \"crm-api-postgres-data:/var/lib/postgresql/data\"\n"),
        "{yaml}"
    );
}

#[test]
fn every_port_is_bound_to_localhost_only() {
    let yaml = compose(
        "[services.postgres]\n[services.redis]\n[services.meilisearch]\n[services.mailpit]\n[services.minio]\n[services.nats]\n",
    );
    // List items are ports, except the volume mounts.
    let ports: Vec<&str> = yaml
        .lines()
        .filter(|line| line.trim_start().starts_with("- \"") && !line.contains("-data:"))
        .collect();
    assert_eq!(ports.len(), 9, "{ports:?}");
    assert!(
        ports
            .iter()
            .all(|line| line.trim_start().starts_with("- \"127.0.0.1:")),
        "{ports:?}"
    );
}
