//! Job runs are logged at target `navis::job`, without payloads.
//!
//! A test binary of its own: `tracing` caches per-callsite interest
//! process-wide, so `navis::job` events first hit by other tests' threads
//! (with no subscriber) could be skipped here if they ran in this process.

#[path = "queue/support.rs"]
mod support;

use std::sync::{Arc, Mutex};

use navis_queue::QueueDriver;

use crate::support::{AlwaysFails, Record, harness};

/// Captures formatted tracing output written while the guard is alive (on
/// this thread: use a current-thread runtime).
#[derive(Clone, Default)]
struct LogCapture(Arc<Mutex<Vec<u8>>>);

impl LogCapture {
    fn install(&self) -> tracing::subscriber::DefaultGuard {
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .with_ansi(false)
            .with_writer(self.clone())
            .finish();
        tracing::subscriber::set_default(subscriber)
    }

    fn lines(&self) -> Vec<String> {
        String::from_utf8_lossy(&self.0.lock().unwrap())
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

impl std::io::Write for LogCapture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogCapture {
    type Writer = LogCapture;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

#[tokio::test(flavor = "current_thread")]
async fn job_runs_are_logged_with_name_attempt_duration_and_outcome() {
    let capture = LogCapture::default();
    let _guard = capture.install();
    let h = harness(QueueDriver::Memory).await;
    h.queue
        .dispatch(Record::new("payload-secret-1"))
        .await
        .unwrap();
    h.queue
        .dispatch(AlwaysFails {
            label: "payload-secret-2".into(),
        })
        .await
        .unwrap();

    h.drain().await;

    let lines = capture.lines();
    let job_lines: Vec<&String> = lines.iter().filter(|l| l.contains("navis::job")).collect();
    let succeeded = job_lines
        .iter()
        .find(|l| l.contains("test.record"))
        .unwrap_or_else(|| panic!("the record job is not logged: {lines:#?}"));
    for field in [
        "attempt=1",
        "duration_ms=",
        "outcome=\"succeeded\"",
        "job_id=",
    ] {
        assert!(succeeded.contains(field), "{field} missing: {succeeded}");
    }
    let released = job_lines
        .iter()
        .find(|l| l.contains("test.always_fails"))
        .expect("the failing job is logged");
    assert!(released.contains("outcome=\"released\""), "{released}");
    assert!(released.contains("boom on attempt 1"), "{released}");
    assert!(
        !lines.iter().any(|l| l.contains("payload-secret")),
        "payloads are never logged: {lines:#?}"
    );
}
