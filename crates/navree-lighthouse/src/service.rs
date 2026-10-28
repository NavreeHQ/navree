//! [`Lighthouse`]: the service your code reports through; the telemetry
//! sink; the background writer.

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Utc;
use navree_kernel::{AppContext, BoxError, BoxFuture};
use navree_telemetry::{
    EventKind, Interest, Telemetry, TelemetryContext, TelemetryEvent, TelemetryRecord,
    TelemetrySink,
};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::alert::Alert;
use crate::dispatch::{Alerter, ChannelResult};
use crate::engine::{
    Batch, BatchWriter, EngineHandle, INTERNAL_UNIT, Input, Labels, Processor, UnitKey,
};
use crate::fake::FakeState;
use crate::options::LighthouseOptions;
use crate::report::{AsError, Report};
use crate::store::{LighthouseError, LighthouseStore};

tokio::task_local! {
    /// Set by the HTTP layer around a request: whether something was
    /// reported during it (so its headers are kept).
    pub(crate) static REPORTED: Arc<AtomicBool>;
}

/// Notes that the current request reported something.
pub(crate) fn mark_reported() {
    let _ = REPORTED.try_with(|flag| flag.store(true, Ordering::Relaxed));
}

/// Runs `future` as Lighthouse's own work: the queries and calls it makes
/// are not recorded.
pub(crate) fn internal<F: Future>(future: F) -> impl Future<Output = F::Output> {
    TelemetryContext::new(Telemetry::disabled())
        .with_job_id(INTERNAL_UNIT)
        .scope(future)
}

/// Application monitoring: report errors, read what was recorded, raise
/// alerts. Provided and exported by `LighthouseModule`; inject
/// `Arc<Lighthouse>`.
///
/// ```
/// use navree_lighthouse::{Level, LighthouseFake, Report};
///
/// let fake = LighthouseFake::new();
/// let lighthouse = fake.lighthouse();
/// let error = std::io::Error::other("card declined");
/// lighthouse.report(&error);
/// lighthouse.capture(Report::message("no customer").level(Level::Warning));
/// fake.assert_reported_times(2);
/// fake.assert_reported::<std::io::Error>(|report| report.message == "card declined");
/// ```
pub struct Lighthouse {
    options: Arc<LighthouseOptions>,
    store: Option<LighthouseStore>,
    engine: Option<EngineHandle>,
    alerter: Option<Arc<Alerter>>,
    runtime: Mutex<Runtime>,
    started: AtomicBool,
    fake: Option<Arc<FakeState>>,
}

/// The background writer: waiting to start, then running.
#[derive(Default)]
struct Runtime {
    pending: Option<(Processor, mpsc::Receiver<Input>)>,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for Lighthouse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lighthouse")
            .field("recording", &self.is_recording())
            .field("store", &self.store.is_some())
            .field("fake", &self.fake.is_some())
            .finish_non_exhaustive()
    }
}

impl Lighthouse {
    /// A recording Lighthouse (built by the module).
    pub(crate) fn live(
        options: Arc<LighthouseOptions>,
        store: Option<LighthouseStore>,
        recorder: Option<(EngineHandle, Processor, mpsc::Receiver<Input>)>,
        alerter: Option<Arc<Alerter>>,
    ) -> Lighthouse {
        let (engine, pending) = match recorder {
            Some((handle, processor, rx)) => (Some(handle), Some((processor, rx))),
            None => (None, None),
        };
        Lighthouse {
            options,
            store,
            engine,
            alerter,
            runtime: Mutex::new(Runtime {
                pending,
                ..Runtime::default()
            }),
            started: AtomicBool::new(false),
            fake: None,
        }
    }

    /// A Lighthouse that records into a fake.
    pub(crate) fn fake(state: Arc<FakeState>) -> Lighthouse {
        Lighthouse {
            options: Arc::new(LighthouseOptions::default()),
            store: None,
            engine: None,
            alerter: None,
            runtime: Mutex::new(Runtime::default()),
            started: AtomicBool::new(false),
            fake: Some(state),
        }
    }

    /// The options.
    pub fn options(&self) -> &LighthouseOptions {
        &self.options
    }

    /// What was recorded (`None` with `lighthouse.store = "off"` and in a
    /// fake).
    pub fn store(&self) -> Option<&LighthouseStore> {
        self.store.as_ref()
    }

    /// Whether records are being taken (`lighthouse.enabled`, a store, or a
    /// fake).
    pub fn is_recording(&self) -> bool {
        self.engine.is_some() || self.fake.is_some()
    }

    /// Reports `error` (with its whole source chain), at the caller's
    /// location, with the context of the current request or job. Returns
    /// at once; the occurrence is written in the background.
    #[track_caller]
    pub fn report<E: AsError + ?Sized>(&self, error: &E) {
        if !self.is_recording() {
            return;
        }
        let mut report = Report::error(error);
        if self.fake.is_some() {
            report.debug = Some(format!("{:?}", error.as_error()));
        }
        self.capture(report);
    }

    /// Records a [`Report`]: an error or message with a level, context
    /// values, tags, a fingerprint, a user or a backtrace.
    pub fn capture(&self, report: Report) {
        let context = Telemetry::current_context();
        if let Some(fake) = &self.fake {
            fake.record(&report, &context);
            return;
        }
        if let Some(engine) = &self.engine {
            mark_reported();
            engine.send(Input::Report(Box::new((report, context, Utc::now()))));
        }
    }

    /// Tags everything the current request or job reports, and its trace.
    pub fn tag_current(&self, key: impl Into<String>, value: impl ToString) {
        let (key, value) = (key.into(), value.to_string());
        if let Some(fake) = &self.fake {
            fake.tag(key, value);
            return;
        }
        if let (Some(engine), Some(unit)) =
            (&self.engine, UnitKey::of(&Telemetry::current_context()))
        {
            engine.send(Input::Tag(unit, key, value));
        }
    }

    /// Raises an alert through every channel that wants its kind (the
    /// cooldown applies): what a queue monitor does for
    /// [`AlertKind::LongWait`](crate::AlertKind::LongWait). Returns the
    /// number of channels it went to.
    pub async fn raise_alert(&self, alert: Alert) -> Result<usize, LighthouseError> {
        if let Some(fake) = &self.fake {
            fake.alert(alert);
            return Ok(1);
        }
        match &self.alerter {
            Some(alerter) => internal(alerter.raise(alert)).await,
            None => Ok(0),
        }
    }

    /// Sends a test alert through every channel, or the one named
    /// `channel` (`lighthouse:test-alerts`).
    pub async fn test_alerts(
        &self,
        channel: Option<&str>,
    ) -> Result<Vec<ChannelResult>, LighthouseError> {
        match &self.alerter {
            Some(alerter) => internal(alerter.test(channel)).await,
            None => Err(LighthouseError::Other(
                "alerts need lighthouse.store = \"database\"".to_owned(),
            )),
        }
    }

    /// Writes everything recorded so far and waits for it (tests, and
    /// before a command exits).
    pub async fn flush(&self) {
        if let Some(engine) = &self.engine
            && self.started.load(Ordering::Acquire)
        {
            engine.flush().await;
        }
    }

    /// Details (samples, occurrences) this process dropped because the
    /// buffer was full.
    pub fn dropped(&self) -> u64 {
        self.engine.as_ref().map_or(0, EngineHandle::dropped)
    }

    /// Details sampled out by this process.
    pub fn sampled_out(&self) -> u64 {
        self.dropped()
    }

    /// Delivers a stored alert (the `lighthouse.alert` job).
    pub(crate) async fn deliver_stored_alert(
        &self,
        alert_id: Uuid,
        attempt: u32,
        max: u32,
    ) -> Result<(), BoxError> {
        match &self.alerter {
            Some(alerter) => alerter.deliver_stored(alert_id, attempt, max).await,
            None => Ok(()),
        }
    }

    /// Hands a telemetry record to the engine (or the fake).
    pub(crate) fn record_telemetry(&self, record: &TelemetryRecord) {
        if let Some(fake) = &self.fake {
            if let TelemetryEvent::ExceptionReported(exception) = &record.event {
                fake.exception(exception, &record.context);
            }
            return;
        }
        if let Some(engine) = &self.engine {
            if matches!(record.event, TelemetryEvent::ExceptionReported(_)) {
                mark_reported();
            }
            engine.send(Input::Record(Box::new(record.clone())));
        }
    }

    /// The kinds the sink wants.
    pub(crate) fn interest(&self) -> Interest {
        if self.fake.is_some() {
            return Interest::only([EventKind::ExceptionReported]);
        }
        if self.engine.is_none() {
            return Interest::NONE;
        }
        Interest::only([
            EventKind::RequestHandled,
            EventKind::QueryExecuted,
            EventKind::JobProcessed,
            EventKind::JobFailed,
            EventKind::ScheduledTaskRan,
            EventKind::MailSent,
            EventKind::Cache,
            EventKind::HttpClientRequest,
            EventKind::ExceptionReported,
        ])
    }

    /// The engine's handle (the HTTP layer sends request captures).
    pub(crate) fn engine(&self) -> Option<&EngineHandle> {
        self.engine.as_ref()
    }

    /// Starts the background writer (at boot).
    pub(crate) async fn start(&self, ctx: &AppContext) -> Result<(), BoxError> {
        if let Some(alerter) = &self.alerter {
            alerter.init(ctx).await?;
        }
        let pending = self
            .runtime
            .lock()
            .map_err(|_| "the Lighthouse runtime lock is poisoned")?
            .pending
            .take();
        let (Some((processor, rx)), Some(store), Some(alerter)) =
            (pending, self.store.clone(), self.alerter.clone())
        else {
            return Ok(());
        };
        let writer: Arc<dyn BatchWriter> = Arc::new(StoreWriter { store, alerter });
        let (stop_tx, stop_rx) = oneshot::channel::<()>();
        let task = tokio::spawn(internal(processor.run(rx, writer, async move {
            let _ = stop_rx.await;
        })));
        let mut runtime = self
            .runtime
            .lock()
            .map_err(|_| "the Lighthouse runtime lock is poisoned")?;
        runtime.stop = Some(stop_tx);
        runtime.task = Some(task);
        self.started.store(true, Ordering::Release);
        Ok(())
    }

    /// Writes what is buffered and stops the writer (at shutdown).
    pub(crate) async fn stop(&self) {
        let (stop, task) = match self.runtime.lock() {
            Ok(mut runtime) => (runtime.stop.take(), runtime.task.take()),
            Err(_) => return,
        };
        if let Some(stop) = stop {
            let _ = stop.send(());
        }
        if let Some(task) = task {
            let _ = tokio::time::timeout(Duration::from_secs(10), task).await;
        }
        self.started.store(false, Ordering::Release);
    }
}

/// Writes batches to the store, then raises their alerts.
struct StoreWriter {
    store: LighthouseStore,
    alerter: Arc<Alerter>,
}

impl BatchWriter for StoreWriter {
    fn write<'a>(
        &'a self,
        batch: Batch,
        labels: &'a Labels,
    ) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(internal(async move {
            let candidates = self.store.write_batch(batch, labels).await?;
            self.alerter.consider(candidates).await;
            Ok(())
        }))
    }
}

/// The telemetry sink: hands records to [`Lighthouse`].
pub(crate) struct LighthouseSink {
    pub(crate) lighthouse: Arc<Lighthouse>,
}

impl TelemetrySink for LighthouseSink {
    fn interest(&self) -> Interest {
        self.lighthouse.interest()
    }

    fn record(&self, record: &TelemetryRecord) {
        self.lighthouse.record_telemetry(record);
    }
}
