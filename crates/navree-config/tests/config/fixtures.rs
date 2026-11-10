//! The merged tree of every fixture, compared with the JSON recorded next to it.
//!
//! `tests/fixtures/` holds real configuration files:
//!
//! - `rendered/`: the `config/app.toml` and `.env` that `navree new` writes for
//!   each starter kit and database, rendered with the defaults (see
//!   `crates/navree-cli/tests/config_fixtures.rs`);
//! - `repo/`: the configuration of a shop API, the system app, a load-test
//!   API and the Navigator test app;
//! - `cases/`: hand-written files for precedence, shapes, environment
//!   mapping, every TOML type and every `.env` form.
//!
//! A fixture directory holds the files its sources read, an optional
//! `fixture.json` (`{"sources": ["dotenv:.env", "file:config/app.toml",
//! "file:config/production.toml", "env"]}`, the conventional three when
//! absent: the config file and `.env` in the order of
//! `ConfigSources::DOTENV_RANKS_ABOVE_FILES`, then the environment) and an
//! optional `env.json`: the environment variables overlaid by
//! the `env` source (a directory without one uses its parent's). Each fixture
//! records two trees:
//!
//! - `expected.json`: everything merged, the environment overlay applied;
//! - `expected-files.json`: the same without the overlay (only when the
//!   fixture has an `env.json`).
//!
//! `NAVREE_CONFIG_RECORD=1 cargo test -p navree-config --test config fixtures`
//! rewrites the recorded files from the current loader. A recorded tree is a
//! contract: change it only on purpose, and say why in the commit.

use std::path::{Path, PathBuf};

use navree_config::{Config, ConfigSources};
use serde_json::Value;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn recording() -> bool {
    std::env::var_os("NAVREE_CONFIG_RECORD").is_some_and(|v| v == "1")
}

/// Every fixture directory: `<group>/<name>` under `tests/fixtures`.
fn fixtures() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for group in ["rendered", "repo", "cases"] {
        let mut group_dirs: Vec<PathBuf> = std::fs::read_dir(root().join(group))
            .unwrap_or_else(|e| panic!("{group}: {e}"))
            .map(|entry| entry.expect("directory entry").path())
            .filter(|path| path.is_dir())
            .collect();
        group_dirs.sort();
        dirs.extend(group_dirs);
    }
    dirs
}

fn name(dir: &Path) -> String {
    dir.strip_prefix(root())
        .expect("under the fixtures root")
        .to_string_lossy()
        .replace('\\', "/")
}

fn read_json(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    Some(serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display())))
}

/// The fixture's environment overlay: its own `env.json`, else its parent's.
fn overlay(dir: &Path) -> Option<Vec<(String, String)>> {
    let json = read_json(&dir.join("env.json"))
        .or_else(|| read_json(&dir.parent().expect("parent").join("env.json")))?;
    let map = json.as_object().expect("env.json is an object");
    Some(
        map.iter()
            .map(|(k, v)| (k.clone(), v.as_str().expect("string value").to_owned()))
            .collect(),
    )
}

fn sources(dir: &Path, env: Option<&[(String, String)]>) -> ConfigSources {
    let spec: Vec<String> = match read_json(&dir.join("fixture.json")) {
        Some(json) => json["sources"]
            .as_array()
            .expect("sources array")
            .iter()
            .map(|v| v.as_str().expect("string").to_owned())
            .collect(),
        None => {
            let (dotenv, file) = ("dotenv:.env", "file:config/app.toml");
            let files = if ConfigSources::DOTENV_RANKS_ABOVE_FILES {
                [file, dotenv]
            } else {
                [dotenv, file]
            };
            files
                .into_iter()
                .chain(["env"])
                .map(str::to_owned)
                .collect()
        }
    };
    let mut sources = ConfigSources::new();
    for item in spec {
        sources = match item.split_once(':') {
            Some(("dotenv", path)) => sources.dotenv(dir.join(path)),
            Some(("file", path)) => sources.file(dir.join(path)),
            None if item == "env" => {
                sources.env_vars("APP_", env.unwrap_or_default().iter().cloned())
            }
            _ => panic!("unknown source `{item}`"),
        };
    }
    sources
}

fn tree(sources: &ConfigSources) -> Value {
    Config::load(sources)
        .expect("fixture loads")
        .section::<Value>("")
        .expect("the whole tree")
}

/// Compares `actual` with the JSON recorded in `file`, or records it.
fn check(dir: &Path, file: &str, actual: &Value, failures: &mut Vec<String>) {
    let path = dir.join(file);
    if recording() {
        let mut text = serde_json::to_string_pretty(actual).expect("json");
        text.push('\n');
        std::fs::write(&path, text).expect("record");
        return;
    }
    match read_json(&path) {
        Some(expected) if expected == *actual => {}
        Some(expected) => failures.push(format!(
            "{}/{file}: the merged tree changed\n  recorded: {expected}\n  loaded:   {actual}",
            name(dir)
        )),
        None => failures.push(format!(
            "{}/{file} is not recorded (NAVREE_CONFIG_RECORD=1)",
            name(dir)
        )),
    }
}

#[test]
fn merged_trees_match_the_recorded_json() {
    let mut failures = Vec::new();
    let all = fixtures();
    assert!(all.len() >= 17, "fixtures are missing: {}", all.len());
    for dir in &all {
        match overlay(dir) {
            Some(env) => {
                check(
                    dir,
                    "expected.json",
                    &tree(&sources(dir, Some(&env))),
                    &mut failures,
                );
                check(
                    dir,
                    "expected-files.json",
                    &tree(&sources(dir, None)),
                    &mut failures,
                );
            }
            None => check(
                dir,
                "expected.json",
                &tree(&sources(dir, None)),
                &mut failures,
            ),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn an_environment_overlay_only_adds_keys_under_the_prefix() {
    // The overlay of every fixture holds names outside `APP_`; none of them
    // may reach the tree, whatever case they are written in.
    for dir in fixtures() {
        let Some(env) = overlay(&dir) else { continue };
        let with = tree(&sources(&dir, Some(&env)));
        let text = with.to_string();
        for (var, value) in &env {
            if var.to_ascii_uppercase().starts_with("APP_") {
                continue;
            }
            assert!(
                !text.contains(value.as_str()),
                "{}: `{var}` is outside the prefix but its value is in the tree",
                name(&dir)
            );
        }
    }
}
