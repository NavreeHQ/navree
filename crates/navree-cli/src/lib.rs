//! # navree-cli
//!
//! The library behind the `navree` command-line tool: it creates apps,
//! generates code, runs the dev loop, the app's local services and deploys.
//! Its only navree dependencies are two tool crates, navree-dock (local
//! services) and navree-harbor (servers and deploys): everything it knows
//! about the framework's runtime is in the code it generates.
//!
//! | Command | What it does |
//! |---|---|
//! | `navree new <name> [--ui next\|none] [--navree-path <path>]` | A new app ([`new_app`]) |
//! | `navree make:module <name> [--crud] [--path src/modules]` | A feature module, wired into the app ([`make::make_module`]) |
//! | `navree make:<kind> <module> <name>` | A controller, service, entity, migration, job, event, listener, mail, notification, policy, command, test or MCP server in a module ([`make::make_item`]) |
//! | `navree make:resource <module> <Entity> [--path src/modules]` | A Helm admin-panel resource for an entity, registered with its module ([`resource::make_resource`]) |
//! | `navree dev [-- <serve args>]` | Build, serve, rebuild and restart on change ([`dev`]) |
//! | `navree check [--graph] [--json] [--only <checks>]` | The module graph, then the app's conformity checks ([`check`]) |
//! | `navree gate [--only <steps>] [--skip <steps>] [--keep-going] [--no-lock]` | fmt, clippy, tests, `navree check` and the front end's typecheck, tests and build, under a machine lock with per-step times ([`gate`]) |
//! | `navree dock up\|down\|status\|logs\|env\|shell` | The app's local services from `dock.toml`, in Docker or Podman ([`dock`]) |
//! | `navree dock:add <service>`, `navree dock:remove <service>` | Edit `dock.toml` ([`dock`]) |
//! | `navree harbor:*`, `deploy`, `deploy:rollback`, `deploy:releases`, `build:image` | Servers and deployments with Harbor (`navree_harbor::cli`) |
//! | `navree browser:install [--version <version>] [--force]` | Download Chrome for Testing into Navree's cache for navree-browser ([`browser`]) |
//! | `navree <anything else> [args...]` | `cargo run --quiet -- <command> [args...]` in the app ([`forward`]) |
//!
//! When the app has a `dock.toml`, `navree dev` and forwarded commands pass
//! its services' settings to the app as environment variables.
//!
//! Generators render MiniJinja templates embedded in the binary
//! ([`templates`]) into a [`scaffold::FileSet`] that is written without
//! overwriting anything; existing files are only edited at `// navree:<name>`
//! markers or module declarations ([`edit`]), and anything that cannot be
//! edited safely becomes a printed hint.
#![forbid(unsafe_code)]

pub mod browser;
pub mod check;
pub mod cli;
pub mod clock;
pub mod commands;
pub mod dev;
pub mod dock;
pub mod edit;
pub mod forward;
pub mod gate;
pub mod make;
pub mod names;
pub mod new_app;
pub mod project;
pub mod resource;
pub mod scaffold;
pub mod templates;
