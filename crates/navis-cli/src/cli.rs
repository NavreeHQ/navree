//! The command line: `navis new`, `navis make:*`, `navis dev`, and every other
//! command forwarded to the app binary.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::make::MakeKind;
pub use crate::new_app::Ui;

/// The default directory of feature modules.
pub const MODULES_DIR: &str = "src/modules";

/// `navis`: create Navis apps, generate code, run the dev loop, and run the
/// app's own commands.
#[derive(Debug, Parser)]
#[command(
    name = "navis",
    version,
    about = "Create Navis apps, generate code and run the dev loop",
    after_help = "Every other command runs in the app binary: `navis <command> [args...]` is \
                  `cargo run --quiet -- <command> [args...]` (migrate, migrate:fresh --seed, db:seed, \
                  queue:work, schedule:run, routes, check --graph, openapi:export, key:generate, \
                  <module>:<command>, ...). `navis list` shows the app's commands."
)]
pub struct Cli {
    /// What to do.
    #[command(subcommand)]
    pub command: Command,
}

/// A `navis` subcommand.
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
    /// Generate a test in a module (tests/NAME.rs).
    #[command(name = "make:test")]
    MakeTest(MakeArgs),
    /// Serve the app, rebuilding and restarting it when the code changes.
    Dev(DevArgs),
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
            Command::MakeTest(a) => (MakeKind::Test, a),
            _ => return None,
        };
        Some((kind, args, None))
    }
}

/// `navis new <name> [--ui next|none] [--navis-path <path>]`.
#[derive(Debug, Args)]
pub struct NewArgs {
    /// The app (package) name: lower-case letters, digits, `-` and `_`.
    pub name: String,
    /// The front end to create next to the API.
    #[arg(long, value_enum, default_value_t = Ui::None)]
    pub ui: Ui,
    /// A local navis checkout (the repository or its crates/navis) to depend
    /// on; default: the checkout this navis was built from, else the
    /// published crate.
    #[arg(long, value_name = "PATH")]
    pub navis_path: Option<PathBuf>,
}

/// `navis make:module <name> [--crud] [--path src/modules]`.
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

/// `navis make:<kind> <module> <name> [--path src/modules]`.
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

/// `navis make:listener <module> <name> [--event <event>]`.
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

/// `navis dev [--debounce-ms N] [--no-web] [-- <serve args>]`.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Command {
        Cli::try_parse_from(std::iter::once("navis").chain(args.iter().copied()))
            .unwrap_or_else(|e| panic!("{args:?}: {e}"))
            .command
    }

    #[test]
    fn no_arguments_print_the_help() {
        let error = Cli::try_parse_from(["navis"]).unwrap_err();
        assert_eq!(
            error.kind(),
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
        assert!(error.to_string().contains("make:module"), "{error}");
    }

    #[test]
    fn new_with_ui_and_navis_path() {
        let Command::New(args) =
            parse(&["new", "blog", "--ui", "next", "--navis-path", "../navis"])
        else {
            panic!("expected new");
        };
        assert_eq!(args.name, "blog");
        assert_eq!(args.ui, Ui::Next);
        assert_eq!(
            args.navis_path.as_deref(),
            Some(std::path::Path::new("../navis"))
        );
        let Command::New(args) = parse(&["new", "blog", "--ui=none"]) else {
            panic!("expected new");
        };
        assert_eq!(args.ui, Ui::None);
        let Command::New(args) = parse(&["new", "blog"]) else {
            panic!("expected new");
        };
        assert_eq!(args.ui, Ui::None);
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
            (&["check", "--graph"], &["check", "--graph"]),
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
