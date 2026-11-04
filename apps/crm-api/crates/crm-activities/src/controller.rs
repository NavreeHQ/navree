//! `/api/v1/activities`.

use crm_core::{Activity, Complete};
use navree::access::CurrentAccess;
use navree::orm::list::List;
use navree::prelude::*;

use crate::dto::{ActivityList, CreateActivity, UpdateActivity};
use crate::resource::{ActivityPage, ActivityResource};
use crate::service::ActivityService;

/// `/api/v1/activities`: everyone in the CRM reads and creates activities;
/// owners (and managers) change and delete them; assignees, owners and
/// managers complete them.
#[injectable]
pub struct ActivitiesController {
    activities: Arc<ActivityService>,
}

#[controller("/activities", version = "1")]
impl ActivitiesController {
    /// A page of activities matching the filters ([`ActivityList`]).
    #[get("/")]
    #[can(Read, Activity)]
    #[api(
        summary = "List activities",
        description = "Query: `filter[subject_type]`, `filter[subject_id]`, `filter[kind]`, `filter[assignee_id]`, `filter[due]` (overdue | today | upcoming, by UTC day), `filter[completed]` (true | false), `page`, `per_page` (default 15, at most 100). With `filter[due]`, earliest due first; with `filter[completed]=true`, most recently completed first; otherwise newest first.",
        tag = "Activities",
        response = ActivityPage,
        responses(400, 401, 403)
    )]
    async fn index(
        &self,
        uow: Uow,
        scope: Accessible<Activity, Read>,
        list: List<ActivityList>,
    ) -> Result<Page<ActivityResource>> {
        self.activities.list(&uow, scope, &list).await
    }

    /// Records an activity, owned by the signed-in user.
    #[post("/")]
    #[can(Create, Activity)]
    #[api(
        summary = "Create an activity",
        tag = "Activities",
        request = CreateActivity,
        response = ActivityResource,
        status = 201,
        responses(400, 401, 403)
    )]
    async fn store(
        &self,
        uow: Uow,
        CurrentAccess(access): CurrentAccess,
        Valid(Json(input)): Valid<Json<CreateActivity>>,
    ) -> Result<Created<ActivityResource>> {
        let activity = self.activities.create(&uow, &access, input).await?;
        Ok(Created(
            format!("/api/v1/activities/{}", activity.id),
            activity,
        ))
    }

    /// Changes the fields sent.
    #[patch("/{id}")]
    #[can(Update, Activity, from = "id")]
    #[api(
        summary = "Update an activity",
        tag = "Activities",
        request = UpdateActivity,
        response = ActivityResource,
        responses(400, 401, 403, 404)
    )]
    async fn update(
        &self,
        uow: Uow,
        activity: Authorized<Activity, Update>,
        Valid(Json(input)): Valid<Json<UpdateActivity>>,
    ) -> Result<Json<ActivityResource>> {
        let ability = activity.ability().clone();
        let updated = self
            .activities
            .update(&uow, &ability, activity.into_inner(), input)
            .await?;
        Ok(Json(updated))
    }

    /// Marks a task completed (the assignee, the owner or a
    /// manager).
    #[post("/{id}/complete")]
    #[can(Complete, Activity, from = "id")]
    #[api(
        summary = "Complete a task",
        tag = "Activities",
        response = ActivityResource,
        responses(400, 401, 403, 404)
    )]
    async fn complete(
        &self,
        uow: Uow,
        activity: Authorized<Activity, Complete>,
    ) -> Result<Json<ActivityResource>> {
        Ok(Json(
            self.activities
                .complete(&uow, activity.into_inner())
                .await?,
        ))
    }

    /// Deletes an activity.
    #[delete("/{id}")]
    #[can(Delete, Activity, from = "id")]
    #[api(
        summary = "Delete an activity",
        tag = "Activities",
        status = 204,
        responses(401, 403, 404)
    )]
    async fn destroy(&self, uow: Uow, activity: Authorized<Activity, Delete>) -> Result<NoContent> {
        self.activities.delete(&uow, activity.into_inner()).await?;
        Ok(NoContent)
    }
}
