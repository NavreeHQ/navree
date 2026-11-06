//! End to end with a real container runtime: Redis and Mailpit started,
//! checked, listed and removed with their data; and, in a test of its own
//! (`#[ignore]`: it pulls six images), every service of the catalog started
//! once and asked what its client asks.
//!
//! Runs only with `NAVREE_TEST_DOCKER=1` (CI's nightly job, or a machine
//! with Docker or Podman running); otherwise it prints `SKIPPED` and
//! passes. `NAVREE_DOCK_RUNTIME` picks the runtime as usual.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::time::Duration;

use navree_dock::{Dock, ProcessExecutor, status_table};

/// The State column of `service`'s row in a status table, whatever the
/// columns' widths (they follow the longest name in the table).
fn state<'a>(table: &'a str, service: &str) -> Option<&'a str> {
    table.lines().find_map(|line| {
        let mut words = line.split_whitespace();
        if words.next() == Some(service) {
            words.next()
        } else {
            None
        }
    })
}

/// A port nothing listens on right now.
fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[test]
fn redis_and_mailpit_start_answer_and_go_away() {
    if std::env::var("NAVREE_TEST_DOCKER").as_deref() != Ok("1") {
        eprintln!("SKIPPED: set NAVREE_TEST_DOCKER=1 to run Dock against Docker or Podman");
        return;
    }
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let project = format!("dock-e2e-{}", std::process::id());
    std::fs::write(
        dir.path().join("dock.toml"),
        format!(
            "project = \"{project}\"\n\n[services.redis]\nport = {}\n\n[services.mailpit]\nport = {}\nui_port = {}\n",
            free_port(),
            free_port(),
            free_port()
        ),
    )
    .unwrap();
    let dock = Dock::open(dir.path(), ProcessExecutor::new(dir.path())).unwrap();

    let mut out = Vec::new();
    let result = dock.up(&[], &mut out);
    let printed = String::from_utf8_lossy(&out).into_owned();
    // Always clean up, even when `up` failed half way.
    let statuses = dock.status();
    let down = dock.down(&[], true, &mut Vec::new());
    result.unwrap_or_else(|e| panic!("{e}\n{printed}"));
    down.unwrap();

    assert!(printed.contains("redis"), "{printed}");
    let env = std::fs::read_to_string(dir.path().join(".env")).unwrap();
    assert!(env.contains("REDIS_URL=redis://127.0.0.1:"), "{env}");
    let table = status_table(&statuses.unwrap());
    assert_eq!(state(&table, "redis"), Some("running"), "{table}");
    assert_eq!(state(&table, "mailpit"), Some("running"), "{table}");
    let after = status_table(&dock.status().unwrap());
    assert_eq!(state(&after, "redis"), Some("stopped"), "{after}");
}

// -- every service of the catalog ------------------------------------------------

/// The container runtime's command.
fn runtime() -> String {
    std::env::var("NAVREE_DOCK_RUNTIME").unwrap_or_else(|_| "docker".to_owned())
}

/// Runs `command` in `container`, with `env` set; its standard output.
fn exec(container: &str, env: &[(&str, &str)], command: &[&str]) -> Result<String, String> {
    let mut run = Command::new(runtime());
    run.arg("exec");
    for (key, value) in env {
        run.arg("-e").arg(format!("{key}={value}"));
    }
    run.arg(container).args(command);
    let output = run.output().map_err(|e| format!("{run:?}: {e}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(format!(
            "{run:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

/// Connects to `port` on 127.0.0.1 and reads what the server sends first,
/// after sending `hello` (nothing when empty).
fn talk(port: u16, hello: &[u8], expect_reply: bool) -> Result<Vec<u8>, String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).map_err(|e| format!("{port}: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| e.to_string())?;
    if !hello.is_empty() {
        stream
            .write_all(hello)
            .map_err(|e| format!("{port}: {e}"))?;
    }
    let mut buffer = [0u8; 512];
    let read = stream
        .read(&mut buffer)
        .map_err(|e| format!("{port}: {e}"))?;
    if expect_reply && read == 0 {
        return Err(format!("{port}: the server said nothing"));
    }
    Ok(buffer[..read].to_vec())
}

/// `GET path` on 127.0.0.1:`port`, the whole response.
fn http_get(port: u16, path: &str) -> Result<String, String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).map_err(|e| format!("{port}: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| e.to_string())?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )
    .map_err(|e| format!("{port}: {e}"))?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|e| format!("{port}: {e}"))?;
    Ok(response)
}

/// Distinct ports nothing listens on right now.
fn free_ports<const N: usize>() -> [u16; N] {
    let mut ports = [0u16; N];
    let mut held = Vec::new();
    for port in &mut ports {
        // Hold each listener until all are chosen, so no two are the same.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        *port = listener.local_addr().unwrap().port();
        held.push(listener);
    }
    ports
}

#[test]
#[ignore = "pulls six images and starts six containers; the integrations workflow runs it"]
fn every_service_of_the_catalog_starts_and_answers_what_its_client_asks() {
    if std::env::var("NAVREE_TEST_DOCKER").as_deref() != Ok("1") {
        eprintln!("SKIPPED: set NAVREE_TEST_DOCKER=1 to run Dock against Docker or Podman");
        return;
    }
    let [
        postgres,
        redis,
        meilisearch,
        smtp,
        mailpit_ui,
        minio,
        minio_console,
        nats,
        nats_monitor,
    ] = free_ports::<9>();
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let project = format!("dock-all-{}", std::process::id());
    std::fs::write(
        dir.path().join("dock.toml"),
        format!(
            "project = \"{project}\"\n\n\
             [services.postgres]\nport = {postgres}\ndatabase = \"navree\"\nuser = \"navree\"\npassword = \"navree-pw\"\n\n\
             [services.redis]\nport = {redis}\n\n\
             [services.meilisearch]\nport = {meilisearch}\nmaster_key = \"navree-test-key-0123456789\"\n\n\
             [services.mailpit]\nport = {smtp}\nui_port = {mailpit_ui}\n\n\
             [services.minio]\nport = {minio}\nconsole_port = {minio_console}\nbucket = \"navree-dock\"\naccess_key = \"navreeadmin\"\nsecret_key = \"navreeadmin-secret\"\n\n\
             [services.nats]\nport = {nats}\nmonitor_port = {nats_monitor}\n"
        ),
    )
    .unwrap();
    let dock = Dock::open(dir.path(), ProcessExecutor::new(dir.path())).unwrap();

    let mut out = Vec::new();
    let up = dock.up(&[], &mut out);
    let printed = String::from_utf8_lossy(&out).into_owned();

    // Everything that can fail is collected, so the containers and their
    // volumes are removed whatever happens.
    let mut problems: Vec<String> = Vec::new();
    let mut check = |what: &str, result: Result<(), String>| {
        if let Err(error) = result {
            problems.push(format!("{what}: {error}"));
        }
    };
    if let Err(error) = &up {
        check("up", Err(format!("{error}\n{printed}")));
    } else {
        // The settings the app gets.
        let env = std::fs::read_to_string(dir.path().join(".env")).unwrap_or_default();
        for line in [
            format!("APP_DATABASE__URL=postgres://navree:navree-pw@127.0.0.1:{postgres}/navree"),
            format!("REDIS_URL=redis://127.0.0.1:{redis}"),
            "APP_SEARCH__ENGINE=meilisearch".to_owned(),
            format!("APP_SEARCH__URL=http://127.0.0.1:{meilisearch}"),
            format!("APP_MAIL__TRANSPORT=smtp://127.0.0.1:{smtp}"),
            format!("AWS_ENDPOINT_URL=http://127.0.0.1:{minio}"),
            format!("APP_MICRO__URL=nats://127.0.0.1:{nats}"),
        ] {
            check(
                "the .env file",
                env.lines()
                    .any(|have| have == line)
                    .then_some(())
                    .ok_or_else(|| format!("no `{line}` in:\n{env}")),
            );
        }
        let table = status_table(&dock.status().unwrap());
        for service in [
            "postgres",
            "redis",
            "meilisearch",
            "mailpit",
            "minio",
            "nats",
        ] {
            check(
                "the status table",
                (state(&table, service) == Some("running"))
                    .then_some(())
                    .ok_or_else(|| format!("{service} is not running:\n{table}")),
            );
        }

        // PostgreSQL answers its protocol (SSLRequest: 'S' or 'N'), and the
        // first-start setup created the TimescaleDB extension.
        check(
            "postgres protocol",
            talk(postgres, &[0, 0, 0, 8, 4, 210, 22, 47], true).and_then(|reply| {
                matches!(reply.first(), Some(b'S' | b'N'))
                    .then_some(())
                    .ok_or_else(|| format!("not a PostgreSQL answer: {reply:?}"))
            }),
        );
        check(
            "postgres extension",
            exec(
                &format!("{project}-postgres"),
                &[],
                &[
                    "psql",
                    "-U",
                    "navree",
                    "-d",
                    "navree",
                    "-tAc",
                    "select extname from pg_extension",
                ],
            )
            .and_then(|extensions| {
                extensions
                    .lines()
                    .any(|name| name.trim() == "timescaledb")
                    .then_some(())
                    .ok_or_else(|| format!("no timescaledb among: {extensions}"))
            }),
        );
        check(
            "redis",
            talk(redis, b"PING\r\n", true).and_then(|reply| {
                reply
                    .starts_with(b"+PONG")
                    .then_some(())
                    .ok_or_else(|| format!("not PONG: {reply:?}"))
            }),
        );
        check(
            "meilisearch",
            http_get(meilisearch, "/health").and_then(|response| {
                (response.starts_with("HTTP/1.1 200") && response.contains("available"))
                    .then_some(())
                    .ok_or_else(|| format!("not healthy: {response}"))
            }),
        );
        check(
            "mailpit",
            talk(smtp, b"", true).and_then(|greeting| {
                greeting
                    .starts_with(b"220")
                    .then_some(())
                    .ok_or_else(|| format!("not an SMTP greeting: {greeting:?}"))
            }),
        );
        // The bucket the first start made exists.
        let mc_host = "http://navreeadmin:navreeadmin-secret@127.0.0.1:9000";
        check(
            "minio bucket",
            exec(
                &format!("{project}-minio"),
                &[("MC_HOST_dock", mc_host)],
                &["mcli", "ls", "dock"],
            )
            .and_then(|buckets| {
                buckets
                    .contains("navree-dock")
                    .then_some(())
                    .ok_or_else(|| format!("no bucket navree-dock among: {buckets}"))
            }),
        );
        check(
            "nats",
            talk(nats, b"", true).and_then(|info| {
                info.starts_with(b"INFO ")
                    .then_some(())
                    .ok_or_else(|| format!("not a NATS greeting: {info:?}"))
            }),
        );
        // JetStream is on: its monitoring endpoint reports a store.
        check(
            "nats jetstream",
            http_get(nats_monitor, "/jsz").and_then(|response| {
                response
                    .contains("store_dir")
                    .then_some(())
                    .ok_or_else(|| format!("JetStream is not on: {response}"))
            }),
        );
        // Starting again changes nothing: every service is already running.
        let mut again = Vec::new();
        check(
            "second up",
            dock.up(&[], &mut again)
                .map_err(|error| error.to_string())
                .and_then(|()| {
                    let text = String::from_utf8_lossy(&again).into_owned();
                    (text.matches("already running").count() == 6)
                        .then_some(())
                        .ok_or_else(|| format!("not all six were already running:\n{text}"))
                }),
        );
    }

    let down = dock.down(&[], true, &mut Vec::new());
    let after = dock.status().map(|statuses| status_table(&statuses));
    assert!(problems.is_empty(), "{}", problems.join("\n"));
    down.unwrap();
    for service in [
        "postgres",
        "redis",
        "meilisearch",
        "mailpit",
        "minio",
        "nats",
    ] {
        let table = after.as_ref().unwrap();
        // Status is read after the removal.
        assert_ne!(state(table, service), Some("running"), "{table}");
    }
}
