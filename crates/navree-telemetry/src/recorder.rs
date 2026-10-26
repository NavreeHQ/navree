//! [`TelemetryRecorder`]: an in-memory sink for tests.

use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use navree_kernel::{Module, ModuleBuilder};
use tokio::sync::Notify;

use crate::{
    CacheEvent, EventKind, ExceptionReported, HttpClientRequest, Interest, JobFailed, JobProcessed,
    LogRecord, MailSent, ModuleBuilderTelemetryExt, NotificationSent, QueryExecuted,
    RequestHandled, ScheduledTaskRan, TelemetryEvent, TelemetryRecord, TelemetrySink,
};

/// A sink that keeps every record in memory, for assertions in tests.
/// Clones share the same records.
///
/// ```
/// use navree_kernel::Navree;
/// use navree_telemetry::TelemetryRecorder;
///
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() -> Result<(), navree_kernel::BoxError> {
/// let recorder = TelemetryRecorder::new();
/// let app = Navree::app().module(recorder.module()).boot().await?;
/// // ... exercise the app ...
/// assert!(recorder.requests().is_empty());
/// app.shutdown().await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct TelemetryRecorder {
    inner: Arc<Inner>,
}

struct Inner {
    records: Mutex<Vec<TelemetryRecord>>,
    interest: Interest,
    arrived: Notify,
}

impl Default for TelemetryRecorder {
    fn default() -> Self {
        Self::new()
    }
}

impl TelemetryRecorder {
    /// A recorder that wants every kind of event.
    pub fn new() -> Self {
        Self::with_interest(Interest::ALL)
    }

    /// A recorder that wants only `kinds` (so emitters skip the others).
    pub fn only(kinds: impl IntoIterator<Item = EventKind>) -> Self {
        Self::with_interest(Interest::only(kinds))
    }

    fn with_interest(interest: Interest) -> Self {
        Self {
            inner: Arc::new(Inner {
                records: Mutex::new(Vec::new()),
                interest,
                arrived: Notify::new(),
            }),
        }
    }

    /// A module that registers this recorder as a sink: add it to the app
    /// (`TestApp::builder().module(recorder.module())`).
    pub fn module(&self) -> impl Module {
        RecorderModule(self.clone())
    }

    fn lock(&self) -> MutexGuard<'_, Vec<TelemetryRecord>> {
        self.inner
            .records
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Every record, in the order they were emitted.
    pub fn records(&self) -> Vec<TelemetryRecord> {
        self.lock().clone()
    }

    /// Every event, in order.
    pub fn events(&self) -> Vec<TelemetryEvent> {
        self.lock().iter().map(|r| r.event.clone()).collect()
    }

    /// The records of one kind.
    pub fn of_kind(&self, kind: EventKind) -> Vec<TelemetryRecord> {
        self.lock()
            .iter()
            .filter(|r| r.event.kind() == kind)
            .cloned()
            .collect()
    }

    /// How many records there are.
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    /// Whether nothing was recorded.
    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    /// Forgets every record.
    pub fn clear(&self) {
        self.lock().clear();
    }

    /// Waits until a record of `kind` exists (it may already), at most
    /// `timeout`; whether one arrived. For events emitted by background
    /// work (a job, an after-commit hook).
    pub async fn wait_for(&self, kind: EventKind, timeout: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let arrived = self.inner.arrived.notified();
            tokio::pin!(arrived);
            arrived.as_mut().enable();
            if self.lock().iter().any(|r| r.event.kind() == kind) {
                return true;
            }
            if tokio::time::timeout_at(deadline, arrived).await.is_err() {
                return self.lock().iter().any(|r| r.event.kind() == kind);
            }
        }
    }

    fn collect<T: Clone>(&self, pick: impl Fn(&TelemetryEvent) -> Option<&T>) -> Vec<T> {
        self.lock()
            .iter()
            .filter_map(|r| pick(&r.event).cloned())
            .collect()
    }

    /// The [`RequestHandled`] events.
    pub fn requests(&self) -> Vec<RequestHandled> {
        self.collect(|e| match e {
            TelemetryEvent::RequestHandled(x) => Some(x),
            _ => None,
        })
    }

    /// The [`QueryExecuted`] events.
    pub fn queries(&self) -> Vec<QueryExecuted> {
        self.collect(|e| match e {
            TelemetryEvent::QueryExecuted(x) => Some(x),
            _ => None,
        })
    }

    /// The [`JobProcessed`] events.
    pub fn jobs_processed(&self) -> Vec<JobProcessed> {
        self.collect(|e| match e {
            TelemetryEvent::JobProcessed(x) => Some(x),
            _ => None,
        })
    }

    /// The [`JobFailed`] events.
    pub fn jobs_failed(&self) -> Vec<JobFailed> {
        self.collect(|e| match e {
            TelemetryEvent::JobFailed(x) => Some(x),
            _ => None,
        })
    }

    /// The [`ScheduledTaskRan`] events.
    pub fn scheduled_tasks(&self) -> Vec<ScheduledTaskRan> {
        self.collect(|e| match e {
            TelemetryEvent::ScheduledTaskRan(x) => Some(x),
            _ => None,
        })
    }

    /// The [`MailSent`] events.
    pub fn mails(&self) -> Vec<MailSent> {
        self.collect(|e| match e {
            TelemetryEvent::MailSent(x) => Some(x),
            _ => None,
        })
    }

    /// The [`NotificationSent`] events.
    pub fn notifications(&self) -> Vec<NotificationSent> {
        self.collect(|e| match e {
            TelemetryEvent::NotificationSent(x) => Some(x),
            _ => None,
        })
    }

    /// The [`CacheEvent`]s.
    pub fn cache_events(&self) -> Vec<CacheEvent> {
        self.collect(|e| match e {
            TelemetryEvent::Cache(x) => Some(x),
            _ => None,
        })
    }

    /// The [`HttpClientRequest`] events.
    pub fn http_client_requests(&self) -> Vec<HttpClientRequest> {
        self.collect(|e| match e {
            TelemetryEvent::HttpClientRequest(x) => Some(x),
            _ => None,
        })
    }

    /// The [`ExceptionReported`] events.
    pub fn exceptions(&self) -> Vec<ExceptionReported> {
        self.collect(|e| match e {
            TelemetryEvent::ExceptionReported(x) => Some(x),
            _ => None,
        })
    }

    /// The [`LogRecord`]s.
    pub fn logs(&self) -> Vec<LogRecord> {
        self.collect(|e| match e {
            TelemetryEvent::Log(x) => Some(x),
            _ => None,
        })
    }
}

impl TelemetrySink for TelemetryRecorder {
    fn interest(&self) -> Interest {
        self.inner.interest
    }

    fn record(&self, record: &TelemetryRecord) {
        self.lock().push(record.clone());
        self.inner.arrived.notify_waiters();
    }
}

impl fmt::Debug for TelemetryRecorder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TelemetryRecorder")
            .field("records", &self.len())
            .finish()
    }
}

/// Registers a [`TelemetryRecorder`] as a sink.
struct RecorderModule(TelemetryRecorder);

impl Module for RecorderModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.telemetry_sink_value(self.0.clone());
    }
}
