---
name: navree-cli-generators
description: "Uses the `navree` CLI: `navree new` (Navree UI by default, `--ui`, `--kit`), every `make:*` generator with the files it writes and what it registers (including `make:page`, `make:form` and `make:table` for Navree UI), the marker comments, `navree dev`, forwarding to the app's own commands, and console commands with `CommandSpec`. Use when generating any code in a Navree app (module, controller, service, model, factory, seeder, migration, job, event, listener, mail, notification, policy, command, test, MCP server, Helm resource, Navree UI page, form or data table), running an app command such as migrate or queue:work, or adding a custom command."
---

# The navree CLI and generators

Two command lines:

- **`navree`** (crate navree-cli) creates apps, generates code, runs the
  dev loop and the app's local services (`navree dock`, skill
  `navree-dock`). It depends on no Navree runtime crate.
- **The app's binary** owns runtime commands (`serve`, `migrate`,
  `queue:work`, ...). `navree <command>` forwards anything the CLI doesn't
  own (`new`, `make:*`, `dev`, `check`, `gate`, `dock*`, the Harbor commands,
  `mcp`, `skills:install`, `browser:install`, `openapi:client`) as
  `cargo run --quiet -- <command>`. In
  production, run the binary itself: `./my-app migrate`. When the app has
  a `dock.toml`, `navree dev` and forwarded commands get its services'
  settings as environment variables.

## Creating an app

```sh
navree new shop                     # the minimal app with screens drawn by Navree UI (the default)
navree new shop --ui none           # the API alone
navree new shop --ui next           # the API and a Next.js front end in web/
navree new shop --kit saas|api|micro   # a starter kit (the saas kit's front end is Next.js)
```

The default front end is Navree UI: sign-in with two-factor authentication
(`AuthPages`), two demo workspaces and a switcher, a customers table, forms,
a home page with live figures, and `tests/journey.rs`. `--ui navree` goes with
no kit. See the skill `navree-new-app`.

## Generators

Run them anywhere inside the app (the CLI finds the package). Names are
`snake_case` (a wrong name fails with a suggestion); module names are
plural. Generators **never overwrite** (nothing is written if a file
exists), format with rustfmt when installed, and register what they create
by editing code at markers; when a marker is missing they print a `next`
hint with the line to add. Keep the markers:

| Marker | File | Generators add |
|---|---|---|
| `// navree:modules` | `src/app.rs` | `.module(modules::<name>::<Name>Module)` |
| `// navree:wire` | a module's `wire` method | controllers, providers, jobs, listeners, templates, commands, migration sets |
| `// navree:policies` | `src/abilities.rs` | `crate::modules::<m>::policies::<p>::define(ctx, rules);` |

| Command | Creates (in `src/modules/<module>/`) | Registers |
|---|---|---|
| `navree make:module posts` | `posts/` with `mod.rs`, `controller.rs` (one authenticated `index`), `service.rs`, `entity.rs` (the `Post` ORM model), `dto.rs`, `migrations/`, `tests/` | `pub mod posts;` in `modules/mod.rs`; the module in `src/app.rs` |
| `navree make:module posts --crud` | the same with five JSON routes (`#[can]` on `Post`), DTOs with `changed_fields`, a `#[model(subject)]` model, owner-based tests | same |
| `make:controller posts admin` | `controllers/admin.rs`, `AdminController` at `/api/v1/admin` | the controller in `wire` |
| `make:service posts pricing` | `services/pricing.rs`, `PricingService` | `m.provider::<..>()` in `wire` |
| `make:model posts Tag` | `models/tag.rs`, an ORM model (table `tags`: id, name, timestamps); `--migration`, `--factory`, `--seeder`, `--controller`, `--policy`, or `--all`, add companions that refer to each other; `--pivot` makes the table of a many-to-many (`post_tag`) (see `navree-orm`) | `m.model::<..>()` in `wire` (and each companion's registration) |
| `make:factory posts tag` | `factories/tag.rs`, a model factory (`--model <model>` when the names differ) | nothing; link it with `#[model(factory = ..)]` |
| `make:seeder posts demo_data` | `seeders/demo_data.rs`, `DemoDataSeeder` named `demo_data` | `seeder::<..>` in `wire` |
| `make:entity posts tag` | `entities/tag.rs`, an ORM model (table `tags`: id, timestamps; `make:model` without companions) | nothing; prints the `make:migration` to run |
| `make:resource posts Tag` | `resources/tag.rs`, a Helm resource with a field per column of `models/tag.rs` (made by `make:model`; see `navree-helm`), none for a hidden column or one named like a secret | `pub mod resources;` and `navree::helm::ModuleBuilderHelmExt::resource::<..>(m)` in `wire` |
| `make:migration posts create_tags_table` | `migrations/m<timestamp>_create_tags_table.rs` (CREATE TABLE when named `create_<table>_table`, else empty up/down) | adds it to the module's `MigrationSet` (creating the set and `m.migrations(..)` if needed) |
| `make:job posts publish_post` | `jobs/publish_post.rs`, job `posts.publish_post` | `m.job::<..>()` in `wire` |
| `make:event posts post_published` | `events/post_published.rs`, event `posts.post_published` | nothing |
| `make:listener posts notify --event post_published` | `listeners/notify.rs` | `listen::<PostPublished, Notify>` in `wire` (without `--event`, prints the line) |
| `make:mail posts welcome` | `mail/welcome.rs` + `welcome.html` + `welcome.txt` | the templates in `wire` |
| `make:notification posts post_liked` | `notifications/post_liked.rs` (database + broadcast) | nothing |
| `make:policy posts moderation` | `policies/moderation.rs` (ability rules) | a call in `src/abilities.rs` |
| `make:command posts publish_scheduled` | `commands/publish_scheduled.rs`, command `posts:publish-scheduled` | `m.command(..)` in `wire` |
| `make:agent posts assistant` | `agents/assistant.rs`, agent `Assistant` named `posts-assistant` with one `#[tool(read_only)]` (needs navree's `ai` feature and `AiModule`) | `navree::ai::ModuleBuilderAiExt::agent::<..>(m)` in `wire` |
| `make:test posts publishing` | `tests/publishing.rs`, a test booting the module | `mod publishing;` in `tests/mod.rs` |
| `make:page dashboard --module posts` | `pages/dashboard.rs`, a Navree UI page (`DashboardPage`, a controller whose `GET /dashboard` takes `AppShell` and draws a card) and `tests/dashboard_page.rs`; `--public` lets guests open it | `navree::http::ModuleBuilderHttpExt::controller::<..>(m)` in `wire` |
| `make:form deal --module posts` | `forms/deal.rs`, a form struct (`Validate` + `Form`) with `page` and `store` functions | nothing; call `forms::deal::page` from a `GET` route and `forms::deal::store` from a `POST` route |
| `make:table deal --module posts` | `tables/deal.rs`, a `TableRow` struct and `index` for the module's model (`models/deal.rs`, `entity.rs` or `entities/deal.rs`, with an `id`, a `name` and `created_at`) | nothing; call `tables::deal::index` from a `GET` route |
| `make:mcp posts assistant` | `mcp/assistant.rs`, `AssistantMcp` with one tool `assistant_echo` (see `navree-mcp`) | `navree::mcp::ModuleBuilderMcpExt::mcp_server::<..>(m)` in `wire`; prints the `mcp` feature and `McpModule` steps |

All `make:<kind>` take `--path <dir>` (default `src/modules`). The module
must exist (`make:module` first); `make:listener --event` needs the event
(`make:event` first). `make:page`, `make:form` and `make:table` name the item
first and the module with `--module` (without it they use the app's only
module, and refuse when there are several); they also take `--dry-run` (print
the files, write nothing) and `--force`.

The Navree UI generators write handlers that take `AppShell`, the app's shell.
In an app made by `navree new` (which has `src/ui/shell.rs` with a `Desk`
extractor), take `Desk` instead: it draws the same shell with the signed-in
person in the user menu, the workspace's name in the sidebar and the workspace
switcher beside the heading, and gives the workspace id for
`WorkspaceRepo::<Model>` queries. The generator says so in its next steps.

After generating: replace the placeholder logic, add columns to both the
model and the migration, then `cargo test` (generated tests pass
as written) and `navree routes`.

## The dev loop

```sh
navree dev                               # build, serve, rebuild + restart on change
navree dev --no-web -- --addr 0.0.0.0:8080
```

Watches `src/`, `config/`, `migrations/`, `crates/`, `Cargo.toml`, `.env`, `build.rs`;
keeps the last good build serving when a build fails; starts `web/`'s dev
server too (pnpm, yarn or npm by lockfile, else by the `packageManager`
field of `web/package.json`).

## App commands

```sh
navree list                     # every command of the app (not `navree help`: that's the CLI's)
navree check [--graph]          # module graph (no boot), then the conformity checks (boots once)
navree check --only workspace --json   # some checks, as JSON (skill navree-module-conformity)
navree gate [--skip web]        # fmt, clippy, tests, check and the front end, one gate per machine
navree routes [--json]          # routes with access rules
navree migrate | migrate:status | migrate:rollback | migrate:fresh [--seed]
navree db:seed [name...]
navree key:generate
navree queue:work [--queue a,b] [--once] [--stop-when-empty] [--max-jobs N]
navree queue:failed | queue:retry <id|all> | queue:flush
navree schedule:work | schedule:run | schedule:list
navree access:role <name> [description] | access:grant <role> <perm> | access:explain <user> <action> <Subject> [id]
navree openapi:export [path]
navree cache:clear | down [--retry S] [--message T] | up
navree serve [--addr HOST:PORT] # the default with no command
```

Every command except `check --graph`, `list` and `help` boots the whole app
(config, database, modules), so it needs a working configuration;
`navree check` validates the graph without booting, then boots once for
`conformity:check`. Modules add their
own (`audit:prune` with `AuditModule`, schedule commands with `ScheduleModule`).

## Writing a console command

A command is a `CommandSpec` (`navree make:command exports clear` writes the
skeleton and registers it); by hand, register it with `m.command(..)` in
the module's `wire`:

```rust
use navree::db::Db;
use navree::kernel::{AppContext, BoxError, CommandSpec};

/// `exports:clear [--dry-run]`.
pub fn clear_command() -> CommandSpec {
    CommandSpec::new("exports:clear", "Delete every generated export ([--dry-run])", clear)
}

async fn clear(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let dry_run = args.iter().any(|arg| arg == "--dry-run");
    if let Some(unknown) = args.iter().find(|arg| *arg != "--dry-run") {
        return Err(format!("unknown option `{unknown}` (expected --dry-run)").into());
    }
    let db = ctx.get::<Db>().await?; // any provider, ignoring module visibility
    // ... work; write through db.transaction(|uow| async move { .. })
    let _ = db;
    println!("{}", if dry_run { "Would delete 0 exports." } else { "Deleted 0 exports." });
    Ok(())
}
```

- Names are unique app-wide (boot error otherwise); prefix them with the
  module (`exports:clear`).
- Arguments arrive unparsed; reject what you don't understand.
- Return `Err(..)` to exit with status 1.
- Schedule it with `m.schedule(|s| { s.command("exports:clear", &[]).daily_at("04:00"); })`.
- Test with `app.run_command("exports:clear", &["--dry-run"]).await`.

## Common mistakes

- Deleting or moving the marker comments: generators then only print hints.
- Generating into a module that doesn't exist, or a listener for an event
  that doesn't exist.
- `CamelCase` or singular module names (`navree make:module Post`): use `posts`.
- Running `navree help` to list app commands (use `navree list`).
- Leaving `AppShell` in a generated page of a Navree UI app that has a `Desk`:
  the page then shows no workspace and the generic user menu.
- `navree new` writes `navree = "1.1.0"` (the CLI's own version) when it
  can't find a checkout: pass `--navree-path`.

## Docs

- https://navree.com/docs/digging-deeper/cli/
- https://navree.com/docs/getting-started/directory-structure/
