//! [`JobRecorder`]: the telemetry sink that turns navree-queue's
//! `JobProcessed` and `JobFailed` events into [`CompletedJob`]s for the
//! worker to report.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use navree_queue::AttemptOutcome;
use navree_telemetry::{EventKind, Interest, TelemetryEvent, TelemetryRecord, TelemetrySink};

use crate::record::CompletedJob;

/// How many unreported attempts a process keeps; older ones are dropped
/// (and counted) when nobody drains them.
pub(crate) const RECORDER_CAPACITY: usize = 10_000;

/// Collects every job attempt that finishes in this process, from the
/// queue's telemetry events. A worker drains it into its reports.
#[derive(Debug, Default)]
pub struct JobRecorder {
    buffer: Mutex<VecDeque<CompletedJob>>,
    dropped: AtomicU64,
}

impl JobRecorder {
    /// An empty recorder.
    pub fn new() -> Self {
        Self::default()
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
