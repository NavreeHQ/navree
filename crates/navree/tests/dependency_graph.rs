//! Enforces Navree's crate hierarchy on every test run.
//!
//! Every `navree*` dependency edge in the workspace (normal, dev and build) must be
//! allowed by [`HIERARCHY`], the table of allowed runtime dependency edges
//! between the layers.
//!
//! Three exceptions, all for dev-dependencies only:
//! - A crate may dev-depend on itself, to turn on its own features in its
//!   tests (navree-workspace's `testing`).
//! - A dev-dependency may point at any navree crate in a lower layer,
//!   so tests can boot lower-layer modules (navree-notify's tests need
//!   ViewModule for MailModule). This covers `navree-testing` (L3) for every
//!   crate in layer 4 or above.
//! - `navree-macros` tests expand the macros against the runtime crates, so
//!   `navree-macros` may dev-depend on any runtime crate (layers 0 to 7).
//!
//! SQLite is opt-in: `navree` with `postgres` or `mysql` and without
//! `sqlite` has no `libsqlite3-sys` in its resolved graph.
//!
//! It also checks the external side of the graph: `Cargo.lock` must not contain
//! aws-lc, OpenSSL, native-tls or CMake (rustls with `ring` only), nor the
//! crates the crypto policy forbids (`rsa`, `p256`, `p384`, `ed25519-dalek`),
//! nor another HTTP framework's router (`axum`, `axum-core`, `matchit`):
//! navree-http routes requests itself.
//!
//! Adding a workspace crate? Give it a row in [`HIERARCHY`] first.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

/// One row of the hierarchy: a workspace crate, its layer, and the navree crates
/// it may depend on.
struct Rule {
    krate: &'static str,
    layer: u8,
    allowed: &'static [&'static str],
}

const fn rule(krate: &'static str, layer: u8, allowed: &'static [&'static str]) -> Rule {
    Rule {
        krate,
        layer,
        allowed,
    }
}

/// The allowed runtime dependency edges: the ecosystem layers (L6 libraries,
/// L7 packages built on them, L8 the entry points), and the app layers (L9
/// and up) whose crates may use anything below them. Kept one row per crate.
#[rustfmt::skip]
const HIERARCHY: &[Rule] = &[
    // L0
    rule("navree-kernel", 0, &[]),
    rule("navree-macros", 0, &[]),
    // The OpenAPI document model and `ToSchema`.
    rule("navree-openapi", 0, &["navree-macros"]),
    // The ORM's derive and attribute macros (re-exported by navree-orm).
    rule("navree-orm-macros", 0, &[]),
    // Dock: local development services (`navree dock`). A tool crate with
    // no navree dependencies; the CLI uses it and the facade re-exports it
    // behind the opt-in `dock` feature.
    rule("navree-dock", 0, &[]),
    // Harbor: servers and deployments. A tool crate for the CLI (and the
    // facade's opt-in `harbor` feature) that builds on no navree crate.
    rule("navree-harbor", 0, &["navree-template"]),
    // `embed_locales!` for navree-i18n (proc-macro; no navree dependencies).
    rule("navree-i18n-macros", 0, &[]),
    // The derives of navree-ui (re-exported by it; proc-macro, no navree
    // dependencies).
    rule("navree-ui-macros", 0, &[]),
    // `#[derive(Validate)]` (re-exported by navree-validation; proc-macro, no
    // navree dependencies).
    rule("navree-validation-macros", 0, &[]),
    // The bounded in-process cache (sharded, SIEVE eviction, TTL and TTI,
    // single-flight loading) under every framework cache: below everything
    // that keeps one (navree-cache, navree-access, navree-oauth,
    // navree-domains, navree-helm) and re-exported by the facade. It depends
    // on no navree crate.
    rule("navree-bounded", 0, &[]),
    // The PDF object layer (reader, writer, text) under navree-documents and
    // navree-media, so both read PDFs with one reader and one set of
    // limits. It depends on no navree crate.
    rule("navree-pdf", 0, &[]),
    // The glob matcher (`*` and `?`) for the patterns apps configure as
    // strings: the browser's URL waits, the HTTP client's host allow-list
    // and fake stubs, the Logbook's ignored routes and the session layer's
    // CSRF exceptions. It depends on no navree crate.
    rule("navree-glob", 0, &[]),
    // Markdown read once and read safely: CommonMark into events that hold no raw
    // HTML, no script link and no unbounded document. navree-ui draws them as
    // elements and navree-api as HTML for the descriptions of its reference. It
    // depends on no navree crate.
    rule("navree-markdown", 0, &[]),
    // Phone numbers: the parser, matcher and formatter over libphonenumber's
    // numbering plans, compiled into tables. It depends on no navree crate;
    // navree-validation's `Phone`, navree-ui's `PhoneInput` and navree-orm's
    // phone fields build on it.
    rule("navree-phone", 0, &[]),
    // The template engine behind views, mail, translated messages and the
    // files the CLI and Harbor generate. It depends on no navree crate.
    rule("navree-template", 0, &[]),
    // Tests that measure the heap a render uses, with a counting allocator.
    // Nothing depends on it.
    rule("navree-template-peak", 0, &["navree-template"]),
    // L1
    rule("navree-contracts", 1, &["navree-kernel"]),
    // `#[derive(ConfigSection)]` comes from navree-macros and is re-exported.
    rule("navree-config", 1, &["navree-kernel", "navree-contracts", "navree-macros"]),
    // Telemetry events and trace context: every emitter (L2 and up) uses it.
    rule("navree-telemetry", 1, &["navree-kernel", "navree-contracts"]),
    // L2
    rule("navree-http", 2, &["navree-openapi", "navree-kernel", "navree-contracts", "navree-config", "navree-telemetry"]),
    rule("navree-log", 2, &["navree-kernel", "navree-contracts", "navree-config", "navree-telemetry"]),
    rule("navree-crypto", 2, &["navree-kernel", "navree-contracts", "navree-config"]),
    // Events report each emit to the telemetry sinks (`EventEmitted`).
    rule("navree-events", 2, &["navree-kernel", "navree-contracts", "navree-config", "navree-telemetry"]),
    rule("navree-view", 2, &["navree-kernel", "navree-contracts", "navree-config", "navree-template"]),
    // The memory store is a navree-bounded cache.
    rule("navree-cache", 2, &["navree-kernel", "navree-bounded", "navree-contracts", "navree-config", "navree-telemetry"]),
    rule("navree-http-client", 2, &["navree-kernel", "navree-contracts", "navree-config", "navree-telemetry", "navree-glob"]),
    // The schema builder and migration language: below navree-db, which
    // runs its migrations, so framework crates at L4 and up can build their
    // tables (and hypertables) with it.
    rule("navree-schema", 2, &["navree-kernel", "navree-contracts", "navree-config"]),
    // Navigator: the MCP server for coding agents (`navree mcp`) and the
    // embedded agent skills (`navree skills:install`). A tool crate the CLI
    // runs outside the app: it reads the app's configuration with
    // navree-config and Dock's settings; the facade re-exports
    // it behind the opt-in `navigator` feature.
    rule("navree-navigator", 2, &["navree-kernel", "navree-contracts", "navree-config", "navree-dock"]),
    // L3
    // navree-schema: migrations written with the schema builder, schema dumps.
    // navree-openapi: the schema of `CursorPage`.
    rule("navree-db", 3, &["navree-openapi", "navree-kernel", "navree-contracts", "navree-config", "navree-telemetry", "navree-http", "navree-schema"]),
    rule("navree-testing", 3, &["navree-kernel", "navree-contracts", "navree-config", "navree-http"]),
    rule("navree-observe", 3, &["navree-kernel", "navree-contracts", "navree-config", "navree-telemetry", "navree-http"]),
    rule("navree-api", 3, &["navree-openapi", "navree-kernel", "navree-contracts", "navree-config", "navree-http", "navree-markdown"]),
    // Mail: API transports over navree-http-client; the delivery log,
    // suppressions, the outbox and tenant transports in navree-db (a
    // same-layer edge, like navree-queue -> navree-schedule), their routes
    // on navree-http, events through navree-events, credentials sealed and
    // DKIM signed with navree-crypto; navree-testing backs its tests.
    // Inbound mail (feature `inbound`) keeps raw messages on navree-storage
    // disks and reads SES's S3 objects from them (a same-layer edge;
    // storage never depends on mail).
    rule("navree-mail", 3, &["navree-openapi", 
        "navree-kernel", "navree-contracts", "navree-config", "navree-telemetry", "navree-view",
        "navree-http", "navree-crypto", "navree-events", "navree-http-client", "navree-db",
        "navree-testing", "navree-storage",
    ]),
    // Realtime: a live nudge inside a unit of work waits for its commit
    // (navree-db, same layer; the database never depends on realtime).
    rule("navree-realtime", 3, &["navree-kernel", "navree-contracts", "navree-config", "navree-http", "navree-crypto", "navree-db", "navree-telemetry"]),
    rule("navree-storage", 3, &["navree-kernel", "navree-contracts", "navree-config", "navree-http", "navree-crypto", "navree-telemetry"]),
    // Time zones: zones, local days, send windows, holidays; the
    // `CurrentZone` extractor reads the request (navree-http) and collects
    // zone sources (navree-contracts) from workspaces and apps. Its local-time
    // SQL are navree-db `Statement` fragments (a same-layer edge, like
    // navree-mail -> navree-db; the database never depends on time).
    rule("navree-time", 3, &["navree-openapi", "navree-kernel", "navree-contracts", "navree-config", "navree-http", "navree-db"]),
    // L4
    rule("navree-session", 4, &["navree-kernel", "navree-contracts", "navree-config", "navree-http", "navree-crypto", "navree-db", "navree-glob"]),
    // Its derive comes from navree-validation-macros; `Validate` for
    // navree-time's zones, days and day ranges (optional feature `time`;
    // time never depends on validation); `Phone` reads numbers with
    // navree-phone (optional feature `phone`).
    rule("navree-validation", 4, &[
        "navree-kernel", "navree-contracts", "navree-config", "navree-http", "navree-db",
        "navree-validation-macros", "navree-time", "navree-phone",
    ]),
    // The audit trail is a navree-orm model (`AuditRecord`), listed through
    // the ORM (same layer; the ORM never depends on audit) and written with
    // navree-db's statements.
    rule("navree-audit", 4, &["navree-kernel", "navree-contracts", "navree-config", "navree-http", "navree-db", "navree-orm"]),
    // Queue: operations mount routes with an SSE stream (http), authorize
    // the private channel `operations.{id}` (realtime) and schedule their
    // watchdog (schedule, same layer; schedule never depends on queue).
    // Failed jobs are navree-orm models (same layer; the ORM never depends
    // on the queue: it dispatches through navree-contracts). Its optional feature `ui` draws an
    // operation's progress with navree-ui (same layer; the toolkit never depends on the queue).
    rule("navree-queue", 4, &["navree-openapi", "navree-kernel", "navree-contracts", "navree-config", "navree-telemetry", "navree-http", "navree-db", "navree-realtime", "navree-schedule", "navree-orm", "navree-ui"]),
    // Schedule: `in_zone` and `per_workspace_local` evaluate cron
    // expressions on local clocks (navree-time).
    rule("navree-schedule", 4, &["navree-kernel", "navree-contracts", "navree-config", "navree-telemetry", "navree-db", "navree-time"]),
    // Each principal's abilities are kept in a navree-bounded cache.
    rule("navree-access", 4, &["navree-kernel", "navree-bounded", "navree-contracts", "navree-config", "navree-http", "navree-db", "navree-cache"]),
    // Stored notifications are navree-orm models, written by the database
    // channel through the ORM (same layer; the ORM never depends on notify),
    // like the delivery log, opt-outs and push subscriptions of the
    // Slack, Teams, SMS, WhatsApp and web push channels. Their providers are
    // called through navree-http-client, web push is encrypted and VAPID
    // signed with navree-crypto, and numbers are read as E.164 with
    // navree-validation's `Phone` (same layer; validation never depends on
    // notify). Preferences: quiet hours, send windows and digests are
    // counted on the recipient's clock (navree-time's zones and windows),
    // `notify:release` runs on the scheduler (navree-schedule, same layer;
    // schedule never depends on notify), digests and workspace overrides
    // render through navree-view, and `#[notification]` comes from
    // navree-macros.
    rule("navree-notify", 4, &[
        "navree-kernel", "navree-contracts", "navree-config", "navree-telemetry", "navree-http",
        "navree-db", "navree-mail", "navree-orm", "navree-crypto", "navree-http-client",
        "navree-validation", "navree-time", "navree-schedule", "navree-view", "navree-macros",
        // The live hub is told when a notification is stored, and the optional feature `ui` draws
        // the bell with navree-ui (same layer; the toolkit never depends on notify).
        "navree-realtime", "navree-ui",
    ]),
    // The UI toolkit. Its forms read navree-validation's rules, and its derives
    // come from navree-ui-macros. Its Markdown is read by navree-markdown, the
    // reader behind the API reference's descriptions too.
    // The table's backend goes through the ORM (same layer, optional; the ORM never
    // depends on the UI), and the sortable's moves run in a unit of work of
    // navree-db's (optional, with the ORM), which it begins at READ COMMITTED.
    // Its phone input (optional feature `phone`) reads and writes numbers with
    // navree-phone. Its uploads (optional feature `storage`) are received through
    // navree-storage's disks and upload rules. Its optional feature `time` reads the
    // reader's zone from navree-time (today's day, the times of a timeline), and its
    // optional feature `access` shows and hides actions by navree-access's abilities.
    rule("navree-ui", 4, &["navree-kernel", "navree-contracts", "navree-config", "navree-http", "navree-validation", "navree-ui-macros", "navree-orm", "navree-db", "navree-phone", "navree-storage", "navree-time", "navree-access", "navree-telemetry", "navree-markdown"]),
    // The ORM: models on navree-db's pool and unit of work. Model events go
    // through navree-events, encrypted and hashed casts through
    // navree-crypto, records as API resources (`__type`, field masks)
    // through navree-api. navree-access (same layer,
    // optional feature `access`): models as subjects, abilities as query
    // filters. navree-testing (optional feature `testing`): `TestApp`
    // seeding. navree-validation (optional feature `validation`): rules
    // that query models. navree-phone (optional feature `phone`): phone
    // numbers as fields.
    rule("navree-orm", 4, &["navree-openapi", 
        "navree-kernel", "navree-contracts", "navree-config", "navree-telemetry", "navree-http",
        "navree-db", "navree-events", "navree-crypto", "navree-api", "navree-orm-macros",
        "navree-access", "navree-testing", "navree-validation", "navree-phone",
    ]),
    // Browser testing: a dev-dependency crate that serves an app's router
    // (navree-http) on a real port and drives Chrome over the DevTools
    // Protocol (may use http and testing).
    rule("navree-browser", 4, &[
        "navree-kernel", "navree-contracts", "navree-config", "navree-http", "navree-testing",
        "navree-glob",
    ]),
    // L5
    // Token exchange fetches trusted issuers' key sets with navree-http-client.
    // The auth tables are navree-orm models; users are navree-access's
    // subject `User`. Browsers sign in on navree-session's session (optional
    // feature `session`), and the sign-in, two-factor and password reset
    // pages are drawn with navree-ui (optional feature `ui`). A native app's
    // push devices end with the sign-in that registered them, and the push
    // channel asks which sign-ins are live (optional feature `notify`;
    // navree-notify never depends on navree-auth).
    rule("navree-auth", 5, &["navree-openapi",
        "navree-kernel", "navree-contracts", "navree-config", "navree-http", "navree-db",
        "navree-crypto", "navree-session", "navree-validation", "navree-cache", "navree-mail",
        "navree-access", "navree-http-client", "navree-orm", "navree-ui", "navree-telemetry",
        "navree-notify",
    ]),
    // Workspaces: user ids come from the request's `Principal`, so the
    // module needs no navree-auth (a sibling at L5). The nightly demo
    // reset is a navree-schedule task. The `session` resolver reads the
    // workspace a browser switched to from navree-session's session
    // (optional feature `session`), and the workspace switcher is drawn
    // with navree-ui (optional feature `ui`).
    rule("navree-workspace", 5, &["navree-openapi",
        "navree-kernel", "navree-contracts", "navree-config", "navree-http", "navree-db",
        "navree-crypto", "navree-events", "navree-mail", "navree-access", "navree-telemetry",
        "navree-session", "navree-ui",
        // `navree_workspace::testing` (feature `testing`): isolation suites
        // on navree-testing's TestApp.
        "navree-testing",
        // Workspace time zones and countries, and its `dyn ZoneSource`.
        "navree-time",
        // The nightly reset of demo workspaces.
        "navree-schedule",
        // The workspace tables are navree-orm models.
        "navree-orm",
    ]),
    // Redis: the shared connections and the Redis drivers of the cache,
    // the queue, sessions and the real-time backplane (may also use http
    // and db).
    rule("navree-redis", 5, &[
        "navree-kernel", "navree-contracts", "navree-config", "navree-http", "navree-cache",
        "navree-db", "navree-queue", "navree-session", "navree-realtime",
    ]),
    // MongoDB: the ORM's document driver (navree-orm), the schema builder's
    // blueprints compiled into collections, validators and time-series
    // collections (navree-schema), units of work that join navree-db's and
    // record its change events, and MongoDB commands reported as query
    // telemetry (navree-telemetry).
    rule("navree-mongo", 5, &[
        "navree-kernel", "navree-contracts", "navree-config", "navree-telemetry",
        "navree-schema", "navree-db", "navree-orm",
    ]),
    // L6: ecosystem libraries.
    // Social sign-in: links provider accounts to navree-auth users and
    // issues their tokens. Linked accounts are navree-orm models.
    rule("navree-social", 6, &["navree-openapi", 
        "navree-kernel", "navree-contracts", "navree-config", "navree-http", "navree-crypto",
        "navree-cache", "navree-events", "navree-http-client", "navree-db", "navree-audit",
        "navree-auth", "navree-orm",
    ]),
    // Webhooks: signed outgoing deliveries through the queue (a job
    // dispatcher from navree-contracts) and the HTTP client, endpoints in the
    // database, routes scoped to the request's workspace. The webhook tables
    // are navree-orm models. With its `notify` feature it verifies the
    // provider callbacks of navree-notify's channels and applies them to
    // the notification delivery log.
    rule("navree-webhooks", 6, &["navree-openapi", 
        "navree-kernel", "navree-contracts", "navree-config", "navree-http", "navree-crypto",
        "navree-events", "navree-http-client", "navree-db", "navree-queue", "navree-access",
        "navree-workspace", "navree-orm", "navree-notify", "navree-telemetry",
    ]),
    // Integrations: workspace and organization connections to third-party
    // APIs. Credentials encrypted with navree-crypto (service-account JWTs
    // signed through its `jose`), token refreshes under navree-cache locks,
    // provider calls through navree-http-client, connections in navree-db,
    // owners from navree-workspace, `integrations:check` on navree-schedule
    // and `ConnectionUnhealthy` on navree-events. The connections table is a
    // navree-orm model. With its `notify` feature, connections give the
    // tokens of navree-notify's Slack and Teams bot routes; with its `mail`
    // feature, the Microsoft 365 and Gmail mailboxes of connections are
    // watched and their messages handed to navree-mail's `Inbound`.
    rule("navree-integrations", 6, &["navree-openapi", 
        "navree-kernel", "navree-contracts", "navree-config", "navree-http", "navree-crypto",
        "navree-cache", "navree-events", "navree-http-client", "navree-db", "navree-schedule",
        "navree-workspace", "navree-orm", "navree-notify", "navree-mail",
    ]),
    // OAuth 2.1 and OpenID Connect provider: clients, grants and personal
    // access tokens for navree-auth users, scopes mapped to navree-access
    // permissions, workspace-limited clients. The OAuth tables are
    // navree-orm models. Token lookups are kept in a navree-bounded cache.
    rule("navree-oauth", 6, &["navree-openapi", 
        "navree-kernel", "navree-bounded", "navree-contracts", "navree-config", "navree-http", "navree-crypto",
        "navree-db", "navree-access", "navree-auth", "navree-workspace", "navree-orm",
        // `navree_oauth::testing` drives the flows with navree-testing's TestClient.
        "navree-testing",
        // The consent page (optional feature `ui`).
        "navree-ui",
    ]),
    // Feature flags: user ids come from the request's `Principal`; navree-auth
    // only gives `From<&User> for FlagScope` (the crate's default `auth`
    // feature). The flags table is a navree-orm model.
    rule("navree-flags", 6, &["navree-openapi", 
        "navree-kernel", "navree-contracts", "navree-config", "navree-http", "navree-cache",
        "navree-db", "navree-events", "navree-audit", "navree-access", "navree-auth",
        "navree-workspace", "navree-orm",
    ]),
    // Settings: typed settings stored per scope (global, organization,
    // workspace, user) with inheritance; patches validated with
    // navree-validation's messages, changes audited (navree-audit) and
    // emitted (navree-events), resolved values cached by tag (navree-cache),
    // workspace and organization scopes from navree-workspace, the global
    // write permission checked with navree-access; it re-exports
    // `#[derive(Settings)]` from navree-macros. With its `notify` feature,
    // the `notification-templates` settings are navree-notify's template
    // overrides. The settings table is a navree-orm model.
    rule("navree-settings", 6, &["navree-openapi", 
        "navree-kernel", "navree-contracts", "navree-config", "navree-macros", "navree-http",
        "navree-cache", "navree-events", "navree-db", "navree-validation", "navree-audit",
        "navree-access", "navree-workspace", "navree-notify", "navree-orm",
    ]),
    // GraphQL: `#[resolver]` fields with route-style access, per-request
    // units of work and dataloaders, subscriptions over WebSockets.
    rule("navree-graphql", 6, &[
        "navree-kernel", "navree-contracts", "navree-config", "navree-http", "navree-events",
        "navree-db", "navree-validation", "navree-access", "navree-auth", "navree-workspace",
    ]),
    // gRPC: `#[grpc_service]` (navree-macros) expands into its call pipeline;
    // navree-testing backs `navree_grpc::testing::TestChannel`.
    rule("navree-grpc", 6, &[
        "navree-kernel", "navree-contracts", "navree-config", "navree-macros", "navree-http",
        "navree-testing", "navree-db", "navree-validation",
    ]),
    // MCP servers: tools, resources and prompts over HTTP and stdio, checked
    // by navree-access; stdio users are found through navree-auth; sessions
    // live in navree-cache; `McpTestClient` wraps navree-testing's client.
    rule("navree-mcp", 6, &["navree-openapi", 
        "navree-kernel", "navree-contracts", "navree-config", "navree-http", "navree-cache",
        "navree-db", "navree-testing", "navree-validation", "navree-access", "navree-auth",
    ]),
    // AI SDK: outgoing provider calls go through navree-http-client (its SSRF
    // guard); conversation memory and pgvector use navree-db; agents run as a
    // navree-auth user with navree-access abilities and report tool argument
    // errors with navree-validation's messages; budgets and the embeddings
    // cache use navree-cache; attachments read navree-storage disks; Bedrock
    // requests are signed with navree-crypto's SigV4 signer; it re-exports
    // `#[agent]` from navree-macros. Its optional feature `ui` draws an
    // assistant with navree-ui, nudges the reader's page through
    // navree-realtime's hub and signs its buttons for navree-session's reader.
    rule("navree-ai", 6, &["navree-openapi", 
        "navree-kernel", "navree-contracts", "navree-config", "navree-macros", "navree-http",
        "navree-http-client", "navree-cache", "navree-db", "navree-storage", "navree-validation",
        "navree-access", "navree-auth", "navree-telemetry", "navree-crypto",
        "navree-ui", "navree-realtime", "navree-session",
    ]),
    // Microservices: message handlers run through navree-http's guards and
    // authorizer, navree-db's unit of work (outbox, inbox, dead letters) and
    // navree-validation's `Valid`, in a navree-telemetry context that
    // continues the sender's trace; `forward_event` uses navree-events;
    // `MicroFake` answers with navree-testing's `TestResponse`.
    rule("navree-micro", 6, &[
        "navree-kernel", "navree-contracts", "navree-config", "navree-telemetry", "navree-http",
        "navree-events", "navree-db", "navree-testing", "navree-validation",
    ]),
    // Single sign-on: SAML 2.0 and OpenID Connect connections owned by an
    // organization or a workspace (navree-workspace), signing in navree-auth
    // users; XML signatures and ID tokens are verified with navree-crypto's
    // `jose` (ring), IdP metadata, key sets and DNS checks go through
    // navree-http-client, and states, replays and one-time codes live in
    // navree-cache. The SSO tables are navree-orm models, and users and
    // workspaces are read as navree-auth's and navree-workspace's models.
    rule("navree-sso", 6, &["navree-openapi", 
        "navree-kernel", "navree-contracts", "navree-config", "navree-http", "navree-crypto",
        "navree-cache", "navree-events", "navree-http-client", "navree-db", "navree-auth",
        "navree-workspace", "navree-orm",
    ]),
    // Localization: Fluent messages, the request's locale (the signed-in
    // user's or the workspace's saved one, a query parameter, a cookie,
    // Accept-Language), translated validation messages, a `t` function in
    // views and notifications in the recipient's locale. Its optional feature
    // `ui` gives Navree UI's labels, money and dates a locale (navree-ui never
    // depends on it: navree-workspace's own `ui` feature would close a cycle).
    rule("navree-i18n", 6, &["navree-openapi", 
        "navree-kernel", "navree-contracts", "navree-config", "navree-http", "navree-view",
        "navree-db", "navree-validation", "navree-notify", "navree-workspace",
        "navree-i18n-macros", "navree-template", "navree-ui",
    ]),
    // Documents: PDF from HTML templates (navree-view) laid out by the
    // native engine or printed by headless Chrome, DOCX, and PDF
    // merge and stamp. Queued renders go through the job dispatcher of
    // navree-contracts and land on a storage disk; with its `operations`
    // feature, a render runs as the work of a navree-queue operation
    // started in a navree-db unit of work.
    rule("navree-documents", 6, &[
        "navree-kernel", "navree-contracts", "navree-config", "navree-http", "navree-view",
        "navree-storage", "navree-db", "navree-queue", "navree-pdf",
    ]),
    // Sheets: CSV and Excel exports (streamed `Download`s, queued exports
    // written to a navree-storage disk and handed back through a
    // navree-queue operation) and imports (uploads on a disk, staging
    // tables in navree-db, rows checked with navree-validation, applied in
    // queue batches); the routes act in the request's workspace
    // (navree-workspace); headers translate with navree-i18n (feature
    // `i18n`); `#[derive(Exportable)]` and `#[derive(Importable)]` live in
    // navree-macros and are re-exported next to their traits.
    rule("navree-sheets", 6, &["navree-openapi", 
        "navree-kernel", "navree-contracts", "navree-config", "navree-macros", "navree-http",
        "navree-db", "navree-storage", "navree-validation", "navree-queue", "navree-workspace",
        "navree-i18n",
    ]),
    // L7: packages built on other packages.
    // Search: full-text and vector search over navree-orm models. The server
    // engines talk HTTP through navree-http-client; the `Searchable` derive
    // lives in navree-macros and is re-exported next to the trait. With its
    // `ai` feature, vectors are embedded and stored by navree-ai (same family
    // of layers: L7 on L6).
    rule("navree-search", 7, &[
        "navree-kernel", "navree-contracts", "navree-config", "navree-macros", "navree-http",
        "navree-http-client", "navree-db", "navree-orm", "navree-queue", "navree-access",
        "navree-workspace", "navree-telemetry", "navree-ai",
    ]),
    // Billing: Stripe and Paddle over navree-http-client, their webhook
    // signatures checked by navree-webhooks' verifiers, billables from
    // navree-auth (users) and navree-workspace (workspaces). The billing
    // tables are navree-orm models.
    rule("navree-billing", 7, &["navree-openapi", 
        "navree-kernel", "navree-contracts", "navree-config", "navree-http", "navree-cache",
        "navree-events", "navree-http-client", "navree-db", "navree-queue", "navree-schedule",
        "navree-access", "navree-auth", "navree-workspace", "navree-orm", "navree-webhooks",
    ]),
    // Lighthouse: a telemetry sink storing issues, occurrences, samples and
    // aggregates in the app's database; alerts by mail (navree-mail) and
    // over HTTP (navree-http-client), queued as jobs (navree-contracts) and
    // pruned on the schedule; a dashboard drawn with navree-ui on
    // navree-session's session (navree-auth's `SessionStrategy`), behind
    // navree-access abilities; with its `mcp`
    // feature, tools, a resource and a prompt on navree-mcp's server, whose
    // arguments derive navree-validation's `Validate`. Its tables are
    // navree-orm models.
    rule("navree-lighthouse", 7, &["navree-openapi", 
        "navree-kernel", "navree-contracts", "navree-config", "navree-telemetry", "navree-http",
        "navree-http-client", "navree-db", "navree-mail",
        "navree-schedule", "navree-access", "navree-auth", "navree-mcp", "navree-orm",
        "navree-validation", "navree-session", "navree-ui",
    ]),
    // Logbook: a telemetry sink and a data-change observer (navree-db), an
    // HTTP layer and UI (navree-http, navree-view), replay as the signed-in
    // user (navree-auth's JwtService).
    rule("navree-logbook", 7, &[
        "navree-kernel", "navree-contracts", "navree-config", "navree-telemetry", "navree-http", "navree-ui",
        "navree-view", "navree-db", "navree-auth", "navree-glob",
    ]),
    // Fleet: supervised queue workers (navree-queue's supervision hooks and
    // introspection), telemetry job records for metrics, dashboard pages
    // drawn with navree-ui on navree-session's session (the user's name from
    // navree-auth) behind navree-access abilities, gauges on
    // navree-observe's `/metrics`, long-wait events.
    rule("navree-fleet", 7, &[
        "navree-kernel", "navree-contracts", "navree-config", "navree-telemetry", "navree-http",
        "navree-events", "navree-ui", "navree-db", "navree-observe", "navree-queue",
        "navree-access", "navree-auth", "navree-session",
    ]),
    // Media: files attached to models (a navree-orm model in navree-db,
    // bytes on navree-storage disks), remote files fetched through
    // navree-http-client (its SSRF guard), conversions queued as jobs and
    // regenerated in navree-queue batches.
    rule("navree-media", 7, &["navree-openapi", 
        "navree-kernel", "navree-contracts", "navree-config", "navree-http", "navree-http-client",
        "navree-db", "navree-orm", "navree-storage", "navree-queue", "navree-pdf",
    ]),
    // Helm: the admin panel over navree-orm models (their queries, access
    // filters and subjects). Markdown fields are written by navree-ui's
    // Markdown renderer, so one set of rules decides what a link or raw HTML may be.
    // navree-crypto seals its workspace cookie; browsers sign in on
    // navree-session's session (navree-auth's `SessionStrategy`), whose CSRF
    // token and sign-in the pages read; with its `settings` feature, the Settings screens edit navree-settings'
    // types (organizations and workspaces named by navree-workspace). Metric
    // results are kept in a navree-bounded cache.
    rule("navree-helm", 7, &[
        "navree-kernel", "navree-bounded", "navree-contracts", "navree-config", "navree-http", "navree-view",
        "navree-cache", "navree-crypto", "navree-db", "navree-storage", "navree-queue",
        "navree-validation", "navree-audit", "navree-access", "navree-auth", "navree-workspace",
        "navree-settings", "navree-orm", "navree-ui", "navree-session", "navree-time",
    ]),
    // Workflows: durable runs stored through navree-db, woken by
    // navree-events listeners and the `workflows:tick` schedule
    // (navree-schedule), with routes scoped to the request's workspace;
    // run jobs go through the `dyn Dispatcher` contract. `#[workflow]`,
    // `#[trigger]`, `#[condition]` and `#[action]` come from navree-macros
    // and are re-exported next to their traits.
    rule("navree-workflows", 7, &["navree-openapi", 
        "navree-kernel", "navree-contracts", "navree-config", "navree-macros", "navree-http",
        "navree-events", "navree-db", "navree-schedule", "navree-workspace",
    ]),
    // Directory sync: a SCIM 2.0 server (navree-http routes) and pull
    // connectors whose Microsoft Graph, Google Workspace and Okta clients
    // come from navree-integrations' connections (LDAP through ldap3, its
    // bind password sealed with navree-crypto); synced users and groups are
    // navree-orm models in navree-db, runs are navree-queue operations
    // with progress, `directory:sync --due` runs on navree-schedule, owners
    // are navree-workspace workspaces and organizations, and the user
    // events go out through navree-events.
    rule("navree-directory", 7, &["navree-openapi", 
        "navree-kernel", "navree-contracts", "navree-config", "navree-http", "navree-crypto",
        "navree-events", "navree-db", "navree-orm", "navree-queue", "navree-schedule",
        "navree-workspace", "navree-integrations",
    ]),
    // Custom domains: workspaces (navree-workspace, whose `domain` resolver
    // reads what its request layer on navree-http finds) on their own
    // domains, verified over DNS and kept as navree-orm models in
    // navree-db; certificate and ACME account keys sealed with
    // navree-crypto; events; `domains:verify --due` and
    // `domains:certificates --due` on navree-schedule; with its
    // `lighthouse` feature, certificates close to expiry reported to
    // navree-lighthouse. Verified hosts are kept in a navree-bounded cache.
    rule("navree-domains", 7, &["navree-openapi", 
        "navree-kernel", "navree-bounded", "navree-contracts", "navree-config", "navree-http", "navree-crypto",
        "navree-events", "navree-db", "navree-orm", "navree-schedule", "navree-workspace",
        "navree-lighthouse",
    ]),
    // L8: entry points.
    rule("navree", 8, RUNTIME_CRATES),
    // The CLI uses no runtime crate; navree-dock runs `navree dock`,
    // navree-harbor runs Harbor's commands and navree-navigator runs
    // `navree mcp` and `navree skills:install`.
    rule("navree-cli", 8, &["navree-dock", "navree-harbor", "navree-navigator", "navree-template"]),
    // L9 and up (apps): anything below, except the CLI binary.
    // The system app (e2e/): one binary on the `navree` facade that wires
    // every package for the Docker system tests.
    rule("system-app", 9, &["navree"]),
    // The toolkit's proof pages: one binary on the `navree` facade.
    rule("ui-proof", 9, &["navree"]),
    rule("ui-table-proof", 9, &["navree"]),
    // The example apps on the `navree` facade: a store's operations, a support inbox
    // and a product team's board.
    rule("store-demo", 9, &["navree"]),
    rule("inbox-demo", 9, &["navree"]),
    rule("board-demo", 9, &["navree"]),
    rule("agent-demo", 9, &["navree"]),
    // The live theme builder the docs frame: the toolkit's themes pages on the facade.
    rule("theme-demo", 9, &["navree"]),
    // The website's UI docs generator: one binary on the `navree` facade, drawing
    // the example apps' views for the website from the apps themselves.
    rule("ui-docs", 10, &["navree", "store-demo", "inbox-demo", "board-demo"]),
    // The skills' Rust blocks, compiled against the facade.
    rule("skill-snippets", 9, &["navree"]),
    // The Rust blocks of the website's UI and component pages, compiled against the facade.
    rule("docs-snippets", 9, &["navree"]),
    // The release tool: it publishes the crates and uses none of them.
    rule("xtask", 9, &[]),
];

/// Every runtime crate (layers 0 to 7) plus navree-macros and the tool
/// crates navree-dock, navree-harbor and navree-navigator: what the facade
/// may re-export and what navree-macros tests may dev-depend on.
const RUNTIME_CRATES: &[&str] = &[
    "navree-kernel",
    "navree-bounded",
    "navree-pdf",
    "navree-markdown",
    "navree-glob",
    "navree-phone",
    "navree-template",
    "navree-macros",
    "navree-openapi",
    "navree-dock",
    "navree-harbor",
    "navree-navigator",
    "navree-contracts",
    "navree-config",
    "navree-telemetry",
    "navree-http",
    "navree-log",
    "navree-crypto",
    "navree-events",
    "navree-view",
    "navree-cache",
    "navree-http-client",
    "navree-schema",
    "navree-db",
    "navree-testing",
    "navree-observe",
    "navree-api",
    "navree-mail",
    "navree-realtime",
    "navree-storage",
    "navree-time",
    "navree-session",
    "navree-validation",
    "navree-queue",
    "navree-audit",
    "navree-access",
    "navree-schedule",
    "navree-notify",
    "navree-ui",
    "navree-ui-macros",
    "navree-orm",
    "navree-browser",
    "navree-auth",
    "navree-workspace",
    "navree-mongo",
    "navree-redis",
    "navree-social",
    "navree-webhooks",
    "navree-integrations",
    "navree-oauth",
    "navree-flags",
    "navree-settings",
    "navree-search",
    "navree-graphql",
    "navree-grpc",
    "navree-ai",
    "navree-billing",
    "navree-mcp",
    "navree-sheets",
    "navree-lighthouse",
    "navree-logbook",
    "navree-micro",
    "navree-sso",
    "navree-fleet",
    "navree-helm",
    "navree-media",
    "navree-i18n",
    "navree-i18n-macros",
    "navree-documents",
    "navree-workflows",
    "navree-directory",
    "navree-domains",
];

/// Cargo dependency kind of an edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Normal,
    Dev,
    Build,
}

/// A dependency edge between two navree crates.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Edge {
    from: String,
    to: String,
    kind: Kind,
}

fn edge(from: &str, to: &str, kind: Kind) -> Edge {
    Edge {
        from: from.to_owned(),
        to: to.to_owned(),
        kind,
    }
}

/// Runs `cargo metadata --no-deps` on the workspace, without the example CRM
/// (it has a repository of its own; a checkout that still carries it is
/// checked as if it did not).
fn workspace_metadata() -> Value {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
    let output = Command::new(cargo)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
        ])
        .arg("--manifest-path")
        .arg(&manifest)
        .output()
        .expect("failed to run `cargo metadata`");
    assert!(
        output.status.success(),
        "`cargo metadata` failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut metadata: Value =
        serde_json::from_slice(&output.stdout).expect("`cargo metadata` printed invalid JSON");
    if let Some(packages) = metadata["packages"].as_array_mut() {
        packages.retain(|p| !str_field(p, "name").starts_with("crm-"));
    }
    metadata
}

/// The `packages` array of `cargo metadata --no-deps` output (workspace
/// members only).
fn packages(metadata: &Value) -> &[Value] {
    metadata["packages"]
        .as_array()
        .expect("cargo metadata has a `packages` array")
}

fn str_field<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key]
        .as_str()
        .unwrap_or_else(|| panic!("cargo metadata entry without string `{key}`: {value}"))
}

/// Names of the workspace member packages.
fn workspace_crates(metadata: &Value) -> BTreeSet<String> {
    packages(metadata)
        .iter()
        .map(|p| str_field(p, "name").to_owned())
        .collect()
}

/// Every dependency edge from a workspace package to a navree crate or to
/// another workspace package.
fn navree_edges(metadata: &Value) -> Vec<Edge> {
    let members = workspace_crates(metadata);
    let is_internal =
        |name: &str| name == "navree" || name.starts_with("navree-") || members.contains(name);
    let mut edges = Vec::new();
    for package in packages(metadata) {
        let from = str_field(package, "name");
        let deps = package["dependencies"]
            .as_array()
            .map_or(&[][..], Vec::as_slice);
        for dep in deps {
            let to = str_field(dep, "name");
            if !is_internal(to) {
                continue;
            }
            let kind = match dep["kind"].as_str() {
                None => Kind::Normal,
                Some("dev") => Kind::Dev,
                Some("build") => Kind::Build,
                Some(other) => panic!("unknown dependency kind `{other}` in {from} -> {to}"),
            };
            edges.push(edge(from, to, kind));
        }
    }
    edges
}

/// Packages that pull in aws-lc, OpenSSL or a CMake build.
const FORBIDDEN_NATIVE_PACKAGES: &[&str] = &[
    "aws-lc-fips-sys",
    "aws-lc-rs",
    "aws-lc-sys",
    "cmake",
    "native-tls",
    "openssl",
    "openssl-sys",
];

/// Packages the crypto policy keeps out: `rsa`
/// (RUSTSEC-2023-0071) and the RustCrypto curve crates. Asymmetric
/// signatures go through `ring`, wrapped in `navree_crypto::jose`.
const FORBIDDEN_CRYPTO_PACKAGES: &[&str] = &["ed25519-dalek", "p256", "p384", "rsa"];

/// Packages of another HTTP framework's request layer: Navree's router,
/// extractors and responses are navree-http's own, on `hyper` and `tower`.
const FORBIDDEN_ROUTER_PACKAGES: &[&str] = &["axum", "axum-core", "matchit"];

/// Names of forbidden native packages listed in a `Cargo.lock`.
fn forbidden_native_packages(lock: &str) -> Vec<String> {
    listed_packages(lock, FORBIDDEN_NATIVE_PACKAGES)
}

/// Names of packages the crypto policy forbids, listed in a `Cargo.lock`.
fn forbidden_crypto_packages(lock: &str) -> Vec<String> {
    listed_packages(lock, FORBIDDEN_CRYPTO_PACKAGES)
}

/// Names of other routers' packages listed in a `Cargo.lock`.
fn forbidden_router_packages(lock: &str) -> Vec<String> {
    listed_packages(lock, FORBIDDEN_ROUTER_PACKAGES)
}

/// Packages of another data layer listed in a `Cargo.lock`: any `sea-*`
/// crate. Navree's data layer is its own (navree-db on SQLx, navree-orm,
/// navree-schema), so none may come back through a dependency.
fn other_data_layer_packages(lock: &str) -> Vec<String> {
    lock.lines()
        .filter_map(|line| line.trim().strip_prefix("name = \""))
        .filter_map(|rest| rest.strip_suffix('"'))
        .filter(|name| name.starts_with("sea-"))
        .map(str::to_owned)
        .collect()
}

/// The packages of `names` that a `Cargo.lock` lists.
fn listed_packages(lock: &str, names: &[&str]) -> Vec<String> {
    lock.lines()
        .filter_map(|line| line.trim().strip_prefix("name = \""))
        .filter_map(|rest| rest.strip_suffix('"'))
        .filter(|name| names.contains(name))
        .map(str::to_owned)
        .collect()
}

/// The names of the packages `navree` builds with `features` (`cargo tree`,
/// every kind of edge: normal, build and dev), from the resolved graph of
/// the lockfile (nothing is compiled or downloaded).
fn packages_in_tree(default_features: bool, features: &[&str]) -> BTreeSet<String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let mut command = Command::new(cargo);
    command
        .args(["tree", "--offline", "--prefix", "none", "--format", "{p}"])
        .args(["-p", "navree", "--manifest-path"])
        .arg(&manifest);
    if !default_features {
        command.arg("--no-default-features");
    }
    if !features.is_empty() {
        command.args(["--features", &features.join(",")]);
    }
    let output = command.output().expect("failed to run `cargo tree`");
    assert!(
        output.status.success(),
        "`cargo tree` failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    package_names(&String::from_utf8_lossy(&output.stdout))
}

/// The package names in `cargo tree --prefix none --format "{p}"` output
/// (`name vX.Y.Z (path or *)` per line).
fn package_names(tree: &str) -> BTreeSet<String> {
    tree.lines()
        .filter_map(|line| line.split_whitespace().next())
        .map(str::to_owned)
        .collect()
}

/// The packages that compile SQLite: the C library and the driver on it.
const SQLITE_PACKAGES: &[&str] = &["libsqlite3-sys", "sqlx-sqlite"];

fn find_rule(krate: &str) -> Option<&'static Rule> {
    HIERARCHY.iter().find(|r| r.krate == krate)
}

/// Edges not allowed by the hierarchy, as human-readable messages.
fn violations(edges: &[Edge]) -> Vec<String> {
    edges.iter().filter_map(violation).collect()
}

fn violation(edge: &Edge) -> Option<String> {
    let Edge { from, to, kind } = edge;
    let Some(from_rule) = find_rule(from) else {
        return Some(format!(
            "{from} -> {to} ({kind:?}): `{from}` is not in the HIERARCHY table; add it with its layer"
        ));
    };
    let Some(to_rule) = find_rule(to) else {
        return Some(format!(
            "{from} -> {to} ({kind:?}): `{to}` is not in the HIERARCHY table; add it with its layer"
        ));
    };
    if from_rule.allowed.contains(&to.as_str()) {
        return None;
    }
    if *kind == Kind::Dev {
        // A crate's tests may turn on its own features (navree-workspace's
        // `testing`) through a dev-dependency on itself.
        if from == to {
            return None;
        }
        // Tests may use any navree crate in a lower layer; this is how
        // navree-testing (L3) serves every crate in layer 4 and above.
        if to_rule.layer < from_rule.layer {
            return None;
        }
        // Proc-macro tests expand against the runtime crates they target.
        if from == "navree-macros" && RUNTIME_CRATES.contains(&to.as_str()) && to != "navree-macros"
        {
            return None;
        }
    }
    Some(format!(
        "{from} (L{}) -> {to} (L{}) ({kind:?}) is not an allowed edge; {from} may depend on: [{}]",
        from_rule.layer,
        to_rule.layer,
        from_rule.allowed.join(", ")
    ))
}

#[test]
fn workspace_dependency_edges_follow_the_hierarchy() {
    let edges = navree_edges(&workspace_metadata());
    let found = violations(&edges);
    assert!(
        found.is_empty(),
        "crate hierarchy violations:\n  {}",
        found.join("\n  ")
    );
}

/// Global constraint: rustls with the `ring` provider; never require
/// aws-lc (cmake/NASM on Windows), OpenSSL or native-tls.
#[test]
fn lockfile_has_no_aws_lc_openssl_native_tls_or_cmake() {
    let lockfile = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock");
    let lock = std::fs::read_to_string(&lockfile).expect("Cargo.lock is readable");
    let found = forbidden_native_packages(&lock);
    assert!(
        found.is_empty(),
        "Cargo.lock contains forbidden native TLS/build packages: {found:?}; \
         use rustls with the `ring` provider (see root Cargo.toml)"
    );
}

/// Crypto policy: no `rsa`, `p256`, `p384` or `ed25519-dalek`; RSA, ECDSA
/// and Ed25519 go through `ring` (`navree_crypto::jose`).
#[test]
fn lockfile_follows_the_crypto_policy() {
    let lockfile = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock");
    let lock = std::fs::read_to_string(&lockfile).expect("Cargo.lock is readable");
    let found = forbidden_crypto_packages(&lock);
    assert!(
        found.is_empty(),
        "Cargo.lock contains packages the crypto policy forbids: {found:?}; \
         sign and verify with navree_crypto::jose (ring) instead, and check \
         `cargo tree -i <package>` for the crate that pulls them in"
    );
}

/// Navree routes requests itself: neither `axum` (nor its core) nor `matchit`
/// is in the resolved graph.
#[test]
fn lockfile_has_no_other_router() {
    let lockfile = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock");
    let lock = std::fs::read_to_string(&lockfile).expect("Cargo.lock is readable");
    let found = forbidden_router_packages(&lock);
    assert!(
        found.is_empty(),
        "Cargo.lock contains {found:?}; navree-http owns the router, the extractors and the \
         responses. Find what pulls them in with `cargo tree -i <package>`"
    );
}

/// Navree's data layer is its own: no `sea-*` crate (sea-orm, sea-query,
/// sea-orm-migration, sea-schema and their helpers) is in the resolved
/// graph, and no workspace crate declares one.
#[test]
fn no_other_data_layer_is_in_the_graph() {
    let lockfile = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock");
    let lock = std::fs::read_to_string(&lockfile).expect("Cargo.lock is readable");
    let found = other_data_layer_packages(&lock);
    assert!(
        found.is_empty(),
        "Cargo.lock contains `sea-*` packages: {found:?}; Navree's data layer is          navree-db, navree-orm and navree-schema. Find what pulls them in with          `cargo tree -i <package>`"
    );
    let metadata = workspace_metadata();
    let declared: Vec<String> = packages(&metadata)
        .iter()
        .flat_map(|package| {
            let from = str_field(package, "name").to_owned();
            package["dependencies"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|dependency| str_field(dependency, "name").to_owned())
                .filter(|name| name.starts_with("sea-"))
                .map(move |name| format!("{from} -> {name}"))
        })
        .collect();
    assert!(
        declared.is_empty(),
        "workspace crates declare `sea-*` dependencies: {declared:?}"
    );
}

/// SQLite is opt-in (`navree-db`'s `sqlite`, the facade's `sqlite`): a
/// PostgreSQL or MySQL build compiles no SQLite C code, with the default
/// features (`full`) as much as without them. The graph is the resolved one
/// for the feature set, dev-dependencies of `navree` included.
#[test]
fn a_build_without_sqlite_has_no_libsqlite3_sys() {
    for (default_features, features) in [
        (false, vec!["postgres"]),
        (false, vec!["mysql"]),
        (false, vec!["postgres", "mysql"]),
        (true, vec!["postgres"]),
        (true, vec!["mysql"]),
    ] {
        let packages = packages_in_tree(default_features, &features);
        for name in SQLITE_PACKAGES {
            assert!(
                !packages.contains(*name),
                "`navree` (default features: {default_features}) with {features:?} builds `{name}`: \
                 SQLite must come only from the `sqlite` feature; find what pulls it in with \
                 `cargo tree -p navree -i {name}`"
            );
        }
        assert!(
            packages.contains("sqlx-core"),
            "the graph of {features:?} has no sqlx (is the output read right?)"
        );
    }
}

/// The other direction: the `sqlite` feature is what brings SQLite in, and
/// brings no other driver with it.
#[test]
fn the_sqlite_feature_builds_sqlite_and_no_other_driver() {
    let packages = packages_in_tree(false, &["sqlite"]);
    for name in SQLITE_PACKAGES {
        assert!(
            packages.contains(*name),
            "`navree` with `sqlite` does not build `{name}`"
        );
    }
    for name in ["sqlx-postgres", "sqlx-mysql"] {
        assert!(
            !packages.contains(name),
            "`navree` with `sqlite` builds `{name}`: each driver is a feature of its own"
        );
    }
}

/// The test kits belong to the `testing` feature: `oauth`, `grpc`, `mcp` and
/// `micro` link no navree-testing without it, and link it with it.
#[test]
fn the_test_kits_come_only_with_the_testing_feature() {
    for feature in ["oauth", "grpc", "mcp", "micro"] {
        let without = packages_in_tree(false, &[feature]);
        assert!(
            !without.contains("navree-testing"),
            "`navree` with `{feature}` links navree-testing: the test kits are the `testing` \
             feature's; find what pulls it in with `cargo tree -p navree -i navree-testing \
             --no-default-features --features {feature}`"
        );
        let with = packages_in_tree(false, &[feature, "testing"]);
        assert!(
            with.contains("navree-testing"),
            "`navree` with `{feature}` and `testing` does not link navree-testing"
        );
    }
}

/// The parts of `documents` compile only what they name: the native engine
/// brings the PDF layout crates, and the DOCX and Chrome parts do not.
#[test]
fn the_parts_of_documents_build_only_their_own_engine() {
    let native = packages_in_tree(false, &["documents-native"]);
    assert!(
        native.contains("krilla"),
        "`documents-native` does not build the native PDF engine"
    );
    for part in ["documents-chrome", "documents-docx"] {
        let packages = packages_in_tree(false, &[part]);
        assert!(
            packages.contains("navree-documents"),
            "`{part}` does not build navree-documents"
        );
        assert!(
            !packages.contains("krilla"),
            "`{part}` builds the native PDF engine (krilla): each part is a feature of its own"
        );
    }
}

#[test]
fn package_names_are_read_from_cargo_tree_output() {
    let tree = [
        r"navree v0.1.0 (C:\work\navree)",
        "sqlx v0.9.0",
        "sqlx-core v0.9.0",
        "sqlx-core v0.9.0 (*)",
    ]
    .join("\n");
    assert_eq!(
        package_names(&tree).into_iter().collect::<Vec<_>>(),
        ["navree", "sqlx", "sqlx-core"]
    );
}

#[test]
fn hierarchy_table_lists_exactly_the_workspace_crates() {
    let in_workspace = workspace_crates(&workspace_metadata());
    let in_table: BTreeSet<String> = HIERARCHY.iter().map(|r| r.krate.to_owned()).collect();
    assert_eq!(
        in_workspace, in_table,
        "every workspace crate needs a row in HIERARCHY (and every row a crate)"
    );
}

/// The table allows same-layer edges (navree-queue -> navree-schedule, both in
/// the same layer), so the invariant is "never upwards" plus "no cycles".
#[test]
fn hierarchy_table_never_points_up_and_has_no_cycles() {
    for rule in HIERARCHY {
        for dep in rule.allowed {
            let dep_rule = find_rule(dep).unwrap_or_else(|| panic!("{dep} is not in HIERARCHY"));
            assert!(
                dep_rule.layer <= rule.layer,
                "{} (L{}) may not depend on {dep} (L{})",
                rule.krate,
                rule.layer,
                dep_rule.layer
            );
        }
    }

    // Depth-first search over the allowed edges; `path` is the current chain.
    fn visit(krate: &'static str, path: &mut Vec<&'static str>) {
        if let Some(start) = path.iter().position(|k| *k == krate) {
            panic!(
                "cycle in HIERARCHY: {} -> {krate}",
                path[start..].join(" -> ")
            );
        }
        path.push(krate);
        for dep in find_rule(krate).map_or(&[][..], |r| r.allowed) {
            visit(dep, path);
        }
        path.pop();
    }
    for rule in HIERARCHY {
        visit(rule.krate, &mut Vec::new());
    }
}

#[test]
fn navree_edges_are_read_from_cargo_metadata_with_their_kind() {
    let metadata = json!({
        "packages": [
            {
                "name": "navree-db",
                "dependencies": [
                    { "name": "navree-http", "kind": null, "rename": null },
                    { "name": "sqlx", "kind": null, "rename": null },
                    { "name": "navree-kernel", "kind": "dev", "rename": "kernel" },
                    { "name": "navree-config", "kind": "build", "rename": null }
                ]
            },
            {
                "name": "some-app",
                "dependencies": [
                    { "name": "navree-http", "kind": null, "rename": null },
                    { "name": "tokio", "kind": "dev", "rename": null }
                ]
            },
            {
                "name": "other-app",
                "dependencies": [{ "name": "some-app", "kind": "dev", "rename": null }]
            }
        ]
    });
    assert_eq!(
        navree_edges(&metadata),
        vec![
            edge("navree-db", "navree-http", Kind::Normal),
            edge("navree-db", "navree-kernel", Kind::Dev),
            edge("navree-db", "navree-config", Kind::Build),
            edge("some-app", "navree-http", Kind::Normal),
            edge("other-app", "some-app", Kind::Dev),
        ]
    );
}

#[test]
fn forbidden_native_packages_are_found_in_a_lockfile() {
    let lock = r#"
version = 4

[[package]]
name = "aws-lc-sys"
version = "0.30.0"

[[package]]
name = "ring"
version = "0.17.14"

[[package]]
name = "openssl-sys"
version = "0.9.109"
"#;
    assert_eq!(
        forbidden_native_packages(lock),
        vec!["aws-lc-sys".to_owned(), "openssl-sys".to_owned()]
    );
}

#[test]
fn other_data_layer_packages_are_found_in_a_lockfile() {
    let lock = r#"
version = 4

[[package]]
name = "sea-query"
version = "1.0.2"

[[package]]
name = "seahash"
version = "4.1.0"

[[package]]
name = "sqlx"
version = "0.9.0"

[[package]]
name = "sea-orm-migration"
version = "2.0.4"
"#;
    assert_eq!(
        other_data_layer_packages(lock),
        vec!["sea-query".to_owned(), "sea-orm-migration".to_owned()]
    );
}

#[test]
fn forbidden_router_packages_are_found_in_a_lockfile() {
    let lock = r#"
version = 4

[[package]]
name = "axum-core"
version = "0.5.6"

[[package]]
name = "hyper"
version = "1.12.0"

[[package]]
name = "matchit"
version = "0.8.4"

[[package]]
name = "axum"
version = "0.8.9"

[[package]]
name = "axum-extra-not-a-router"
version = "1.0.0"
"#;
    assert_eq!(
        forbidden_router_packages(lock),
        vec![
            "axum-core".to_owned(),
            "matchit".to_owned(),
            "axum".to_owned()
        ]
    );
}

#[test]
fn forbidden_crypto_packages_are_found_in_a_lockfile() {
    let lock = r#"
version = 4

[[package]]
name = "rsa"
version = "0.9.8"

[[package]]
name = "ring"
version = "0.17.14"

[[package]]
name = "p256"
version = "0.13.2"

[[package]]
name = "ed25519"
version = "3.0.0"
"#;
    assert_eq!(
        forbidden_crypto_packages(lock),
        vec!["rsa".to_owned(), "p256".to_owned()]
    );
}

#[test]
fn allowed_edges_pass() {
    let edges = [
        edge("navree-contracts", "navree-kernel", Kind::Normal),
        edge("navree-auth", "navree-session", Kind::Normal),
        // `impl navree_access::Subject for navree_auth::User`.
        edge("navree-auth", "navree-access", Kind::Normal),
        edge("navree", "navree-auth", Kind::Normal),
        edge("navree", "navree-macros", Kind::Normal),
        // L6 ecosystem libraries build on L5 and below; the facade re-exports them.
        edge("navree-mcp", "navree-auth", Kind::Normal),
        edge("navree-mcp", "navree-testing", Kind::Normal),
        edge("navree", "navree-mcp", Kind::Normal),
        edge("system-app", "navree", Kind::Normal),
        edge("navree-db", "navree-http", Kind::Dev),
    ];
    assert_eq!(violations(&edges), Vec::<String>::new());
}

#[test]
fn upward_normal_edge_is_reported() {
    let found = violations(&[edge("navree-kernel", "navree-http", Kind::Normal)]);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("navree-kernel") && found[0].contains("navree-http"));
}

#[test]
fn sideways_edge_within_a_layer_is_reported() {
    let found = violations(&[edge("navree-http", "navree-crypto", Kind::Normal)]);
    assert_eq!(found.len(), 1, "{found:?}");
}

#[test]
fn forbidden_dev_and_build_edges_are_reported() {
    let found = violations(&[
        edge("navree-http", "navree-db", Kind::Dev),
        edge("navree-queue", "navree-session", Kind::Build),
    ]);
    assert_eq!(found.len(), 2, "{found:?}");
}

#[test]
fn navree_testing_is_an_allowed_dev_dependency_from_layer_4_up() {
    let edges = [
        edge("navree-session", "navree-testing", Kind::Dev),
        edge("navree-auth", "navree-testing", Kind::Dev),
        edge("system-app", "navree-testing", Kind::Dev),
    ];
    assert_eq!(violations(&edges), Vec::<String>::new());
}

#[test]
fn dev_dependency_on_any_lower_layer_crate_is_allowed() {
    // Tests may boot lower-layer modules (MailModule needs ViewModule).
    let edges = [
        edge("navree-notify", "navree-view", Kind::Dev),
        edge("navree-auth", "navree-view", Kind::Dev),
        edge("navree-session", "navree-cache", Kind::Dev),
    ];
    assert_eq!(violations(&edges), Vec::<String>::new());
}

#[test]
fn dev_dependency_within_the_same_layer_or_upwards_is_reported() {
    let found = violations(&[
        edge("navree-notify", "navree-queue", Kind::Dev),
        edge("navree-view", "navree-http", Kind::Dev),
        edge("navree-session", "navree-auth", Kind::Dev),
    ]);
    assert_eq!(found.len(), 3, "{found:?}");
}

#[test]
fn navree_testing_is_not_a_normal_dependency_of_layer_4() {
    let found = violations(&[edge("navree-session", "navree-testing", Kind::Normal)]);
    assert_eq!(found.len(), 1, "{found:?}");
}

#[test]
fn navree_testing_is_not_a_dev_dependency_below_layer_4() {
    let found = violations(&[edge("navree-db", "navree-testing", Kind::Dev)]);
    assert_eq!(found.len(), 1, "{found:?}");
}

#[test]
fn a_crate_may_dev_depend_on_itself_only() {
    // navree-workspace's tests turn on its own `testing` feature.
    assert_eq!(
        violations(&[edge("navree-workspace", "navree-workspace", Kind::Dev)]),
        Vec::<String>::new()
    );
    let found = violations(&[edge("navree-workspace", "navree-workspace", Kind::Normal)]);
    assert_eq!(found.len(), 1, "{found:?}");
}

#[test]
fn navree_macros_may_dev_depend_on_runtime_crates_only() {
    assert_eq!(
        violations(&[
            edge("navree-macros", "navree-kernel", Kind::Dev),
            edge("navree-macros", "navree-http", Kind::Dev),
            edge("navree-macros", "navree-access", Kind::Dev),
        ]),
        Vec::<String>::new()
    );
    let found = violations(&[
        edge("navree-macros", "navree-http", Kind::Normal),
        edge("navree-macros", "navree", Kind::Dev),
        edge("navree-macros", "navree-cli", Kind::Dev),
    ]);
    assert_eq!(found.len(), 3, "{found:?}");
}

#[test]
fn navree_cli_depends_on_the_tool_crates_only() {
    let found = violations(&[edge("navree-cli", "navree-kernel", Kind::Normal)]);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(
        violations(&[edge("navree-cli", "navree-dock", Kind::Normal)]),
        Vec::<String>::new()
    );
    let found = violations(&[edge("navree-dock", "navree-config", Kind::Normal)]);
    assert_eq!(found.len(), 1, "{found:?}");
}

#[test]
fn navree_cli_uses_harbor_which_uses_no_navree_crate() {
    assert_eq!(
        violations(&[
            edge("navree-cli", "navree-harbor", Kind::Normal),
            edge("navree", "navree-harbor", Kind::Normal),
        ]),
        Vec::<String>::new()
    );
    let found = violations(&[edge("navree-harbor", "navree-crypto", Kind::Normal)]);
    assert_eq!(found.len(), 1, "{found:?}");
}

#[test]
fn unknown_navree_crates_are_reported() {
    let found = violations(&[
        edge("navree-unknown", "navree-kernel", Kind::Normal),
        edge("navree-http", "navree-unknown", Kind::Normal),
    ]);
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(
        found.iter().all(|m| m.contains("navree-unknown")),
        "{found:?}"
    );
}
