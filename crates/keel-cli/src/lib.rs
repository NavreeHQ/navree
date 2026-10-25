//! # keel-cli
//!
//! The library behind the `keel` command-line tool (Laravel's Artisan,
//! NestJS's CLI). It depends on no keel crate: everything it knows about the
//! framework is in the code it generates.
//!
//! | Command | What it does |
//! |---|---|
//! | `keel new <name> [--ui next\|none] [--keel-path <path>]` | A new app ([`new_app`]) |
//! | `keel make:module <name> [--crud] [--path src/modules]` | A feature module, wired into the app ([`make::make_module`]) |
//! | `keel make:<kind> <module> <name>` | A controller, service, entity, migration, job, event, listener, mail, notification, policy, command or test in a module ([`make::make_item`]) |
//! | `keel dev [-- <serve args>]` | Build, serve, rebuild and restart on change ([`dev`]) |
//! | `keel <anything else> [args...]` | `cargo run --quiet -- <command> [args...]` in the app ([`forward`]) |
//!
//! Generators render MiniJinja templates embedded in the binary
//! ([`templates`]) into a [`scaffold::FileSet`] that is written without
//! overwriting anything; existing files are only edited at `// keel:<name>`
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
pub mod scaffold;
pub mod templates;
