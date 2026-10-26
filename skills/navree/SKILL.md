---
name: navree
description: "Navree, the complete backend framework for Rust: what it is, how a Navree app is laid out, the rules every change follows, and which navree-* skill to load for each task. Use when working in any project whose Cargo.toml depends on `navree` or whose code uses `navree::prelude`, `#[module]`, `#[injectable]` or `#[controller]`, or when the user mentions Navree."
---

# Navree

Navree is a framework for building secure, scalable server-side applications
in Rust, on Tokio and Axum. Features live in **modules**; modules declare
what they import, provide and export; a **dependency-injection** container
wires them when the app boots and refuses to start if anything is missing.
Around that structure sits a standard library for backends: routing,
validation, an ORM layer on SeaORM, JWT auth, roles and abilities, queues,
scheduling, mail, notifications, storage, real time, OpenAPI, health checks
and a CLI with generators.

## Recognize a Navree app

- `Cargo.toml` depends on `navree` (plus `serde`, `sea-orm`,
  `sea-orm-migration` and `garde`, whose derives name their own crates).
- `src/app.rs` builds the root with `Navree::app().module(..)` and a
  `// navree:modules` marker; `src/main.rs` calls `app(ConfigSources::defaults()).run()`.
- Feature code lives in `src/modules/<name>/` (`mod.rs`, `controller.rs`,
  `service.rs`, `entity.rs`, `dto.rs`, `migrations/`, `tests/`).
- An `AGENTS.md` at the root holds the project's conventions. Read it first.

```text
config/app.toml        shared settings (APP_* environment variables override them)
.env                   machine secrets such as APP_KEY (never committed)
src/main.rs            runs `serve` by default, or any command
src/app.rs             the root module: every module, configured once
src/abilities.rs       who may do what
src/modules/<name>/    feature modules
tests/                 tests that boot the whole app
```

## The rules every change follows

1. **Generate, then fill in.** Use `navree make:*` (see `navree-cli-generators`):
   generators register what they create at `// navree:modules`,
   `// navree:wire` and `// navree:policies`. Keep those markers.
2. **Deny by default.** Every route has `#[public]`, `#[authenticated]`,
   `#[roles(..)]`, `#[permissions(..)]` or `#[can(..)]`; without one it does
   not compile. Never make a route public to silence a 401 or 403.
3. **Inject, don't construct.** Services are `#[injectable]` structs with
   `Arc<T>` fields, registered in a module's `providers`. Cross-module use
   needs `imports` on one side and `exports` on the other.
4. **Validate at the edge.** Bodies and queries arrive through
   `Valid<Json<T>>` / `Valid<Query<T>>` with garde rules on `T`.
5. **One error shape.** Handlers return `Result<T>` (`AppError`); every
   error is `{ "statusCode", "message", "error" }`, plus `errors` for
   validation failures.
6. **The request is a unit of work.** Services take `&Uow`; writes go
   through `Repo::<E>::insert/update/delete`; everything commits when the
   response status is below 400 and rolls back otherwise.
7. **Test every change** with `navree::testing::TestApp`, then run
   `cargo fmt`, `cargo clippy --all-targets -- -D warnings` and `cargo test`.

## Imports

`use navree::prelude::*;` brings the macros, `Arc`, the kernel types
(`AppContext`, `ModuleBuilder`, `Module`, `Navree`, `BoxError`), `Result`
(= `navree::Result<T, AppError>`, shadowing std's), `AppError`, `Json`,
`Path`, `Query`, `Created`, `NoContent`, `Uow`, `Repo`, `Page`, `PageQuery`,
`Valid`, `ValidAsync`, the access markers (`Read`, `Create`, `Update`,
`Delete`, `Manage`, `Accessible`, `Authorized`, `Abilities`, `Subject`),
`CurrentUser`, every `*Module` and every `ModuleBuilder` extension (`m.job`,
`m.listen`, `m.migrations`, `m.schedule`, `m.templates`, ...). Test helpers
(`navree::testing`) are not in the prelude. Name the error type when it is
not `AppError`: `Result<(), BoxError>`.

## Commands

```sh
navree dev                                  # serve on 127.0.0.1:8080 and rebuild on save
cargo test                                  # each test gets its own SQLite database
cargo clippy --all-targets -- -D warnings
navree check                                # validate the module graph (no boot)
navree routes                               # every route with its access rule
navree migrate                              # pending migrations
navree list                                 # every command of the app
```

`navree <cmd>` runs `cargo run --quiet -- <cmd>` for anything that is not
`new`, `make:*` or `dev`.

## Which skill to load

| Task | Skill |
|---|---|
| Start a project, run it, understand the generated files | `navree-new-app` |
| Add or wire a service, module, import/export; boot errors such as "cannot resolve" or "private to module" | `navree-modules-and-di` |
| Add an endpoint, controller, path/query parameter, response type; 401/403/404/405 on a route | `navree-controllers-and-routing` |
| Request DTOs, validation rules, `unique`/`exists`, custom messages | `navree-validation` |
| Return a status or error, map an error type, headers, downloads, API resources | `navree-errors-and-responses` |
| Tables, entities, queries, `Repo`, transactions, migrations, seeders, factories, pagination | `navree-database` |
| Sign-in, tokens, current user, password reset, email verification, two-factor, API keys | `navree-auth` |
| Roles, permissions, ability rules, record checks, per-user list filters | `navree-access-control` |
| Background jobs, workers, retries, cron schedules | `navree-queues-and-scheduling` |
| Events and listeners, live updates over WebSockets, private channels | `navree-events-and-broadcasting` |
| Emails, templates, notifications | `navree-mail-and-notifications` |
| Uploads, downloads, disks, signed links | `navree-storage` |
| Caching, locks, calling third-party APIs | `navree-cache-and-http-client` |
| API docs, OpenAPI document, TypeScript clients | `navree-openapi` |
| Settings, environment variables, APP_KEY, config sections | `navree-config` |
| Writing or fixing tests, fakes | `navree-testing` |
| Any `navree make:*` generator, console commands | `navree-cli-generators` |
| Shipping to production | `navree-deployment` |
| Security review before a release | `navree-security-checklist` |
| Multi-tenancy, workspaces, members and invitations | `navree-workspaces` |

## Common mistakes

- Writing a module by hand and forgetting to add it to `src/app.rs`, or
  adding a provider without registering it: the boot names the module and
  the type. Use the generators.
- Depending on another module's service without importing that module (or
  without the other module exporting it).
- Building SQL strings or writing through `db.conn()` inside a request: use
  `uow.tx()` and `Repo`, or the request's writes escape its rollback.
- Using `Utc::now()` where tests need a fixed time: inject `Arc<dyn Clock>`.
- Setting a key in `.env` that `config/app.toml` also sets: `.env` only
  fills keys the TOML file leaves out. Real environment variables win.
- Calling `ActiveModel::save` on a Navree entity: use `Repo::insert` for new
  rows and `Repo::update` for existing ones.

## More

- `references/crates.md`: every facade module, its crate, its module type
  and its configuration section.
- Docs: https://navree.com/docs/ (as text for models: https://navree.com/llms.txt).
- The reference app: `apps/crm-api` in https://github.com/NavreeHQ/navree.
