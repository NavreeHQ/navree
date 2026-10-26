//! Test helpers: [`SpanRecorder`].

use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use navree_kernel::{Module, ModuleBuilder};
use navree_telemetry::{Interest, ModuleBuilderTelemetryExt, TelemetryRecord, TelemetrySink};

use crate::RecordedSpan;
use crate::spans::span_interest;

/// Collects finished spans in memory, for assertions in tests. It is a
/// telemetry sink: add [`module`](SpanRecorder::module) to the app. It
/// sees the spans of sampled traces whether or not an endpoint is
/// configured, so tests never export anything. Clones share the spans.
///
/// ```
/// use navree_kernel::Navree;
/// use navree_observe::testing::SpanRecorder;
///
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() -> Result<(), navree_kernel::BoxError> {
/// let spans = SpanRecorder::new();
/// let app = Navree::app().module(spans.module()).boot().await?;
/// // ... send requests, run jobs ...
/// assert!(spans.all().is_empty());
/// app.shutdown().await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Default)]
pub struct SpanRecorder {
    spans: Arc<Mutex<Vec<RecordedSpan>>>,
}

impl SpanRecorder {
    /// An empty recorder.
    pub fn new() -> Self {
        Self::default()
    }

    /// A module that registers this recorder as a telemetry sink.
    pub fn module(&self) -> impl Module {
        RecorderModule(self.clone())
    }

    fn lock(&self) -> MutexGuard<'_, Vec<RecordedSpan>> {
        self.spans.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Every finished span, in the order they ended.
    pub fn all(&self) -> Vec<RecordedSpan> {
        self.lock().clone()
    }

    /// The first span named `name`.
    ///
    /// # Panics
    ///
    /// When there is none (the message lists the names recorded).
    pub fn find(&self, name: &str) -> RecordedSpan {
        let spans = self.lock();
        match spans.iter().find(|span| span.name == name) {
            Some(span) => span.clone(),
            None => {
                let names: Vec<&str> = spans.iter().map(|span| span.name.as_str()).collect();
                panic!("no span named `{name}`; recorded: {names:?}")
            }
        }
    }

    /// Every span named `name`.
    pub fn find_all(&self, name: &str) -> Vec<RecordedSpan> {
        self.lock()
            .iter()
            .filter(|span| span.name == name)
            .cloned()
            .collect()
    }

    /// Forgets every span.
    pub fn clear(&self) {
        self.lock().clear();
    }
}

impl TelemetrySink for SpanRecorder {
    fn interest(&self) -> Interest {
        span_interest()
    }

    fn record(&self, record: &TelemetryRecord) {
        if let Some(span) = RecordedSpan::from_record(record) {
            self.lock().push(span);
        }
    }
}

impl fmt::Debug for SpanRecorder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SpanRecorder")
            .field("spans", &self.lock().len())
            .finish()
    }
}

/// Registers a [`SpanRecorder`] as a telemetry sink.
struct RecorderModule(SpanRecorder);

impl Module for RecorderModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.telemetry_sink_value(self.0.clone());
    }
}
