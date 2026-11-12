//! The command line: `navree new`, `navree make:*`, `navree dev`, and every other
//! command forwarded to the app binary.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

pub use crate::kits::Kit;
use crate::make::{Companions, MakeKind};
pub use crate::new_app::{Database, Ui};

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
                  model:show, queue:work, schedule:run, routes, openapi:export, key:generate, \
                  config:show, config:check, <module>:<command>, ...). `navree list` shows the app's commands."
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
    /// Generate a bare ORM model in a module (entities/NAME.rs); `make:model` adds the companions.
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
    /// Generate an ORM model in a module (models/NAME.rs), with its migration, factory, seeder, controller and policy on request.
    #[command(name = "make:model")]
    MakeModel(MakeModelArgs),
    /// Generate a model factory in a module (factories/NAME.rs).
    #[command(name = "make:factory")]
    MakeFactory(MakeFactoryArgs),
    /// Generate a seeder in a module (seeders/NAME.rs), registered for db:seed.
    #[command(name = "make:seeder")]
    MakeSeeder(MakeArgs),
    /// Generate a Helm admin-panel resource for a model of a module (resources/MODEL.rs).
    #[command(name = "make:resource")]
    MakeResource(MakeResourceArgs),
    /// Generate a Navree UI page in a module (pages/NAME.rs): a GET route in the app's shell, with its test.
    #[command(name = "make:page")]
    MakePage(MakePageArgs),
    /// Generate a form in a module (forms/NAME.rs): a struct with its rules, and the handlers that draw and read it.
    #[command(name = "make:form")]
    MakeForm(MakeUiArgs),
    /// Generate a data table in a module (tables/NAME.rs): a row with sortable, searchable and filterable columns, and its handler.
    #[command(name = "make:table")]
    MakeTable(MakeUiArgs),
    /// Serve the app, rebuilding and restarting it when the code changes.
    Dev(DevArgs),
    /// Check the app: its module graph, then the conformity checks (routes documented, entities classified, workspace isolation, translations, migrations).
    Check(CheckArgs),
    /// Run fmt, clippy, the tests, navree check, config:check and the front end's typecheck, tests and build, one gate at a time on this machine.
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
    /// Export the OpenAPI document and generate the front end's typed client schema (schema.d.ts).
    #[command(name = "openapi:client")]
    OpenapiClient(OpenapiClientArgs),
    /// Ask each page route as a native client and print its grade (idiom, layout or web) and what is missing; --fail-on makes it a gate.
    #[command(name = "native:coverage")]
    NativeCoverage(NativeCoverageArgs),
    /// Check what a native build needs, without changing anything: the native preview feature, the settings for links, passkeys, push and sign-in, the Android and Apple toolchains.
    #[command(name = "native:doctor")]
    NativeDoctor(NativeDoctorArgs),
    /// A command of the app binary.
    #[command(external_subcommand)]
    Forward(Vec<String>),
}

/// The options of a `make:<kind>` command line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MakeFlags<'a> {
    /// `make:listener --event <event>`.
    pub event: Option<&'a str>,
    /// `make:factory --model <model>`.
    pub model: Option<&'a str>,
    /// What `make:model` generates with the model.
    pub companions: Companions,
}

impl Command {
    /// The kind, arguments and options of a `make:<kind>` command (not
    /// `make:module`).
    pub fn make_item(&self) -> Option<(MakeKind, &MakeArgs, MakeFlags<'_>)> {
        let none = MakeFlags::default();
        let (kind, args, flags) = match self {
            Command::MakeController(a) => (MakeKind::Controller, a, none),
            Command::MakeService(a) => (MakeKind::Service, a, none),
            Command::MakeEntity(a) => (MakeKind::Entity, a, none),
            Command::MakeMigration(a) => (MakeKind::Migration, a, none),
            Command::MakeJob(a) => (MakeKind::Job, a, none),
            Command::MakeEvent(a) => (MakeKind::Event, a, none),
            Command::MakeListener(a) => (
                MakeKind::Listener,
                &a.item,
                MakeFlags {
                    event: a.event.as_deref(),
                    ..none
                },
            ),
            Command::MakeMail(a) => (MakeKind::Mail, a, none),
            Command::MakeNotification(a) => (MakeKind::Notification, a, none),
            Command::MakePolicy(a) => (MakeKind::Policy, a, none),
            Command::MakeCommand(a) => (MakeKind::Command, a, none),
            Command::MakeAgent(a) => (MakeKind::Agent, a, none),
            Command::MakeTest(a) => (MakeKind::Test, a, none),
            Command::MakeMcp(a) => (MakeKind::Mcp, a, none),
            Command::MakeModel(a) => (
                MakeKind::Model,
                &a.item,
                MakeFlags {
                    companions: a.companions(),
                    ..none
                },
            ),
            Command::MakeFactory(a) => (
                MakeKind::Factory,
                &a.item,
                MakeFlags {
                    model: a.model.as_deref(),
                    ..none
                },
            ),
            Command::MakeSeeder(a) => (MakeKind::Seeder, a, none),
            _ => return None,
        };
        Some((kind, args, flags))
    }
}

/// `navree new <name> [--kit saas|api|micro] [--ui next|navree|none] [--database <db>] [--navree-path <path>]`.
#[derive(Debug, Args)]
pub struct NewArgs {
    /// The app (package) name: lower-case letters, digits, `-` and `_`.
    pub name: String,
    /// A starter kit: `saas` (API and Next.js front end), `api` (the API
    /// alone) or `micro` (a microservice); default: the minimal app.
    #[arg(long, value_enum, value_name = "KIT")]
    pub kit: Option<Kit>,
    /// The front end to create next to the API: `navree` (screens drawn on
    /// the server with Navree UI, in `src/`; no Node; the default for the
    /// minimal app), `next` (a Next.js app in `web/`) or `none` (the API
    /// alone). With a kit, only `saas` has a front end, and it is Next.js
    /// (`--kit saas --ui next` is the same as `--kit saas`); `--ui navree`
    /// goes with no kit.
    #[arg(long, value_enum)]
    pub ui: Option<Ui>,
    /// The database: TimescaleDB (PostgreSQL with time-series hypertables,
    /// run by `navree dock up`), plain PostgreSQL, MySQL/MariaDB, or SQLite
    /// (a file; nothing to install). The kits run on TimescaleDB or
    /// PostgreSQL.
    #[arg(long, value_enum, default_value_t = Database::Timescaledb)]
    pub database: Database,
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

/// `navree make:resource <module> <Model> [--path src/modules]`.
#[derive(Debug, Args)]
pub struct MakeResourceArgs {
    /// The module (a directory under --path, created by make:module).
    pub module: String,
    /// The model, as `Deal` or `deal` (in models/MODEL.rs, made by make:model).
    pub model: String,
    /// The directory holding feature modules, relative to the app.
    #[arg(long, default_value = MODULES_DIR)]
    pub path: String,
}

/// `navree make:form|table <Name> [--module <module>] [--path src/modules]
/// [--dry-run] [--force]`.
#[derive(Debug, Args)]
pub struct MakeUiArgs {
    /// The name, as `new_deal` or `NewDeal`.
    pub name: String,
    /// The module (a directory under --path, created by make:module); default: the app's only module.
    #[arg(long, value_name = "MODULE")]
    pub module: Option<String>,
    /// The directory holding feature modules, relative to the app.
    #[arg(long, default_value = MODULES_DIR)]
    pub path: String,
    /// Say what would be created and edited, and write nothing.
    #[arg(long)]
    pub dry_run: bool,
    /// Replace the files that exist.
    #[arg(long)]
    pub force: bool,
}

/// `navree make:page <Name> [--module <module>] [--public] [--path src/modules]
/// [--dry-run] [--force]`.
#[derive(Debug, Args)]
pub struct MakePageArgs {
    /// The name and the module.
    #[command(flatten)]
    pub item: MakeUiArgs,
    /// Anyone may open the page, not only signed-in users.
    #[arg(long)]
    pub public: bool,
}

/// `navree make:model <module> <Name> [--migration] [--factory] [--seeder]
/// [--controller] [--policy] [--all]`.
#[derive(Debug, Args)]
pub struct MakeModelArgs {
    /// The module and the model name (`deal` or `Deal`).
    #[command(flatten)]
    pub item: MakeArgs,
    /// Also a migration creating the table (create_NAMES_table, with an id, a name and timestamps).
    #[arg(long, short = 'm')]
    pub migration: bool,
    /// Also a factory (factories/NAME.rs), linked to the model: `Model::factory()`.
    #[arg(long, short = 'f')]
    pub factory: bool,
    /// Also a seeder (seeders/NAME.rs) for db:seed, using the factory when there is one.
    #[arg(long, short = 's')]
    pub seeder: bool,
    /// Also a JSON controller (controllers/NAME.rs) with records bound by route key.
    #[arg(long, short = 'c')]
    pub controller: bool,
    /// Also a policy (policies/NAME.rs) attached to the model, which makes it an access subject.
    #[arg(long)]
    pub policy: bool,
    /// A pivot model: the table of a many-to-many between the two models NAME joins (deal_tag), holding their keys.
    #[arg(long, short = 'p')]
    pub pivot: bool,
    /// All of the above (for a pivot: migration, factory and seeder).
    #[arg(long, short = 'a')]
    pub all: bool,
}

impl MakeModelArgs {
    /// The companions asked for.
    pub fn companions(&self) -> Companions {
        match (self.all, self.pivot) {
            (true, false) => Companions::all(),
            (true, true) => Companions::all_for_pivot(),
            (false, pivot) => Companions {
                migration: self.migration,
                factory: self.factory,
                seeder: self.seeder,
                controller: self.controller,
                policy: self.policy,
                pivot,
            },
        }
    }
}

/// `navree make:factory <module> <name> [--model <model>]`.
#[derive(Debug, Args)]
pub struct MakeFactoryArgs {
    /// The module and the factory name.
    #[command(flatten)]
    pub item: MakeArgs,
    /// The module's model the factory builds (models/MODEL.rs); default: NAME.
    #[arg(long, value_name = "MODEL")]
    pub model: Option<String>,
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
    /// Only these steps: fmt, clippy, test, check, config, web:typecheck, web:test, web:build (`web` = every web step).
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

/// `navree native:coverage [--platform <p>] [--form <f>] [--json] [--fail-on <grade>] [--features <f>] [--file <path>]`.
#[derive(Debug, Args)]
pub struct NativeCoverageArgs {
    /// The system to ask as; default android.
    #[arg(long, value_parser = clap::builder::PossibleValuesParser::new(crate::native::PLATFORMS))]
    pub platform: Option<String>,
    /// The kind of device to ask as; default phone.
    #[arg(long, value_parser = clap::builder::PossibleValuesParser::new(crate::native::FORMS))]
    pub form: Option<String>,
    /// Print the report as JSON.
    #[arg(long)]
    pub json: bool,
    /// Exit with an error when a page has this grade or a worse one: `web`,
    /// or `layout` (layout and web).
    #[arg(long, value_enum, value_name = "GRADE")]
    pub fail_on: Option<crate::native_coverage::FailOn>,
    /// Cargo features to build the app with (default: the one in Cargo.toml that turns the native preview on).
    #[arg(long, value_delimiter = ',', value_name = "FEATURE")]
    pub features: Vec<String>,
    /// The file of example values for route parameters (default: native.toml in the app).
    #[arg(long, value_name = "FILE")]
    pub file: Option<PathBuf>,
}

/// `navree native:doctor [--platform <system>] [--url <site>] [--json]`.
#[derive(Debug, Args)]
pub struct NativeDoctorArgs {
    /// Check for these systems: `ios` (the Apple systems), `android` or `all`.
    #[arg(long, value_enum, default_value_t)]
    pub platform: crate::native_doctor::Scope,
    /// Also ask this running site (https://orders.example.com) for the association files Apple and Android read.
    #[arg(long, value_name = "SITE")]
    pub url: Option<String>,
    /// Print the findings as JSON.
    #[arg(long)]
    pub json: bool,
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

/// `navree openapi:client [--out web/src/lib/api] [--input openapi.json]`.
#[derive(Debug, Args)]
pub struct OpenapiClientArgs {
    /// Where openapi.json and schema.d.ts go: relative to the app, or to the
    /// current directory with `--input`.
    #[arg(long, default_value = crate::openapi_client::DEFAULT_OUT)]
    pub out: PathBuf,
    /// Generate from this OpenAPI document (JSON) instead of exporting the
    /// app's; no app is needed.
    #[arg(long, value_name = "FILE")]
    pub input: Option<PathBuf>,
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
    fn openapi_client_defaults_to_the_web_api_folder() {
        let Command::OpenapiClient(args) = parse(&["openapi:client"]) else {
            panic!("expected openapi:client");
        };
        assert_eq!(args.out, PathBuf::from("web/src/lib/api"));
        assert_eq!(args.input, None);
        let Command::OpenapiClient(args) = parse(&[
            "openapi:client",
            "--out",
            "web/app/lib/api",
            "--input",
            "openapi.json",
        ]) else {
            panic!("expected openapi:client");
        };
        assert_eq!(args.out, PathBuf::from("web/app/lib/api"));
        assert_eq!(args.input, Some(PathBuf::from("openapi.json")));
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
        assert_eq!(args.ui, Some(Ui::Next));
        assert_eq!(
            args.navree_path.as_deref(),
            Some(std::path::Path::new("../navree"))
        );
        let Command::New(args) = parse(&["new", "blog", "--ui=none"]) else {
            panic!("expected new");
        };
        assert_eq!(args.ui, Some(Ui::None));
        let Command::New(args) = parse(&["new", "blog", "--ui", "navree"]) else {
            panic!("expected new");
        };
        assert_eq!(args.ui, Some(Ui::Navree));
        // Nothing said: the app resolves it (Navree UI, or a kit's own).
        let Command::New(args) = parse(&["new", "blog"]) else {
            panic!("expected new");
        };
        assert_eq!(args.ui, None);
        assert_eq!(args.kit, None);
        assert_eq!(args.database, Database::Timescaledb);
    }

    #[test]
    fn new_with_a_kit() {
        for (flag, kit) in [
            ("saas", Kit::Saas),
            ("api", Kit::Api),
            ("micro", Kit::Micro),
        ] {
            let Command::New(args) = parse(&["new", "agra", "--kit", flag]) else {
                panic!("expected new");
            };
            assert_eq!(args.kit, Some(kit));
        }
        let Command::New(args) = parse(&["new", "agra", "--kit=saas", "--navree-path", ".."])
        else {
            panic!("expected new");
        };
        assert_eq!(args.kit, Some(Kit::Saas));
        assert!(args.navree_path.is_some());
        // The SaaS kit's front end is Next.js; `create_app` refuses a
        // front end for the other kits.
        let Command::New(args) = parse(&["new", "agra", "--kit", "saas", "--ui", "next"]) else {
            panic!("expected new");
        };
        assert_eq!((args.kit, args.ui), (Some(Kit::Saas), Some(Ui::Next)));
        let error = Cli::try_parse_from(["navree", "new", "agra", "--kit", "crm"]).unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::InvalidValue);
    }

    #[test]
    fn new_takes_the_database() {
        for (flag, database) in [
            ("timescaledb", Database::Timescaledb),
            ("timescale", Database::Timescaledb),
            ("postgres", Database::Postgres),
            ("postgresql", Database::Postgres),
            ("mysql", Database::Mysql),
            ("mariadb", Database::Mysql),
            ("sqlite", Database::Sqlite),
        ] {
            let Command::New(args) = parse(&["new", "blog", "--database", flag]) else {
                panic!("expected new");
            };
            assert_eq!(args.database, database, "{flag}");
        }
        let error = Cli::try_parse_from(["navree", "new", "blog", "--database", "oracle"])
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("timescaledb, postgres, mysql, sqlite"),
            "{error}"
        );
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
            let (parsed_kind, args, flags) =
                parsed.make_item().unwrap_or_else(|| panic!("{command}"));
            assert_eq!(parsed_kind, kind);
            assert_eq!(
                (args.module.as_str(), args.name.as_str()),
                ("posts", "thing")
            );
            assert_eq!(flags, MakeFlags::default());
        }
        let parsed = parse(&[
            "make:listener",
            "posts",
            "notify",
            "--event",
            "post_published",
        ]);
        assert_eq!(parsed.make_item().unwrap().2.event, Some("post_published"));
        let parsed = parse(&["make:factory", "crm", "vip_deal", "--model", "deal"]);
        assert_eq!(parsed.make_item().unwrap().2.model, Some("deal"));
    }

    #[test]
    fn every_ui_kind_has_a_command() {
        for kind in MakeKind::UI {
            let command = format!("make:{}", kind.noun());
            let name = match parse(&[&command, "dashboard"]) {
                Command::MakePage(args) => args.item.name,
                Command::MakeForm(args) | Command::MakeTable(args) => args.name,
                other => panic!("{command}: {other:?}"),
            };
            assert_eq!(name, "dashboard", "{command}");
        }
        // They take `<Name> [--module]`, not `<module> <name>`, so they are
        // not among the kinds `make_item` reads.
        assert!(parse(&["make:form", "new_deal"]).make_item().is_none());
        assert!(!MakeKind::ALL.iter().any(|kind| MakeKind::UI.contains(kind)));
    }

    #[test]
    fn make_ui_options() {
        let Command::MakePage(args) = parse(&["make:page", "Dashboard"]) else {
            panic!("expected make:page");
        };
        assert_eq!(args.item.name, "Dashboard");
        assert_eq!(args.item.module, None);
        assert_eq!(args.item.path, "src/modules");
        assert!(!args.public && !args.item.dry_run && !args.item.force);
        let Command::MakePage(args) = parse(&[
            "make:page",
            "about",
            "--module",
            "site",
            "--public",
            "--dry-run",
            "--force",
            "--path",
            "src/features",
        ]) else {
            panic!("expected make:page");
        };
        assert_eq!(args.item.module.as_deref(), Some("site"));
        assert_eq!(args.item.path, "src/features");
        assert!(args.public && args.item.dry_run && args.item.force);
        let Command::MakeTable(args) = parse(&["make:table", "deals", "--module=crm", "--dry-run"])
        else {
            panic!("expected make:table");
        };
        assert_eq!(args.module.as_deref(), Some("crm"));
        assert!(args.dry_run && !args.force);
        // `<module> <name>` is the other generators' form; a second name is a mistake.
        assert!(Cli::try_parse_from(["navree", "make:page", "site", "about"]).is_err());
        // Only a page can be public.
        assert!(Cli::try_parse_from(["navree", "make:form", "new_deal", "--public"]).is_err());
    }

    #[test]
    fn the_help_lists_the_ui_generators() {
        let help = Cli::try_parse_from(["navree", "--help"])
            .unwrap_err()
            .to_string();
        for command in ["make:page", "make:form", "make:table"] {
            assert!(help.contains(command), "{help}");
        }
        assert!(help.contains("forms/NAME.rs"), "{help}");
    }

    #[test]
    fn make_model_companions() {
        let companions = |args: &[&str]| {
            let parsed = parse(&[&["make:model", "crm", "Deal"], args].concat());
            let (kind, item, flags) = parsed.make_item().unwrap();
            assert_eq!((kind, item.name.as_str()), (MakeKind::Model, "Deal"));
            flags.companions
        };
        assert_eq!(companions(&[]), Companions::default());
        assert_eq!(
            companions(&["--migration", "--factory", "--policy"]),
            Companions {
                migration: true,
                factory: true,
                policy: true,
                ..Companions::default()
            }
        );
        assert_eq!(
            companions(&["-mfsc"]),
            Companions {
                migration: true,
                factory: true,
                seeder: true,
                controller: true,
                policy: false,
                pivot: false,
            }
        );
        assert_eq!(companions(&["--all"]), Companions::all());
        assert_eq!(companions(&["-a", "--seeder"]), Companions::all());
        assert_eq!(
            companions(&["--pivot", "--all"]),
            Companions::all_for_pivot()
        );
        assert_eq!(
            companions(&["-pm"]),
            Companions {
                migration: true,
                pivot: true,
                ..Companions::default()
            }
        );
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
            "/home/anu/code/crm",
            "--no-build",
            "--allow-remote-database",
            "--docs-offline",
        ]) else {
            panic!("expected mcp");
        };
        assert_eq!(
            args.path.as_deref(),
            Some(std::path::Path::new("/home/anu/code/crm"))
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
    fn native_coverage_flags() {
        use crate::native_coverage::FailOn;

        let Command::NativeCoverage(args) = parse(&["native:coverage"]) else {
            panic!("expected native:coverage");
        };
        assert_eq!(args.platform, None);
        assert_eq!(args.form, None);
        assert!(!args.json && args.fail_on.is_none() && args.features.is_empty());
        let Command::NativeCoverage(args) = parse(&[
            "native:coverage",
            "--platform",
            "ios",
            "--form=tablet",
            "--json",
            "--fail-on",
            "web",
            "--features",
            "native-preview,orm",
            "--file",
            "ci/native.toml",
        ]) else {
            panic!("expected native:coverage");
        };
        assert_eq!(args.platform.as_deref(), Some("ios"));
        assert_eq!(args.form.as_deref(), Some("tablet"));
        assert!(args.json);
        assert_eq!(args.fail_on, Some(FailOn::Web));
        assert_eq!(args.features, ["native-preview", "orm"]);
        assert_eq!(
            args.file.as_deref(),
            Some(std::path::Path::new("ci/native.toml"))
        );
        let Command::NativeCoverage(args) = parse(&["native:coverage", "--fail-on=layout"]) else {
            panic!("expected native:coverage");
        };
        assert_eq!(args.fail_on, Some(FailOn::Layout));
        // The names are the protocol's.
        for bad in [
            &["native:coverage", "--platform", "symbian"][..],
            &["native:coverage", "--form", "fridge"],
            &["native:coverage", "--fail-on", "idiom"],
        ] {
            let mut all = vec!["navree"];
            all.extend_from_slice(bad);
            assert!(Cli::try_parse_from(all).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn native_doctor_flags() {
        use crate::native_doctor::Scope;

        let Command::NativeDoctor(args) = parse(&["native:doctor"]) else {
            panic!("expected native:doctor");
        };
        assert_eq!(args.platform, Scope::All);
        assert_eq!(args.url, None);
        assert!(!args.json);
        let Command::NativeDoctor(args) = parse(&[
            "native:doctor",
            "--platform",
            "ios",
            "--url",
            "https://orders.example.com",
            "--json",
        ]) else {
            panic!("expected native:doctor");
        };
        assert_eq!(args.platform, Scope::Ios);
        assert_eq!(args.url.as_deref(), Some("https://orders.example.com"));
        assert!(args.json);
        let Command::NativeDoctor(args) = parse(&["native:doctor", "--platform=android"]) else {
            panic!("expected native:doctor");
        };
        assert_eq!(args.platform, Scope::Android);
        assert!(Cli::try_parse_from(["navree", "native:doctor", "--platform", "tvos"]).is_err());
    }

    #[test]
    fn the_help_lists_the_native_commands() {
        let help = Cli::try_parse_from(["navree", "--help"])
            .unwrap_err()
            .to_string();
        assert!(help.contains("native:coverage"), "{help}");
        assert!(help.contains("native:doctor"), "{help}");
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
