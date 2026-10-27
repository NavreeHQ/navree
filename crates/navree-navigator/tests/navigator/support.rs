//! A fixture app on disk, a SQLite database for it, and a Navigator over
//! a fake process runner.

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;

use navree_navigator::app::{FakeRunner, ProcessOutput};
use navree_navigator::docs::{DocsSettings, EMBEDDED_VERSION, HttpFetcher};
use navree_navigator::{Navigator, NavigatorOptions, ToolOutput};
use serde_json::{Value, json};
use sqlx::ConnectOptions;
use sqlx::Executor;
use sqlx::sqlite::SqliteConnectOptions;

/// A fixture app: `Cargo.toml`, `Cargo.lock` pinning navree, config.
pub struct Fixture {
    pub dir: tempfile::TempDir,
    pub runner: Arc<FakeRunner>,
}

impl Fixture {
    pub fn new(app_toml: &str) -> Fixture {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        write(
            &root.join("Cargo.toml"),
            "[package]\nname = \"crm\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nnavree = { version = \"0.1\", features = [\"postgres\"] }\n",
        );
        write(
            &root.join("Cargo.lock"),
            &format!(
                "version = 4\n\n[[package]]\nname = \"crm\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"navree\"\nversion = \"{EMBEDDED_VERSION}\"\n"
            ),
        );
        write(&root.join("config/app.toml"), app_toml);
        Fixture {
            dir,
            runner: FakeRunner::new(),
        }
    }

    pub fn root(&self) -> &Path {
        self.dir.path()
    }

    pub fn navigator(&self) -> Navigator {
        self.navigator_with(NavigatorOptions {
            path: self.root().to_path_buf(),
            docs_offline: true,
            ..NavigatorOptions::default()
        })
    }

    pub fn navigator_with(&self, options: NavigatorOptions) -> Navigator {
        let docs = DocsSettings {
            home: Some(self.root().join(".navree-home")),
            base_url: "http://127.0.0.1:9".to_owned(),
            offline: options.docs_offline,
            fetcher: Arc::new(HttpFetcher::new().expect("fetcher")),
        };
        Navigator::with(options, self.runner.clone(), docs)
    }

    /// Scripts `cargo metadata` and a successful `cargo build` of the
    /// `crm` binary (a placeholder file).
    pub fn built(&self) -> PathBuf {
        let binary = self.root().join("target/debug/crm.exe");
        write(&binary, "binary");
        let metadata = json!({
            "packages": [{"name": "crm", "targets": [{"name": "crm", "kind": ["bin"]}]}],
            "target_directory": self.root().join("target"),
        });
        let build = [
            json!({"reason": "compiler-artifact", "target": {"name": "crm", "kind": ["bin"]}, "executable": binary}),
            json!({"reason": "build-finished", "success": true}),
        ]
        .map(|v| v.to_string())
        .join("\n");
        self.runner
            .on("cargo", &["metadata"], ok(&metadata.to_string()))
            .on("cargo", &["build"], ok(&build))
            .on(
                "rustc",
                &["--version"],
                ok("rustc 1.95.0 (abc 2026-09-01)\n"),
            );
        binary
    }

    /// Scripts the binary's answer to `args`.
    pub fn answers(&self, args: &[&str], value: Value) {
        self.runner.on(
            "crm",
            args,
            ok(&serde_json::to_string_pretty(&value).unwrap()),
        );
    }
}

pub fn ok(stdout: &str) -> ProcessOutput {
    ProcessOutput {
        success: true,
        stdout: stdout.to_owned(),
        stderr: String::new(),
    }
}

pub fn write(path: &Path, contents: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

/// Creates the SQLite database `path` and runs `statements` on it.
pub async fn sqlite_db(path: &Path, statements: &[&str]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut conn = SqliteConnectOptions::from_str("sqlite://x")
        .unwrap()
        .filename(path)
        .create_if_missing(true)
        .connect()
        .await
        .expect("create the database");
    for statement in statements {
        conn.execute(sqlx::AssertSqlSafe(statement.to_string()))
            .await
            .expect(statement);
    }
    sqlx::Connection::close(conn).await.unwrap();
}

/// A tool's JSON answer, panicking on errors.
pub async fn call(navigator: &Navigator, tool: &str, args: Value) -> Value {
    match navigator.call(tool, args).await {
        Ok(ToolOutput::Json(value)) => value,
        Ok(ToolOutput::Markdown(text)) => Value::String(text),
        Err(error) => panic!("{tool}: {error}"),
    }
}

/// A tool's error message, panicking on success.
pub async fn refused(navigator: &Navigator, tool: &str, args: Value) -> String {
    match navigator.call(tool, args).await {
        Ok(output) => panic!("{tool} answered {output:?}"),
        Err(error) => error.0,
    }
}
