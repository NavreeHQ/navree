//! `navree mcp` and `navree skills:install` through the binary: Navigator
//! spoken to as JSON-RPC lines over the child process's stdio, against the
//! fixture app in `tests/fixtures/navigator-app`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;

use serde_json::{Value, json};
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{ConnectOptions, Executor};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};

fn workspace() -> PathBuf {
    std::path::absolute(Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")).unwrap()
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A copy of the fixture app with its `Cargo.toml` (pointing at the
/// workspace's facade) and the workspace's `Cargo.lock`.
fn fixture_app() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/navigator-app");
    copy_dir(&fixtures, dir.path());
    let facade = workspace().join("crates").join("navree");
    let manifest = std::fs::read_to_string(dir.path().join("Cargo.toml.in"))
        .unwrap()
        .replace("{navree}", &facade.display().to_string().replace('\\', "/"));
    std::fs::write(dir.path().join("Cargo.toml"), manifest).unwrap();
    std::fs::copy(
        workspace().join("Cargo.lock"),
        dir.path().join("Cargo.lock"),
    )
    .unwrap();
    dir
}

async fn create_database(app: &Path) {
    let path = app.join("storage/app.db");
    let mut conn = SqliteConnectOptions::from_str("sqlite://x")
        .unwrap()
        .filename(&path)
        .create_if_missing(true)
        .connect()
        .await
        .unwrap();
    for statement in [
        "CREATE TABLE notes (id INTEGER PRIMARY KEY, title TEXT NOT NULL, api_token TEXT)",
        "INSERT INTO notes (title, api_token) VALUES ('first', 'tok_1'), ('second', 'tok_2')",
    ] {
        conn.execute(statement).await.unwrap();
    }
    sqlx::Connection::close(conn).await.unwrap();
}

/// `navree mcp`, spoken to as JSON-RPC lines over its stdio.
struct Navigator {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: i64,
}

impl Navigator {
    /// Sends request `method` and waits for its answer.
    async fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        let message = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        self.send(&message).await;
        loop {
            let mut line = String::new();
            let read = self.stdout.read_line(&mut line).await.unwrap();
            assert!(read > 0, "navree mcp closed its stdout");
            let answer: Value = serde_json::from_str(&line).unwrap();
            if answer["id"] == id {
                return answer;
            }
        }
    }

    async fn send(&mut self, message: &Value) {
        let mut line = message.to_string();
        line.push('\n');
        self.stdin.write_all(line.as_bytes()).await.unwrap();
        self.stdin.flush().await.unwrap();
    }

    /// Closes stdin and waits for the server to exit.
    async fn close(mut self) {
        drop(self.stdin);
        let status = tokio::time::timeout(std::time::Duration::from_secs(30), self.child.wait())
            .await
            .expect("navree mcp exits when its client goes")
            .unwrap();
        assert!(status.success(), "{status}");
    }
}

/// `navree mcp <args>` as a child process, initialized. With `own_target`,
/// the app builds into its own `target/` instead of the shared
/// `CARGO_TARGET_DIR`, so no earlier build is found.
async fn navigator(app: &Path, extra: &[&str], own_target: bool) -> (Navigator, Value) {
    let mut args = vec![
        "mcp".to_owned(),
        "--path".to_owned(),
        app.display().to_string(),
    ];
    args.extend(extra.iter().map(|a| (*a).to_owned()));
    let home = app.join(".navree-home");
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_navree"));
    command
        .args(&args)
        .env("NAVREE_HOME", &home)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .kill_on_drop(true);
    if own_target {
        command.env("CARGO_TARGET_DIR", app.join("target"));
    }
    let mut child = command.spawn().expect("navree mcp starts");
    let stdin = child.stdin.take().unwrap();
    let stdout = BufReader::new(child.stdout.take().unwrap());
    let mut navigator = Navigator {
        child,
        stdin,
        stdout,
        next_id: 0,
    };
    let info = navigator
        .request(
            "initialize",
            json!({
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": { "name": "navigator-test", "version": "1" },
            }),
        )
        .await["result"]
        .clone();
    navigator
        .send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
        .await;
    (navigator, info)
}

async fn call(client: &mut Navigator, tool: &str, args: Value) -> (bool, String) {
    let answer = client
        .request("tools/call", json!({ "name": tool, "arguments": args }))
        .await;
    let result = &answer["result"];
    let text = result["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("a text answer: {answer}"))
        .to_owned();
    (result["isError"] == true, text)
}

#[tokio::test]
async fn navree_mcp_serves_the_tools_over_stdio() {
    let app = fixture_app();
    create_database(app.path()).await;
    let (mut client, info) = navigator(app.path(), &["--no-build", "--docs-offline"], true).await;

    assert_eq!(info["serverInfo"]["name"], "navree");
    let tools = client.request("tools/list", json!({})).await;
    assert_eq!(tools["result"]["tools"].as_array().unwrap().len(), 12);

    let (error, text) = call(&mut client, "read_config", json!({})).await;
    assert!(!error, "{text}");
    let config: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(config["app"]["name"], "navigator-app");
    assert_eq!(config["mail"]["password"], "[redacted]");

    let (error, text) = call(
        &mut client,
        "query_database",
        json!({"sql": "SELECT id, title, api_token FROM notes ORDER BY id"}),
    )
    .await;
    assert!(!error, "{text}");
    let rows: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        rows["rows"],
        json!([[1, "first", "[redacted]"], [2, "second", "[redacted]"]])
    );

    let (error, text) = call(
        &mut client,
        "query_database",
        json!({"sql": "DELETE FROM notes"}),
    )
    .await;
    assert!(error && text.contains("not read-only"), "{text}");

    let (error, text) = call(&mut client, "last_error", json!({})).await;
    assert!(!error, "{text}");
    let last: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(last["request_id"], "0199c4b1-aaaa");
    assert_eq!(last["request"]["status"], 500);

    let (error, text) = call(
        &mut client,
        "search_docs",
        json!({"query": "agent skills install"}),
    )
    .await;
    assert!(!error, "{text}");
    assert!(text.contains("ai/agent-skills"), "{text}");

    // Nothing is built, and --no-build never builds: the app tools say so.
    let (error, text) = call(&mut client, "list_routes", json!({})).await;
    assert!(error && text.contains("--no-build"), "{text}");

    client.close().await;
}

#[test]
fn skills_install_writes_every_skill_into_both_folders() {
    let dir = tempfile::tempdir().unwrap();
    let skills = dir.path().join(".claude/skills");
    std::fs::create_dir_all(skills.join("crm-pipeline-stages")).unwrap();
    std::fs::write(skills.join("crm-pipeline-stages/SKILL.md"), "ours").unwrap();
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_navree"))
            .args(["skills:install"])
            .current_dir(dir.path())
            .output()
            .unwrap()
    };
    let output = run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Navree skills into .claude/skills and .agents/skills"),
        "{stdout}"
    );
    for folder in [".claude/skills", ".agents/skills"] {
        for skill in ["navree", "navree-database", "navree-navigator"] {
            let file = dir.path().join(folder).join(skill).join("SKILL.md");
            let text = std::fs::read_to_string(&file).unwrap();
            assert!(text.starts_with("---\nname: "), "{}", file.display());
        }
    }
    assert_eq!(
        std::fs::read_to_string(skills.join("crm-pipeline-stages/SKILL.md")).unwrap(),
        "ours"
    );
    // Again: the same files, nothing removed.
    let again = run();
    assert!(again.status.success());
    assert!(!String::from_utf8_lossy(&again.stdout).contains("removed"));
}

/// Builds the fixture app against the workspace and asks it for its
/// routes, module graph and commands. Ignored by default: it compiles the
/// framework in a fresh project. Run it with
/// `cargo test -p navree-cli --test navigator -- --ignored`.
#[tokio::test]
#[ignore = "builds a Navree app; run with --ignored"]
async fn navree_mcp_asks_a_built_app() {
    let app = fixture_app();
    let (mut client, _) = navigator(app.path(), &["--docs-offline"], false).await;

    let (error, text) = call(&mut client, "list_routes", json!({"path": "/api/notes"})).await;
    assert!(!error, "{text}");
    let routes: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(routes[0]["method"], "GET");
    assert_eq!(routes[0]["access"], "public");
    assert_eq!(routes[0]["module"], "NotesModule");
    assert_eq!(routes[0]["handler"], "NotesController::index");

    let (error, text) = call(
        &mut client,
        "module_graph",
        json!({"module": "NotesModule"}),
    )
    .await;
    assert!(!error, "{text}");
    let notes: Value = serde_json::from_str(&text).unwrap();
    assert!(
        notes["providers"].to_string().contains("NotesController"),
        "{notes}"
    );

    let (error, text) = call(&mut client, "list_commands", json!({})).await;
    assert!(!error, "{text}");
    assert!(
        text.contains("notes:prune") && text.contains("migrate:status"),
        "{text}"
    );

    let (error, text) = call(&mut client, "app_info", json!({})).await;
    assert!(!error, "{text}");
    let info: Value = serde_json::from_str(&text).unwrap();
    assert!(
        info["modules"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m == "NotesModule"),
        "{info}"
    );

    client.close().await;
}
