//! `config/env/<env>.toml`: the file of the environment the app runs in.

use std::path::Path;

use navree_config::{Config, ConfigSources};
use tempfile::TempDir;

fn app() -> TempDir {
    tempfile::Builder::new()
        .prefix("navree-environments-")
        .tempdir()
        .expect("tempdir")
}

fn write(dir: &Path, path: &str, contents: &str) {
    let target = dir.join(path);
    std::fs::create_dir_all(target.parent().expect("parent")).expect("directory");
    std::fs::write(target, contents).expect("write");
}

fn load(dir: &TempDir) -> Config {
    Config::load(&ConfigSources::defaults_in(dir.path())).expect("load")
}

fn port(config: &Config) -> Option<String> {
    config.get_string("server.port")
}

#[test]
fn the_environment_comes_from_app_toml() {
    let dir = app();
    write(
        dir.path(),
        "config/app.toml",
        "[app]\nenv = \"production\"\n[server]\nport = 80\nhost = \"base\"\n",
    );
    write(
        dir.path(),
        "config/env/production.toml",
        "[server]\nport = 443\n",
    );
    write(
        dir.path(),
        "config/env/staging.toml",
        "[server]\nport = 8443\n",
    );
    let config = load(&dir);
    assert_eq!(port(&config).as_deref(), Some("443"));
    assert_eq!(config.get_string("server.host").as_deref(), Some("base"));
    let origin = config.origin("server.port").expect("set").to_string();
    assert!(
        origin
            .replace('\\', "/")
            .ends_with("config/env/production.toml"),
        "{origin}"
    );
}

#[test]
fn a_top_level_env_in_app_toml_counts_too() {
    let dir = app();
    write(
        dir.path(),
        "config/app.toml",
        "env = \"testing\"\n[server]\nport = 80\n",
    );
    write(
        dir.path(),
        "config/env/testing.toml",
        "[server]\nport = 81\n",
    );
    assert_eq!(port(&load(&dir)).as_deref(), Some("81"));
}

#[test]
fn dotenv_names_the_environment_before_app_toml() {
    let dir = app();
    write(
        dir.path(),
        "config/app.toml",
        "[app]\nenv = \"production\"\n[server]\nport = 80\n",
    );
    write(dir.path(), ".env", "APP_ENV=staging\n");
    write(
        dir.path(),
        "config/env/production.toml",
        "[server]\nport = 443\n",
    );
    write(
        dir.path(),
        "config/env/staging.toml",
        "[server]\nport = 8443\n",
    );
    let config = load(&dir);
    assert_eq!(port(&config).as_deref(), Some("8443"));
    assert_eq!(
        config.environment().map(|e| e.to_string()).as_deref(),
        Some("staging")
    );
}

#[test]
fn without_a_name_the_environment_is_local() {
    let dir = app();
    write(dir.path(), "config/app.toml", "[server]\nport = 80\n");
    write(
        dir.path(),
        "config/env/local.toml",
        "[server]\nport = 3000\n",
    );
    write(
        dir.path(),
        "config/env/production.toml",
        "[server]\nport = 443\n",
    );
    assert_eq!(port(&load(&dir)).as_deref(), Some("3000"));
}

#[test]
fn a_missing_file_is_skipped() {
    let dir = app();
    write(
        dir.path(),
        "config/app.toml",
        "[app]\nenv = \"production\"\n[server]\nport = 80\n",
    );
    assert_eq!(port(&load(&dir)).as_deref(), Some("80"));
}

#[test]
fn dotenv_and_the_process_environment_outrank_the_environment_file() {
    let dir = app();
    write(
        dir.path(),
        "config/app.toml",
        "[app]\nenv = \"production\"\n[server]\nport = 80\n",
    );
    write(
        dir.path(),
        "config/env/production.toml",
        "[server]\nport = 443\n",
    );
    write(dir.path(), ".env", "APP_SERVER__PORT=9\n");
    assert_eq!(port(&load(&dir)).as_deref(), Some("9"));
}

#[test]
fn the_environment_file_is_not_a_duplicate_of_app_toml() {
    let dir = app();
    write(
        dir.path(),
        "config/app.toml",
        "[app]\nenv = \"production\"\n[server]\nport = 80\n",
    );
    write(
        dir.path(),
        "config/env/production.toml",
        "[server]\nport = 443\n",
    );
    assert!(
        load(&dir).shadowed().is_empty(),
        "an environment file is meant to replace what app.toml says"
    );
}

#[test]
fn a_name_that_is_not_a_file_name_loads_no_file() {
    let dir = app();
    write(
        dir.path(),
        "config/app.toml",
        "[app]\nenv = \"../secret\"\n[server]\nport = 80\n",
    );
    write(dir.path(), "secret.toml", "[server]\nport = 1\n");
    let sources = ConfigSources::defaults_in(dir.path());
    assert!(!format!("{sources:?}").contains("secret"), "{sources:?}");
    assert_eq!(
        port(&Config::load(&sources).expect("load")).as_deref(),
        Some("80")
    );
}
