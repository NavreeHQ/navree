//! [`PlanResolver`]: the plan a workspace is on, for rollouts by plan.

use navree_kernel::{BoxError, BoxFuture};
use uuid::Uuid;

/// Tells which plan a workspace is on, for rollouts limited to plans
/// (`Rollout::plans([..])`). Register one with
/// [`ModuleBuilderFlagsExt::plan_resolver`](crate::ModuleBuilderFlagsExt::plan_resolver)
/// or as a `dyn PlanResolver` provider; with billing, it reads the
/// workspace's local subscription copy (`Billing::subscription`). A
/// workspace it gives no plan (`None`) is outside every rollout by plan.
///
/// ```
/// use navree_flags::PlanResolver;
/// use navree_kernel::{BoxError, BoxFuture};
/// use uuid::Uuid;
///
/// struct EveryoneOnPro;
///
/// impl PlanResolver for EveryoneOnPro {
///     fn plan(&self, _workspace: Uuid) -> BoxFuture<'_, Result<Option<String>, BoxError>> {
///         Box::pin(async { Ok(Some("pro".to_owned())) })
///     }
/// }
/// ```
pub trait PlanResolver: Send + Sync + 'static {
    /// The plan of workspace `workspace` (`None` when it has none).
    fn plan(&self, workspace: Uuid) -> BoxFuture<'_, Result<Option<String>, BoxError>>;
}
