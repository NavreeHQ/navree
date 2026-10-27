//! [`WorkspaceZones`] and [`OrganizationZones`]: the time zones of
//! workspaces and organizations, for navree-time.

use std::sync::Arc;

use http::request::Parts;
use navree_contracts::{ZoneLevel, ZoneOwner, ZoneRequest, ZoneSource};
use navree_kernel::{BoxError, BoxFuture};
use uuid::Uuid;

use crate::current::try_resolve;
use crate::entity::{Organization, Workspace};
use crate::org_current::try_organization;
use crate::{Organizations, Workspaces};

/// The `dyn ZoneSource` of workspaces ([`ZoneLevel::Workspace`]),
/// registered by `WorkspaceModule`: a request's workspace is the one
/// [`CurrentWorkspace`](crate::CurrentWorkspace) resolves (when the request
/// names one the signed-in user may enter), with its `timezone` and
/// `country` columns.
///
/// Outside a request ([`zone_of`](ZoneSource::zone_of) and
/// [`owners`](ZoneSource::owners), which jobs and per-workspace schedules
/// read), a workspace with neither a zone nor a country takes its
/// organization's, so it runs on the same clock as its requests do.
pub struct WorkspaceZones {
    workspaces: Arc<Workspaces>,
}

impl WorkspaceZones {
    pub(crate) fn new(workspaces: Arc<Workspaces>) -> Self {
        WorkspaceZones { workspaces }
    }
}

fn workspace_owner(workspace: &Workspace) -> ZoneOwner {
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
                .map(|current| workspace_owner(current.workspace())))
        })
    }

    fn zone_of(&self, id: Uuid) -> BoxFuture<'_, Result<Option<ZoneOwner>, BoxError>> {
        Box::pin(async move { Ok(self.workspaces.zone_owner(id).await?) })
    }

    fn owners(&self) -> BoxFuture<'_, Result<Vec<ZoneOwner>, BoxError>> {
        Box::pin(async move { Ok(self.workspaces.zone_owners().await?) })
    }
}

/// The `dyn ZoneSource` of organizations ([`ZoneLevel::Organization`]),
/// registered by `WorkspaceModule` and silent unless
/// `workspace.organizations.enabled`: a request's organization is the one
/// its workspace is attached to, or, in a request without a workspace, the
/// one it names (`X-Organization`, a path prefix, a subdomain or a claim)
/// when the signed-in user is a member. It reads the organizations'
/// `timezone` and `country` columns.
pub struct OrganizationZones {
    organizations: Arc<Organizations>,
}

impl OrganizationZones {
    pub(crate) fn new(organizations: Arc<Organizations>) -> Self {
        OrganizationZones { organizations }
    }
}

fn organization_owner(organization: &Organization) -> ZoneOwner {
    ZoneOwner {
        id: organization.id,
        zone: organization.timezone.clone(),
        country: organization.country.clone(),
    }
}

impl ZoneSource for OrganizationZones {
    fn level(&self) -> ZoneLevel {
        ZoneLevel::Organization
    }

    fn zone_of_request<'a>(
        &'a self,
        request: &'a mut ZoneRequest<'_>,
    ) -> BoxFuture<'a, Result<Option<ZoneOwner>, BoxError>> {
        Box::pin(async move {
            if !self.organizations.enabled() {
                return Ok(None);
            }
            let Some(parts) = request.head::<Parts>() else {
                return Ok(None);
            };
            let organization = match try_resolve(parts).await {
                Some(current) => match current.workspace().organization_id {
                    Some(id) => self.organizations.find(id).await?,
                    None => None,
                },
                None => try_organization(parts).await,
            };
            Ok(organization.as_ref().map(organization_owner))
        })
    }

    fn zone_of(&self, id: Uuid) -> BoxFuture<'_, Result<Option<ZoneOwner>, BoxError>> {
        Box::pin(async move {
            if !self.organizations.enabled() {
                return Ok(None);
            }
            Ok(self
                .organizations
                .find(id)
                .await?
                .as_ref()
                .map(organization_owner))
        })
    }

    fn owners(&self) -> BoxFuture<'_, Result<Vec<ZoneOwner>, BoxError>> {
        Box::pin(async move {
            if !self.organizations.enabled() {
                return Ok(Vec::new());
            }
            Ok(self.organizations.zone_owners().await?)
        })
    }
}
