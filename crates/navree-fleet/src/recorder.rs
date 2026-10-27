//! [`JobRecorder`]: the telemetry sink that turns navree-queue's
//! `JobProcessed` and `JobFailed` events into [`CompletedJob`]s for the
//! worker to report.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use navree_queue::AttemptOutcome;
use navree_telemetry::{EventKind, Interest, TelemetryEvent, TelemetryRecord, TelemetrySink};

use crate::record::CompletedJob;

/// How many unreported attempts a process keeps; older ones are dropped
/// (and counted) when nobody drains them.
pub(crate) const RECORDER_CAPACITY: usize = 10_000;

/// Collects every job attempt that finishes in this process, from the
/// queue's telemetry events, once a Fleet worker runs in the process
/// ([`arm`](JobRecorder::arm)); a worker drains it into its reports. In
/// other processes (the web server, a plain `queue:work`) it keeps
/// nothing.
#[derive(Debug, Default)]
pub struct JobRecorder {
    buffer: Mutex<VecDeque<CompletedJob>>,
    dropped: AtomicU64,
    armed: AtomicBool,
}

impl JobRecorder {
    /// An empty recorder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts recording (a Fleet worker runs in this process).
    pub fn arm(&self) {
        self.armed.store(true, Ordering::SeqCst);
    }

    /// Whether it records.
    pub fn is_armed(&self) -> bool {
        self.armed.load(Ordering::SeqCst)
    }

    /// Takes every recorded attempt, oldest first.
    pub fn drain(&self) -> Vec<CompletedJob> {
        let mut buffer = self
            .buffer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        buffer.drain(..).collect()
    }

    /// Attempts dropped because the buffer was full.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    fn push(&self, job: CompletedJob) {
        let mut buffer = self
            .buffer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if buffer.len() >= RECORDER_CAPACITY {
            buffer.pop_front();
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
        buffer.push_back(job);
    }
}

impl TelemetrySink for JobRecorder {
    fn interest(&self) -> Interest {
        Interest::only([EventKind::JobProcessed, EventKind::JobFailed])
    }

    fn record(&self, record: &TelemetryRecord) {
        if !self.is_armed() {
            return;
        }
        let job = match &record.event {
            TelemetryEvent::JobProcessed(e) => CompletedJob {
                job_id: e.job_id.clone(),
                queue: e.queue.clone(),
                job_type: e.job_type.clone(),
                attempt: e.attempt,
                outcome: AttemptOutcome::Succeeded,
                runtime_ms: u64::try_from(e.duration.as_millis()).unwrap_or(u64::MAX),
                finished_at: record.at,
            },
            TelemetryEvent::JobFailed(e) => CompletedJob {
                job_id: e.job_id.clone(),
                queue: e.queue.clone(),
                job_type: e.job_type.clone(),
                attempt: e.attempt,
                outcome: if e.will_retry {
                    AttemptOutcome::Released
                } else {
                    AttemptOutcome::Failed
                },
                runtime_ms: u64::try_from(e.duration.as_millis()).unwrap_or(u64::MAX),
                finished_at: record.at,
            },
            _ => return,
        };
        self.push(job);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use chrono::Utc;
    use navree_telemetry::{EventContext, JobFailed, JobProcessed};

    use super::*;

    fn record(event: TelemetryEvent) -> TelemetryRecord {
        TelemetryRecord {
            at: Utc::now(),
            context: EventContext::default(),
            event,
        }
    }

    fn processed() -> TelemetryEvent {
        TelemetryEvent::JobProcessed(JobProcessed {
            queue: "mail".into(),
            job_type: "mail.send".into(),
            job_id: "j1".into(),
            attempt: 1,
            duration: Duration::from_millis(42),
        })
    }

    #[test]
    fn it_records_only_once_armed() {
        let recorder = JobRecorder::new();
        recorder.record(&record(processed()));
        assert!(recorder.drain().is_empty());
        recorder.arm();
        recorder.record(&record(processed()));
        recorder.record(&record(TelemetryEvent::JobFailed(JobFailed {
            queue: "mail".into(),
            job_type: "mail.send".into(),
            job_id: "j2".into(),
            attempt: 2,
            duration: Duration::from_millis(7),
            error: "boom".into(),
            will_retry: true,
        })));
        let jobs = recorder.drain();
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].outcome, AttemptOutcome::Succeeded);
        assert_eq!(jobs[0].runtime_ms, 42);
        assert_eq!(jobs[1].outcome, AttemptOutcome::Released);
        assert!(recorder.drain().is_empty());
    }

    #[test]
    fn a_full_buffer_drops_the_oldest() {
        let recorder = JobRecorder::new();
        recorder.arm();
        for _ in 0..RECORDER_CAPACITY + 3 {
            recorder.record(&record(processed()));
        }
        assert_eq!(recorder.dropped(), 3);
        assert_eq!(recorder.drain().len(), RECORDER_CAPACITY);
    }
}
