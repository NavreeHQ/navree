//! The container: the validated graph plus memoized singleton instances, and
//! the per-unit-of-work [`RequestScope`] cache.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use tokio::sync::OnceCell;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::CommandSpec;
use crate::builder::Instance;
use crate::graph::Graph;

/// Shared state behind [`AppContext`](crate::AppContext) and every [`Resolver`](crate::Resolver).
pub(crate) struct AppInner {
    pub(crate) graph: Graph,
    /// One cell per provider; only singleton providers use theirs.
    pub(crate) singletons: Vec<OnceCell<Instance>>,
    pub(crate) shutdown: CancellationToken,
    pub(crate) tracker: TaskTracker,
    /// The command the app was booted to run, if any.
    pub(crate) command: Option<CommandSpec>,
}

impl AppInner {
    pub(crate) fn new(
        graph: Graph,
        shutdown: CancellationToken,
        command: Option<CommandSpec>,
    ) -> Self {
        let singletons = graph.providers.iter().map(|_| OnceCell::new()).collect();
        Self {
            graph,
            singletons,
            shutdown,
            tracker: TaskTracker::new(),
            command,
        }
    }
}

/// Identifies one app in a [`RequestScope`] cache.
pub(crate) fn app_identity(inner: &Arc<AppInner>) -> usize {
    Arc::as_ptr(inner).addr()
}

/// The cache of one unit of work (an HTTP request, a job run, a command, ...):
/// request-scoped providers are built at most once per `RequestScope`.
/// Clones share the same cache.
#[derive(Clone, Default)]
pub struct RequestScope {
    cells: Arc<Mutex<ScopeCells>>,
}

/// Memoization cells keyed by (app identity, provider index).
type ScopeCells = HashMap<(usize, usize), Arc<OnceCell<Instance>>>;

impl RequestScope {
    /// A fresh, empty scope.
    pub fn new() -> Self {
        Self::default()
    }

    /// The memoization cell of provider `provider` of app `app`.
    pub(crate) fn cell(&self, app: usize, provider: usize) -> Arc<OnceCell<Instance>> {
        let mut cells = self.cells.lock().unwrap_or_else(PoisonError::into_inner);
        cells.entry((app, provider)).or_default().clone()
    }
}

impl fmt::Debug for RequestScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let len = self.cells.lock().map_or(0, |cells| cells.len());
        f.debug_struct("RequestScope")
            .field("cached", &len)
            .finish()
    }
}
