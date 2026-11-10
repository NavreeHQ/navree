//! [`HealthCheck`]: readiness checks collected by navree-observe.

use crate::BoxFuture;

/// A readiness check, registered as a multi-binding of `dyn HealthCheck`.
pub trait HealthCheck: Send + Sync + 'static {
    /// The check name shown in the readiness report, e.g. `database`.
    fn name(&self) -> &str;

    /// Runs the check.
    fn check(&self) -> BoxFuture<'_, HealthStatus>;
}

/// The result of a [`HealthCheck`].
#[derive(Clone, Debug, PartialEq)]
pub enum HealthStatus {
    /// Healthy.
    Up,
    /// Unhealthy, with a reason (must not contain secrets).
    Down(String),
}
