//! [`MetricsCollector`]: metrics refreshed when `/metrics` is scraped.

use std::time::Duration;

use navree_kernel::BoxFuture;

/// How long one collector may take before the exposition is rendered
/// without waiting for it.
pub const COLLECT_TIMEOUT: Duration = Duration::from_secs(2);

/// Refreshes metrics right before `GET /metrics` renders them, for numbers
/// read from somewhere else (a database, another process) instead of
/// counted as they happen: set gauges with the `metrics` macros in
/// [`collect`](MetricsCollector::collect).
///
/// Register one as a `dyn MetricsCollector` multi-binding. Collectors run
/// one after the other on every scrape, each limited to
/// [`COLLECT_TIMEOUT`]; one that takes longer is left behind and the
/// exposition shows the previous values.
///
/// ```
/// use std::sync::Arc;
///
/// use navree_kernel::{BoxFuture, Factory, Module, ModuleBuilder};
/// use navree_observe::MetricsCollector;
///
/// struct Backlog;
///
/// impl MetricsCollector for Backlog {
///     fn collect(&self) -> BoxFuture<'_, ()> {
///         Box::pin(async {
///             metrics::gauge!("orders_backlog").set(12.0);
///         })
///     }
/// }
///
/// struct OrdersModule;
///
/// impl Module for OrdersModule {
///     fn register(&self, m: &mut ModuleBuilder) {
///         m.multi(Factory::<dyn MetricsCollector>::new(|_| async {
///             Ok(Arc::new(Backlog) as Arc<dyn MetricsCollector>)
///         }));
///     }
/// }
/// ```
pub trait MetricsCollector: Send + Sync + 'static {
    /// Sets the collector's metrics. Errors are the collector's to log.
    fn collect(&self) -> BoxFuture<'_, ()>;
}
