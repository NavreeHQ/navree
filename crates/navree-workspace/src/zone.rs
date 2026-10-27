//! [`WorkspaceZones`]: the workspaces' time zones for navree-time.

use std::sync::Arc;

use http::request::Parts;
use navree_contracts::{ZoneLevel, ZoneOwner, ZoneRequest, ZoneSource};
use navree_kernel::{BoxError, BoxFuture};
use uuid::Uuid;

use crate::Workspaces;
use crate::current::try_resolve;
use crate::entity::Workspace;

/// The `dyn ZoneSource` of workspaces ([`ZoneLevel::Workspace`]),
/// registered by `WorkspaceModule`: a request's workspace is the one
/// [`CurrentWorkspace`](crate::CurrentWorkspace) resolves (when the request
/// names one the signed-in user belongs to), with its `timezone` and
/// `country` columns.
pub struct WorkspaceZones {
    workspaces: Arc<Workspaces>,
}

impl WorkspaceZones {
    pub(crate) fn new(workspaces: Arc<Workspaces>) -> Self {
        WorkspaceZones { workspaces }
    }
}

fn owner(workspace: &Workspace) -> ZoneOwner {
    ZoneOwner {
        id: workspace.id,
        zone: workspace.timezone.clone(),
        country: workspace.country.clone(),
    }
}

impl ZoneSource for WorkspaceZones {
    fn level(&self) -> ZoneLevel {
        ZoneLevel::Workspace
    }

    fn zone_of_request<'a>(
        &'a self,
        request: &'a mut ZoneRequest<'_>,
    ) -> BoxFuture<'a, Result<Option<ZoneOwner>, BoxError>> {
        Box::pin(async move {
            let Some(parts) = request.head::<Parts>() else {
                return Ok(None);
            };
            Ok(try_resolve(parts)
                .await
                .map(|current| owner(current.workspace())))
        })
    }

    fn zone_of(&self, id: Uuid) -> BoxFuture<'_, Result<Option<ZoneOwner>, BoxError>> {
        Box::pin(async move {
            Ok(self
                .workspaces
                .find(id)
                .await?
                .map(|workspace| owner(&workspace)))
        })
    }

    fn owners(&self) -> BoxFuture<'_, Result<Vec<ZoneOwner>, BoxError>> {
        Box::pin(async move { Ok(self.workspaces.zone_owners().await?) })
    }
}
