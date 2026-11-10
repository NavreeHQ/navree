//! # navree-telemetry
//!
//! Typed telemetry events for the whole framework, delivered to the sinks
//! an app registers, plus W3C Trace Context.
//!
//! - **Events** ([`TelemetryEvent`]): [`RequestHandled`] (navree-http),
//!   [`QueryExecuted`] (navree-db), [`JobProcessed`] and [`JobFailed`]
//!   (navree-queue), [`ScheduledTaskRan`] (navree-schedule), [`MailSent`]
//!   (navree-mail), [`NotificationSent`] (navree-notify), [`CacheEvent`]
//!   (navree-cache), [`HttpClientRequest`] (navree-http-client),
//!   [`ExceptionReported`] (navree-http, or your code), [`LogRecord`]
//!   (navree-log) and [`EventEmitted`] (navree-events); and the platform
//!   modules' [`AiCall`] (navree-ai), [`BroadcastSent`] (navree-realtime),
//!   [`StorageOp`] (navree-storage), [`WebhookDelivery`] (navree-webhooks),
//!   [`SearchQueried`] (navree-search), [`SignInEvent`] (navree-auth) and
//!   [`UiOperation`] (navree-ui). A sink receives each one as a
//!   [`TelemetryRecord`]: the event, when it was emitted and the
//!   [`EventContext`] (request id, job id, trace, user, workspace) it
//!   happened in.
//! - **Sinks** ([`TelemetrySink`]) are `dyn TelemetrySink` multi-bindings,
//!   registered with [`ModuleBuilderTelemetryExt`]; [`TelemetryModule`]
//!   collects them into the app's [`Telemetry`] registry when the app boots.
//!   Each sink states its [`Interest`] in event kinds.
//! - **No sink, no cost.** Emitters resolve the registry once, when they
//!   are built ([`Telemetry::from_app`]). Without a sink there is no
//!   registry, and every emit is one branch on a `None`; with sinks,
//!   [`Telemetry::enabled`] is one atomic load, and events no sink wants
//!   are never built.
//! - **Context** ([`TelemetryContext`]): the request or job the current task
//!   works for, set by navree-http and navree-queue, with its
//!   [`TraceContext`]: the W3C `traceparent` / `tracestate` that navree-http
//!   reads from incoming requests and navree-http-client sends with
//!   outgoing ones.
//! - **Tests** use [`TelemetryRecorder`], an in-memory sink.
//!
//! ```
//! use navree_kernel::Navree;
//! use navree_telemetry::{Telemetry, TelemetryRecorder};
//!
//! # #[tokio::main(flavor = "current_thread")]
//! # async fn main() -> Result<(), navree_kernel::BoxError> {
//! let recorder = TelemetryRecorder::new();
//! let app = Navree::app().module(recorder.module()).boot().await?;
//! let telemetry = Telemetry::from_app(app.ctx()).await;
//! assert!(telemetry.is_active());
//! telemetry.report_exception(&std::io::Error::other("disk full"));
//! assert_eq!(recorder.exceptions()[0].message, "disk full");
//! app.shutdown().await?;
//! # Ok(())
//! # }
//! ```
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod context;
mod event;
mod module;
mod platform;
mod recorder;
mod registry;
mod sink;
mod trace;

pub use context::{TelemetryContext, capture_panic_locations, current_trace_id};
pub use event::{
    CacheEvent, CacheOperation, EventContext, EventEmitted, EventKind, ExceptionReported,
    HttpClientRequest, JobFailed, JobProcessed, LogLevel, LogRecord, MailSent, NotificationSent,
    QueryExecuted, RequestHandled, RequestInfo, ScheduledTaskRan, TelemetryEvent, TelemetryRecord,
};
pub use module::{ModuleBuilderTelemetryExt, TelemetryModule};
pub use platform::{
    AiCall, BroadcastSent, Outcome, SearchMode, SearchQueried, SignInEvent, SignInMethod,
    StorageOp, StorageOperation, UiOperation, UiOperationKind, WebhookDelivery,
};
pub use recorder::TelemetryRecorder;
pub use registry::Telemetry;
pub use sink::{Interest, TelemetrySink};
pub use trace::{
    InvalidId, SAMPLE_RATIO_ENV, SpanId, SpanLink, TraceContext, TraceId, TracePolicy,
    TracePolicySection, sample_trace,
};
