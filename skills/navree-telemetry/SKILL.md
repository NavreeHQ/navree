---
name: navree-telemetry
description: "Telemetry events and tracing with navree-telemetry and navree-observe: the typed events every module emits (`RequestHandled`, `QueryExecuted`, `JobProcessed`, `JobFailed`, `ScheduledTaskRan`, `MailSent`, `NotificationSent`, `CacheEvent`, `HttpClientRequest`, `ExceptionReported`, `LogRecord`), sinks (`TelemetrySink`, `Interest`, `m.telemetry_sink::<S>()`), the zero-cost rule, `Telemetry::current().report_exception`, `TelemetryContext` (request and job context, `propagate` for spawned tasks), W3C `traceparent` in and out, the `observe.tracing` section, OTLP export behind the opt-in `otel` feature, `current_trace_id`, and `TelemetryRecorder` / `SpanRecorder` in tests. Use when receiving requests, queries, jobs, mail or log lines in your own code (metrics, audit, a monitoring tool), exporting traces to an OpenTelemetry backend, reporting a handled error, keeping a trace across `tokio::spawn`, asserting on emitted events or spans in tests, or debugging a missing event, span or trace id."
---

# Telemetry events and tracing

`navree::telemetry` (crate `navree-telemetry`, facade feature `telemetry`, in
`full`; every emitting module turns it on) defines the typed events Navree's
modules emit and the sinks that receive them. `navree::observe` (crate
`navree-observe`) turns the events into OpenTelemetry spans and exports them
over OTLP when the opt-in `otel` feature is on. When in doubt read
`crates/navree-telemetry/src`, its tests in `crates/navree-telemetry/tests/`,
each emitter's `tests/<crate>/telemetry.rs`, and `crates/navree/tests/telemetry.rs`
(the docs examples through the facade).

Docs: `/docs/digging-deeper/health-and-metrics/#telemetry-events` (events,
sinks, `TelemetryRecorder`) and `/docs/microservices/tracing/` (spans,
propagation, export, `SpanRecorder`).

## The events

| Event (`TelemetryEvent::*`) | `EventKind` | Emitted by | When |
|---|---|---|---|
| `RequestHandled` | `RequestHandled` | navree-http | the response is ready (`method`, `route` template or `None`, `path`, `status`, `duration`, `user_id`, `workspace_id`, `request_id`, `client_ip`) |
| `QueryExecuted` | `QueryExecuted` | navree-db | a statement ran (`sql` with placeholders, `duration` it ran, `pool_wait` before that, `rows` when known, `connection` without password, `failed`) |
| `JobProcessed` / `JobFailed` | `JobProcessed` / `JobFailed` | navree-queue workers | an attempt ended (`queue`, `job_type`, `job_id`, `attempt`, `duration`; failed adds `error`, `will_retry`) |
| `ScheduledTaskRan` | `ScheduledTaskRan` | navree-schedule | a task ran (`task`, `tick`, `duration`, `error`) |
| `MailSent` | `MailSent` | navree-mail | the transport accepted it (recipients, subject, bodies, `transport`, `duration`) |
| `NotificationSent` | `NotificationSent` | navree-notify | one channel delivered (`notification_id`, `notification_type`, `notifiable_type`, `notifiable_id`, `channel`) |
| `Cache(CacheEvent)` | `Cache` | navree-cache | `CacheOperation::Hit`, `Miss`, `Write`, `Forget` (`key` without prefix, `store`) |
| `HttpClientRequest` | `HttpClientRequest` | navree-http-client | a call finished after retries (`url` without query or credentials, `status`, `attempts`, `error`, `span_id`) |
| `ExceptionReported` | `ExceptionReported` | navree-http (500s, 5xx with a source, panics) or your code | `message`, `chain`, `location`, `panic`, `request` |
| `Log(LogRecord)` | `Log` | navree-log | a line passed the level filter (`level`, `target`, `message`, redacted `fields`) |

A sink receives a `TelemetryRecord { at, context, event }`. `context` is an
`EventContext` (`request_id`, `job_id`, `trace`, `user_id`, `workspace_id`)
captured from the current `TelemetryContext`. Records serialize with a
`type` tag and durations in milliseconds.

## Steps: receive events in your code

1. Implement `TelemetrySink`. Declare the kinds you want in `interest`
   (default: all); `record` gets each record:

```rust
use navree::prelude::*;
use navree::telemetry::{EventKind, Interest, TelemetryEvent, TelemetryRecord, TelemetrySink};

/// Counts failed job attempts by queue and job type, on `/metrics`.
#[injectable]
pub struct JobFailureMetrics;

impl TelemetrySink for JobFailureMetrics {
    fn interest(&self) -> Interest {
        Interest::only([EventKind::JobFailed])
    }

    fn record(&self, record: &TelemetryRecord) {
        if let TelemetryEvent::JobFailed(job) = &record.event {
            metrics::counter!(
                "job_failures_total",
                "queue" => job.queue.clone(),
                "job" => job.job_type.clone(),
            )
            .increment(1);
        }
    }
}
```

2. Register it in a module. `m.telemetry_sink::<S>()` (container-built,
   `S: Injectable`), `m.telemetry_sink_value(sink)` (already built) or
   `m.telemetry_sink_factory(Factory::<dyn TelemetrySink>::new(...))`.
   Each imports `TelemetryModule` (global) for you; the trait comes with
   `navree::prelude::*`:

```rust
#[module(register = Self::wire)]
pub struct MonitoringModule;

impl MonitoringModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.telemetry_sink::<JobFailureMetrics>();
    }
}
```

3. If the sink does real work (writes to a database, sends over the
   network), hand records to a channel and do the work in a background
   task. `record` runs on the emitting task, inside a request, a query or
   a log call:

```rust
use tokio::sync::mpsc;

/// Hands every record but log lines to a background task; never blocks.
pub struct ChannelSink(mpsc::Sender<TelemetryRecord>);

impl TelemetrySink for ChannelSink {
    fn interest(&self) -> Interest {
        Interest::ALL.without(EventKind::Log)
    }

    fn record(&self, record: &TelemetryRecord) {
        // A full buffer drops the record rather than slow the request.
        let _ = self.0.try_send(record.clone());
    }
}

// in a module's register:
m.telemetry_sink_factory(Factory::<dyn TelemetrySink>::new(|_r: Resolver| async move {
    let (tx, mut rx) = mpsc::channel::<TelemetryRecord>(1024);
    tokio::spawn(async move {
        while let Some(record) = rx.recv().await {
            // store or send the record
        }
    });
    Ok(Arc::new(ChannelSink(tx)) as Arc<dyn TelemetrySink>)
}));
```

## Steps: report a handled error

Inside a request, job or scheduled task, `Telemetry::current()` is the app's
registry. `report_exception` records the error, its source chain and the
caller's `file:line:column`:

```rust
use navree::telemetry::Telemetry;

if let Err(error) = sync_calendar(&user).await {
    Telemetry::current().report_exception(&error);
}
```

Outside one (a provider, a background loop), resolve once with
`Telemetry::from_app(ctx).await` (or `Telemetry::from_app(&r.app()).await`
in a factory) and keep the handle. Both are disabled when no sink exists.

## Steps: export traces

1. Turn on the opt-in feature (it adds the OpenTelemetry SDK, tonic and
   prost): `navree = { ..., features = ["otel"] }`. It turns on `observe`.
2. Add `ObserveModule::from_config()` and point `observe.tracing.endpoint`
   at an OTLP collector (`http://127.0.0.1:4317`; `protocol = "http"` for
   OTLP/HTTP on 4318). Other keys: `enabled`, `headers`, `service_name`,
   `resource`, `sample_ratio`, `trust_incoming`, `stdout`, `timeout_ms`,
   `max_queue_size`, `trace_url`. `OTEL_EXPORTER_OTLP_ENDPOINT`,
   `OTEL_EXPORTER_OTLP_PROTOCOL`, `OTEL_EXPORTER_OTLP_HEADERS`,
   `OTEL_RESOURCE_ATTRIBUTES`, `OTEL_SERVICE_NAME` and
   `OTEL_TRACES_SAMPLER_ARG` fill keys the section doesn't set.
3. Without a backend, `observe.tracing.stdout = true` prints finished spans
   as JSON lines (works without `otel`).
4. Inject `Arc<TracingOptions>` (from `navree::observe`) to read the
   resolved section; `trace_link(&trace_id)` builds a `trace_url` link.

Propagation needs no setup: navree-http reads `traceparent`/`tracestate` on
every request (`observe.tracing.enabled`, `trust_incoming` and
`sample_ratio` apply without `ObserveModule`), `HttpClient` sends a child
`traceparent` inside a request or job (a caller-set `traceparent` header is
kept), and queued jobs store the dispatching span in `jobs.trace_parent`
(queue migration `m20261026_000001_add_jobs_trace_parent`).

## Steps: keep the context across `tokio::spawn`

`TelemetryContext` is task-local. Wrap spawned work so its queries, calls
and log lines stay in the request's trace and carry its request id:

```rust
use navree::telemetry::TelemetryContext;

tokio::spawn(TelemetryContext::propagate(async move {
    refresh_forecast(&db, deal_id).await
}));
```

`navree::telemetry::current_trace_id()` (also `navree::observe::current_trace_id`)
gives the id to show users; `TelemetryContext::current()` gives the whole
context.

## Steps: test

Events, with `TelemetryRecorder` (from `navree::telemetry`):

```rust
use http::Method;
use navree::http_client::{FakeResponse, HttpClient};
use navree::prelude::*;
use navree::telemetry::{EventKind, TelemetryRecorder};
use navree::testing::TestApp;

#[tokio::test]
async fn the_rates_call_happens_inside_the_request() {
    let recorder = TelemetryRecorder::new();
    let (client, server) = HttpClient::fake();
    server.on(Method::GET, "https://rates.example.com/*", FakeResponse::new(200));
    let app = TestApp::builder()
        .module(HttpClientModule::from_config())
        .module(RatesModule)
        .module(recorder.module())
        .override_value(Arc::new(client))
        .build()
        .await;

    app.client().get("/api/v1/rates/eur").send().await.assert_status(200);

    let request = &recorder.requests()[0];
    assert_eq!(request.route.as_deref(), Some("/api/v1/rates/{currency}"));
    let record = &recorder.of_kind(EventKind::HttpClientRequest)[0];
    assert_eq!(record.context.request_id.as_deref(), Some(request.request_id.as_str()));

    app.shutdown().await.unwrap();
}
```

Accessors: `requests`, `queries`, `jobs_processed`, `jobs_failed`,
`scheduled_tasks`, `mails`, `notifications`, `cache_events`,
`http_client_requests`, `exceptions`, `logs`, plus `records`, `events`,
`of_kind`, `len`, `is_empty`, `clear`. For background work:
`recorder.wait_for(EventKind::JobProcessed, Duration::from_secs(5)).await`.
`TelemetryRecorder::only([...])` records only some kinds.

Spans, with `SpanRecorder` (from `navree::observe::testing`): it is a
telemetry sink too, so add `spans.module()` (no endpoint, nothing is
exported), then `spans.find(name)` (fails listing the names it has),
`find_all`, `all`, `clear`; each `RecordedSpan` has `name`, `kind`
(`SpanKind`), `trace_id`, `span_id`, `parent_span_id`, `start`, `end`,
`status` (`SpanStatus::Unset` or `SpanStatus::Error { message }`),
`attributes` (JSON values by name) and `links`. It sees sampled traces only.

## Emitting from a new module (Lighthouse, Logbook, Fleet and friends)

- Add a new event kind only in navree-telemetry (`EventKind`,
  `TelemetryEvent`, `EventKind::ALL`, `as_str`, a `TelemetryRecorder`
  accessor), with a test.
- In the emitter's factory: `let telemetry = Telemetry::from_app(&r.app()).await;`
  keep it in the service, then
  `telemetry.emit_with(EventKind::X, || TelemetryEvent::X(...))` so nothing
  is built when no sink wants the kind.
- A new unit of work (a consumer loop, a command) runs inside
  `TelemetryContext::new(telemetry).with_job_id(id).with_trace(trace).scope(fut)`;
  start its trace with `TracePolicy::start` (incoming headers) or
  `TracePolicy::resume` (a `traceparent` your own app recorded).
- Respect the layer rule: navree-telemetry is L1 (kernel and contracts
  only); emitters list it in their `HIERARCHY` row in
  `crates/navree/tests/dependency_graph.rs`.

## Checks

```sh
cargo test -p navree-telemetry
cargo test -p navree --test telemetry
cargo test -p navree-observe --features otel   # OTLP over HTTP and gRPC, in-process receivers
```

In an app: with a sink registered, `Telemetry::from_app(app.ctx()).await.enabled(EventKind::X)`
is `true` for the kinds it wants; without one, `is_disabled()` is `true`.

## Gotchas

- **No sink, no events, no cost.** Without a sink there is no registry and
  every emit is one branch; that is why `Telemetry::current()` reports
  nothing in an app with no sink. Register `TelemetryRecorder` in tests.
- **Interest is read once, at boot.** A sink can't change its kinds later.
  Sinks receive records only after every provider is built (`on_init`) and
  stop at shutdown: events emitted while the app boots are dropped.
- **Never block in `record`**, and never `.await`: it runs on the emitting
  task. Use `try_send` to a bounded channel.
- **Log feedback loops.** A record emitted while a sink on the same thread
  is receiving one is dropped, so a sink that logs inside `record` can't
  loop. A background task that logs once per record *can*: exclude
  `EventKind::Log` from its interest (as `ChannelSink` does) or don't log
  per record.
- **Spawned tasks lose the context** (task-local): wrap with
  `TelemetryContext::propagate`, or their events have no request id or
  trace and their outgoing calls start no child span.
- **Hand-built services report nothing** unless you give them the registry:
  `Cache::with_telemetry(telemetry, "memory")`, `Mailer::with_telemetry(telemetry)`,
  `HttpClient::with_telemetry(telemetry)`. An `HttpClient` without one
  reports to the current request's or job's registry.
- **`QueryExecuted::rows` is `None`** for `BEGIN`, savepoints, `COMMIT` and
  `ROLLBACK`, which return no rows; statements report the rows they read
  or changed.
- **`MailSent` carries the rendered bodies.** A sink that stores records
  stores mail contents; drop `text` and `html` if that's not wanted.
- **`endpoint` without the `otel` feature fails the boot**, naming the
  feature; `OTEL_EXPORTER_OTLP_ENDPOINT` without it only warns.
- **`trust_incoming = false` on public edges**: each request starts its own
  trace and keeps the caller's span as a link.
