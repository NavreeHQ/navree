//! Exporting spans: a bounded queue filled by the telemetry sink and
//! drained in batches by a background task, so export never slows
//! requests; stdout and (with feature `otel`) OTLP exporters.

use std::future::Future;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use navree_kernel::BoxFuture;
use navree_telemetry::{Interest, TelemetryRecord, TelemetrySink};
use tokio::sync::mpsc;

use crate::RecordedSpan;
use crate::spans::span_interest;

/// The most spans sent in one export.
const MAX_BATCH: usize = 512;

/// How long a span waits for its batch to fill before it is sent.
const FLUSH_INTERVAL: Duration = Duration::from_secs(2);

/// The metric counting spans dropped because the queue was full.
pub const SPANS_DROPPED_METRIC: &str = "navree_tracing_spans_dropped_total";

/// Where batches of spans go.
pub(crate) trait SpanExport: Send + Sync + 'static {
    /// Sends one batch.
    fn export(&self, batch: Vec<RecordedSpan>) -> BoxFuture<'_, Result<(), String>>;

    /// Releases the exporter once the last batch was sent.
    fn shutdown(&self) -> BoxFuture<'_, ()> {
        Box::pin(async {})
    }
}

/// Prints each span as one JSON line.
pub(crate) struct WriterExport<W: Write + Send + 'static>(pub(crate) Mutex<W>);

impl<W: Write + Send + 'static> SpanExport for WriterExport<W> {
    fn export(&self, batch: Vec<RecordedSpan>) -> BoxFuture<'_, Result<(), String>> {
        let result = (|| {
            let mut out = self.0.lock().unwrap_or_else(PoisonError::into_inner);
            for span in &batch {
                let line = serde_json::to_string(span).map_err(|error| error.to_string())?;
                writeln!(out, "{line}").map_err(|error| error.to_string())?;
            }
            out.flush().map_err(|error| error.to_string())
        })();
        Box::pin(async move { result })
    }
}

/// Sends every batch to each exporter in turn.
pub(crate) struct Fanout(pub(crate) Vec<Arc<dyn SpanExport>>);

impl SpanExport for Fanout {
    fn export(&self, batch: Vec<RecordedSpan>) -> BoxFuture<'_, Result<(), String>> {
        Box::pin(async move {
            let mut errors = Vec::new();
            for exporter in &self.0 {
                if let Err(error) = exporter.export(batch.clone()).await {
                    errors.push(error);
                }
            }
            if errors.is_empty() {
                Ok(())
            } else {
                Err(errors.join("; "))
            }
        })
    }

    fn shutdown(&self) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            for exporter in &self.0 {
                exporter.shutdown().await;
            }
        })
    }
}

/// The telemetry sink that queues spans for export.
pub(crate) struct ExportSink {
    sender: mpsc::Sender<RecordedSpan>,
    dropped: Arc<AtomicU64>,
}

impl ExportSink {
    /// Spans dropped because the queue was full.
    #[cfg(test)]
    pub(crate) fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

impl TelemetrySink for ExportSink {
    fn interest(&self) -> Interest {
        span_interest()
    }

    fn record(&self, record: &TelemetryRecord) {
        let Some(span) = RecordedSpan::from_record(record) else {
            return;
        };
        if self.sender.try_send(span).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            metrics::counter!(SPANS_DROPPED_METRIC).increment(1);
        }
    }
}

/// A sink that wants nothing: tracing exports nowhere.
pub(crate) struct NoExport;

impl TelemetrySink for NoExport {
    fn interest(&self) -> Interest {
        Interest::NONE
    }

    fn record(&self, _record: &TelemetryRecord) {}
}

/// Starts the export task: the returned sink queues up to `capacity`
/// spans; the task sends them in batches (each export bounded by
/// `timeout`) until `shutdown` completes, then sends what is left and
/// shuts the exporter down. Returns the sink and the task.
pub(crate) fn start(
    exporter: Arc<dyn SpanExport>,
    capacity: usize,
    timeout: Duration,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> (ExportSink, impl Future<Output = ()> + Send + 'static) {
    let (sender, receiver) = mpsc::channel(capacity.max(1));
    let sink = ExportSink {
        sender,
        dropped: Arc::new(AtomicU64::new(0)),
    };
    (sink, run(receiver, exporter, timeout, shutdown))
}

async fn run(
    mut receiver: mpsc::Receiver<RecordedSpan>,
    exporter: Arc<dyn SpanExport>,
    timeout: Duration,
    shutdown: impl Future<Output = ()> + Send,
) {
    tokio::pin!(shutdown);
    let mut batch = Vec::new();
    let mut ticker = tokio::time::interval(FLUSH_INTERVAL);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            biased;
            () = &mut shutdown => break,
            span = receiver.recv() => match span {
                Some(span) => {
                    batch.push(span);
                    if batch.len() >= MAX_BATCH {
                        send(exporter.as_ref(), &mut batch, timeout).await;
                    }
                }
                None => break,
            },
            _ = ticker.tick() => send(exporter.as_ref(), &mut batch, timeout).await,
        }
    }
    // Shutting down: send what is queued.
    receiver.close();
    while let Ok(span) = receiver.try_recv() {
        batch.push(span);
        if batch.len() >= MAX_BATCH {
            send(exporter.as_ref(), &mut batch, timeout).await;
        }
    }
    send(exporter.as_ref(), &mut batch, timeout).await;
    exporter.shutdown().await;
}

/// Sends and empties `batch` (nothing when it is empty); failures are logged.
async fn send(exporter: &dyn SpanExport, batch: &mut Vec<RecordedSpan>, timeout: Duration) {
    if batch.is_empty() {
        return;
    }
    let spans = std::mem::take(batch);
    let count = spans.len();
    match tokio::time::timeout(timeout, exporter.export(spans)).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            tracing::warn!(target: "navree::tracing", count, %error, "exporting spans failed");
        }
        Err(_) => {
            tracing::warn!(target: "navree::tracing", count, timeout_ms = timeout.as_millis() as u64, "exporting spans timed out");
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use navree_telemetry::{EventContext, JobProcessed, TelemetryEvent, TraceContext};
    use tokio::sync::oneshot;

    use super::*;

    #[derive(Default)]
    struct Collect {
        batches: Mutex<Vec<Vec<RecordedSpan>>>,
        shut_down: Mutex<bool>,
    }

    impl SpanExport for Arc<Collect> {
        fn export(&self, batch: Vec<RecordedSpan>) -> BoxFuture<'_, Result<(), String>> {
            self.batches.lock().unwrap().push(batch);
            Box::pin(async { Ok(()) })
        }

        fn shutdown(&self) -> BoxFuture<'_, ()> {
            *self.shut_down.lock().unwrap() = true;
            Box::pin(async {})
        }
    }

    fn job(sampled: bool) -> TelemetryRecord {
        TelemetryRecord {
            at: Utc::now(),
            context: EventContext {
                trace: Some(TraceContext::root(sampled)),
                ..EventContext::default()
            },
            event: TelemetryEvent::JobProcessed(JobProcessed {
                queue: "default".into(),
                job_type: "mail.send".into(),
                job_id: "1".into(),
                attempt: 1,
                duration: Duration::from_millis(5),
            }),
        }
    }

    #[tokio::test]
    async fn queued_spans_are_sent_when_the_app_shuts_down() {
        let collect = Arc::new(Collect::default());
        let (stop, stopped) = oneshot::channel::<()>();
        let (sink, task) = start(
            Arc::new(collect.clone()),
            16,
            Duration::from_secs(5),
            async move {
                let _ = stopped.await;
            },
        );
        let task = tokio::spawn(task);
        sink.record(&job(true));
        sink.record(&job(true));
        sink.record(&job(false)); // unsampled: not a span
        stop.send(()).unwrap();
        task.await.unwrap();
        let batches = collect.batches.lock().unwrap().clone();
        let spans: usize = batches.iter().map(Vec::len).sum();
        assert_eq!(spans, 2);
        assert_eq!(batches[0][0].name, "job mail.send");
        assert!(*collect.shut_down.lock().unwrap());
    }

    #[tokio::test]
    async fn a_full_queue_drops_new_spans() {
        let collect = Arc::new(Collect::default());
        let (sink, _task) = start(
            Arc::new(collect.clone()),
            2,
            Duration::from_secs(5),
            std::future::pending(),
        );
        for _ in 0..5 {
            sink.record(&job(true));
        }
        assert_eq!(
            sink.dropped(),
            3,
            "the task never ran, so the queue filled up"
        );
    }

    #[tokio::test]
    async fn the_writer_prints_one_json_line_per_span() {
        let export = WriterExport(Mutex::new(Vec::new()));
        let span = RecordedSpan::from_record(&job(true)).unwrap();
        export.export(vec![span.clone(), span]).await.unwrap();
        let out = String::from_utf8(export.0.into_inner().unwrap()).unwrap();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2);
        let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first["name"], "job mail.send");
        assert_eq!(first["kind"], "consumer");
        assert_eq!(first["trace_id"].as_str().unwrap().len(), 32);
    }

    #[test]
    fn no_export_wants_nothing() {
        assert!(NoExport.interest().is_empty());
    }
}
