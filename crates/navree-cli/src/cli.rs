//! The command line: `navree new`, `navree make:*`, `navree dev`, and every other
//! command forwarded to the app binary.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

pub use crate::kits::Kit;
use crate::make::MakeKind;
pub use crate::new_app::Ui;

/// The default directory of feature modules.
pub const MODULES_DIR: &str = "src/modules";

/// `navree`: create Navree apps, generate code, run the dev loop, and run the
/// app's own commands.
#[derive(Debug, Parser)]
#[command(
    name = "navree",
    version,
    about = "Create Navree apps, generate code and run the dev loop",
    after_help = "Every other command runs in the app binary: `navree <command> [args...]` is \
                  `cargo run --quiet -- <command> [args...]` (migrate, migrate:fresh --seed, db:seed, \
                  queue:work, schedule:run, routes, openapi:export, key:generate, \
                  <module>:<command>, ...). `navree list` shows the app's commands."
)]
pub struct Cli {
    /// What to do.
    #[command(subcommand)]
    pub command: Command,
}

/// A `navree` subcommand.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create a new app in ./NAME.
    New(NewArgs),
    /// Generate a feature module: mod.rs, controller.rs, service.rs, entity.rs, dto.rs, migrations/, tests/.
    #[command(name = "make:module")]
    MakeModule(MakeModuleArgs),
    /// Generate a controller in a module (controllers/NAME.rs).
    #[command(name = "make:controller")]
    MakeController(MakeArgs),
    /// Generate a service in a module (services/NAME.rs).
    #[command(name = "make:service")]
    MakeService(MakeArgs),
    /// Generate a database entity in a module (entities/NAME.rs).
    #[command(name = "make:entity")]
    MakeEntity(MakeArgs),
    /// Generate a migration in a module (create_TABLE_table creates the table).
    #[command(name = "make:migration")]
    MakeMigration(MakeArgs),
    /// Generate a queued job in a module (jobs/NAME.rs).
    #[command(name = "make:job")]
    MakeJob(MakeArgs),
    /// Generate an event in a module (events/NAME.rs).
    #[command(name = "make:event")]
    MakeEvent(MakeArgs),
    /// Generate an event listener in a module (listeners/NAME.rs).
    #[command(name = "make:listener")]
    MakeListener(MakeListenerArgs),
    /// Generate a mail with HTML and text templates in a module (mail/NAME.rs).
    #[command(name = "make:mail")]
    MakeMail(MakeArgs),
    /// Generate a notification in a module (notifications/NAME.rs).
    #[command(name = "make:notification")]
    MakeNotification(MakeArgs),
    /// Generate ability rules in a module (policies/NAME.rs), added to src/abilities.rs.
    #[command(name = "make:policy")]
    MakePolicy(MakeArgs),
    /// Generate a CLI command in a module (commands/NAME.rs).
    #[command(name = "make:command")]
    MakeCommand(MakeArgs),
    /// Generate an AI agent with one tool in a module (agents/NAME.rs).
    #[command(name = "make:agent")]
    MakeAgent(MakeArgs),
    /// Generate a test in a module (tests/NAME.rs).
    #[command(name = "make:test")]
    MakeTest(MakeArgs),
    /// Generate an MCP server with one tool in a module (mcp/NAME.rs).
    #[command(name = "make:mcp")]
    MakeMcp(MakeArgs),
    /// Generate a Helm admin-panel resource for an entity of a module (resources/ENTITY.rs).
    #[command(name = "make:resource")]
    MakeResource(MakeResourceArgs),
    /// Serve the app, rebuilding and restarting it when the code changes.
    Dev(DevArgs),
    /// Check the app: its module graph, then the conformity checks (routes documented, entities classified, workspace isolation, translations, migrations).
    Check(CheckArgs),
    /// Run fmt, clippy, the tests, navree check and the front end's typecheck, tests and build, one gate at a time on this machine.
    Gate(GateArgs),
    /// Run the app's local services (dock.toml) with Docker or Podman.
    #[command(subcommand)]
    Dock(DockCommand),
    /// Add a service to dock.toml: postgres, redis, meilisearch, mailpit, minio or nats.
    #[command(name = "dock:add")]
    DockAdd(DockAddArgs),
    /// Stop a service and remove it from dock.toml.
    #[command(name = "dock:remove")]
    DockRemove(DockRemoveArgs),
    /// Harbor: harbor:init, harbor:provision, harbor:secrets, deploy,
    /// deploy:rollback, deploy:releases, harbor:status, harbor:logs,
    /// harbor:run, harbor:ssh, harbor:backup, harbor:ci and build:image.
    #[command(flatten)]
    Harbor(navree_harbor::HarborCommand),
    /// Start Navigator, the MCP server for your coding agent, on stdin/stdout.
    Mcp(McpArgs),
    /// Install the Navree agent skills into .claude/skills and .agents/skills.
    #[command(name = "skills:install")]
    SkillsInstall(SkillsInstallArgs),
    /// Download Chrome for Testing into Navree's cache, for navree-browser.
    #[command(name = "browser:install")]
    BrowserInstall(BrowserInstallArgs),
    /// A command of the app binary.
    #[command(external_subcommand)]
    Forward(Vec<String>),
}

impl Command {
    /// The kind and arguments of a `make:<kind>` command (not `make:module`),
    /// with the listener's `--event`.
    pub fn make_item(&self) -> Option<(MakeKind, &MakeArgs, Option<&str>)> {
        let (kind, args) = match self {
            Command::MakeController(a) => (MakeKind::Controller, a),
            Command::MakeService(a) => (MakeKind::Service, a),
            Command::MakeEntity(a) => (MakeKind::Entity, a),
            Command::MakeMigration(a) => (MakeKind::Migration, a),
            Command::MakeJob(a) => (MakeKind::Job, a),
            Command::MakeEvent(a) => (MakeKind::Event, a),
            Command::MakeListener(a) => {
                return Some((MakeKind::Listener, &a.item, a.event.as_deref()));
            }
            Command::MakeMail(a) => (MakeKind::Mail, a),
            Command::MakeNotification(a) => (MakeKind::Notification, a),
            Command::MakePolicy(a) => (MakeKind::Policy, a),
            Command::MakeCommand(a) => (MakeKind::Command, a),
            Command::MakeAgent(a) => (MakeKind::Agent, a),
            Command::MakeTest(a) => (MakeKind::Test, a),
            Command::MakeMcp(a) => (MakeKind::Mcp, a),
            _ => return None,
        };
        Some((kind, args, None))
    }
}

/// `navree new <name> [--kit saas|api|micro] [--ui next|none] [--navree-path <path>]`.
#[derive(Debug, Args)]
pub struct NewArgs {
    /// The app (package) name: lower-case letters, digits, `-` and `_`.
    pub name: String,
    /// A starter kit: `saas` (API and Next.js front end), `api` (the API
    /// alone) or `micro` (a microservice); default: the minimal app.
    #[arg(long, value_enum, value_name = "KIT")]
    pub kit: Option<Kit>,
    /// The front end to create next to the API. With a kit, only `saas`
    /// has one, and it is Next.js (`--kit saas --ui next` is the same as
    /// `--kit saas`).
    #[arg(long, value_enum, default_value_t = Ui::None)]
    pub ui: Ui,
    /// A local navree checkout (the repository or its crates/navree) to depend
    /// on; default: the checkout this navree was built from, else the
    /// published crate.
    #[arg(long, value_name = "PATH")]
    pub navree_path: Option<PathBuf>,
}

/// `navree make:module <name> [--crud] [--path src/modules]`.
#[derive(Debug, Args)]
pub struct MakeModuleArgs {
    /// The module name in snake_case, usually plural (`posts`).
    pub name: String,
    /// Generate the five JSON resource routes with access checks, DTOs and tests.
    #[arg(long)]
    pub crud: bool,
    /// The directory holding feature modules, relative to the app.
    #[arg(long, default_value = MODULES_DIR)]
    pub path: String,
}

/// `navree make:<kind> <module> <name> [--path src/modules]`.
#[derive(Debug, Args)]
pub struct MakeArgs {
    /// The module (a directory under --path, created by make:module).
    pub module: String,
    /// The item name in snake_case.
    pub name: String,
    /// The directory holding feature modules, relative to the app.
    #[arg(long, default_value = MODULES_DIR)]
    pub path: String,
}

/// `navree make:resource <module> <Entity> [--path src/modules]`.
#[derive(Debug, Args)]
pub struct MakeResourceArgs {
    /// The module (a directory under --path, created by make:module).
    pub module: String,
    /// The entity, as `Deal` or `deal` (in entity.rs or entities/ENTITY.rs).
    pub entity: String,
    /// The directory holding feature modules, relative to the app.
    #[arg(long, default_value = MODULES_DIR)]
    pub path: String,
}

/// `navree make:listener <module> <name> [--event <event>]`.
#[derive(Debug, Args)]
pub struct MakeListenerArgs {
    /// The module and listener name.
    #[command(flatten)]
    pub item: MakeArgs,
    /// The module's event to handle (events/EVENT.rs); the listener is
    /// subscribed to it.
    #[arg(long, value_name = "EVENT")]
    pub event: Option<String>,
}

/// `navree dev [--debounce-ms N] [--no-web] [-- <serve args>]`.
#[derive(Debug, Args)]
pub struct DevArgs {
    /// How long the files must be quiet before a rebuild, in milliseconds.
    #[arg(long, default_value_t = 300)]
    pub debounce_ms: u64,
    /// Do not start the front end's dev server (web/).
    #[arg(long)]
    pub no_web: bool,
    /// Arguments for the app's `serve` command (after `--`).
    #[arg(last = true)]
    pub serve_args: Vec<String>,
}

/// `navree check [--graph] [--json] [--only <check,...>]`.
#[derive(Debug, Args)]
pub struct CheckArgs {
    /// Print the module graph (every module, provider and command) and stop.
    #[arg(long)]
    pub graph: bool,
    /// Print the conformity report as JSON.
    #[arg(long)]
    pub json: bool,
    /// Only these checks: names or prefixes (`api.docs`, `workspace`).
    #[arg(long, value_delimiter = ',', value_name = "CHECK")]
    pub only: Vec<String>,
}

/// `navree gate [--only <step,...>] [--skip <step,...>] [--keep-going] [--no-lock] [--web <dir>]`.
#[derive(Debug, Args)]
pub struct GateArgs {
    /// Only these steps: fmt, clippy, test, check, web:typecheck, web:test, web:build (`web` = every web step).
    #[arg(long, value_delimiter = ',', value_name = "STEP")]
    pub only: Vec<String>,
    /// Leave these steps out.
    #[arg(long, value_delimiter = ',', value_name = "STEP")]
    pub skip: Vec<String>,
    /// Run every step even after one fails.
    #[arg(long)]
    pub keep_going: bool,
    /// Do not wait for other gates on this machine.
    #[arg(long)]
    pub no_lock: bool,
    /// The front end's directory, relative to the app (default web/).
    #[arg(long, value_name = "DIR")]
    pub web: Option<PathBuf>,
}

/// `navree dock <command>`: the app's local services.
#[derive(Debug, Subcommand, PartialEq, Eq)]
pub enum DockCommand {
    /// Start every service, or the ones named, and wait until each is ready.
    Up {
        /// Services to start (default: all of dock.toml).
        services: Vec<String>,
    },
    /// Stop and remove the containers of every service, or of the ones named.
    Down {
        /// Services to stop (default: all of dock.toml).
        services: Vec<String>,
        /// Also delete their data (the volumes).
        #[arg(long)]
        volumes: bool,
    },
    /// Each service's state, image and address.
    Status,
    /// A service's output.
    Logs {
        /// The service.
        service: String,
        /// Keep printing new output.
        #[arg(long, short)]
        follow: bool,
    },
    /// Print the settings your app gets, as NAME=value lines.
    Env {
        /// Prefix each line with `export` (for `eval` in a shell).
        #[arg(long)]
        export: bool,
    },
    /// Open the service's client: psql, redis-cli, the nats CLI, or a shell.
    Shell {
        /// The service.
        service: String,
    },
}

/// `navree dock:add <service>`.
#[derive(Debug, Args)]
pub struct DockAddArgs {
    /// postgres, redis, meilisearch, mailpit, minio or nats.
    pub service: String,
}

/// `navree mcp [--path <dir>] [--no-build] [--allow-remote-database] [--docs-offline]`.
#[derive(Debug, Args, PartialEq, Eq)]
pub struct McpArgs {
    /// The app to serve, instead of the current directory.
    #[arg(long, value_name = "DIR")]
    pub path: Option<PathBuf>,
    /// Never run `cargo build`; use the last built binary, or answer without one.
    #[arg(long)]
    pub no_build: bool,
    /// Let query_database and describe_schema connect to a database that isn't on this machine.
    #[arg(long)]
    pub allow_remote_database: bool,
    /// Never download docs; use the cache or the docs built into Navigator.
    #[arg(long)]
    pub docs_offline: bool,
}

/// `navree skills:install [--path <dir>]`.
#[derive(Debug, Args, PartialEq, Eq)]
pub struct SkillsInstallArgs {
    /// The project to install into, instead of the current directory.
    #[arg(long, value_name = "DIR")]
    pub path: Option<PathBuf>,
}

/// `navree dock:remove <service> [--volumes]`.
#[derive(Debug, Args)]
pub struct DockRemoveArgs {
    /// The service to remove.
    pub service: String,
    /// Also delete its data (the volume).
    #[arg(long)]
    pub volumes: bool,
}

/// `navree browser:install [--version <version>] [--force]`.
#[derive(Debug, Args)]
pub struct BrowserInstallArgs {
    /// The Chrome for Testing version (default: the one this navree pins).
    #[arg(long = "version", value_name = "VERSION")]
    pub chrome_version: Option<String>,
    /// Download again even if that version is installed.
    #[arg(long)]
    pub force: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Command {
        Cli::try_parse_from(std::iter::once("navree").chain(args.iter().copied()))
            .unwrap_or_else(|e| panic!("{args:?}: {e}"))
            .command
    }

    #[test]
    fn no_arguments_print_the_help() {
        let error = Cli::try_parse_from(["navree"]).unwrap_err();
        assert_eq!(
            error.kind(),
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
        assert!(error.to_string().contains("make:module"), "{error}");
    }

    #[test]
    fn new_with_ui_and_navree_path() {
        let Command::New(args) =
            parse(&["new", "blog", "--ui", "next", "--navree-path", "../navree"])
        else {
            panic!("expected new");
        };
        assert_eq!(args.name, "blog");
        assert_eq!(args.ui, Ui::Next);
        assert_eq!(
            args.navree_path.as_deref(),
            Some(std::path::Path::new("../navree"))
        );
        let Command::New(args) = parse(&["new", "blog", "--ui=none"]) else {
            panic!("expected new");
        };
        assert_eq!(args.ui, Ui::None);
        let Command::New(args) = parse(&["new", "blog"]) else {
            panic!("expected new");
        };
        assert_eq!(args.ui, Ui::None);
        assert_eq!(args.kit, None);
    }

    #[test]
    fn new_with_a_kit() {
        for (flag, kit) in [
            ("saas", Kit::Saas),
            ("api", Kit::Api),
            ("micro", Kit::Micro),
        ] {
            let Command::New(args) = parse(&["new", "acme", "--kit", flag]) else {
                panic!("expected new");
            };
            assert_eq!(args.kit, Some(kit));
        }
        let Command::New(args) = parse(&["new", "acme", "--kit=saas", "--navree-path", ".."])
        else {
            panic!("expected new");
        };
        assert_eq!(args.kit, Some(Kit::Saas));
        assert!(args.navree_path.is_some());
        // The SaaS kit's front end is Next.js; `create_app` refuses a
        // front end for the other kits.
        let Command::New(args) = parse(&["new", "acme", "--kit", "saas", "--ui", "next"]) else {
            panic!("expected new");
        };
        assert_eq!((args.kit, args.ui), (Some(Kit::Saas), Ui::Next));
        let error = Cli::try_parse_from(["navree", "new", "acme", "--kit", "crm"]).unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::InvalidValue);
    }

    #[test]
    fn make_module_flags() {
        let Command::MakeModule(args) = parse(&["make:module", "posts", "--crud"]) else {
            panic!("expected make:module");
        };
        assert_eq!(
            (args.name.as_str(), args.crud, args.path.as_str()),
            ("posts", true, "src/modules")
        );
        let Command::MakeModule(args) = parse(&["make:module", "posts", "--path", "src/features"])
        else {
            panic!("expected make:module");
        };
        assert_eq!((args.crud, args.path.as_str()), (false, "src/features"));
    }

    #[test]
    fn every_make_kind_has_a_command() {
        for kind in MakeKind::ALL {
            let command = format!("make:{}", kind.noun());
            let parsed = parse(&[&command, "posts", "thing"]);
            let (parsed_kind, args, event) =
                parsed.make_item().unwrap_or_else(|| panic!("{command}"));
            assert_eq!(parsed_kind, kind);
            assert_eq!(
                (args.module.as_str(), args.name.as_str()),
                ("posts", "thing")
            );
            assert_eq!(event, None);
        }
        let parsed = parse(&[
            "make:listener",
            "posts",
            "notify",
            "--event",
            "post_published",
        ]);
        assert_eq!(parsed.make_item().unwrap().2, Some("post_published"));
    }

    #[test]
    fn dev_options() {
        let Command::Dev(args) = parse(&["dev"]) else {
            panic!("expected dev");
        };
        assert_eq!(args.debounce_ms, 300);
        assert!(!args.no_web);
        assert!(args.serve_args.is_empty());
        let Command::Dev(args) = parse(&["dev", "--no-web", "--", "--addr", "0.0.0.0:8080"]) else {
            panic!("expected dev");
        };
        assert!(args.no_web);
        assert_eq!(args.serve_args, ["--addr", "0.0.0.0:8080"]);
    }

    fn dock(args: &[&str]) -> DockCommand {
        let mut all = vec!["dock"];
        all.extend(args);
        let Command::Dock(command) = parse(&all) else {
            panic!("{args:?} must be a dock command");
        };
        command
    }

    #[test]
    fn dock_commands() {
        assert_eq!(dock(&["up"]), DockCommand::Up { services: vec![] });
        assert_eq!(
            dock(&["up", "postgres", "nats"]),
            DockCommand::Up {
                services: vec!["postgres".into(), "nats".into()]
            }
        );
        assert_eq!(
            dock(&["down"]),
            DockCommand::Down {
                services: vec![],
                volumes: false
            }
        );
        assert_eq!(
            dock(&["down", "postgres", "--volumes"]),
            DockCommand::Down {
                services: vec!["postgres".into()],
                volumes: true
            }
        );
        assert_eq!(dock(&["status"]), DockCommand::Status);
        assert_eq!(
            dock(&["logs", "postgres", "--follow"]),
            DockCommand::Logs {
                service: "postgres".into(),
                follow: true
            }
        );
        assert_eq!(
            dock(&["logs", "redis", "-f"]),
            DockCommand::Logs {
                service: "redis".into(),
                follow: true
            }
        );
        assert_eq!(dock(&["env"]), DockCommand::Env { export: false });
        assert_eq!(
            dock(&["env", "--export"]),
            DockCommand::Env { export: true }
        );
        assert_eq!(
            dock(&["shell", "postgres"]),
            DockCommand::Shell {
                service: "postgres".into()
            }
        );
        assert!(Cli::try_parse_from(["navree", "dock", "logs"]).is_err());
        assert!(Cli::try_parse_from(["navree", "dock", "restart"]).is_err());
    }

    #[test]
    fn dock_add_and_remove() {
        let Command::DockAdd(args) = parse(&["dock:add", "postgres"]) else {
            panic!("expected dock:add");
        };
        assert_eq!(args.service, "postgres");
        let Command::DockRemove(args) = parse(&["dock:remove", "redis", "--volumes"]) else {
            panic!("expected dock:remove");
        };
        assert_eq!((args.service.as_str(), args.volumes), ("redis", true));
        let Command::DockRemove(args) = parse(&["dock:remove", "redis"]) else {
            panic!("expected dock:remove");
        };
        assert!(!args.volumes);
    }

    #[test]
    fn harbor_commands_are_not_forwarded() {
        for args in [
            &["harbor:init"][..],
            &["harbor:provision", "app-1"],
            &["harbor:secrets", "set", "APP_KEY", "--generate"],
            &["deploy", "--no-migrate"],
            &["deploy:rollback", "--to", "20261025-163003-9b17e44"],
            &["deploy:releases"],
            &["harbor:status"],
            &["harbor:logs", "--follow"],
            &["harbor:run", "migrate:status"],
            &["harbor:ssh"],
            &["harbor:backup", "--download"],
            &["harbor:ci", "github"],
            &["build:image", "--push"],
        ] {
            assert!(
                matches!(parse(args), Command::Harbor(_)),
                "{args:?} must be a Harbor command"
            );
        }
        let error = Cli::try_parse_from(["navree", "--help"]).unwrap_err();
        let help = error.to_string();
        assert!(
            help.contains("harbor:provision") && help.contains("deploy:rollback"),
            "{help}"
        );
    }

    #[test]
    fn check_and_gate_options() {
        let Command::Check(args) = parse(&["check"]) else {
            panic!("expected check");
        };
        assert!(!args.graph && !args.json && args.only.is_empty());
        let Command::Check(args) = parse(&["check", "--graph"]) else {
            panic!("expected check");
        };
        assert!(args.graph);
        let Command::Check(args) = parse(&["check", "--json", "--only", "api.docs,workspace"])
        else {
            panic!("expected check");
        };
        assert!(args.json);
        assert_eq!(args.only, ["api.docs", "workspace"]);

        let Command::Gate(args) = parse(&["gate"]) else {
            panic!("expected gate");
        };
        assert!(args.only.is_empty() && args.skip.is_empty());
        assert!(!args.keep_going && !args.no_lock && args.web.is_none());
        let Command::Gate(args) = parse(&[
            "gate",
            "--skip",
            "web,clippy",
            "--only=fmt",
            "--keep-going",
            "--no-lock",
            "--web",
            "frontend",
        ]) else {
            panic!("expected gate");
        };
        assert_eq!(args.skip, ["web", "clippy"]);
        assert_eq!(args.only, ["fmt"]);
        assert!(args.keep_going && args.no_lock);
        assert_eq!(args.web.as_deref(), Some(std::path::Path::new("frontend")));
    }

    #[test]
    fn mcp_options() {
        let Command::Mcp(args) = parse(&["mcp"]) else {
            panic!("expected mcp");
        };
        assert_eq!(
            args,
            McpArgs {
                path: None,
                no_build: false,
                allow_remote_database: false,
                docs_offline: false
            }
        );
        let Command::Mcp(args) = parse(&[
            "mcp",
            "--path",
            "/home/ada/code/crm",
            "--no-build",
            "--allow-remote-database",
            "--docs-offline",
        ]) else {
            panic!("expected mcp");
        };
        assert_eq!(
            args.path.as_deref(),
            Some(std::path::Path::new("/home/ada/code/crm"))
        );
        assert!(args.no_build && args.allow_remote_database && args.docs_offline);
    }

    #[test]
    fn skills_install_options() {
        let Command::SkillsInstall(args) = parse(&["skills:install"]) else {
            panic!("expected skills:install");
        };
        assert_eq!(args, SkillsInstallArgs { path: None });
        let Command::SkillsInstall(args) = parse(&["skills:install", "--path", "../crm"]) else {
            panic!("expected skills:install");
        };
        assert_eq!(args.path.as_deref(), Some(std::path::Path::new("../crm")));
    }

    #[test]
    fn browser_install_options() {
        let Command::BrowserInstall(args) = parse(&["browser:install"]) else {
            panic!("expected browser:install");
        };
        assert_eq!((args.chrome_version, args.force), (None, false));
        let Command::BrowserInstall(args) =
            parse(&["browser:install", "--version", "141.0.7390.54", "--force"])
        else {
            panic!("expected browser:install");
        };
        assert_eq!(args.chrome_version.as_deref(), Some("141.0.7390.54"));
        assert!(args.force);
    }

    #[test]
    fn other_commands_are_forwarded_with_their_arguments() {
        for (args, expected) in [
            (&["migrate"][..], &["migrate"][..]),
            (&["migrate:fresh", "--seed"], &["migrate:fresh", "--seed"]),
            (&["db:seed", "demo"], &["db:seed", "demo"]),
            (
                &["queue:work", "--queue", "a,b", "--once"],
                &["queue:work", "--queue", "a,b", "--once"],
            ),
            (&["schedule:run"], &["schedule:run"]),
            (&["routes", "--json"], &["routes", "--json"]),
            (
                &["openapi:export", "web/openapi.json"],
                &["openapi:export", "web/openapi.json"],
            ),
            (&["billing:sync-plans", "-v"], &["billing:sync-plans", "-v"]),
        ] {
            let Command::Forward(forwarded) = parse(args) else {
                panic!("{args:?} must be forwarded");
            };
            assert_eq!(forwarded, expected);
        }
    }
}
