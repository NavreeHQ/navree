//! [`QueueFake`]: records dispatched jobs for assertions in tests.

use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use navree_contracts::{BoxError, BoxFuture, Dispatcher, Job, JobEnvelope};

use crate::store::Store;
use crate::{Queue, QueueOptions};

/// A queue that records jobs instead of running them, for tests.
///
/// Swap it in for the app's queue with
/// `override_value(Arc::new(fake.queue()))` (the module's `dyn Dispatcher`
/// then records too), or, in an app without `QueueModule`, with
/// `override_value(fake.dispatcher())`. Then assert on what was dispatched:
///
/// ```
/// use navree_contracts::{BoxError, Job, JobContext};
/// use navree_queue::QueueFake;
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Debug, Serialize, Deserialize)]
/// struct SendInvoice {
///     invoice: u32,
/// }
///
/// impl Job for SendInvoice {
///     const NAME: &'static str = "billing.send_invoice";
///
///     async fn handle(self, _ctx: JobContext) -> Result<(), BoxError> {
///         Ok(())
///     }
/// }
///
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() {
/// let fake = QueueFake::new();
/// fake.queue().dispatch(SendInvoice { invoice: 7 }).await.unwrap();
///
/// fake.assert_dispatched::<SendInvoice>(|job| job.invoice == 7);
/// assert_eq!(fake.dispatched::<SendInvoice>().len(), 1);
/// # }
/// ```
///
/// Clones share the record. Jobs dispatched in a unit of work are recorded
/// when it commits. The fake queue never runs jobs: `work` and
/// `run_pending` return empty stats and nothing ever fails.
#[derive(Clone, Default)]
pub struct QueueFake {
    recorded: Arc<Mutex<Vec<JobEnvelope>>>,
}

impl QueueFake {
    /// An empty fake.
    pub fn new() -> Self {
        Self::default()
    }

    /// A [`Queue`] whose dispatches are recorded by this fake. It accepts
    /// any job type, registered or not.
    pub fn queue(&self) -> Queue {
        Queue::new(
            Store::Fake(self.clone()),
            QueueOptions::default(),
            None,
            Vec::new(),
            None,
        )
    }

    /// This fake as a `dyn Dispatcher`.
    pub fn dispatcher(&self) -> Arc<dyn Dispatcher> {
        Arc::new(self.clone())
    }

    /// Every recorded job, in dispatch order.
    pub fn envelopes(&self) -> Vec<JobEnvelope> {
        self.lock().clone()
    }

    /// The recorded jobs of type `J`, in dispatch order.
    ///
    /// # Panics
    ///
    /// When a recorded payload of type `J` does not deserialize as `J`.
    pub fn dispatched<J: Job>(&self) -> Vec<J> {
        self.lock()
            .iter()
            .filter(|e| e.job_type == J::NAME)
            .map(|e| {
                serde_json::from_value(e.payload.clone()).unwrap_or_else(|error| {
                    panic!("a recorded `{}` job does not deserialize: {error}", J::NAME)
                })
            })
            .collect()
    }

    /// Asserts a job of type `J` matching `pred` was dispatched.
    ///
    /// # Panics
    ///
    /// When none was, naming the job type and what was dispatched.
    pub fn assert_dispatched<J: Job>(&self, pred: impl Fn(&J) -> bool) {
        let jobs = self.dispatched::<J>();
        assert!(
            jobs.iter().any(pred),
            "expected a matching `{}` job to be dispatched; {} `{}` job(s) were, and these job types: {:?}",
            J::NAME,
            jobs.len(),
            J::NAME,
            self.job_types()
        );
    }

    /// Asserts exactly `times` jobs of type `J` were dispatched.
    ///
    /// # Panics
    ///
    /// When the count differs.
    pub fn assert_dispatched_times<J: Job>(&self, times: usize) {
        let count = self.dispatched::<J>().len();
        assert_eq!(
            count,
            times,
            "expected `{}` to be dispatched {times} time(s), it was {count} time(s)",
            J::NAME
        );
    }

    /// Asserts no job of type `J` was dispatched.
    ///
    /// # Panics
    ///
    /// When one was.
    pub fn assert_not_dispatched<J: Job>(&self) {
        let count = self.dispatched::<J>().len();
        assert_eq!(
            count,
            0,
            "expected no `{}` job to be dispatched, {count} were",
            J::NAME
        );
    }

    /// Asserts nothing was dispatched.
    ///
    /// # Panics
    ///
    /// When something was.
    pub fn assert_nothing_dispatched(&self) {
        let types = self.job_types();
        assert!(
            types.is_empty(),
            "expected no jobs to be dispatched, these were: {types:?}"
        );
    }

    /// Forgets every recorded job.
    pub fn clear(&self) {
        self.lock().clear();
    }

    /// Records `envelope`.
    pub(crate) fn record(&self, envelope: JobEnvelope) {
        self.lock().push(envelope);
    }

    /// How many recorded jobs are on `queue`.
    pub(crate) fn count_on(&self, queue: &str) -> u64 {
        self.lock()
            .iter()
            .filter(|e| e.options.queue == queue)
            .count() as u64
    }

    fn job_types(&self) -> Vec<String> {
        self.lock().iter().map(|e| e.job_type.clone()).collect()
    }

    fn lock(&self) -> MutexGuard<'_, Vec<JobEnvelope>> {
        self.recorded.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Dispatcher for QueueFake {
    fn dispatch_envelope(&self, env: JobEnvelope) -> BoxFuture<'_, Result<String, BoxError>> {
        Box::pin(async move { Ok(self.queue().push(env).await?) })
    }
}

impl fmt::Debug for QueueFake {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QueueFake")
            .field("dispatched", &self.job_types())
            .finish()
    }
}
