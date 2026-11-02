//! # navree-lighthouse
//!
//! `LighthouseModule`: application monitoring inside your app. It records
//! exceptions, slow requests and queries, failed jobs, failed scheduled
//! tasks and failing outgoing calls, groups them into **issues** by
//! fingerprint, keeps per-minute aggregates and sampled traces in the app's
//! database, sends alerts, and serves a dashboard and a JSON API at
//! `lighthouse.path` (`/_lighthouse`).
//!
//! - **Recording.** The module registers a telemetry sink
//!   (navree-telemetry): every request, query, job attempt, scheduled run,
//!   mail, cache call, outgoing call and exception the framework reports.
//!   The sink and [`Lighthouse::report`] add the record to in-memory
//!   counters on the calling thread (sharded locks, no I/O) and group each
//!   request's or job's events; when a request or job ends, what is worth
//!   keeping goes into a bounded queue. One background task writes both
//!   in batches (every second, or at 500 details).
//! - **Issues** ([`Issue`]) group occurrences by fingerprint ([`IssueKind`]
//!   lists the kinds); statuses ([`IssueStatus`]) are open, resolved,
//!   resolved in the next release and ignored, and a resolved issue that
//!   happens again reopens as a regression.
//! - **Counts are exact under any load; details are sampled**
//!   (`lighthouse.sample.*`, one ordinary trace in ten by default):
//!   per-minute histograms of every request give the percentiles, and
//!   errors, failed jobs, slow requests and the first 100 occurrences of
//!   an issue per hour are always kept. A full queue drops traces first and
//!   never a count; every dropped detail is counted as sampled out
//!   ([`Lighthouse::sampled_out`], shown on the dashboard).
//! - **Alerts** ([`AlertKind`]): email (navree-mail), Slack, signed
//!   webhooks (Standard Webhooks) and your own [`AlertChannel`]s, sent as
//!   `lighthouse.alert` jobs when the app has a queue, with a cooldown per
//!   issue and kind.
//! - **The dashboard** is server-rendered (navree-view templates compiled
//!   into the crate, its own small stylesheet and script) and protected by
//!   the `view` and `resolve` abilities on the subject `Lighthouse`
//!   (permissions `lighthouse.view` and `lighthouse.resolve`). Its JSON API
//!   ([`LighthouseApiController`]) serves the same data to scripts and
//!   agents with an access token.
//! - **MCP** (feature `mcp`): with navree-mcp's `McpModule` in the app,
//!   `LighthouseMcp` offers the same reads and status changes as tools
//!   (`lighthouse_list_issues`, `lighthouse_get_issue`,
//!   `lighthouse_resolve_issue`, ...), each issue as the resource
//!   `lighthouse://issues/{id}` and the prompt `fix-lighthouse-issue`,
//!   behind the same abilities (`lighthouse.mcp = false` turns them off).
//! - **Commands**: `lighthouse:prune [--days N]` and
//!   `lighthouse:test-alerts [channel]`; the scheduler prunes daily at
//!   `lighthouse.prune_at`.
//! - **Tests** use [`LighthouseFake`].
//!
//! ```
//! use navree_lighthouse::{Level, LighthouseFake, Report};
//!
//! let fake = LighthouseFake::new();
//! let lighthouse = fake.lighthouse();
//! let error = std::io::Error::other("card declined");
//! lighthouse.capture(
//!     Report::error(&error)
//!         .level(Level::Warning)
//!         .tag("provider", "acme-pay")
//!         .fingerprint(["acme-pay", "card-declined"]),
//! );
//! assert_eq!(fake.reports()[0].tags["provider"], "acme-pay");
//! ```
#![forbid(unsafe_code)]

pub mod entity;
#[cfg(feature = "mcp")]
pub mod mcp;

mod aggregate;
mod alert;
mod api;
mod chart;
mod commands;
mod config_api;
mod config_pages;
mod config_store;
mod dashboard;
mod dispatch;
mod engine;
mod fake;
mod histogram;
mod http;
mod migration;
mod model;
mod module;
mod normalize;
mod options;
mod overview;
mod redact;
mod release;
mod report;
mod service;
mod session;
mod settings;
mod settings_view;
mod store;

pub use aggregate::SlowCall;
pub use alert::{
    Alert, AlertChannel, AlertKind, DeliverAlert, ModuleBuilderLighthouseExt, webhook_signature,
};
pub use api::{
    IgnoreInput, LighthouseApiController, PageQuery, PeriodQuery, ResolveInput, RouteQuery,
};
pub use config_api::LighthouseConfigController;
pub use config_store::{AlertCounts, AlertHistoryRow, StoredSetting};
pub use dashboard::{
    CONTENT_SECURITY_POLICY, DASHBOARD_CSS, DASHBOARD_JS, LighthouseDashboardController,
};
pub use dispatch::ChannelResult;
pub use engine::Span;
pub use fake::{LighthouseFake, Reported};
pub use histogram::Histogram;
#[cfg(feature = "mcp")]
pub use mcp::LighthouseMcp;
pub use migration::LIGHTHOUSE_MIGRATIONS;
pub use model::{Issue, IssueKind, IssueStatus, Level};
pub use module::{CONFIGURE, LighthouseModule, LighthouseSubject, RESOLVE, VIEW};
pub use normalize::{normalize_message, normalize_sql, path_matches};
pub use options::{
    AlertOptions, CaptureOptions, DEFAULT_ALERT_KINDS, EmailAlertOptions, IgnoreOptions,
    LighthouseOptions, Period, SampleRates, SlackAlertOptions, SpikeOptions, StoreKind, Thresholds,
    WebhookAlertOptions,
};
pub use overview::OverviewSeries;
pub use redact::REDACTED;
pub use release::{ReleaseMarker, ReleaseReport, ReleaseSummary};
pub use report::{AsError, Report};
pub use service::Lighthouse;
pub use settings::ChannelState;
pub use store::{
    AlertRecord, FeedItem, HostStats, IgnoreFor, IssueDetail, IssueFilter, IssueSort, IssueSummary,
    JobStats, LighthouseError, LighthouseStore, ListPage, Occurrence, Origin, Overview, PageMeta,
    PruneSummary, QueryStats, RouteDetail, RouteStats, Sample, SampleSummary, TimelinePoint,
};
