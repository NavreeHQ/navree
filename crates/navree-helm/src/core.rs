//! [`Core`]: the panel's shared state (options, registry, the app's templates
//! and the metric cache), built once when the panel boots.

use std::sync::Arc;
use std::time::Duration;

use navree_kernel::{AppContext, BoxError};
use navree_validation::Messages;
use navree_view::Views;

use crate::field::FieldKind;
use crate::metric::MetricResult;
use crate::registry::Registry;
use crate::workspace::{StaffCheck, StaffOrganizationCheck, StaffOrganizationPolicy, StaffPolicy};
use crate::{HelmOptions, save};

/// The panel's shared state.
pub(crate) struct Core {
    pub(crate) options: Arc<HelmOptions>,
    pub(crate) registry: Registry,
    pub(crate) messages: Messages,
    /// The app's templates (`ViewModule`), for view fields and pages.
    pub(crate) app_views: Option<Arc<Views>>,
    pub(crate) metrics: Option<navree_bounded::Cache<String, Arc<MetricResult>>>,
    /// The sidebar's counts, per user, access and workspace.
    pub(crate) counts: Option<navree_bounded::Cache<String, Arc<u64>>>,
    /// Who may work in a workspace they are not in, if the app says.
    pub(crate) staff: Option<StaffCheck>,
    /// Who may work with an organization's settings they are not in: read by the Settings
    /// screens alone.
    #[cfg_attr(not(feature = "settings"), allow(dead_code))]
    pub(crate) staff_organizations: Option<StaffOrganizationCheck>,
}

impl Core {
    /// Builds the registry and everything else; fails the boot with the
    /// first problem in a resource or page.
    pub(crate) async fn new(app: AppContext, options: Arc<HelmOptions>) -> Result<Core, BoxError> {
        let registry = Registry::build(&app, options.clone()).await?;
        let app_views = app.try_get::<Views>().await;
        if app_views.is_none() {
            let view_field = registry.resources().iter().find_map(|r| {
                r.info()
                    .fields
                    .iter()
                    .find(|f| matches!(f.kind, FieldKind::View { .. }))
                    .map(|f| (r.info().type_name, f.name.clone()))
            });
            if let Some((resource, field)) = view_field {
                return Err(format!(
                    "Helm resource `{resource}`: view field `{field}` renders an app template, so the app needs ViewModule"
                )
                .into());
            }
            if let Some(page) = registry.pages().first() {
                return Err(format!(
                    "Helm page `{}` renders an app template, so the app needs ViewModule",
                    page.page.slug
                )
                .into());
            }
        }
        let metrics = (options.metrics_cache_secs > 0).then(|| {
            navree_bounded::Cache::builder(10_000)
                .time_to_live(Duration::from_secs(options.metrics_cache_secs))
                .build()
        });
        let counts = (options.metrics_cache_secs > 0).then(|| {
            navree_bounded::Cache::builder(10_000)
                .time_to_live(Duration::from_secs(options.metrics_cache_secs))
                .build()
        });
        let mut policies = app.contributions::<StaffPolicy>().into_iter();
        let staff = policies.next().map(|policy| policy.value.0.clone());
        if policies.next().is_some() {
            return Err("Helm: `helm_staff_workspaces` is registered more than once".into());
        }
        let mut policies = app.contributions::<StaffOrganizationPolicy>().into_iter();
        let staff_organizations = policies.next().map(|policy| policy.value.0.clone());
        if policies.next().is_some() {
            return Err("Helm: `helm_staff_organizations` is registered more than once".into());
        }
        Ok(Core {
            staff,
            staff_organizations,
            messages: save::messages(&app).await,
            registry,
            app_views,
            metrics,
            counts,
            options,
        })
    }
}
