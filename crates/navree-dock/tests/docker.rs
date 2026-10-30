//! End to end with a real container runtime: Redis and Mailpit started,
//! checked, listed and removed with their data.
//!
//! Runs only with `NAVREE_TEST_DOCKER=1` (CI's nightly job, or a machine
//! with Docker or Podman running); otherwise it prints `SKIPPED` and
//! passes. `NAVREE_DOCK_RUNTIME` picks the runtime as usual.

use std::net::TcpListener;

use navree_dock::{Dock, ProcessExecutor, status_table};

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
    assert!(table.contains("redis        running"), "{table}");
    assert!(table.contains("mailpit      running"), "{table}");
    let after = status_table(&dock.status().unwrap());
    assert!(after.contains("redis        stopped"), "{after}");
}
