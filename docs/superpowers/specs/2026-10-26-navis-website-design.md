# Navis: the rename, the website and the documentation

Date: 2026-10-26. Status: approved decisions from the user, the rest are the
builder's calls (marked).

## 1. Decisions the user made

- **Name: Navis** (Latin for "ship": the hull that rides on a keel, which was
  the framework's working name). Domain **navis.sh** (free on 2026-10-26; the
  user buys it, nothing is bought for them). crates.io `navis` was free; the
  npm name `navis` is taken (irrelevant for a Rust framework).
- **Look: neon green on black.**
- **Rename everything**: crates, CLI, config, docs, the CRM and the benchmark
  all say Navis, so the documentation and the code match.
- The website and documentation should be in the spirit of Laravel's:
  - a landing page that sells the framework;
  - complete documentation that covers everything;
  - a benchmark section transparent enough that sceptical readers accept it.

## 2. The rename (Keel → Navis)

The mapping is mechanical and total:

| Area | Old | New |
|---|---|---|
| Crate names and directories | `keel-*` | `navis-*` (`crates/navis-http`, …) |
| Facade | `keel` | `navis` |
| CLI binary and package | `keel` | `navis` |
| Paths | `keel::`, `::keel::` | `navis::`, `::navis::` |
| Type and prose names | `Keel` | `Navis` |
| Constants | `KEEL_` | `NAVIS_` |

That covers env vars like `NAVIS_API_URL`, cookies like `navis_at`/`navis_rt`, the default JWT issuer (`"navis"`), the realtime key (`navis-crm-key`), and PostgreSQL `set_config` names (`navis.actor`). Proc-macro output must emit `::navis::`. Trybuild `.stderr` files and insta snapshots are updated by re-running their tests, not by hand-editing.

Builder's calls:
- **Historical documents are not rewritten.** `docs/prd/`, `docs/superpowers/specs|plans|notes` from 2026-10-25 describe the build under its working name. The README gets one line: "Navis was developed under the working name Keel."
- **Unchanged:** the repository directory, the git branch name and the T3 project.
- **Benchmark.** Its JWT issuer becomes `"navis"` (`bench/tools/fixtures.mjs`, `run.mjs`), and any implementation that hard-codes `"keel"` reads `JWT_ISSUER` instead. `bench/frameworks/keel` becomes `bench/frameworks/navis` (image `bench-navis`). Results files are renamed and relabelled (`navis.json`, label "Navis"): the numbers stay valid because the measured code is the same, and the method notes say it was measured under the working name. Every implementation's smoke run (`node bench/run.mjs --smoke --only <name>`) must still pass conformance.
- **Done means:**
  - `cargo test --workspace`, clippy (`-D warnings`) and `cargo fmt --check` are green;
  - crm-web's lint, typecheck, Vitest and build are green;
  - `git grep -i keel` outside the historical documents and this design returns only deliberate mentions (the "formerly Keel" line).

## 3. The website (`apps/website`)

Builder's calls on how it gets made:
- **Stack.** Next.js 16 (App Router) with `output: "export"`: a fully static site that any static host can serve (Vercel, Cloudflare Pages, GitHub Pages). pnpm, TypeScript, Tailwind. Standalone like `apps/crm-web`, no workspace root.
- **Content and search.** Docs are MDX files under `apps/website/content/docs/<section>/<page>.mdx`, with front-matter (`title`, `description`) and one navigation manifest. Code highlighting uses Shiki at build time (rust, toml, sh, ts, json, sql). Search is client-side, built at build time; no external service.
- **Theme.** Dark only:
  - near-black backgrounds;
  - one neon-green accent, used sparingly (links, focus, key highlights, the logo) so it stays striking rather than garish;
  - off-white body text with WCAG AA contrast;
  - system UI sans with a monospace for code.
  
  New logo and favicon: a simple hull/keel mark that works at 16 px.
- **Configuration.** All site-wide strings live in one `site.config.ts`: the name, tagline, `repoUrl` and the install command. The GitHub URL isn't decided yet, so it is a placeholder marked TODO there and listed for the user.

### Pages
1. **Landing (`/`).**
   - **Hero:** the name, a one-line promise (Laravel's completeness, NestJS's structure, Rust's speed and footprint), a get-started CTA and an install snippet.
   - **Code:** short, real code tabs: a module, a controller with access attributes, validation, a queued job.
   - **Features:** a grid grouped like the docs.
   - **Benchmark teaser:** honest headline numbers linking to the benchmark section.
   - **Closing:** "what's in the box" (every module) and a footer.
2. **Docs (`/docs/...`).** A Laravel-style layout:
   - a left sidebar with sections, a right "on this page" table of contents and a page body;
   - previous/next links, copy buttons on code blocks, anchor links on headings, search (⌘K) and an "edit this page" link built from `repoUrl`.
3. **Benchmarks (`/benchmarks`).** Built from `bench/results/*.json` at build time (copied or imported; no hand-typed numbers):
   - interactive ranked bar charts per test, Navis highlighted;
   - a full results table, p99 latency, memory and startup.
   
   The page also covers:
   - methodology: the spec, the 170-check conformance test, fairness rules, CPU pinning, the drift check and caveats (load-generator limits, DB-bound tests, the Laravel-FPM port exhaustion, wrk's clock fix);
   - per-framework stack details (from each implementation's README) with links to their source;
   - the raw JSON for download, and "reproduce it yourself" commands.
   
   It states plainly where Navis is slower and why. Credibility comes from candour.

### Documentation pages (all real, no placeholders)
The sections follow Laravel's docs. Every page is written from the actual code and tests of the crates it covers, and every code sample uses real APIs. A non-trivial Rust snippet must compile against the workspace: the writer checks it in a scratch crate or a `tests/` example, or quotes it from existing tests or the CRM. Content owners are given in brackets.

- **Prologue** [A]: Introduction (what Navis is, who it's for, the comparison with Laravel/NestJS/Loco/Axum, "why the name"), Release notes 0.1, Roadmap (the deferred items from the build design §2: Redis, apalis, passkeys and so on), Contributing.
- **Getting started** [A]: Installation, Your first app (a tutorial from `navis new` to a CRUD API with auth and tests), Configuration, Directory structure, Deployment.
- **Architecture concepts** [B]: Request lifecycle, Modules, Providers and dependency injection, Lifecycle hooks, The crate hierarchy, The facade and features.
- **The basics** [B]: Routing and controllers, Access control on routes, The request pipeline and middleware, Requests and extractors, Validation, Responses and API resources, Errors, Logging, Rate limiting, CORS and security headers, Sessions and CSRF.
- **Security** [C]: Authentication (JWT, refresh tokens, strategies, `CurrentUser`), Account flows (registration, password reset, email verification, two-factor), Authorization (roles, permissions, CASL abilities, record checks, SQL filters), Encryption and hashing, The audit log.
- **Database** [D]: Getting started (SQLite and PostgreSQL), Entities and repositories, The unit of work and transactions, Migrations, Seeding and factories, Pagination.
- **Digging deeper**:
  - [E]: Cache, Events, Queues, Task scheduling, Mail, Notifications.
  - [F]: File storage, HTTP client, Real-time broadcasting, Views and templates, OpenAPI, Health and metrics, Console commands and the CLI, Maintenance mode.
- **Testing** [D]: Getting started, HTTP tests, Database tests, Fakes.
- **Example application** [F]: A tour of the CRM (API modules, abilities, the BFF web app, real time).
- **Benchmarks**: the `/benchmarks` page, linked from the docs sidebar.

## 4. Order of work

1. **In parallel:**
   - the rename;
   - the site shell: design system, landing, docs layout and every page in the manifest as a stub, the benchmarks page and a docs-authoring guide in `apps/website/AUTHORING.md`.
2. **Then the docs content**, by owners A–F in parallel, against the renamed code.
3. **Then the checks:**
   - an accuracy review (sampled claims and snippets against the code), links and the build;
   - visual checks of the key pages at desktop and phone widths.
4. **Nothing is deployed or published.** Buying navis.sh, creating the GitHub organisation, publishing crates and deploying the site are the user's steps; the README says how.
