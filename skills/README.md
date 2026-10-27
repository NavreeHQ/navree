# Navree agent skills

Agent skills teach coding agents how to build with Navree. Each skill is a
folder with a `SKILL.md`: YAML front-matter (`name` and a `description` that
says when to use it), then the steps, the CLI commands, code templates, the
checks to run and the common mistakes for one task. An agent loads a skill's
description up front and reads the body only when the task matches, so a
project can carry all of them at little cost.

Every skill describes features that exist in the framework, and every API it
names is checked against the code. The code templates compile against the
current `navree` crate.

## The skills

Start with **`navree`**: it explains the framework, the app layout and the
conventions, and says which skill to load for each task.

| Skill | Use it to |
|---|---|
| [`navree`](navree/SKILL.md) | Understand a Navree app and pick the right skill |
| [`navree-new-app`](navree-new-app/SKILL.md) | Create an app with `navree new`, set the key, run `navree dev` |
| [`navree-modules-and-di`](navree-modules-and-di/SKILL.md) | Define modules and providers, wire dependencies, fix graph errors |
| [`navree-controllers-and-routing`](navree-controllers-and-routing/SKILL.md) | Add controllers, routes, extractors and access attributes |
| [`navree-validation`](navree-validation/SKILL.md) | Validate request bodies and query strings |
| [`navree-errors-and-responses`](navree-errors-and-responses/SKILL.md) | Return errors in Navree's JSON shape, map error types, shape responses |
| [`navree-orm`](navree-orm/SKILL.md) | Models, queries, relationships and eager loading, scopes, soft deletes, casts, events, factories, model tests, route model binding |
| [`navree-migrations`](navree-migrations/SKILL.md) | Migrations with the schema builder: tables, columns, indexes, foreign keys, running, rolling back and squashing them |
| [`navree-database`](navree-database/SKILL.md) | Database configuration, the unit of work, transactions in jobs, SeaORM entities and `Repo`, seeders, pagination, TimescaleDB |
| [`navree-auth`](navree-auth/SKILL.md) | JWT sign-in, refresh tokens, the current user, account flows, two-factor, sessions and step-up, impersonation, passwordless and passkeys, platform sign-in, token exchange |
| [`navree-access-control`](navree-access-control/SKILL.md) | Roles, permissions, abilities, record checks and SQL list filters |
| [`navree-queues-and-scheduling`](navree-queues-and-scheduling/SKILL.md) | Jobs, workers, retries, failed jobs, batches, operations with progress, and cron-style schedules |
| [`navree-events-and-broadcasting`](navree-events-and-broadcasting/SKILL.md) | Events and listeners; real-time channels over WebSockets |
| [`navree-mail-and-notifications`](navree-mail-and-notifications/SKILL.md) | Mail with templates and transports; notifications over several channels |
| [`navree-mail-delivery`](navree-mail-delivery/SKILL.md) | Email APIs, tenant transports, the delivery log, suppressions, hold for approval, tracking, DKIM and domain checks |
| [`navree-notification-channels`](navree-notification-channels/SKILL.md) | Notifications over Slack, Teams, SMS, WhatsApp and web push; opt-outs, push subscriptions, the delivery log and provider callbacks |
| [`navree-storage`](navree-storage/SKILL.md) | Disks (local, S3, Google Cloud Storage, Azure), uploads, direct browser uploads, signed links and disk sync |
| [`navree-cache-and-http-client`](navree-cache-and-http-client/SKILL.md) | The cache, locks, and outgoing HTTP with retries and an SSRF guard |
| [`navree-openapi`](navree-openapi/SKILL.md) | The OpenAPI 3.1 document, schemas and typed clients |
| [`navree-frontend-next`](navree-frontend-next/SKILL.md) | A Next.js front end with `@navree/next`, `@navree/client` and `@navree/theme`: the BFF, sign-in, server components, list screens, real time |
| [`navree-frontend-nuxt`](navree-frontend-nuxt/SKILL.md) | A Nuxt front end with the `@navree/nuxt` module: the BFF as Nitro routes, the session gate, composables, list screens |
| [`navree-config`](navree-config/SKILL.md) | `.env`, `config/app.toml`, `APP_*` variables, the app key, typed sections |
| [`navree-testing`](navree-testing/SKILL.md) | `TestApp`, HTTP tests, database tests and fakes |
| [`navree-cli-generators`](navree-cli-generators/SKILL.md) | The `navree` CLI, every `make:*` generator and console commands |
| [`navree-deployment`](navree-deployment/SKILL.md) | Release builds, processes, systemd, proxies and health checks |
| [`navree-security-checklist`](navree-security-checklist/SKILL.md) | What to check before shipping |
| [`navree-jose`](navree-jose/SKILL.md) | Sign and verify JWTs that cross the app boundary, load `.p8` and PEM keys, publish a JWKS |
| [`navree-module-conformity`](navree-module-conformity/SKILL.md) | Audit a module with `navree check`, isolation suites and `navree gate` |
| [`navree-workspaces`](navree-workspaces/SKILL.md) | Multi-tenancy at the workspace level, with organizations above and partitions inside |
| [`navree-time-zones`](navree-time-zones/SKILL.md) | Zones for users and workspaces, local days and report windows, send windows, holidays, local-time schedules |
| [`navree-billing`](navree-billing/SKILL.md) | Subscriptions, trials, seats, metered usage, invoices and webhooks with Stripe or Paddle |
| [`navree-lighthouse`](navree-lighthouse/SKILL.md) | Application monitoring: issues, alerts, the `/_lighthouse` dashboard and its JSON API |
| [`navree-fleet`](navree-fleet/SKILL.md) | Supervised queue workers (`navree fleet`), balancing, recycling, pause and continue, the `/_fleet` dashboard, metrics and long-wait alerts |
| [`navree-dock`](navree-dock/SKILL.md) | Run PostgreSQL, Redis, Meilisearch, Mailpit, MinIO and NATS locally and in CI with `navree dock` |
| [`navree-harbor`](navree-harbor/SKILL.md) | Provision Ubuntu servers and ship zero-downtime releases with `navree deploy`: `harbor.toml`, secrets, rollbacks, several servers, `build:image`, GitHub Actions |
| [`navree-social`](navree-social/SKILL.md) | Sign in with Google, GitHub, Apple and other providers; linked accounts |
| [`navree-sso`](navree-sso/SKILL.md) | SAML 2.0 and OpenID Connect single sign-on per organization or workspace, verified domains, provisioning |
| [`navree-webhooks`](navree-webhooks/SKILL.md) | Signed outgoing webhooks with retries, and verified incoming ones |
| [`navree-integrations`](navree-integrations/SKILL.md) | Workspace and organization connections to Microsoft 365, Google Workspace, Slack, OAuth 2.0 APIs and API keys: consent flows, service accounts, scope sets, typed clients, health checks, `IntegrationsFake` (feature `integrations`) |
| [`navree-flags`](navree-flags/SKILL.md) | Feature flags: define, check, roll out by percentage or plan, kill, test |
| [`navree-settings`](navree-settings/SKILL.md) | Typed settings per app, organization, workspace or user, inherited field by field with the source of every value |
| [`navree-search`](navree-search/SKILL.md) | Full-text and vector search over entities, engines and index syncing |
| [`navree-telemetry`](navree-telemetry/SKILL.md) | Telemetry events and sinks, OpenTelemetry tracing and OTLP export, trace context, `TelemetryRecorder` and `SpanRecorder` in tests |
| [`navree-graphql`](navree-graphql/SKILL.md) | GraphQL resolvers, dataloaders and subscriptions (feature `graphql`) |
| [`navree-grpc`](navree-grpc/SKILL.md) | gRPC, gRPC-Web and Connect services in modules, gRPC clients, and their tests |
| [`navree-mcp`](navree-mcp/SKILL.md) | An MCP server for the product: tools, resources and prompts for AI clients |
| [`navree-ai`](navree-ai/SKILL.md) | Models in your app: text, structured output, streaming, failover, budgets, agents with tools, embeddings, vector stores, `AiFake` |
| [`navree-redis`](navree-redis/SKILL.md) | Redis connections and the Redis cache, locks, queue, sessions, rate limits and real-time backplane (feature `redis`) |
| [`navree-logbook`](navree-logbook/SKILL.md) | The local debugging timeline at `/_logbook`: filters, N+1 queries, replay, query counts in tests |
| [`navree-microservices`](navree-microservices/SKILL.md) | Messages and events between services: handlers, `ServiceClient`, the outbox and inbox, dead letters, the transports, and `MicroFake` tests (feature `micro`) |
| [`navree-helm`](navree-helm/SKILL.md) | The Helm admin panel: resources, fields, filters, actions, metrics, custom pages |
| [`navree-navigator`](navree-navigator/SKILL.md) | Connect a coding agent to Navigator (`navree mcp`), use its tools, install the skills |
| [`navree-i18n`](navree-i18n/SKILL.md) | Localization: Fluent files, `t!`, the request's locale, formatting, translated validation, mail and notifications (feature `i18n`) |
| [`navree-browser-testing`](navree-browser-testing/SKILL.md) | Browser tests with navree-browser: Chrome over the DevTools Protocol, waiting steps, failure screenshots, `TestServer` and `FrontEnd` |
| [`navree-performance`](navree-performance/SKILL.md) | Measure and lower per-request cost: the pipeline benchmarks, pool and transaction settings, hot-path habits |
| [`navree-documents`](navree-documents/SKILL.md) | PDF from HTML templates (native engine or Chrome) or `PdfDocument`, DOCX, merge and stamps, queued renders, `DocumentsFake` (feature `documents`) |
| [`navree-media`](navree-media/SKILL.md) | Files attached to models (`HasMedia`, collections), uploads and remote files, image conversions and `srcset`s, `media:regenerate`, zip archives, text extraction, FFmpeg video, `MediaFake` (feature `media`) |
| [`navree-sheets`](navree-sheets/SKILL.md) | CSV and Excel exports (downloads, queued files with signed links) and imports (encoding detection, header mapping, a staging preview, duplicate modes, error files) (feature `sheets`) |
| [`navree-workflows`](navree-workflows/SKILL.md) | Durable workflows (`#[workflow]`, steps, sleeps, event waits, versions) and end-user automations (`#[trigger]`, `#[condition]`, `#[action]`, versioned definitions, enrollments, simulation) (feature `workflows`) |

## Installing

Pick one:

- **With the skills CLI:** `npx skills add NavreeHQ/navree` installs every
  skill of this repository for the agents it finds in your project.
- **By hand:** copy the folders of this directory into your project's
  `.claude/skills/` (Claude Code) or `.agents/skills/` (other agents). Copy
  only the ones you need if you like, but keep `navree`: it indexes the
  rest.
- **With the navree CLI:** `navree skills:install` copies the skills of
  the CLI's Navree version into the current project's `.claude/skills/`
  and `.agents/skills/` (`--path <dir>` names another project), replacing
  the `navree` and `navree-*` folders and leaving other skills alone.

Apps created with `navree new` also get an `AGENTS.md` with the project's
conventions (and a `CLAUDE.md` that imports it), so an agent knows the
layout, the generators and the checks to run before it loads any skill.

## Docs for agents

The skills link to the documentation at https://navree.com/docs/. Models
can read it as plain text: https://navree.com/llms.txt indexes every page,
https://navree.com/llms-full.txt holds all of them, and every page is
available as Markdown at `https://navree.com/docs/<section>/<page>.md`.

## Writing a skill

Keep a skill to one task. The `description` says what the skill does and
exactly when to use it, with the words a user would say. The body gives the
steps in order, the commands, a template that compiles, the checks to run,
the mistakes to avoid and links to the docs. Name only APIs that exist:
grep for each one in `crates/` before writing it down.
