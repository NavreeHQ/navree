//! The release state machine against in-memory servers: deploys, failed
//! health checks, rolling deploys over several web servers, rollbacks and
//! pruning.

mod common;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use common::{MULTI, SINGLE, config};
use navree_harbor::HarborConfig;
use navree_harbor::deploy::{
    Artifact, Build, DeployOptions, Deployer, ReleaseMeta, Target, format_releases,
};
use navree_harbor::fake::FakeHost;
use navree_harbor::host::Host;
use navree_harbor::layout::Slot;
use navree_harbor::output::{FixedClock, Recorder};
use navree_harbor::render::{AppInfo, FrontendInfo};

const R1: &str = "20261024-110741-52ad0f3";
const R2: &str = "20261025-163003-9b17e44";
const NEW: &str = "20261026-091512-3f2a9c1";

/// A build that records its target and returns an empty archive.
#[derive(Default)]
struct FakeBuild {
    targets: Mutex<Vec<Target>>,
}

impl Build for FakeBuild {
    fn build(&self, _release: &ReleaseMeta, target: &Target) -> navree_harbor::Result<Artifact> {
        self.targets.lock().unwrap().push(target.clone());
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("release.tar.gz");
        std::fs::write(&archive, vec![0u8; 14_200_000]).unwrap();
        Ok(Artifact {
            archive,
            size: 14_200_000,
            summary: format!("crm-api for {}, in a container", target.triple),
            keep: Some(dir),
        })
    }
}

fn meta() -> ReleaseMeta {
    ReleaseMeta {
        id: NEW.into(),
        commit: "3f2a9c1d0e".into(),
        subject: "feat: stage probabilities".into(),
    }
}

fn secrets() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("APP_KEY".to_owned(), "base64:k".to_owned()),
        ("APP_DATABASE__URL".to_owned(), "postgres://x".to_owned()),
        ("APP_MAIL__TRANSPORT".to_owned(), "smtps://y".to_owned()),
    ])
}

struct Run {
    recorder: Recorder,
    clock: FixedClock,
}

impl Run {
    fn new() -> Self {
        Self {
            recorder: Recorder::new(),
            clock: FixedClock::example(),
        }
    }

    fn deployer<'a>(
        &'a self,
        config: &'a HarborConfig,
        info: &'a AppInfo,
        hosts: &[Arc<FakeHost>],
    ) -> Deployer<'a> {
        Deployer {
            config,
            env_name: "production",
            info,
            hosts: hosts.iter().map(|h| h.clone() as Arc<dyn Host>).collect(),
            progress: &self.recorder,
            clock: &self.clock,
        }
    }
}

#[test]
fn a_first_deploy_starts_blue() {
    let config = config(SINGLE);
    let info = AppInfo::default();
    let host = Arc::new(FakeHost::new(&config, "app-1"));
    let run = Run::new();
    let build = FakeBuild::default();
    let id = run
        .deployer(&config, &info, std::slice::from_ref(&host))
        .deploy(&build, &meta(), &secrets(), &DeployOptions::default())
        .unwrap();
    assert_eq!(id, NEW);
    insta::assert_snapshot!(run.recorder.text(), @r"
    Deploying crm-api to production (1 server)
      build     crm-api for x86_64-unknown-linux-gnu, in a container (0s)
      upload    app-1  releases/20261026-091512-3f2a9c1, 14.2 MB (0s)
      env       app-1  7 settings, 3 secrets
      migrate   app-1  migrate done (0s)
      start     app-1  blue on 127.0.0.1:8081
      health    app-1  GET /health/ready 200 after 0.4 s
      switch    app-1  Caddy -> blue, current -> 20261026-091512-3f2a9c1
      restart   app-1  worker x2, scheduler
      prune     app-1  nothing to remove
    Released 20261026-091512-3f2a9c1 in 0s.
    ");
    let state = host.state();
    assert_eq!(state.live, Some(Slot::Blue));
    assert_eq!(state.current.as_deref(), Some(NEW));
    assert_eq!(state.blue.as_deref(), Some(NEW));
    assert_eq!(
        host.running(),
        [
            "crm-api-scheduler",
            "crm-api-web@blue",
            "crm-api-worker@1",
            "crm-api-worker@2"
        ]
    );
    assert_eq!(host.enabled(), ["crm-api-web@blue"]);
    assert!(host.site().contains("reverse_proxy 127.0.0.1:8081"));
    let env = host.env().unwrap();
    assert_eq!(env.get("APP_LIGHTHOUSE__RELEASE"), Some(NEW));
    assert_eq!(env.get("APP_KEY"), Some("base64:k"));
    assert_eq!(env.get("APP_DATABASE__MIGRATE_ON_BOOT"), Some("false"));
    // Secrets never reach the output.
    assert!(!run.recorder.text().contains("base64:k"));
    assert_eq!(
        build.targets.lock().unwrap()[0].glibc.as_deref(),
        Some("2.39")
    );
}

#[test]
fn the_next_deploy_starts_the_idle_slot_and_drains_the_live_one() {
    let config = config(SINGLE);
    let info = AppInfo::default();
    let host = Arc::new(FakeHost::new(&config, "app-1").with_releases(&[R1, R2], Slot::Blue));
    let run = Run::new();
    run.deployer(&config, &info, std::slice::from_ref(&host))
        .deploy(
            &FakeBuild::default(),
            &meta(),
            &secrets(),
            &DeployOptions::default(),
        )
        .unwrap();
    let text = run.recorder.text();
    assert!(
        text.contains("  start     app-1  green on 127.0.0.1:8082"),
        "{text}"
    );
    assert!(
        text.contains("  drain     app-1  blue stopped after its requests finished"),
        "{text}"
    );
    let state = host.state();
    assert_eq!(state.live, Some(Slot::Green));
    assert_eq!(state.current.as_deref(), Some(NEW));
    assert_eq!(state.blue.as_deref(), Some(R2));
    assert!(!host.running().contains(&"crm-api-web@blue".to_owned()));
    assert_eq!(host.enabled(), ["crm-api-web@green"]);
    insta::assert_snapshot!(host.events().join("\n"), @r"
    inspect
    upload 20261026-091512-3f2a9c1
    env (10 settings)
    run migrate from 20261026-091512-3f2a9c1
    start crm-api-web@green (20261026-091512-3f2a9c1, 127.0.0.1:8082)
    health 8082/health/ready
    switch green 20261026-091512-3f2a9c1
    stop crm-api-web@blue
    restart crm-api-worker@1
    restart crm-api-worker@2
    restart crm-api-scheduler
    prune (0 removed)
    ");
}

#[test]
fn an_unhealthy_release_never_gets_traffic() {
    let config = config(SINGLE);
    let info = AppInfo::default();
    let host = Arc::new(
        FakeHost::new(&config, "app-1")
            .with_releases(&[R1, R2], Slot::Blue)
            .unhealthy(NEW),
    );
    let run = Run::new();
    let error = run
        .deployer(&config, &info, std::slice::from_ref(&host))
        .deploy(
            &FakeBuild::default(),
            &meta(),
            &secrets(),
            &DeployOptions::default(),
        )
        .unwrap_err();
    let message = error.to_string();
    assert!(message.contains("app-1: GET /health/ready on 127.0.0.1:8082 did not answer 200 within 60s (last status: 503)"), "{message}");
    assert!(message.contains("database connection refused"), "{message}");
    let state = host.state();
    assert_eq!(state.live, Some(Slot::Blue));
    assert_eq!(state.current.as_deref(), Some(R2));
    assert_eq!(host.running(), ["crm-api-web@blue"]);
    let events = host.events();
    assert!(
        !events.iter().any(|e| e.starts_with("switch")),
        "{events:?}"
    );
    assert!(
        !events.iter().any(|e| e.starts_with("restart")),
        "{events:?}"
    );
    assert!(
        events.contains(&"stop crm-api-web@green".to_owned()),
        "{events:?}"
    );
}

#[test]
fn a_failed_migration_stops_the_deploy_before_anything_changes() {
    let config = config(SINGLE);
    let info = AppInfo::default();
    let host = Arc::new(
        FakeHost::new(&config, "app-1")
            .with_releases(&[R2], Slot::Blue)
            .failing("migrate"),
    );
    let run = Run::new();
    let error = run
        .deployer(&config, &info, std::slice::from_ref(&host))
        .deploy(
            &FakeBuild::default(),
            &meta(),
            &secrets(),
            &DeployOptions::default(),
        )
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "migrate failed on app-1 (exit code 1):\nmigrate failed"
    );
    assert_eq!(host.state().current.as_deref(), Some(R2));
    assert!(!host.events().iter().any(|e| e.starts_with("start")));
}

#[test]
fn no_migrate_and_maintenance() {
    let config = config(SINGLE);
    let info = AppInfo::default();
    let host = Arc::new(FakeHost::new(&config, "app-1").with_releases(&[R2], Slot::Blue));
    let run = Run::new();
    let deployer = run.deployer(&config, &info, std::slice::from_ref(&host));
    deployer
        .deploy(
            &FakeBuild::default(),
            &meta(),
            &secrets(),
            &DeployOptions {
                no_migrate: true,
                ..DeployOptions::default()
            },
        )
        .unwrap();
    assert!(!host.events().iter().any(|e| e.contains("migrate")));

    let host = Arc::new(FakeHost::new(&config, "app-1").with_releases(&[R2], Slot::Blue));
    let run = Run::new();
    run.deployer(&config, &info, std::slice::from_ref(&host))
        .deploy(
            &FakeBuild::default(),
            &meta(),
            &secrets(),
            &DeployOptions {
                maintenance: true,
                ..DeployOptions::default()
            },
        )
        .unwrap();
    let runs: Vec<String> = host
        .events()
        .into_iter()
        .filter(|e| e.starts_with("run") || e.starts_with("switch"))
        .collect();
    assert_eq!(
        runs,
        [
            format!("run down from {R2}"),
            format!("run migrate from {NEW}"),
            format!("switch green {NEW}"),
            format!("run up from {NEW}"),
        ]
    );
    let text = run.recorder.text();
    assert!(
        text.contains("  down      app-1  maintenance mode on"),
        "{text}"
    );
    assert!(
        text.contains("  up        app-1  maintenance mode off"),
        "{text}"
    );
}

#[test]
fn an_uploaded_release_deploys_without_a_build() {
    let config = config(SINGLE);
    let info = AppInfo::default();
    let host = Arc::new(FakeHost::new(&config, "app-1").with_releases(&[R1, R2], Slot::Green));
    let run = Run::new();
    let build = FakeBuild::default();
    let deployer = run.deployer(&config, &info, std::slice::from_ref(&host));
    let error = deployer
        .deploy(
            &build,
            &meta(),
            &secrets(),
            &DeployOptions {
                release: Some("20200101-000000".into()),
                ..DeployOptions::default()
            },
        )
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("release 20200101-000000 is not on app-1")
    );
    deployer
        .deploy(
            &build,
            &meta(),
            &secrets(),
            &DeployOptions {
                release: Some(R1.into()),
                ..DeployOptions::default()
            },
        )
        .unwrap();
    assert!(build.targets.lock().unwrap().is_empty());
    assert_eq!(host.state().current.as_deref(), Some(R1));
    assert_eq!(host.state().live, Some(Slot::Blue));
}

#[test]
fn unprovisioned_servers_and_mixed_architectures_are_refused() {
    let config = config(SINGLE);
    let info = AppInfo::default();
    let host = Arc::new(FakeHost::new(&config, "app-1").unprovisioned());
    let run = Run::new();
    let error = run
        .deployer(&config, &info, &[host])
        .deploy(
            &FakeBuild::default(),
            &meta(),
            &secrets(),
            &DeployOptions::default(),
        )
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("run `navree harbor:provision app-1` first"),
        "{error}"
    );

    let config = common::config(MULTI);
    let hosts = [
        Arc::new(FakeHost::new(&config, "web-1")),
        Arc::new(FakeHost::new(&config, "web-2").with_arch("aarch64")),
    ];
    let error = run
        .deployer(&config, &info, &hosts)
        .deploy(
            &FakeBuild::default(),
            &meta(),
            &secrets(),
            &DeployOptions::default(),
        )
        .unwrap_err();
    assert!(error.to_string().contains("mix architectures"), "{error}");
}

fn multi_hosts(config: &HarborConfig, unhealthy_on_web_2: bool) -> Vec<Arc<FakeHost>> {
    ["web-1", "web-2", "worker-1"]
        .iter()
        .map(|name| {
            let mut host = FakeHost::new(config, name).with_releases(&[R1, R2], Slot::Blue);
            if unhealthy_on_web_2 && *name == "web-2" {
                host = host.unhealthy(NEW);
            }
            Arc::new(host)
        })
        .collect()
}

#[test]
fn a_rolling_deploy_switches_web_servers_one_at_a_time() {
    let config = config(MULTI);
    let info = AppInfo::default();
    let hosts = multi_hosts(&config, false);
    let run = Run::new();
    run.deployer(&config, &info, &hosts)
        .deploy(
            &FakeBuild::default(),
            &meta(),
            &secrets(),
            &DeployOptions::default(),
        )
        .unwrap();
    insta::assert_snapshot!(run.recorder.text(), @r"
    Deploying crm-api to production (3 servers)
      build     crm-api for x86_64-unknown-linux-gnu, in a container (0s)
      upload    web-1  releases/20261026-091512-3f2a9c1, 14.2 MB (0s)
      upload    web-2  releases/20261026-091512-3f2a9c1, 14.2 MB (0s)
      upload    worker-1  releases/20261026-091512-3f2a9c1, 14.2 MB (0s)
      env       web-1  4 settings, 3 secrets
      env       web-2  4 settings, 3 secrets
      env       worker-1  4 settings, 3 secrets
      migrate   web-1  migrate done (0s)
      start     web-1  green on 127.0.0.1:8082
      health    web-1  GET /health/ready 200 after 0.4 s
      switch    web-1  Caddy -> green, current -> 20261026-091512-3f2a9c1
      drain     web-1  blue stopped after its requests finished
      start     web-2  green on 127.0.0.1:8082
      health    web-2  GET /health/ready 200 after 0.4 s
      switch    web-2  Caddy -> green, current -> 20261026-091512-3f2a9c1
      drain     web-2  blue stopped after its requests finished
      switch    worker-1  current -> 20261026-091512-3f2a9c1
      restart   web-1  scheduler
      restart   worker-1  worker x1
      prune     web-1  nothing to remove
      prune     web-2  nothing to remove
      prune     worker-1  nothing to remove
    Released 20261026-091512-3f2a9c1 in 0s.
    ");
    for host in &hosts {
        assert_eq!(host.state().current.as_deref(), Some(NEW));
    }
    // Migrations ran once, on the first web server.
    let migrations = hosts
        .iter()
        .flat_map(|h| h.events())
        .filter(|e| e.contains("migrate"))
        .count();
    assert_eq!(migrations, 1);
    // The worker relays broadcasts to web-1's internal entry.
    assert_eq!(
        hosts[2].env().unwrap().get("APP_REALTIME__RELAY_URL"),
        Some("http://10.0.0.11:8079")
    );
}

#[test]
fn when_one_web_server_fails_the_others_go_back() {
    let config = config(MULTI);
    let info = AppInfo::default();
    let hosts = multi_hosts(&config, true);
    let run = Run::new();
    let error = run
        .deployer(&config, &info, &hosts)
        .deploy(
            &FakeBuild::default(),
            &meta(),
            &secrets(),
            &DeployOptions::default(),
        )
        .unwrap_err();
    assert!(
        error.to_string().starts_with("web-2: GET /health/ready"),
        "{error}"
    );
    let text = run.recorder.text();
    assert!(
        text.contains(&format!("  revert    web-1  back on {R2}")),
        "{text}"
    );
    for host in &hosts {
        assert_eq!(
            host.state().current.as_deref(),
            Some(R2),
            "{}",
            host.server().name
        );
    }
    assert_eq!(hosts[0].state().live, Some(Slot::Blue));
    assert_eq!(hosts[0].running(), ["crm-api-web@blue"]);
    // Workers never restarted.
    assert!(!hosts[2].events().iter().any(|e| e.starts_with("restart")));
}

#[test]
fn a_rollback_goes_to_the_previous_release() {
    let config = config(SINGLE);
    let info = AppInfo::default();
    let host = Arc::new(FakeHost::new(&config, "app-1").with_releases(&[R1, R2, NEW], Slot::Green));
    let run = Run::new();
    let target = run
        .deployer(&config, &info, std::slice::from_ref(&host))
        .rollback(None)
        .unwrap();
    assert_eq!(target, R2);
    insta::assert_snapshot!(run.recorder.text(), @r"
    Rolling back crm-api on app-1 to 20261025-163003-9b17e44
      start     app-1  blue on 127.0.0.1:8081
      health    app-1  GET /health/ready 200 after 0.4 s
      switch    app-1  Caddy -> blue, current -> 20261025-163003-9b17e44
      drain     app-1  green stopped after its requests finished
      restart   app-1  worker x2, scheduler
    Rolled back in 0s.
    ");
    assert_eq!(host.state().live, Some(Slot::Blue));
    assert!(!host.events().iter().any(|e| e.starts_with("upload")));
}

#[test]
fn a_rollback_to_a_named_release() {
    let config = config(SINGLE);
    let info = AppInfo::default();
    let host = Arc::new(FakeHost::new(&config, "app-1").with_releases(&[R1, R2, NEW], Slot::Green));
    let run = Run::new();
    let deployer = run.deployer(&config, &info, std::slice::from_ref(&host));
    assert_eq!(deployer.rollback(Some(R1)).unwrap(), R1);
    assert_eq!(host.state().current.as_deref(), Some(R1));
    let error = deployer.rollback(Some("20000101-000000")).unwrap_err();
    assert!(error.to_string().contains("is not on app-1"), "{error}");

    let fresh = Arc::new(FakeHost::new(&config, "app-1").with_releases(&[R1], Slot::Blue));
    let error = run
        .deployer(&config, &info, &[fresh])
        .rollback(None)
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("no release before the current one"),
        "{error}"
    );
}

#[test]
fn old_releases_are_pruned_but_never_a_linked_one() {
    let mut config = config(SINGLE);
    config.deploy.keep_releases = 2;
    let info = AppInfo::default();
    let host = Arc::new(FakeHost::new(&config, "app-1").with_releases(&[R1, R2], Slot::Blue));
    let run = Run::new();
    run.deployer(&config, &info, std::slice::from_ref(&host))
        .deploy(
            &FakeBuild::default(),
            &meta(),
            &secrets(),
            &DeployOptions::default(),
        )
        .unwrap();
    let ids: Vec<String> = host.state().releases.iter().map(|r| r.id.clone()).collect();
    // R1 is beyond the two newest and no link points at it; R2 stays as
    // the blue slot's release.
    assert_eq!(ids, [R2, NEW]);
    assert!(
        run.recorder
            .text()
            .contains("  prune     app-1  1 old release removed")
    );
}

#[test]
fn the_releases_listing_marks_the_current_one() {
    let config = config(SINGLE);
    let info = AppInfo::default();
    let host = Arc::new(FakeHost::new(&config, "app-1").with_releases(&[R1, R2], Slot::Blue));
    let run = Run::new();
    let releases = run.deployer(&config, &info, &[host]).releases().unwrap();
    assert_eq!(
        format_releases(&releases),
        [
            format!("{R2}  current  subject of {R2}"),
            format!("{R1}           subject of {R1}"),
        ]
    );
}

#[test]
fn a_front_end_switches_with_its_api() {
    let text = format!("{SINGLE}\n[frontend]\npath = \"web\"\n");
    let config = config(&text);
    let info = AppInfo {
        h2c: false,
        frontend: Some(FrontendInfo {
            path: "web".into(),
            manager: "npm".into(),
            build: "npm ci && npm run build".into(),
            command: "npm start".into(),
            health_check: "/".into(),
            ports: [3001, 3002],
            node_version: 24,
        }),
    };
    let host = Arc::new(FakeHost::new(&config, "app-1").with_releases(&[R2], Slot::Blue));
    let run = Run::new();
    run.deployer(&config, &info, std::slice::from_ref(&host))
        .deploy(
            &FakeBuild::default(),
            &meta(),
            &secrets(),
            &DeployOptions::default(),
        )
        .unwrap();
    let text = run.recorder.text();
    assert!(
        text.contains("  start     app-1  front end green on 127.0.0.1:3002"),
        "{text}"
    );
    assert!(
        text.contains("  health    app-1  GET / 200 after 0.4 s"),
        "{text}"
    );
    let events = host.events();
    assert!(
        events.contains(&format!("start crm-api-frontend@green ({NEW}, 3002)")),
        "{events:?}"
    );
    assert!(
        events.contains(&"stop crm-api-frontend@blue".to_owned()),
        "{events:?}"
    );
    assert!(host.site().contains("reverse_proxy 127.0.0.1:3002"));
    assert_eq!(
        host.enabled(),
        ["crm-api-frontend@green", "crm-api-web@green"]
    );
}
