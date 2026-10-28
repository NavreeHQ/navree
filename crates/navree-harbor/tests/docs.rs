//! The Harbor docs page (`apps/website/content/docs/tools/harbor.mdx`) and
//! the skill (`skills/navree-harbor/SKILL.md`) describe this crate: every
//! name they use exists, every default they state holds, and every complete
//! `harbor.toml` they show is valid.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::Parser;
use navree_harbor::cli::{ENV_VAR, HarborCommand};
use navree_harbor::config::BuildMethod;
use navree_harbor::exec::{Cmd, Executor, FakeExecutor, Output, SystemExecutor};
use navree_harbor::fake::FakeHost;
use navree_harbor::host::{Host, SshHost};
use navree_harbor::layout::{Layout, Slot};
use navree_harbor::project::KEY_VAR;
use navree_harbor::provision::{DATABASE_URL, REDIS_URL};
use navree_harbor::secrets::{KEY_PREFIX, SECRET_PREFIX};
use navree_harbor::ssh::SSH_KEY_VAR;
use navree_harbor::{Environment, HarborConfig, HarborError, Project, Role, SecretKey};

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The docs page, when this crate sits in the Navree repository.
fn page() -> Option<String> {
    std::fs::read_to_string(repository().join("apps/website/content/docs/tools/harbor.mdx")).ok()
}

/// The skill, when this crate sits in the Navree repository.
fn skill() -> Option<String> {
    std::fs::read_to_string(repository().join("skills/navree-harbor/SKILL.md")).ok()
}

#[derive(Debug, Parser)]
struct TestCli {
    #[command(subcommand)]
    command: HarborCommand,
}

/// Every Harbor command line in the `sh` blocks of `text`: `navree` and its
/// arguments, without trailing comments, with `[optional]` brackets
/// dropped and `<placeholders>` filled in.
fn harbor_command_lines(text: &str) -> Vec<Vec<String>> {
    let mut found = Vec::new();
    for block in blocks(text, "```sh") {
        for line in block.lines() {
            let Some(start) = line.find("navree ") else {
                continue;
            };
            let command = &line[start..];
            let command = command.split(" #").next().unwrap_or(command);
            let words: Vec<String> = command
                .split_whitespace()
                .map(|w| w.trim_matches(|c| c == '[' || c == ']'))
                .filter(|w| !w.is_empty())
                .map(|w| {
                    if w.starts_with('<') && w.ends_with('>') {
                        "x".to_owned()
                    } else {
                        w.to_owned()
                    }
                })
                .collect();
            let harbor = words.get(1).is_some_and(|w| {
                w.starts_with("harbor:") || w.starts_with("deploy") || w == "build:image"
            });
            if harbor {
                found.push(words);
            }
        }
    }
    found
}

/// The contents of every fenced block whose opening line contains `opening`.
fn blocks(text: &str, opening: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        // Fences may be indented (inside a list item).
        let fence = line.trim_start().starts_with("```");
        match &mut current {
            None if fence && line.contains(opening) => {
                current = Some(String::new());
            }
            Some(block) if fence => {
                found.push(std::mem::take(block));
                current = None;
            }
            Some(block) => {
                block.push_str(line);
                block.push('\n');
            }
            None => {}
        }
    }
    found
}

#[test]
fn files_variables_and_formats() {
    assert_eq!(KEY_VAR, "NAVREE_HARBOR_KEY");
    assert_eq!(SSH_KEY_VAR, "NAVREE_HARBOR_SSH_KEY");
    assert_eq!(ENV_VAR, "NAVREE_HARBOR_ENV");
    assert_eq!(DATABASE_URL, "APP_DATABASE__URL");
    assert_eq!(REDIS_URL, "REDIS_URL");
    assert_eq!(SECRET_PREFIX, "harbor:v1:");
    assert_eq!(KEY_PREFIX, "base64:");
    let production = Environment::production();
    assert_eq!(production.config_file(), "harbor.toml");
    assert_eq!(production.key_file(), "harbor.key");
    let staging = Environment::named("staging").unwrap();
    assert_eq!(staging.config_file(), "harbor.staging.toml");
    assert_eq!(staging.key_file(), "harbor.staging.key");
    assert_eq!(staging.flag(), " --env staging");
    let key = SecretKey::generate();
    assert!(key.encode().starts_with("base64:"));
    assert!(key.encrypt("APP_KEY", "x").starts_with("harbor:v1:"));
}

/// The defaults of the reference table.
#[test]
fn the_defaults_of_the_keys_table() {
    let config = HarborConfig::parse(
        "app = \"crm-api\"\ndomain = \"crm.example.com\"\n\n[frontend]\npath = \"web\"\n\n[servers.app-1]\nhost = \"203.0.113.10\"\n",
        Path::new("harbor.toml"),
    )
    .unwrap();
    assert!(config.validate().is_empty(), "{:?}", config.validate());
    assert!(config.aliases.is_empty());
    assert_eq!(config.api_domain, None);
    let frontend = config.frontend.as_ref().unwrap();
    assert_eq!(frontend.health_check, "/");
    assert_eq!(frontend.ports, [3001, 3002]);
    assert_eq!(frontend.node_version, 24);
    assert_eq!(config.build.method, BuildMethod::Auto);
    assert!(config.build.features.is_empty());
    assert_eq!(config.build.target, None);
    assert_eq!(config.build.image, None);
    assert_eq!(config.deploy_path(), "/srv/crm-api");
    assert_eq!(config.deploy.keep_releases, 5);
    assert_eq!(config.deploy.health_check, "/health/ready");
    assert_eq!(config.deploy.health_timeout_secs, 60);
    assert!(config.deploy.migrate);
    assert_eq!(config.deploy.include, ["config", "resources"]);
    assert_eq!(config.deploy.shared, ["storage"]);
    assert_eq!(config.deploy.ports, [8081, 8082]);
    assert_eq!(config.deploy.internal_port, 8079);
    assert!(config.deploy.trusted_proxies.is_empty());
    assert_eq!(config.processes.web.command, "serve");
    assert_eq!(config.processes.worker.command, "queue:work");
    assert_eq!(config.processes.worker.instances, 1);
    assert_eq!(config.processes.scheduler.command, "schedule:work");
    assert_eq!(
        config.servers[0].roles,
        [Role::Web, Role::Worker, Role::Scheduler]
    );
    assert_eq!(config.ssh.user, "navree");
    assert_eq!(config.ssh.provision_user, "root");
    assert_eq!(config.ssh.port, 22);
    assert_eq!(config.ssh.key, None);
    assert_eq!(config.ssh.authorized_keys, None);
    assert!(config.web.metrics_allow.is_empty());
    assert!(!config.web.proxy_protocol);
    assert!(!config.web.custom_domains.enabled);
    assert_eq!(config.web.custom_domains.ask_path, "/_domains/allow");
    assert_eq!(
        config.web.custom_domains.to,
        navree_harbor::config::CustomDomainsTo::App
    );
    assert_eq!(config.postgres.version, 17);
    assert_eq!(config.database(), "crm_api");
    assert_eq!(config.postgres.backup_days, 7);
    assert_eq!(config.redis.maxmemory_mb, 256);

    // The names the page shows for crm-api.
    let layout = Layout::new(&config);
    assert_eq!(layout.web_unit(Slot::Blue), "crm-api-web@blue");
    assert_eq!(layout.frontend_unit(Slot::Green), "crm-api-frontend@green");
    assert_eq!(layout.worker_unit(2), "crm-api-worker@2");
    assert_eq!(layout.scheduler_unit(), "crm-api-scheduler");
    assert_eq!(layout.backup_unit(), "crm-api-backup");
    assert_eq!(layout.current(), "/srv/crm-api/current");
    assert_eq!(layout.env_file(), "/etc/crm-api/env");
}

/// Unknown keys are errors.
#[test]
fn typos_are_errors() {
    let error = HarborConfig::parse(
        "app = \"crm-api\"\n[deploy]\nkeep_relases = 3\n",
        Path::new("harbor.toml"),
    )
    .unwrap_err();
    assert!(error.to_string().contains("keep_relases"), "{error}");
    // `redis.version` is not a key: Redis comes from Ubuntu's packages.
    assert!(HarborConfig::parse("[redis]\nversion = 7\n", Path::new("harbor.toml")).is_err());
}

/// Every complete `harbor.toml` on the page (one with servers) is valid.
#[test]
fn the_page_examples_are_valid() {
    let Some(page) = page() else {
        eprintln!("SKIPPED: the docs page is not next to this crate");
        return;
    };
    let examples: Vec<String> = blocks(&page, "title=\"harbor.toml\"")
        .into_iter()
        .filter(|b| b.contains("[servers."))
        .collect();
    assert_eq!(examples.len(), 3, "the page's harbor.toml examples");
    for text in examples {
        // The page abbreviates the encrypted values and the host key.
        let text = text
            .lines()
            .filter(|l| !l.contains('…'))
            .collect::<Vec<_>>()
            .join("\n");
        let config = common::config(&text);
        assert_eq!(config.app, "crm-api");
    }
}

/// The library example: `Project::load` and `servers_with`.
#[test]
fn the_library_example() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("harbor.toml"), common::MULTI).unwrap();
    let project = Project::load(dir.path(), Environment::production()).unwrap();
    let web: Vec<&str> = project
        .config
        .servers_with(Role::Web)
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(web, ["web-1", "web-2"]);
    assert_eq!(project.config.domain.as_deref(), Some("crm.example.com"));
    let missing = Project::load(&dir.path().join("nope"), Environment::production());
    assert!(matches!(missing, Err(HarborError::NoConfig { .. })));
}

/// Every Harbor command the page and the skill show is one the CLI accepts.
#[test]
fn every_command_shown_parses() {
    let (Some(page), Some(skill)) = (page(), skill()) else {
        eprintln!("SKIPPED: the docs page and the skill are not next to this crate");
        return;
    };
    for (source, text, at_least) in [("the page", &page, 15), ("the skill", &skill, 20)] {
        let lines = harbor_command_lines(text);
        assert!(
            lines.len() >= at_least,
            "{source} shows {} Harbor commands",
            lines.len()
        );
        for words in lines {
            if let Err(error) = TestCli::try_parse_from(&words) {
                panic!("{source}: `{}` does not parse:\n{error}", words.join(" "));
            }
        }
    }
}

/// The skill: its front-matter, its `harbor.toml` template and its Rust
/// example (reproduced here).
#[test]
fn the_skill() {
    let Some(skill) = skill() else {
        eprintln!("SKIPPED: the skill is not next to this crate");
        return;
    };
    assert!(
        skill.starts_with("---\nname: navree-harbor\ndescription: \""),
        "the skill's front-matter"
    );
    let templates: Vec<String> = blocks(&skill, "```toml")
        .into_iter()
        .filter(|b| b.contains("[servers."))
        .collect();
    assert_eq!(templates.len(), 1, "the skill's harbor.toml template");
    let config = common::config(&templates[0]);
    assert_eq!(config.app, "crm-api");
    assert_eq!(config.processes.worker.instances, 2);

    // The example of "Using the library".
    let example = || -> navree_harbor::Result<()> {
        let config = HarborConfig::parse(
            "app = \"crm-api\"\ndomain = \"crm.example.com\"\n\n[servers.app-1]\nhost = \"203.0.113.10\"\n",
            Path::new("harbor.toml"),
        )?;
        assert!(config.validate().is_empty());
        assert_eq!(config.servers_with(Role::Web).count(), 1);

        let fake = FakeExecutor::new(|cmd| match cmd.program.as_str() {
            "ssh" => Output::ok("arch=x86_64\n"),
            _ => Output::failed(127, "not found"),
        });
        fake.run(&Cmd::new("ssh").arg("navree@203.0.113.10").arg("bash -s"))?;
        assert_eq!(fake.calls()[0].program, "ssh");
        Ok(())
    };
    example().unwrap();
    assert!(
        skill.contains(
            "fake.run(&Cmd::new(\"ssh\").arg(\"navree@203.0.113.10\").arg(\"bash -s\"))?;"
        ),
        "the skill's example changed: update this test"
    );
}

/// The traits and implementations the page names.
#[test]
fn executors_and_hosts() {
    let _system: Arc<dyn Executor> = Arc::new(SystemExecutor);
    let _fake: Arc<dyn Executor> = Arc::new(FakeExecutor::default());
    let config = common::config(common::SINGLE);
    let _memory: Arc<dyn Host> = Arc::new(FakeHost::new(&config, "app-1"));
    fn is_host<T: Host>() {}
    is_host::<SshHost>();
}
