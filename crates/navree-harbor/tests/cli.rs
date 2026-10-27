//! Harbor's commands end to end, with a fake executor standing in for
//! `ssh`, `cargo`, `git` and `docker`: what each command runs, what it
//! prints and what it writes.

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

use clap::Parser;
use navree_harbor::cli::{Context, HarborCommand, run};
use navree_harbor::exec::{Cmd, FakeExecutor, Output};
use navree_harbor::output::{FixedClock, Recorder};
use navree_harbor::{Environment, Project};

#[derive(Debug, Parser)]
struct TestCli {
    #[command(subcommand)]
    command: HarborCommand,
}

fn parse(args: &[&str]) -> HarborCommand {
    TestCli::try_parse_from(std::iter::once("navree").chain(args.iter().copied()))
        .unwrap_or_else(|e| panic!("{args:?}: {e}"))
        .command
}

/// An app directory with Cargo.toml (and a home with an SSH key).
struct App {
    dir: tempfile::TempDir,
    home: tempfile::TempDir,
    recorder: Recorder,
    vars: Arc<Mutex<BTreeMap<String, String>>>,
    stdin: Arc<Mutex<String>>,
}

impl App {
    fn new() -> App {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"crm-api\"\nversion = \"0.1.0\"\n\n[dependencies]\nnavree = { version = \"0.1\", features = [\"postgres\"] }\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        std::fs::write(dir.path().join("config/app.toml"), "[http]\n").unwrap();
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".ssh")).unwrap();
        std::fs::write(home.path().join(".ssh/id_ed25519"), "PRIVATE KEY\n").unwrap();
        std::fs::write(
            home.path().join(".ssh/id_ed25519.pub"),
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMine me@laptop\n",
        )
        .unwrap();
        App {
            dir,
            home,
            recorder: Recorder::new(),
            vars: Arc::default(),
            stdin: Arc::default(),
        }
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn context(&self, exec: &FakeExecutor) -> Context {
        let vars = self.vars.clone();
        let stdin = self.stdin.clone();
        Context {
            cwd: self.path().to_path_buf(),
            vars: Box::new(move |name| vars.lock().unwrap().get(name).cloned()),
            home: Some(self.home.path().to_path_buf()),
            exec: Arc::new(exec.clone()),
            clock: Arc::new(FixedClock::example()),
            out: Arc::new(self.recorder.clone()),
            prompt: Box::new(|_| Ok("typed-secret".to_owned())),
            stdin: Box::new(move || Ok(stdin.lock().unwrap().clone())),
            host_os: "linux".into(),
            host_arch: "x86_64".into(),
        }
    }

    fn run(&self, exec: &FakeExecutor, args: &[&str]) -> anyhow::Result<ExitCode> {
        run(&parse(args), &self.context(exec))
    }

    fn harbor(&self, text: &str) {
        std::fs::write(self.path().join("harbor.toml"), text).unwrap();
    }

    fn harbor_text(&self) -> String {
        std::fs::read_to_string(self.path().join("harbor.toml")).unwrap()
    }

    /// Everything printed so far.
    fn output(&self) -> String {
        self.recorder.text()
    }
}

/// The step a script names on its first line (`# harbor: <step> on ...`),
/// inside the strict-mode wrapper.
fn step_of(cmd: &Cmd) -> String {
    cmd.stdin_text()
        .lines()
        .take(3)
        .find_map(|l| l.strip_prefix("# harbor: "))
        .unwrap_or("")
        .to_owned()
}

/// The known-hosts file an `ssh` command uses.
fn known_hosts(cmd: &Cmd) -> Option<PathBuf> {
    cmd.args.iter().find_map(|a| {
        a.strip_prefix("UserKnownHostsFile=\"")
            .and_then(|p| p.strip_suffix('"'))
            .map(PathBuf::from)
    })
}

#[test]
fn every_command_parses() {
    for args in [
        &["harbor:init"][..],
        &["harbor:init", "--env", "staging"],
        &["harbor:provision"],
        &["harbor:provision", "app-1", "--env", "staging"],
        &["harbor:secrets", "set", "APP_KEY", "--generate"],
        &[
            "harbor:secrets",
            "set",
            "TOKEN",
            "--stdin",
            "--env",
            "staging",
        ],
        &["harbor:secrets", "list"],
        &["harbor:secrets", "reveal", "APP_KEY"],
        &["harbor:secrets", "unset", "APP_KEY"],
        &["harbor:secrets", "export"],
        &["harbor:secrets", "rotate-key"],
        &["deploy"],
        &[
            "deploy",
            "--server",
            "web-1",
            "--no-migrate",
            "--maintenance",
            "--env",
            "staging",
        ],
        &["deploy", "--release", "20261026-091512-3f2a9c1"],
        &["deploy:rollback"],
        &["deploy:rollback", "--to", "20261025-163003-9b17e44"],
        &["deploy:releases"],
        &["harbor:status", "--server", "app-1"],
        &[
            "harbor:logs",
            "--server",
            "app-1",
            "--process",
            "web",
            "--lines",
            "200",
            "--follow",
        ],
        &["harbor:run", "migrate:status"],
        &[
            "harbor:run",
            "--server",
            "web-1",
            "queue:retry",
            "all",
            "--force",
        ],
        &["harbor:ssh"],
        &["harbor:ssh", "web-2"],
        &["harbor:backup", "--download"],
        &["harbor:ci", "github"],
        &["build:image"],
        &[
            "build:image",
            "--tag",
            "registry.example.com/crm-api:1.4.0",
            "--push",
        ],
        &["build:image", "--platform", "linux/arm64"],
        &["build:image", "--dockerfile"],
    ] {
        parse(args);
    }
    let HarborCommand::Run(run) = parse(&["harbor:run", "queue:retry", "all", "--force"]) else {
        panic!("expected harbor:run");
    };
    assert_eq!(run.command, ["queue:retry", "all", "--force"]);
    assert!(
        TestCli::try_parse_from([
            "navree",
            "harbor:secrets",
            "set",
            "A",
            "--generate",
            "--stdin"
        ])
        .is_err()
    );
    assert!(TestCli::try_parse_from(["navree", "harbor:ci", "gitlab"]).is_err());
}

#[test]
fn init_and_secrets() {
    let app = App::new();
    let fake = FakeExecutor::default();
    app.run(&fake, &["harbor:init"]).unwrap();
    insta::assert_snapshot!(app.output(), @r"
      created  harbor.toml
      created  harbor.key         # the secrets key: never commit it
      created  .gitignore         # ignores harbor.key
    Next: set the servers in harbor.toml, then `navree harbor:provision`.
    ");

    app.run(&fake, &["harbor:secrets", "set", "APP_KEY", "--generate"])
        .unwrap();
    app.run(&fake, &["harbor:secrets", "set", "APP_MAIL__TRANSPORT"])
        .unwrap();
    *app.stdin.lock().unwrap() = "token-from-stdin\n".into();
    app.run(
        &fake,
        &[
            "harbor:secrets",
            "set",
            "APP_OBSERVE__METRICS_TOKEN",
            "--stdin",
        ],
    )
    .unwrap();
    let text = app.harbor_text();
    assert!(text.contains("APP_KEY = \"harbor:v1:"), "{text}");
    assert!(!text.contains("typed-secret") && !text.contains("token-from-stdin"));

    let before = app.recorder.lines().len();
    app.run(&fake, &["harbor:secrets", "list"]).unwrap();
    app.run(&fake, &["harbor:secrets", "reveal", "APP_MAIL__TRANSPORT"])
        .unwrap();
    app.run(
        &fake,
        &["harbor:secrets", "reveal", "APP_OBSERVE__METRICS_TOKEN"],
    )
    .unwrap();
    let lines = app.recorder.lines()[before..].to_vec();
    assert_eq!(
        lines,
        [
            "APP_KEY",
            "APP_MAIL__TRANSPORT",
            "APP_OBSERVE__METRICS_TOKEN",
            "typed-secret",
            "token-from-stdin",
        ]
    );

    let before = app.recorder.lines().len();
    app.run(&fake, &["harbor:secrets", "export"]).unwrap();
    let export = app.recorder.lines()[before..].to_vec();
    assert_eq!(export[0], "APP_APP__URL=https://crm-api.example.com");
    assert!(export[1].starts_with("APP_KEY=base64:"), "{export:?}");
    assert_eq!(export[2], "APP_MAIL__TRANSPORT=typed-secret");
    assert_eq!(export[3], "APP_OBSERVE__METRICS_TOKEN=token-from-stdin");

    app.run(
        &fake,
        &["harbor:secrets", "unset", "APP_OBSERVE__METRICS_TOKEN"],
    )
    .unwrap();
    let old_key = std::fs::read_to_string(app.path().join("harbor.key")).unwrap();
    app.run(&fake, &["harbor:secrets", "rotate-key"]).unwrap();
    let new_key = std::fs::read_to_string(app.path().join("harbor.key")).unwrap();
    assert_ne!(old_key, new_key);
    let output = app.recorder.text();
    assert!(
        output.contains("Re-encrypted 2 secrets under a new key in harbor.key."),
        "{output}"
    );
    let before = app.recorder.lines().len();
    app.run(&fake, &["harbor:secrets", "reveal", "APP_MAIL__TRANSPORT"])
        .unwrap();
    assert_eq!(app.recorder.lines()[before], "typed-secret");

    // An empty value is refused, and an unknown secret is an error.
    *app.stdin.lock().unwrap() = "\n".into();
    assert!(
        app.run(&fake, &["harbor:secrets", "set", "X", "--stdin"])
            .is_err()
    );
    assert!(
        app.run(&fake, &["harbor:secrets", "reveal", "NOPE"])
            .is_err()
    );
}

#[test]
fn the_key_can_come_from_the_environment() {
    let app = App::new();
    let fake = FakeExecutor::default();
    app.run(&fake, &["harbor:init", "--env", "staging"])
        .unwrap();
    let key = std::fs::read_to_string(app.path().join("harbor.staging.key")).unwrap();
    std::fs::remove_file(app.path().join("harbor.staging.key")).unwrap();
    app.vars
        .lock()
        .unwrap()
        .insert("NAVREE_HARBOR_ENV".into(), "staging".into());
    assert!(
        app.run(&fake, &["harbor:secrets", "set", "APP_KEY", "--generate"])
            .is_err()
    );
    app.vars
        .lock()
        .unwrap()
        .insert("NAVREE_HARBOR_KEY".into(), key.trim().into());
    app.run(&fake, &["harbor:secrets", "set", "APP_KEY", "--generate"])
        .unwrap();
    let text = std::fs::read_to_string(app.path().join("harbor.staging.toml")).unwrap();
    assert!(text.contains("APP_KEY = \"harbor:v1:"), "{text}");
    assert!(!app.path().join("harbor.toml").exists());
}

const PROVISION: &str = r#"
app = "crm-api"
domain = "crm.example.com"

[servers.app-1]
host = "203.0.113.10"
roles = ["web", "worker", "scheduler", "postgres", "redis"]
"#;

/// Answers provisioning scripts: the facts, and the host key ssh learns.
fn provisioning_server() -> FakeExecutor {
    FakeExecutor::new(|cmd| {
        if cmd.program != "ssh" {
            return Output::ok("");
        }
        if let Some(path) = known_hosts(cmd)
            && std::fs::read_to_string(&path)
                .unwrap_or_default()
                .is_empty()
        {
            std::fs::write(
                &path,
                "harbor-app-1 ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILearnedKey\n",
            )
            .unwrap();
        }
        if step_of(cmd).starts_with("facts") {
            Output::ok("id=ubuntu\nversion=24.04\narch=x86_64\nmemory_mb=1987\n")
        } else {
            Output::ok("")
        }
    })
}

#[test]
fn provisioning_pins_the_host_key_and_stores_the_urls() {
    let app = App::new();
    let fake = FakeExecutor::default();
    app.run(&fake, &["harbor:init"]).unwrap();
    app.harbor(PROVISION);
    let server = provisioning_server();
    app.run(&server, &["harbor:provision"]).unwrap();
    let output = app.recorder.text();
    let tail = output
        .split_once("Provisioning")
        .map(|(_, rest)| format!("Provisioning{rest}"))
        .unwrap();
    insta::assert_snapshot!(tail, @r"
    Provisioning app-1 (203.0.113.10, Ubuntu 24.04, x86_64)
      base      updates, unattended upgrades, chrony, 2 GB swap, UTC
      users     navree, with 1 SSH key
      ssh       no passwords, root by key only, host key recorded
      firewall  ufw: 22/tcp, 80/tcp, 443/tcp, 443/udp; fail2ban
      postgres  PostgreSQL 17, database crm_api; URL saved to [secrets] as APP_DATABASE__URL
      redis     Redis on 127.0.0.1, 256 MB; URL saved to [secrets] as REDIS_URL
      caddy     Caddy 2, site crm.example.com
      systemd   crm-api-web@blue, @green, crm-api-worker@1, crm-api-scheduler, crm-api-backup.timer
    Provisioned app-1 in 0s. Run `navree deploy` next.
    ");
    // Every script ran as root, over the connection that learned the key.
    let steps: Vec<String> = server.calls().iter().map(step_of).collect();
    assert_eq!(
        steps,
        [
            "facts on app-1",
            "base on app-1",
            "users on app-1",
            "ssh on app-1",
            "firewall on app-1",
            "postgres on app-1",
            "redis on app-1",
            "caddy on app-1",
            "systemd on app-1",
        ]
    );
    assert!(
        server
            .calls()
            .iter()
            .all(|c| c.args.contains(&"root@203.0.113.10".to_owned()))
    );
    let project = Project::load(app.path(), Environment::production()).unwrap();
    assert_eq!(
        project.config.servers[0].host_key.as_deref(),
        Some("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILearnedKey")
    );
    let key = project.key(None).unwrap();
    let db = project.reveal(&key, "APP_DATABASE__URL").unwrap();
    assert!(
        db.starts_with("postgres://crm_api:") && db.ends_with("@127.0.0.1:5432/crm_api"),
        "{db}"
    );
    let redis = project.reveal(&key, "REDIS_URL").unwrap();
    assert!(
        redis.starts_with("redis://:") && redis.ends_with("@127.0.0.1:6379/0"),
        "{redis}"
    );
    // The generated password is in the postgres script, not in the output.
    let password = db
        .split(':')
        .nth(2)
        .unwrap()
        .split('@')
        .next()
        .unwrap()
        .to_owned();
    assert!(server.calls()[5].stdin_text().contains(&password));
    assert!(!output.contains(&password));

    // Again: the host key is checked, the passwords and URLs stay.
    let again = provisioning_server();
    app.run(&again, &["harbor:provision", "app-1"]).unwrap();
    assert!(
        again.calls()[0]
            .args
            .contains(&"StrictHostKeyChecking=yes".to_owned())
    );
    let reread = Project::load(app.path(), Environment::production()).unwrap();
    assert_eq!(reread.reveal(&key, "APP_DATABASE__URL").unwrap(), db);
    let output = app.recorder.text();
    assert!(
        output.contains("URL in [secrets] as APP_DATABASE__URL"),
        "{output}"
    );
    assert!(output.contains("host key checked"), "{output}");
}

#[test]
fn provisioning_a_debian_server_is_refused() {
    let app = App::new();
    app.harbor(
        PROVISION
            .replace(
                "roles = [\"web\", \"worker\", \"scheduler\", \"postgres\", \"redis\"]",
                "roles = [\"worker\"]",
            )
            .as_str(),
    );
    let server = FakeExecutor::new(|_| Output::ok("id=debian\nversion=12\narch=x86_64\n"));
    let error = app.run(&server, &["harbor:provision"]).unwrap_err();
    assert!(
        error.to_string().contains("Ubuntu 22.04 or 24.04"),
        "{error}"
    );
    assert_eq!(server.calls().len(), 1);
}

const DEPLOY: &str = r#"
app = "crm-api"
domain = "crm.example.com"

[processes]
worker = { command = "queue:work", instances = 2 }

[servers.app-1]
host = "203.0.113.10"
roles = ["web", "worker", "scheduler", "postgres", "redis"]
host_key = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIPinned"

[env]
APP_LOG__LEVEL = "info"
"#;

/// A server with one live release (blue), and a local toolchain whose
/// `cargo build` writes the binary.
fn deploy_world(app: &App, health: &'static str) -> FakeExecutor {
    let target = app.path().join("target");
    let metadata = serde_json::json!({
        "workspace_root": app.path(),
        "target_directory": target,
        "packages": [{"name": "crm-api", "source": null, "manifest_path": app.path().join("Cargo.toml")}],
    })
    .to_string();
    FakeExecutor::new(move |cmd| match cmd.program.as_str() {
        "git" if cmd.args[0] == "rev-parse" => Output::ok("3f2a9c1d0e5b7a\n"),
        "git" => Output::ok("feat: stage probabilities\n"),
        "getconf" => Output::ok("glibc 2.35\n"),
        "cargo" if cmd.args[0] == "metadata" => Output::ok(metadata.clone()),
        "cargo" => {
            let dir = target.join("x86_64-unknown-linux-gnu").join("release");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("crm-api"), b"\x7fELF").unwrap();
            Output::ok("")
        }
        "ssh" => {
            let step = step_of(cmd);
            if step.starts_with("inspect") {
                Output::ok(
                    "arch=x86_64\nglibc=2.39\nprovisioned=yes\nlive=blue\ncurrent=20261025-163003-9b17e44\nblue=20261025-163003-9b17e44\nrelease=20261025-163003-9b17e44\t9b17e44\tfix: deal board ordering\n",
                )
            } else if step.starts_with("health") {
                Output::ok(format!("status={health}\nmillis=400\n"))
            } else if step.starts_with("migrate") {
                Output::ok("Migrated: deals/m1\nMigrated: deals/m2\n")
            } else if step.starts_with("logs") {
                Output::ok("error: database connection refused\n")
            } else {
                Output::ok("")
            }
        }
        _ => Output::failed(127, "not found"),
    })
}

#[test]
fn a_deploy_over_ssh() {
    let app = App::new();
    app.run(&FakeExecutor::default(), &["harbor:init"]).unwrap();
    app.harbor(DEPLOY);
    app.run(
        &FakeExecutor::default(),
        &["harbor:secrets", "set", "APP_KEY", "--generate"],
    )
    .unwrap();
    let world = deploy_world(&app, "200");
    app.run(&world, &["deploy"]).unwrap();
    let output = app.recorder.text();
    let tail = output
        .split_once("Deploying")
        .map(|(_, rest)| format!("Deploying{rest}"))
        .unwrap();
    // The archive's size depends on compression; mask it.
    let tail = tail
        .lines()
        .map(|l| {
            if l.trim_start().starts_with("upload") {
                l.split(", ").next().unwrap().to_owned() + ", <size> (0s)"
            } else {
                l.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!(tail, @r"
    Deploying crm-api to production (1 server)
      build     crm-api for x86_64-unknown-linux-gnu, natively (0s)
      upload    app-1  releases/20261026-091512-3f2a9c1, <size> (0s)
      env       app-1  5 settings, 1 secret
      migrate   app-1  2 migrations ran (0s)
      start     app-1  green on 127.0.0.1:8082
      health    app-1  GET /health/ready 200 after 0.4 s
      switch    app-1  Caddy -> green, current -> 20261026-091512-3f2a9c1
      drain     app-1  blue stopped after its requests finished
      restart   app-1  worker x2, scheduler
      prune     app-1  nothing to remove
    Released 20261026-091512-3f2a9c1 in 0s.
    ");
    let calls = world.calls();
    let ssh: Vec<&Cmd> = calls.iter().filter(|c| c.program == "ssh").collect();
    let remote: Vec<String> = ssh
        .iter()
        .map(|c| {
            let step = step_of(c);
            if step.is_empty() {
                c.args.last().unwrap().clone()
            } else {
                step
            }
        })
        .collect();
    insta::assert_snapshot!(remote.join("\n"), @r"
    inspect on app-1
    cat > '/srv/crm-api/releases/20261026-091512-3f2a9c1.tar.gz'
    unpack 20261026-091512-3f2a9c1 on app-1
    env on app-1
    migrate on app-1
    start web green on app-1
    health of 127.0.0.1:8082 on app-1
    switch to green on app-1
    drain on app-1
    restart on app-1
    prune on app-1
    ");
    // Every connection is the deploy user with the pinned key.
    for cmd in &ssh {
        assert!(cmd.args.contains(&"navree@203.0.113.10".to_owned()));
        assert!(cmd.args.contains(&"StrictHostKeyChecking=yes".to_owned()));
    }
    // The build ran natively for the servers' target.
    let build = calls
        .iter()
        .find(|c| c.program == "cargo" && c.args[0] == "build")
        .unwrap();
    assert_eq!(
        build.to_string(),
        "cargo build --release --package crm-api --target x86_64-unknown-linux-gnu"
    );
    // The upload streamed a gzip archive.
    let upload = ssh[1];
    assert_eq!(upload.stdin_text().as_bytes().first(), Some(&0x1f));
    // The env file travels base64-encoded; the APP_KEY is in it, not in the
    // script's text.
    let env_script = ssh[3].stdin_text();
    assert!(env_script.contains("base64 -d > /etc/crm-api/env.new <<'HARBOR_EOF'"));
    assert!(!env_script.contains("APP_KEY"), "{env_script}");
    insta::assert_snapshot!("deploy_switch_script", ssh[7].stdin_text());
    insta::assert_snapshot!("deploy_start_script", ssh[5].stdin_text());
    insta::assert_snapshot!("deploy_prune_script", ssh[10].stdin_text());
    insta::assert_snapshot!("deploy_inspect_script", ssh[0].stdin_text());
    insta::assert_snapshot!("deploy_unpack_script", ssh[2].stdin_text());
}

#[test]
fn a_failed_health_check_over_ssh_stops_the_new_slot() {
    let app = App::new();
    app.harbor(DEPLOY);
    let world = deploy_world(&app, "503");
    let error = app.run(&world, &["deploy", "--no-migrate"]).unwrap_err();
    let message = error.to_string();
    assert!(message.contains("did not answer 200"), "{message}");
    assert!(message.contains("database connection refused"), "{message}");
    let steps: Vec<String> = world
        .calls()
        .iter()
        .filter(|c| c.program == "ssh")
        .map(step_of)
        .filter(|s| !s.is_empty())
        .collect();
    assert_eq!(
        steps,
        [
            "inspect on app-1",
            "unpack 20261026-091512-3f2a9c1 on app-1",
            "env on app-1",
            "start web green on app-1",
            "health of 127.0.0.1:8082 on app-1",
            "logs of crm-api-web@green on app-1",
            "stop on app-1",
        ]
    );
}

#[test]
fn deploys_need_pinned_host_keys() {
    let app = App::new();
    app.harbor(&DEPLOY.replace(
        "host_key = \"ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIPinned\"\n",
        "",
    ));
    let error = app.run(&FakeExecutor::default(), &["deploy"]).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("servers.app-1.host_key is not set: run `navree harbor:provision app-1`"),
        "{error}"
    );
}

#[test]
fn rollback_and_releases_over_ssh() {
    let app = App::new();
    app.harbor(DEPLOY);
    let world = FakeExecutor::new(|cmd| {
        let step = step_of(cmd);
        if step.starts_with("inspect") {
            Output::ok(
                "arch=x86_64\nprovisioned=yes\nlive=green\ncurrent=20261026-091512-3f2a9c1\nblue=20261025-163003-9b17e44\ngreen=20261026-091512-3f2a9c1\nrelease=20261024-110741-52ad0f3\t52ad0f3\tchore: dependencies\nrelease=20261025-163003-9b17e44\t9b17e44\tfix: deal board ordering\nrelease=20261026-091512-3f2a9c1\t3f2a9c1\tfeat: stage probabilities\n",
            )
        } else if step.starts_with("health") {
            Output::ok("status=200\nmillis=300\n")
        } else {
            Output::ok("")
        }
    });
    app.run(&world, &["deploy:releases"]).unwrap();
    app.run(&world, &["deploy:rollback"]).unwrap();
    insta::assert_snapshot!(app.recorder.text(), @r"
    20261026-091512-3f2a9c1  current  feat: stage probabilities
    20261025-163003-9b17e44           fix: deal board ordering
    20261024-110741-52ad0f3           chore: dependencies
    Rolling back crm-api on app-1 to 20261025-163003-9b17e44
      start     app-1  blue on 127.0.0.1:8081
      health    app-1  GET /health/ready 200 after 0.3 s
      switch    app-1  Caddy -> blue, current -> 20261025-163003-9b17e44
      drain     app-1  green stopped after its requests finished
      restart   app-1  worker x2, scheduler
    Rolled back in 0s.
    ");
    assert!(!world.calls().iter().any(|c| c.program != "ssh"));
}

#[test]
fn working_with_servers() {
    let app = App::new();
    app.harbor(DEPLOY);
    let world = FakeExecutor::new(|cmd| {
        let step = step_of(cmd);
        if step.starts_with("status") {
            Output::ok(
                "live=blue\ncurrent=r2\nreleases=2\nunit=crm-api-web@blue active\nunit=caddy active\nunit=crm-api-worker@1 active\nunit=crm-api-worker@2 failed\nunit=crm-api-scheduler active\nunit=postgresql active\nunit=redis-server active\ndisk=18% 83886080\nmemory=4000000 2360000\nload=0.12\n",
            )
        } else if step.starts_with("backup") {
            Output::ok("file=/var/backups/postgresql/crm-api/crm_api-20261026-091512.dump\n")
        } else if cmd.args.last().is_some_and(|a| a.starts_with("journalctl")) {
            Output::ok("2026-10-26T09:15:12+0000 app-1 crm-api[1]: serving\n")
        } else if cmd.args.last().is_some_and(|a| a.starts_with("cat '")) {
            Output::ok("DUMP")
        } else {
            Output::ok("")
        }
    });
    app.run(&world, &["harbor:status"]).unwrap();
    app.run(&world, &["harbor:logs", "--process", "web", "--lines", "5"])
        .unwrap();
    app.run(&world, &["harbor:run", "queue:retry", "all"])
        .unwrap();
    app.run(&world, &["harbor:ssh"]).unwrap();
    let cwd = app.path().to_path_buf();
    app.run(&world, &["harbor:backup", "--download"]).unwrap();
    let lines = app.recorder.lines();
    let (downloaded, shown): (Vec<String>, Vec<String>) = lines
        .into_iter()
        .partition(|l| l.starts_with("Downloaded to"));
    assert_eq!(downloaded.len(), 1);
    let output = shown.join("\n");
    insta::assert_snapshot!(output, @r"
    app-1  203.0.113.10  web, worker, scheduler, postgres, redis
      release    r2 (current), 1 older
      web        blue, running on 127.0.0.1:8081
      caddy      running
      worker     1 of 2 running
      scheduler  running
      postgres   running
      redis      running
      system     disk 18% of 85 GB, memory 41% of 4 GB, load 0.12
    2026-10-26T09:15:12+0000 app-1 crm-api[1]: serving
    Backed up to /var/backups/postgresql/crm-api/crm_api-20261026-091512.dump on app-1.
    ");
    let lines = world.command_lines();
    assert!(
        lines.iter().any(|l| l.ends_with(
            "journalctl --unit 'crm-api-web@*' --lines 5 --no-pager --output short-iso\""
        )),
        "{lines:#?}"
    );
    let run_line = lines.iter().find(|l| l.contains("exec ./crm-api")).unwrap();
    assert!(run_line.starts_with("ssh -t "), "{run_line}");
    assert!(
        run_line.contains("exec ./crm-api 'queue:retry' 'all'"),
        "{run_line}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("ssh -t ") && l.ends_with("navree@203.0.113.10"))
    );
    let dump = cwd.join("crm_api-20261026-091512.dump");
    assert_eq!(std::fs::read_to_string(dump).unwrap(), "DUMP");
}

#[test]
fn the_github_workflow() {
    let app = App::new();
    std::fs::create_dir(app.path().join(".git")).unwrap();
    app.harbor(DEPLOY);
    app.run(&FakeExecutor::default(), &["harbor:ci", "github"])
        .unwrap();
    let workflow =
        std::fs::read_to_string(app.path().join(".github/workflows/deploy.yml")).unwrap();
    assert!(workflow.contains("- run: navree deploy\n"), "{workflow}");
    assert!(
        app.recorder
            .text()
            .starts_with("  created  .github/workflows/deploy.yml")
    );
    assert!(
        app.run(&FakeExecutor::default(), &["harbor:ci", "github"])
            .is_err()
    );
}

#[test]
fn container_images() {
    let app = App::new();
    let root = app.path().to_path_buf();
    let metadata = serde_json::json!({ "workspace_root": root }).to_string();
    let docker = FakeExecutor::new(move |cmd| match cmd.program.as_str() {
        "cargo" => Output::ok(metadata.clone()),
        "git" => Output::ok("3f2a9c1d0e5b7a\n"),
        "docker" => Output::ok(""),
        _ => Output::failed(127, ""),
    });
    // Without harbor.toml: the package and the defaults.
    app.run(&docker, &["build:image"]).unwrap();
    assert_eq!(
        app.recorder.lines().last().unwrap(),
        "Built crm-api:3f2a9c1 (linux/amd64) with docker."
    );
    app.run(
        &docker,
        &[
            "build:image",
            "--tag",
            "registry.example.com/crm-api:1.4.0",
            "--push",
            "--platform",
            "linux/arm64",
        ],
    )
    .unwrap();
    assert_eq!(
        app.recorder.lines().last().unwrap(),
        "Built registry.example.com/crm-api:1.4.0 (linux/arm64) with docker, and pushed it."
    );
    assert!(
        docker
            .command_lines()
            .contains(&"docker push registry.example.com/crm-api:1.4.0".to_owned())
    );
    // --dockerfile writes the files into the app; later builds use them.
    app.run(&docker, &["build:image", "--dockerfile"]).unwrap();
    assert!(app.path().join("Dockerfile").is_file());
    assert!(app.path().join(".dockerignore").is_file());
    let dockerfile = std::fs::read_to_string(app.path().join("Dockerfile")).unwrap();
    assert!(
        dockerfile.contains("COPY config /app/config"),
        "{dockerfile}"
    );
    app.run(&docker, &["build:image"]).unwrap();
    let last = docker
        .command_lines()
        .into_iter()
        .rfind(|l| l.starts_with("docker build"))
        .unwrap();
    assert!(
        last.contains(&format!(
            "--file {}",
            navree_harbor::exec::slash_path(&app.path().join("Dockerfile"))
        )),
        "{last}"
    );
}
