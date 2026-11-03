# From spec items to Navree

Which Navree feature takes each kind of spec row, and which skill the
builder loads to build it. Everything named here exists in this version of
Navree. A spec item with no row here is built from plain parts (a model, a
service, a route) or is a task of its own; never assume a feature that
isn't listed. When in doubt, check the skill named, which is checked
against the code.

## Base layer and CRUD

| Spec item | Navree | Skill |
|---|---|---|
| A module | `navree make:module <name>` (`--crud` for five JSON routes, request types and tests) | `navree-cli-generators`, `navree-modules-and-di` |
| An entity | a `#[derive(Model)]` struct and its migration: `navree make:model <module> <Model> --migration --factory --policy` | `navree-orm`, `navree-migrations` |
| Key | `id: Uuid` (a UUID v7 on create); old key in `legacy_id`, unique per workspace | `navree-orm` |
| Text, long text | `String`; `length` rules | `navree-validation` |
| Money | `i64` minor units plus a `currency` column; or `Decimal` with `cast = "decimal(N)"` | `navree-orm` |
| Date, instant | `NaiveDate`; `DateTime<Utc>` | `navree-orm`, `navree-time-zones` |
| Enum | a unit enum with `cast = "string_enum"` (or `int_enum`) | `navree-orm` |
| Reference | `Uuid` column, `#[relations]` (`belongs_to`, `has_many`, ...), foreign key in the migration, `exists` rule | `navree-orm`, `navree-migrations`, `navree-validation` |
| JSON blob | `cast = "json"` with a typed struct | `navree-orm` |
| Secret or personal field | `cast = "encrypted"`; `#[model(hidden)]` | `navree-orm` |
| Phone, email, URL, country, rich text | `Phone` (E.164), `email`, `url`, `Country`, `SafeHtml` | `navree-validation` |
| Soft delete | `#[model(soft_deletes)]` and `deleted_at` | `navree-orm` |
| Never deleted | no delete route; a status instead | `navree-controllers-and-routing` |
| Field rules | `#[derive(Validate)]` request types with `Valid<Json<T>>`, `Valid<Query<T>>`; `unique`/`exists` with `ValidAsync` | `navree-validation` |
| Refusals with a code | `AppError::conflict(..).with_code("invoice_not_voidable")` | `navree-errors-and-responses` |
| CRUD API | controller routes with access attributes, a service taking `&Uow`, `#[api]` docs | `navree-controllers-and-routing`, `navree-database` |
| Back-office CRUD | a Helm resource: `navree make:resource <module> <Model>`; fields, filters, actions, metrics | `navree-helm` |
| Lookup tables shared by every tenant | a model classified with `global_entity`, filled by a seeder (`navree make:seeder`) | `navree-database`, `navree-module-conformity` |
| Lookup per tenant | a workspace model; or a setting | `navree-workspaces`, `navree-settings` |
| Audit of changes | writes through the unit of work record change events; the audit trail keeps them | `navree-database` (and the docs page on the audit log) |
| Integrity rules | unique indexes and foreign keys in the migration; service checks for the rest | `navree-migrations` |

## Lists, forms and screens

| Spec item | Navree | Skill |
|---|---|---|
| A server-side list (API) | a route taking `PageQuery`, a filter type through `Valid<Query<T>>` and `Accessible<M, Read>`; the query with `when_some`, `order_by`, `with(..)`, `paginate`; returns `Page<T>` (`{data, meta}`) | `navree-controllers-and-routing`, `navree-orm`, `navree-access-control` |
| Cursor paging for feeds and very large tables | `cursor_paginate` and `CursorPage` | `navree-database` |
| Related columns, counts, sums | `with("relation")`, `load_count`, `load_sum` | `navree-orm` |
| The list on a page rendered on the server | navree-ui: `#[derive(TableRow)]`, `TableQuery<R>` and `DataTable`; search, filters, sort, pages, bulk actions and export from one definition | `navree-design-to-ui` |
| The list on a web front end | `@navree/next` `DataTable` with `useListQuery`; `@navree/nuxt` `<NavreeDataTable>` with `useListQuery` | `navree-frontend-next`, `navree-frontend-nuxt` |
| A back-office list | a Helm resource: search, filter chips, sort, pages, bulk actions, CSV export | `navree-helm` |
| Options of a select | the enum; or a lookup endpoint | `navree-controllers-and-routing` |
| A combobox over a large table | an options route searching on the server (`mistakes.md`, entry 2); Helm's `Field::belongs_to` and `Filter::belongs_to`; in navree-ui pages a select from `FormView::options` for a small set, the combobox is coming | `navree-controllers-and-routing`, `navree-helm` |
| A form on a page rendered on the server | navree-ui: `#[derive(Validate, Form)]`, `FormView`, `Submit<T>`; the type picks the control, errors land under fields, with or without JavaScript | `navree-design-to-ui`, `navree-validation` |
| Form errors under fields | the 400 `errors` shape; `applyFieldErrors` in `@navree/next` | `navree-validation`, `navree-frontend-next` |
| Bulk actions | a route taking ids, checking each record (`Authorized` or the ability), saving each through the unit of work; queued with progress when large (operations); Helm actions | `navree-access-control`, `navree-queues-and-scheduling`, `navree-helm` |
| Exports | `navree-sheets`: `Download::csv`/`xlsx`, named exports queued with a signed link | `navree-sheets` |
| Imports of spreadsheets | `navree-sheets` importers: mapping, staging preview, duplicate modes, error files | `navree-sheets` |
| Screens and their states | navree-ui pages in the app shell (`AppShell`, components, `Empty`, `Skeleton`, `Region`), built from a design or the old screens; or the front end (`@navree/next`, `@navree/nuxt`); or Helm pages (`HelmPage`) | `navree-design-to-ui`, `navree-frontend-next`, `navree-frontend-nuxt`, `navree-helm` |
| What a role may see in the page | `GET /api/me/abilities` (the user's rules, in the CASL format front ends read), `AbilityProvider`/`<Can>`, `useAbility` | `navree-access-control`, `navree-frontend-next` |
| Translated texts | `navree-i18n` (Fluent, `t!`, translated validation) | `navree-i18n` |
| Files on records | `navree-storage` disks, `navree-media` collections | `navree-storage`, `navree-media` |
| Search box | `#[derive(Searchable)]` and an engine; or a `like` filter for small tables | `navree-search` |

## Services, workers, workflows

| Spec item | Navree | Skill |
|---|---|---|
| A business rule | a method of an `#[injectable]` service taking `&Uow`, called by routes, jobs and commands alike | `navree-modules-and-di`, `navree-database` |
| A rule on every save | model events and observers | `navree-orm` |
| Work after commit | `uow.after_commit(..)`; jobs dispatched in a request are queued only if it commits | `navree-database`, `navree-queues-and-scheduling` |
| A worker | a `Job` (`navree make:job`): `JobOptions` (attempts, backoff, timeout, `unique_for_secs`); a guard record for idempotency | `navree-queues-and-scheduling` |
| A schedule | `m.schedule(..)`: `daily_at`, `cron`, `in_zone`, `per_workspace_local`, `without_overlapping` | `navree-queues-and-scheduling`, `navree-time-zones` |
| Long work users watch | batches and operations (progress, cancel) | `navree-queues-and-scheduling` |
| A manual script | a console command (`navree make:command`) | `navree-cli-generators` |
| Events between modules | events and listeners | `navree-events-and-broadcasting` |
| Live updates on screens | broadcasting over WebSockets | `navree-events-and-broadcasting` |
| A workflow's states and moves | an enum status field; one service method per transition that checks the state and refuses with a code; one custom action per user move (`ActionMarker`) with `#[can(.., from = "id")]`; ability rules with `field("status")` conditions | `navree-orm`, `navree-access-control` |
| Timers and waits ("3 days after", "until signed") | a durable `#[workflow]` with `sleep`, `sleep_until`, `wait_for` | `navree-workflows` |
| Rules users build themselves | automations: `#[trigger]`, `#[condition]`, `#[action]` | `navree-workflows` |
| Approvals with a person in the loop | a status move by the approver's custom action; a durable workflow when it waits | `navree-access-control`, `navree-workflows` |

## Reporting

| Spec item | Navree | Skill |
|---|---|---|
| A metric | an aggregate query (`count`, `sum`, `avg` on columns; `Statement` for raw SQL) in a service | `navree-orm`, `navree-database` |
| By local day, week, month | the workspace's zone, `LocalDay` and `DayRange` turned into UTC bounds; `AT TIME ZONE` with a checked zone | `navree-time-zones` |
| Large time series | TimescaleDB hypertables (the default database) | `navree-database` |
| Back-office figures and charts | Helm `Metric::value`, `trend`, `partition` | `navree-helm` |
| PDF reports | `navree-documents` (HTML templates or `PdfDocument`) | `navree-documents` |
| Spreadsheet reports | `navree-sheets` exports | `navree-sheets` |
| Scheduled report mail | a schedule, a job, a mailable with the file | `navree-queues-and-scheduling`, `navree-mail-and-notifications` |

## Integrations

| Spec item | Navree | Skill |
|---|---|---|
| Calls to another API | `HttpClient`: timeouts, retries of GET, PUT, DELETE and the like (never POST or PATCH), a guard against calls to internal addresses, `HttpClient::fake()`; a POST that must be retried runs in a job with an idempotency key | `navree-cache-and-http-client` |
| A tenant's account on another system | `navree-integrations` connections (Microsoft 365, Google Workspace, Slack, OAuth 2.0 APIs, API keys) | `navree-integrations` |
| Webhooks we send | `navree-webhooks` outgoing: signed, retried, delivery log | `navree-webhooks` |
| Webhooks we receive | `VerifiedWebhook<S>` (Stripe, GitHub, Paddle, Standard Webhooks, ... or `Hmac`, or a verifier of your own) | `navree-webhooks` |
| Mail in, replies | `navree-inbound-mail` | `navree-inbound-mail` |
| Payments and subscriptions | `navree-billing` (Stripe, Paddle) | `navree-billing` |
| Messages between services | `navree-microservices` (outbox, inbox, transports) | `navree-microservices` |
| Directory and single sign-on | `navree-directory`, `navree-sso`, `navree-social` | those skills |

## Cross-cutting

| Spec item | Navree | Skill |
|---|---|---|
| Sign-in, sessions, two-factor, password reset | `navree-auth` | `navree-auth` |
| Role × action × scope | permissions `<prefix>.<action>` on roles; rules in `src/abilities.rs` with record conditions; `#[can]`, `Authorized`, `Accessible`; field rules | `navree-access-control` |
| App-wide staff (support) | app roles; Helm staff access to workspaces (`helm_staff_workspaces`, audited) | `navree-access-control`, `navree-helm` |
| Tenant | a workspace (organizations above, partitions inside); `#[model(workspace)]` or `workspace_scoped` models; isolation by row, schema or database | `navree-workspaces` |
| Proof of isolation | `app.isolation_suite::<M>()`, `navree check` (`workspace.*`) | `navree-module-conformity` |
| Notifications | mail, database, broadcast channels; Slack, SMS, push; preferences per category | `navree-mail-and-notifications`, `navree-notification-channels`, `navree-notification-preferences` |
| Settings | typed settings per app, organization, workspace or user | `navree-settings` |
| Feature switches | `navree-flags` | `navree-flags` |
| Time zones | user and workspace zones, local schedules, holidays | `navree-time-zones` |
| Errors in production | `navree-lighthouse` | `navree-lighthouse` |

## Data migration and tests

| Spec item | Navree | Skill |
|---|---|---|
| Reading the old database | a named connection (`[database.connections.legacy]`, `db.connection("legacy")`) and `Statement`s | `navree-database` |
| The import | console commands per table, batched, writing through units of work (`data-migration.md`) | `navree-cli-generators`, `navree-database` |
| Writing into a workspace from a command or job | `navree::orm::workspace::scope(workspace_id, ..)` (documented in `crates/navree-orm/src/workspace.rs`; `data-migration.md` shows it) | `navree-workspaces` |
| Spreadsheet data | `navree-sheets` importers | `navree-sheets` |
| Test data | factories and seeders | `navree-orm`, `navree-database` |
| Acceptance tests | `TestApp`, HTTP tests, acting as users, fakes | `navree-testing` |
| Browser journeys | Chrome driven from Rust tests (`TestServer`, `FrontEnd`) | `navree-browser-testing` |
| Query counts | `navree-logbook` in tests | `navree-logbook` |
| The whole gate | `navree gate` (fmt, clippy, tests, check, front end) | `navree-module-conformity` |
| Reading the new app's routes, schema and data | Navigator (`navree mcp`) | `navree-navigator` |

## Building a layer: the order inside a module

1. Models and migrations (`navree-orm`, `navree-migrations`), lookups and
   seeders.
2. Access: the subject, permissions, rules in `src/abilities.rs`
   (`navree-access-control`); the workspace classification
   (`navree-workspaces`).
3. Services with their rules, test first (`navree-testing`).
4. Routes: CRUD, lists, options, custom actions; `#[api]` docs.
5. Workers and schedules; workflows and timers.
6. Integrations, with fakes in the tests.
7. Reports and exports.
8. Screens: navree-ui pages (the `navree-design-to-ui` skill, phases 3 to 6),
   the front end, or Helm.
9. The isolation suite, `navree check`, `navree gate`.
