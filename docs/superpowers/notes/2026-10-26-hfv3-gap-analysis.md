# HFv3 on Navree: gap analysis

Date: 2026-10-26. Worker: `gaps` (research only). Read with the site spec
(`docs/superpowers/specs/2026-10-26-navree-site-v2-design.md`) and the
ecosystem build plan (`docs/superpowers/plans/2026-10-26-navree-ecosystem-build-plan.md`);
task keys in brackets (`[webhooks]`, `[billing-core]`) are that plan's keys.

**Question.** What must Navree provide so a product the size of HFv3 can be
built on it end to end, and what is missing today, after counting what the
ecosystem run is already building?

**Answer in one paragraph.** The core (modules and DI, HTTP, validation,
SeaORM, auth with two-factor, access rules with CASL JSON, audit, cache,
events, queues, schedules, mail, notifications, storage, real time, OpenAPI,
config, testing) covers about half of what HFv3 uses. The ecosystem run
covers another quarter: Redis, OpenTelemetry, webhooks, flags, social sign-in,
the OAuth server, billing, search, the AI SDK and agents, RAG, MCP,
Lighthouse, Fleet, Helm, Harbor, Dock, localization, browser testing and the
starter kits. The remaining quarter is the list in section 5: **36 build
items** (34 in the framework, 2 in the front-end layer), plus the agent
skills in section 6. Eight items are P0: no HFv3-shaped product can ship
without them. They are organizations above workspaces, tenant-aware mail
delivery, list queries, error codes, time zones, S3 storage, entitlements
with manual plans, and job batches with progress. The front-end layer (shared
packages, the Nuxt kit, the dark theme) and seven new agent skills complete
the picture.

Sources read (read-only): `D:/Infosec/hfv3-apps` (root and app `CLAUDE.md`,
`apps/api/package.json`, `apps/web/package.json`, `apps/lander`, `apps/observability`,
`docker-compose.yml`, every `apps/api/src/modules/*` module file and
controller prefix, `src/auth`, `src/common`, `src/redis`, the schema and
migration counts, `tools/*`, and 70 project skills in `.claude/skills`), the
user's global skills (`nestjs-enterprise-backend`, `prototype-to-saas`,
`claude-design-to-app`, `encore-backend`), and Navree's crates, docs and 22
skills.

---

## 1. HFv3 at a glance

| Fact | Value |
|---|---|
| Product | HumanFirewall: human-risk management and security-awareness training. Organizations run consented phishing, smishing and vishing simulations against their own staff, then train them (LMS, just-in-time coaching, a gamified trainer), score human risk and triage reported mail. |
| Shape | pnpm monorepo. `apps/api` (NestJS 11 on Express 5, SWC), `apps/web` (Next.js 16, React 19, Tailwind 4, shadcn, TanStack Query and Table, Zustand), `apps/lander` (Nuxt 4: simulation landing pages), `apps/addin` (Outlook add-in), `apps/observability` and `apps/dev-insights` (FastAPI sidecars), `tools/*` (Tauri desktop agents, a video renderer, a SCORM packager, a node editor, a document builder). |
| Size | 75 API modules (`src/modules`), 432 controllers (411 in modules, 21 in `src/auth`, `src/common` and `src/health`), 239 Drizzle schema files, 280 SQL migrations, about 100 BullMQ queues, 67 files with `@Cron`. |
| Data | PostgreSQL 16/17 with TimescaleDB, Drizzle ORM, Redis 7 (cache, queues, leases). |
| Processes | `api` and `worker` (separate entry points; consumers only in the worker), plus web, lander and the sidecars. PM2 behind nginx. |
| Tenancy | Company (billing) → Workspace (configuration, SSO, settings) → Domain (data partition, the hot-path filter). Every table has `companyId`, `workspaceId` and `domainId`. MSSP partners manage many companies. |
| Instances | India production, staging, UAE in-country (data residency, AI commentary off), demo boxes. |
| Rules that shape the framework needs | Frontend drives backend. Soft deletes only. Additive, schema-only migrations. Reports aggregate in SQL. Every module is locale-ready (en, ar, hi, fr, right-to-left) and time-zone-correct from its first commit. Refusals carry an `errorCode`. One gate script before every push. |

---

## 2. Module inventory (75 modules)

One line each. Grouped by area; the counts of controllers come from the code.

### Identity, tenancy and administration

| Module | Purpose |
|---|---|
| `access-requests` | People ask to join a company; admins approve (creating the user) or decline. |
| `ad-sync` | Endpoints for the desktop AD/LDAP syncer: sync tokens and run logs. |
| `admin` | The platform super-admin console API (35 controllers): companies, analytics, AI credits, async jobs, audit logs, automation, customer success, content and AI-studio playgrounds, per-company LMS catalog. |
| `audit-logs` | Workspace-wide reader over the shared `audit_logs` table that the global audit interceptor fills. |
| `billing` | Subscription and usage view per company: tier, status, seat and recipient ceilings read through the enforcement code. No payment provider. |
| `companies` | Company profile and company settings (the top tenant, the billing entity). |
| `domains` | Domains inside a workspace: the data partition every CRUD query filters by. |
| `hfv2-bridge` | Sign-in bridge from the previous product generation: single-use tokens, opt-in kill switch. |
| `identity-links` | Linked sign-in methods per user and the "sign-in method connected" notice. |
| `meta` | Reference data for forms (time zones). |
| `module-entitlements` | Registry of product modules with a minimum tier, always-on modules, per-company toggles, route-prefix gating and `/me/modules` for navigation. |
| `onboarding` | Company onboarding: primary domain and default sender addresses. |
| `persons` | One human across domains (company-scoped pointer, no PII), with match suggestions to approve. |
| `roles` | Customer-defined company roles built from the permission vocabulary. |
| `settings` | Platform settings surfaces (SMTP, AI chat, tooltips). |
| `team` | Company team members and invitations. |
| `users` | Users, role changes (revoking sessions), user preferences. |
| `view-preferences` | Per-user table and view preferences, in Redis. |
| `workspaces` | Workspaces inside a company. |

### Delivery, messaging and integrations

| Module | Purpose |
|---|---|
| `delivery-transports` | Credentials per sending domain for SMTP, Microsoft Graph and Gmail; direct-delivery fallback. |
| `email-delivery` | Transactional mail settings and the resolver that routes system mail through SMTP, Graph or Gmail. |
| `email-gate` | Outgoing-mail gate: send-time checks and an admin outbox that holds mail for approval. |
| `email-logs` | Read-only email log (campaign and auth mail) per workspace. |
| `email-rendering` | Global renderer (React Email templates, CSS inlining) for every send path. |
| `employee-suppressions` | Per-recipient suppression of campaigns, LMS or announcements. |
| `inbox` | Unified inbound message inbox for every module, full-text searchable (tsvector). |
| `integrations` | Google service accounts, Microsoft app registrations, SES notification setup, integration health, per-workspace integrations. |
| `notification-delivery` | Delivery engine for catalog notification mail: waiting list, delivery settings, gating. |
| `notifications` | Notification catalog and per-module email styles, with inherited settings. |
| `teams-bot` | Microsoft Teams bot: messages and actionable cards. |
| `webhooks` | Inbound pushes: Gmail (Pub/Sub, OIDC-verified) and Microsoft Graph change notifications. |
| `tracking-domains` | Tenant tracking and landing domains: DNS verification, certificate issuance (certbot), nginx configuration. |

### Platform services

| Module | Purpose |
|---|---|
| `async-jobs` | Generic async-job kernel: a registry (start, progress, complete, fail, cancel), a stale-job watchdog, per-queue listeners. |
| `brand-logos` | Brand logo library with PNG streaming and a three-tier cache. |
| `client-errors` | Browser error reports from the web app. |
| `dashboard-widgets` | Home-widget framework: widget registry, contribution sources, layouts. |
| `home-dashboard` | Home dashboard composition, including the Human Risk Index. |
| `images` | SSRF-safe re-hosting of external images. |
| `uploads` | Generic image upload for the block editor. |
| `shared` | Cross-module helpers: audience preview, campaign recipients, remedial policy, reminders. |
| `feedback` | In-app tickets with threads, screenshots, status history and clusters, plus an AI triage, fix and deploy pipeline. |
| `capture` | Events from simulation landing pages (fingerprints, engagement), tenant resolved by origin. |

### Simulation product

| Module | Purpose |
|---|---|
| `phishing` | Phishing campaigns (43 controllers): targets, templates, landing pages, AI drafts, bot management, brand themes, dashboards. |
| `smishing` | SMS, WhatsApp and RCS simulations: campaigns, templates, transports, opt-outs. |
| `vishing` | Voice simulations: campaigns, attempts, voice clones and providers, live call streams (LiveKit). |
| `blueprints` | Campaign blueprints deployed to one or more workspaces. |
| `preflight` | Pre-send checks (delivery credentials, allowlisting). |
| `risk-assessment` | Risk scoring behind the first agent of the AI phishing flow. |
| `er` | Email reporting and remediation: report button add-in and cards, reported mail, threat logs, search and remediate across mailboxes. |
| `esa` | Email security analyzer: ingests mail from Gmail and Graph, scores it, tags it, adds banners; trusted senders. |
| `entities` | Organizational entities (departments, groups) with derivation rules and risk drill-downs. |
| `recipients` | Employees: groups, custom fields, import and export, bulk operations, org chart, data quality, recycle bin. |

### Training and engagement

| Module | Purpose |
|---|---|
| `courses` | The LMS (31 controllers): courses, assignments, live sessions, certificates and templates, automations, analytics. |
| `lms` | The LMS assistant tool manifest (the LMS itself lives in `courses`). |
| `jit-training` | Just-in-time coaching: endpoint agent pairing, EDR pollers (CrowdStrike, Entra, Defender), dispatch. |
| `phishtrainer` | Gamified phishing trainer: programs, rounds, player, invites, generations, reports. |
| `gamification` | Points ledger, badges, levels, leaderboards, streaks, challenges. |
| `portal` | Employee portal API: home, enrollments, certificates, leaderboard, challenges, news. |
| `portal-news` | Portal news. |
| `portal-settings` | Portal settings and themes. |
| `team-portal` | Manager dashboards over their reportees. |
| `team-portal-settings` | Visibility settings for manager dashboards. |
| `announcements` | Security announcement campaigns: categories, templates, landing pages, reminders, reports. |
| `surveys` | Surveys: templates, responses (portal and public), results. |
| `policies` | Policy library with versions, attestations, exemptions, evidence, external auditors and audit reports. |
| `knowledge` | Knowledge base: ingest, chunk, embed (pgvector plus tsvector), answer questions in the portal. |

### Content, AI and reporting

| Module | Purpose |
|---|---|
| `ai-chat` | In-app assistant: Anthropic, OpenAI and Gemini providers, tool calls over the app's own API from generated manifests, SSE streaming. |
| `ai-metering` | AI credits: allowance per caller, monthly statement, charge ledger. |
| `ai-studio` | AI content generation sessions and drafts, attachment previews. |
| `content-studio` | Decks, palettes, ideas, news, free-form pages, header images, video jobs and exports. |
| `risk-bot` | Answers questions about human-risk data in the screen's language. |
| `vcro` | Virtual Cyber Risk Officer: proposes and runs actions across LMS, announcements and phishing, measured nightly (a sold module). |
| `reports` | Scheduled reports and their delivery cron; risk reports. |
| `company-reports` | Company-level reports. |
| `workflows` | End-user automations: block catalog, templates, enrollments, engine, review queue, simulation. |

Outside `src/modules`: `src/auth` (JWT and Passport strategies, API keys with
permission lists, CASL ability factory and permission vocabulary, passkeys,
two-factor and its enforcement guard, sessions, magic links, email OTP, Google
and Microsoft OAuth, employee-portal SAML and OIDC, SCORM launch tokens, a
separate super-admin sign-in with step-up re-authentication, impersonation
guards, tenancy services), `src/common` (about 70 helper areas: audit,
channels, crypto and credential encryption with re-encryption after key
rotation, DNS and Cloudflare, certbot, nginx, error journal, forensics and bot
detection, locale, time, PDF, phone, sanitize, SSE, storage with S3,
throttling, validators) and `src/redis` (connections, cron leases, worker drain).

---

## 3. Platform capabilities HFv3 relies on

Each line names the evidence in HFv3.

1. **Authentication.** Passwords with JWT access tokens and sessions; TOTP two-factor with an enforcement guard; passkeys (`@simplewebauthn/server`); magic links; email OTP; Google and Microsoft sign-in with admin consent; SAML (`samlify`) and OIDC discovery for the employee portal; SCORM launch tokens; API keys with fixed permission lists; a separate super-admin sign-in with its own JWT, throttle and step-up re-authentication; impersonation (`@BlockedDuringImpersonation`, `@ImpersonationOnly`); a token bridge from another system.
2. **Directory sync, no SCIM.** LDAP/AD through a desktop syncer (`ldapts`), Google Workspace (service accounts, domain-wide delegation), Microsoft Graph, Okta; `RECIPIENT_SYNC` and `STAGING_ANALYZE` queues.
3. **Multi-tenancy.** Company → Workspace → Domain on every row; `TenantContextGuard`; parameters ordered `companyId → workspaceId → domainId → id`; MSSP partners; one person across domains.
4. **Authorization.** CASL abilities from roles, a permission vocabulary, built-in grants, customer-defined roles, admin consoles with their own roles, a module-access guard, guards that refuse assistant-originated calls (`NotFromAssistant`) and non-humans (`HumanOnly`).
5. **Settings inheritance.** Global → Company defaults → Workspace overrides, with `_fieldSources` in responses so the UI shows where each value comes from.
6. **Library content.** Templates, courses, scenarios and badges in Global, Company and Workspace scope, cloned on use (`hfv3-library-module`).
7. **Audit.** An `@Audit` decorator and interceptor, per-module audit logs, admin audit logs, impersonation visibility, value redaction, a TimescaleDB hypertable.
8. **Queues and job orchestration.** About 100 BullMQ queues, a worker process, an async-job registry with progress, a watchdog and ownership, SSE job progress, drain on shutdown.
9. **Cron.** `@nestjs/schedule` everywhere, made single-run with Redis leases (`acquireCronLease`, owned leases with renewal); people-facing jobs run at 09:00 workspace time, Monday to Friday (`claimLocalSlot`).
10. **Webhooks in and out.** In: Gmail Pub/Sub with OIDC verification, Graph change notifications, SES notifications over SNS, Twilio callbacks. Out: ticket integration pushes.
11. **Email delivery.** SMTP (nodemailer), SES v2 with two lanes (simulation and notification), Graph `sendMail`, Gmail API insert, Mailgun; transports resolved in five tiers (domain, workspace, company, platform row, environment); delivery log, outbox gate, suppressions, tracking, sending-domain readiness (SPF, DKIM, DMARC).
12. **Email rendering.** React Email templates, CSS inlining (`juice`), plain-text generation, HTML sanitization.
13. **Notifications.** A notification catalog with per-module styles and inherited settings, a delivery engine with a waiting list, Teams delivery, the in-app inbox.
14. **Uploads and media.** Multipart uploads, `sharp` (resize, convert, rasterize logos), SSRF-safe re-hosting, S3 (`@aws-sdk/client-s3`, presigner, multipart upload) with a local hydrate and a reconcile job, an FFmpeg video renderer, zip archives (SCORM), PDF and DOCX text extraction for the knowledge base.
15. **PDF, Excel, CSV.** `pdfkit`, `pdf-lib` with fonts, Puppeteer (HTML certificates to PDF), `docx`, `xlsx`, CSV with a byte-order mark and encoding detection on upload.
16. **SCORM and LMS.** SCORM token guard, LMS telemetry injector, package languages, a SCORM packager for video lessons.
17. **Languages.** A web catalogue in en, ar, hi and fr with ICU plurals; logical (right-to-left-safe) layout classes; API refusals as `errorCode`s mapped to keys; country-first tenant defaults.
18. **Time zones.** Workspace zone as the tenant zone, calendar-day windows counted in workspace days, send windows, public holidays, PostgreSQL-safe zone names (`resolvePostgresTimeZone`), dates bound as UTC.
19. **AI.** Three provider SDKs; a model catalog with tiers and policy; structured output with JSON salvage; an assistant that calls the app's own routes from manifests generated from the OpenAPI document, with destructive operations classified; SSE streaming; a credits ledger; RAG over pgvector and tsvector; image generation; voice agents (LiveKit).
20. **Search.** PostgreSQL full text (tsvector, generated columns) and pgvector (1536 dimensions).
21. **Analytics and reports.** Aggregates in SQL only, TimescaleDB hypertables and rollups, scheduled reports by mail, a widget registry, PDF reports.
22. **Real time.** Server-sent events (job progress, AI chat, live activity), LiveKit for voice. No WebSockets.
23. **Caching.** Redis caches (logos, entitlements, lander domains), view preferences in Redis.
24. **Rate limiting.** `@nestjs/throttler` on 64 files, a tracking throttle, an nginx client throttle, sign-in throttles.
25. **Feature entitlements.** Module registry by tier, per-company overrides, plan limits (seats, recipients), a sold module (vCRO), demo mode.
26. **Billing.** Operator-set tiers and seats; no payment provider (a `stripe_customer_id` column is unused).
27. **Observability.** Sentry on API and web, an error journal with fingerprints and issues, a job-failure reporter, a log journal, a FastAPI observability service (database metrics, queue depth, sessions, nginx status, uptime history), health endpoints.
28. **Client error capture.** `client-errors`, a browser error reporter, Sentry in Next.js.
29. **Deployment and infrastructure.** Docker Compose (TimescaleDB, Redis, migrate, api, worker, web, lander); PM2 and nginx on DigitalOcean and Azure; Cloudflare; per-tenant certificates; three production-grade instances; release records per deploy; nightly `pg_dump`; stack profiles so two checkouts run side by side.
30. **Testing.** Jest (about 3,000 API spec files), in-process PostgreSQL (PGlite), an integration lane on Docker PostgreSQL, Playwright, guard specs that read source files, a gate script with a cache and memory budgets. There is no CI.
31. **API conventions.** URI versions (`/v1`), Zod and class-validator DTOs, response DTOs with a static `from()`, one paginated envelope with server-side filters, sort and facets, Title Case enum values, `errorCode`s, soft deletes, Swagger.
32. **Configuration.** `@nestjs/config`, a required-boot-variable list, an `ENVIRONMENT.md` contract.
33. **Migrations and ORM.** Drizzle and drizzle-kit, additive and schema-only migration rules, data repairs as dry-run-first scripts, layered seeds (global, tenant, top-up), a REPL.
34. **Security extras.** SSRF-safe fetch, HTML sanitizer, credential encryption with re-encryption after key rotation, signed URLs, bot detection and IP forensics, Helmet, CORS allowlists.

---

## 4. Mapping: HFv3 capability → Navree

Legend: **Built** names the crate. **Run** names the ecosystem-plan key that
is building it now. **Gap** names the item in section 5.

| # | Capability | Built | Run | Gap |
|---|---|---|---|---|
| 1 | Password sign-in, JWT, rotating refresh tokens, TOTP two-factor, recovery codes, account flows, sign-in throttle | navree-auth | | |
| 1 | Session list and revoke, impersonation, step-up, magic links, email OTP, two-factor policy per tenant, token exchange | | | `auth-plus` |
| 1 | Passkeys | | after launch (roadmap) | `auth-plus` (phase 2) |
| 1 | Google, Microsoft and other social sign-in | | `[social]` | |
| 1 | Enterprise SSO (SAML 2.0, OIDC per tenant, verified domains, JIT provisioning) | | | `sso` |
| 1 | API keys with abilities | | `[oauth-core]` (personal access tokens) | |
| 2 | Directory sync (SCIM server, Graph, Google, LDAP pull) | | | `directory-sync` |
| 3 | Workspaces: members, roles, invitations, resolution, row/schema/database isolation | navree-workspace | `[postgres]` verifies RLS | |
| 3 | Organizations above workspaces, partitions inside them, org-wide roles, access requests | | | `organizations` |
| 4 | Roles, permissions, CASL abilities, record checks, SQL list filters, deny by default | navree-access, navree-http | | |
| 4 | Customer-defined roles | navree-access (roles in the database) | | |
| 5 | Settings with inheritance and value sources | | | `settings` |
| 6 | Library content in three scopes with clone on use | | | `library-scopes` |
| 7 | Audit of data changes, sign-ins and permission changes | navree-audit | | |
| 7 | Audit on a hypertable, impersonator on entries | | | `db-timescale`, `auth-plus` |
| 8 | Jobs, retries, backoff, unique and delayed jobs, failed jobs | navree-queue | `[redis-queue]` | |
| 8 | Queue dashboard and supervised workers | | `[fleet]` | |
| 8 | Batches, progress, user-visible operations, SSE progress, watchdog | | | `queue-batches` |
| 8 | Durable workflows and end-user automations | | | `workflows` |
| 9 | Cron schedules, single-node runs, no overlap | navree-schedule | | |
| 9 | Schedules in each tenant's local time, send windows | | | `time-zones` |
| 10 | Outgoing signed webhooks, incoming verified (Stripe, GitHub, Standard Webhooks) | | `[webhooks]` | |
| 10 | Incoming from AWS SNS, Google Pub/Sub, Microsoft Graph, Twilio, Slack | | | `webhook-sources` |
| 11 | Mail over SMTP, failover, queued, templates | navree-mail | | |
| 11 | SES, Graph, Gmail and HTTP-API transports, tenant transport tiers, delivery log, suppressions, hold-for-approval, domain checks | | | `mail-delivery` |
| 11 | Inbound mail | | | `mail-inbound` |
| 12 | MiniJinja mail templates | navree-view, navree-mail | | |
| 12 | CSS inlining and HTML-to-text | | | `mail-delivery` |
| 13 | Notifications over mail, database and broadcast, read state | navree-notify | | |
| 13 | Slack, Teams, SMS, WhatsApp, web push channels | | | `notify-channels` |
| 13 | Preferences, digests, quiet hours, per-tenant templates | | | `notify-preferences` |
| 14 | Disks, streaming, signed URLs, content-checked uploads | navree-storage (local, memory) | | |
| 14 | S3, GCS, Azure disks, presigned uploads | | after launch (roadmap) | `storage-s3` |
| 14 | Image conversions, attachments, text extraction, archives, video | | | `media` |
| 15 | CSV and Excel import and export | | | `sheets` |
| 15 | PDF and DOCX | | | `documents` |
| 16 | SCORM runtime and packages | | | app-level (section 7), built on `media` and `storage-s3` |
| 17 | Translations, plurals, locale negotiation, translated validation | | `[i18n]` | |
| 17 | Machine-readable error codes in the JSON body | navree-http has `AppError.code` but does not render it | | `error-codes` |
| 17 | Phone (E.164), HTML sanitizing, Unicode names, digit folding | | | `validation-intl` |
| 18 | Time zones: tenant zone, local days, windows, holidays | | | `time-zones` |
| 19 | AI text, structured output, streaming, failover, budgets per user | | `[ai-core]`, `[ai-providers]` | |
| 19 | Agents and tools, memory | | `[ai-agents]` | |
| 19 | Embeddings and RAG (pgvector, Qdrant) | | `[ai-rag]` | |
| 19 | MCP servers | | `[mcp]` | |
| 19 | Workspace credit ledger, allowance tiers, statements | | | `ai-credits` |
| 19 | The app's own routes as agent tools, destructive-call approval | | | `ai-route-tools` |
| 19 | Image generation, speech, transcription | | | `ai-media` |
| 20 | Full-text and vector search | | `[search-core]`, `[search-engines]`, `[search-vectors]` | |
| 21 | Aggregates, pagination | navree-db | | |
| 21 | Hypertables, retention, compression, continuous aggregates | | | `db-timescale` |
| 21 | Report types, schedules, exports, dashboard widgets | | | `reports` |
| 22 | Pusher-protocol WebSockets (public, private, presence) | navree-realtime | `[redis-realtime]` backplane | |
| 22 | SSE responses | Axum `Sse` through navree-http (compression skips `text/event-stream`) | | `queue-batches` adds the progress stream |
| 23 | Cache, tags, locks, stale-while-revalidate | navree-cache | `[redis-core]` | |
| 24 | Rate limits, throttles per route | navree-http | `[redis-core]` store | |
| 25 | Flags per user, workspace, plan, percentage | | `[flags]` | |
| 25 | Entitlements (modules and limits per plan), operator-assigned plans | | | `entitlements` |
| 25 | Demo workspaces | | | `demo-workspaces` |
| 26 | Subscriptions, seats, trials, metering, invoices (Stripe, Paddle) | | `[billing-core]`, `[billing-stripe]`, `[billing-paddle]` | |
| 27 | Logs with redaction, health, Prometheus metrics | navree-log, navree-observe | | |
| 27 | Traces | | `[otel]`, `[instrument]` | |
| 27 | Exceptions, slow requests and queries, failed jobs, alerts | | `[lighthouse-core]`, `[lighthouse-alerts]`, `[lighthouse-ui]` | |
| 27 | Local debugging timeline | | `[logbook]` | |
| 28 | Browser errors, source maps, uptime checks, host metrics | | | `lighthouse-plus` |
| 29 | Local services | | `[dock]` | |
| 29 | Provisioning, zero-downtime deploys, rollbacks, backups, several servers | | `[harbor-provision]`, `[harbor-deploy]`, `[harbor-config]`, `[build-image]` | |
| 29 | Tenant custom domains with automatic TLS | | | `domains` |
| 29 | Admin panel | | `[helm-api]`, `[helm-ui]`, `[helm-cli]` | |
| 30 | `TestApp`, in-process HTTP tests, fakes, SQLite per test | navree-testing | `[test-services]` for PostgreSQL | |
| 30 | Browser journeys | | `[browser]` | |
| 30 | Tenant isolation suites, conformity checks, one gate command | | | `conformity-checks` |
| 31 | `/api` prefix and versions, validation, one JSON error shape, OpenAPI 3.1 | navree-http, navree-validation, navree-api | | |
| 31 | Filters, sorts, search and facets from the query string; select-all bulk actions; OpenAPI query parameters | | | `list-query` |
| 31 | Soft deletes and a recycle bin | | | `soft-deletes` |
| 32 | Typed, validated configuration, `APP_*` variables, app key rotation | navree-config, navree-crypto | | |
| 33 | Module-owned migrations, seeders, factories, unit of work | navree-db | `[postgres]` | |
| 33 | Zero-downtime migration lint, data repair commands | | | `migration-safety` |
| 34 | SSRF guard, signed URLs, encryption with key rotation, security headers, CORS, CSRF | navree-http-client, navree-crypto, navree-http, navree-session | | |
| — | gRPC, GraphQL, microservices (not used by HFv3, useful for its sidecars) | | `[grpc]`, `[graphql]`, `[micro-*]` | |
| — | Next.js SaaS kit | `--ui next` template | `[kit-saas-api]`, `[kit-saas-web]`, `[kit-api-micro]` | |
| — | Nuxt front end and kit, shared front-end packages, the dark theme | `apps/crm-nuxt` in progress (untracked) | | `kit-nuxt`, `frontend-layer` |

---

## 5. Missing capabilities: design sketches

Every sketch follows Navree's conventions: `XModule::{for_root, from_config,
for_feature::<T>()}`, one config section per module with `APP_<SECTION>__<KEY>`
variables, options with `#[serde(default)]`, `AppError` responses, `&Uow`
first on writes, events after commit, commands named `<section>:<verb>`,
migration sets named after the section, `ModuleBuilder<X>Ext` extensions,
deny-by-default routes, and a fake in the crate. Layers follow the plan's §3
(L6 ecosystem libraries, L7 packages on packages, facade at L8).

Priorities: **P0** blocks an HFv3-shaped product; **P1** needed for a full
port; **P2** needed for parity; **P3** polish.

### 5.1 Core additions (small, unblock everything)

**`error-codes` · P0 · S · navree-http.** Render `AppError.code` in the body:
`{ "statusCode": 409, "message": "...", "error": "Conflict", "code": "campaign_already_running" }`.
`AppError::with_code("...")`; stable codes for framework errors
(`validation_failed`, `not_found`, `unauthenticated`, `two_factor_required`,
`limit_reached`, `demo_read_only`); `http.errors.code = true` (default on);
the OpenAPI error schema gains `code`. The front-end kits map `code` to a
translation key, which is how a refusal reaches an Arabic user in Arabic.
HFv3: every module (rule 9, `errorCode` on refusals).

**`soft-deletes` · P1 · M · navree-db.** `impl SoftDelete for Entity`
(columns `deleted_at`, `deleted_by`); `Repo::delete` stamps them; queries
exclude deleted rows unless `.with_trashed()`; `Repo::restore`,
`Repo::force_delete`; `db:prune --soft-deleted --older-than 90d`; audit
actions `*.deleted`, `*.restored`; a `RecycleBin<T>` controller helper.
HFv3: recipients (recycle bin), every CRUD module ("soft deletes only").

**`list-query` · P0 · L · navree-db, navree-api, navree-macros.**
`#[derive(Listable)]` on a resource with `#[list(filter(status, owner_id, created_at), sort(created_at, name), search(name, email), facets(status))]`;
a `ListQuery<R>` extractor that parses `filter[status]=Active`,
`filter[created_at][gte]=2026-10-01`, `sort=-created_at`, `search=`,
`page`/`per_page` or `cursor`; `list.apply(select)` builds the SeaORM query
(combined with `Accessible` and `WorkspaceRepo`); `Page<T>` carries
`meta.facets`; `BulkSelection` turns "all matching this filter" into a safe
bulk operation; OpenAPI query parameters and a `Page<T>` schema (the plan's
core backlog item). HFv3: `applyServerListQuery`, `bulk-select-all-filters`,
`filter-facet.transforms`, every `UnifiedDataTable` list.

**`storage-s3` · P0 · M · navree-storage.** Drivers `s3` (AWS, MinIO,
Cloudflare R2, DigitalOcean Spaces), `gcs`, `azure` on `object_store` 0.14
with `aws-base`/`gcp-base`/`azure-base` and ring (plan §11); presigned upload
URLs (`disk.presigned_put(path, ttl, rules)`) for direct browser uploads;
signed GETs; `storage:sync` reconcile between disks. Config
`storage.disks.<name>.driver = "s3"`, `bucket`, `region`, `endpoint`,
`access_key`, `secret_key`. HFv3: `common/storage` (S3, hydrate, reconcile),
uploads, images, knowledge, content-studio.

**`validation-intl` · P2 · S · navree-validation.** `Phone` (E.164 through
the `phonenumber` crate, default region from the tenant), `SafeHtml` (an
ammonia policy applied at deserialize), `rules::unicode_name` (letters,
marks, digits in any script), `fold_digits` (Arabic-Indic and Devanagari
digits to ASCII before checking a code), `Country` (ISO 3166-1), `Locale`.
HFv3: `common/validators`, `common/text`, `common/phone`, `sanitize`.

**`migration-safety` · P2 · M · navree-db, navree-cli.** `migrate:lint`
refuses `DROP`, `RENAME`, `SET NOT NULL` and data statements in pending
migrations unless the migration opts in with `Migration::breaking()`;
`make:repair <name>` generates a command that dry-runs by default, prints
counts and changes data only with `--apply`, recorded in `repair_runs`.
HFv3: the additive and schema-only rules, `scripts/*repair*`, release records.

### 5.2 Tenancy and identity

**`organizations` · P0 · XL · navree-workspace (feature `workspace`).**
An optional level above workspaces and an optional level inside them.

```rust
WorkspaceModule::from_config()             // workspace.organizations.enabled = true

// services
organizations.create(&uow, owner_id, NewOrganization { name, slug, country, timezone }).await?;
organizations.attach(&uow, org_id, workspace_id).await?;
org_members.grant(&uow, org_id, user_id, OrgRole::Admin, WorkspaceSelector::All).await?;
partitions.create(&uow, ws.id(), NewPartition { name: "acme.co.uk" }).await?;

// extractors and guards
CurrentOrganization, InOrganization::guard(), CurrentPartition (X-Partition header)
OrgRepo<T>   // workspace_id IN (the org's workspaces this user may see)
```

- Org roles (`org_owner`, `org_admin`, `org_member`, `org_viewer`, app-defined) grant the matching workspace role in all or selected workspaces; workspace resolution accepts membership or an org grant.
- Partitions: a named data slice inside a workspace (`partition_id` column, member grants `All` or `Only([...])`). This is HFv3's Domain level.
- Access requests: `POST /api/organizations/{id}/access-requests`, approve or decline.
- RLS adds `navree.organization`; org dashboards read through `OrgRepo` only.
- Tables `organizations`, `organization_members`, `organization_grants`, `workspace_partitions`, `partition_grants`, `access_requests`; `workspaces.organization_id`.
- Events `OrganizationCreated`, `WorkspaceAttached`, `OrgMemberAdded`, `AccessRequested`.
- HFv3: companies, workspaces, domains, team, access-requests, roles, users, persons, admin, company-reports, blueprints, billing.

**`settings` · P1 · L · new crate `navree-settings` (L6, feature `settings`).**

```rust
#[derive(Settings, Serialize, Deserialize, ToSchema, Validate, Default)]
#[settings(key = "portal", scopes(global, organization, workspace))]
pub struct PortalSettings { pub show_leaderboard: bool, pub welcome_text: String }

m.settings::<PortalSettings>();                                    // ModuleBuilderSettingsExt
let s = settings.resolve::<PortalSettings>(&Scope::workspace(ws)).await?;
s.value.show_leaderboard; s.source("show_leaderboard");           // Source::Organization
settings.update::<PortalSettings>(&uow, Scope::workspace(ws), patch).await?;
settings.inherit::<PortalSettings>(&uow, Scope::workspace(ws), "welcome_text").await?;
```

`Setting<T>` extractor for the current workspace; routes
`GET/PATCH /api/settings/{key}` return `{ data, sources, overridden }`; a user
scope covers preferences; stored in `settings` (scope type, scope id, key,
JSON value), cached by tag, audited, and editable from Helm. HFv3: settings,
companies (defaults), portal-settings, team-portal-settings, notifications,
view-preferences, every `company-default-*` table.

**`time-zones` · P0 · L · new crate `navree-time` (L3, feature `time`, in `full`).**
`chrono-tz` underneath.

```rust
TimeModule::from_config()                          // time.default_zone, time.holidays
async fn h(zone: CurrentZone, range: Valid<Query<DayRange>>) -> Result<WithZone<Report>>
let (from, to) = range.utc_bounds(&zone);          // calendar days counted in the tenant's days
SendWindow::weekdays().between("09:00", "17:00").next_slot(now, &zone)
s.job::<Digest>().daily_at("09:00").per_workspace_local()   // navree-schedule fan-out
```

`Zone` validates IANA names and normalizes legacy aliases (`Asia/Calcutta`
to `Asia/Kolkata`) so PostgreSQL 17 accepts them; resolution is user, then
workspace, then organization, then the app default (`dyn ZoneSource`
multi-binding in navree-contracts, so navree-schedule does not depend on
workspaces); `LocalDay` (`yyyy-MM-dd`) with serde and garde; `Holidays` by
country; SQL helpers for `AT TIME ZONE` with a checked zone; `WithZone<T>`
echoes `timeZone`. HFv3: meta, reports, notifications, announcements,
courses, surveys, policies, gamification, phishing (send windows),
companies and workspaces (country to zone).

**`auth-plus` · P1 · L · navree-auth.**
- Impersonation: `POST /api/auth/impersonate/{user}` (permission `users.impersonate`, reason required), `DELETE /api/auth/impersonate`; the token carries an `act` claim; `Actor` extractor; audit entries record the impersonator; `#[not_while_impersonating]`.
- Step-up: `POST /api/auth/confirm` (password or code) sets `auth_time`; `#[recent_auth(minutes = 10)]`.
- Sessions: `GET /api/auth/sessions` (refresh-token families with device, IP, last use), `DELETE /api/auth/sessions/{id}`.
- Two-factor policy: `auth.two_factor.required_for` (roles) and per-workspace or per-organization `require_two_factor`, refusing with code `two_factor_required`.
- Passwordless: `POST /api/auth/magic-link`, `POST /api/auth/otp`, `POST /api/auth/otp/verify` (signed, single-use, throttled).
- Platform admins: a `platform` audience for super admins outside workspaces, its own throttle, required two-factor; Helm signs in with it.
- Token exchange: `auth.trusted_issuers` accepts a signed JWT from a registered sibling system once (kill switch per issuer).
- Phase 2, passkeys: WebAuthn registration and assertion with ES256 and Ed25519 verified through `navree_crypto::jose` (ring); evaluate `passkey` 0.6 against the crypto policy.
- HFv3: auth (sessions, two-factor, passkeys, magic link, OTP, super-admin), users, admin, hfv2-bridge.

**`sso` · P1 · XL · new crate `navree-sso` (L6, feature `sso`).**
`SsoModule::from_config()`; `SsoConnections` per organization or workspace:
`Saml { metadata_url | metadata_xml }` or `Oidc { issuer, client_id, client_secret }`;
verified email domains (DNS TXT) route sign-in (`POST /api/sso/discover {email}`)
and can forbid passwords for those users; just-in-time provisioning with
attribute and group mapping to roles (`SsoUserProvisioned`). Routes
`GET /api/sso/{connection}/login`, `POST /api/sso/saml/{connection}/acs`,
`GET /api/sso/saml/{connection}/metadata`, `GET /api/sso/oidc/{connection}/callback`.
Tables `sso_connections`, `sso_domains`, `sso_identities`. `SsoFake` signs
test assertions. Crypto: XML signature verification with RSA-SHA256 through
ring (verification only, no `rsa` crate), exclusive canonicalization written
in the crate on `quick-xml`, SHA-1 refused by default; OIDC shares the
hand-rolled client of `[social]`. HFv3: `auth/portal/saml`, `auth/portal/oidc`,
portal, identity-links.

**`entitlements` · P0 · L · navree-billing (feature `entitlements`) with navree-flags.**
Plans declare features and limits:
`billing.plans.business = { features = ["phishing", "lms"], limits = { seats = 50, recipients = 5000 } }`.
A `manual` provider assigns plans by an operator (invoiced contracts, no
checkout). Per-tenant overrides grant or revoke one feature.
`entitlements.has(&scope, "phishing")`, `entitlements.limit(&scope, "recipients")`,
`entitlements.reserve(&uow, &scope, "recipients", n)` (refuses with code
`limit_reached`); `#[requires_entitlement("phishing")]`;
`GET /api/me/entitlements` returns features, limits and usage for navigation;
`EntitlementChanged` event; Helm screens. HFv3: module-entitlements, billing,
`common/limits`, admin companies, vcro, ai-metering tiers.

**`library-scopes` · P3 · M · navree-workspace.** `impl Library for Template`:
rows carry `scope` (`Global`, `Organization`, `Workspace`) with a check
constraint; `LibraryRepo::visible_to(&ws)` unions the three; 
`library.clone_into(&uow, item_id, ws.id())` copies with `cloned_from_id`;
seeders place Global rows; Helm manages Global rows. HFv3: courses, phishing
templates and scenarios, announcement templates, workflow templates, badges,
survey templates, policy library, content palettes, landing templates.

**`demo-workspaces` · P3 · M · navree-workspace, navree-db.** `WorkspaceKind::Demo`;
`m.demo_seeder::<CrmDemo>()`; writes refused with `demo_read_only` or reset
nightly (`workspace:demo-reset`); an `X-Navree-Demo` response header the kits
use for their Live/Demo switch. HFv3: demo mode conventions,
`hfv3-api-mode-conformance`.

### 5.3 Messaging and integrations

**`mail-delivery` · P0 · XL · navree-mail.**
- Transports: `ses` (SES v2 HTTP API, SigV4 signed with hmac and sha2), `mailgun`, `postmark`, `resend`, `graph` (`sendMail`, app-only tokens from `integrations`), `gmail` (send and insert); existing SMTP, log, array, failover.
- Tenant transports: `TransportResolver` resolves domain → workspace → organization → platform → config; `mailer.for_scope(&scope).send(mail)`; transports stored encrypted in `mail_transports`; a `/usable` check before a tenant picks one.
- Delivery log `mail_deliveries` (status, provider id, template, recipient hash) with events from provider webhooks: `MailDelivered`, `MailBounced`, `MailComplained`, `MailOpened`, `MailClicked`; optional open and click tracking with signed redirects.
- Suppressions: hard bounces and complaints suppress automatically; `Suppressions` service, per workspace.
- Hold for approval: `mail.hold(reason)` puts a message in an outbox that an admin releases or discards.
- Rendering: CSS inlining (`css-inline`), plain text generated from HTML, `dir` and `lang` from the locale.
- Sending domains: SPF, DKIM and DMARC checks (`hickory-resolver`), DKIM signing for SMTP.
- Commands `mail:test`, `mail:suppressions`, `mail:domains`.
- HFv3: email-delivery, delivery-transports, email-logs, email-gate, email-rendering, employee-suppressions, notification-delivery, announcements, phishing, tracking-domains.

**`mail-inbound` · P2 · L · navree-mail (feature `mail-inbound`).** `InboundMail`
parsed with `mail-parser` (headers, parts, attachments, raw `.eml` stored);
sources: SES receiving (SNS plus S3), Mailgun routes, Postmark inbound,
raw MIME POST, Gmail push and Graph notifications through `integrations`;
`m.mailbox("reply+{token}@in.example.com", handler)` routes by address with
signed reply tokens; `InboundMailReceived` event. HFv3: inbox, esa, er,
feedback (mail replies), announcements (reply tracking), webhooks.

**`webhook-sources` · P2 · M · navree-webhooks.** Built-in verifiers:
`AwsSns` (signature v1 and v2, certificate host check, subscription
confirmation), `GooglePubSub` (OIDC token, audience and service account),
`MicrosoftGraph` (`validationToken` echo, `clientState`, lifecycle
notifications), `Twilio` (HMAC-SHA1 over URL and parameters), `Slack` (v0
signature and timestamp), `Mailgun`, `Postmark`. HFv3: webhooks, esa, er,
smishing, vishing, integrations (SES notifications).

**`integrations` · P1 · XL · new crate `navree-integrations` (L6, feature `integrations`).**
Workspace- or organization-level connections to third-party APIs:
`impl Provider for MicrosoftGraph` (also `GoogleWorkspace`, `Slack`, a generic
`OAuth2`), admin-consent flows (`GET /api/integrations/{provider}/connect`,
callback), service-account keys (RS256 JWT signing through ring), API keys;
credentials encrypted in `integration_connections`; token refresh under a
lock; scope sets declared per feature (least privilege: directory read
separate from mail send); scheduled `integrations:check` with
`ConnectionUnhealthy`; typed clients on `HttpClient`
(`graph.get("/users").send().await?`); `Connection<MicrosoftGraph>` extractor.
HFv3: integrations, delivery-transports, esa, er, jit-training pollers,
teams-bot, preflight.

**`directory-sync` · P2 · XL · new crate `navree-directory` (L7, feature `directory`).**
A SCIM 2.0 server (`/scim/v2/Users`, `/Groups`, `/ServiceProviderConfig`,
`/Schemas`; filter parser; PATCH; a bearer token per connection, hashed) and
pull connectors on the queue (Microsoft Graph with delta queries, Google
Workspace Directory, LDAP and Active Directory through `ldap3` with rustls,
Okta). A normalized `DirectoryUser` and `DirectoryGroup`; the app maps them
with `impl DirectorySink for RecipientSink`; runs stage, diff and apply, with
a dry run; events `DirectoryUserCreated`, `Updated`, `Deactivated`. HFv3:
recipients, ad-sync, integrations, entities, persons.

**`notify-channels` · P2 · L · navree-notify.** Channels `Slack`,
`MicrosoftTeams` (incoming webhook or proactive bot message through
`integrations`), `Sms` (Twilio, Vonage, AWS SNS; E.164; STOP opt-outs),
`WhatsApp` (Twilio, Meta Cloud API), `WebPush` (VAPID, ES256 through
`navree_crypto::jose`); `Notification::to_slack`, `to_teams`, `to_sms`,
`to_whatsapp`, `to_web_push`; deliveries recorded. HFv3: teams-bot,
smishing sends, notifications, announcements, jit-training.

**`notify-preferences` · P2 · M · navree-notify.** Per-user and per-workspace
preferences by category and channel (`GET/PATCH /api/notifications/preferences`);
digests (`#[notification(digest = "daily")]` batches into one mail); quiet
hours and send windows in the recipient's zone (`time-zones`); per-workspace
template and style overrides (`settings`). HFv3: notifications,
notification-delivery, portal-settings.

### 5.4 Work, data and documents

**`queue-batches` · P0 · L · navree-queue.**

```rust
let batch = Batch::new("recipients.import")
    .add(chunks.map(ImportChunk::from))
    .then(FinishImport { id }).catch(ReportImportFailure { id })
    .dispatch(&uow).await?;
ctx.progress(done, total, "Importing rows").await?;      // inside a job
let op = operations.start(&uow, Operation::new("export").owner(user).workspace(ws)).await?;
```

An `operations` table for user-visible long work (kind, owner, workspace,
status, progress, result, error, `heartbeat_at`); `GET /api/operations/{id}`,
an SSE stream `GET /api/operations/{id}/events` and a private channel
`operations.{id}`; cooperative cancel `DELETE /api/operations/{id}`; a
watchdog schedule marks stale operations and re-dispatches idempotent jobs;
Fleet shows batches. HFv3: async-jobs, `common/sse` job progress, recipient
import, export and bulk operations, content-studio exports and video,
knowledge ingest, reports.

**`workflows` · P2 · XL · new crate `navree-workflows` (L7, feature `workflows`).**
Two halves on one engine:
- Durable workflows in code: `#[workflow] async fn onboarding(ctx: WorkflowCtx, input: Onboarding) -> Result<()>` with `ctx.step("send-welcome", ...)` (result stored, replayed on resume), `ctx.sleep(Duration::days(3))`, `ctx.wait_for::<CourseCompleted>(filter, timeout)`; `workflows.start::<Onboarding>(&uow, input)`; tables `workflow_runs`, `workflow_steps`; versions and cancellation.
- End-user automations: a registry of `#[trigger]` (from events), conditions and `#[action]` types whose config derives `ToSchema` (so the editor gets a JSON Schema); definitions stored per workspace as a versioned graph, validated against the registry; enrollments; a dry run (`POST /api/automations/{id}/simulate`); templates from `library-scopes`.
- HFv3: workflows, courses (LMS automations), phishing (automation rules, scheduler), announcements (reminders), vcro (action runs), feedback pipeline, admin automation.

**`sheets` · P1 · L · new crate `navree-sheets` (L6, feature `sheets`).**
Exports: `#[derive(Exportable)]` with `#[export(header = "...")]` (headers
through `[i18n]`), CSV with a byte-order mark and formula-injection escaping,
XLSX through `rust_xlsxwriter`, streamed `Download::csv(...)` or a queued
export that writes to a disk and returns a signed URL through an operation.
Imports: `Importer<T>`: upload, encoding detection (`encoding_rs`,
`chardetng`), CSV or XLSX (`calamine`), header mapping (automatic plus saved
mappings), garde validation per row, an `import_rows` staging table, a
preview, batched apply with progress (`queue-batches`), duplicate modes
(skip, update, fail) and an error file. HFv3: recipients, reports,
company-reports, courses, surveys, phishing results, policies.

**`documents` · P2 · L · new crate `navree-documents` (L6, feature `documents`).**
PDF with two engines: `typst` (embedded compiler, bundled Noto fonts
including Arabic and Devanagari, `.typ` templates with JSON data) and `chrome`
(HTML to PDF over the DevTools protocol with a small pool);
`documents.pdf("certificates/classic", &data).await?`; DOCX through `docx-rs`;
merge and stamp through `lopdf`; queued rendering. HFv3: courses
(certificates), reports, policies (audit reports), content-studio exports,
`common/pdf`.

**`media` · P2 · XL · new crate `navree-media` (L7, feature `media`).**
Attachments on entities (`impl HasMedia for Course`, collections, a `media`
table); image conversions (`image`, `fast_image_resize`, WebP, `resvg` for
SVG) with EXIF stripped, responsive variants, queued and regenerable
(`media:regenerate`); `Media::from_url` with the SSRF guard; zip archives with
zip-slip and size-bomb guards; text extraction from PDF, DOCX and HTML for
RAG; video and audio through an FFmpeg sidecar (probe, transcode, poster)
behind `media-ffmpeg`. HFv3: uploads, images, brand-logos, knowledge,
content-studio, courses (SCORM packages, thumbnails), portal-news, phishing
assets.

**`db-timescale` · P2 · M · navree-db (feature `timescale`).** Migration
helpers `create_hypertable(table, "occurred_at", chunk)`,
`add_retention_policy`, `add_compression_policy`,
`create_continuous_aggregate`; a `time_bucket` expression for SeaORM
queries; `audit.storage = "hypertable"`; a TimescaleDB service in the test
harness. HFv3: audit-logs, campaign delivery events, tag events, engagement
rollups, AI usage log.

**`reports` · P2 · XL · new crate `navree-reports` (L7, feature `reports`).**
`impl Report for PhishRateReport { type Params; async fn run(...) -> ReportData }`
(aggregates in SQL), rendered as JSON, CSV or XLSX (`sheets`) or PDF
(`documents`); schedules per workspace in local time (`report_schedules`,
`time-zones`), delivered by mail with an attachment or a signed link;
history. Dashboard widgets: `m.widget::<HumanRiskIndex>()`,
`GET /api/dashboard/widgets` (filtered by entitlements and access),
`GET /api/dashboard/widgets/{key}?from=&to=`, layouts per user (`settings`
user scope). HFv3: reports, company-reports, home-dashboard,
dashboard-widgets, er, esa and phishing dashboards, team-portal.

### 5.5 AI

**`ai-credits` · P2 · M · navree-ai.** A credit ledger per workspace
(`ai_credit_grants`, `ai_credit_charges` with idempotent outcome ids), rates
per model and operation, allowances from `entitlements`, a check before each
call (refused with `ai_credits_exhausted`), `GET /api/ai/credits/statement`,
model policy per tier, and an optional push to billing metered usage. HFv3:
ai-metering, admin AI credits, ai-chat, ai-studio, content-studio, knowledge.

**`ai-route-tools` · P2 · L · navree-ai (agents).**
`Agent::route_tools(RouteFilter::prefix("/api/v1/deals"))` turns the app's own
routes into tools, with schemas from the OpenAPI document; each call runs
in-process through the full pipeline as the signed-in user, so access rules
apply; calls are classified `read`, `write` or `destructive` (method plus
`#[api(destructive)]`) and destructive ones need approval (`ToolApproval`);
`#[not_for_agents]` opts a route out; `ai:tools --check` snapshots the tool
set; an assistant endpoint (`POST /api/assistant/chat`, SSE) for the kits.
HFv3: ai-chat (16 tool modules, generated manifests), risk-bot, admin
assistant.

**`ai-media` · P3 · M · navree-ai.** `ai.image(...)` (generate and edit),
`ai.speech(...)` (text to speech), `ai.transcribe(...)`, with providers that
support them, files written to a disk, fakes in `AiFake`. HFv3:
content-studio (images, narration), ai-studio, vishing,
`hfv3-image-generation`.

### 5.6 Operations

**`lighthouse-plus` · P2 · L · navree-lighthouse and an npm package.**
`@navree/lighthouse` for browsers (errors, unhandled rejections, failed
fetches, optional web vitals; Next.js and Nuxt integrations), ingest at
`POST /_lighthouse/browser` (public, origin allowlist, throttled, size-capped,
PII scrubbed), `navree lighthouse:sourcemaps <dir> --release <v>` and
symbolication with the `sourcemap` crate, browser issues next to server
ones; uptime checks (`lighthouse.uptime.checks`) and host metrics (CPU,
memory, disk) on the dashboard. HFv3: client-errors, error journal, Sentry,
the FastAPI observability service.

**`domains` · P2 · L · new crate `navree-domains` (L7, feature `domains`).**
Custom domains per workspace (CNAME to the app), DNS verification by TXT
with polling (`hickory-resolver`), automatic TLS two ways: Caddy on-demand
TLS through an `ask` endpoint `GET /_domains/allow?domain=` (Harbor writes
the Caddy config) or in-process ACME (`instant-acme` and a rustls certificate
resolver); a `domain` workspace resolver (`workspace.resolve = ["domain", "header"]`);
an optional Cloudflare DNS driver; certificate expiry in Lighthouse. HFv3:
tracking-domains, domains, phishing decoy domains and the lander, capture,
`common/certbot`, `common/nginx`, `common/dns`, sending domains.

**`conformity-checks` · P1 · L · navree-testing, navree-cli.**
`navree check`: every route documented with `#[api]` (`api.require_docs`),
every entity classified as workspace or global, workspace entities only
reached through `WorkspaceRepo`, translations complete, migrations linted.
`TestApp::isolation_suite::<DealsModule>()` generates cross-workspace tests
for every route in a module (workspace A reading, updating or deleting B's
records gets 404) from the module's factories. `navree gate` runs fmt,
clippy, tests, `navree check` and the front end's typecheck, tests and build
in one command, with a machine lock and per-step timings. HFv3:
`hfv3-module-conformity`, `hfv3-security-audit`, `hfv3-wiring-completeness`,
`scripts/gate.sh`, its guard specs.

### 5.7 The front-end layer

**`frontend-layer` · P1 · XL · new npm packages under `packages/js/`.**

| Package | What it does |
|---|---|
| `@navree/client` | A typed client on `openapi-fetch` and the schema from `navree openapi:export` plus `openapi-typescript`: base URL, bearer or cookie mode, `X-Workspace`, request ids, `NavreeError { status, code, message, errors }`, a list-query builder. |
| `@navree/next` | The backend-for-frontend from `apps/crm-web` as a library: route handlers with httpOnly token cookies, single-flight refresh, CSRF, the `/admin` proxy and `/broadcasting/auth`; `auth()` for server components; route middleware; `AbilityProvider` and `useAbility` (`@casl/react`); `useChannel` (pusher-js, laravel-echo); `useOperation(id)` (SSE); a `DataTable` on TanStack Table driven by `list-query`; next-intl wiring with right-to-left helpers and error-code keys. |
| `@navree/nuxt` | A Nuxt module with the same features: Nitro server routes for the BFF, `useNavreeAuth`, `useWorkspace`, `useAbility` (`@casl/vue`), `useChannel`, `useOperation`, a `DataTable` on shadcn-vue and TanStack Vue Table, `@nuxtjs/i18n` with right-to-left. |
| `@navree/theme` | The site's dark theme as Tailwind 4 `@theme` CSS: ink surfaces (`--color-ink-0..3`), lines, off-white text steps, the accent `#3dff8f` with green tints, amber for warnings, the display face, the UI sans and Monaspace Neon; shadcn/ui and shadcn-vue presets so both kits look like navree.com. |

`navree openapi:client --out web/src/lib/api` regenerates the schema. CI
builds both packages and runs their tests. HFv3: `apps/web` (fetchApi, query
keys, `UnifiedDataTable`, i18n, CASL), the BFF patterns in `apps/crm-web`.

**`kit-nuxt` · P1 · L · navree-cli templates.**
- `navree new <name> --ui nuxt`: a Nuxt 4 app in `web/` from `apps/crm-nuxt`'s shell (shadcn-nuxt and reka-ui, Tailwind 4, Pinia, TanStack Vue Query, `@casl/vue`, pusher-js, vee-validate with zod), sign-in, the workspace switcher and one example list page; `Ui` gains `Nuxt` in `crates/navree-cli/src/new_app.rs`.
- `navree new <name> --kit saas --ui nuxt`: every screen in the SaaS kit's "What users get" table, in Vue, against the same API; `--ui next` stays the default for `saas`.
- `navree dev` starts the API on :8080 and `nuxt dev` on :3000; `--no-web` skips it.
- Both kits use `@navree/theme` (dark only) and `@navree/nuxt` or `@navree/next`.
- Tests: CI generates both UIs, runs `pnpm install && pnpm build && pnpm test`; nightly a `[browser]` journey (sign up, create a workspace, invite, fake checkout) on each.

Commands the docs and the hero terminal show:

```sh
navree new acme --kit saas --ui next   # API on :8080, Next.js on :3000
navree new acme --kit saas --ui nuxt   # API on :8080, Nuxt on :3000
cd acme && navree dock up && navree migrate:fresh --seed && navree dev
```

The reference CRM today (from `apps/crm-web/README.md` and `apps/crm-nuxt/package.json`):

```sh
cd apps/crm-api && cargo run -p crm-api -- migrate:fresh --seed
cargo run -p crm-api -- serve          # http://127.0.0.1:8080, REST and Pusher WebSocket
cd apps/crm-web  && pnpm install && pnpm dev    # Next.js, http://localhost:3000
cd apps/crm-nuxt && pnpm install && pnpm dev    # Nuxt, http://localhost:3102
```

---

## 6. Skills

### 6.1 HFv3 workflows and where Navree stands

| HFv3 skill (or the user's global skill) | Workflow | Navree today | Needed |
|---|---|---|---|
| `hfv3-agent`, `hfv3-build-module`, `hfv3-module-pipeline`, `nestjs-enterprise-backend` | Build a feature module end to end: contract, schema, migration, repository, service, controller with access, tests, front end | `navree-cli-generators`, `navree-modules-and-di`, `navree-database`, `navree-controllers-and-routing` (pieces, no orchestration) | **`navree-build-module`** |
| `hfv3-module-conformity`, `hfv3-security-audit`, `hfv3-wiring-completeness`, `hfv3-module-verify`, `hfv3-module-surety`, `hfv3-critique` | Audit a module: conventions, tenant isolation, access, contract parity, migrations | `navree-security-checklist` | **`navree-module-conformity`** (runs `navree check`, isolation suites, OpenAPI and client parity) |
| `hfv3-frontend-builder`, `hfv3-api-mode-conformance`, `hfv3-ui-uniformity`, `claude-design-to-app` | Build front-end pages from API routes: schemas, query keys, hooks, data tables, forms, demo mode | none | **`navree-frontend-next`**, **`navree-frontend-nuxt`** |
| `hfv3-local-stack-windows`, `-wsl`, `-macos`, `hfv3-worktree-stack` | Run the stack locally, parallel stacks per branch without port or database clashes | `navree-new-app` (no local services) | **`navree-local-stack`** (Dock, `.env` profiles, ports per worktree, Windows, WSL and macOS notes) |
| `hfv3-locale`, `hfv3-timezone` | Make every screen, mail, job and export work in four languages, right to left, in the tenant's zone | none | **`navree-locale`** (with `[i18n]`, `time-zones`, `error-codes`, kit i18n) |
| `hfv3-infra`, `hfv3-new-instance-bootstrap`, `production-deploy`, `staging-deploy`, `uae-deploy`, `hfv3-release-captain`, `hfv3-reset-rebuild` | Provision instances, deploy, roll back, promote staging to production, keep release records, back up and restore | `navree-deployment` (systemd, nginx) | **`navree-harbor`** (provision, deploy, rollback, several instances and regions, release records, backups); the per-box secrets stay in the product's own local skills |
| `hfv3-gap-analysis`, `prototype-to-saas`, `hfv3-story-mining` | Inventory an existing backend or prototype, map it to modules, list the gaps | none | **`navree-port-an-app`** (routes, entities, queues, crons and abilities into a module plan and a gap table; stack-neutral wording) |
| `hfv3-library-module` | Global, organization and workspace content with clone on use | none | in `navree-workspaces` once `library-scopes` lands |
| `hfv3-ai-chat-coverage` | Expose a module to the in-app assistant | none | in the `navree-ai` skill once `ai-route-tools` lands |
| `hfv3-test-suite-author`, `hfv3-runtime-smoke`, `hfv3-manual-qa`, `hfv3-autopilot-qa` | Test plans, smoke journeys, exploratory QA | `navree-testing` | in `navree-testing` (isolation suites) and the `[browser]` skill |
| `hfv3-enum-convention` | One enum value from database to label | `navree-database` | a section in `navree-build-module` (serde rename rules and labels) |
| `encore-backend` | Services, pub/sub, per-service databases | none | covered by the micro skill that `[micro-client]` and `[micro-handlers]` write |
| `hfv3-customer-docs`, `hfv3-clickup`, `hfv3-session`, deploy skills with credentials | Product operations | n/a | stay with the product |

Packages built in the ecosystem run each bring their own skill (plan §4,
definition of done): `navree-billing`, `navree-search`, `navree-ai`, `navree-mcp`,
`navree-webhooks`, `navree-flags`, `navree-oauth`, `navree-social`,
`navree-i18n`, `navree-lighthouse`, `navree-fleet`, `navree-helm`,
`navree-dock`, `navree-redis`, and so on. Every gap item in section 5 adds
one too.

### 6.2 Skills to add (seven)

1. `navree-build-module`: the end-to-end recipe with a checklist (entity with `workspace_id`, migration set, `WorkspaceRepo`, service with `&Uow` first, controller with access attributes and `#[api]`, resource types, events, jobs, isolation suite, typed client regeneration, the front-end page), using the generators.
2. `navree-module-conformity`: the audit: `navree check`, `TestApp::isolation_suite`, OpenAPI coverage, client parity (regenerate `schema.d.ts`, run `tsc`), deny-by-default review, N+1 hints from Logbook.
3. `navree-frontend-next`: typed client, query keys, TanStack Query hooks, `DataTable` with `list-query`, forms with zod, abilities, real time, operations, i18n and right to left, the dark theme.
4. `navree-frontend-nuxt`: the same for Nuxt 4 with `@navree/nuxt`, shadcn-vue and Pinia.
5. `navree-local-stack`: Dock, environment profiles, parallel stacks per worktree, platform notes for Windows, WSL and macOS.
6. `navree-locale`: translations in every catalogue, error codes, right-to-left layout, time zones, CSV with BOM, fonts in PDFs.
7. `navree-harbor`: instances, deploys, rollbacks, promotion, release records, backups and restores.

Plus `navree-port-an-app` (8th, lower priority).

---

## 7. What stays in the app

These HFv3 parts are product, not framework. Navree provides the blocks:

| HFv3 part | Built from |
|---|---|
| Phishing, smishing and vishing simulations, decoy domains, the lander, bot detection | `mail-delivery`, `notify-channels`, `domains`, `webhook-sources`, navree-http (trusted proxies, client metadata) |
| SCORM runtime and package import | `media` (archives), `storage-s3` (signed asset URLs), a `scorm` module in the app with the SCORM 1.2 and 2004 runtime endpoints |
| Gamification (points, badges, leaderboards, streaks) | navree-db, navree-events, `[redis-core]` (sorted sets), `time-zones` (streak days) |
| Voice agents (LiveKit), Teams bot conversations, Outlook add-in, desktop agents | app code and their SDKs; `integrations` for tokens |
| Policies, surveys, announcements, courses, ESA and ER analyzers, vCRO | feature modules on the core plus `workflows`, `reports`, `documents`, `sheets` |
| The FastAPI observability and dev-insights sidecars | replaced by `[lighthouse-core]`, `[fleet]`, `lighthouse-plus`; or kept as services behind `[micro-*]` |

---

## 8. Recommended order

Ecosystem keys in brackets must be integrated first. Within a wave the items
run in parallel (at most four Rust builders on this machine).

| Wave | Items | Waits for |
|---|---|---|
| **A. Core truths** | `error-codes`, `soft-deletes`, `list-query`, `storage-s3`, `validation-intl` | `[postgres]`, `[test-services]` |
| **B. Tenancy and identity** | `organizations`, `time-zones`, `settings`, `auth-plus`, `entitlements` | A; `[billing-core]` and `[flags]` for `entitlements`; `[jose]` for passkeys |
| **C. Front end** | `frontend-layer`, then `kit-nuxt`; skills 3, 4 and 6 | `error-codes`, `list-query`; `[kit-saas-web]` for the kit |
| **D. Messaging** | `mail-delivery`, `webhook-sources`, `integrations`, `sso` | B; `[webhooks]`, `[social]`, `[jose]` |
| **E. Work and data** | `queue-batches`, `sheets`, `conformity-checks`, `migration-safety`, `db-timescale` | A; `[redis-queue]` optional |
| **F. Reach** | `notify-channels`, `notify-preferences`, `mail-inbound`, `directory-sync` | C, D |
| **G. Documents and insight** | `documents`, `media`, `reports`, `workflows` | E; `[i18n]`, `[browser]` |
| **H. AI and operations** | `ai-credits`, `ai-route-tools`, `ai-media`, `lighthouse-plus`, `domains` | `[ai-agents]`, `[mcp]`, `[lighthouse-core]`, `[harbor-provision]` |
| **I. Polish** | `library-scopes`, `demo-workspaces`, skills 1, 2, 5, 7 and 8 | B and the items each skill describes |

Critical path for a first HFv3 vertical slice (recipients, a mail campaign,
results report, in four languages): `error-codes` → `list-query` →
`organizations` → `time-zones` → `mail-delivery` → `queue-batches` →
`sheets` → `frontend-layer`.

---

## 9. Decisions for the lead

1. **Docs manifest.** The manifest is final for this round, so the new pages in section 5 (and a new `frontend` section) need a manifest change by the lead in the next round.
2. **Error shape.** Adding `code` to `{ statusCode, message, error }` changes Navree's canonical JSON. Recommendation: add it (additive, on by default), and update `basics/errors.mdx`, the OpenAPI error schema and the CRM front ends together.
3. **Organizations inside navree-workspace or a new crate.** Recommendation: inside, behind `workspace.organizations.enabled`, because resolution, RLS and roles must change together.
4. **Time zones as their own crate.** Recommendation: `navree-time` at L3, so navree-schedule can use it without depending on workspaces (zone sources arrive as a multi-binding).
5. **"NextUI".** The user's request says "NextUI". This note reads it as the Next.js front end (shadcn/ui restyled to the dark theme, as on the site). If the user means the HeroUI component library (formerly NextUI), `frontend-layer` would need a third preset; ask before building one.
6. **S3 before launch.** The plan puts object storage after launch; any HFv3-sized product needs it, so `storage-s3` is P0 here.
7. **SAML in Rust under the crypto policy.** XML canonicalization and RSA-SHA256 verification on ring is feasible but needs a spike (two days) before `sso` is scheduled.
