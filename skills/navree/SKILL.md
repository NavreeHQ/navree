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
   error is `{ "statusCode", "message", "error", "code" }`, plus `errors`
   for validation failures. A refusal users see gets its own `code`
   (`.with_code("campaign_already_running")`), which front ends translate.
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
| Sign-in, tokens, current user, password reset, email verification, two-factor, sessions, step-up, impersonation, magic links, passkeys, platform admins, token exchange, API keys | `navree-auth` |
| Roles, permissions, ability rules, record checks, per-user list filters | `navree-access-control` |
| Background jobs, workers, retries, batches, progress operations, cron schedules | `navree-queues-and-scheduling` |
| Events and listeners, live updates over WebSockets, private channels | `navree-events-and-broadcasting` |
| Emails, templates, notifications | `navree-mail-and-notifications` |
| SES, Mailgun, Postmark or Resend; customers' own mail transports or domains; bounces, complaints, suppressions; the mail log; holding mail for approval; open and click tracking; SPF, DKIM and DMARC | `navree-mail-delivery` |
| Uploads, downloads, disks, signed links | `navree-storage` |
| Caching, locks, calling third-party APIs | `navree-cache-and-http-client` |
| API docs, OpenAPI document, TypeScript clients | `navree-openapi` |
| A Next.js web front end: sign-in through a BFF, server components, list screens, real time, the dark theme | `navree-frontend-next` |
| A Nuxt or Vue web front end on the `@navree/nuxt` module | `navree-frontend-nuxt` |
| Settings, environment variables, APP_KEY, config sections | `navree-config` |
| Writing or fixing tests, fakes | `navree-testing` |
| Any `navree make:*` generator, console commands | `navree-cli-generators` |
| Shipping to production | `navree-deployment` |
| Security review before a release | `navree-security-checklist` |
| Signing a JWT another service verifies, verifying a token against a provider's JWKS, publishing a JWKS, loading a `.p8` or PEM key | `navree-jose` |
| Finishing or reviewing a module: `navree check`, cross-workspace isolation suites, `navree gate` before a push | `navree-module-conformity` |
| Multi-tenancy, workspaces, members, invitations and demo workspaces | `navree-workspaces` |
| Time zones of users and workspaces, reports by local day, messages in working hours, holidays, schedules at local time | `navree-time-zones` |
| Paid plans, subscriptions, trials, usage billing, invoices, Stripe or Paddle webhooks | `navree-billing` |
| Production errors and slow requests: issues, reporting handled errors, alerts, the `/_lighthouse` dashboard, reading issues from a script or agent | `navree-lighthouse` |
| Running queue workers in production, balancing them across queues, the queue dashboard, failed-job retries from a UI, pausing workers, long-wait alerts | `navree-fleet` |
| Local PostgreSQL, Redis, Meilisearch, Mailpit, MinIO or NATS for development and CI (`dock.toml`, `navree dock`) | `navree-dock` |
| Provisioning Ubuntu servers and zero-downtime deploys, rollbacks, deploy secrets, container images, push-to-deploy (`harbor.toml`, `navree deploy`) | `navree-harbor` |
| Sign in with Google, GitHub, Apple, Microsoft or an OpenID Connect provider | `navree-social` |
| A customer's company identity provider (Okta, Entra ID, Google Workspace, AD FS) per organization or workspace: SAML or OpenID Connect, verified domains, forcing SSO, provisioning members | `navree-sso` |
| Sending webhooks to customers' URLs, receiving Stripe, GitHub, Paddle or other signed webhooks | `navree-webhooks` |
| Feature flags: hiding a feature until it is released, gradual rollouts, a kill switch, flags for the front end | `navree-flags` |
| Settings an app, organization, workspace or user can change, inherited down that chain; showing where a value comes from; settings screens and resets | `navree-settings` |
| Search box, command palette, indexes, Meilisearch/Typesense/Tantivy, vector search | `navree-search` |
| Receiving requests, queries, jobs, mail or log lines in your own code; tracing with OpenTelemetry; trace ids; asserting on events or spans in tests | `navree-telemetry` |
| A GraphQL API: resolvers, dataloaders, subscriptions | `navree-graphql` |
| A gRPC API (tonic services, `.proto` files, gRPC-Web, Connect), calling another gRPC service | `navree-grpc` |
| Tools, resources or prompts for Claude, ChatGPT, Cursor and other MCP clients (`#[mcp_server]`, `/mcp`, `mcp:serve`) | `navree-mcp` |
| AI features: summaries, drafts, extraction, an assistant that calls your services, answers from your own data, embeddings and vector search | `navree-ai` |
| Several processes or servers sharing the cache, locks, jobs, sessions, rate limits or WebSocket broadcasts; Redis commands | `navree-redis` |
| Seeing what a request, job or scheduled task did (`/_logbook`), finding N+1 queries, counting queries in a test, replaying a request | `navree-logbook` |
| Messages and events between services (NATS, Kafka, RabbitMQ, Redis Streams, MQTT, TCP), splitting a module into a service, `MicroFake` tests | `navree-microservices` |
| An admin panel or back office: list, edit and act on records at `/admin` (`make:resource`, Helm) | `navree-helm` |
| Connecting a coding agent to the app (`navree mcp`, Navigator's routes, module graph, schema, read-only SQL, logs and docs tools), installing the skills (`navree skills:install`) | `navree-navigator` |
| Testing a user journey in a real browser: sign-in forms, the front end and its BFF, drag and drop, phone layouts, failure screenshots | `navree-browser-testing` |
| Measuring or lowering per-request cost: the pipeline benchmarks, pool and transaction settings, hot-path habits | `navree-performance` |
| PDF or Word documents: certificates, invoices, reports, exports; Arabic or other right-to-left documents; merging or stamping PDFs | `navree-documents` |

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
