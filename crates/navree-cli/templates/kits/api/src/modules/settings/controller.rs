//! `/api/v1/settings`: the current workspace's settings and the member's
//! notification preferences. Plans, checkout, the customer portal and
//! invoices are BillingModule's routes (`/api/billing/*`), for the same
//! workspace.

use navree::billing::Billing;
use navree::prelude::*;
use navree::queue::Queue;

use super::dto::{NotificationSettings, UpdateNotifications, UpdateWorkspace, WorkspaceSettings};
use super::service::SettingsService;

/// The settings routes, inside the request's workspace (`X-Workspace`).
#[injectable]
pub struct SettingsController {
    settings: Arc<SettingsService>,
    billing: Arc<Billing>,
    queue: Arc<Queue>,
}

#[controller("/settings", version = "1")]
#[guard(InWorkspace::guard())]
impl SettingsController {
    /// `GET /api/v1/settings/workspace`
    #[get("/workspace")]
    #[permissions("workspace.read")]
    #[api(
        summary = "Show the workspace's settings",
        tag = "Settings",
        response = WorkspaceSettings,
        responses(401, 403)
    )]
    async fn workspace(&self, ws: CurrentWorkspace) -> Json<WorkspaceSettings> {
        Json(ws.workspace().clone().into())
    }

    /// `PATCH /api/v1/settings/workspace`: the name and the slug.
    #[patch("/workspace")]
    #[permissions("workspace.update")]
    #[api(
        summary = "Change the workspace's name or slug",
        tag = "Settings",
        request = UpdateWorkspace,
        response = WorkspaceSettings,
        responses(400, 401, 403)
    )]
    async fn update_workspace(
        &self,
        uow: Uow,
        ws: CurrentWorkspace,
        Valid(Json(input)): Valid<Json<UpdateWorkspace>>,
    ) -> Result<Json<WorkspaceSettings>> {
        let workspace = self
            .settings
            .update_workspace(&uow, ws.workspace().clone(), input)
            .await?;
        Ok(Json(workspace.into()))
    }

    /// `GET /api/v1/settings/abilities`: the caller's rules in this
    /// workspace (CASL's JSON rule array: the permissions of their role plus
    /// the conditions of `src/abilities.rs`), so the front end shows only
    /// what the API allows. `GET /api/me/abilities` has the global ones.
    #[get("/abilities")]
    #[authenticated]
    #[api(
        summary = "My rules in this workspace",
        tag = "Settings",
        responses(401, 403)
    )]
    async fn abilities(&self, ws: CurrentWorkspace) -> Json<serde_json::Value> {
        Json(ws.ability().to_casl_json())
    }

    /// `GET /api/v1/settings/notifications`: the caller's preferences here.
    #[get("/notifications")]
    #[authenticated]
    #[api(
        summary = "Show my notification preferences",
        tag = "Settings",
        response = NotificationSettings,
        responses(401, 403)
    )]
    async fn notifications(
        &self,
        uow: WorkspaceUow,
        ws: CurrentWorkspace,
    ) -> Result<Json<NotificationSettings>> {
        let settings = self
            .settings
            .notifications(&uow, ws.id(), ws.user_id())
            .await?;
        Ok(Json(settings))
    }

    /// `PUT /api/v1/settings/notifications`
    #[put("/notifications")]
    #[authenticated]
    #[api(
        summary = "Change my notification preferences",
        tag = "Settings",
        request = UpdateNotifications,
        response = NotificationSettings,
        responses(400, 401, 403)
    )]
    async fn update_notifications(
        &self,
        uow: WorkspaceUow,
        ws: CurrentWorkspace,
        Valid(Json(input)): Valid<Json<UpdateNotifications>>,
    ) -> Result<Json<NotificationSettings>> {
        let settings = self
            .settings
            .update_notifications(&uow, ws.id(), ws.user_id(), input)
            .await?;
        Ok(Json(settings))
    }

    /// `POST /api/v1/settings/billing/fake-checkout/{id}`: completes a
    /// checkout of the fake provider as if the customer paid, then runs the
    /// queued billing jobs, so the subscription shows at once. Development
    /// only: a 404 unless `billing.provider = "fake"`.
    #[post("/billing/fake-checkout/{id}")]
    #[permissions("billing.manage")]
    #[api(
        summary = "Complete a fake checkout (development)",
        tag = "Settings",
        status = 204,
        responses(401, 403, 404)
    )]
    async fn complete_fake_checkout(&self, Path(id): Path<String>) -> Result<NoContent> {
        let Some(fake) = self.billing.fake() else {
            return Err(AppError::not_found());
        };
        fake.complete_checkout(&id)
            .await
            .map_err(|error| AppError::bad_request(error.to_string()))?;
        self.queue.run_pending().await?;
        Ok(NoContent)
    }
}
