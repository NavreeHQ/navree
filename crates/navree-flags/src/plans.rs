//! [`PlanResolver`]: the plan a workspace is on, for rollouts by plan.

use navree_kernel::{BoxError, BoxFuture};
use uuid::Uuid;

/// Tells which plan a workspace is on, for rollouts limited to plans
/// (`Rollout::plans([..])`). Billing registers one that reads the
/// workspace's local subscription copy; an app can register its own with
/// [`ModuleBuilderFlagsExt::plan_resolver`](crate::ModuleBuilderFlagsExt::plan_resolver)
/// or as a `dyn PlanResolver` provider.
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
