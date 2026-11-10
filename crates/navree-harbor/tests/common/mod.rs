//! Shared fixtures of the integration tests.
#![allow(dead_code)]

use std::path::Path;
use std::process::Command;

use navree_harbor::HarborConfig;
use navree_harbor::render::AppInfo;

/// The single-server CRM of the docs.
pub const SINGLE: &str = r#"
app = "crm-api"
domain = "crm.example.com"

[build]
features = ["postgres"]

[deploy]
keep_releases = 5
health_check = "/health/ready"
health_timeout_secs = 60
shared = ["storage"]

[processes]
web = { command = "serve" }
worker = { command = "queue:work", instances = 2 }
scheduler = { command = "schedule:work" }

[servers.app-1]
host = "203.0.113.10"
roles = ["web", "worker", "scheduler", "postgres", "redis"]
host_key = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJzPinnedKey"

[env]
APP_APP__URL = "https://crm.example.com"
APP_MAIL__FROM = "Navree CRM <crm@example.com>"
APP_LOG__LEVEL = "info"
"#;

/// The several-servers CRM of the docs.
pub const MULTI: &str = r#"
app = "crm-api"
domain = "crm.example.com"
aliases = ["www.crm.example.com"]

[deploy]
trusted_proxies = ["10.0.0.0/24"]

[web]
metrics_allow = ["10.0.0.5"]
proxy_protocol = true

[servers.web-1]
host = "203.0.113.11"
private_ip = "10.0.0.11"
roles = ["web", "scheduler"]

[servers.web-2]
host = "203.0.113.12"
private_ip = "10.0.0.12"
roles = ["web"]

[servers.worker-1]
host = "203.0.113.21"
private_ip = "10.0.0.21"
roles = ["worker"]

[servers.data-1]
host = "203.0.113.31"
private_ip = "10.0.0.31"
roles = ["postgres", "redis"]
"#;

/// Parses and validates `text`.
pub fn config(text: &str) -> HarborConfig {
    let config = HarborConfig::parse(text, Path::new("harbor.toml")).expect("valid TOML");
    let problems = config.validate();
    assert!(problems.is_empty(), "{problems:#?}");
    config
}

/// An app without a front end or gRPC.
pub fn plain() -> AppInfo {
    AppInfo::default()
}

/// Checks `script`'s syntax with `bash -n` when bash is installed.
pub fn assert_bash_syntax(name: &str, script: &str) {
    use std::io::Write as _;
    use std::process::Stdio;
    let Ok(mut child) = Command::new("bash")
        .arg("-n")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    else {
        eprintln!("bash is not installed; skipping the syntax check of {name}");
        return;
    };
    child
        .stdin
        .take()
        .unwrap()
        .write_all(navree_harbor::ssh::wrap_script(script).as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    // bash always says what is wrong with a script. A failure with nothing to
    // say is a launcher that didn't start it: on Windows `bash` can be the WSL
    // launcher in System32.
    if !output.status.success() && output.stderr.is_empty() {
        eprintln!("bash did not start; skipping the syntax check of {name}");
        return;
    }
    assert!(
        output.status.success(),
        "{name} is not valid bash:\n{}\n{script}",
        String::from_utf8_lossy(&output.stderr)
    );
}
