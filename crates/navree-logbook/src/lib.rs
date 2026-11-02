//! # navree-logbook
//!
//! Logbook shows what your app did, while you develop. It records every
//! request with everything that happened during it (the queries it ran,
//! the events it emitted, the mail it sent, the cache calls it made, the
//! log lines it wrote, the jobs it queued) and shows them as one timeline
//! at `/_logbook`, served by the app.
//!
//! - [`LogbookModule`]: global, configured from the `logbook` section
//!   ([`LogbookOptions`]). On by default in debug builds, off in release
//!   builds unless `logbook.enabled = true` forces it (with a warning at
//!   every boot). Off, it adds no route, no layer and no watcher.
//! - **Watchers** turn the app's telemetry (navree-telemetry) and data
//!   changes (navree-db) into [`Entry`]s: requests, queries, jobs, mail,
//!   events, cache calls, log lines, exceptions, outgoing HTTP calls,
//!   notifications, data changes and scheduled tasks. Entries recorded
//!   while a request, job or scheduled task ran belong to its [`Batch`].
//! - **Stores**: a SQLite file of its own (`SqliteStore`, the default with the
//!   `sqlite` feature)
//!   or memory ([`MemoryStore`]), bounded by `logbook.max_entries` and
//!   `logbook.keep_hours`; batches are pruned whole.
//! - **The UI** at `logbook.path`: the list with [filters](Filter), a
//!   request's detail with its queries, events, jobs, mail, logs and a
//!   waterfall, and [replay](Logbook::replay). Loopback clients only,
//!   unless `logbook.allow_remote` (then the permission `logbook.view`).
//! - **The service** [`Logbook`]: [`tag`](Logbook::tag) and
//!   [`dump`](Logbook::dump) from your code; in tests,
//!   [`last_request`](Logbook::last_request) and friends.
//!
//! ```
//! use navree_logbook::{Logbook, LogbookOptions};
//!
//! # #[tokio::main(flavor = "current_thread")]
//! # async fn main() {
//! let logbook = Logbook::in_memory(LogbookOptions::default());
//! logbook.dump("answer", &42);
//! assert_eq!(logbook.count().await.unwrap(), 1);
//! # }
//! ```
#![forbid(unsafe_code)]

mod batch;
mod capture;
mod entry;
mod error;
mod filter;
mod framework;
mod logbook;
mod module;
mod options;
mod record;
mod redact;
mod replay;
mod store;
mod ui;

pub use batch::{Batch, DuplicateQuery};
pub use entry::{
    BatchSummary, CacheEntry, CapturedBody, Content, DumpEntry, Entry, EntryKind, EventEntry,
    ExceptionEntry, HttpClientEntry, HttpMessage, JobEntry, JobOutcome, LogEntry, MailEntry,
    ModelEntry, NotificationEntry, QueryEntry, RequestEntry, ScheduleEntry,
};
pub use error::LogbookError;
pub use filter::{Comparison, Filter, FilterError, StatusMatch, Term};
pub use logbook::Logbook;
pub use module::{LogbookModule, PRUNE_EVERY};
pub use options::{Activation, CaptureOptions, IgnoreOptions, LogbookOptions, StoreKind, Watchers};
pub use redact::REDACTED;
pub use replay::{REPLAY_TOKEN_TTL, ReplayRequest, Replayed, is_unsafe};
#[cfg(feature = "sqlite")]
pub use store::SqliteStore;
pub use store::{ListQuery, LogbookStore, MemoryStore};
pub use ui::VIEW_PERMISSION;
