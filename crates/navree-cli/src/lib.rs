//! # navree-cli
//!
//! The library behind the `navree` command-line tool: it creates apps,
//! generates code, runs the dev loop and the app's local services, and
//! serves Navigator to coding agents. Its navree dependencies are tool
//! crates, navree-dock and navree-navigator: everything else it knows about
//! the framework's runtime is in the code it generates.
//!
//! | Command | What it does |
//! |---|---|
//! | `navree new <name> [--ui next\|none] [--navree-path <path>]` | A new app ([`new_app`]) |
//! | `navree make:module <name> [--crud] [--path src/modules]` | A feature module, wired into the app ([`make::make_module`]) |
//! | `navree make:<kind> <module> <name>` | A controller, service, entity, migration, job, event, listener, mail, notification, policy, command or test in a module ([`make::make_item`]) |
//! | `navree dev [-- <serve args>]` | Build, serve, rebuild and restart on change ([`dev`]) |
//! | `navree dock up\|down\|status\|logs\|env\|shell` | The app's local services from `dock.toml`, in Docker or Podman ([`dock`]) |
//! | `navree dock:add <service>`, `navree dock:remove <service>` | Edit `dock.toml` ([`dock`]) |
//! | `navree mcp [--path <dir>] [--no-build] [--allow-remote-database] [--docs-offline]` | Navigator, the MCP server for coding agents, on stdio (navree-navigator) |
//! | `navree skills:install [--path <dir>]` | The Navree agent skills into `.claude/skills/` and `.agents/skills/` |
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

pub mod cli;
pub mod clock;
pub mod commands;
pub mod dev;
pub mod dock;
pub mod edit;
pub mod forward;
pub mod make;
pub mod names;
pub mod new_app;
pub mod project;
pub mod scaffold;
pub mod templates;
