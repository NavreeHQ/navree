//! [`HelmActionJob`]: a queued action (`Action::queued`).

use navree_access::AbilityFactory;
use navree_contracts::{AuthVia, Job, JobContext, Principal};
use navree_db::Db;
use navree_kernel::BoxError;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::core::Core;
use crate::cx::{Client, Cx};

/// The `helm.action` job: runs a queued action of a resource on the
/// records the user picked, as that user. Their access is checked again
/// when the job runs, so records they may no longer act on are left out.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HelmActionJob {
    /// The resource's slug.
    pub resource: String,
    /// The action's name.
    pub action: String,
    /// The record keys.
    pub ids: Vec<String>,
    /// The values of the action's form.
    pub input: Map<String, Value>,
    /// The user who ran it.
    pub user_id: Uuid,
    /// The workspace it was started in, which it runs in again.
    #[serde(default)]
    pub workspace: Option<Uuid>,
}

impl Job for HelmActionJob {
    const NAME: &'static str = "helm.action";

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        let core = ctx.app.get::<Core>().await?;
        let handle = core
            .registry
            .by_slug(&self.resource)
            .ok_or_else(|| format!("helm.action: no resource `{}`", self.resource))?;
        let db = ctx.app.get::<Db>().await?;
        let factory = ctx.app.get::<AbilityFactory>().await?;
        let principal = Principal {
            user_id: self.user_id,
            via: AuthVia::Token,
            token_abilities: None,
            session_id: None,
        };
        let access = factory.access_for(Some(&principal)).await?;
        let uow = db
            .uow(Some(self.user_id), Some(format!("job-{}", ctx.job_id)))
            .await;
        if let Some(workspace) = self.workspace {
            crate::workspace::enter(&uow, Some(workspace))
                .await
                .map_err(|error| {
                    format!(
                        "helm.action: could not enter the workspace: {}",
                        error.title
                    )
                })?;
        }
        let cx = Cx {
            app: ctx.app.clone(),
            uow: uow.clone(),
            access,
            user_id: self.user_id,
            core: core.clone(),
            client: Client::default(),
            tz_offset: None,
        };
        let outcome = uow
            .scope(handle.run_action(&cx, &self.action, self.ids, self.input, true))
            .await;
        match outcome {
            Ok(_) => {
                uow.commit().await?;
                Ok(())
            }
            Err(error) => {
                if let Err(rollback) = uow.rollback().await {
                    tracing::warn!(target: "navree::helm", error = %rollback, "rolling back a failed action failed");
                }
                Err(format!(
                    "helm.action `{}` on `{}` failed: {} {}",
                    self.action,
                    self.resource,
                    error.status.as_u16(),
                    error.detail.as_deref().unwrap_or(&error.title)
                )
                .into())
            }
        }
    }
}
