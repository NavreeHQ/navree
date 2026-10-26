# Navree: the rename, the website and the documentation

Date: 2026-10-26. Status: approved decisions from the user, the rest are the
builder's calls (marked).

## 1. Decisions the user made

- **Name: Navree.** It is a blend of two names that matter to the user. The
  framework was built as Keel and was briefly called Navis (navis.sh), until
  the user required a .com domain. The user bought **navree.com** on Cloudflare
  on 2026-10-26, and the site will be deployed there. crates.io `navree` and
  `navree-*` were free, and so was the npm name. The GitHub name `navree` is
  taken (`navree-rs` and `navreehq` were free), so `repoUrl` stays a marked
  placeholder until the user creates the organisation.
- **Privacy:** the public site doesn't name the people behind the name unless
  the user asks for a dedication.
- **Look: neon green on black.**
- **Rename everything**: crates, CLI, config, docs, the CRM and the benchmark
  all say Navree, so the documentation and the code match.
- The website and documentation should be in the spirit of Laravel's:
  - a landing page that sells the framework;
  - complete documentation that covers everything;
  - a benchmark section transparent enough that sceptical readers accept it.

## 2. The rename (Keel → Navis → Navree)

The first pass renamed Keel to Navis (commit deb4fa4). The second pass applies the same mapping from the Navis names to Navree (`navis` → `navree`, `Navis` → `Navree`, `NAVIS` → `NAVREE`), including `apps/website`, and the domain changes from navis.sh to navree.com. The table shows the end state.

The mapping is mechanical and total:

| Area | Old | New |
|---|---|---|
| Crate names and directories | `keel-*` | `navree-*` (`crates/navree-http`, …) |
| Facade | `keel` | `navree` |
| CLI binary and package | `keel` | `navree` |
| Paths | `keel::`, `::keel::` | `navree::`, `::navree::` |
| Type and prose names | `Keel` | `Navree` |
| Constants | `KEEL_` | `NAVREE_` |

That covers env vars like `NAVREE_API_URL`, cookies like `navree_at`/`navree_rt`, the default JWT issuer (`"navree"`), the realtime key (`navree-crm-key`), and PostgreSQL `set_config` names (`navree.actor`). Proc-macro output must emit `::navree::`. Trybuild `.stderr` files and insta snapshots are updated by re-running their tests, not by hand-editing.

Builder's calls:
- **Historical documents are not rewritten.** `docs/prd/`, `docs/superpowers/specs|plans|notes` from 2026-10-25 describe the build under its working name. The README gets one line: "Navree was developed under the working name Keel."
- **Unchanged:** the repository directory, the git branch name and the T3 project.
- **Benchmark.** Its JWT issuer becomes `"navree"` (`bench/tools/fixtures.mjs`, `run.mjs`), and any implementation that hard-codes `"keel"` reads `JWT_ISSUER` instead. `bench/frameworks/keel` becomes `bench/frameworks/navree` (image `bench-navree`). Results files are renamed and relabelled (`navree.json`, label "Navree"): the numbers stay valid because the measured code is the same, and the method notes say it was measured under the working name. Every implementation's smoke run (`node bench/run.mjs --smoke --only <name>`) must still pass conformance.
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
   - interactive ranked bar charts per test, Navree highlighted;
   - a full results table, p99 latency, memory and startup.
   
   The page also covers:
   - methodology: the spec, the 170-check conformance test, fairness rules, CPU pinning, the drift check and caveats (load-generator limits, DB-bound tests, the Laravel-FPM port exhaustion, wrk's clock fix);
   - per-framework stack details (from each implementation's README) with links to their source;
   - the raw JSON for download, and "reproduce it yourself" commands.
   
   It states plainly where Navree is slower and why. Credibility comes from candour.

### Documentation pages (all real, no placeholders)
The sections follow Laravel's docs. Every page is written from the actual code and tests of the crates it covers, and every code sample uses real APIs. A non-trivial Rust snippet must compile against the workspace: the writer checks it in a scratch crate or a `tests/` example, or quotes it from existing tests or the CRM. Content owners are given in brackets.

- **Prologue** [A]: Introduction (what Navree is, who it's for, the comparison with Laravel/NestJS/Loco/Axum, "why the name"), Release notes 0.1, Roadmap (the deferred items from the build design §2: Redis, apalis, passkeys and so on), Contributing.
- **Getting started** [A]: Installation, Your first app (a tutorial from `navree new` to a CRUD API with auth and tests), Configuration, Directory structure, Deployment.
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
4. **Nothing is published without the user.** The user owns navree.com (Cloudflare). The site deploys to Cloudflare Pages on navree.com once the user gives the go-ahead and a fresh API token through the secure prompt. Creating the GitHub organisation and publishing crates are the user's steps; the README says how.

## 5. Repository, releases and hosting (user decisions, 2026-10-26)

- **One repository, `NavreeHQ/navree`**: the framework crates, the CLI, `apps/website` (navree.com and the docs), the CRM example and `bench/`. It can be split later.
  - No git submodules.
  - No `illuminate/*`-style read-only split repositories. crates.io publishes each crate directly from the monorepo, so Packagist's one-repository-per-package rule doesn't apply.
- **Private until launch.** The repository flips to public once the docs and site are finished and the user has reviewed them.
  - Only the `main` branch is pushed; local `keel/framework` is pushed as `main`.
  - T3's local `refs/t3/*` checkpoints are never pushed.
  - Before every push, check the history with gitleaks; the three known findings are test values.
- **Licence.** `MIT OR Apache-2.0`, in `LICENSE-MIT` and `LICENSE-APACHE`, copyright Amritpal Singh and the Navree contributors.
- **Versioning.** All crates share one version (`workspace.package.version`), Laravel-style.
  - A release tag `vX.Y.Z` on `main` triggers a GitHub Action.
  - It publishes the crates to crates.io in layer order (L0 → L6), creates the GitHub release and deploys the docs.
  - Patch branches (`0.1`, …) come later, once there are users.
- **Hosting: everything on the user's Laravel Forge server** (`forge@206.189.137.83`, `production-app-server-2`, Ubuntu 24.04).
  - It already hosts about a dozen production sites. Nothing outside the Navree site may be touched.
  - There is no passwordless sudo and no Docker, so nginx, TLS and daemons are configured through Forge (the user's Forge UI), not by hand.
  - navree.com is a Forge site whose web root serves the website's static export.
  - DNS stays on Cloudflare (proxied A record to the server). A Cloudflare token, if needed, is read from the git-ignored `cf.token` file and never printed.
  - A live CRM demo (crm-api binary, crm-web, a PostgreSQL database on the server) can follow at `demo.navree.com`.
