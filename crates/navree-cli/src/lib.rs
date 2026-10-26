//! # navree-cli
//!
//! The library behind the `navree` command-line tool: it creates apps,
//! generates code and runs the dev loop. It depends on no navree crate:
//! everything it knows about the framework is in the code it generates.
//!
//! | Command | What it does |
//! |---|---|
//! | `navree new <name> [--ui next\|none] [--navree-path <path>]` | A new app ([`new_app`]) |
//! | `navree make:module <name> [--crud] [--path src/modules]` | A feature module, wired into the app ([`make::make_module`]) |
//! | `navree make:<kind> <module> <name>` | A controller, service, entity, migration, job, event, listener, mail, notification, policy, command or test in a module ([`make::make_item`]) |
//! | `navree make:resource <module> <Entity> [--path src/modules]` | A Helm admin-panel resource for an entity, registered with its module ([`resource::make_resource`]) |
//! | `navree dev [-- <serve args>]` | Build, serve, rebuild and restart on change ([`dev`]) |
//! | `navree <anything else> [args...]` | `cargo run --quiet -- <command> [args...]` in the app ([`forward`]) |
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
pub mod edit;
pub mod forward;
pub mod make;
pub mod names;
pub mod new_app;
pub mod project;
pub mod resource;
pub mod scaffold;
pub mod templates;
