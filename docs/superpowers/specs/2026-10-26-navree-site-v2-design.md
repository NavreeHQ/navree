# Navree site v2: positioning, ecosystem, architectures and AI-native docs

Date: 2026-10-26. This replaces the positioning in
`2026-10-26-navree-website-design.md` §1 and §3 (the stack, the docs layout and
the deploy plan there still hold). Every worker on this round reads this file
first. If you need a decision that isn't here, make the smallest reasonable
call, write it in your final report, and keep going. Never message other
agents.

## 1. What the user asked for

- **No comparisons, anywhere.** The site, the docs, the README and the crate
  docs describe what Navree is and does, in its own words. No "like Laravel",
  "NestJS-style", "Rails for Rust", "X-class", "parity with", "the way you know
  from", and no tables or charts ranking Navree against other frameworks.
- **Describe Navree the way great frameworks describe themselves.**
  laravel.com, nestjs.com and loco.rs never compare: each states what it is,
  what it is best at and what it includes. Navree's homepage does the same,
  top to bottom: modular structure, dependency injection, API-first,
  AI-native, the whole ecosystem.
- **Architectures.** Show how to build different shapes of system with Navree:
  a modular monolith, microservices, multi-tenant SaaS (workspace level),
  event-driven, API-first, backend-for-frontend. **Diagrams are built
  carefully** (rules in §7).
- **Workspaces are part of the framework by default.** Multi-tenancy at the
  workspace level is a core module, on in the default `navree` features. It
  is built for real in this round (`crates/navree-workspace`).
- **World-class look**, same brand: neon green on black. Use shadcn/ui and
  Magic UI components.
- **AI-native.** Agent skills for every common task, docs agents can read
  (`llms.txt`, Markdown for every page), and AI features in the framework.
- **A first-party ecosystem.** Everything other ecosystems sell separately
  (monitoring, deployment, admin panels, queue dashboards, search, billing,
  OAuth and so on) is part of Navree, open source and free.
- **On the website, assume everything is built.** The site and docs describe
  the full ecosystem as available. The gap between the site and the code
  becomes the build plan (`docs/superpowers/plans/2026-10-26-navree-ecosystem-build-plan.md`).

## 2. Builder's calls

- **Benchmarks become Performance.** `/benchmarks/` is replaced by
  `/performance/`: Navree's own measurements, the workload, the method and
  how to reproduce them. No other framework appears on the site. The
  cross-framework harness and results stay in `bench/` in the repository
  (internal engineering data, not site content). `/benchmarks/` becomes a
  static redirect page to `/performance/`.
- **Status flag for unbuilt things.** Pages and catalogue entries for things
  not yet built carry `status: "planned"` (docs front-matter, and the
  `status` field in `src/content/ecosystem.ts`). `site.config.ts` has
  `showPlannedBadges: false`. While it is false the site renders everything
  as available, as the user asked. Flipping it to true shows an
  "In development" badge on every planned page and card. The user decides
  before launch.
- **Names.** Tools with a UI or a CLI of their own get product names with a
  nautical theme (the logo is a hull). Libraries get plain names. Section 5
  is the canonical list. Use these names, crates, module types and commands
  exactly, on every page, so the homepage, the ecosystem page, the docs and
  the skills agree.
- **Agents don't commit.** Workers never run git commands that change the
  index, the working tree or history (`add`, `commit`, `stash`, `checkout`,
  `reset`, `restore`). The lead commits at the end.

## 3. Voice

- Confident, concrete, short sentences. Say what Navree does and what you
  get. Prefer verbs and numbers to adjectives.
- Never name another web framework to compare. Naming the libraries Navree is
  built on is good: Tokio, Axum, Hyper, Tower, SeaORM, sqlx, garde, tonic,
  rustls, Argon2. Protocols and standards are fine: "speaks the Pusher
  protocol, so pusher-js and any Pusher client connects", "OpenAPI 3.1",
  "OAuth 2.1", "OpenTelemetry", "MCP".
- Describe the JSON error shape (`{ "statusCode", "message", "error" }`)
  as Navree's, without attributing it.
- British or American spelling: keep what the file already uses; new files
  use American spelling ("organize", "color").
- No exclamation marks, no "blazing", "revolutionary", "next-gen",
  "seamless", "robust", "leverage", "unlock", "supercharge".

### Canonical copy (use verbatim or as the basis)

- **Category line / H1:** "The complete backend framework for Rust."
- **Short tagline** (`site.tagline`): "Modular, API-first and AI-native. Everything your backend needs, checked by the compiler."
- **Meta description** (`site.description`): "Navree is a batteries-included Rust framework for building secure, scalable server-side applications. Modules and dependency injection give every app the same clear structure, and auth, workspaces, an ORM, queues, search, real time, monitoring, deployment and AI come in the box."
- **Hero sub-copy:** "Navree gives every backend the same clear structure: modules wired by dependency injection, API-first by default and AI-native from the first command. Auth, workspaces, an ORM, queues, search, real time, monitoring and deployment are already in the box, so you build the product, not the plumbing."
- **Philosophy** (homepage "What is Navree" and docs Introduction): "Navree is a framework for building efficient, secure and scalable server-side applications in Rust. It gives you an application architecture out of the box: features live in modules, modules declare what they import and export, and a dependency-injection container wires them together when the app boots, refusing to start if anything is missing. Around that structure sits a complete standard library for backends, from routing and validation to workspaces, queues, monitoring and AI, designed together, documented together and released together. The result is code that stays easy to test, easy to change and easy to hand to a new teammate, or to an AI agent."
- **Pillars** (homepage grid; the order is a suggestion):
  1. **Modular by design.** Every feature is a module that declares what it imports, provides and exports. Apps stay navigable at five modules or five hundred.
  2. **Dependency injection.** A type-keyed container wires services at boot and refuses to start with a missing or private dependency, naming the module and the type.
  3. **API-first.** Versioned routes, validated input, one JSON error shape and an OpenAPI 3.1 document generated from your code.
  4. **Type-safe end to end.** Requests, queries, jobs, events and configuration are typed. If it compiles, it's wired.
  5. **Secure by default.** Routes deny until you allow them. Rotating refresh tokens, two-factor, Argon2id, encrypted cookies, CSRF, rate limits, security headers, an audit log and an SSRF guard ship on.
  6. **Workspaces built in.** Multi-tenancy at the workspace level: members, roles, invitations, and isolation by row, schema or database.
  7. **AI-native.** Agent skills, an MCP server for your coding agent, docs written for models as well as people, and an AI SDK for the product itself.
  8. **Observable.** Structured logs, traces, metrics, health checks and Lighthouse monitoring, self-hosted and on by default.
  9. **Scales out.** Start as one binary; split into services over NATS, Kafka, RabbitMQ, Redis Streams or gRPC without rewriting your modules.
  10. **Fast and frugal.** One static binary, about 30 MB of memory under load and ready in milliseconds.

## 4. Site map

| Route | What | Owner (§9) |
|---|---|---|
| `/` | Homepage | home |
| `/docs/...` | Documentation (manifest in `content/docs/manifest.ts`) | writers |
| `/ecosystem/` | Every first-party package and tool, grouped | ecosystem |
| `/architecture/` | Architectures you can build, with diagrams | architecture |
| `/ai/` | Navree for AI: skills, Navigator, llms.txt, AI SDK, MCP | ai |
| `/performance/` | Navree's own measurements, method, reproduce | performance |
| `/benchmarks/` | Redirect page to `/performance/` | performance |
| `/llms.txt`, `/llms-full.txt` | The docs for models | ai |
| `/docs/<section>/<page>.md` | Every docs page as Markdown | ai |
| `/skills/<name>/SKILL.md`, `/skills/index.json` | The agent skills, served | ai |
| `/styleguide/` | Docs components (existing) | foundation |

Main navigation: Docs, Ecosystem, Architecture, AI, Performance; search;
GitHub; a "Get started" button. On phones: a menu sheet.

## 5. The ecosystem (canonical catalogue)

Everything below is first-party, open source (MIT OR Apache-2.0), free and
self-hosted. "Status" is the truth in the repository today; the site shows
everything as available (§2). Data lives in `apps/website/src/content/ecosystem.ts`.

### Core (built)
All the existing crates (README crate table): HTTP and routing, validation,
database (SeaORM), auth (JWT, rotating refresh tokens, two-factor, account
flows), access control (roles, permissions, abilities, SQL filters), audit
log, cache, events, queues, scheduling, mail, notifications, storage, real
time (Pusher-protocol WebSocket server), HTTP client, views, OpenAPI, health
and metrics, sessions and CSRF, crypto, testing, the CLI.

### Workspaces (core, built in this round)
- Crate `navree-workspace`, facade feature `workspace` (in `full`, so on by default), module `WorkspaceModule`.
- Request extractor `CurrentWorkspace`; services `Workspaces` (create, rename, list a user's workspaces, transfer ownership, delete), `Members` (add, change role, remove), `Invitations` (invite by email, accept by token, revoke, expire).
- Roles: `Owner`, `Admin`, `Member`, `Viewer`, plus app-defined roles; workspace-scoped permissions that feed `navree-access`.
- Resolution: from a header (`X-Workspace`), a subdomain (`acme.app.com`), a path prefix (`/w/acme/...`) or a token claim; configurable order.
- Isolation: `Row` (a `workspace_id` column, filtered by the repository layer and enforced by PostgreSQL row-level security through `set_config('navree.workspace', ...)` per transaction), `Schema` (one PostgreSQL schema per workspace) and `Database` (one database per workspace).
- Docs: `/docs/workspaces/*`. The crate's final API wins over this sketch; its docs are written from its code.

### Tools (planned)
| Slug | Name | One-liner | Crate / command | Key API |
|---|---|---|---|---|
| `lighthouse` | Lighthouse | Application monitoring: exceptions, slow requests and queries, failed jobs, alerts | `navree-lighthouse`, dashboard at `/_lighthouse` | `LighthouseModule::from_config()`; issues grouped by fingerprint; alerts to email, Slack, webhooks; MCP tools so agents read and resolve issues; OpenTelemetry export; `navree lighthouse:prune` |
| `logbook` | Logbook | A local debugging timeline of every request, query, job, mail, event, cache call and log line | `navree-logbook`, UI at `/_logbook` (development only) | `LogbookModule::from_config()`; filters by tag and user; request replay |
| `fleet` | Fleet | Queue dashboard and worker supervisor | `navree-fleet`, UI at `/_fleet`, `navree fleet` | supervised workers per queue with auto-balancing; throughput, wait and runtime metrics; retry or forget failed jobs; pause and continue; long-wait alerts |
| `helm` | Helm | Admin panel generated from your entities | `navree-helm`, served at `/admin`, `navree make:resource` | `HelmModule`; `impl Resource for DealResource` with fields, filters, actions, metrics; respects your access policies |
| `harbor` | Harbor | Servers and zero-downtime deployments | `navree harbor:provision`, `navree deploy`, `navree deploy:rollback`; config `harbor.toml` | provisions Ubuntu servers over SSH (Caddy, PostgreSQL, Redis, TLS, firewall, systemd units for web, workers and scheduler); atomic releases with health checks; rollbacks; encrypted environment secrets; multiple servers and roles; container images with `navree build:image`; push-to-deploy from GitHub Actions |
| `dock` | Dock | Local services for development | `navree dock up`, `navree dock:add <service>`; config `dock.toml` | starts PostgreSQL, Redis, Meilisearch, Mailpit, MinIO and NATS in containers (Docker or Podman) |
| `navigator` | Navigator | The MCP server for your coding agent | `navree mcp` (stdio) | tools: list routes, the module graph and providers, entity schemas, read-only SQL, recent logs and the last error, configuration, version-matched docs search; ships guidelines (`AGENTS.md`) and installs skills (`navree skills:install`) |

### Libraries (planned)
| Slug | Name | One-liner | Crate | Key API |
|---|---|---|---|---|
| `search` | Search | Full-text and vector search over your entities | `navree-search` | `SearchModule::for_root(engine)`; `#[derive(Searchable)]`; `Search` service; engines: PostgreSQL full text, Meilisearch, Typesense, Tantivy (embedded); vectors in pgvector or Qdrant; indexes kept in sync through the queue; `navree search:import <Entity>` |
| `ai` | AI SDK | Text, structured output, agents with tools, embeddings and RAG | `navree-ai` | `AiModule::for_root(...)`; `Ai` service (`ai.text`, `ai.structured::<T>`, `ai.stream`); `Agent` trait with `#[tool]` methods; memory; provider failover (Anthropic, OpenAI, Google Gemini, Mistral, Ollama, AWS Bedrock); `Embeddings`, `VectorStore`; `AiFake` for tests |
| `mcp` | MCP servers | Expose your app's tools, resources and prompts to AI clients | `navree-mcp` | `McpModule`; `#[mcp_tool]`, `#[mcp_resource]`, `#[mcp_prompt]`; streamable HTTP and stdio; auth through Navree auth or OAuth; per-tool permissions |
| `microservices` | Microservices | Request-response and events between services | `navree-micro` | `MicroModule::for_root(Transport::nats(...))`; `#[message_pattern("deals.get")]`, `#[event_pattern("deal.won")]`; `ServiceClient` (`client.send`, `client.emit`); transports TCP, NATS, Kafka, RabbitMQ, Redis Streams, MQTT; hybrid apps (HTTP plus microservice in one binary) |
| `grpc` | gRPC | gRPC and Connect services that share your modules | `navree-grpc` | `GrpcModule`; `#[grpc_service]` on a tonic service impl; guards and access rules apply; gRPC-Web and Connect |
| `graphql` | GraphQL | Schema-first or code-first GraphQL with dataloaders and subscriptions | `navree-graphql` | `GraphqlModule`; `#[resolver]` impls on async-graphql; dataloaders per request; subscriptions over WebSockets; GraphiQL at `/graphql` in development |
| `oauth` | OAuth server | Issue tokens to third parties with OAuth 2.1 and OpenID Connect | `navree-oauth` | `OAuthServerModule`; authorization code with PKCE, client credentials, device code, refresh; scopes and consent; personal access tokens; `navree oauth:client` |
| `social` | Social sign-in | Sign in with Google, GitHub, Apple, Microsoft and more | `navree-social` | `SocialModule`; `Social` service (`social.redirect("github")`, `social.user("github", code)`); links accounts to Navree auth users |
| `billing` | Billing | Subscriptions, usage metering and invoices with Stripe or Paddle | `navree-billing` | `BillingModule`; `Billable` trait for users or workspaces; plans, trials, metered usage, invoices, the customer portal, verified webhooks |
| `flags` | Feature flags | Roll features out per user, workspace or percentage | `navree-flags` | `FlagsModule`; `#[flag]` definitions; `Flags` service (`flags.active::<NewPipeline>(&scope)`); `#[requires_flag(...)]` on routes; stored in your database, cached |
| `webhooks` | Webhooks | Signed outgoing webhooks with retries, and verified incoming ones | `navree-webhooks` | `WebhooksModule`; HMAC signatures, queue-backed retries with backoff, a delivery log; verifiers for Stripe, GitHub and Standard Webhooks |
| `i18n` | Localization | Translations, plurals and locale negotiation | `navree-i18n` | `I18nModule`; Fluent files (`locales/en/main.ftl`); `t!("deal-created", count = 3)`; locale from the user, the workspace or `Accept-Language`; translated validation messages |
| `browser-testing` | Browser testing | Drive a real browser from your tests | `navree-browser` | `Browser` over the Chrome DevTools Protocol: `visit`, `type_into`, `press`, `assert_see`, screenshots on failure |
| `starter-kits` | Starter kits | Production-ready apps from one command | `navree new <name> --kit <saas\|api\|micro>` | `saas`: a Next.js and shadcn/ui front end (a BFF holding tokens in httpOnly cookies) with sign-in, workspaces, billing, settings and admin; `api`: API-only; `micro`: a microservice |

### Mapping to the docs
Each tool and library has a docs page (§6). The ecosystem page links each
card to its docs page.

## 6. Documentation layout

The manifest (`content/docs/manifest.ts`) is final for this round; every page
exists as a stub. Writers fill their stubs and **never edit the manifest**.
New sections and owners:

| Section (slug) | Pages | Owner | Status |
|---|---|---|---|
| Getting started (`getting-started`) | + `starter-kits` | patterns | planned |
| Architecture patterns (`patterns`) | `overview`, `modular-monolith`, `microservices`, `multi-tenancy`, `event-driven`, `api-first`, `backend-for-frontend` | patterns | mixed: describe built features as built |
| Workspaces (`workspaces`) | `introduction`, `resolution`, `members-and-roles`, `invitations`, `isolation` | workspace | built (written from the crate) |
| Microservices (`microservices`) | `introduction`, `transports`, `request-response`, `events`, `grpc`, `graphql`, `tracing` | micro | planned |
| AI (`ai`) | `introduction`, `agent-skills`, `navigator`, `docs-for-agents`, `ai-sdk`, `agents-and-tools`, `embeddings-and-rag`, `mcp-servers` | aidocs | `agent-skills` and `docs-for-agents` built in this round; the rest planned |
| Packages (`packages`) | `search`, `billing`, `feature-flags`, `oauth-server`, `social-sign-in`, `webhooks`, `localization`, `browser-testing` | pkg | planned |
| Tools (`tools`) | `lighthouse`, `logbook`, `fleet`, `helm`, `harbor`, `dock` | tools | planned |

Existing pages (prologue, getting started, architecture concepts, basics,
security, database, digging deeper, testing, example app) belong to the
**scrub** worker.

### Rules for planned pages
- Front-matter has `status: "planned"`. Write the page as complete,
  confident documentation of a shipped package: installation (the facade
  feature or the crate), configuration (config keys and env vars), the API
  with realistic examples, testing it, and a reference table where useful.
- The page is also the design of the package. Design an API that fits
  Navree's existing conventions exactly: read `docs/superpowers/notes/framework-api-notes.md`,
  `crates/navree/src/lib.rs` (prelude), a few existing modules
  (`crates/navree-queue`, `crates/navree-mail`, `crates/navree-access`) and the
  existing docs pages to see how modules are configured (`for_root`,
  `from_config`, `for_feature`), how config keys and env vars are named, how
  macros and extractors look, how errors are returned and how fakes work.
  Use §5's names exactly.
- Code samples are realistic and internally consistent, but they can't be
  compiled yet. Don't claim they were.
- Built pages keep the existing accuracy rule: every API you name exists.

### Rules for every page
- `apps/website/AUTHORING.md` for the components, code blocks and links.
- Link to `/performance/`, never `/benchmarks/`.
- `node scripts/check-docs.ts <files>` (from `apps/website`) passes on every
  file you touch.
- `<Diagram name="..." />` is available in MDX for the architecture diagrams
  in §7 (names: `modular-monolith`, `microservices`, `multi-tenancy`,
  `event-driven`, `api-first`, `backend-for-frontend`, `request-lifecycle`,
  `module-graph`). Use it where a picture explains the page.

## 7. Design system and diagrams

- **Brand.** Keep the tokens in `src/app/globals.css`: ink surfaces
  (`--color-ink-0..3`), lines, off-white text steps and the one accent,
  `--color-accent` `#3dff8f`. The accent is for emphasis: primary buttons,
  links, focus, the logo, the one highlighted element in a diagram. Use
  green tints (`rgb(61 255 143 / x)`) for glows and fills. Amber only for
  warnings. Dark only.
- **Type.** A characterful display face for headings with tight tracking, a
  clean UI sans for body, and Monaspace Neon for code. Self-hosted (no
  runtime font requests).
- **Components.** shadcn/ui primitives (restyled to the tokens) in
  `src/components/ui/`, Magic UI components in `src/components/magicui/`.
  Good fits: Terminal (install), Word rotate or morphing text (hero),
  Animated beam (flows in diagrams), Bento grid (pillars), Magic card and
  Border beam (feature cards), Marquee (ecosystem), Number ticker
  (performance), Dot or grid pattern and Flickering grid (backgrounds),
  Animated list (Lighthouse feed), Orbiting circles (module hub), Blur fade
  (section entrance). Restraint: one signature motion per section, not five.
- **Motion.** `motion` (Framer Motion). Everything respects
  `prefers-reduced-motion`. Animations never block reading or shift layout.
- **Quality bar.** It should feel like the best developer-tool site there
  is: generous spacing, a strict grid, crisp 1px lines, real product mocks
  instead of stock illustrations, no "AI slop" (no purple gradients, no
  emoji icons, no generic three-column feature cards with icons in circles).
- **Accessibility.** WCAG AA contrast, visible focus, semantic headings, every
  interactive thing keyboard reachable, `aria` labels for icon buttons.
- **Responsive.** Perfect at 390 px and 1440 px; no horizontal scrolling at
  any width from 320 px up.
- **Static export.** `output: "export"`; no server features at runtime.
- **Diagram rules.**
  - Hand-built SVG (or HTML+SVG) React components in `src/components/diagrams/`, one per diagram, exported for both pages and MDX (`<Diagram name>`).
  - Draw on an 8 px grid. Boxes align; lines meet box edges at the centre of a side; no line crosses a box or a label; arrowheads show direction; every box and line that matters has a label.
  - One accent element per diagram (the thing the diagram is about); everything else in ink and line tokens.
  - A short legend when there are more than two kinds of line.
  - Each diagram has `role="img"`, a `<title>` and a `<desc>` that says in words what the picture shows.
  - On phones the diagram re-lays out (stacks vertically) rather than shrinking text below 12 px. Test at 390 px.
  - Animated beams show flow (a request, an event, a message); they stop under reduced motion.

### Generated imagery

The lead is generating a matched image series with Gemini (script
`apps/website/scripts/generate-images.ts`): premium, minimal dark 3D renders,
black materials, one neon-green (`#3dff8f`) light, pure black backgrounds
that blend into the page, no text. Files land in
`apps/website/public/images/generated/` as WebP, with
`manifest.json` (key, src, width, height, alt). They may still be arriving
when you start; check the manifest, and look at each image before using it.

| Key | Ratio | Subject | Intended use |
|---|---|---|---|
| `tool-lighthouse` | 1:1 | lighthouse with a green beam | Lighthouse card/feature block |
| `tool-fleet` | 1:1 | five vessels in formation | Fleet |
| `tool-harbor` | 1:1 | gantry crane lowering a green cube | Harbor |
| `tool-helm` | 1:1 | machined ship's wheel | Helm |
| `tool-logbook` | 1:1 | black logbook with green ribbon | Logbook |
| `tool-dock` | 1:1 | container grid, one lit green | Dock |
| `tool-navigator` | 1:1 | black compass with green needle | Navigator |
| `hero-hull` | 21:9 | bow of a black hull over water | homepage hero or OG |
| `hero-archipelago` | 21:9 | modular islands linked by light | /architecture/ |
| `hero-constellation` | 21:9 | star route across the sky | /ai/ |
| `hero-wake` | 21:9 | hull leaving a green wake | /performance/ |
| `hero-rooms` | 21:9 | isolated glass rooms, one green | workspaces spotlight |
| `hero-harbor` | 21:9 | harbor at night with lighthouse | /ecosystem/ |

Use them where they lift the design: product identity on tool cards and
feature blocks, a quiet backdrop behind a page hero (masked or faded into
the black, text kept on the empty side), never as filler. Use `next/image`
with `unoptimized` (static export) or a plain `<img>` with width, height,
`loading="lazy"` below the fold and the manifest's alt text (or `alt=""`
when purely decorative). If an image doesn't fit or looks off, leave it
out and say so in your report.

## 8. AI-native, concretely

- **Agent skills** in `skills/` at the repository root: one directory per
  skill with a `SKILL.md` (YAML front-matter `name` and `description` that
  says when to use it; then steps, commands and code templates). An umbrella
  `skills/navree/SKILL.md` indexes the rest. Skills for built features are
  verified against the code; the workspace skill comes from the crate. The
  site serves them at `/skills/<name>/SKILL.md` and lists them in
  `/skills/index.json`.
- **`navree new`** writes `AGENTS.md` (project conventions for coding
  agents) and a `CLAUDE.md` that points at it.
- **Docs for agents:** `/llms.txt` (index with one line per page linking the
  `.md`), `/llms-full.txt` (everything), `/docs/<section>/<page>.md`, and on
  every docs page "Copy page" (Markdown), "Open in Claude" and "Open in
  ChatGPT" actions.
- **Navigator, the AI SDK and MCP servers** are documented as built (§5),
  and are planned in the build plan.

## 9. Workers and file ownership

Only touch files you own. Shared files are read-only to you unless listed.

| Worker | Owns |
|---|---|
| foundation | `apps/website/package.json`, the lockfile, `components.json`, `src/components/ui/**`, `src/components/magicui/**`, `src/lib/utils.ts`, `src/app/globals.css`, `src/app/fonts.ts`, `src/app/layout.tsx`, `src/components/site/**`, `src/components/docs/sidebar.tsx`, `src/components/docs/mobile-nav.tsx`, `src/lib/docs.ts`, `site.config.ts`, `src/app/og.png/**`, `src/app/styleguide/**`; creates the stub routes and section components below and then hands them over |
| home | `src/app/page.tsx`, `src/app/home.css`, `src/components/home/**` except the four `*-section.tsx` files below |
| ecosystem | `src/app/ecosystem/**`, `src/components/ecosystem/**`, `src/components/home/ecosystem-section.tsx`, may enrich `src/content/ecosystem.ts` (keep its exports and names stable) |
| architecture | `src/app/architecture/**`, `src/components/diagrams/**`, `src/components/mdx/diagram.tsx`, `src/components/home/architecture-section.tsx` |
| ai | `src/app/ai/**`, `src/components/home/ai-section.tsx`, `src/components/docs/page-actions.tsx`, `src/app/docs/[...slug]/page.tsx`, new route handlers and scripts for `llms.txt`, `.md` pages and served skills |
| performance | `src/app/performance/**`, `src/app/benchmarks/**`, `src/components/bench/**`, `src/components/home/bench-teaser.tsx`, `src/components/home/performance-section.tsx`, `src/lib/bench*.ts`, the benchmark part of `scripts/prebuild.ts`, `public/data/bench/**` |
| workspace | `crates/navree-workspace/**`, its rows in the root `Cargo.toml`, `crates/navree/**` (feature, re-export, hierarchy test row), `content/docs/workspaces/*.mdx`, `skills/navree-workspaces/**` |
| patterns, micro, aidocs, pkg, tools | their docs pages (§6) |
| scrub | every pre-existing `content/docs/**/*.mdx`, `README.md`, comments and descriptions in `crates/**` and `apps/crm-*/**` (never behaviour) |
| skills | `skills/**` except `skills/navree-workspaces/`, `crates/navree-cli/**` (the `AGENTS.md` / `CLAUDE.md` templates and their tests) |

**Shared dev server.** One `next dev` serves `apps/website` at
`http://localhost:3210` and reloads on save. Use it to look at your work;
don't start another (Next refuses a second one and prints the running one).
If it is down, start it in the background from `apps/website` with
`pnpm exec next dev --port 3210`. Take screenshots with Playwright (it is a
dev dependency; Chromium is installed) into
`apps/website/screenshots/v2/<your-worker>/`, at 1440 and 390 px wide.

**Rust builds.** Use your own target dir (`CARGO_TARGET_DIR=target/agents/<worker>`)
and `CARGO_BUILD_JOBS=6`. The machine has 24 cores but often only a few GB
of free memory.
