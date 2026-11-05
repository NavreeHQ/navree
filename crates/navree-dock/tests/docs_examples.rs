//! The examples of the Dock docs page (`apps/website/content/docs/tools/dock.mdx`)
//! hold: each `dock.toml` parses and gives the settings the page shows, and
//! the printed output has the page's shape.

use navree_dock::{
    BLOCK_END, BLOCK_START, CmdOutput, Dock, DockFile, FakeExecutor, Service, ServiceOptions,
    env_lines, status_table, update_dotenv,
};

fn parse(text: &str) -> DockFile {
    DockFile::parse(text, "crm-api").unwrap_or_else(|e| panic!("{e}"))
}

fn setting(file: &DockFile, name: &str) -> String {
    file.env()
        .into_iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value)
        .unwrap_or_else(|| panic!("no setting {name}"))
}

/// "dock.toml": the page's full example.
const FULL_EXAMPLE: &str = r#"# Local services for crm-api. `navree dock up` starts them.

[services.postgres]
version = "17"
database = "crm_api"

[services.redis]

[services.mailpit]

[services.meilisearch]

[services.minio]
bucket = "crm-uploads"

[services.nats]
jetstream = true
"#;

#[test]
fn the_full_dock_toml_example() {
    let file = parse(FULL_EXAMPLE);
    let names: Vec<&str> = file.services.iter().map(|s| s.name()).collect();
    assert_eq!(
        names,
        [
            "postgres",
            "redis",
            "mailpit",
            "meilisearch",
            "minio",
            "nats"
        ]
    );
    assert_eq!(file.project, "crm-api");
    assert_eq!(file.services[0].container, "crm-api-postgres");
    assert_eq!(file.services[0].volume, "crm-api-postgres-data");
}

/// "The services": the settings each service gives the app.
#[test]
fn the_services_table() {
    let file = parse(FULL_EXAMPLE);
    assert_eq!(
        setting(&file, "APP_DATABASE__URL"),
        "postgres://navree:navree@127.0.0.1:5432/crm_api"
    );
    assert_eq!(setting(&file, "REDIS_URL"), "redis://127.0.0.1:6379");
    assert_eq!(setting(&file, "APP_SEARCH__ENGINE"), "meilisearch");
    assert_eq!(setting(&file, "APP_SEARCH__URL"), "http://127.0.0.1:7700");
    assert_eq!(
        setting(&file, "APP_SEARCH__KEY"),
        "dock-meilisearch-master-key"
    );
    assert_eq!(
        setting(&file, "APP_MAIL__TRANSPORT"),
        "smtp://127.0.0.1:1025"
    );
    assert_eq!(setting(&file, "AWS_ENDPOINT_URL"), "http://127.0.0.1:9000");
    assert_eq!(setting(&file, "AWS_ACCESS_KEY_ID"), "navree");
    assert_eq!(setting(&file, "AWS_SECRET_ACCESS_KEY"), "navree-secret");
    assert_eq!(setting(&file, "AWS_REGION"), "us-east-1");
    // The broker's address only: the transport kind is the app's shape and
    // stays in config/app.toml.
    assert!(
        file.env()
            .iter()
            .all(|(key, _)| key != "APP_MICRO__TRANSPORT"),
        "Dock sets APP_MICRO__TRANSPORT"
    );
    assert_eq!(setting(&file, "APP_MICRO__URL"), "nats://127.0.0.1:4222");

    let ports: Vec<Vec<u16>> = file
        .services
        .iter()
        .map(|s| s.ports.iter().map(|p| p.host).collect())
        .collect();
    assert_eq!(
        ports,
        [
            vec![5432],
            vec![6379],
            vec![1025, 8025],
            vec![7700],
            vec![9000, 9001],
            vec![4222, 8222]
        ]
    );
    let images: Vec<&str> = file.services.iter().map(|s| s.image.as_str()).collect();
    assert_eq!(
        images,
        [
            "timescale/timescaledb:2.31.0-pg17",
            "redis:7",
            "axllent/mailpit:v1.31",
            "getmeili/meilisearch:v1.55",
            "pgsty/minio:RELEASE.2026-08-04T00-00-00Z",
            "nats:2.14"
        ]
    );
    // The TimescaleDB image has pgvector too; plain PostgreSQL switches to
    // pgvector's image.
    let vector = parse("[services.postgres]\nextensions = [\"vector\"]\n");
    assert_eq!(
        vector.services[0].image,
        "timescale/timescaledb:2.31.0-pg17"
    );
    let plain = parse("[services.postgres]\ntimescaledb = false\n");
    assert_eq!(plain.services[0].image, "postgres:17");
    let plain_vector =
        parse("[services.postgres]\ntimescaledb = false\nextensions = [\"vector\"]\n");
    assert_eq!(plain_vector.services[0].image, "pgvector/pgvector:pg17");
}

/// "PostgreSQL and TimescaleDB": the page's plain PostgreSQL and own-image
/// examples.
#[test]
fn the_postgres_examples() {
    let plain = parse("[services.postgres]\ntimescaledb = false\nversion = \"16\"\n");
    assert_eq!(plain.services[0].image, "postgres:16");
    let pg18 = parse("[services.postgres]\nversion = \"18\"\n");
    assert_eq!(pg18.services[0].image, "timescale/timescaledb:2.31.0-pg18");
    let postgis = parse("[services.postgres]\nimage = \"postgis/postgis:17-3.5\"\n");
    assert!(matches!(
        postgis.services[0].options,
        ServiceOptions::Postgres {
            timescaledb: false,
            ..
        }
    ));
    assert!(postgis.services[0].setup().is_empty());
}

/// "Service settings": every documented default.
#[test]
fn the_service_settings_defaults() {
    let file = parse(
        "[services.postgres]\n[services.redis]\n[services.meilisearch]\n[services.mailpit]\n[services.minio]\n[services.nats]\n",
    );
    assert_eq!(
        file.services[0].options,
        ServiceOptions::Postgres {
            database: "crm_api".into(),
            user: "navree".into(),
            password: "navree".into(),
            extensions: vec![],
            timescaledb: true,
        }
    );
    assert_eq!(
        file.services[1].options,
        ServiceOptions::Redis { maxmemory_mb: 256 }
    );
    assert_eq!(
        file.services[2].options,
        ServiceOptions::Meilisearch {
            master_key: "dock-meilisearch-master-key".into()
        }
    );
    assert_eq!(
        file.services[3].addresses(),
        ["127.0.0.1:1025", "127.0.0.1:8025"]
    );
    assert_eq!(
        file.services[4].options,
        ServiceOptions::Minio {
            bucket: "crm-api".into(),
            access_key: "navree".into(),
            secret_key: "navree-secret".into(),
        }
    );
    assert_eq!(file.services[4].port_of("console_port"), Some(9001));
    assert_eq!(
        file.services[5].options,
        ServiceOptions::Nats { jetstream: true }
    );
    assert_eq!(file.services[5].port_of("monitor_port"), Some(8222));
    for service in Service::ALL {
        let keys = service.keys();
        assert!(
            !keys.contains(&"image") && !keys.contains(&"env"),
            "{service}"
        );
    }
}

/// "Changing what your app gets".
#[test]
fn the_env_example() {
    let file = parse(
        r#"
[services.postgres]
database = "crm_api"
env = { APP_DATABASE__URL = "postgres://{user}:{password}@{host}:{port}/{database}?application_name=crm-dev" }

[services.minio]
bucket = "crm-uploads"
env = { CRM_UPLOADS_ENDPOINT = "http://{host}:{port}", CRM_UPLOADS_BUCKET = "{bucket}" }
"#,
    );
    assert_eq!(
        file.env(),
        [
            (
                "APP_DATABASE__URL".to_owned(),
                "postgres://navree:navree@127.0.0.1:5432/crm_api?application_name=crm-dev"
                    .to_owned()
            ),
            (
                "CRM_UPLOADS_ENDPOINT".to_owned(),
                "http://127.0.0.1:9000".to_owned()
            ),
            ("CRM_UPLOADS_BUCKET".to_owned(), "crm-uploads".to_owned()),
        ]
    );
}

/// "Several apps on one machine": other ports, and the settings follow.
#[test]
fn the_second_app_example() {
    let file = parse("[services.postgres]\nport = 5433\n\n[services.redis]\nport = 6380\n");
    assert_eq!(
        setting(&file, "APP_DATABASE__URL"),
        "postgres://navree:navree@127.0.0.1:5433/crm_api"
    );
    assert_eq!(setting(&file, "REDIS_URL"), "redis://127.0.0.1:6380");
}

/// "How your app finds the services": the `.env` block.
#[test]
fn the_dotenv_example() {
    let file = parse("[services.postgres]\n[services.mailpit]\n");
    let existing = "APP_KEY=base64:G60q3lWREHaFECJUwNhN58ArvmrSl0sptSaOEWgeBLk=\n";
    let update = update_dotenv(existing, &file.env());
    assert_eq!(
        update.text,
        format!(
            "APP_KEY=base64:G60q3lWREHaFECJUwNhN58ArvmrSl0sptSaOEWgeBLk=\n\n{BLOCK_START}\nAPP_DATABASE__URL=postgres://navree:navree@127.0.0.1:5432/crm_api\nAPP_MAIL__TRANSPORT=smtp://127.0.0.1:1025\n{BLOCK_END}\n"
        )
    );
    assert_eq!(
        BLOCK_START,
        "# dock:start (written by navree dock up; edit dock.toml instead)"
    );
    assert_eq!(BLOCK_END, "# dock:end");
}

/// `navree dock env` and `navree dock env --export`.
#[test]
fn the_env_command() {
    let file = parse("[services.postgres]\n[services.nats]\n");
    assert_eq!(
        env_lines(&file.env(), false),
        "APP_DATABASE__URL=postgres://navree:navree@127.0.0.1:5432/crm_api\nAPP_MICRO__URL=nats://127.0.0.1:4222\n"
    );
    assert!(
        env_lines(&file.env(), true).starts_with(
            "export APP_DATABASE__URL=postgres://navree:navree@127.0.0.1:5432/crm_api\n"
        )
    );
}

/// "Getting started": `navree dock up` after adding postgres and mailpit.
#[test]
fn the_getting_started_output() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let file = parse("[services.postgres]\n[services.mailpit]\n");
    let dock = Dock::new(dir.path(), file, FakeExecutor::new()).unwrap();
    let mut out = Vec::new();
    dock.up(&[], &mut out).unwrap();
    let printed = String::from_utf8(out).unwrap();
    let lines: Vec<&str> = printed.lines().collect();
    assert_eq!(lines[0], "Starting crm-api services with docker");
    assert!(
        lines[1].starts_with(
            "  postgres  timescale/timescaledb:2.31.0-pg17  127.0.0.1:5432  ready in "
        ),
        "{printed}"
    );
    assert!(
        lines[2].starts_with(
            "  mailpit   axllent/mailpit:v1.31              127.0.0.1:1025  ready in "
        ),
        "{printed}"
    );
    assert_eq!(lines[3], "Mailpit's inbox: http://127.0.0.1:8025");
    assert_eq!(
        lines[4],
        "Wrote 2 settings to .env: APP_DATABASE__URL, APP_MAIL__TRANSPORT."
    );
}

/// "Commands": the `navree dock status` table.
#[test]
fn the_status_output() {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let file = parse(
        "[services.postgres]\n[services.redis]\n[services.mailpit]\n[services.meilisearch]\n",
    );
    let ps = "crm-api-postgres|running|timescale/timescaledb:2.31.0-pg17|127.0.0.1:5432->5432/tcp\n\
              crm-api-redis|running|redis:7|127.0.0.1:6379->6379/tcp\n\
              crm-api-mailpit|running|axllent/mailpit:v1.31|127.0.0.1:1025->1025/tcp, 127.0.0.1:8025->8025/tcp\n";
    let exec = FakeExecutor::new().respond("ps --all", CmdOutput::ok(ps));
    let dock = Dock::new(dir.path(), file, exec).unwrap();
    assert_eq!(
        status_table(&dock.status().unwrap()),
        "Service      State    Image                              Address\n\
         postgres     running  timescale/timescaledb:2.31.0-pg17  127.0.0.1:5432\n\
         redis        running  redis:7                            127.0.0.1:6379\n\
         mailpit      running  axllent/mailpit:v1.31              127.0.0.1:1025, 127.0.0.1:8025\n\
         meilisearch  stopped\n"
    );
}

/// "The library" (docs) and "Using the library" (skill `navree-dock`).
#[test]
fn the_library_examples() -> Result<(), navree_dock::DockError> {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    std::fs::write(dir.path().join("dock.toml"), "[services.redis]\n").unwrap();
    let file = DockFile::load(dir.path())?;
    let lines: Vec<String> = file
        .env()
        .into_iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect();
    assert_eq!(lines, ["REDIS_URL=redis://127.0.0.1:6379"]);

    let file = DockFile::parse("[services.postgres]\n", "crm-api")?;
    let exec = FakeExecutor::new().respond_once("pg_isready", CmdOutput::failed(2, "no response"));
    let dock = Dock::new(dir.path(), file, exec)?;
    dock.up(&[], &mut Vec::new())?;
    assert!(
        dock.executor()
            .commands()
            .iter()
            .any(|c| c.contains("compose"))
    );
    Ok(())
}

/// "Docker or Podman": the label on every container.
#[test]
fn the_project_label() {
    assert_eq!(
        navree_dock::compose::PROJECT_LABEL,
        "dev.navree.dock.project"
    );
    let yaml = navree_dock::compose::render(&parse("[services.redis]\n"));
    assert!(
        yaml.contains("dev.navree.dock.project: \"crm-api\""),
        "{yaml}"
    );
}
