//! # navree-cli
//!
//! The library behind the `navree` command-line tool: it creates apps,
//! generates code, runs the dev loop and the app's local services. Its only
//! navree dependency is navree-dock (a tool crate): everything it knows
//! about the framework's runtime is in the code it generates.
//!
//! | Command | What it does |
//! |---|---|
//! | `navree new <name> [--ui next\|none] [--navree-path <path>]` | A new app ([`new_app`]) |
//! | `navree make:module <name> [--crud] [--path src/modules]` | A feature module, wired into the app ([`make::make_module`]) |
//! | `navree make:<kind> <module> <name>` | A controller, service, entity, migration, job, event, listener, mail, notification, policy, command, test or MCP server in a module ([`make::make_item`]) |
//! | `navree dev [-- <serve args>]` | Build, serve, rebuild and restart on change ([`dev`]) |
//! | `navree dock up\|down\|status\|logs\|env\|shell` | The app's local services from `dock.toml`, in Docker or Podman ([`dock`]) |
//! | `navree dock:add <service>`, `navree dock:remove <service>` | Edit `dock.toml` ([`dock`]) |
//! | `navree openapi:client [--out web/src/lib/api] [--input <file>]` | The OpenAPI document and the front end's `schema.d.ts` for `@navree/client` ([`openapi_client`]) |
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
pub mod openapi_client;
pub mod project;
pub mod scaffold;
pub mod templates;
