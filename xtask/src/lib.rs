//! Release tooling for the Navree workspace, run as `cargo xtask release ...`
//! (the alias is in `.cargo/config.toml`).
//!
//! Every crate, the CLI, the starter kits and the `@navree/*` npm packages
//! share one version, and Navree's crates require each other at exactly that
//! version (`=1.2.3`), so a release always resolves as the set that was
//! tested. The commands:
//!
//! | Command | What it does |
//! |---|---|
//! | `prepare <version>` | sets the version everywhere and adds the changelog section ([`prepare`]) |
//! | `check` | every version equal, every internal requirement exact, no dev-dependency cycle ([`check`]) |
//! | `order` | prints the publish order ([`order`]) |
//! | `stage [--clean]` | copies the skills, the docs and the UI catalog into Navigator's crate ([`stage`]) |
//! | `publish` | publishes every crate in dependency order, resumably ([`publish`]) |
//!
//! Nothing here keeps state of its own: the registry's index says which
//! crates already have the version, so a publish that stops part-way is
//! started again with the same command.

pub mod changelog;
pub mod check;
pub mod index;
pub mod metadata;
pub mod order;
pub mod prepare;
pub mod process;
pub mod publish;
pub mod stage;
pub mod time;
