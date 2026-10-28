---
name: navree-lighthouse
description: "Application monitoring with navree-lighthouse: the `lighthouse` facade feature (in `full`), `LighthouseModule::from_config()` / `for_root(LighthouseOptions)` and the `lighthouse` config section, what it records through its telemetry sink (5xx and panics, slow requests and queries, failed jobs and scheduled tasks, failing outgoing calls), issues grouped by fingerprint with statuses (open, resolved, resolved in the next release, ignored), reporting handled errors with `Arc<Lighthouse>` (`report`, `capture(Report::..)`, `tag_current`), sampling (0.1 by default; errors and slow work always kept), exact counts under load and the sampled-out figure, alerts (email, Slack, signed Standard Webhooks, your own `AlertChannel` via `m.alert_channel::<C>()`, `lighthouse.alert` jobs, cooldown, spikes, `raise_alert`), the `/_lighthouse` dashboard and its sign-in, the JSON API under `/_lighthouse/api` behind the `view` and `resolve` abilities (`lighthouse.view`, `lighthouse.resolve`), MCP tools for agents (`lighthouse_list_issues`, `lighthouse_get_issue`, `lighthouse_resolve_issue`, ..., the resource `lighthouse://issues/{id}` and the prompt `fix-lighthouse-issue`, with the `mcp` feature and `McpModule`), `lighthouse:prune` and `lighthouse:test-alerts`, a separate database, and `LighthouseFake` in tests. Use when adding monitoring to an app, reporting an error you handle, configuring alerts, giving someone or an agent access to the dashboard, its API or its MCP tools, writing tests that assert on reports, or debugging why an error is not an issue, why an alert was not sent, or why the dashboard answers 401, 403 or 404."
---

# Lighthouse

`navree::lighthouse` (crate `navree-lighthouse`, facade feature `lighthouse`,
part of `full`) monitors the app from inside it. A telemetry sink receives
every request, query, job attempt, scheduled run, mail, cache call, outgoing
call and exception the framework emits; one background task groups each
request's or job's events, decides what to keep and writes batches to five
tables (`lighthouse_issues`, `_occurrences`, `_samples`, `_aggregates`,
`_alerts`). The dashboard at `/_lighthouse` and the JSON API under
`/_lighthouse/api` read them. Everything below is in the code: when in doubt
read `crates/navree-lighthouse/src` and its tests in
`crates/navree-lighthouse/tests/lighthouse/` and
`crates/navree/tests/lighthouse_docs.rs` (the docs examples through the
facade). The docs page is `apps/website/content/docs/tools/lighthouse.mdx`.

## Steps: add Lighthouse to an app

1. The feature is in `full`, so `navree = { .. }` with default features has
   it. With `default-features = false`, add `"lighthouse"` (it turns on
   `telemetry`, `http`, `crypto`, `view`, `http-client`, `db`, `mail`,
   `schedule`, `access`, `auth`). `LighthouseModule` and
   `ModuleBuilderLighthouseExt` are in the prelude.

2. Add the module after the ones it builds on. `navree new` apps already
   have it in `src/app.rs`:

```rust
Navree::app()
    .module(ConfigModule::for_root(sources))
    .module(CryptoModule::from_config())     // the dashboard's session cookie
    .module(HttpModule::from_config())
    .module(DatabaseModule::from_config())   // the store (unless lighthouse.store = "off")
    .module(MailModule::from_config())       // email alerts only
    .module(QueueModule::from_config())      // optional: alerts as lighthouse.alert jobs
    .module(AuthModule::from_config())       // the dashboard's sign-in
    .module(AccessModule::for_root(AccessOptions { abilities: abilities::define, super_admin_role: Some("admin".into()) }))
    .module(LighthouseModule::from_config())
```

   The dashboard's routes declare `can("view" | "resolve", "Lighthouse")`, so
   **an app without `AccessModule` fails to boot** unless
   `lighthouse.dashboard = false`.

3. Run `navree migrate` (the `lighthouse` migration set).

4. Grant access: `navree access:role ops`, `navree access:grant ops
   lighthouse.view`, `navree access:grant ops lighthouse.resolve`, then give
   users the role. Super-admins pass every check. Or in the abilities
   function: `rules.can("view", "Lighthouse"); rules.can("resolve", "Lighthouse");`.

5. Open `/_lighthouse` and sign in with an app account (email, password,
   then a two-factor code if the account has one).

## Reporting errors you handle

```rust
use navree::lighthouse::{Level, Lighthouse, Report};
use navree::prelude::*;

#[injectable]
pub struct PaymentWebhooks {
    lighthouse: Arc<Lighthouse>,
}

impl PaymentWebhooks {
    fn on_failure(&self, error: &InvoiceError) {
        // Concrete type: its type name joins the fingerprint.
        self.lighthouse.report(error);
        // More control:
        self.lighthouse.capture(
            Report::error(error)
                .level(Level::Warning)            // Error (default), Warning, Info (never alerts)
                .context("invoice_id", 1042)      // any Serialize; masked by name
                .tag("provider", "acme-pay")      // filter issues by tag=provider:acme-pay
                .fingerprint(["acme-pay", "card-declined"]) // group however messages differ
                .user(user_id)                    // outside a request
                .backtrace(),
        );
        self.lighthouse.capture(Report::message("payment event without a customer"));
        self.lighthouse.tag_current("plan", "pro"); // tags everything this request reports
    }
}
```

`report` and `capture` return at once. Inside a request or job the
occurrence waits for the request or job to end, so it gets its route (the
issue's culprit), user, workspace and trace.

## What becomes an issue

| Kind | When | Fingerprint |
|---|---|---|
| `exception` | a 5xx response (navree-http's `ExceptionReported`), a panic, or `report`/`capture` | error type (when known) + route or job + normalized message chain; panics: file:line + message; `.fingerprint([..])` overrides |
| `slow_request` | duration >= `thresholds.slow_request_ms` (1000) | method + route template |
| `slow_query` | duration >= `thresholds.slow_query_ms` (200) | the normalized SQL (`?` for literals, `IN (?)`) |
| `failed_job` | `JobFailed` with `will_retry = false` | job type + normalized error |
| `scheduled_task` | `ScheduledTaskRan` with an error | task + normalized error |
| `outgoing_request` | `HttpClient` error, 5xx, or >= `slow_outgoing_ms` (2000) | host + `5xx` / `slow` / `timeout` / `connect` / `error` |

Numbers, UUIDs, quoted strings, emails and hex ids in messages are replaced
before hashing. 4xx responses only count towards the route's error rate.
Lighthouse's own routes, `lighthouse.ignore.routes` (`/health/*`, `/metrics`),
`lighthouse.ignore.jobs` and `lighthouse.alert` jobs are never recorded.
Queries run with `execute_unprepared` are not timed by SeaORM, so they never
show up.

## Counts, sampling and load

- **Counts are exact under any load.** The sink adds every request, query,
  job attempt, outgoing call, exception and issue hit to in-memory
  per-minute/per-hour counters on the recording thread (64 sharded locks,
  no I/O, nothing queued); the writer flushes them every second as this
  process's rows. Never "fix" a count by raising `lighthouse.buffer`.
- **Details are sampled.** Kept always: 5xx and requests that reported,
  failed job attempts, slow requests/jobs, the first 100 occurrences per
  issue per hour. The rest: `lighthouse.sample.requests` / `.jobs`
  (default `0.1`), `.exceptions` (default `1.0`). Deterministic by request
  id.
- **Bounded detail queue** (`lighthouse.buffer`, 10000): when full, a trace
  goes before an occurrence; the hit (count) is always kept. Every dropped
  detail is counted as *sampled out*: `Lighthouse::sampled_out()`,
  `lighthouse_sampled_out_total` on `/metrics`, the overview's `sampled_out`
  (all processes, the period) and the dashboard's Sampled out figure.
- **Failed writes**: counters and hits go again with the next write; the
  batch's details are lost and counted as sampled out.
- **Several processes**: issue status, `regressed_at` and ignore counters
  change by compare-and-set, so one process raises a regression; counts
  are relative updates.
- **Proof**: `e2e/run.sh --scenarios checks,lighthouse --load-profile load
  --vus 200` fails the check "lighthouse: request count matches k6" when
  Lighthouse's count differs from k6's by more than 1 %.

Statuses: `resolve(id, false, comment)` reopens as a regression on the next
occurrence; `resolve(id, true, ..)` stores the current `lighthouse.release`
and reopens only in another release; `ignore(id, IgnoreFor::Forever |
Until(t) | Occurrences(n), ..)`; `reopen(id)`. All on `LighthouseStore`
(`lighthouse.store().unwrap()`) or through the API.

## Alerts

```toml
[lighthouse.alerts]
cooldown_minutes = 30                       # per issue and kind
spike = { occurrences = 50, minutes = 5 }

[lighthouse.alerts.email]
to = ["oncall@example.com"]                 # needs MailModule

[lighthouse.alerts.slack]
# webhook_url from APP_LIGHTHOUSE__ALERTS__SLACK__WEBHOOK_URL

[lighthouse.alerts.webhook]
url = "https://ops.example.com/hooks/lighthouse"
# secret (>= 32 bytes, required with url) from APP_LIGHTHOUSE__ALERTS__WEBHOOK__SECRET
on = ["new_issue", "regression", "spike", "failed_job", "long_wait"]
```

- Kinds: `new_issue`, `regression`, `spike`, `failed_job`, `long_wait`
  (raised by a queue monitor with `lighthouse.raise_alert(alert)`). Default
  `on`: new_issue, regression, spike. `Info` issues and ignored issues never
  alert.
- One `lighthouse_alerts` row per channel. With a `dyn Dispatcher` (a queue)
  each is a `lighthouse.alert` job (5 attempts, 10 s to 10 min backoff);
  without one it is sent once from the writer.
- Webhooks follow Standard Webhooks (`webhook-id`, `webhook-timestamp`,
  `webhook-signature: v1,<base64 HMAC-SHA256 of id.timestamp.body>`); a
  `whsec_` secret is base64-decoded. Verify with
  `navree::lighthouse::webhook_signature(key, id, timestamp, body)`.
- Your own channel: implement `AlertChannel` (`name`, `wants`, `send`) on an
  injectable and call `m.alert_channel::<C>()` in a module's register.
- `navree lighthouse:test-alerts [channel]` sends a test alert (`"test":
  true`) through each channel, stores nothing, exits non-zero on a failure.

## The JSON API (scripts and agents)

Bearer access tokens work on `/_lighthouse/api/*` (the dashboard's cookie
works too, with an `x-csrf-token` header on changes):

| Route | Ability |
|---|---|
| `GET /api/overview?period=1h\|24h\|7d` | view |
| `GET /api/issues?status=&kind=&level=&release=&environment=&route=&job=&tag=key:value&user=&workspace=&search=&sort=last_seen\|first_seen\|count&page=&per_page=` | view |
| `GET /api/issues/{id}`, `GET /api/issues/{id}/occurrences` | view |
| `POST /api/issues/{id}/resolve` `{in_next_release?, comment?}` | resolve |
| `POST /api/issues/{id}/ignore` `{}` or `{until}` or `{occurrences}`, `comment?` | resolve |
| `POST /api/issues/{id}/reopen` | resolve |
| `GET /api/routes`, `/api/routes/detail?endpoint=GET /api/v1/x`, `/api/queries`, `/api/jobs`, `/api/outgoing`, `/api/traces/{id}`, `/api/alerts` | view |

## MCP tools (agents)

With the facade features `lighthouse` and `mcp` both on (`mcp` is opt-in;
the facade then turns on navree-lighthouse's own `mcp` feature) and
`McpModule` in the app, `LighthouseModule` registers `LighthouseMcp` on the
app's MCP server at `/mcp`. Nothing to write:

```toml
navree = { git = "https://github.com/NavreeHQ/navree", features = ["mcp"] }
```

```rust
Navree::app()
    // ... DatabaseModule, AuthModule, AccessModule
    .module(LighthouseModule::from_config())
    .module(McpModule::from_config())
```

| Tool | Arguments | Ability |
|---|---|---|
| `lighthouse_list_issues` | `status` (`open` default, `resolved`, `resolved_in_next_release`, `ignored`, `any`), `kind`, `level`, `release`, `environment`, `route`, `job`, `tag` (`key:value`), `user`, `workspace`, `search`, `sort` (`last_seen`, `first_seen`, `count`), `page`, `per_page` (1..=100) | view |
| `lighthouse_get_issue` | `issue_id` | view |
| `lighthouse_list_occurrences` | `issue_id`, `page`, `per_page` | view |
| `lighthouse_get_trace` | `trace_id` (an occurrence's `sample_id`) | view |
| `lighthouse_get_overview` | `period` (`1h`, `24h` default, `7d`) | view |
| `lighthouse_list_routes` / `_queries` / `_jobs` / `_outgoing` | `period` | view |
| `lighthouse_get_route` | `endpoint` (`GET /api/v1/deals/{id}`), `period` | view |
| `lighthouse_list_alerts` | none | view |
| `lighthouse_resolve_issue` | `issue_id`, `in_next_release`, `comment` | resolve |
| `lighthouse_ignore_issue` | `issue_id`, `until` or `occurrences` (not both), `comment` | resolve |
| `lighthouse_reopen_issue` | `issue_id` | resolve |

Also the resource `lighthouse://issues/{id}` (`navree::lighthouse::mcp::ISSUE_RESOURCE`,
the issue's JSON) and the prompt `fix-lighthouse-issue`
(`FIX_ISSUE_PROMPT`, argument `issue_id`): the issue as a resource plus
steps (find the cause, failing test, fix, run tests, then
`lighthouse_resolve_issue` with `in_next_release: true` and a comment).
Results are the JSON API's bodies; a list is also `{"result": [..]}` in
`structuredContent`. Unknown ids are tool errors with a 404 body, invalid
arguments 400. `lighthouse.mcp = false` (`APP_LIGHTHOUSE__MCP=false`)
or `lighthouse.store = "off"` offers nothing.

Test with `navree::mcp::testing::McpTestClient`:

```rust
let mcp = McpTestClient::new(app.client()).bearer(token).initialize().await;
mcp.call_tool("lighthouse_resolve_issue",
        json!({ "issue_id": id, "in_next_release": true, "comment": "fixed in #812" }))
    .await
    .assert_ok()
    .assert_json_path("status", json!("resolved_in_next_release"));
```

## Tests

```rust
use navree::lighthouse::{Level, LighthouseFake};

let fake = LighthouseFake::new();
let app = TestApp::builder()
    .temp_database()
    .module(DatabaseModule::from_config())
    .module(LighthouseModule::from_config())
    .module(BillingModule)
    .override_value(Arc::new(fake.lighthouse()))
    .build()
    .await;
// ... exercise the code ...
fake.assert_reported::<InvoiceError>(|report| report.message.contains("not found"));
fake.assert_reported_times(1);
assert_eq!(fake.reports()[0].level, Level::Error);
```

- `assert_reported::<E>` matches the **type given to `report`** (its type
  name), not types deeper in the source chain; the predicate gets the
  `Reported` record (`message`, `chain`, `error_type`, `debug` (the error's
  `Debug`), `level`, `tags`, `context`, `user`, `route`, `request_id`,
  `panic`).
- The fake also records framework-captured exceptions (a handler answering
  500 shows up with its `route`), since the sink hands them to whichever
  `Lighthouse` the app holds.
- With the real store in tests: call `app.get::<Lighthouse>().await.flush().await`
  before reading `lighthouse.store()`; writes happen in the background.
- Without the fake and without interest in monitoring, set
  `lighthouse.enabled = false` (nothing recorded; the dashboard still reads).
- Alerts in tests: override `HttpClient` with `HttpClient::fake()` and stub
  the Slack/webhook URLs; with `queue.driver = "memory"` run
  `Queue::run_pending()` to deliver queued alerts.

## Checks

- `cargo test -p navree-lighthouse --features mcp` (the MCP tests are
  behind the crate's `mcp` feature), `cargo test -p navree --test
  lighthouse_docs` and `cargo test -p navree --features mcp --test
  lighthouse_mcp`.
- Several processes on PostgreSQL: `cargo test -p navree-lighthouse
  --features postgres --test postgres` with `NAVREE_TEST_POSTGRES_URL` set
  (skipped without it).
- After changing options: `LighthouseOptions::check()` names the key of every
  problem (sample rates 0..=1, `path` absolute, `prune_at` "HH:MM" or empty,
  `buffer` >= 100, `spike.minutes` 1..=60, webhook `secret` >= 32 bytes).
- Retention: `navree lighthouse:prune [--days N]`; with `ScheduleModule` a
  `lighthouse:prune` task runs daily at `prune_at` (UTC).

## Gotchas

- **Dashboard 404**: `lighthouse.dashboard = false`, `lighthouse.store =
  "off"`, or a different `lighthouse.path`. **Boot error about a route
  authorizer**: add `AccessModule` or turn the dashboard off.
- **Sign-in says it needs AuthModule and CryptoModule**: both are required
  for cookie sessions; the API still works with tokens.
- **Guest gets 303 to `/login`, user gets a 403 page**: missing
  `lighthouse.view`. API answers 401/403 JSON instead.
- **CSRF 403 on API calls from a browser session**: send `x-csrf-token`
  (the `data-csrf` attribute of the page's `<body>`).
- **No trace for a request**: `lighthouse.sample.requests` (0.1 by
  default) and the request was fast, OK and reported nothing. Counts stay
  exact. Set `1.0` in development to keep every trace.
- **"sampled out" banner / figure / `lighthouse_sampled_out_total`**: the
  database is not keeping up with the details (`lighthouse.buffer`); traces
  go first, counts are never dropped. Lower `sample.requests` or use
  `lighthouse.database_url`.
- **No `lighthouse_*` tools on `/mcp`**: the facade's `mcp` feature is
  off (it is not in `full`), `McpModule` is missing, `lighthouse.mcp =
  false`, `lighthouse.store = "off"`, or the user lacks `lighthouse.view`
  (lists show only what the user may call; calling a hidden tool answers
  `Unknown tool`). Resolve tools need `lighthouse.resolve`.
- **Separate database** (`lighthouse.database_url`): tables are created
  there at boot; the app database keeps empty ones from the migration.
- **Release**: `lighthouse.release` (deploys set `APP_LIGHTHOUSE__RELEASE`),
  else `app.version`, else `unknown`. "Resolved in next release" compares
  against it.
- **Traces in a tracing backend**: export is `ObserveModule`'s job
  (`observe.tracing`, facade feature `otel`); set
  `observe.tracing.trace_url` with `{trace_id}` and the dashboard links trace
  ids there.
