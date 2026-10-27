//! `up`, `down`, `status`, `logs` and `shell` against the fake executor:
//! the exact runtime commands Dock runs (snapshots) and what it prints.

use std::path::Path;
use std::time::Duration;

use navree_dock::{
    CmdOutput, Dock, DockError, DockFile, FakeExecutor, READY_TIMEOUT, status_table,
};

/// An app directory with `Cargo.toml` (package `crm-api`) and `dock.toml`.
fn app(dock_toml: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"crm-api\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("dock.toml"), dock_toml).unwrap();
    dir
}

fn dock(dir: &Path, exec: FakeExecutor) -> Dock<FakeExecutor> {
    Dock::open(dir, exec).unwrap_or_else(|e| panic!("{e}"))
}

/// The commands run, then what was printed.
fn transcript(dock: &Dock<FakeExecutor>, printed: &[u8]) -> String {
    format!(
        "$ {}\n--- printed ---\n{}",
        dock.executor().commands().join("\n$ "),
        String::from_utf8_lossy(printed)
    )
}

const PS_ALL: &str = "ps --all --filter";

#[test]
fn up_starts_postgres_and_mailpit_and_writes_env() {
    let dir = app("[services.postgres]\n\n[services.mailpit]\n");
    std::fs::write(dir.path().join(".env"), "APP_KEY=base64:abc=\n").unwrap();
    let exec = FakeExecutor::new()
        // The TimescaleDB image is not on the machine yet.
        .respond_once(
            "'{{.Id}}' timescale/timescaledb:2.31.0-pg17",
            CmdOutput::failed(1, "No such image"),
        )
        // PostgreSQL needs a few checks; Mailpit's inbox answers on the second.
        .respond_once("pg_isready", CmdOutput::failed(2, "no response"))
        .respond_once("pg_isready", CmdOutput::failed(2, "no response"))
        .http_ready_after(8025, 1);
    let dock = dock(dir.path(), exec);
    let mut out = Vec::new();
    dock.up(&[], &mut out).unwrap();
    insta::assert_snapshot!("up_postgres_mailpit", transcript(&dock, &out));
    assert_eq!(
        std::fs::read_to_string(dir.path().join(".env")).unwrap(),
        "APP_KEY=base64:abc=\n\n# dock:start (written by navree dock up; edit dock.toml instead)\nAPP_DATABASE__URL=postgres://navree:navree@127.0.0.1:5432/crm_api\nAPP_MAIL__TRANSPORT=smtp://127.0.0.1:1025\n# dock:end\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join(".navree/dock/compose.yaml")).unwrap(),
        navree_dock::compose::render(dock.file())
    );
}

#[test]
fn running_up_again_leaves_running_services_alone() {
    let dir = app("[services.postgres]\n[services.mailpit]\n");
    let ps = "crm-api-postgres|running|timescale/timescaledb:2.31.0-pg17|127.0.0.1:5432->5432/tcp\n\
              crm-api-mailpit|running|axllent/mailpit:v1.31|127.0.0.1:1025->1025/tcp, 127.0.0.1:8025->8025/tcp\n";
    // The ports answer: our own containers hold them.
    let exec = FakeExecutor::new()
        .respond(PS_ALL, CmdOutput::ok(ps))
        .take_port(5432)
        .take_port(1025)
        .take_port(8025);
    let dock = dock(dir.path(), exec);
    let mut out = Vec::new();
    dock.up(&[], &mut out).unwrap();
    let printed = String::from_utf8(out).unwrap();
    assert!(
        printed.contains(
            "  postgres  timescale/timescaledb:2.31.0-pg17  127.0.0.1:5432  already running\n"
        ),
        "{printed}"
    );
    assert!(printed.contains("Wrote 2 settings to .env"), "{printed}");
    let mut again = Vec::new();
    dock.up(&[], &mut again).unwrap();
    assert!(
        String::from_utf8(again)
            .unwrap()
            .ends_with(".env already has the 2 settings.\n")
    );
    assert!(
        !dock
            .executor()
            .commands()
            .iter()
            .any(|c| c.contains(" pull ")),
        "no pulls: the images are there"
    );
}

#[test]
fn a_changed_port_counts_as_a_new_start() {
    let dir = app("[services.postgres]\nport = 5433\n");
    let ps = "crm-api-postgres|running|timescale/timescaledb:2.31.0-pg17|127.0.0.1:5432->5432/tcp\n";
    let dock = dock(
        dir.path(),
        FakeExecutor::new().respond(PS_ALL, CmdOutput::ok(ps)),
    );
    let mut out = Vec::new();
    dock.up(&[], &mut out).unwrap();
    let printed = String::from_utf8(out).unwrap();
    assert!(
        printed.contains("127.0.0.1:5433  ready in 0.0 s"),
        "{printed}"
    );
}

#[test]
fn up_with_names_starts_only_those_but_writes_every_setting() {
    let dir = app("[services.postgres]\n[services.redis]\n[services.nats]\n");
    let dock = dock(dir.path(), FakeExecutor::new());
    let mut out = Vec::new();
    dock.up(&["nats".into(), "postgres".into()], &mut out)
        .unwrap();
    let commands = dock.executor().commands();
    let compose_up = commands
        .iter()
        .find(|c| c.contains("compose") && c.contains(" up "))
        .unwrap();
    assert!(
        compose_up.ends_with("up --detach postgres nats"),
        "{compose_up}"
    );
    assert!(!commands.iter().any(|c| c.contains("redis-cli")));
    let env = std::fs::read_to_string(dir.path().join(".env")).unwrap();
    assert!(env.contains("REDIS_URL=redis://127.0.0.1:6379"), "{env}");
    let error = dock.up(&["mailpit".into()], &mut Vec::new()).unwrap_err();
    assert_eq!(
        error.to_string(),
        "mailpit is not in dock.toml; add it with `navree dock:add mailpit`"
    );
}

#[test]
fn a_taken_port_stops_before_anything_starts() {
    let dir = app("[services.postgres]\n[services.mailpit]\n");
    let exec = FakeExecutor::new().take_port(5432).take_port(8025).respond(
        "ps --format",
        CmdOutput::ok("shop-postgres|0.0.0.0:5432->5432/tcp\nother|\n"),
    );
    let dock = dock(dir.path(), exec);
    let error = dock.up(&[], &mut Vec::new()).unwrap_err();
    assert!(
        matches!(error, DockError::PortsTaken(ref c) if c.len() == 2),
        "{error}"
    );
    assert_eq!(
        error.to_string(),
        "port 5432 (postgres) is taken by container shop-postgres; set `port` in [services.postgres] of dock.toml\n\
         port 8025 (mailpit) is taken by another program; set `ui_port` in [services.mailpit] of dock.toml\n\
         Nothing was started. Another app's services may be running: give this app other ports in dock.toml."
    );
    let commands = dock.executor().commands();
    assert!(
        !commands
            .iter()
            .any(|c| c.contains("compose") || c.contains("pull")),
        "{commands:?}"
    );
}

#[test]
fn a_service_that_never_gets_ready_fails_with_its_logs() {
    let dir = app("[services.postgres]\n");
    let exec = FakeExecutor::new()
        .respond("pg_isready", CmdOutput::failed(2, "no response"))
        .respond(
            "logs --tail 20",
            CmdOutput::ok(
                "FATAL:  data directory \"/var/lib/postgresql/data\" has invalid permissions\n",
            ),
        );
    let dock = dock(dir.path(), exec);
    let error = dock.up(&[], &mut Vec::new()).unwrap_err();
    assert_eq!(
        error.to_string(),
        "postgres was not ready after 60 s. Its last log lines:\nFATAL:  data directory \"/var/lib/postgresql/data\" has invalid permissions"
    );
    assert_eq!(dock.executor().elapsed(), READY_TIMEOUT);
    assert!(
        !dir.path().join(".env").exists(),
        ".env is only written once everything is ready"
    );
}

#[test]
fn a_runtime_that_does_not_answer_is_reported() {
    let dir = app("[services.redis]\n");
    let dock = dock(dir.path(), FakeExecutor::new().hang(PS_ALL));
    let error = dock.up(&[], &mut Vec::new()).unwrap_err();
    assert!(
        matches!(error, DockError::NotResponding { seconds: 30, .. }),
        "{error}"
    );
    assert!(
        error
            .to_string()
            .starts_with("docker did not answer within 30 s; is it running?"),
        "{error}"
    );
    assert_eq!(dock.executor().elapsed(), Duration::from_secs(30));
}

#[test]
fn a_missing_compose_is_explained() {
    let dir = app("[services.redis]\n");
    let docker = dock(
        dir.path(),
        FakeExecutor::new().respond(
            " compose ",
            CmdOutput::failed(
                1,
                "docker: 'compose' is not a docker command.\nSee 'docker --help'",
            ),
        ),
    );
    let error = docker.up(&[], &mut Vec::new()).unwrap_err();
    assert!(
        error.to_string().contains("install Docker Compose v2"),
        "{error}"
    );
    let podman = dock(
        dir.path(),
        FakeExecutor::new().with_programs(["podman"]).respond(
            " compose ",
            CmdOutput::failed(125, "Error: looking up compose provider failed"),
        ),
    );
    let error = podman.up(&[], &mut Vec::new()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("install podman-compose or docker-compose"),
        "{error}"
    );
}

#[test]
fn a_failed_pull_stops_up() {
    let dir = app("[services.redis]\n");
    let exec = FakeExecutor::new()
        .respond("image inspect", CmdOutput::failed(1, "No such image"))
        .respond("pull redis:7", CmdOutput::failed(1, ""));
    let dock = dock(dir.path(), exec);
    let error = dock.up(&[], &mut Vec::new()).unwrap_err();
    assert_eq!(
        error.to_string(),
        "`docker pull redis:7` failed (exit code 1)"
    );
}

#[test]
fn podman_runs_the_same_commands() {
    let dir = app("[services.redis]\n");
    let exec = FakeExecutor::new()
        .with_programs(["docker", "podman"])
        .with_var("NAVREE_DOCK_RUNTIME", "podman");
    let dock = dock(dir.path(), exec);
    let mut out = Vec::new();
    dock.up(&[], &mut out).unwrap();
    insta::assert_snapshot!("up_redis_podman", transcript(&dock, &out));
}

#[test]
fn extensions_and_the_bucket_are_created_once_ready() {
    let dir = app(
        "[services.postgres]\nextensions = [\"vector\", \"pg_trgm\"]\n\n[services.minio]\nbucket = \"crm-uploads\"\n",
    );
    let exec = FakeExecutor::new().http_ready_after(9000, 3);
    let dock = dock(dir.path(), exec);
    let mut out = Vec::new();
    dock.up(&[], &mut out).unwrap();
    insta::assert_snapshot!("up_vector_minio", transcript(&dock, &out));
}

#[test]
fn timescaledb_is_created_in_the_database_once_ready() {
    let dir = app("[services.postgres]\n");
    let dock = dock(dir.path(), FakeExecutor::new());
    dock.up(&[], &mut Vec::new()).unwrap();
    let commands = dock.executor().commands();
    let ready = commands
        .iter()
        .position(|c| c.contains("pg_isready"))
        .unwrap();
    let created = commands
        .iter()
        .position(|c| {
            c == "docker exec crm-api-postgres psql -v ON_ERROR_STOP=1 -U navree -d crm_api -c 'CREATE EXTENSION IF NOT EXISTS \"timescaledb\"'"
        })
        .unwrap_or_else(|| panic!("{commands:#?}"));
    assert!(ready < created, "{commands:#?}");
    let compose = std::fs::read_to_string(dir.path().join(".navree/dock/compose.yaml")).unwrap();
    assert!(
        compose.contains("    image: \"timescale/timescaledb:2.31.0-pg17\"\n"),
        "{compose}"
    );
}

#[test]
fn plain_postgres_runs_without_timescaledb() {
    let dir = app("[services.postgres]\ntimescaledb = false\n");
    let dock = dock(dir.path(), FakeExecutor::new());
    let mut out = Vec::new();
    dock.up(&[], &mut out).unwrap();
    let printed = String::from_utf8(out).unwrap();
    assert!(
        printed.contains("  postgres  postgres:17  127.0.0.1:5432  ready in "),
        "{printed}"
    );
    assert!(
        !dock
            .executor()
            .commands()
            .iter()
            .any(|c| c.contains("CREATE EXTENSION")),
        "nothing to create"
    );
}

#[test]
fn a_failed_setup_step_is_an_error() {
    let dir = app("[services.postgres]\nextensions = [\"postgis\"]\n");
    let exec = FakeExecutor::new().respond(
        "EXISTS \"postgis\"",
        CmdOutput::failed(1, "ERROR:  extension \"postgis\" is not available"),
    );
    let dock = dock(dir.path(), exec);
    let error = dock.up(&[], &mut Vec::new()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("extension \"postgis\" is not available"),
        "{error}"
    );
}

#[test]
fn write_env_false_leaves_dotenv_alone() {
    let dir = app("write_env = false\n[services.redis]\n");
    let dock = dock(dir.path(), FakeExecutor::new());
    let mut out = Vec::new();
    dock.up(&[], &mut out).unwrap();
    assert!(!dir.path().join(".env").exists());
    assert!(!String::from_utf8(out).unwrap().contains(".env"));
}

#[test]
fn an_empty_dock_toml_has_nothing_to_start() {
    let dir = app("# nothing yet\n");
    let dock = dock(dir.path(), FakeExecutor::new());
    let error = dock.up(&[], &mut Vec::new()).unwrap_err();
    assert_eq!(
        error.to_string(),
        "dock.toml: no services yet; add one with `navree dock:add <service>`"
    );
}

#[test]
fn down_stops_everything_or_the_services_named() {
    let dir = app("[services.postgres]\n[services.redis]\n");
    let ps = "crm-api-postgres|running|postgres:17|127.0.0.1:5432->5432/tcp\n";
    let exec = FakeExecutor::new()
        .respond(PS_ALL, CmdOutput::ok(ps))
        .respond(
            "volume rm --force crm-api-redis-data",
            CmdOutput::failed(1, "Error: no such volume: crm-api-redis-data"),
        );
    let dock = dock(dir.path(), exec);
    let mut out = Vec::new();
    dock.down(&[], false, &mut out).unwrap();
    dock.down(&[], true, &mut out).unwrap();
    dock.down(&["postgres".into()], false, &mut out).unwrap();
    dock.down(&["postgres".into(), "redis".into()], true, &mut out)
        .unwrap();
    insta::assert_snapshot!("down", transcript(&dock, &out));
}

#[test]
fn status_lists_every_service() {
    let dir =
        app("[services.postgres]\n[services.redis]\n[services.mailpit]\n[services.meilisearch]\n");
    let ps = "crm-api-postgres|running|timescale/timescaledb:2.31.0-pg17|127.0.0.1:5432->5432/tcp\n\
              crm-api-redis|running|redis:7|127.0.0.1:6379->6379/tcp\n\
              crm-api-mailpit|running|axllent/mailpit:v1.31|127.0.0.1:1025->1025/tcp, 127.0.0.1:8025->8025/tcp\n\
              crm-api-meilisearch|exited|getmeili/meilisearch:v1.55|\n";
    let dock = dock(
        dir.path(),
        FakeExecutor::new().respond(PS_ALL, CmdOutput::ok(ps)),
    );
    let table = status_table(&dock.status().unwrap());
    insta::assert_snapshot!("status", table);
    assert_eq!(
        dock.executor().commands(),
        [
            "docker ps --all --filter label=dev.navree.dock.project=crm-api --format '{{.Names}}|{{.State}}|{{.Image}}|{{.Ports}}'"
        ]
    );
}

#[test]
fn logs_and_shells_run_attached() {
    let dir = app(
        "[services.postgres]\ndatabase = \"crm\"\n[services.redis]\n[services.meilisearch]\n[services.mailpit]\n[services.minio]\n[services.nats]\n",
    );
    let exec = FakeExecutor::new().respond("logs crm-api-mailpit", CmdOutput::failed(1, ""));
    let dock = dock(dir.path(), exec);
    assert_eq!(dock.logs("postgres", false).unwrap(), 0);
    assert_eq!(dock.logs("redis", true).unwrap(), 0);
    assert_eq!(dock.logs("mailpit", false).unwrap(), 1);
    for service in [
        "postgres",
        "redis",
        "meilisearch",
        "mailpit",
        "minio",
        "nats",
    ] {
        assert_eq!(dock.shell(service).unwrap(), 0);
    }
    assert_eq!(
        dock.executor().commands(),
        [
            "[attached] docker logs crm-api-postgres",
            "[attached] docker logs --follow crm-api-redis",
            "[attached] docker logs crm-api-mailpit",
            "[attached] docker exec -it crm-api-postgres psql -U navree -d crm",
            "[attached] docker exec -it crm-api-redis redis-cli",
            "[attached] docker exec -it crm-api-meilisearch sh",
            "[attached] docker exec -it crm-api-mailpit sh",
            "[attached] docker exec -it -e MC_HOST_dock=http://navree:navree-secret@127.0.0.1:9000 crm-api-minio sh",
            "[attached] docker run --rm -it --network crm-api_default -e NATS_URL=nats://nats:4222 natsio/nats-box:0.20.1 sh",
        ]
    );
    assert!(matches!(
        dock.logs("mysql", false),
        Err(DockError::UnknownService(_))
    ));
}

#[test]
fn opening_without_a_runtime_fails() {
    let dir = app("[services.redis]\n");
    let error = Dock::open(
        dir.path(),
        FakeExecutor::new().with_programs::<[&str; 0]>([]),
    )
    .unwrap_err();
    assert!(matches!(error, DockError::NoRuntime), "{error}");
    let file = DockFile::parse("runtime = \"podman\"\n", "x").unwrap();
    let error = Dock::new(dir.path(), file, FakeExecutor::new()).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Dock is set to use podman, but `podman` is not on PATH"
    );
}

#[test]
fn remove_container_stops_one_service() {
    let dir = app("[services.redis]\n");
    let ps = "crm-api-redis|exited|redis:7|\n";
    let dock = dock(
        dir.path(),
        FakeExecutor::new().respond(PS_ALL, CmdOutput::ok(ps)),
    );
    dock.remove_container("redis", true).unwrap();
    let commands = dock.executor().commands();
    assert_eq!(
        &commands[1..],
        [
            "docker rm --force crm-api-redis",
            "docker volume rm --force crm-api-redis-data"
        ]
    );
}
