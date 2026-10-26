---
name: navree-cli-generators
description: "Uses the `navree` CLI: `navree new`, every `make:*` generator with the files it writes and what it registers, the marker comments, `navree dev`, forwarding to the app's own commands, and console commands with `CommandSpec`. Use when generating any code in a Navree app (module, controller, service, entity, migration, job, event, listener, mail, notification, policy, command, test, MCP server), running an app command such as migrate or queue:work, or adding a custom command."
---

# The navree CLI and generators

Two command lines:

- **`navree`** (crate navree-cli) creates apps, generates code, runs the
  dev loop and the app's local services (`navree dock`, skill
  `navree-dock`). It depends on no Navree runtime crate.
- **The app's binary** owns runtime commands (`serve`, `migrate`,
  `queue:work`, ...). `navree <command>` forwards anything that isn't `new`,
  `make:*`, `dev` or `dock*` as `cargo run --quiet -- <command>`. In
  production, run the binary itself: `./my-app migrate`. When the app has
  a `dock.toml`, `navree dev` and forwarded commands get its services'
  settings as environment variables.

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
| `navree make:module posts` | `posts/` with `mod.rs`, `controller.rs` (one authenticated `index`), `service.rs`, `entity.rs`, `dto.rs`, `migrations/`, `tests/` | `pub mod posts;` in `modules/mod.rs`; the module in `src/app.rs` |
| `navree make:module posts --crud` | the same with five JSON routes (`#[can]` on `Post`), DTOs with `changed_fields`, `Subject` impl, owner-based tests | same |
| `make:controller posts admin` | `controllers/admin.rs`, `AdminController` at `/api/v1/admin` | the controller in `wire` |
| `make:service posts pricing` | `services/pricing.rs`, `PricingService` | `m.provider::<..>()` in `wire` |
| `make:entity posts tag` | `entities/tag.rs` (table `tags`: id, timestamps) | nothing; prints the `make:migration` to run |
| `make:migration posts create_tags_table` | `migrations/m<timestamp>_create_tags_table.rs` (CREATE TABLE when named `create_<table>_table`, else empty up/down) | adds it to the module's `MigrationSet` (creating the set and `m.migrations(..)` if needed) |
| `make:job posts publish_post` | `jobs/publish_post.rs`, job `posts.publish_post` | `m.job::<..>()` in `wire` |
| `make:event posts post_published` | `events/post_published.rs`, event `posts.post_published` | nothing |
| `make:listener posts notify --event post_published` | `listeners/notify.rs` | `listen::<PostPublished, Notify>` in `wire` (without `--event`, prints the line) |
| `make:mail posts welcome` | `mail/welcome.rs` + `welcome.html` + `welcome.txt` | the templates in `wire` |
| `make:notification posts post_liked` | `notifications/post_liked.rs` (database + broadcast) | nothing |
| `make:policy posts moderation` | `policies/moderation.rs` (ability rules) | a call in `src/abilities.rs` |
| `make:command posts publish_scheduled` | `commands/publish_scheduled.rs`, command `posts:publish-scheduled` | `m.command(..)` in `wire` |
| `make:test posts publishing` | `tests/publishing.rs`, a test booting the module | `mod publishing;` in `tests/mod.rs` |
| `make:mcp posts assistant` | `mcp/assistant.rs`, `AssistantMcp` with one tool `assistant_echo` (see `navree-mcp`) | `navree::mcp::ModuleBuilderMcpExt::mcp_server::<..>(m)` in `wire`; prints the `mcp` feature, utoipa and `McpModule` steps |

All `make:<kind>` take `--path <dir>` (default `src/modules`). The module
must exist (`make:module` first); `make:listener --event` needs the event
(`make:event` first). There is no generator for seeders or factories.

After generating: replace the placeholder logic, add columns to both the entity
and the migration, then `cargo test` (generated tests pass as written) and
`navree routes`.

## The dev loop

```sh
navree dev                               # build, serve, rebuild + restart on change
navree dev --no-web -- --addr 0.0.0.0:8080
```

Watches `src/`, `config/`, `migrations/`, `Cargo.toml`, `.env`, `build.rs`;
keeps the last good build serving when a build fails; starts `web/`'s dev
server too (pnpm, yarn or npm by lockfile).

## App commands

```sh
navree list                     # every command of the app (not `navree help`: that's the CLI's)
navree check [--graph]          # validate the module graph; no boot (cheap CI step)
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

Every command except `check`, `list` and `help` boots the whole app (config,
database, modules), so it needs a working configuration. Modules add their
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
- `navree new` writes `navree = "0.1.0"` when it can't find a checkout:
  pass `--navree-path`.

## Docs

- https://navree.com/docs/digging-deeper/cli/
- https://navree.com/docs/getting-started/directory-structure/
