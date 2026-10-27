//! [`QueueWorkload`]: one queue's backlog, the numbers the balancer and the
//! dashboard work from.

use serde::{Deserialize, Serialize};

/// The runtime assumed for a queue whose jobs have not run in the last
/// five minutes, so a backlog still counts as work.
pub const ASSUMED_RUNTIME_MS: u64 = 1000;

/// One queue's backlog at a moment, from [`Fleet::workload`](crate::Fleet::workload).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QueueWorkload {
    /// The queue.
    pub queue: String,
    /// Jobs not finished yet: waiting, delayed and running.
    pub pending: u64,
    /// Jobs available now, waiting for a worker.
    pub waiting: u64,
    /// Jobs available later (delayed, or released for a retry).
    pub delayed: u64,
    /// Jobs a worker runs now.
    pub running: u64,
    /// How long the oldest waiting job has waited, in seconds.
    pub oldest_wait_secs: u64,
    /// The average runtime of the queue's jobs over the last five minutes,
    /// in milliseconds (0 when none ran).
    pub avg_runtime_ms: u64,
    /// The time the queue would take to clear on one process, in seconds:
    /// its waiting and running jobs times their average runtime
    /// ([`ASSUMED_RUNTIME_MS`] when none ran lately).
    pub workload_secs: f64,
    /// The processes working the queue, on every running master.
    pub processes: usize,
    /// The time the queue would take to clear on its processes, in seconds.
    pub time_to_clear_secs: u64,
    /// Jobs of the queue that finished in the last minute.
    pub jobs_per_minute: u64,
    /// The queue's `fleet.long_wait` threshold, in seconds.
    pub long_wait_threshold_secs: Option<u64>,
    /// Whether its oldest job has waited longer than the threshold (the
    /// dashboard marks the queue in amber).
    pub over_threshold: bool,
}

impl QueueWorkload {
    /// A workload of `waiting` and `running` jobs with this average
    /// runtime; the other figures start at zero.
    pub fn new(
        queue: &str,
        waiting: u64,
        running: u64,
        oldest_wait_secs: u64,
        avg_runtime_ms: u64,
    ) -> Self {
        let mut workload = Self {
            queue: queue.to_owned(),
            pending: waiting + running,
            waiting,
            delayed: 0,
            running,
            oldest_wait_secs,
            avg_runtime_ms,
            workload_secs: 0.0,
            processes: 0,
            time_to_clear_secs: 0,
            jobs_per_minute: 0,
            long_wait_threshold_secs: None,
            over_threshold: false,
        };
        workload.compute();
        workload
    }

    /// Jobs that need a worker now: waiting and running.
    pub fn active(&self) -> u64 {
        self.waiting + self.running
    }

    /// Recomputes the workload, the time to clear and the threshold flag
    /// from the other fields.
    pub(crate) fn compute(&mut self) {
        let runtime = if self.avg_runtime_ms == 0 {
            ASSUMED_RUNTIME_MS
        } else {
            self.avg_runtime_ms
        };
        self.workload_secs = self.active() as f64 * runtime as f64 / 1000.0;
        let processes = self.processes.max(1) as f64;
        self.time_to_clear_secs = (self.workload_secs / processes).ceil() as u64;
        self.over_threshold = self
            .long_wait_threshold_secs
            .is_some_and(|threshold| self.oldest_wait_secs > threshold);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workload_is_active_jobs_times_average_runtime() {
        let w = QueueWorkload::new("mail", 30, 2, 4, 500);
        assert_eq!(w.active(), 32);
        assert_eq!(w.pending, 32);
        assert!((w.workload_secs - 16.0).abs() < f64::EPSILON);
        assert_eq!(w.time_to_clear_secs, 16);
    }

    #[test]
    fn a_queue_without_recent_runtimes_assumes_one_second_per_job() {
        let w = QueueWorkload::new("mail", 5, 0, 0, 0);
        assert!((w.workload_secs - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn processes_and_thresholds_are_applied() {
        let mut w = QueueWorkload::new("mail", 40, 0, 31, 1000);
        w.processes = 4;
        w.long_wait_threshold_secs = Some(30);
        w.compute();
        assert_eq!(w.time_to_clear_secs, 10);
        assert!(w.over_threshold);
        w.oldest_wait_secs = 30;
        w.compute();
        assert!(!w.over_threshold);
    }
}
