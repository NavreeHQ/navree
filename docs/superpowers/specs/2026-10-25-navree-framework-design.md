# Navree framework + Navree CRM — build design

Date: 2026-10-25 · Source PRD: `docs/prd/navree-prd.md` (read it; this document only records the decisions layered on top of it).

## 1. Intent

Build Navree, the Laravel-class, NestJS-structured Rust framework described in the PRD, as small crates ("building blocks") assembled bottom-up with test-driven development. Then prove it by building a small CRM on top: a Navree API (`apps/crm-api`) and a Next.js front end (`apps/crm-web`).

"Done" means `cargo test --workspace` is green, `cargo clippy --workspace --all-targets -- -D warnings` is clean, the CRM API's integration tests pass, and Playwright end-to-end tests drive the CRM web app against the real CRM API.

## 2. Scope decisions (calls made by the builder, not the user)

The PRD is a 60-engineer-month plan with 70 parity rows. This build covers the PRD's Phase 0 and Phase 1, plus the Phase 2 modules the CRM needs.

**In scope (crates):** navree (facade), navree-kernel, navree-contracts, navree-macros, navree-config, navree-log, navree-crypto, navree-http, navree-validation, navree-db, navree-session, navree-auth, navree-access, navree-audit, navree-cache, navree-events, navree-queue, navree-schedule, navree-view, navree-mail, navree-notify, navree-storage, navree-http-client, navree-api, navree-observe, navree-realtime, navree-testing, navree-cli.

**Deferred, with the reason:**

| Deferred | Why |
|---|---|
| navree-redis and every Redis driver | No Redis server on the build machine, so nothing could be tested. Every module exposes a driver trait so a Redis driver can be added without API changes |
| apalis queue driver | Native database driver plus memory driver cover the PRD's fallback; apalis can be a later driver |
| Passkeys (webauthn-rs), social login, OAuth2 server | webauthn-rs needs OpenSSL on Windows; the other two are Phase 3–4 in the PRD |
| navree-i18n, navree-flags, navree-billing, navree-ai, navree-grpc, navree-transport, navree-export, navree-search, navree-admin, navree-graphql | Phase 2–4 modules with external services; not needed by the CRM |
| Nuxt kit, Teams, multi-tenancy, OpenTelemetry/Sentry, request inspector | Phase 2–3 |
| PostgreSQL-only behaviour verified by tests | No PostgreSQL on the build machine. Code paths for PostgreSQL are written behind `DbBackend` checks (FOR UPDATE, SKIP LOCKED, advisory locks), but tests run on SQLite. SQLite is the PRD's "development and tests" database |

## 3. The hierarchy (what depends on what)

Eight layers. A crate may depend only on crates in lower layers (dev-dependencies included). Runtime dependencies must also match the table below. A **dev-dependency** may point at any navree crate in a lower layer (for example navree-view as a dev-dependency of navree-notify or navree-auth, so tests can boot MailModule, which needs ViewModule); the dependency-graph test encodes both rules. `crates/navree/tests/dependency_graph.rs` enforces the allowed edges below on every test run.

```
L7  apps/crm-api (Navree app)                   apps/crm-web (Next.js 16)
L6  navree (facade: re-exports + prelude)       navree-cli (binary `navree`; no runtime navree deps)
L5  navree-auth
L4  navree-session  navree-validation  navree-queue  navree-audit  navree-access  navree-schedule  navree-notify
L3  navree-db  navree-testing  navree-observe  navree-api  navree-mail  navree-realtime  navree-storage
L2  navree-http  navree-log  navree-crypto  navree-events  navree-view  navree-cache  navree-http-client
L1  navree-contracts  navree-config
L0  navree-kernel                                navree-macros (proc-macro; no runtime navree deps)
```

### Allowed runtime dependency edges (besides external crates)

| Crate | May depend on |
|---|---|
| navree-kernel | — |
| navree-macros | — (proc-macro) |
| navree-contracts | kernel |
| navree-config | kernel, contracts |
| navree-log, navree-crypto, navree-events, navree-view, navree-cache, navree-http-client | kernel, contracts, config |
| navree-http | kernel, contracts, config |
| navree-db, navree-testing, navree-observe, navree-api | kernel, contracts, config, http |
| navree-mail | kernel, contracts, config, view |
| navree-realtime, navree-storage | kernel, contracts, config, http, crypto |
| navree-session | kernel, contracts, config, http, crypto, db |
| navree-validation, navree-audit | kernel, contracts, config, http, db |
| navree-queue, navree-schedule | kernel, contracts, config, db |
| navree-access | kernel, contracts, config, http, db, cache |
| navree-notify | kernel, contracts, config, http, db, mail |
| navree-auth | kernel, contracts, config, http, db, crypto, session, validation, cache, mail, access (only for `impl Subject for user::Entity`) |
| navree | every navree-* runtime crate + navree-macros |
| navree-cli | — |

Cross-cutting needs go through `navree-contracts` (PRD: AuditSink, EventSink, Broadcaster, SessionStore, HealthCheck), extended here with `Principal`, `AuthState`, `Secret<T>`, `RequestId`, `Clock`, and the job contract (`Job`, `JobOptions`, `Dispatcher`, `JobRegistration`) so mail, events, schedule and notify can queue work without depending on navree-queue.

### Build waves (what runs in parallel)

| Wave | Agents (one per row item) | Needs |
|---|---|---|
| W0 Foundation | skeleton → kernel+contracts → config → review | — |
| W1 Platform | http, log, crypto, events, view, cache, http-client, access-engine (pure CASL rules), macros-di | W0 |
| W2 Data & surface | db, macros-http, testing, observe, api, mail, realtime, storage | W1 |
| W3 Capabilities | session, validation, queue, audit, access-module, schedule, notify | W2 |
| W4 Identity | auth-core, cli | W3 |
| W5 Assembly | auth-extras, facade + full-stack tests | W4 |
| W6 CRM | crm-scaffold → companies+contacts ‖ deals ‖ activities+dashboard → crm-integrate | W5 |
| W7 Ship | crm-web integration + Playwright E2E, security review, PRD-conformance review, fixer | W6 |
| (parallel from W0) | crm-web build against the API contract in §7 | contract only |

Every wave ends with an integrator agent: full workspace test + clippy + dependency-graph test, fixes cross-crate breakage, then commits the wave on branch `navree/framework`.

## 4. Global technical decisions

- Rust 2024 edition, `rust-version = "1.95"`, toolchain 1.99.1 (installed). `#![forbid(unsafe_code)]` in every crate.
- Versions: use the PRD's versions (they match crates.io on 2026-10-25: axum 0.8.9, tower-http 0.7.1, sea-orm 2.0.4, garde 0.23.0, croner 4.0.2, utoipa 6.0.0, minijinja 3.0.3, …). All external versions live in root `[workspace.dependencies]`; crates use `dep = { workspace = true }`.
- TLS: rustls with the `ring` provider wherever a crate offers the choice (aws-lc-sys may need cmake/NASM on Windows).
- Time: `chrono` (SeaORM's `with-chrono`). IDs: UUID v7 (`uuid` with `v7`, `serde`).
- Errors: `thiserror` per crate; `navree_kernel::BoxError` across boundaries; HTTP errors are `navree_http::AppError` rendered in the NestJS error format (see §10).
- Database: SeaORM 2.0 with `sqlx-sqlite` (tests and dev) and `sqlx-postgres` behind a `postgres` feature. Tests use a fresh temporary SQLite **file** per test (in-memory SQLite gives every pool connection its own database).
- Configuration: `.env` + TOML files + environment variables. Keys are nested with `__`, prefix `APP_` (e.g. `APP_DATABASE__URL` → `database.url`). Navree's own `.env` parser never calls `std::env::set_var` (unsafe in edition 2024); `.env` entries are fed to config-rs as a source.
- Framework crates do **not** use navree-macros; they hand-write `impl Module` / `impl Injectable` / `impl Controller` (the PRD's escape hatch). Apps and the facade's tests use the macros. This lets every crate build before the macros exist.
- Every configurable module offers `for_root(options)` and `from_config()` (reads its section from `navree_config::Config` at boot, like NestJS `forRootAsync`).
- Request pipeline, outermost first: request ID → tracing → CORS → panic catcher → timeout → security headers → compression (skipped for `text/event-stream`) → maintenance mode → per-IP rate limit → body limit → **[Session slot]** (navree-session: session + CSRF) → **[AuthContext slot]** (navree-http calls the registered `Authenticator`, inserts `AuthState`) → **[Uow slot]** (navree-db: lazy unit of work, commit on status < 400, rollback otherwise, run to completion in a spawned task) → router → per-route layers: require authentication (unless public) → per-user throttle → custom guards → `RouteAuthorizer` (navree-access) → audit action → handler.
- Deny by default: a route with no `#[public]`, `#[roles]`, `#[permissions]` or `#[can]` is a compile error in `#[controller]` and a boot error in navree-http for hand-registered routes.
- Logging on by default; secrets redacted (`Secret<T>`, configured key names).

## 5. Build environment conventions (every agent)

- Windows 11, Git Bash for shell commands. Repo root: `C:\Users\bopar\.t3\projects\navree`.
- `.cargo/config.toml` sets `rustc-wrapper = "sccache"`, so dependency builds are shared through sccache.
- **Each agent uses its own target directory** to avoid Cargo's build-directory lock: prefix every cargo command with `CARGO_TARGET_DIR=target/agents/<agent-label>`, e.g. `CARGO_TARGET_DIR=target/agents/http cargo test -p navree-http`.
- Only edit files inside the crate(s) your task owns. Never edit another crate's source; if you need a change there, describe it in your final report. Root `Cargo.toml` edits are limited to adding a missing external crate to `[workspace.dependencies]` (re-read the file immediately before editing; others may be editing too).
- TDD: write the failing test, run it and see it fail for the expected reason, write the minimal code, see it pass, refactor. Unit tests inline under `#[cfg(test)]`; integration tests under `crates/<crate>/tests/`.
- Before reporting done: `cargo test -p <crate>`, `cargo clippy -p <crate> --all-targets -- -D warnings`, `cargo fmt -p <crate>` all clean.
- Workers do not commit; the wave integrator commits.

## 6. CRM product design

**Users:** a small sales team. Roles: `admin` (everything incl. users, roles, audit log), `manager` (all customer records, reassign deals, export contacts, reports), `rep` (reads all companies/contacts/deals/activities; creates records; edits and deletes only records they own; cannot delete deals that are won or lost).

**Records:** companies, contacts (belong to an optional company), deals (belong to a company, optional contact; move through stages lead 10% → qualified 25% → proposal 50% → negotiation 75% → won 100% | lost 0%), activities (note, call, email, meeting, task; attached to a company, contact or deal; tasks have due dates and can be completed).

**Background work:** when a deal is assigned to someone else or won, the owner is notified (database + mail, queued). A scheduled task at 08:00 UTC daily notifies assignees of tasks due today. `audit:prune` runs daily.

**Real-time:** deal changes broadcast on `private-crm.deals` (event `deal.updated`) so open boards refresh.

**Demo data (`db:seed`):** admin@navree.test, manager@navree.test, rep1@navree.test, rep2@navree.test, all with password `password123`; ~20 companies, ~50 contacts, ~30 deals across stages, ~60 activities.

## 7. CRM API contract (crm-web builds against this; crm-api must match it)

Conventions: JSON; UUID ids; RFC 3339 UTC timestamps; money as integer `amount_cents` + `currency`. Every resource carries `"__type"` (`"Company" | "Contact" | "Deal" | "Activity" | "User"`) for CASL. Lists return `{ "data": [...], "meta": { "current_page", "per_page", "total", "last_page" } }` with `page` (default 1) and `per_page` (default 15, max 100) query params. Errors use the NestJS format: `{ "statusCode", "message" (string or string[]), "error" (reason phrase) }`, content type `application/json`; validation failures are 400 with `message: string[]` plus `errors: { "field": ["message"] }`; statuses 400, 401, 403, 404, 409, 429; the request id is in the `X-Request-Id` response header.

Auth is NestJS-style JWT (see §10). The browser never holds tokens: crm-web's Next.js server is a backend-for-frontend (BFF). Its route handlers forward `/api/*` and `/broadcasting/*` to the Navree API (`NAVREE_API_URL`, default `http://127.0.0.1:8080`) with `Authorization: Bearer <access token>` taken from an httpOnly cookie; `POST /api/auth/login` through the BFF stores the access and refresh tokens in httpOnly cookies and returns only `{user}`; on a 401 the BFF calls `POST /api/auth/refresh` once, updates the cookies and retries; logout revokes the refresh token and clears the cookies. The BFF rejects non-GET requests whose `Origin` is not its own (CSRF defence for the cookie hop).

| Method & path | Body / query | Response |
|---|---|---|
| POST /api/auth/login | `{email, password}` | 200 `{user: User, access_token, refresh_token, token_type: "Bearer", expires_in}` (the BFF keeps the tokens and returns `{user}`); 401 "Invalid credentials"; 429 when throttled |
| POST /api/auth/refresh | `{refresh_token}` | 200 new token pair (rotated); 401 when invalid, expired or reused |
| POST /api/auth/logout | `{refresh_token?, all?}` (bearer required) | 204 |
| GET /api/auth/me | — | 200 `User` or 401 |
| GET /api/me/abilities | — | 200 CASL rule array `[{action, subject, conditions?, fields?, inverted?}]`, with ETag |
| GET /api/v1/me | — | `{id, name, email, roles: string[]}` |
| GET /api/v1/users | `search?` | Page of `User` (any signed-in user; for owner pickers) |
| POST /api/v1/users | `{name, email, password, role}` | 201 `User` (admin) |
| PATCH /api/v1/users/{id} | `{name?, role?}` | `User` (admin) |
| GET /api/v1/companies | `search?, owner_id?, industry?, sort? (name, -name, created_at, -created_at), page, per_page` | Page of `Company` |
| POST /api/v1/companies | `{name*, domain?, industry?, size?, phone?, website?, city?, country?, owner_id?}` | 201 `Company` |
| GET /api/v1/companies/{id} | — | `Company` plus `contacts_count, open_deals_count, open_deals_amount_cents` |
| PATCH /api/v1/companies/{id} | any Company input field | `Company` |
| DELETE /api/v1/companies/{id} | — | 204 |
| GET /api/v1/contacts | `search?, company_id?, status?, owner_id?, page, per_page` | Page of `Contact` |
| POST /api/v1/contacts | `{first_name*, last_name*, email?, phone?, title?, status? (default lead), company_id?, owner_id?}` | 201 `Contact` |
| GET, PATCH, DELETE /api/v1/contacts/{id} | | `Contact` / `Contact` / 204 |
| GET /api/v1/contacts/export | same filters as list | `text/csv` (admin, manager) |
| GET /api/v1/deals | `stage?, owner_id?, company_id?, search?, page, per_page` | Page of `Deal` |
| GET /api/v1/deals/board | `owner_id?` | `{stages: [{stage, label, probability, count, amount_cents, deals: Deal[]}]}` in stage order |
| POST /api/v1/deals | `{title*, company_id*, contact_id?, amount_cents* (≥ 0), currency? (USD), stage? (lead), expected_close_date? (YYYY-MM-DD), owner_id?}` | 201 `Deal` |
| GET, PATCH, DELETE /api/v1/deals/{id} | | `Deal` / `Deal` / 204 |
| POST /api/v1/deals/{id}/move | `{stage*, position?, lost_reason? (required when stage = lost)}` | `Deal` |
| GET /api/v1/activities | `subject_type?, subject_id?, kind?, assignee_id?, due? (overdue, today, upcoming), completed? (true, false), page, per_page` | Page of `Activity` |
| POST /api/v1/activities | `{kind*, subject*, body?, due_at?, subject_type* (company, contact, deal), subject_id*, assignee_id?}` | 201 `Activity` |
| PATCH /api/v1/activities/{id} | any input field | `Activity` |
| POST /api/v1/activities/{id}/complete | — | `Activity` |
| DELETE /api/v1/activities/{id} | — | 204 |
| GET /api/v1/dashboard | — | `{pipeline: [{stage, count, amount_cents}], won_this_month: {count, amount_cents}, open_deals: {count, amount_cents, weighted_amount_cents}, tasks: {overdue, due_today, upcoming}, recent_activities: Activity[] (10)}` |
| GET /api/notifications | `unread?` | Page of `{id, type, data, read_at, created_at}` |
| GET /api/notifications/unread-count | — | `{count}` |
| POST /api/notifications/{id}/read, POST /api/notifications/read-all | — | 204 |
| GET /api/audit | `actor_id?, subject_type?, subject_id?, action?, page` | Page of `{id, kind, action, actor_id, subject_type, subject_id, old_values, new_values, request_id, created_at}` (admin) |
| GET /api/openapi.json | — | OpenAPI 3.1 document |

Resource shapes:

- `User`: `{__type, id, name, email, email_verified_at, roles: string[], created_at}`
- `Company`: `{__type, id, name, domain, industry, size ("1-10" | "11-50" | "51-200" | "201-1000" | "1000+" | null), phone, website, city, country, owner_id, owner: {id, name}, created_at, updated_at}`
- `Contact`: `{__type, id, first_name, last_name, full_name, email, phone, title, status ("lead" | "prospect" | "customer" | "churned"), company_id, company: {id, name} | null, owner_id, owner: {id, name}, created_at, updated_at}`
- `Deal`: `{__type, id, title, amount_cents, currency, stage ("lead" | "qualified" | "proposal" | "negotiation" | "won" | "lost"), probability, position, expected_close_date, closed_at, lost_reason, company_id, company: {id, name}, contact_id, contact: {id, full_name} | null, owner_id, owner: {id, name}, created_at, updated_at}`
- `Activity`: `{__type, id, kind ("note" | "call" | "email" | "meeting" | "task"), subject, body, due_at, completed_at, subject_type, subject_id, subject_label, owner_id, owner: {id, name}, assignee_id, assignee: {id, name} | null, created_at}`

CASL subjects and actions used by the UI: `Company`, `Contact`, `Deal`, `Activity`, `User` with `read`, `create`, `update`, `delete`, `manage`, plus `move` on `Deal` and `export` on `Contact`. Conditions use `owner_id` and `stage`.

Real-time: Pusher protocol served by the CRM API on `ws://127.0.0.1:8080/app/navree-crm-key`; private channels authorise via `POST /broadcasting/auth` (through the BFF, which adds the bearer token). Channels: `private-crm.deals` (event `deal.updated`, data `{deal: Deal}`), `private-users.{id}` (event `notification.created`).

## 8. What is left to the builders

Internal module layout inside each crate, private helper names, test case selection beyond the tests the plan names, exact log message wording, the CRM's visual design (follow the `frontend-design:frontend-design` skill: a clean, professional sales tool, not a template look).

## 9. Contract clarifications (settled while building crm-web)

These are binding for crm-api as well:

1. Completing a task is its own action: `complete` on `Activity`. Reps may `complete` an activity when `assignee_id = me` or `owner_id = me`; managers and admins may complete any. `POST /api/v1/activities/{id}/complete` checks `#[can(Complete, Activity, from = "id")]`; the UI checks `can('complete', activity)`.
2. Managers get the base rules plus `manage` on Company/Contact/Deal/Activity from their permissions; the final rule `cannot delete Deal when stage $in [won, lost]` applies to everyone except admin (super-admin).
3. Owner pickers are shown only to users who can `manage` the record type; otherwise the creator owns the record (`owner_id` defaults to the current user; reps sending another `owner_id` get 403).
4. `position` in `POST /deals/{id}/move` is a 0-based index within the target stage; other deals in that stage shift.
5. `due` filter: `overdue` = `kind = task`, not completed, `due_at` before the start of today (UTC); `today` = `due_at` within today (UTC); `upcoming` = `due_at` after today.
6. Dashboard: `pipeline`, `won_this_month`, `open_deals` are team-wide; `tasks` counts are for the current user (assignee, or owner when unassigned); amounts are summed across currencies.
7. Notification `data` shape: `{title, message, url, deal_id?}`; the same object is the `notification.created` broadcast payload.
8. Real-time event names are un-namespaced (`deal.updated`, `notification.created`); `/broadcasting/auth` accepts JSON `{socket_id, channel_name}` as well as form encoding.
9. crm-web test hooks to keep stable: `data-testid="deal-card"` (with `data-stage`), `column-count`, `task-row`, `user-row`, `audit-row`.

## 10. Direction change: NestJS approach for auth and API errors (user decision, 2026-10-25)

The user decided that auth and the API follow the NestJS approach, not Laravel Sanctum. Everything else (pagination shape, Pusher-protocol real-time, queues, scheduler, events) stays as designed.

1. **Auth = NestJS-style JWT** (plan Task 28/30): stateless JWT access tokens (HS256, 15 min) and opaque rotating refresh tokens (30 days, SHA-256 at rest, reuse detection revokes the token family); Passport-style `Strategy` trait with a built-in JWT strategy; global guard with `#[public]` opt-out (already navree-http's deny-by-default); `CurrentUser` extractor. Routes: `POST /api/auth/register|login|refresh|logout`, `GET /api/auth/me`. No Sanctum CSRF-cookie endpoint, no personal access tokens, no session login. navree-session remains an optional module that the CRM does not import.
2. **Web token storage = Next.js server (BFF)**, as described in §7: tokens live in httpOnly cookies on the Next.js side and are forwarded as `Authorization: Bearer`.
3. **Error format = NestJS**: every error body is `{"statusCode": 404, "message": "Not Found", "error": "Not Found"}` (`message` may be a custom string); validation failures are `400` with `{"statusCode": 400, "message": ["email must be a valid email address", ...], "error": "Bad Request", "errors": {"email": ["email must be a valid email address"]}}`; 500 bodies never leak internals (`"Internal server error"`); content type `application/json`; the request id stays in the `X-Request-Id` header. navree-session's CSRF failure becomes 403 `"Invalid CSRF token"`. 422 and 419 are no longer used.
