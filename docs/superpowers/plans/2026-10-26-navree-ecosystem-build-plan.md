# Navree ecosystem build plan

> **For agentic workers:** REQUIRED SUB-SKILL: use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to run this plan task by task, and superpowers:test-driven-development for every task. Steps use checkboxes (`- [ ]`).

**Date:** 2026-10-26. **Owner:** the lead. **Ready to start:** M0, then M1.

**Goal.** Make the repository match navree.com. The website and the docs describe the whole first-party ecosystem as available (site spec `docs/superpowers/specs/2026-10-26-navree-site-v2-design.md` §1 and §2). This plan builds every package and tool they describe. It also makes the core statements the site already presents as fact true, such as PostgreSQL support, the performance numbers and `cargo install`. It ends with the checks that must pass before the site goes public.

**The docs are the API spec.** Each planned docs page documents its package as shipped: the module, config keys and env vars, types, macros, routes, commands, tables, events and fake. Build exactly that. Site spec §5 fixes the names, crates and commands. When the code has to differ from a page, change the page in the same change and log the change (see "How to run this plan").

**Inputs.** Read the ones your task names before you start:

- Site spec §5 (catalogue and status), §6 (docs layout) and §8 (AI-native).
- The planned docs pages. Appendix B maps each one to its task.
- `apps/website/src/content/ecosystem.ts`: one `status` per catalogue entry.
- `crates/navree-workspace` and `apps/website/content/docs/workspaces/*`.
- `docs/superpowers/notes/2026-10-26-roadmap-removed-from-site.md`: deferred work, known limits and the eight performance causes.
- `docs/superpowers/notes/framework-open-issues.md`.
- `docs/superpowers/notes/2026-10-26-issues-found-writing-docs.md`, cited below as "issue #n".
- `docs/superpowers/notes/framework-api-notes.md`: how the real APIs look and are configured.
- `bench/results/findings.html` and the "Next" list in `apps/website/src/app/performance/page.tsx`.
- `crates/navree/tests/dependency_graph.rs`: the crate hierarchy.

---

## How to run this plan

1. **Milestones are dependency groups, not strict phases.**
   - A task starts when the tasks named in its "Order" line are integrated.
   - Tasks inside a milestone run in parallel.
   - M0 starts first. M1 starts as soon as M0-1 (CI) and M0-2 (service harness) are in.
2. **One agent per task.** The label is in brackets, for example `[redis-core]`.
   - An agent edits the paths its task names, plus its own docs pages, its agent skill, its entry's `status` in `ecosystem.ts`, its facade feature and re-export lines, and its row in the hierarchy test.
   - Some files are shared: root `Cargo.toml`, `crates/navree/Cargo.toml`, `crates/navree/src/{lib,prelude}.rs`, `crates/navree/tests/dependency_graph.rs`, `crates/navree-macros/src/*` and `apps/website/src/content/ecosystem.ts`. They take small, additive edits. The milestone integrator resolves conflicts in them.
3. **Every milestone ends with an integrator task (`<milestone>-integrate`).**
   - Merge the milestone's work and run CI.
   - Fix the seams between tasks.
   - Update `docs/superpowers/notes/ecosystem-build-notes.md`, one line each for deviations from the docs, decisions and open issues.
   - Then commit. Workers never run git commands that change the index, the working tree or the history. Only the integrator or the lead commits.
4. **Machine limits.** The machine runs Windows 11 with 24 cores and often only a few GB of free memory.
   - At most four agents compile Rust at once.
   - Each one uses `CARGO_TARGET_DIR=target/agents/<label>` and `CARGO_BUILD_JOBS=6`.
   - Docker and WSL don't respond on this machine today. Tests that need services run in CI (M0-1) and are skipped locally with a visible notice (M0-2).
5. **Never message other agents.** Put decisions in the notes file and in your final report.

**Effort:** S ≈ one agent-day; M ≈ 2–4 agent-days; L ≈ 1–2 agent-weeks; XL ≈ 3 or more agent-weeks, split across agents where the task says so.

---

## 1. Where things stand

### 1.1 Built

- **The core.** Every crate in the README table (L0–L5): HTTP, validation, database, auth, access, audit, cache, events, queues, scheduling, mail, notifications, storage, real time, HTTP client, views, OpenAPI, health and metrics, sessions and CSRF, crypto, testing and the CLI.
- **Workspaces.** `navree-workspace` is wired into the facade, is in `full` and has five docs pages and a skill.
- **Agent support.** 21 agent skills (`skills/`), `AGENTS.md` and `CLAUDE.md` in `navree new`, docs for agents (`llms.txt`, `llms-full.txt`, `.md` pages, the page actions, `/skills/index.json`) and the website itself.

The gaps that the site still states as fact are listed in 1.3.

### 1.2 Documented as available, not built

| Item | Docs pages (`apps/website/content/docs/`) | Crate or command | Task |
|---|---|---|---|
| Redis drivers (cache, locks, queue, sessions, rate limits, real-time backplane) | the roadmap; `APP_REDIS__URL` in `tools/harbor`, `tools/dock` | `navree-redis` | M1-1 |
| OpenTelemetry tracing | `microservices/tracing`; pillar "Observable" | `navree-observe` (`observe.tracing`) | M1-2, M4-5 |
| Navigator | `ai/navigator` | `navree mcp` (navree-cli) | M1-4 |
| `navree skills:install` | `ai/agent-skills`, `ai/introduction`, `skills/README.md` | navree-cli | M1-5 |
| Webhooks | `packages/webhooks` | `navree-webhooks` | M2-1 |
| Feature flags | `packages/feature-flags` | `navree-flags` | M2-2 |
| Social sign-in | `packages/social-sign-in` | `navree-social` | M2-3 |
| OAuth server | `packages/oauth-server` | `navree-oauth` | M2-4 |
| Billing | `packages/billing` | `navree-billing` | M2-5 |
| Search | `packages/search` | `navree-search` | M2-6, M5-5 |
| Logbook | `tools/logbook` | `navree-logbook` | M3-1 |
| Lighthouse | `tools/lighthouse` | `navree-lighthouse` | M3-2, M5-6 |
| Fleet | `tools/fleet` | `navree-fleet` | M3-3 |
| Dock | `tools/dock` | `navree dock …` (navree-cli) | M3-4 |
| Harbor | `tools/harbor`, `getting-started/deployment` (Harbor section) | `navree harbor:*`, `deploy*`, `build:image` (navree-cli) | M3-5 |
| Microservices | `microservices/{introduction,transports,request-response,events}`, `patterns/microservices` | `navree-micro` | M4-1, M4-2 |
| gRPC | `microservices/grpc` | `navree-grpc` | M4-3 |
| GraphQL | `microservices/graphql` | `navree-graphql` | M4-4 |
| AI SDK | `ai/ai-sdk`, `ai/introduction` | `navree-ai` | M5-1 |
| Agents and tools | `ai/agents-and-tools` | `navree-ai` | M5-2 |
| Embeddings and RAG | `ai/embeddings-and-rag` | `navree-ai` | M5-3 |
| MCP servers | `ai/mcp-servers` | `navree-mcp` | M5-4 |
| Helm | `tools/helm` | `navree-helm`, `navree make:resource` | M6-1 |
| Localization | `packages/localization` | `navree-i18n` | M6-2 |
| Browser testing | `packages/browser-testing` | `navree-browser`, `navree browser:install` | M6-3 |
| Starter kits | `getting-started/starter-kits` | `navree new --kit saas\|api\|micro` | M6-4 |

### 1.3 Core statements on the site that aren't true yet

| Statement | Where | Fixed by |
|---|---|---|
| Workspace isolation by row (PostgreSQL row-level security through `navree.workspace`), schema and database | pillar 6, workspaces pages, homepage spotlight | M0-3. The two tests in `crates/navree-workspace/tests/postgres.rs` have never run. |
| PostgreSQL is a supported database | database pages | M0-3. Migrations, `migrate:fresh`/`rollback`, `Repo::update`/`delete`, `FOR UPDATE`, cursor pagination and the queue's `SKIP LOCKED` have never run on a real server. |
| "Traces … on by default" | pillar 8 | M1-2 |
| "One **static** binary" / "one static executable" | homepage pillar 10, `ship.tsx` | Decision D1. The binary links against glibc today. |
| `cargo install navree-cli` | homepage hero terminal | M7-2 (crates.io). `site.install.cli` still uses `--git`. |
| `navree new acme --kit saas` and "API on :8080, web app on :3000" | hero terminal | M6-4 |
| `navree new` apps include `LighthouseModule` and `LogbookModule` | `tools/lighthouse`, `tools/logbook` | M3-1, M3-2 (template) |
| Workspaces are on by default | site spec §1, pillar 6 | M0-6. The `navree new` template has no `WorkspaceModule`. |
| `/performance/` numbers and its "Next" list | `/performance/`, the homepage tile | M0-4, then M7-1 |
| `navree new` works outside this checkout | `getting-started/installation` | M0-5 (issue #31), M7-2 |

---

## 2. Global constraints (every task)

- **Docs-first TDD.**
  - Start from the page. Turn each documented behavior and example into a failing test, then implement.
  - Each package keeps `tests/docs_examples.rs`, which reproduces every non-trivial Rust example from its pages, compiles them and runs them where it can. Add a `tests/docs_surface.rs` that names every documented public item (types, methods, config keys, commands and routes), so a rename fails the build.
- **Navree conventions** (see `framework-api-notes.md`):
  - Modules expose `XModule::{for_root, from_config, for_feature::<T>()}`.
  - Each module has one config section; the env vars are `APP_<SECTION>__<KEY>`.
  - Options structs use `#[serde(default)]` with `Default`, and their validation errors name the key.
  - Errors are `AppError` in the JSON shape `{ "statusCode", "message", "error" }`.
  - Writes take `&Uow` first. Events are emitted after commit.
  - Commands are named `<section>:<verb>`. Migration sets are named after the section and ordered with `.after([...])`.
  - Builder extensions are named `ModuleBuilder<X>Ext`.
  - Every route a package mounts declares its access (deny by default).
  - Framework crates implement `Module`, `Injectable` and `Controller` by hand. New attribute and derive macros live in `navree-macros`, expand to `::navree::<crate>::…` paths and get trybuild tests (update `KEYS` and `suggestion()` when `#[module]` or `#[controller]` gains a key).
- **Dependencies.**
  - External versions are declared only in the root `[workspace.dependencies]`. Check the current version with `cargo search` before adding one; Appendix A lists what was current on 2026-10-26.
  - rustls uses the `ring` provider only. The lockfile test forbids aws-lc, OpenSSL, native-tls and CMake, so set each crate's features as Appendix A says.
  - Avoid adding a second major version of a crate already in the tree (`cargo tree -d`).
- **Crypto policy.** No `rsa` crate (RUSTSEC-2023-0071, removed on purpose), and no `p256`/`p384`/`ed25519-dalek` either. RS256 verification and Ed25519/ES256 signing go through `ring`, wrapped in `navree_crypto::jose` (M1-6). HMAC uses the existing `hmac`/`sha2`.
- **Zero cost when unused.** A package that is compiled in but not registered adds nothing to the request path. Instrumentation, tracing and recorders check a flag decided once at boot. M0-4's pipeline benchmark guards this.
- **Fakes.** Each package ships its fake in its own crate (`BillingFake`, `AiFake`, `MicroFake`, `SocialFake`, `FlagsFake`, `LighthouseFake`, `McpTestClient`, `SpanRecorder` and so on). A fake must agree with the real driver: wherever a driver trait exists, the same conformance suite runs against the fake, the memory driver and every real driver. Issue #19 (`QueueFake`) is the lesson.
- **No comparisons.** Code comments, crate docs and docs pages describe Navree in its own terms (site spec §3).
- **Spelling.** New files use American spelling; existing files keep theirs.

---

## 3. Crate hierarchy after this plan

New crates need a row in `HIERARCHY` (`crates/navree/tests/dependency_graph.rs`) before their first build. Ecosystem crates depend on each other (billing on webhooks, search on ai, and so on), so two layers are added and the facade moves up.

| Layer | Crates | What the layer is |
|---|---|---|
| L0–L4 | unchanged | kernel, contracts and config, platform, data and surface, capabilities |
| L4 | **navree-browser** (new): kernel, contracts, config, http, testing | Browser testing; a dev-dependency crate |
| L5 | auth, workspace, **navree-redis** (new): kernel, contracts, config, http, cache, db, queue, session, realtime | Domain foundations and shared drivers |
| L6 (new) | **dashboard, webhooks, flags, oauth, social, i18n, micro, grpc, graphql, ai, mcp** | Ecosystem libraries |
| L7 (new) | **search, billing, lighthouse, logbook, fleet, helm** | Packages built on other packages |
| L8 | `navree` (facade: every runtime crate), `navree-cli` (may now depend on `navree-config`, decision D7) | Entry points |
| L9–L12 | crm-core L9, crm-testkit L10, crm-users/companies/deals/activities L11, crm-api L12 | The reference app (each was +2) |

Allowed navree edges for the new crates (each also gets kernel, contracts and config):

| Crate | Facade feature | In `full` | May depend on |
|---|---|---|---|
| navree-redis | `redis` | no | http, cache, db, queue, session, realtime |
| navree-browser | none (dev-dependency) | n/a | http, testing |
| navree-dashboard | `dashboard` (turned on by the four tools) | via tools | http, crypto, cache, db, access, auth |
| navree-webhooks | `webhooks` | no | http, crypto, events, http-client, db, queue, access, workspace |
| navree-flags | `flags` | no | http, cache, db, audit, access, workspace |
| navree-oauth | `oauth` | no | http, crypto, cache, db, validation, audit, access, auth, workspace |
| navree-social | `social` | no | http, crypto, cache, events, http-client, db, audit, auth |
| navree-i18n | `i18n` | no | http, view, db, mail, validation, notify, workspace |
| navree-micro | `micro`, `micro-nats`, `micro-kafka`, `micro-rabbitmq`, `micro-redis`, `micro-mqtt` | no | http, events, db, validation, access, auth, workspace, redis |
| navree-grpc | `grpc` | no | http, db, validation, access, auth, workspace |
| navree-graphql | `graphql` | no | http, events, db, validation, access, auth, workspace |
| navree-ai | `ai` | no | http, cache, events, db, queue, realtime, validation, access, auth |
| navree-mcp | `mcp` | no | http, db, validation, access, auth |
| navree-search | `search`, `search-tantivy` | no | http, events, db, queue, access, workspace, ai |
| navree-billing | `billing` (turns on `webhooks`) | no | http, cache, events, http-client, db, queue, schedule, access, auth, workspace, webhooks, flags |
| navree-lighthouse | `lighthouse` | **yes** | http, log, events, http-client, db, observe, mail, queue, schedule, access, auth, dashboard, mcp |
| navree-logbook | `logbook` | **yes** | http, log, db, queue, access, auth, dashboard |
| navree-fleet | `fleet` | **yes** | http, events, db, observe, queue, schedule, access, auth, dashboard, redis |
| navree-helm | `helm` | **yes** | http, view, cache, db, storage, queue, validation, audit, access, auth, workspace, dashboard |

Also update, in the same integrator change:

- `RUNTIME_CRATES` in the hierarchy test.
- The `navree-macros` dev-dependency exception, so it covers layers 0–7.
- `layerNames` in `apps/website/src/components/home/in-the-box.tsx`, which names layers 0–6 today. `crates.json` reads layers from the hierarchy test.
- `content/docs/architecture/crate-hierarchy.mdx`, the README crate table and `architecture/facade-and-features.mdx` (one row per new feature).

---

## 4. Definition of done (every package)

- [ ] Every item on its docs pages exists with the documented name, default and behavior: module, config keys and env vars, types, methods, macros, routes, commands, tables, events and fake. `tests/docs_surface.rs` and `tests/docs_examples.rs` pass.
- [ ] It has unit and `TestApp` integration tests and passes the conformance suite of every driver trait it implements, on the fake, memory and every real driver. The service-backed suites are green in CI.
- [ ] `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --all --check` and the hierarchy and lockfile test pass.
- [ ] Docs:
  - remove `status: "planned"` from its pages;
  - make every deviation match the code and log it in the notes file;
  - add its row to `architecture/facade-and-features.mdx`;
  - pass `node scripts/check-docs.ts <pages>`.
- [ ] `ecosystem.ts` status is `"stable"`.
- [ ] Agent skill: `skills/navree-<package>/SKILL.md` exists and is verified against the code. Add its row to the table in `ai/agent-skills.mdx`, and its line to the umbrella `skills/navree/SKILL.md` and to `skills/README.md`.
- [ ] Release-notes line in `prologue/release-notes.mdx`.
- [ ] Performance: the pipeline benchmark (M0-4) shows no regression when the module is compiled in but not registered. Where the page states an overhead with the module registered, it is measured.
- [ ] Security review (M7-4 checklist) for anything that handles credentials, tokens, payments, secrets or remote access: oauth, social, billing, webhooks, the dashboards, Harbor, Navigator and MCP.

---

## 5. Test infrastructure

**The convention (M0-2):**

- A test that needs a service reads `NAVREE_TEST_<SERVICE>_URL`. When the variable is missing, it prints `SKIPPED <test>: set NAVREE_TEST_<SERVICE>_URL` and returns.
- CI sets `NAVREE_TEST_REQUIRE_SERVICES=1`, which turns a missing service into a failure, so CI can never pass by skipping. Today `crates/navree-workspace/tests/postgres.rs` prints "ok" when it skips; M0-2 fixes that.

| Service | Image (pin tag and digest at implementation) | Env var | Used by |
|---|---|---|---|
| PostgreSQL 17 | `postgres:17` | `NAVREE_TEST_POSTGRES_URL` | M0-3 (whole suite), workspace, queue, search, billing, Lighthouse, Fleet, Helm |
| PostgreSQL + pgvector | `pgvector/pgvector:pg17` | `NAVREE_TEST_PGVECTOR_URL` | M5-3, M5-5 |
| Redis 7 | `redis:7-alpine` (Harbor provisions 7) | `NAVREE_TEST_REDIS_URL` | M1-1, M4-2 (Redis Streams), Fleet |
| Meilisearch | `getmeili/meilisearch` | `NAVREE_TEST_MEILISEARCH_URL`, `_KEY` | M2-6 |
| Typesense | `typesense/typesense` | `NAVREE_TEST_TYPESENSE_URL`, `_KEY` | M2-6 |
| Qdrant | `qdrant/qdrant` | `NAVREE_TEST_QDRANT_URL` | M5-3, M5-5 |
| stripe-mock | `stripe/stripe-mock` | `NAVREE_TEST_STRIPE_MOCK_URL` | M2-5 |
| NATS with JetStream | `nats:2` (`-js`) | `NAVREE_TEST_NATS_URL` | M4-2 |
| Kafka 4 (KRaft) | `apache/kafka:4` | `NAVREE_TEST_KAFKA_BROKERS` | M4-2 |
| RabbitMQ 4 | `rabbitmq:4` | `NAVREE_TEST_RABBITMQ_URL` | M4-2 |
| MQTT 5 | `eclipse-mosquitto:2` | `NAVREE_TEST_MQTT_URL` | M4-2 |
| Ubuntu 24.04 with systemd and sshd | built from `crates/navree-cli/tests/harbor/Dockerfile` | `NAVREE_TEST_HARBOR_HOST` | M3-5 (nightly) |
| Docker or Podman itself | runner | `NAVREE_TEST_DOCKER=1` | M3-4, M3-5 `build:image` |
| Chrome for Testing | installed by `navree browser:install` | `NAVREE_BROWSER_CHROME` | M6-3, M6-4, the dashboard UIs |
| Live AI providers | none | `NAVREE_TEST_<PROVIDER>_API_KEY` | M5-1 (nightly only) |
| Live Stripe test mode, Paddle sandbox | none | `NAVREE_TEST_STRIPE_SECRET_KEY`, `NAVREE_TEST_PADDLE_API_KEY` | M2-5 (nightly only) |

**Conformance suites** run on the fake, memory and every real driver:

- **cache:** get, put, forget, remember, flexible, increments, tags, lock ownership and expiry.
- **queue:** insert, reserve, release, fail, retry, unique, delay, visibility timeout, concurrent reservation (each job once), transactional dispatch, unknown job, rollback drops the dispatch.
- **session store**, **rate-limit store**, **real-time backplane**.
- **search engine:** index, filters, sort, pagination, highlights, fresh-import swap, workspace isolation, and accessible-filter equivalence with SQL `Accessible`.
- **vector store**.
- **micro transport:** request-reply, timeout, no responders, at-least-once events, consumer groups, broadcast, per-key order, dead letter after `max_attempts`, header round trip, size limit.
- **AI provider** (recorded fixtures), **billing provider**, **social provider** (fixtures), **webhook verifier** (published test vectors).

**CI jobs (M0-1, extended by later tasks):**

- **lint:** fmt, clippy, the hierarchy and lockfile test.
- **test-windows:** SQLite, no services.
- **test-linux:** a matrix of `sqlite` and `postgres`, with Redis.
- **website:** typecheck, lint, check-docs, static build.
- **crm-web**.
- **ui:** builds the dashboard bundles, checks they are fresh and runs the UI tests.
- **services-search** and **services-brokers:** on changes to those crates, and nightly.
- **nightly:** Harbor end to end, Dock, `build:image`, starter kits, browser journeys, live AI and payment suites, OpenID conformance.

---

## 6. Milestone map

| Milestone | Tasks | Needs | Effort (agent-weeks, rough) | Parallel streams |
|---|---|---|---|---|
| **M0** Core truth | CI, service harness, PostgreSQL, request overhead, core fixes, template, docs-example check | — | ~6 | 4 |
| **M1** Shared foundations | Redis, OpenTelemetry and instrumentation records, dashboard kit, Navigator and versioned docs, `skills:install`, JOSE helpers | M0-1, M0-2; M0-5 for the dashboard kit and Navigator | ~10 | 4 |
| **M2** SaaS | webhooks, flags, social sign-in, OAuth server, billing, search (full text) | M1-6 for social and OAuth | ~14 | 4 |
| **M3** Operate | Logbook, Lighthouse, Fleet, Dock, Harbor | M1-2b, M1-3; M1-1b for Fleet | ~11 | 4 |
| **M4** Connect | micro core, six transports, gRPC, GraphQL, tracing across services | M1-2; M1-1a for Redis Streams | ~15 | 4 |
| **M5** AI | AI SDK, agents, embeddings and RAG, MCP servers, search vectors, Lighthouse MCP tools | M2-4 for MCP's OAuth mode; M2-6 for vectors; M3-2 for Lighthouse tools | ~9 | 4 |
| **M6** Build faster | Helm, localization, browser testing, starter kits | M1-3; M2-3, M2-5, M4-1, M6-1 and M6-3 for the kits | ~10 | 3 |
| **M7** Launch | re-measure, crates.io, status flip, security review, release notes | everything | ~2 | 3 |

About 77 agent-weeks in all. With four concurrent agents, which is the machine's limit, that is roughly 20 calendar weeks plus integration.

- **Running ahead:** Dock (M3-4) has no dependencies and can run any time after M0. Browser testing (M6-3) can move up to M3 to test the dashboard UIs (decision D11).
- **Critical path:** M0-1 → M1-3 (dashboard kit) → M3-2 (Lighthouse) and M6-1 (Helm) → M6-4 (starter kits).

---

## M0: Core truth (prerequisite)

### M0-1 CI pipeline `[ci]` M

Files: `.github/workflows/ci.yml` (new), `.github/workflows/nightly.yml` (new), `scripts/ci/*`.

- [ ] Add the **lint**, **test-windows**, **test-linux** (sqlite and postgres matrix, with service containers for postgres:17 and redis:7, `NAVREE_TEST_REQUIRE_SERVICES=1`), **website** and **crm-web** jobs from §5. Use `Swatinem/rust-cache`.
- [ ] Make `pnpm typecheck` and `pnpm lint` pass. `apps/website/scripts/generate-images.ts` fails today: `sharp` is missing and it has 3 `no-explicit-any` errors. Add `sharp` as a dev dependency and fix the types, or exclude the script from both checks.
- [ ] The postgres job may run with `continue-on-error` until M0-3 makes it required.
- **Done when:** CI is green on the current tree and the website build produces the static export with `llms.txt`.

### M0-2 Service test harness `[test-services]` M

Files: `crates/navree-testing/src/services.rs` (new), `crates/navree-testing/src/app.rs`, `dev/services.compose.yml` (new).

- [ ] Add `navree_testing::services::require(Service::X) -> Option<Url>`, with the skip and require behavior from §5. Use it in `crates/navree-workspace/tests/postgres.rs`.
- [ ] `TestApp::builder().temp_database()` honors `NAVREE_TEST_DATABASE=postgres`:
  - it creates `navree_test_<uuid>` on `NAVREE_TEST_POSTGRES_URL` and drops it on `shutdown()`;
  - a sweeper drops leftovers older than one hour.

  sqlx is an external dependency of navree-testing; the hierarchy is unaffected.
- [ ] Add a `conformance_suite!` macro: a list of async test fns over a driver factory becomes `#[tokio::test]`s, so each driver crate runs the same suite. Document it with an example.
- [ ] Add a compose file listing every §5 service and its env var, for contributors who have Docker (until Dock exists).
- **Done when:** the workspace PostgreSQL tests run in CI and print `SKIPPED` locally.

### M0-3 PostgreSQL verification `[postgres]` L

- [ ] Run the whole workspace suite and the CRM end-to-end tests with `NAVREE_TEST_DATABASE=postgres`, and fix every failure.
- [ ] Add a queue concurrency test: two workers and N jobs, each job reserved exactly once (`SKIP LOCKED`).
- [ ] Run the workspace tests:
  - per-transaction `navree.workspace`;
  - the row-level security policy, including that a raw query without the setting sees nothing;
  - schema isolation: create, migrate, `search_path` per request, drop.
- [ ] Remove the honest-but-now-stale limits from the docs: the warning callout in `database/getting-started.mdx#postgresql`, and the "tests run on SQLite" lines in `digging-deeper/queues.mdx` and elsewhere (grep `SQLite only|not yet run|haven't run`).
- **Done when:** `test-linux (postgres)` is green and required.

### M0-4 Request pipeline overhead `[perf]` L

Inputs: the eight causes in `bench/results/findings.html`, and the open issue that `AppContext::get` scans every provider.

- [ ] **navree-db pool checks.** Add `database.test_before_acquire` (default `false`), plus `idle_timeout_secs` and `max_lifetime_secs`.
- [ ] **navree-db transaction context.** Remove the extra round trip per transaction: send `BEGIN` and the `navree.actor`/`navree.request_id` settings as one simple-query batch. The values are validated as a UUID and a request id, so they are safe to send inline. Workspace row-level security keeps the setting on.
- [ ] **navree-db Uow layer.** Stop spawning a task per request. Create the Uow lazily on first extraction. A drop guard spawns only when a request is cancelled mid-transaction, which keeps the "runs to completion" rule.
- [ ] **navree-http locks.** Hold the maintenance flag in an atomic, refreshed by a file watcher or a one-second check. Shard the per-IP limiter with `DashMap` (M1-1c makes it pluggable).
- [ ] **navree-http per-request work.** Decide once at router build whether a metrics recorder exists, and skip the labels when none does. Create the request scope lazily. Measure the request id cost.
- [ ] **navree-auth `CurrentUser`.** Resolve `AuthService` once. Select only the columns `User` exposes; the password hash and 2FA secrets are read only by the `AuthService` methods that need them.
- [ ] **navree-validation.** Deserialize JSON into the DTO with `serde_path_to_error`. Build a `Value` only on failure, to report every field. This also addresses "serde errors are reported one at a time".
- [ ] **navree-db change events.** Serialize change events only when a `ChangeObserver` or the audit log listens; decide the flag at boot.
- [ ] **navree-kernel.** `AppContext::get`, `try_get` and `get_named` look providers up in a map built at boot.
- [ ] Add `criterion` (0.8.2) benchmarks in `crates/navree/benches/pipeline.rs`: `/json` through the router with `oneshot`, and a Uow begin and commit on SQLite. CI runs them without failing the build.
- [ ] A/B re-run of `bench/run.mjs` on the same rig (Docker Desktop on WSL2, same CPU pinning), against `bench/results/navree.json`.
  - Internal targets: `/json` at least 2× the 0.1 result (≥ 430k req/s) and `/db` at least 1.5× (≥ 70k req/s).
  - p99 must not regress, and peak memory stays at or below 40 MB.
  - The hand-written Axum and sqlx reference app in `bench/frameworks/` shows the rig's ceiling. Reach at least 60% of its throughput on `/json` and `/db`.
- **Tests:** each item has a test that proves the behavior. Examples: a statement log shows one round trip per transaction; the runtime's alive-task count doesn't grow for `/json`; observers off means no serialization.
- **Docs:** rewrite the "Next" list on `/performance/` as items get done. M7-1 publishes the final numbers.

### M0-5 Core fixes the ecosystem builds on L

Split into three agents. Each fix also updates the page that documents today's limit (grep for the sentence).

- **`[core-fixes-kernel]`**
  - [ ] Issue #6: make `dyn Authenticator` a chain (multi-binding; the first `Some(principal)` wins and an `Err` stops). Dashboard cookies, OAuth tokens, PATs and JWTs must coexist; M1-3, M2-4 and M5-4 need this.
  - [ ] Issue #2: add `CommandSpec` boot modes. `check`, `list`, `routes`, `key:generate`, `openapi:export` and the new `introspect` run without connecting: the pool is lazy and `migrate_on_boot` is skipped. `migrate:*` never auto-migrates first. Navigator (M1-4) needs this.
  - [ ] Issue #1: `.env` overrides `config/app.toml`, and real environment variables override `.env` (decision D3). Fix the template comments and `getting-started/configuration.mdx`.
  - [ ] Issue #4: register a default `dyn Clock`.
  - [ ] Issue #3: print boot and command errors with `Display`.
  - [ ] Issue #27: re-export `tracing` from the facade and add it to the template.
  - [ ] Issue #28: inside an app, `navree help` lists the app's commands.
  - [ ] Issue #31: `navree new` writes `navree = { git = "https://github.com/NavreeHQ/navree", tag = "v<version>" }` until M7-2, and a path only with `--navree-path`.
  - [ ] Issue #32: `migrate --seed`.
  - [ ] Issue #26: fix doc comments that name items that don't exist.
- **`[core-fixes-auth]`** (security, before launch)
  - [ ] Require the password for `2fa/enable` and `2fa/recovery-codes`.
  - [ ] Add route throttles for register, forgot-password and reset-password (`auth.throttle.*`).
  - [ ] Add `auth:prune` for refresh tokens and expired reset tokens; schedule it when `ScheduleModule` is present.
  - [ ] Add a soft-delete scope for record checks and `exists` rules (`AccessModule::for_feature::<E>().soft_delete("deleted_at")`), so a soft-deleted record answers 404.
  - [ ] Issue #17: add `access:assign <user> <role>`.
  - [ ] Issue #15: `email/verification` without a `Mailer` must not claim it sent mail.
- **`[core-fixes-jobs]`**
  - [ ] Issue #19: `QueueFake` drops dispatches whose unit of work rolls back and rejects unregistered jobs. Both cases go into the queue conformance suite (M1-1b).
  - [ ] Issue #20: `Mailer::queue` without a queue sends after commit, and queued mail payloads are encrypted when `CryptoModule` is present.
  - [ ] Issues #22 and #34:
    - per-listener `JobOptions` (`m.listen_with::<E, L>(opts)`);
    - stable job names from a `const NAME`;
    - queued listeners are built only on the worker.
  - [ ] Issue #9: log refused requests at INFO, exempt the relay endpoint, and pause workers and the scheduler while the app is down (`queue.respect_maintenance`, default `true`).
  - [ ] Issue #21: a crashed scheduler run doesn't block `without_overlapping` for 24 h (stale-lock timeout).

### M0-6 Template and workspace loose ends `[template]` S

- [ ] The `navree new` app adds `WorkspaceModule::from_config()` (site spec §1: on by default). Update the generated-app test and the AGENTS.md template.
- [ ] Workspace-aware abilities: behind `InWorkspace`, `GET /api/me/abilities` returns the member's workspace rules (or the docs keep the guarded-route pattern). Pick one and make `workspaces/members-and-roles.mdx` match.
- [ ] `security/authorization.mdx`: the `team_id` row says "Always `None`". Behind the `InWorkspace` guard, it is the workspace id.

### M0-7 Docs example check `[docs-check]` S

- [ ] `apps/website/scripts/check-examples.ts <package>` lists every titled ```rust block on the package's pages and fails when `crates/<crate>/tests/docs_examples.rs` doesn't contain it (whitespace-normalized match of its first three code lines).
- [ ] Add a CI job, run per package as it ships.

### M0-integrate `[m0-integrate]`

Merge, make postgres CI required, log decisions D3 and D7 in the notes file, and commit.

---

## M1: Shared foundations

### M1-1 Redis (`navree-redis`, L5, feature `redis`) XL, four agents

External: `redis` 1.8.0 with `default-features = false` and `tokio-comp`, `tokio-rustls-comp`, `connection-manager`, `streams`, `script`. Confirm the lockfile test stays green (rustls with ring). Service: redis:7.

**M1-1a `[redis-core]` M: the crate, the cache store and locks.**

- [ ] `RedisModule::from_config()`, section `redis`:
  - `url` (`APP_REDIS__URL`, default `redis://127.0.0.1:6379`), `prefix`, `connect_timeout_ms`, `response_timeout_ms`;
  - `connections.<name>.url` for separate servers;
  - `rediss://` for TLS.

  Add the `redis` health check and a `Redis` service: a connection manager, plus dedicated connections for blocking commands.
- [ ] navree-cache gets a driver registry. Replace the closed `CacheStoreKind` with a store name resolved through contributed `CacheDriver { name, build }` registrations: memory from `CacheModule`, redis from `RedisModule`. Add `cache.store = "redis"` and `cache.connection`.
- [ ] Redis cache store: values with TTL, tags, `flexible`, atomic increments.
- [ ] Redis lock store: `SET NX PX` and a token-checked Lua release.
- [ ] Add `navree_cache::conformance` and run it on memory and redis.

**M1-1b `[redis-queue]` L: the queue driver.**

- [ ] Add a public `QueueDriver` trait to navree-queue, replacing the private `Store` enum (`crates/navree-queue/src/store/mod.rs`). Its methods: insert, reserve, release, delete, fail, retry, unique, delay, pending count, oldest wait. Database and memory become implementations, and the fake records through the same seam. Add `queue.driver = "redis"`.
- [ ] Redis driver, per queue:
  - a ready list, a delayed sorted set, and a reserved sorted set with a visibility timeout (reliable-queue moves in Lua);
  - unique jobs through `SET NX` with a TTL;
  - failed jobs kept for `queue:failed`/`retry`/`flush`.

  Jobs dispatched inside a unit of work are pushed after commit. Document that Redis delivery is at-least-once after commit.
- [ ] Add `navree_queue::conformance`, including the M0-5 fake-parity cases. Run it on fake, memory, database (SQLite and PostgreSQL) and redis.

**M1-1c `[redis-session]` M: sessions and rate limits.**

- [ ] `session.store = "redis"` through `contracts::SessionStore`.
- [ ] navree-http gets a `RateLimitStore` trait with memory (sharded, M0-4) and redis (one Lua `INCR`+`PEXPIRE`) implementations. Add `http.rate_limit.store = "redis"`. Throttles that use the cache become cross-process once the cache is Redis.

**M1-1d `[redis-realtime]` M: the real-time backplane.**

- [ ] navree-realtime gets a `Backplane` trait (publish with an except-socket, subscribe, presence join/leave/list) and `realtime.backplane = "redis"` (pub/sub plus presence hashes with heartbeats). Several `serve` processes then share broadcasts and presence. The relay stays for processes without sockets.
- [ ] Test: two in-process hubs on one Redis deliver to each other's sockets, and their presence lists agree.

**Docs:**

- Redis sections in `digging-deeper/cache.mdx`, `digging-deeper/queues.mdx`, `basics/sessions-and-csrf.mdx`, `basics/rate-limiting.mdx` and `digging-deeper/broadcasting.mdx`.
- The `redis` section in `getting-started/configuration.mdx`. No page defines `APP_REDIS__URL` today, but Harbor and Dock use it.
- Remove the "on the roadmap" lines and the roadmap's Redis bullet.
- Update the skills for cache, queues and events-and-broadcasting.

### M1-2 OpenTelemetry `[otel]` L

External: `opentelemetry` 0.33.1 (the API, usable from L2 crates), `opentelemetry_sdk` 0.33.1, `opentelemetry-otlp` 0.33.1 (`grpc-tonic` + `tls-ring`, `http-proto` + `reqwest-client`; it uses reqwest 0.13 and tonic 0.14, the same as ours) and `tracing-opentelemetry` 0.34.0.

- [ ] navree-observe: every `observe.tracing.*` key from `microservices/tracing.mdx`, the OTLP export (gRPC and HTTP/protobuf), the batch processor, parent-based ratio sampling, `trust_incoming`, `stdout`, `resource`, `headers`, `timeout_ms`, `max_queue_size`, `trace_url` and the `OTEL_*` fallbacks.
- [ ] Subscriber: navree-log gains a layer registration point (`LoggerLayers` contributions, read when `LoggerModule` installs the subscriber). navree-observe contributes the OpenTelemetry layer. JSON log lines carry `trace_id` and `span_id`.
- [ ] Propagation:
  - navree-http extracts `traceparent`/`tracestate`; navree-http-client injects them.
  - `JobEnvelope` gains `trace_context` (serde default, so old jobs still load), set on dispatch and continued by queue workers, queued listeners and queued mail.
  - The realtime relay forwards it.
- [ ] Spans named and attributed as the table in `tracing.mdx` says: HTTP server, outgoing HTTP, query, commit and rollback, job dispatch and run, cache, mail, in-process event. Messages, gRPC and GraphQL come in M4-5. `db.query.text` is normalized (literals removed); the normalizer is shared with Lighthouse.
- [ ] `navree::observe::testing::SpanRecorder` with the documented assertions.
- [ ] Zero cost:
  - `enabled = false` installs nothing;
  - with no endpoint and no recorder, only the cheap pass-through propagation runs;
  - the M0-4 benchmark gates it.
- [ ] Tests: span names, attributes and parent-child links across a queued job, and in-process OTLP/HTTP and OTLP/gRPC receivers that check the export. No Docker.

### M1-2b Instrumentation records `[instrument]` M

- [ ] Add `navree_contracts::instrument`: typed records delivered to `dyn Instrument` multi-bindings:
  - requests and exceptions/panics;
  - queries and transactions;
  - jobs dispatched, attempted and failed;
  - mail, with rendered bodies when a sink `wants` them;
  - events and their listeners;
  - cache calls and outgoing HTTP calls;
  - notifications and broadcasts;
  - model changes;
  - access checks, with the deciding rule;
  - commands and scheduled runs;
  - log lines.
- [ ] Every emitter checks one flag decided at boot, so apps without Lighthouse or Logbook pay nothing.
- [ ] Emit from navree-http, -db, -queue, -mail, -events, -cache, -http-client, -notify, -realtime, -access, -kernel (commands), -schedule and -log.
- [ ] `TestApp::instrument()` records them. Add one test per emitter.
- Consumers: Logbook (M3-1), Lighthouse (M3-2) and Fleet (M3-3).

### M1-3 Dashboard kit (`navree-dashboard`, L6) `[dashboard]` L

Order: after M0-5 (authenticator chain).

- [ ] Add a `Dashboard::mount(path)` builder, shared by Lighthouse, Fleet, Helm and Logbook, as `tools/*` describes:
  - sign-in through `AuthService`, with two-factor and the login throttle;
  - an encrypted, httpOnly, `SameSite=Strict` session cookie scoped to the mount path;
  - CSRF on writes;
  - Bearer tokens on `<path>/api`;
  - abilities on a registered subject (`Lighthouse`, `Fleet`, `Helm`, `Logbook`; grants like `lighthouse.view`);
  - an optional loopback-only mode (Logbook's `allow_remote`);
  - embedded assets with immutable caching and a strict CSP with no inline scripts;
  - JSON API helpers (the page shape and errors).
- [ ] Add the UI workspace `ui/` (pnpm):
  - `ui/kit` is built with the site's stack: React 19, TypeScript 6, Tailwind 4, and shadcn/ui primitives restyled to the site tokens. It holds tables, charts, filters, the sign-in screen and the layout.
  - Each tool gets one Vite app.
  - Built bundles are committed under `crates/navree-<tool>/ui/`, so `cargo build` never needs Node. CI rebuilds them and fails on any difference.
  - Budget: ≤ 250 KB gzip per tool (decision D8).
- [ ] Tests:
  - Rust: sign-in, 2FA, throttle, CSRF, cookie scope, Bearer, abilities, loopback mode.
  - UI: Playwright in `ui/`, then navree-browser after M6-3.

### M1-4 Navigator (`navree mcp`, navree-cli) `[navigator]` L

External: `rmcp` 3.7.0 (`server`, `transport-io`, `macros`), sqlx (existing; SQLite and PostgreSQL with rustls ring) and `bm25` 2.3.2 (or a small in-house index). Order: after M0-5 (boot modes).

- [ ] `navree mcp` takes `[--path] [--no-build] [--allow-remote-database] [--docs-offline]`. Tools, arguments and answers are exactly as `ai/navigator.mdx` documents them:
  - `app_info`, `list_routes`, `module_graph`, `list_providers`, `list_commands`;
  - `describe_schema`, `query_database`, `read_logs`, `last_error`, `read_config`;
  - `search_docs`, `read_doc`.
- [ ] Introspection:
  - a kernel built-in, `introspect --json`, in graph mode with no construction: modules, imports and exports, providers (scope, visibility, dependencies), commands, facade features and the navree version;
  - `routes --json` in graph boot mode.
- [ ] Navigator builds the app with `cargo build --message-format=json` (debug, the app's own target dir) and caches the answers per source fingerprint. When the build fails, the four app tools return the compiler's first error and the other tools keep working.
- [ ] Database access follows the page's Security section exactly:
  - statements are parsed before sending; `WITH … DELETE` and multiple statements are refused;
  - the transaction is read-only, with a 5 s timeout and a 100-row limit;
  - only local hosts are allowed without the flag;
  - secret-looking columns are masked.
- [ ] Configuration is read with navree-config (decision D7) and `Config::redacted()`. Logs are read from `log.file`, JSON or pretty.
- [ ] Docs: download `https://navree.com/v/<version>/docs.json` once into `~/.navree/docs/<version>/`, search it with BM25, and serve pages with `read_doc`.
- [ ] Tests:
  - a fixture app in `crates/navree-cli/tests/fixtures/`;
  - protocol tests over a child process with rmcp's client;
  - one test per security refusal;
  - search on a fixture bundle.

### M1-4b Versioned docs bundle `[site-versions]` S

- [ ] The website build writes `out/v/<version>/docs.json` (slug, title, description and sections as Markdown) with the converter behind the `.md` pages (`src/lib/agent-markdown.ts`), plus `out/v/latest.json`.
- [ ] The deploy script keeps every earlier `/v/*`.

### M1-5 `navree skills:install` `[skills-install]` S

- [ ] As `ai/agent-skills.mdx` says: copy every Navree skill into `.claude/skills/` and `.agents/skills/`, replacing the `navree` and `navree-*` folders and leaving other skills alone.
- [ ] Skills are embedded at build time from `skills/` by `build.rs`. The published crate carries a copy made by the release script (M7-2), so the skills always match the CLI's version.
- [ ] Mention it in the AGENTS.md template (`crates/navree-cli/templates/app/AGENTS.md.j2`). `skills/README.md` already does.
- [ ] Tests: install into a temp dir, idempotence, foreign skills untouched, and every embedded `SKILL.md` parses with `name` equal to its folder.

### M1-6 JOSE helpers (`navree_crypto::jose`) `[jose]` M

- [ ] On `ring`:
  - JWS sign and verify for HS256, EdDSA (Ed25519) and ES256, and verify-only RS256 (`RsaPublicKeyComponents`);
  - JWK and JWKS parsing and publishing, and key ids;
  - PKCS#8 loading (Apple `.p8`) and Ed25519 key generation.
- [ ] Use RFC 7515 (A.1, A.3) and RFC 8037 (A.4) test vectors, plus provider JWKS fixtures.
- Users: M2-3, M2-4 and M5-4.

### M1-integrate

Hierarchy changes (§3) and the website `layerNames`. Facade features `redis` and `dashboard`. Release-notes lines. Commit.

---

## M2: SaaS

### M2-1 Webhooks (`navree-webhooks`, L6, `webhooks`) `[webhooks]` L

- **Spec:** `packages/webhooks.mdx`.
- **Outgoing:**
  - `WebhooksModule` (`from_config`, `for_root`, `for_feature::<E>()`, `for_type`).
  - An event fans out to one delivery job per endpoint, signed per Standard Webhooks (`webhook-id`, `webhook-timestamp`, `webhook-signature: v1,<base64>`, `whsec_` secrets).
  - The documented retry schedule: eight attempts, from at once to 10 h apart.
  - `disable_after_days`, secret rotation with 24 h of double signing, `max_payload_bytes`, the delivery log.
  - The `Webhooks` service and the `/api/webhooks` routes (`webhooks.manage`), workspace-scoped endpoints, the events and the commands.
  - Endpoint URLs go through navree-http-client's SSRF policy, and `require_https` applies.
- **Incoming:**
  - the `VerifiedWebhook<S>` extractor;
  - the built-in sources (Stripe, GitHub, Paddle, Standard Webhooks) and HMAC sources defined in config;
  - tolerance, `max_body_bytes`, and receipts with `mark_processed(&uow)`.
- **Tables:** `webhook_endpoints`, `webhook_deliveries` and `webhook_receipts` (migration set `webhooks`).
- **External:** `sha1` (RustCrypto 0.11) for SHA-1 sources; otherwise the existing crates.
- **Services:** none. Receivers are in-process (`HttpClient::fake()`), with `FixedClock` for retries.
- **Tests:** the Standard Webhooks spec vectors and the Stripe, GitHub and Paddle signature examples; schedule, disable and rotation overlap; SSRF refusal; workspace scoping.
- **Order:** first in M2, because billing needs it.

### M2-2 Feature flags (`navree-flags`, L6, `flags`) `[flags]` M

- **Spec:** `packages/feature-flags.mdx`.
- **Build:**
  - `#[flag]` (navree-macros: `name`, `default`, `expose`) and `FlagsModule` (`for_root`, `from_config`, `for_feature::<F>()`).
  - The `Flags` service, the `CurrentFlags` extractor, and a `#[requires_flag(F)]` controller key (a route guard).
  - User, workspace and everyone scopes, resolved in the documented order.
  - Percentage rollouts with a stable hash bucket. Rollouts by plan go through a `dyn PlanResolver` that billing registers.
  - The kill switch, the cache (`cache_ttl_secs`), and the `feature_flags` and `feature_flag_overrides` tables.
  - `GET /api/flags` and `/api/admin/flags` (`flags.manage`), the `flags:*` commands, audit entries, `FlagsFake` and `store = "memory"`.
- **Tests:**
  - a resolution matrix;
  - a proptest that raising the percentage never turns a scope off;
  - cache invalidation, the route guard, front-end exposure, the commands.

### M2-3 Social sign-in (`navree-social`, L6, `social`) `[social]` L

- **Spec:** `packages/social-sign-in.mdx`. **Order:** after M1-6.
- **Build:**
  - Providers `google`, `github`, `apple`, `microsoft`, `gitlab`, `slack`, `linkedin` and `discord`, plus `oidc` (discovery) and `custom` (`SocialModule::for_feature::<P>()`).
  - The `Social` service and the `/api/auth/social` routes.
  - A sealed state, encrypted with navree-crypto, holding the PKCE verifier, nonce, intent, `return_to` and expiry. PKCE uses S256.
  - ID tokens are verified through `navree_crypto::jose` against cached JWKS. Apple's ES256 client secret is signed the same way. GitHub's primary verified email is read.
  - Linking rules: `link_by_email`, allowed domains, registration, and `two_factor = require|skip`, which yields a `LoginOutcome` challenge.
  - `social_accounts` (migration set `social`, after `auth`), with provider tokens stored encrypted when `store_tokens` is on, and `Social::token` refresh.
  - Events, audit entries, the login throttle on the callback, and `SocialFake`.
- **External:** none new. OAuth 2.0 and OIDC are hand-rolled (decision D13): `openidconnect` 4.0.1 pulls `rsa` 0.9, `p256` and `p384`.
- **Tests:**
  - an in-process mock identity provider (axum) serving authorize, token, userinfo and JWKS, signed with a fixture key;
  - per-provider response fixtures;
  - state tampering and expiry, PKCE and nonce, the linking matrix, 2FA, domain limits and token refresh.

### M2-4 OAuth server (`navree-oauth`, L6, `oauth`) XL, two agents

- **Spec:** `packages/oauth-server.mdx`; `ai/mcp-servers.mdx` (OAuth section). **Order:** after M0-5 (authenticator chain) and M1-6.
- **`[oauth-core]`:**
  - Authorization code with PKCE (S256 only).
  - Client credentials, acting as the owner and narrowed to the scopes.
  - Device authorization (RFC 8628).
  - Refresh with rotation and reuse detection that revokes the family.
  - Consent and device decisions as API endpoints for the app's front end (`consent_url`, `device_url`).
  - Opaque `nva_` access tokens and `nvp_` PATs stored as SHA-256 digests.
  - The `oauth` strategy registered with navree-auth (`m.strategy`); scopes map to permissions through `Principal::token_abilities`.
  - The documented `WWW-Authenticate` errors, workspace-limited clients and the token cache.
- **`[oauth-oidc]`:**
  - Discovery (RFC 8414 and OpenID Connect), JWKS, and EdDSA ID tokens with key rotation (`previous_signing_keys`).
  - Introspection (RFC 7662) and revocation (RFC 7009).
  - The PAT routes, audit actions, commands and `oauth:prune`.
  - **Dynamic client registration (RFC 7591) behind `oauth.dynamic_registration`**, which the MCP page relies on but the OAuth page doesn't document (decision D12; add it to the page).
- **Tables:** `oauth_clients`, `oauth_authorization_requests`, `oauth_codes`, `oauth_device_codes`, `oauth_access_tokens`, `oauth_refresh_tokens`, `oauth_personal_access_tokens` and `oauth_consents` (migration set `oauth`, after `auth`).
- **Tests:**
  - every grant's happy path and its RFC error codes;
  - PKCE downgrade refusal;
  - exact redirect URI matching, with the loopback port exception;
  - code reuse revokes its tokens; refresh reuse revokes the family;
  - `insufficient_scope`, the PAT lifetime cap, introspection authentication;
  - an independent client test with the `oauth2` 5.0.0 crate as a dev-dependency;
  - nightly: the OpenID Foundation conformance suite (Docker) for the basic and PKCE profiles.

### M2-5 Billing (`navree-billing`, L7, `billing`) XL, three agents

- **Spec:** `packages/billing.mdx`. **Order:** after M2-1; M2-2 provides `PlanResolver` (stub it if flags isn't in yet).
- **`[billing-core]`:**
  - `BillingModule` (`from_config`, `for_root`, `for_feature::<B>()`), and the `Billable` trait for `Workspace`, `User`, `CurrentWorkspace` and `CurrentUser`.
  - Plans from config, the checkout builder, the `Subscription` model and checks, and route guards that require a subscription.
  - Swap, cancel and resume. Trials, including trials without a subscription.
  - Seats that follow the member count, through listeners on `MemberAdded` and `MemberRemoved`.
  - Metered usage, recorded locally and reported by `billing:report-usage` every minute.
  - Cached invoices, the customer portal, and the `/api/billing/*` routes (`billing.manage`).
  - The webhook endpoint, verified by navree-webhooks; events are stored in `billing_events` and processed idempotently on the queue.
  - The documented events, tables (migration set `billing`, after `workspace` and `auth`) and commands, `BillingFake`, `billing.provider = "fake"`, and the `PlanResolver` for flags.
- **`[billing-stripe]`** and **`[billing-paddle]`:** a `BillingProvider` trait with narrow, typed clients on reqwest. The Stripe API version is pinned with `Stripe-Version`. Decision D14: no `async-stripe`, whose last release was 2025-04 and which is a large code-generated crate.
- **Tests:**
  - a provider conformance suite on the fake, Stripe (stripe-mock in CI) and Paddle (recorded fixtures); live Stripe test mode and the Paddle sandbox run nightly;
  - the webhook state machine with duplicate and out-of-order events;
  - seat sync, usage reporting and grace periods with `FixedClock`, the route guard, workspace and user billables;
  - fake parity: the fake emits the same events in the same order as the provider mapping.

### M2-6 Search, full text (`navree-search`, L7, `search` + `search-tantivy`) XL, two agents

- **Spec:** `packages/search.mdx`, except "Vector search", which is M5-5.
- **`[search-core]`:**
  - `#[derive(Searchable)]` with every documented field and model attribute, and `SearchModule` (`for_root(engine)`, `from_config`, `for_feature::<E>()`).
  - Sync on change through the queue (`search.sync` queued/inline/off, `unique_for_secs`). Soft delete. Workspace scoping from `WorkspaceScoped`.
  - Hits and highlights, `across`, the commands (`search:import [--fresh] [--all]`, `flush`, `settings`, `status`), index settings, errors and test helpers.
  - The `memory` engine, and the `postgres` engine: `search_documents` with tsvector and GIN, optional `pg_trgm`.
  - **`accessible` filters.** The postgres engine applies navree-access's SQL filter directly. Other engines index the columns that the rules' conditions name and translate the conditions. A rule that can't be translated fails at boot with a documented error.
- **`[search-engines]`:**
  - `meilisearch` and `typesense` over REST. No SDK: the Meilisearch SDK's default features pull aws-lc through jsonwebtoken, and the `typesense` crate is thin.
  - `tantivy` 0.27.0 behind `search-tantivy`.
- **Services:** postgres, meilisearch, typesense (the `services-search` job).
- **Tests:**
  - the engine conformance suite on every engine;
  - a property test that `accessible` results are a subset of the SQL `Accessible` set for random rule sets, like the access parity test;
  - fresh-import swap with live queries.

### M2-integrate

Facade features, the `ecosystem.ts` statuses for webhooks, flags, social, oauth, billing and search, and skills. Commit.

---

## M3: Operate

### M3-1 Logbook (`navree-logbook`, L7, `logbook` in `full`) `[logbook]` L

- **Spec:** `tools/logbook.mdx`. **Order:** after M1-2b and M1-3.
- **Build:**
  - The 15 watchers, from M1-2b records plus a tracing layer for log lines, and batches per request, job and command.
  - Stores: `file` (its own SQLite file through sqlx, never the app's database) and `memory`.
  - The filter language, tags, duplicate-query detection, and replay: requests through the in-process router; jobs re-dispatched.
  - Pruning (`max_entries`, `keep_hours`) and `logbook:clear`.
  - The UI at `/_logbook`: loopback only unless `allow_remote`, then the `view` ability.
  - On by default in debug builds, off in release builds. `TestApp` sets `logbook.enabled = false` unless a test turns it on; it is a config value, so no crate dependency is needed.
  - The test API from the page.
- **Template:** `navree new` adds `LogbookModule`.
- **Tests:** one per watcher, a property test for the filter parser, replay, pruning, release-build defaults and the loopback guard.

### M3-2 Lighthouse (`navree-lighthouse`, L7, `lighthouse` in `full`) XL, three agents

- **Spec:** `tools/lighthouse.mdx`. **Order:** after M1-2, M1-2b and M1-3.
- **`[lighthouse-core]`:**
  - Recording from M1-2b records and spans: exceptions and panics, requests with sampled traces, slow requests, queries, jobs and outgoing calls, failed jobs, failed scheduled tasks.
  - The `Report` API and the fingerprints from the page's table, using M1-2's SQL normalizer.
  - Issues with statuses, including "resolved in next release" on `lighthouse.release`.
  - Occurrences, and per-minute aggregates with p95 from `hdrhistogram` 7.6.0.
  - The background writer with `buffer`, storage in the app's database or `database_url`, sampling, redaction (`log.redact` plus `lighthouse.redact`), and retention with `lighthouse:prune` scheduled at `prune_at`.
  - `LighthouseFake`, and the export as decided in D5.
- **`[lighthouse-alerts]`:**
  - `new_issue`, `regression`, `spike`, `failed_job` and `long_wait`.
  - Email (`MailModule`), Slack (incoming webhook), signed webhooks (Standard Webhooks format) and `m.alert_channel::<C>()`.
  - Cooldown, `lighthouse.alert` jobs when a queue exists, and `lighthouse:test-alerts`.
- **`[lighthouse-ui]`:** the dashboard at `/_lighthouse` with the `view` and `resolve` abilities.
- **Template:** `navree new` adds `LighthouseModule`.
- **Tests:**
  - every fingerprint case;
  - regression and next-release behavior with `FixedClock` and release labels;
  - alert routing and cooldown, the spike window, exact counts under sampling, redaction and pruning;
  - the dashboard API by ability;
  - overhead with the module registered at ≤ 5% of `/db` throughput.

### M3-3 Fleet (`navree-fleet`, L7, `fleet` in `full`) `[fleet]` L

- **Spec:** `tools/fleet.mdx`. **Order:** after M1-1b (`QueueDriver` pending and wait APIs), M1-2b and M1-3.
- **Build:**
  - The `fleet` command (master) supervising `queue:work` children of the same binary. New worker flags: `--max-memory-mb`, `--max-time-secs` and `--fleet <supervisor>`; memory is read with `sysinfo` 0.39.6.
  - `auto`, `simple` and `off` balancing, with cooldown and max shift. Recycling limits and graceful stop.
  - Heartbeat tables for masters, supervisors and processes, and `fleet.only` for multi-server setups.
  - `fleet:pause`, `continue` and `terminate` through the database, so they work across servers. `fleet:status` and `fleet:forget`.
  - Metric snapshots and the Prometheus gauges on `/metrics`.
  - `LongWaitDetected`, which Lighthouse sends as `long_wait`.
  - Recent jobs from M1-2b job records, failed-job retry and forget, and the dashboard at `/_fleet` (`view`, `operate`).
- **Tests:**
  - the balancer as a pure function, with properties: never above max, never below min per queue;
  - the supervisor lifecycle with a fake spawner;
  - one real end-to-end test that spawns the test binary as workers on Linux and Windows;
  - pause across two masters, long-wait events and the gauges, on the database and Redis drivers.

### M3-4 Dock (navree-cli) `[dock]` M

- **Spec:** `tools/dock.mdx`. **Order:** none; it can start any time after M0.
- **Build:**
  - `dock.toml`: `project`, `runtime`, `write_env`, and services with their keys, `env` templates and `port` overrides.
  - Services: postgres (the pgvector image with `extensions = ["vector"]`), redis, meilisearch, mailpit, minio (bucket created) and nats (JetStream).
  - The Docker or Podman CLI through shell-out, honoring `NAVREE_DOCK_RUNTIME`. Readiness checks and named volumes.
  - `dock up`, `down`, `status`, `logs`, `env` and `shell`, plus `dock:add` and `dock:remove`.
  - The `.env` block between `# dock:start` and `# dock:end`, and real environment variables passed by `navree dev` and every forwarded command.
- **External:** `toml_edit` 0.25.17, so edits keep comments.
- **Tests:** snapshot the exact runtime commands (insta) without Docker. A Docker end-to-end test runs in CI.

### M3-5 Harbor (navree-cli) XL, four agents

- **Spec:** `tools/harbor.mdx`, plus `getting-started/deployment.mdx` (Harbor section).
- **`[harbor-config]`:**
  - `harbor.toml` with every key in the reference, and environments (`harbor.<env>.toml`, `NAVREE_HARBOR_ENV`).
  - `harbor:init`, and the secrets: AES-256-GCM with the `harbor:v1:` format, `harbor.key` or `NAVREE_HARBOR_KEY`, and `set`, `list`, `reveal`, `unset`, `export` and `rotate-key`, with `--generate` and `--stdin`.
- **`[harbor-provision]`:**
  - Idempotent steps over SSH: base system, users, SSH hardening, host key pinning, ufw and fail2ban.
  - PostgreSQL 17 from the PGDG apt repository, with daily backups. Redis 7. Caddy 2 with TLS, HTTP/3, the `/metrics` allowlist and h2c for apps with `GrpcModule`.
  - systemd units for the blue and green web slots, the workers and the scheduler.
  - Every file is rendered from minijinja templates and snapshot-tested.
- **`[harbor-deploy]`:**
  - `deploy` (native or container build for the servers' target): upload, env files, migrate, start the idle slot, health check, Caddy switch, drain, restart workers, prune; with `--server`, `--env`, `--no-migrate`, `--maintenance` and `--release`.
  - `deploy:rollback [--to]`, `deploy:releases`, and `harbor:status`, `logs`, `run`, `ssh` and `backup`.
  - `[frontend]`, several servers with roles and private networking, `harbor:ci github`, and `APP_LIGHTHOUSE__RELEASE` set on every deploy.
- **`[build-image]`:** `navree build:image [--tag] [--push] [--platform] [--dockerfile]` with the image layout from the page.
- **SSH:** `russh` 0.65.0 with `default-features = false, features = ["ring", "flate2"]` (Ed25519 and ECDSA keys; no RSA, which would need the `rsa` crate). Spike first (one day): agent support and host-key pinning. Fall back to the system `ssh` binary if the spike fails (decision D15).
- **Tests:**
  - dry-run snapshots for every command, the secrets format and rotation;
  - the switch state machine against a fake remote;
  - nightly end to end: an Ubuntu 24.04 systemd container provisioned for real, the CRM deployed and then rolled back.

### M3-integrate

Facade features `lighthouse`, `logbook`, `fleet` and `dashboard` in `full`. Measure the `navree new` app's clean build time with the new defaults and record it for decision D10. Template updates. Commit.

---

## M4: Connect

### M4-1 navree-micro core (L6, `micro`) XL, two agents

- **Spec:** `microservices/introduction.mdx`, `request-response.mdx`, `events.mdx` and the generic parts of `transports.mdx`.
- **`[micro-handlers]`:**
  - `MicroModule` (`for_root(Transport::…)`, `from_config`) with the `micro.*` keys.
  - `#[message_handlers]`, `#[message_pattern]` and `#[event_pattern]`, with the options `max_attempts`, `ordered`, `broadcast` and `concurrency`.
  - `m.message_handlers::<T>()`.
  - Messages are presented as request parts, so every existing extractor works: `Payload`, `Valid<Payload>`, `ValidAsync`, `Uow`, `CurrentPrincipal`, `CurrentUser`, `Abilities`, `Accessible`, `Authorized`, `Scoped`, `CurrentWorkspace` (from `x-workspace`) and `MessageContext`.
  - Access attributes and guards, and a unit of work that commits on `Ok`.
  - Codecs json and msgpack (`rmp-serde` 1.3.1) and `max_message_bytes`.
  - Hybrid `serve` with `listen_with_serve`, and the commands `micro:listen`, `patterns`, `send`, `emit`, `setup`, `failed`, `retry` and `flush`.
- **`[micro-client]`:**
  - `ServiceClient`: `send` with `timeout`, `retries`, `bearer`, `without_auth` and `header`; `emit` with `key`; named clients.
  - The envelope header table, deadlines, and the `MicroError` → status table.
  - `m.forward_event::<E>()`, `forward_event_keyed` and `m.receive_event::<E>()`.
  - The outbox (`micro_outbox` in the unit of work, a relay with `outbox_poll_ms`), the inbox (`micro_inbox`, `dedupe` replies), retries, backoff and dead letters (`micro.events.*`).
  - `MicroFake` and `Reply`.
  - The `MicroTransport` trait and the transport conformance suite.
  - The `memory` and `tcp` transports: length-delimited frames, routes, a pool, and TLS or mutual TLS with `tokio-rustls` 0.26.6 on ring.
- **Tests:**
  - conformance on memory and TCP;
  - handler parity with HTTP routes: 403 on access, 400 with field errors, rollback on `Err`;
  - outbox crash recovery: kill the process between commit and publish;
  - inbox dedupe, deadlines and the error table;
  - `MicroFake` refuses unknown patterns the way real transports answer "no responders".

### M4-2 Transports. Each one passes the conformance suite in `services-brokers`.

- **`[micro-nats]` L.** `async-nats` 0.50.0 with `default-features = false`, plus `ring`, `jetstream`, `nkeys` and the server-version features.
  - Core request-reply.
  - JetStream streams from `micro.nats.*`, with one durable consumer per service (the consumer group). The ack wait is raised to the handler's timeout.
  - Broadcast over core subjects, and credentials, token and TLS.
- **`[micro-redis]` M.** Built on navree-redis:
  - streams per pattern, with one consumer group per service;
  - `XAUTOCLAIM` after `claim_after_ms`;
  - reply streams and `max_len`.
- **`[micro-rabbitmq]` L.** `lapin` 4.12.3 with `default-features = false` and `rustls--ring` + `default-runtime`:
  - a topic exchange, and quorum queues per service and pattern;
  - direct reply-to, `prefetch`, and a dead-letter exchange.
- **`[micro-mqtt]` M.** `rumqttc` 0.25.1 with `use-rustls-no-provider` (install the ring provider), MQTT 5:
  - shared subscriptions `$share/<service>/…` as consumer groups;
  - response topics with correlation data for request-reply;
  - QoS 1.
- **`[micro-kafka]` XL.** Start with a spike (decision D4).
  - Pure-Rust options: `rskafka` 0.6.0 has no consumer groups and its last release was 2025-03. `kafka-protocol` 0.18.0 is the protocol layer only. `rdkafka` is excluded (C library, CMake and OpenSSL).
  - Recommended: a client on `kafka-protocol` that uses the KIP-848 consumer-group protocol, where the broker does the assignment. It needs Kafka 4.0 or later; add that to the transports page.
  - Keyed produce for ordering, reply topics, topic setup in `micro:setup`, SASL PLAIN and SCRAM, and TLS on ring.

### M4-3 gRPC (`navree-grpc`, L6, `grpc`) `[grpc]` L

- **Spec:** `microservices/grpc.mdx`.
- **Build:**
  - `GrpcModule` (`for_root`, `from_config`, the `grpc.*` keys), and `#[grpc_service(server = …)]` (navree-macros) on a tonic service impl that takes Navree extractors after `Request<T>`. Register with `m.grpc_service::<T>()` and `m.grpc_descriptors(..)`.
  - The call pipeline: authenticator, access, guards, `Valid`, and one unit of work per call.
  - The `AppError` → gRPC code table, with field errors in `google.rpc.BadRequest` (tonic-types). Streaming, deadlines and the metadata table.
  - A listener shared with `http.addr` (h2c) or a separate `grpc.addr`.
  - gRPC-Web (`tonic-web`), reflection (`tonic-reflection`) and health (`tonic-health`, fed by `dyn HealthCheck`).
  - The Connect protocol: spike `connectrpc` 0.9.1 against a small in-crate unary and streaming adapter; pick one.
  - `GrpcChannels` clients with `forward_auth` and TLS, and `TestChannel`.
- **External:** `tonic` 0.14.6 (`tls-ring`), `tonic-web`, `tonic-reflection`, `tonic-health`, `tonic-prost` and `tonic-types` 0.14.6, `prost` 0.14.4; dev: `tonic-prost-build`.
- **Docs fix:** the install snippet pins `pbjson`/`pbjson-types`/`pbjson-build` 0.8; the current version is 0.9.0.

### M4-4 GraphQL (`navree-graphql`, L6, `graphql`) `[graphql]` L

- **Spec:** `microservices/graphql.mdx`.
- **Build:**
  - `GraphqlModule` with the `graphql.*` keys.
  - `#[resolver]` (navree-macros) over async-graphql, with `#[query]`, `#[mutation]` and `#[subscription]`. Register with `m.resolver::<T>()` and `m.dataloader::<L>()`.
  - The `ContextExt` helper table, field access attributes and `Valid<T>` inputs.
  - Errors in `extensions` as the page's table says.
  - Per-request dataloaders and one unit of work per mutation.
  - Subscriptions over WebSockets at `<path>/ws`, fed by `EventStream`.
  - Depth and complexity limits, batching, GraphiQL and introspection in debug builds, `graphql:schema [--check]`, and the test helpers.
- **External:** `async-graphql` 7.2.1 and `async-graphql-axum` 7.2.1.

### M4-5 Tracing across services `[otel-connect]` M

- [ ] Add spans and propagation for messages (send, process, publish), gRPC in both directions, and GraphQL operations and subscriptions, per `tracing.mdx`.
- [ ] Lighthouse links to traces through `observe.tracing.trace_url`.
- Then `microservices/tracing.mdx` and `patterns/microservices.mdx` lose `status: "planned"`.

### M4-integrate

The micro features stay out of `full`, as the docs say. Run the `services-brokers` job. Commit.

---

## M5: AI

### M5-1 AI SDK (`navree-ai`, L6, `ai`) XL, two agents

- **Spec:** `ai/ai-sdk.mdx`, `ai/introduction.mdx`. **Load the `claude-api` skill before writing the Anthropic provider** (model ids, parameters, streaming events, structured outputs).
- **`[ai-core]`:**
  - `AiModule` (`for_root(AiOptions)`, `from_config`) with the `ai.*` keys.
  - The `Ai` service:
    - `text`;
    - `structured::<T>`, with the JSON schema from utoipa `ToSchema`, native structured output where the provider has it, and `.validated()` with the repair loop;
    - `stream`: `TextStream` is an SSE response, with `chunked()`, `on_finish()` and delivery over real time.
  - `Message`, attachments (image, pdf, `from_disk`) and the request options.
  - Failover and retries on 429, 5xx, timeouts and connection failures, honoring `Retry-After`.
  - Per-user token budgets (in the cache), the metrics, `AiError` → status, and the `log_content` policy.
  - `AiFake`, `FakeReply` and the assertions; `ai.fake = true`.
  - Providers `anthropic` and `openai`, plus `driver = "openai"` for compatible servers.
- **`[ai-providers]`:** `gemini`, `mistral`, `ollama`, and `bedrock` (Converse API). Bedrock's SigV4 signing is written in-crate with `hmac` and `sha2`, which keeps the AWS SDK and aws-lc out.
- **HTTP:** a dedicated reqwest client. navree-http-client's SSRF policy would refuse Ollama on 127.0.0.1. SSE is parsed with `sse-stream` 0.3.0 or a small in-crate parser.
- **Tests:**
  - per-provider conformance on recorded fixtures: text, structured, stream, a tool-call round trip, error mapping and usage;
  - failover order, budgets with `FixedClock`, and fake parity (streams split like real ones; structured validation applies);
  - nightly live smoke tests when `NAVREE_TEST_<PROVIDER>_API_KEY` is set.

### M5-2 Agents and tools `[ai-agents]` L

- **Spec:** `ai/agents-and-tools.mdx`.
- **Build:**
  - The `Agent` trait, `#[agent]`, and `#[tool]` (`read_only`, `needs_approval`) with typed arguments (schema via utoipa).
  - Tool access attributes (`#[can]`, `#[permissions]`, `#[roles]`), checked on every call.
  - `AgentRun` (user, access, ability, accessible, authorize, a unit of work per tool call, the clock) and the run builder.
  - `AgentOptions` and `Memory`. `ai_conversations` and `ai_messages` (migration set `ai`) with summaries and titles. The `Conversations` service.
  - Approvals, `AgentStream`, queued runs, `m.agent::<A>()`, `navree make:agent`, `ai:prune` and `ai.memory.*`.
- **Tests:** scripted with `AiFake` tool calls: access refusals reach the model as tool errors, approvals pause and resume, step limits.

### M5-3 Embeddings and RAG `[ai-rag]` L

- **Spec:** `ai/embeddings-and-rag.mdx`.
- **Build:**
  - `Embeddings`: `embed`, `embed_many`, `batch_size`, and a cache keyed by text.
  - `VectorStore` with three stores:
    - `pgvector`: `pgvector` 0.4.2 with its `sqlx` feature (it accepts sqlx 0.9), an HNSW index and jsonb metadata with GIN;
    - `qdrant` over REST, avoiding qdrant-client's tonic transport without TLS;
    - `memory`.
  - `m.vector_collection(..)`, `VectorRecord`, `VectorMatch` and `Filter`.
  - `Chunker`, with `text-splitter` 0.33.1 (markdown, plus `tiktoken-rs` sizing).
  - `ai.rerank`, scored by the model.
  - The RAG endpoint example, as an integration test.
- **Services:** pgvector and qdrant. **Tests:** the vector-store conformance suite on all three stores.

### M5-4 MCP servers (`navree-mcp`, L6, `mcp`) `[mcp]` L

- **Spec:** `ai/mcp-servers.mdx`. **Order:** bearer mode first; OAuth mode after M2-4.
- **Build:**
  - `McpModule` with the `mcp.*` keys.
  - `#[mcp_server]`, `#[mcp_tool]` (`name`, `title`, `read_only`, `destructive`, `idempotent`, `open_world`), `#[mcp_resource(uri)]` (templates included) and `#[mcp_prompt]`. Register with `m.mcp_server::<S>()`.
  - `McpContext` and `McpNotifier`. Access attributes per tool; a tool without one doesn't compile.
  - Progress for long-running tools.
  - Streamable HTTP at `/mcp` (POST, GET, DELETE; sessions with a TTL; `allowed_origins`; `max_request_bytes`), mounted on the app's router so request ids, CORS and the authenticator chain apply.
  - stdio through `mcp:serve --user <email>`.
  - `auth = "bearer"` accepts any token the chain accepts (JWT, `nvp_`).
  - `auth = "oauth"` serves `/.well-known/oauth-protected-resource` (RFC 9728) and answers 401 with `resource_metadata`. Tokens come from navree-oauth's strategy; there is no crate dependency.
  - Spans and metrics, `McpTestClient`, and `navree make:mcp`.
- **External:** `rmcp` 3.7.0 (`server`, `transport-streamable-http-server`, `transport-streamable-http-server-session`, `transport-io`), behind Navree's own macros. Tool schemas come from utoipa.
- **Tests:** `McpTestClient` suites, and in CI the MCP Inspector CLI against the CRM example server.

### M5-5 Search vectors `[search-vectors]` M

- **Build:** `search.vectors.*`, `#[searchable(embed)]`, `similar`, `similar_to` and `hybrid`, using M5-3's stores and M5-1's embeddings. The `search` migration adds the pgvector `embedding` column and its index.
- **Order:** after M2-6, M5-1 and M5-3.

### M5-6 Agent-facing tools `[ai-tools-glue]` S

- [ ] Lighthouse's seven `lighthouse_*` MCP tools, registered when `McpModule` is present, each checking its ability.
- [ ] Navigator offers the five read tools against the local Lighthouse tables. The Lighthouse page promises this; add a line to `ai/navigator.mdx`.

### M5-integrate

Facade features `ai` and `mcp`. The `ai/*` pages lose `status: "planned"`. Commit.

---

## M6: Build faster

### M6-1 Helm (`navree-helm`, L7, `helm` in `full`) XL, three agents

- **Spec:** `tools/helm.mdx`.
- **`[helm-api]`:**
  - `HelmModule::from_config()` with the `helm.*` keys.
  - The `Resource` trait: `Entity`, `LABEL`, `SINGULAR`, `GROUP`, `ICON`, `TITLE`, `SEARCH`, `SLUG`, `PER_PAGE`, `fields`, `filters`, `actions`, `metrics` and `query`. Register with `m.resource::<R>()`.
  - The field table with its modifiers, the filters, and scoping the query.
  - Actions: ability, confirm, destructive, fields, `queued` (as `helm.action` jobs) and `handle`.
  - Metrics (value, trend, partition), cached per `helm.metrics_cache_secs`.
  - The authorization table through navree-access: type-level and record-level checks, `Accessible` lists, and fields the rules don't permit shown read-only.
  - Saving through your services, custom pages, and the JSON routes under `/admin/api`.
- **`[helm-ui]`:** `ui/helm` on the dashboard kit, driven by the resource metadata API, with the `helm.title`, `logo` and `accent` settings.
- **`[helm-cli]`:** `navree make:resource <module> <Entity> [--path]`, which parses the entity with `syn` (a new navree-cli dependency, already in the workspace) and accepts `Deal` or `deal`.
- **Tests:**
  - the API by ability matrix per route, with field validation messages;
  - queued actions and metric ranges;
  - `make:resource` snapshots;
  - UI journeys with navree-browser.

### M6-2 Localization (`navree-i18n`, L6, `i18n`) `[i18n]` L

- **Spec:** `packages/localization.mdx`.
- **Build:**
  - `I18nModule` with the `i18n.*` keys.
  - Fluent files in `locales/<tag>/*.ftl`, loaded into `fluent-bundle` 0.16.0 concurrent bundles (`intl-memoizer` 0.5.3).
  - `embed_locales!`, `t!` and `args!`.
  - The `Locale` extractor (`scope`, `current`, money, date and number formatting through ICU4X `icu` 2.4.0, decimal and datetime components only) and the `I18n` service.
  - Negotiation from query, user, workspace, cookie and header. User and workspace locales live in i18n's own tables.
  - `PUT /api/me/locale` and `GET /api/i18n/locales`.
  - Translated validation messages through a new translator hook in navree-validation, and `i18n:publish validation`.
  - Mail and notifications in the recipient's locale, through a `t` helper in views.
  - `i18n:check` and `i18n:missing`, `reload`, bidi isolation, and the test helpers.
- **External:** `fluent-bundle` 0.16.0, `fluent-syntax` 0.12.0, `fluent-langneg` 0.14.2, `unic-langid` 0.9.6, `intl-memoizer` 0.5.3 and `icu` 2.4.0.

### M6-3 Browser testing (`navree-browser`, L4) `[browser]` L

- **Spec:** `packages/browser-testing.mdx`.
- **Build:**
  - `Browser`, `Page`, `Element`, `Target` (CSS, `@testid`, labels, text and accessible names) and `within`.
  - Automatic waiting, screenshots and failure files under `NAVREE_BROWSER_ARTIFACTS`, and viewports.
  - `TestServer` on a real port.
  - `FrontEnd` and `WebSession`, which start the front end and route its API calls to the test app.
  - The env vars table.
  - `navree browser:install` in the CLI, which downloads Chrome for Testing into a cache dir with reqwest and `zip` 9.1.2.
- **External:** `chromiumoxide` 0.9.1 with `default-features = false` (no fetcher).
- **Tests:** a fixture HTML app, the CRM journey from the page, and the failure artifacts.
- **Order:** none. It can move up to M3 (decision D11).

### M6-4 Starter kits (navree-cli `--kit`) XL, three agents

- **Spec:** `getting-started/starter-kits.mdx`; hero terminal. **Order:** after M2-3, M2-5, M4-1, M6-1 and M6-3.
- **`[kit-saas-api]`:**
  - API modules `accounts`, `settings`, `projects` and `admin`, with `AuthModule`, `WorkspaceModule` (header resolver), `BillingModule`, `SocialModule`, `HelmModule` for super admins, real time, and mail through Mailpit.
  - Seeds for a demo workspace, a Dock config, and `[package.metadata.navree] kit` and `kit_version`.
- **`[kit-saas-web]`:**
  - `web/`: Next.js and shadcn/ui, built on the existing `--ui next` template.
  - The BFF ported from `apps/crm-web`: httpOnly cookies, single-flight refresh, CSRF, `X-Workspace` forwarding and the `/admin` proxy.
  - Every screen in the page's "What users get" table.
  - `navree dev` starts the API on :8080 and the web app on :3000; `--no-web` already exists.
- **`[kit-api-micro]`:**
  - The `api` kit: the SaaS API without `web/`, with typed-client generation.
  - The `micro` kit: a message handler, an event handler, an in-memory test, NATS configured and a contracts crate.
- **Tests:**
  - CI generates each kit and runs `cargo test`, then `pnpm install && pnpm build && pnpm test`;
  - nightly: a navree-browser journey (sign up → create workspace → invite → fake checkout).

### M6-integrate

Facade features `helm` (in `full`) and `i18n`. Template and kit statuses. Commit.

---

## M7: Launch

- **M7-1 Re-measure `[perf-measure]` M.**
  - Same rig and method: `bench/run.mjs`, three runs plus the repeat.
  - Update `bench/results/navree.json` and `bench/results/extra/navree-control.json`. `scripts/prebuild.ts` regenerates `/performance/`.
  - Rewrite the "Next" list for what remains, and check the homepage tile and the introduction's "under 40 MB, ready in milliseconds".
  - Docker must work on the rig first; it didn't respond on 2026-10-26.
- **M7-2 crates.io `[release]` M.**
  - Reserve every `navree-*` name. Set `publish = true` and the `include` lists: the CLI carries the embedded skills; the tools carry their UI bundles.
  - Publish in layer order.
  - `navree new` writes `navree = "<version>"`. `site.config.ts` switches to `cargo install navree-cli` and `cargo add navree`.
  - Remove every "isn't on crates.io yet" line (grep the docs, README and templates).
- **M7-3 Claims audit and status flip `[launch-audit]` S.** Work through §8.
- **M7-4 Security review `[security]` M.**
  - Use the cso and security-review skills: the auth-sensitive packages, the dashboards, Harbor, Navigator, MCP and OAuth.
  - OSV is clean. Run a secret scan of the whole git history (`gemini.token` is ignored and untracked today; keep it that way).
  - Run through the `navree-security-checklist` skill against the CRM and the SaaS kit.
- **M7-5 Release notes `[release-notes]` S.** `prologue/release-notes.mdx` and the roadmap describe exactly what ships.

---

## 8. Before public launch (checklist)

**Website claims that must hold:**

- [ ] **Hero terminal:**
  - `cargo install navree-cli` works and prints the real version;
  - `navree new acme --kit saas` writes `src/` (auth, workspaces, billing), `web/` and `AGENTS.md`;
  - `navree dev` serves the API on :8080 and the web app on :3000.
- [ ] **Proof strip** (`hero.tsx`): crates, layers, core modules and the "packages and tools" count are regenerated from `crates.json` and `ecosystem.ts` and read correctly. `layerNames` covers every layer (§3).
- [ ] **The ten pillars**, as written in `pillars.tsx`:
  - pillar 6: row, schema and database isolation, verified on PostgreSQL (M0-3);
  - pillar 7: skills, Navigator, docs for models and the AI SDK all exist;
  - pillar 8: traces are on by default and Lighthouse is in `navree new`;
  - pillar 9: NATS, Kafka, RabbitMQ, Redis Streams and gRPC transports exist;
  - pillar 10: decide D1 ("static"), and the memory figure is the re-measured one.
- [ ] **Ship section** (`ship.tsx`): every Harbor fact holds, and "one static executable" follows D1.
- [ ] **`/ecosystem/`:** every mock shows behavior the shipped tool has. Spot-check the Lighthouse feed, the Fleet balancing, the Harbor blue/green switch, the Helm action and the Navigator tools.
- [ ] **`/ai/`:** the Navigator tool names, the start of `llms.txt`, the AGENTS.md excerpt and the AI SDK tab code match the code.
- [ ] **`/architecture/`:** each pattern's code excerpt compiles against the released crates.
- [ ] **The docs:**
  - no page has `status: "planned"`;
  - `check-docs` passes on every page;
  - `check-examples` passes for every package;
  - the Appendix B pages have been reread against the shipped code.

**The flag:**

- [ ] `ecosystem.ts` has no `status: "planned"` left, or the lead sets `showPlannedBadges: true` in `site.config.ts` for what remains.
- [ ] Add `apps/website/scripts/check-status.ts`, run in CI: it fails when a docs page and its catalogue entry disagree, and in a launch build when `showPlannedBadges` is false while anything is planned (decision D11).

**Performance:**

- [ ] M0-4 is done, and M7-1 has re-measured on the same rig.
- [ ] `/performance/` and the homepage tile show the new numbers, and the "Next" list names only work that remains.
- [ ] The prose matches the data: the introduction's "under 40 MB" and "ready in milliseconds", and the pillar copy.

**Everything else:**

- [ ] Crates are published, and the docs, README and templates use crates.io installs (M7-2).
- [ ] `github.com/NavreeHQ/navree` is public. The docs install from it and link to it, and `npx skills add NavreeHQ/navree` reads it.
- [ ] Versioned docs exist for Navigator: `/v/<version>/docs.json` is deployed (M1-4b).
- [ ] The security review (M7-4) is done and its findings are fixed.
- [ ] Licensing: MIT OR Apache-2.0 in every crate. The embedded UI bundles carry their third-party license notices.
- [ ] The release notes and roadmap are accurate (M7-5).

---

## 9. Decisions for the lead

| # | Decision | Recommendation |
|---|---|---|
| D1 | "One **static** binary": the binary links against glibc, and every measurement is on glibc. | Change the homepage pillar and `ship.tsx` to "One binary", as the docs introduction already says. A musl static build with an alternative allocator would need new measurements. |
| D2 | Ecosystem layers: the facade moves to L8 and the CRM shifts by 2 (§3). | Accept. Update the hierarchy test, the homepage `layerNames` and `crate-hierarchy.mdx` together. |
| D3 | `.env` overrides `config/app.toml` (issue #1, a behavior change). | Fix it in M0-5. The template comments already promise this. Simplify the `tools/dock.mdx` paragraph that works around it. |
| D4 | The Kafka client. | Spike, then use `kafka-protocol` with the KIP-848 consumer protocol and require Kafka 4.0 or later. Don't use rdkafka (C library, CMake, OpenSSL). |
| D5 | Two OTLP exporters are documented: `observe.tracing.*` (traces, gRPC default) and `lighthouse.otel.*` (traces and metrics, HTTP default). | One pipeline in navree-observe. `lighthouse.otel.*` exports Lighthouse's metrics, and its trace keys become aliases used only when `observe.tracing` is unset. Edit the Lighthouse page in M3-2. |
| D6 | Vector stores: `search.vectors.*` and `ai.vectors.*`. | One implementation in navree-ai. Search keeps its own settings and collections. No docs change. |
| D7 | `navree-cli` may depend on `navree-config`, so Navigator and Dock read configuration exactly as the app does. | Accept. It is the CLI's only navree edge. |
| D8 | The dashboard UI stack (React, Vite, shadcn/ui tokens) and committing the built bundles. | Accept. `cargo build` stays Node-free, and CI keeps the bundles fresh. |
| D9 | apalis as a queue backend (from the old roadmap). | Don't build it. The Redis driver covers "more queue backends", and an apalis driver would bypass Navree's unit-of-work dispatch and job registry. |
| D10 | Four tools are in `full`, as the docs say. That raises the default build time. | Keep them. Measure in M3-integrate, and move Helm out of `full` (with a docs edit) if a clean build of a new app grows by more than 25%. |
| D11 | Browser testing in M3 rather than M6, and the launch rule for `showPlannedBadges`. | Move browser testing up if the dashboard UI tests need it. Launch with no planned entries; otherwise flip the flag. |
| D12 | MCP OAuth mode relies on `oauth.dynamic_registration`, which the OAuth page doesn't document. | Build RFC 7591 behind that key (default off) and document it. Consider client ID metadata documents (current MCP authorization spec) as a follow-up. |
| D13 | Social sign-in without `openidconnect`/`oauth2`. | Hand-roll on `ring` via `navree_crypto::jose`. Those crates pull `rsa`, `p256`/`p384` and older `rand`/`sha2`. |
| D14 | Stripe and Paddle clients. | Narrow, typed clients on reqwest with the Stripe API version pinned. Skip `async-stripe`: its last release was 2025-04 and its code generation is heavy. |
| D15 | Harbor's SSH. | `russh` with `ring` (no RSA keys) after a one-day spike; the system `ssh` binary as the fallback. |

---

## 10. Docs corrections found while planning (make now, or with the named task)

1. `ai/mcp-servers.mdx` line 319 shows a token `navree_pat_7Kq2...`. The OAuth page defines PATs as `nvp_…`; use `nvp_`.
2. `packages/oauth-server.mdx` lacks `oauth.dynamic_registration`, which `ai/mcp-servers.mdx` uses (D12, M2-4).
3. `microservices/grpc.mdx` installs `pbjson`/`pbjson-types`/`pbjson-build` 0.8; the current version is 0.9.0 (M4-3).
4. `security/authorization.mdx`: `team_id` is not always `None`; inside `InWorkspace` it is the workspace id (M0-6).
5. `ai/agent-skills.mdx`: the skills table lacks `navree-workspaces`, which now exists in `skills/`.
6. `ai/docs-for-agents.mdx`: "steps become a numbered list". The converter writes numbered headings (`### 1. …`).
7. `ai/navigator.mdx`: doesn't mention the Lighthouse read tools that `tools/lighthouse.mdx` promises (M5-6).
8. No page defines the `redis` config section, although `APP_REDIS__URL` appears in `tools/harbor.mdx` and `tools/dock.mdx` (M1-1).
9. `tools/lighthouse.mdx`: the OpenTelemetry export overlaps `observe.tracing` (D5, M3-2).
10. `tools/dock.mdx`: the paragraph that works around `.env` losing to `config/app.toml` changes with D3.
11. `<FileTree>`: MDX strips up to two leading spaces in the template literal, so the first nesting level is lost. Three docs workers worked around it, and the example in `apps/website/AUTHORING.md` itself renders wrong. Fix it in `src/components/mdx/file-tree.tsx` (or the MDX pipeline), or document the extra-indent rule in AUTHORING.md.

---

## 11. After launch

These are on the site's roadmap page as future work, so the launch doesn't need them:

- the 1.0 API freeze and LTS policy;
- object storage disks (S3, GCS, Azure). Use `object_store` 0.14.2 with `aws-base`/`gcp-base`/`azure-base` + `ring` + `reqwest`, never its `aws`, `gcp`, `azure` or `http` features, which turn on aws-lc;
- passkeys. `webauthn-rs` needs OpenSSL; evaluate `passkey` 0.6.0 with `rust-crypto` against the crypto policy;
- revocable access tokens;
- Windows services;
- more starter kits.

**Core backlog (any time, low risk):**

- OpenAPI: query parameters, non-JSON bodies, a `Page<T>` schema, and schemas for the framework's own routes;
- a size limit for the memory cache;
- keeping the socket across `navree dev` restarts;
- `TestAppBuilder::fake_auth` mixed with real tokens;
- issues #7, #8, #10–12, #16, #18, #23–25, #29, #30, #33 and #35;
- the missing `/reset-password` page in crm-web (issue #13) and BFF client addresses by default (issue #14).

---

## Appendix A: External crates (current on crates.io, 2026-10-26)

| Crate | Version | Released | Use | Feature notes |
|---|---|---|---|---|
| redis | 1.8.0 | 2026-10-23 | M1-1, M4-2 | `default-features = false`; `tokio-comp`, `tokio-rustls-comp`, `connection-manager`, `streams`, `script` |
| opentelemetry / _sdk / -otlp / -semantic-conventions | 0.33.1 | 2026-10-09 | M1-2 | otlp: `grpc-tonic` + `tls-ring`, `http-proto` + `reqwest-client`; never `tls-aws-lc` |
| tracing-opentelemetry | 0.34.0 | 2026-09-23 | M1-2 | |
| rmcp | 3.7.0 | 2026-10-22 | M1-4, M5-4 | `server`, `transport-io`, `transport-streamable-http-server(-session)`, `macros`; never `reqwest-native-tls` |
| bm25 | 2.3.2 | 2025-09-07 | M1-4 | or an in-house index |
| hdrhistogram | 7.6.0 | 2026-07-18 | M3-2 | |
| sysinfo | 0.39.6 | 2026-07-09 | M3-3 | |
| toml_edit | 0.25.17 | 2026-10-09 | M3-4, M3-5 | |
| russh | 0.65.0 | 2026-10-25 | M3-5 | `default-features = false`, `ring`, `flate2` (defaults pull aws-lc and rsa) |
| tantivy | 0.27.0 | 2026-10-17 | M2-6 | optional, `search-tantivy` |
| meilisearch-sdk | 0.33.0 | 2026-04-22 | not used | defaults pull aws-lc through jsonwebtoken; use REST |
| typesense | 0.4.0 | 2026-03-23 | not used | thin; use REST |
| qdrant-client | 1.20.0 | 2026-10-22 | not used | tonic without TLS features; use REST |
| pgvector | 0.4.2 | 2026-05-22 | M5-3 | `sqlx` (accepts sqlx `>=0.8, <0.10`) |
| async-stripe | 0.41.0 | 2025-04-08 | not used | D14 |
| oauth2 / openidconnect | 5.0.0 / 4.0.1 | 2025 | dev-only (oauth2) / not used | openidconnect pulls `rsa`, `p256`, `p384` |
| sha1 (RustCrypto) | 0.11 | | M2-1 | HMAC-SHA1 incoming sources only |
| async-nats | 0.50.0 | 2026-07-20 | M4-2 | `default-features = false`, `ring`, `jetstream`, `nkeys` |
| kafka-protocol | 0.18.0 | 2026-08-20 | M4-2 | D4 |
| rskafka | 0.6.0 | 2025-03-20 | spike only | no consumer groups |
| lapin | 4.12.3 | 2026-10-18 | M4-2 | `default-features = false`, `rustls--ring`, `default-runtime` |
| rumqttc | 0.25.1 | 2025-11-21 | M4-2 | `default-features = false`, `use-rustls-no-provider` |
| rmp-serde | 1.3.1 | 2025-12-23 | M4-1 | |
| tokio-rustls | 0.26.6 | 2026-09-28 | M4-1 | ring |
| tonic, tonic-web, -reflection, -health, -prost, -types | 0.14.6 | 2026-05-07 | M4-3 | `tls-ring` |
| prost | 0.14.4 | 2026-06-07 | M4-3 | |
| pbjson / -types / -build | 0.9.0 | 2025-12-09 | docs (M4-3) | |
| connectrpc | 0.9.1 | 2026-09-21 | M4-3 spike | |
| async-graphql, async-graphql-axum | 7.2.1 | 2026-01-20 | M4-4 | |
| sse-stream | 0.3.0 | 2026-09-18 | M5-1 | or an in-crate SSE parser |
| text-splitter | 0.33.1 | 2026-10-25 | M5-3 | `markdown`, `tiktoken-rs` |
| tiktoken-rs | 0.12.1 | 2026-09-24 | M5-3 | through text-splitter |
| aws-sigv4 | 1.6.3 | 2026-10-22 | not used | SigV4 is written in-crate |
| fluent-bundle / fluent-syntax / fluent-langneg | 0.16.0 / 0.12.0 / 0.14.2 | 2025 | M6-2 | |
| unic-langid / intl-memoizer | 0.9.6 / 0.5.3 | 2025 | M6-2 | |
| icu | 2.4.0 | 2026-10-22 | M6-2 | decimal and datetime components only |
| chromiumoxide | 0.9.1 | 2026-02-25 | M6-3 | `default-features = false` |
| zip | 9.1.2 | 2026-10-26 | M6-3 | |
| criterion | 0.8.2 | 2026-02-04 | M0-4 | dev |
| object_store | 0.14.2 | 2026-09-15 | after launch | `aws-base` + `ring` + `reqwest`; never `aws`/`gcp`/`azure`/`http` (they turn on aws-lc) |
| apalis | 1.0.0 | 2026-10-12 | not used | D9 |
| passkey | 0.6.0 | 2026-10-01 | after launch | evaluate `rust-crypto` |
| webauthn-rs | 0.5.5 | 2026-04-30 | not used | OpenSSL |

Run `cargo search <crate>` again when you add one. Versions move.

## Appendix B: Planned docs pages and their tasks

| Page (`apps/website/content/docs/`) | Task |
|---|---|
| `ai/navigator.mdx` | M1-4 (Lighthouse tools M5-6) |
| `ai/introduction.mdx` | M1-4, M1-5, M5-1 |
| `ai/ai-sdk.mdx` | M5-1 |
| `ai/agents-and-tools.mdx` | M5-2 |
| `ai/embeddings-and-rag.mdx` | M5-3 |
| `ai/mcp-servers.mdx` | M5-4 |
| `packages/webhooks.mdx` | M2-1 |
| `packages/feature-flags.mdx` | M2-2 |
| `packages/social-sign-in.mdx` | M2-3 |
| `packages/oauth-server.mdx` | M2-4 |
| `packages/billing.mdx` | M2-5 |
| `packages/search.mdx` | M2-6, M5-5 |
| `packages/localization.mdx` | M6-2 |
| `packages/browser-testing.mdx` | M6-3 |
| `tools/logbook.mdx` | M3-1 |
| `tools/lighthouse.mdx` | M3-2 |
| `tools/fleet.mdx` | M3-3 |
| `tools/dock.mdx` | M3-4 |
| `tools/harbor.mdx` | M3-5 |
| `tools/helm.mdx` | M6-1 |
| `microservices/introduction.mdx`, `request-response.mdx`, `events.mdx`, `transports.mdx` | M4-1, M4-2 |
| `microservices/grpc.mdx` | M4-3 |
| `microservices/graphql.mdx` | M4-4 |
| `microservices/tracing.mdx` | M1-2, M4-5 |
| `patterns/microservices.mdx` | M4-1, M4-5 |
| `getting-started/starter-kits.mdx` | M6-4 |

These pages are not marked planned but describe planned work in sections: `getting-started/installation` (Dock, `--kit`), `getting-started/deployment` (Harbor), `digging-deeper/cli` (ecosystem commands), `digging-deeper/health-and-metrics` (Lighthouse), `digging-deeper/queues` (Fleet), `security/authentication` (social sign-in and OAuth), `prologue/introduction` and `prologue/release-notes`. Recheck them in M7-3.
