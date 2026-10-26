//! [`Health`]: runs every registered [`HealthCheck`] and aggregates a
//! [`HealthReport`].

use std::collections::BTreeMap;
use std::fmt;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::Duration;

use futures_util::FutureExt;
use futures_util::future::join_all;
use navree_contracts::{HealthCheck, HealthStatus};
use serde::Serialize;
use tokio::time::Instant;

/// How long one check may take before it counts as down.
pub const DEFAULT_CHECK_TIMEOUT: Duration = Duration::from_secs(2);

/// The readiness checks of the app: every `dyn HealthCheck` multi-binding
/// (`m.multi(Factory::<dyn HealthCheck>::new(..))`), collected at boot.
pub struct Health {
    checks: Vec<Arc<dyn HealthCheck>>,
}

impl Health {
    /// Aggregates `checks`.
    pub fn new(checks: Vec<Arc<dyn HealthCheck>>) -> Self {
        Self { checks }
    }

    /// Runs every check concurrently, each limited to
    /// [`DEFAULT_CHECK_TIMEOUT`], and reports each result. A check that does not finish in time, or that
    /// panics, is down. Checks sharing a name are reported as `name`,
    /// `name#2`, `name#3`, ...
    pub async fn check(&self) -> HealthReport {
        let results = join_all(
            self.checks
                .iter()
                .map(|check| run(check.as_ref(), DEFAULT_CHECK_TIMEOUT)),
        )
        .await;
        let mut checks = BTreeMap::new();
        for (check, result) in self.checks.iter().zip(results) {
            let name = unique_name(&checks, check.name());
            if let Some(error) = &result.error {
                tracing::warn!(
                    target: "navree::health",
                    check = %name,
                    error = %error,
                    latency_ms = result.latency_ms,
                    "health check down"
                );
            }
            checks.insert(name, result);
        }
        let status = if checks.values().all(|c| c.status == CheckState::Up) {
            CheckState::Up
        } else {
            CheckState::Down
        };
        HealthReport { status, checks }
    }
}

impl fmt::Debug for Health {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<&str> = self.checks.iter().map(|c| c.name()).collect();
        f.debug_struct("Health").field("checks", &names).finish()
    }
}

/// `name`, or `name#2`, `name#3`, ... when `name` is already reported.
fn unique_name<V>(reported: &BTreeMap<String, V>, name: &str) -> String {
    if !reported.contains_key(name) {
        return name.to_owned();
    }
    (2..)
        .map(|n| format!("{name}#{n}"))
        .find(|candidate| !reported.contains_key(candidate))
        .unwrap_or_else(|| name.to_owned())
}

/// Runs one check within `timeout`, timing it; a panic counts as down.
async fn run(check: &dyn HealthCheck, timeout: Duration) -> CheckReport {
    let start = Instant::now();
    // Calling `check()` inside the async block also catches a panic raised
    // before the check returns its future.
    let guarded = AssertUnwindSafe(async { check.check().await }).catch_unwind();
    let status = match tokio::time::timeout(timeout, guarded).await {
        Ok(Ok(status)) => status,
        Ok(Err(_)) => HealthStatus::Down("the check panicked".to_owned()),
        Err(_) => HealthStatus::Down(format!("timed out after {} ms", timeout.as_millis())),
    };
    let latency_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
    match status {
        HealthStatus::Up => CheckReport {
            status: CheckState::Up,
            latency_ms,
            error: None,
        },
        HealthStatus::Down(error) => CheckReport {
            status: CheckState::Down,
            latency_ms,
            error: Some(error),
        },
    }
}

/// The readiness report served by `GET /health/ready`:
/// `{"status": "up"|"down", "checks": {name: {status, latency_ms, error?}}}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HealthReport {
    /// `up` when every check is up.
    pub status: CheckState,
    /// Each check's result, by check name.
    pub checks: BTreeMap<String, CheckReport>,
}

impl HealthReport {
    /// Whether every check is up.
    pub fn is_up(&self) -> bool {
        self.status == CheckState::Up
    }
}

/// The result of one check.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CheckReport {
    /// Up or down.
    pub status: CheckState,
    /// How long the check took, in milliseconds.
    pub latency_ms: u64,
    /// Why the check is down.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Up or down, serialized as `"up"` / `"down"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckState {
    /// Healthy.
    Up,
    /// Unhealthy.
    Down,
}
