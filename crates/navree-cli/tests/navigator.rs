//! `navree mcp` and `navree skills:install` through the binary: Navigator
//! spoken to by rmcp's client over the child process's stdio, against the
//! fixture app in `tests/fixtures/navigator-app`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;

use rmcp::RoleClient;
use rmcp::ServiceExt;
use rmcp::model::CallToolRequestParams;
use rmcp::service::RunningService;
use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};
use serde_json::{Value, json};
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{ConnectOptions, Executor};

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

/// `navree mcp <args>` as a child process, connected.
async fn navigator(app: &Path, extra: &[&str]) -> RunningService<RoleClient, ()> {
    let mut args = vec![
        "mcp".to_owned(),
        "--path".to_owned(),
        app.display().to_string(),
    ];
    args.extend(extra.iter().map(|a| (*a).to_owned()));
    let home = app.join(".navree-home");
    let transport = TokioChildProcess::new(
        tokio::process::Command::new(env!("CARGO_BIN_EXE_navree")).configure(|command| {
            command.args(&args).env("NAVREE_HOME", &home);
        }),
    )
    .expect("navree mcp starts");
    ().serve(transport).await.expect("initialize")
}

async fn call(client: &RunningService<RoleClient, ()>, tool: &str, args: Value) -> (bool, String) {
    let mut params = CallToolRequestParams::new(tool.to_owned());
    if let Value::Object(map) = args {
        params = params.with_arguments(map);
    }
    let result = client
        .call_tool(params)
        .await
        .expect("the call is answered");
    let text = result.content[0].as_text().expect("text").text.clone();
    (result.is_error == Some(true), text)
}

#[tokio::test]
async fn navree_mcp_serves_the_tools_over_stdio() {
    let app = fixture_app();
    create_database(app.path()).await;
    let client = navigator(app.path(), &["--no-build", "--docs-offline"]).await;

    let info = client.peer_info().expect("server info");
    assert_eq!(
        info.server_info.as_ref().map(|s| s.name.as_str()),
        Some("navree")
    );
    let tools = client.list_all_tools().await.unwrap();
    assert_eq!(tools.len(), 12);

    let (error, text) = call(&client, "read_config", json!({})).await;
    assert!(!error, "{text}");
    let config: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(config["app"]["name"], "navigator-app");
    assert_eq!(config["mail"]["password"], "[redacted]");

    let (error, text) = call(
        &client,
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
        &client,
        "query_database",
        json!({"sql": "DELETE FROM notes"}),
    )
    .await;
    assert!(error && text.contains("not read-only"), "{text}");

    let (error, text) = call(&client, "last_error", json!({})).await;
    assert!(!error, "{text}");
    let last: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(last["request_id"], "0199c4b1-aaaa");
    assert_eq!(last["request"]["status"], 500);

    let (error, text) = call(
        &client,
        "search_docs",
        json!({"query": "agent skills install"}),
    )
    .await;
    assert!(!error, "{text}");
    assert!(text.contains("ai/agent-skills"), "{text}");

    // Nothing is built with --no-build: the app tools say so.
    let (error, text) = call(&client, "list_routes", json!({})).await;
    assert!(error, "{text}");

    client.cancel().await.unwrap();
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
    let client = navigator(app.path(), &["--docs-offline"]).await;

    let (error, text) = call(&client, "list_routes", json!({"path": "/api/notes"})).await;
    assert!(!error, "{text}");
    let routes: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(routes[0]["method"], "GET");
    assert_eq!(routes[0]["access"], "public");
    assert_eq!(routes[0]["module"], "NotesModule");
    assert_eq!(routes[0]["handler"], "NotesController::index");

    let (error, text) = call(&client, "module_graph", json!({"module": "NotesModule"})).await;
    assert!(!error, "{text}");
    let notes: Value = serde_json::from_str(&text).unwrap();
    assert!(
        notes["providers"].to_string().contains("NotesController"),
        "{notes}"
    );

    let (error, text) = call(&client, "list_commands", json!({})).await;
    assert!(!error, "{text}");
    assert!(
        text.contains("notes:prune") && text.contains("migrate:status"),
        "{text}"
    );

    let (error, text) = call(&client, "app_info", json!({})).await;
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

    client.cancel().await.unwrap();
}
