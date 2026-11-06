//! Harbor against a real Ubuntu host: a server with systemd and sshd that
//! `harbor:provision` prepares (twice), `navree deploy` ships releases to, and
//! `deploy:rollback` moves back, with the real `ssh` command and the real
//! scripts. Everything else in this crate's suites runs the same code against
//! a fake executor and an in-memory host.
//!
//! What runs for real: the facts script, the provisioning scripts of the
//! `web` role (updates, the deploy user, SSH hardening, the firewall, Caddy
//! from its apt repository, systemd units), idempotent re-provisioning, the
//! release upload and unpack, the environment file, the blue and green systemd
//! units, the health check, the switch of Caddy and the `current` link, a
//! release that fails its health check (the live one is left alone), and the
//! rollback. The app is a small Python program that answers `/health/ready`,
//! so no Rust build is involved (the build step is a `Build` of the test's own).
//!
//! What stays out: the `postgres` and `redis` roles, the front end, several
//! servers, and a certificate from Let's Encrypt (the host has no public name).
//!
//! This runs in the integration workflow, which starts the host with
//! `scripts/integration-services.sh start harbor`; it is `#[ignore]`d in the
//! other suites. Run it with `-- --ignored`. The variables:
//!
//! - `NAVREE_TEST_HARBOR_HOST` and `NAVREE_TEST_HARBOR_PORT`: where sshd listens;
//! - `NAVREE_TEST_HARBOR_KEY`: the private key root accepts (its `.pub` next to
//!   it).
//!
//! A missing variable fails the test when `NAVREE_TEST_REQUIRE_SERVICES=1` and
//! prints `SKIPPED` otherwise.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use navree_harbor::config::Environment;
use navree_harbor::deploy::{Artifact, Build, DeployOptions, Deployer, ReleaseMeta, Target};
use navree_harbor::exec::SystemExecutor;
use navree_harbor::host::Host;
use navree_harbor::ops::Connector;
use navree_harbor::output::{Recorder, SystemClock};
use navree_harbor::project::Project;
use navree_harbor::provision::Provisioner;
use navree_harbor::render::AppInfo;
use navree_harbor::ssh::Identity;

const APP: &str = "demo";

/// The three release ids, oldest first (ids sort by time).
const R1: &str = "20261106-100000-aaaaaaa";
const R2: &str = "20261106-100100-bbbbbbb";
const R3: &str = "20261106-100200-ccccccc";

/// A small web app: answers `/health/ready` with 200 and `/` with the id of
/// its release, on the address in `APP_HTTP__ADDR`. A release whose `BROKEN`
/// file exists answers the health check with 503.
const APP_PROGRAM: &str = r#"#!/usr/bin/env python3
import http.server, os, sys

here = os.path.dirname(os.path.abspath(__file__))
release = open(os.path.join(here, ".harbor-release")).read().split("\n")[0].removeprefix("id=")
broken = os.path.exists(os.path.join(here, "BROKEN"))
host, port = os.environ.get("APP_HTTP__ADDR", "127.0.0.1:8081").rsplit(":", 1)


class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/health/ready":
            status, body = (503, "broken") if broken else (200, "ready")
        else:
            status, body = 200, release
        self.send_response(status)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body.encode())

    def log_message(self, *args):
        pass


http.server.ThreadingHTTPServer((host, int(port)), Handler).serve_forever()
"#;

/// The value of `variable`; `None` after a notice, or a panic when
/// `NAVREE_TEST_REQUIRE_SERVICES=1`.
fn service(variable: &str, test: &str) -> Option<String> {
    match std::env::var(variable) {
        Ok(value) if !value.trim().is_empty() => Some(value),
        _ => {
            assert!(
                std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() != Ok("1"),
                "{test}: {variable} is not set and NAVREE_TEST_REQUIRE_SERVICES=1"
            );
            println!("SKIPPED {test}: set {variable}");
            None
        }
    }
}

/// Builds a release archive: the program, the release's `.harbor-release`
/// file, and `BROKEN` when the release fails its health check.
struct FakeApp {
    broken: bool,
}

impl Build for FakeApp {
    fn build(&self, release: &ReleaseMeta, _: &Target) -> navree_harbor::Result<Artifact> {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        let tree = dir.path().join("tree");
        std::fs::create_dir(&tree).unwrap();
        std::fs::write(tree.join(APP), APP_PROGRAM).unwrap();
        std::fs::write(tree.join(".harbor-release"), release.file()).unwrap();
        if self.broken {
            std::fs::write(tree.join("BROKEN"), "").unwrap();
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(tree.join(APP), std::fs::Permissions::from_mode(0o755))
                .unwrap();
        }
        let archive = dir.path().join("release.tar.gz");
        let status = Command::new("tar")
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(&tree)
            .arg(".")
            .status()
            .expect("tar runs");
        assert!(status.success(), "tar failed");
        let size = std::fs::metadata(&archive).unwrap().len();
        Ok(Artifact {
            archive,
            size,
            summary: format!("{APP} (a Python program)"),
            keep: Some(dir),
        })
    }
}

/// The server, as the test reaches it.
struct Server {
    host: String,
    port: u16,
    key: PathBuf,
    /// Keeps the key copy alive.
    _dir: tempfile::TempDir,
}

impl Server {
    /// Runs `command` on the server as root, over `ssh`.
    fn root(&self, command: &str) -> String {
        let output = Command::new("ssh")
            .args(["-p", &self.port.to_string()])
            .args(["-o", "BatchMode=yes"])
            .args(["-o", "StrictHostKeyChecking=no"])
            .args(["-o", "UserKnownHostsFile=/dev/null"])
            .args(["-o", "LogLevel=ERROR"])
            .arg("-i")
            .arg(&self.key)
            .args(["-o", "IdentitiesOnly=yes"])
            .arg(format!("root@{}", self.host))
            .arg(command)
            .output()
            .expect("ssh runs");
        assert!(
            output.status.success(),
            "`{command}` failed on the server: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    /// What the app answers on the loopback interface of the server.
    fn get(&self, port: u16, path: &str) -> String {
        self.root(&format!(
            "curl --silent --max-time 5 http://127.0.0.1:{port}{path}"
        ))
    }

    /// The release the live slot serves, by asking the app through the port
    /// of the slot.
    fn live(&self) -> (String, String) {
        let slot = self.root(&format!("cat /srv/{APP}/.harbor-live"));
        let port = if slot == "blue" { 8081 } else { 8082 };
        (slot, self.get(port, "/"))
    }

    fn current(&self) -> String {
        self.root(&format!("readlink /srv/{APP}/current"))
    }
}

/// The server from the variables, with a copy of its key that only this user
/// can read (ssh refuses a key others can read).
fn server(test: &str) -> Option<Server> {
    let host = service("NAVREE_TEST_HARBOR_HOST", test)?;
    let port = service("NAVREE_TEST_HARBOR_PORT", test)?
        .parse()
        .expect("a port");
    let source = PathBuf::from(service("NAVREE_TEST_HARBOR_KEY", test)?);
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let key = dir.path().join("id_ed25519");
    std::fs::copy(&source, &key).expect("the key can be read");
    let mut public = source.clone().into_os_string();
    public.push(".pub");
    std::fs::copy(PathBuf::from(public), dir.path().join("id_ed25519.pub"))
        .expect("the public key is next to the key");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    Some(Server {
        host,
        port,
        key,
        _dir: dir,
    })
}

/// `harbor.toml` for the server, in a directory of its own.
fn project(server: &Server, dir: &Path) -> Project {
    std::fs::write(
        dir.join("harbor.toml"),
        format!(
            r#"app = "{APP}"
domain = "demo.acme.test"

[deploy]
health_check = "/health/ready"
health_timeout_secs = 20
migrate = false
keep_releases = 5

[servers.app-1]
host = "{host}"
roles = ["web"]

[ssh]
port = {port}
key = "{key}"
"#,
            host = server.host,
            port = server.port,
            key = server.key.display().to_string().replace('\\', "/"),
        ),
    )
    .unwrap();
    Project::load(dir, Environment::production()).expect("a valid harbor.toml")
}

fn provision(project: &mut Project, server: &Server) -> String {
    let recorder = Recorder::new();
    Provisioner {
        exec: Arc::new(SystemExecutor),
        identity: Identity::file(&server.key),
        progress: &recorder,
        clock: &SystemClock,
    }
    .run(project, None, None)
    .unwrap_or_else(|error| panic!("provisioning failed: {error}\n{}", recorder.text()));
    recorder.text()
}

/// A deployer for the project's one server.
fn deployer<'a>(
    project: &'a Project,
    info: &'a AppInfo,
    server: &Server,
    progress: &'a Recorder,
) -> Deployer<'a> {
    let connector = Connector {
        exec: Arc::new(SystemExecutor),
        identity: Identity::file(&server.key),
    };
    let host = connector
        .host(&project.config, &project.config.servers[0], info)
        .expect("the host key was recorded by provisioning");
    Deployer {
        config: &project.config,
        env_name: project.env.name(),
        info,
        hosts: vec![Arc::new(host) as Arc<dyn Host>],
        progress,
        clock: &SystemClock,
    }
}

fn release(id: &str, subject: &str) -> ReleaseMeta {
    ReleaseMeta {
        id: id.to_owned(),
        commit: String::new(),
        subject: subject.to_owned(),
    }
}

#[test]
#[ignore = "needs the servers of scripts/integration-services.sh"]
fn a_real_ubuntu_host_is_provisioned_twice_deployed_to_and_rolled_back() {
    let test = "a_real_ubuntu_host_is_provisioned_twice_deployed_to_and_rolled_back";
    let Some(server) = server(test) else {
        return;
    };
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let mut project = project(&server, dir.path());

    // -- provision, then again ------------------------------------------------
    let first = provision(&mut project, &server);
    assert!(first.contains("Provisioned app-1"), "{first}");
    assert!(
        project.config.servers[0].host_key.is_some(),
        "the host key is pinned in harbor.toml"
    );
    assert_eq!(server.root("id -un navree"), "navree");
    assert_eq!(server.root("systemctl is-active caddy"), "active");
    assert!(
        server
            .root("sshd -T | grep -i '^passwordauthentication'")
            .ends_with("no"),
        "password logins are off"
    );
    let state = |server: &Server| {
        server.root(
            "sha256sum /etc/caddy/Caddyfile /etc/sudoers.d/* /etc/ssh/sshd_config.d/* 2>/dev/null; \
             systemctl show -p MainPID --value caddy",
        )
    };
    let before = state(&server);

    let second = provision(&mut project, &server);
    assert!(second.contains("Provisioned app-1"), "{second}");
    assert_eq!(
        state(&server),
        before,
        "provisioning again changes no file and restarts nothing"
    );

    // -- deploy ----------------------------------------------------------------
    let info = AppInfo::detect(&project.config, dir.path());
    let progress = Recorder::new();
    let deployer = deployer(&project, &info, &server, &progress);
    let secrets = BTreeMap::new();
    let options = DeployOptions::default();

    let id = deployer
        .deploy(
            &FakeApp { broken: false },
            &release(R1, "first"),
            &secrets,
            &options,
        )
        .unwrap_or_else(|error| panic!("the first deploy failed: {error}\n{}", progress.text()));
    assert_eq!(id, R1);
    assert_eq!(server.live(), ("blue".to_owned(), R1.to_owned()));
    assert_eq!(server.current(), format!("releases/{R1}"));

    deployer
        .deploy(
            &FakeApp { broken: false },
            &release(R2, "second"),
            &secrets,
            &options,
        )
        .unwrap_or_else(|error| panic!("the second deploy failed: {error}\n{}", progress.text()));
    assert_eq!(
        server.live(),
        ("green".to_owned(), R2.to_owned()),
        "the new release goes to the idle slot and takes over"
    );
    // The old slot is still there to go back to.
    assert_eq!(server.get(8081, "/"), R1);

    // -- a release that fails its health check ---------------------------------
    let refused = deployer
        .deploy(
            &FakeApp { broken: true },
            &release(R3, "third"),
            &secrets,
            &options,
        )
        .expect_err("the health check fails");
    assert!(
        refused.to_string().contains("health"),
        "{refused}\n{}",
        progress.text()
    );
    assert_eq!(
        server.live(),
        ("green".to_owned(), R2.to_owned()),
        "the live release is left alone"
    );
    assert_eq!(server.current(), format!("releases/{R2}"));

    // -- roll back ---------------------------------------------------------------
    let back = deployer.rollback(None).expect("a rollback");
    assert_eq!(back, R1, "to the release before the current one");
    assert_eq!(server.live().1, R1);
    assert_eq!(server.current(), format!("releases/{R1}"));

    let forward = deployer
        .rollback(Some(R2))
        .expect("a rollback to a named release");
    assert_eq!(forward, R2);
    assert_eq!(server.live().1, R2);

    let listed = deployer.releases().expect("the releases");
    let ids: Vec<&str> = listed
        .iter()
        .map(|(release, _)| release.id.as_str())
        .collect();
    assert_eq!(ids, [R3, R2, R1], "newest first, the failed one included");
    assert!(
        listed
            .iter()
            .any(|(release, live)| release.id == R2 && *live)
    );
}
