//! # navree-cli
//!
//! The library behind the `navree` command-line tool: it creates apps,
//! generates code, runs the dev loop, the app's local services and deploys,
//! and serves Navigator to coding agents. Its only navree dependencies are
//! three tool crates, navree-dock (local services), navree-harbor (servers
//! and deploys) and navree-navigator (`navree mcp`, `navree skills:install`):
//! everything it knows about the framework's runtime is in the code it
//! generates.
//!
//! | Command | What it does |
//! |---|---|
//! | `navree new <name> [--database timescaledb\|postgres\|mysql\|sqlite] [--ui next\|navree\|none] [--navree-path <path>]` | A new app ([`new_app`]), made a Git repository when it is not inside one (nothing is committed) |
//! | `navree new <name> --kit saas\|api\|micro` | A new app from a starter kit ([`kits`]), with its `harbor.key` |
//! | `navree make:module <name> [--crud] [--path src/modules]` | A feature module, wired into the app ([`make::make_module`]) |
//! | `navree make:<kind> <module> <name>` | A controller, service, entity, migration, job, event, listener, mail, notification, policy, command, test, MCP server, factory (`--model <model>`) or seeder in a module ([`make::make_item`]) |
//! | `navree make:model <module> <Name> [--migration --factory --seeder --controller --policy \| --all]` | An ORM model with the companions asked for, which refer to each other ([`make::make_model`]) |
//! | `navree make:resource <module> <Model> [--path src/modules]` | A Helm admin-panel resource for a navree-orm model, registered with its module ([`resource::make_resource`]) |
//! | `navree make:page <Name> [--module <module>] [--public]`, `make:form <Name>`, `make:table <Name>` (each with `--dry-run` and `--force`) | A Navree UI page (a controller and its test), a form struct with its handlers, or a data table row with its handler ([`make_ui::make_ui_item`]) |
//! | `navree dev [-- <serve args>]` | Build, serve, rebuild and restart on change ([`dev`]) |
//! | `navree check [--graph] [--json] [--only <checks>]` | The module graph, then the app's conformity checks ([`check`]) |
//! | `navree gate [--only <steps>] [--skip <steps>] [--keep-going] [--no-lock]` | fmt, clippy, tests, `navree check` and the front end's typecheck, tests and build, under a machine lock with per-step times ([`gate`]) |
//! | `navree dock up\|down\|status\|logs\|env\|shell` | The app's local services from `dock.toml`, in Docker or Podman ([`dock`]) |
//! | `navree dock:add <service>`, `navree dock:remove <service>` | Edit `dock.toml` ([`dock`]) |
//! | `navree harbor:*`, `deploy`, `deploy:rollback`, `deploy:releases`, `build:image` | Servers and deployments with Harbor (`navree_harbor::cli`) |
//! | `navree mcp [--path <dir>] [--no-build] [--allow-remote-database] [--docs-offline]` | Navigator, the MCP server for coding agents, on stdio (navree-navigator) |
//! | `navree skills:install [--path <dir>]` | The Navree agent skills into `.claude/skills/` and `.agents/skills/` |
//! | `navree browser:install [--version <version>] [--force]` | Download Chrome for Testing into Navree's cache for navree-browser ([`browser`]) |
//! | `navree openapi:client [--out web/src/lib/api] [--input <file>]` | The OpenAPI document and the front end's `schema.d.ts` for `@navree/client` ([`openapi_client`]) |
//! | `navree <anything else> [args...]` | `cargo run --quiet -- <command> [args...]` in the app ([`forward`]) |
//!
//! When the app has a `dock.toml`, `navree dev` and forwarded commands pass
//! its services' settings to the app as environment variables.
//!
//! Generators render templates embedded in the binary
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
pub mod kits;
pub mod make;
pub mod make_ui;
pub mod names;
pub mod new_app;
pub mod openapi_client;
pub mod project;
pub mod resource;
pub mod scaffold;
pub mod templates;
