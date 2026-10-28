# The evidence harness

`e2e/` proves that Navree's packages work in real containers, against real
backing services, under load. One command builds the evidence app into a
release image, starts it as three processes (web, worker, scheduler) next to
TimescaleDB, Redis, Mailpit, Meilisearch, MinIO and NATS, runs every check,
samples resource usage while k6 drives load, and writes the evidence to
`target/evidence/<run-id>/`.

```sh
e2e/run.sh                                                    # smoke: every check, short k6 runs
e2e/run.sh --load-profile load --duration 2m --vus 50         # sustained load
e2e/run.sh --load-profile spike --scenarios checks,baseline   # a spike to 200 VUs
```

From PowerShell, `e2e/run.ps1` takes the same arguments (it runs `run.sh`
with Git Bash or WSL's bash). The only host requirements are Docker with
Compose v2 and bash.

## What is here

| Path | What |
|---|---|
| `docker/compose.yml` | The stack: backing services with health checks and pinned images, the app's three processes, k6 and the screenshot runner, behind profiles |
| `docker/Dockerfile` | The evidence app's image: cargo-chef dependency layer, release build, Debian slim runtime, non-root, tini, health check |
| `docker/evidence.env` | The app's settings inside the compose network |
| `load/lib/` | The k6 library: `config.js` (environment), `auth.js` (sign-in, the workspace header, a session pool), `http.js` (tagged requests), `profiles.js` (smoke, load, spike and thresholds), `summary.js` (the files a run reads) |
| `load/scenarios/` | One file per package: `auth.js`, `tasks.js` (ORM and workspaces), `reports.js` (queue), `search.js`, `realtime.js`, and `baseline.js`, which mixes them |
| `load/checks/http.js` | Runs the rows of `checks/http.json` |
| `checks/http.json` | HTTP smoke rows: method, path, expected status, per package |
| `browser/` | Playwright screenshots of every UI tool in `browser/tools.json` at 1440 and 390 pixels wide |
| `tools/summarize.mjs` | Builds `summary.json` and `summary.md` from a run's files |
| `run.sh`, `run.ps1` | The harness |
| `mcp-sdk/` | Not part of the harness: the official MCP TypeScript SDK's client (pinned in `package.json`) against navree-mcp over stdio and Streamable HTTP. Run `npm ci --prefix e2e/mcp-sdk`, then `cargo test -p navree-mcp --test sdk_conformance -- --ignored --nocapture` |

The app is `apps/evidence-app`: a Navree application that wires the
framework's packages and tools (`src/platform.rs`), with domain models and
the `evidence` seeder (three workspaces, an operator who is a super admin,
200 load accounts `load-001@evidence.test` and up; password
`evidence-password`):

| Module | What it exercises |
|---|---|
| `projects` | A SeaORM entity behind `WorkspaceRepo`, row-level security, `#[derive(Searchable)]` synced to Meilisearch by queued jobs, broadcasts on change |
| `tasks` | A navree-orm model with `#[model(workspace)]`, route-model binding with `#[can(.., from = "id")]`, `Accessible` list filters |
| `reports` | `POST /api/v1/reports` queues `reports.generate` in the request's unit of work; the worker computes and emits `report.computed` on NATS JetStream; the micro event handler in `serve` marks it `done` |
| `live` | `POST /api/v1/live/publish` on the workspace's private channel; Redis backplane between processes |
| `showcase` | GraphQL fields (`taskCounts`, `tasks`), the MCP tool `workspace_summary`, two feature flags |
| `admin` | Helm resources (users, workspaces, projects) with dashboard metrics |

The three process types are `serve` (web), `fleet` (worker: Fleet's master
supervising `fleet:work` children, visible at `/_fleet`) and
`schedule:work` (scheduler).

## Running it

`run.sh` goes through these steps, each recorded as a check:

1. **image: build**: `docker build -f e2e/docker/Dockerfile` (skip with `--no-build`).
2. **stack: services healthy**: `docker compose up --wait` for the services of
   the chosen profiles (`--services "search storage nats"` by default).
3. **app: migrate and seed**: `migrate:fresh --seed` in a one-off container.
4. **app: web, worker and scheduler up**: the three processes, web healthy at
   `/health/ready`.
5. **checks**: the HTTP rows (`checks/http.json`).
6. **k6: \<scenario\>**: each scenario under the profile (`--scenarios`,
   default `checks,baseline`), with `docker stats` sampled every 2 s.
7. **browser: screenshots**: every tool at both widths.
8. **app: \<process\> survived**: no restart, no OOM kill.

Then it collects every container's log and tears the stack down (`--keep`
leaves it up; `e2e/run.sh down` removes it). The exit status is 0 only when
every check passed.

Host ports (all overridable with `E2E_*_PORT`, see `docker/compose.yml`):
the app on 47080, PostgreSQL 47432, Redis 47379, Mailpit 47825 (SMTP 47025),
Meilisearch 47700, MinIO 47900 (console 47901), NATS 47222.

### Running the app outside Docker

`e2e/run.sh up` starts the backing services alone. Then, from
`apps/evidence-app` with `.env` copied from `.env.example`:

```sh
cargo run -p evidence-app -- migrate:fresh --seed
cargo run -p evidence-app -- serve           # http://localhost:8080
cargo run -p evidence-app -- queue:work
```

and point k6 at it: `docker run --rm -i --network host -v "$PWD/e2e/load:/scripts" grafana/k6:1.4.0 run -e BASE_URL=http://host.docker.internal:8080 /scripts/scenarios/baseline.js`.

## Load profiles

`load/lib/profiles.js` defines three profiles, picked with `PROFILE`
(`--load-profile`):

| Profile | Shape | Default thresholds |
|---|---|---|
| `smoke` | 2 VUs for 30 s | p95 < 500 ms, p99 < 1500 ms, errors < 1 %, checks > 99 % |
| `load` | ramp to `VUS` (50) in 30 s, hold for `DURATION` (3m), ramp down | p95 < 500 ms, p99 < 1500 ms, errors < 1 %, checks > 99 % |
| `spike` | 10 VUs, jump to `SPIKE_VUS` (200) for 30 s, back to 10 | p95 < 1500 ms, p99 < 3000 ms, errors < 1 % |

`P95_MS`, `P99_MS` and `ERROR_RATE` override the thresholds for a run.
Scenarios add their own: `reports.js` requires 99 % of reports done within
the polling window and `report_done_ms` p95 < 5 s; `realtime.js` requires
99 % of WebSocket round trips delivered and `realtime_delivery_ms`
p95 < 1 s. A failed threshold fails the scenario's check.

## Adding a package

A package proves itself with up to three things, all data or small files:

1. **HTTP rows** in `checks/http.json`, one per route that must answer:

   ```json
   { "name": "flags", "package": "navree-flags", "method": "GET", "path": "/api/flags",
     "auth": true, "workspace": true, "expect": 200, "json_path": "data.0" }
   ```

   `auth` sends the operator's bearer token, `workspace` the `X-Workspace`
   header (`acme`); `expect` is a status or a list; `json_path` must exist
   in the body.

2. **A scenario** in `load/scenarios/<package>.js`. Export a step function
   that takes a session (`{ token, workspace }`) so `baseline.js` can mix it
   in, and make the file runnable alone:

   ```js
   import { check } from 'k6';
   import { signInPool, sessionFor } from '../lib/auth.js';
   import { WORKSPACE } from '../lib/config.js';
   import { get } from '../lib/http.js';
   import { options as profileOptions } from '../lib/profiles.js';
   import { summaryFor } from '../lib/summary.js';

   export function flagsStep(session) {
     const res = get(session, '/flags', 'GET /api/flags');   // the name groups latencies
     check(res, { 'flags: 200': (r) => r.status === 200 });
   }

   export const options = profileOptions({ extra: { 'http_req_duration{name:GET /api/flags}': ['p(95)<200'] } });
   export function setup() {
     return { pool: signInPool(20).map((s) => ({ token: s.token, workspace: WORKSPACE })) };
   }
   export default function (data) { flagsStep(sessionFor(data.pool)); }
   export const handleSummary = summaryFor('flags');
   ```

   Run it with `e2e/run.sh --scenarios checks,flags --load-profile load`.
   Tag every request with its route template (`GET /api/v1/tasks/{id}`,
   not the id) so latencies group per route.

3. **A UI tool** in `browser/tools.json`, when the package serves pages:

   ```json
   { "name": "logbook", "package": "navree-logbook", "pages": ["/_logbook"], "expect": "Logbook" }
   ```

   The runner signs in on any page that shows a password field (as the
   operator), screenshots each page at each width and fails the row when
   the status is 400 or more, the `expect` text is missing or the page
   still asks for a password.

The app side goes in `apps/evidence-app`: the module in `src/platform.rs`
(configured in `config/app.toml`, hosts in `e2e/docker/evidence.env`), a
domain module under `src/modules/` when the package needs one, and seed data
in `src/seed.rs`.

## The evidence format

`target/evidence/<run-id>/` (the run id is the UTC start time and the
profile, or `--run-id`):

| File | What |
|---|---|
| `summary.json` | The run: `status` (`pass` or `fail`), `totals`, `checks[]` (`name`, `status`, `seconds`, `detail`), `scenarios[]` (each k6 digest), `resources` (per phase and container: CPU and memory, average and peak), commit, host, Docker version, image size |
| `summary.md` | The same, as tables |
| `checks.jsonl` | The harness steps as they ran |
| `k6/<scenario>-<profile>.json` | k6's full end-of-test data |
| `k6/<scenario>-<profile>.digest.json` | Requests, RPS, error rate, latency (avg, med, p90, p95, p99, max), per-route latencies, thresholds and whether they held |
| `k6/<scenario>-<profile>.txt` | The text report |
| `screenshots/<tool>--<page>--<width>.png` | The screenshots, and `results.json` (status, console errors, horizontal overflow per page) |
| `resources/<scenario>-<profile>.jsonl` | `docker stats` samples, every 2 s while the scenario ran |
| `logs/<service>.log` | Every container's log, with timestamps; `logs/step-*.log` the output of each step |
| `run.log` | What the harness did |

A digest looks like:

```json
{
  "scenario": "baseline", "profile": "load", "passed": true,
  "requests": 151234, "rps": 840.2, "error_rate": 0,
  "latency_ms": { "avg": 12.1, "med": 8.3, "p90": 21.0, "p95": 29.4, "p99": 61.2, "max": 412.0 },
  "per_request": { "POST /api/v1/tasks": { "p(95)": 31.2, "...": 0 } },
  "thresholds": { "http_req_duration: p(95)<500": true }
}
```

## The first baseline

`e2e/run.sh --load-profile load --duration 3m --vus 50 --scenarios checks,baseline`
on Docker Desktop (WSL2, 24 CPUs, 16 GB; k6 on the same host), commit
`953e2145`, 50 virtual users with no think time:

| Requests | RPS | Errors | p50 | p95 | p99 | Max |
|---|---|---|---|---|---|---|
| 408,206 | 1,848 | 0 % | 9.7 ms | 19.0 ms | 27.8 ms | 132.6 ms |

Every route's p99 stayed under 26 ms except sign-in (Argon2id, p99
53.6 ms). 8,811 reports went through the queue and NATS, all done (p95
1.06 s from the request to the read that saw `done`, polled every 250 ms);
498 WebSocket round trips were delivered (p95 47 ms). The web process
averaged 4 cores and peaked at 961 MB; PostgreSQL averaged 2.6 cores.

The spike profile on the same build (`--load-profile spike`: 10 VUs, a
jump to 200 for 30 s, back to 10) served 116,719 requests with no error,
p95 97.2 ms and p99 110.1 ms, and every process survived.

## Known limits

- Typesense (profile `typesense`) exits with a segfault before printing
  anything on Docker Desktop's WSL2 kernel 6.18, for every image from 28
  to 30.1; the app uses Meilisearch. The profile is there for hosts where
  it runs.
- k6 runs in a container on the same Docker host as the app, so the load
  generator and the app share CPUs.
- Every package's module boots in the app (its migrations run, its routes
  are mounted, its health checks report), but several have no scenario or
  domain surface yet: social sign-in, SSO, directory sync, custom domains
  and integrations have no fake providers wired in; gRPC has no service;
  documents, media, sheets, workflows, webhooks, settings and i18n have no
  domain use; MySQL and MongoDB start (profiles `mysql`, `mongo`) but the
  app does not use them. These are the rows and scenarios later builders
  add.
- Logbook is off (`logbook.enabled = false`): it records every request's
  bodies, which is development tooling, not something to measure under
  load. `APP_LOGBOOK__ENABLED=true` turns it on in the release image;
  reaching it from outside the container also needs `logbook.allow_remote`
  and a route authorizer (see the Logbook docs).
