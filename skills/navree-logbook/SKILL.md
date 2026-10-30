---
name: navree-logbook
description: "Debug a Navree app locally with navree-logbook: `LogbookModule` and the `logbook` config section, the timeline UI at `/_logbook`, watchers (requests with headers and bodies, queries, jobs, mail, events, cache, logs, exceptions, outgoing HTTP, notifications, data changes, scheduled tasks), the filter language (`user:`, `tag:`, `status:4xx`, `route:`, `type:`, `slow`, `failed`, `duration>200`, `batch:`, `trace:`), `Logbook::tag` and `Logbook::dump`, duplicate-query (N+1) detection, request replay as the same user, `logbook:clear`, and the test API (`last_request().queries()`, `duplicate_queries()`). Use when you need to see what a request, job or scheduled task did, find an N+1 query, assert the number of queries in a test, replay a failing request, or when `/_logbook` answers 404 or records nothing."
---

# Logbook

Logbook records what the app does while you develop and shows it at
`/_logbook`. The module is `navree::logbook` (crate `navree-logbook`, facade
feature `logbook`, in `full`). It is **on in debug builds, off in release
builds**, and **off in `TestApp`** unless a test turns it on. Read
`crates/navree-logbook/src` and its tests in `crates/navree-logbook/tests/logbook/`
when in doubt.

## Steps: add it to an app

1. Register the module in the app root (an app from `navree new` already
   has it). It needs `HttpModule`; every other module it watches is optional:

```rust
Navree::app()
    .module(ConfigModule::for_root(sources))
    .module(HttpModule::from_config())
    // ... DatabaseModule, QueueModule, MailModule, EventsModule ...
    .module(LogbookModule::from_config()) // section `logbook`
```

2. Run `navree dev` and open `http://127.0.0.1:8080/_logbook` (the boot log
   prints the URL: `navree::logbook: recording to storage/logbook.db, open ...`).
3. Entries go to `storage/logbook.db` (its own SQLite file, never the app's
   database) and survive restarts. `navree logbook:clear` empties it. The
   file store needs the `sqlite` feature (`navree`'s, or `navree-logbook`'s);
   a PostgreSQL or MySQL build without it keeps entries in memory, and
   `logbook.store = "file"` stops the boot.

## Steps: find out what a request did

1. Filter the list: `route:/api/v1/deals/{id} status:5xx`, `user:<id prefix>`,
   `tag:deal:<id prefix>` (every request or job that changed that record),
   `type:query slow`, `duration>200`, `batch:<request id>`,
   `trace:<trace id>` (a request and the jobs it queued).
2. Open the request: tabs show the request and response (headers, bodies),
   queries (repeated statements first), events, jobs in its trace, mail,
   cache calls, logs, exceptions, outgoing calls, data changes, dumps and a
   timeline waterfall.
3. Fix the code, then **Replay** it from the Replay tab (or
   `POST /_logbook/api/requests/{id}/replay`). Unsafe methods need a
   confirmation (`confirm: true`, otherwise 409).

## Code templates

Tag the running batch and dump a value (both do nothing when Logbook is off):

```rust
use navree::logbook::Logbook;
use navree::prelude::*;

#[injectable]
pub struct ContactImport {
    logbook: Arc<Logbook>,
}

impl ContactImport {
    pub async fn run(&self, file: &ImportFile) -> Result<ImportReport, BoxError> {
        self.logbook.tag(format!("import:{}", file.id));
        let rows = parse_rows(file)?;
        self.logbook.dump("parsed rows", &rows[..rows.len().min(5)]);
        // ...
    }
}
```

Assert what a request did in a test:

```rust
let app = TestApp::builder()
    .temp_database()
    .config("logbook.enabled", true)
    .config("logbook.store", "memory")
    .module(DatabaseModule::from_config())
    .module(LogbookModule::from_config())
    .module(DealsModule)
    .build()
    .await;
app.acting_as(principal(user_id))
    .get("/api/v1/deals/board")
    .send()
    .await
    .assert_status(200);
let batch = app.get::<Logbook>().await.last_request().await.unwrap();
assert!(batch.queries().len() <= 3, "{:#?}", batch.queries());
assert!(batch.duplicate_queries().is_empty());
```

`Batch` also has `events()`, `jobs()`, `mail()`, `cache_calls()`, `logs()`,
`exceptions()`, `http_calls()`, `notifications()`, `models()`, `dumps()`,
`tags()`, `head()`, `request()` and `entries()`. `Logbook` has
`last_request()`, `last_job()`, `last_schedule()`, `batch(id)`,
`entries(&ListQuery::new(Filter::parse("type:query")?, 50))`, `clear()`,
`prune()`, `replay(id, ReplayRequest { confirm: true, ..Default::default() })`
and `curl(id)`. Outside an app, `Logbook::in_memory(options)`.

## Configuration

| Key | Default | Notes |
|---|---|---|
| `logbook.enabled` | debug: `true`, release: `false` | `true` in a release build forces it on, with a boot warning |
| `logbook.path` | `/_logbook` | |
| `logbook.store` / `logbook.file` | `file` / `storage/logbook.db` | `memory` for tests |
| `logbook.max_entries` / `logbook.keep_hours` | `10000` / `24` | pruned at boot and every 5 minutes, batches whole |
| `logbook.allow_remote` | `false` | then every route needs permission `logbook.view` and a `RouteAuthorizer` |
| `logbook.log_level` | `debug` | only lines that also pass `log.level` arrive |
| `logbook.slow_query_ms` / `logbook.duplicate_queries` | `100` / `5` | |
| `logbook.capture.{request_body,response_body,max_body_kb}` | `true`, `true`, `64` | |
| `logbook.replay` | `true` | |
| `logbook.ignore.routes` | `["/health/*", "/metrics"]` | comma-separated from the environment |
| `logbook.watchers.<name>` | `true` | `requests`, `queries`, `jobs`, `mail`, `events`, `cache`, `logs`, `exceptions`, `http_client`, `notifications`, `models`, `schedule` |

Environment: `APP_LOGBOOK__STORE=memory`, `APP_LOGBOOK__WATCHERS__CACHE=false`.

## Checks

- `cargo test -p navree-logbook` (stores, filters, watchers, UI, replay).
- In a test, `app.get::<Logbook>().await.is_enabled()` tells whether it runs.
- `GET /_logbook/api/entries?q=type:request` returns JSON for scripts.

## Gotchas

- **404 on `/_logbook`**: Logbook is off (release build, `TestApp` without
  `.config("logbook.enabled", true)`, or `logbook.enabled = false`), or the
  request did not come from a loopback address (`127.0.0.1`, `::1`). Behind
  a proxy on another machine, use `logbook.allow_remote = true`.
- **Boot fails with `allow_remote`**: the routes then declare the
  permission `logbook.view`, which needs a `RouteAuthorizer` (`AccessModule`).
- **No debug lines**: Logbook only sees log lines that pass `log.level`; set
  `log.level = "debug"` (or the target in `log.levels`).
- **Queries show placeholders, not values**: navree-db reports statements
  without bound values; duplicate detection compares the statements.
- **Bodies**: `Authorization`, `Cookie` and `Set-Cookie` are never stored;
  fields named like `log.redact` fragments are `[redacted]`; bodies over
  `max_body_kb` are cut, binary and streamed bodies are not stored.
- **Jobs**: a job attempt is a batch keyed by the job id; it continues the
  dispatching request's trace, which is how the two are linked. Job
  payloads are not recorded, so jobs cannot be dispatched again from
  Logbook.
- **Replay as the same user** needs `AuthModule` (Logbook signs a 5-minute
  access token); without it the replay goes out as a guest.
