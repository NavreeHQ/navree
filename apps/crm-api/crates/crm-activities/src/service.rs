//! [`ActivityService`]: listing, creating, changing, completing and deleting
//! activities. Writes run in the caller's unit of work (the request's), so
//! the change and its audit rows commit together.

use std::sync::Arc;

use crm_core::models::{Activity, ActivityScopes, Company, Contact, Deal, User};
use crm_core::{ActivityKind, SubjectType};
use navree::access::{Ability, Accessible, PrincipalAccess, Read};
use navree::contracts::Clock;
use navree::orm::access::{AbilityExt, AccessibleQuery};
use navree::orm::list::{ListQuery, ListQueryExt, ListResource};
use navree::orm::{OrmError, Record};
use navree::prelude::*;
use navree::validation::ValidationErrors;
use uuid::Uuid;

use crate::dto::{ActivityList, CreateActivity, UpdateActivity};
use crate::due::{Due, Today};
use crate::resource::ActivityResource;

/// Lists, creates, changes, completes and deletes activities, in the
/// caller's unit of work (the request's), so a change and its audit rows
/// commit together.
#[injectable]
pub struct ActivityService {
    clock: Arc<dyn Clock>,
}

impl ActivityService {
    /// One page of the activities `scope` allows that match `list` (read
    /// against [`ActivityList`]): with `filter[due]`, earliest due first;
    /// with `filter[completed]=true`, most recently completed first;
    /// otherwise newest first.
    pub async fn list(
        &self,
        uow: &Uow,
        scope: Accessible<Activity, Read>,
        list: &ListQuery,
    ) -> Result<Page<ActivityResource>> {
        let today = Today::of(self.clock.now());
        let due = list.eq("due").and_then(Due::from_word);
        let completed = list.eq("completed").map(|word| word == "true");
        let query = scope
            .apply(Activity::query())
            .list(list, &ActivityList::SPEC)?
            .when_some(due, |query, due| due.apply(query, today))
            .when_some(completed, |query, completed| {
                if completed {
                    query.where_not_null(Activity::COMPLETED_AT)
                } else {
                    query.where_null(Activity::COMPLETED_AT)
                }
            });
        let query = if completed == Some(true) {
            query
                .order_by_desc(Activity::COMPLETED_AT)
                .order_by_desc(Activity::ID)
        } else if due.is_some() {
            query.order_by(Activity::DUE_AT).order_by(Activity::ID)
        } else {
            query.newest_first()
        };
        let page = query
            .with_all(ActivityResource::EMBEDS)
            .paginate(uow, list.page_request(&ActivityList::SPEC))
            .await?;
        Ok(page.map(|activity| activity.to_resource::<ActivityResource>()))
    }

    /// Creates an activity owned by the signed-in user. The record it is
    /// attached to must exist (and be readable) and the assignee must be a
    /// user, else a 400.
    pub async fn create(
        &self,
        uow: &Uow,
        access: &PrincipalAccess,
        input: CreateActivity,
    ) -> Result<ActivityResource> {
        let me = access.user.ok_or_else(AppError::unauthenticated)?;
        check_references(
            uow,
            &access.ability,
            Some((input.subject_type, input.subject_id)),
            input.assignee_id,
        )
        .await?;
        let activity = Activity::create(uow, input.into_activity(me)).await?;
        self.present_one(uow, activity).await
    }

    /// Changes the fields present in `input` (checked like on create when
    /// they change what the activity points at); `ability` is the caller's.
    /// A request that changes nothing writes nothing.
    pub async fn update(
        &self,
        uow: &Uow,
        ability: &Ability,
        mut activity: Record<Activity>,
        input: UpdateActivity,
    ) -> Result<ActivityResource> {
        let (subject, assignee) = (
            (activity.subject_type, activity.subject_id),
            activity.assignee_id,
        );
        input.apply_to(&mut activity);
        let moved = (activity.subject_type, activity.subject_id) != subject;
        let reassigned = activity
            .assignee_id
            .filter(|_| activity.assignee_id != assignee);
        check_references(
            uow,
            ability,
            moved.then_some((activity.subject_type, activity.subject_id)),
            reassigned,
        )
        .await?;
        activity.save(uow).await?;
        self.present_one(uow, activity).await
    }

    /// Marks a task completed now; completing it again changes nothing.
    /// Other kinds of activity are not completed (a 400).
    pub async fn complete(
        &self,
        uow: &Uow,
        mut activity: Record<Activity>,
    ) -> Result<ActivityResource> {
        if activity.kind != ActivityKind::Task {
            return Err(AppError::bad_request("Only tasks can be completed"));
        }
        if activity.completed_at.is_none() {
            let now = self.clock.now();
            activity
                .update(uow, |activity| activity.completed_at = Some(now))
                .await?;
        }
        self.present_one(uow, activity).await
    }

    /// Deletes `activity`.
    pub async fn delete(&self, uow: &Uow, mut activity: Record<Activity>) -> Result<()> {
        activity.delete(uow).await?;
        Ok(())
    }

    /// `activity` as a resource, its relations loaded in `uow`'s
    /// transaction.
    pub async fn present_one(
        &self,
        uow: &Uow,
        mut activity: Record<Activity>,
    ) -> Result<ActivityResource> {
        activity.load_all(uow, ActivityResource::EMBEDS).await?;
        Ok(activity.to_resource())
    }
}

/// Checks what an activity points at: the record it is attached to must
/// exist (not soft-deleted) and be readable by `ability`, and the assignee
/// must be a user; a 400 naming the fields otherwise. `None` skips a check.
async fn check_references(
    uow: &Uow,
    ability: &Ability,
    subject: Option<(SubjectType, Uuid)>,
    assignee: Option<Uuid>,
) -> Result<()> {
    let mut errors = ValidationErrors::new();
    if let Some((subject_type, id)) = subject
        && !readable(uow, ability, subject_type, id).await?
    {
        errors.add("subject_id", "subject_id does not exist");
    }
    if let Some(assignee) = assignee
        && User::query().where_key(assignee).doesnt_exist(uow).await?
    {
        errors.add("assignee_id", "assignee_id does not exist");
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.into_app_error())
    }
}

/// Whether the record an activity would be attached to exists (deleted
/// companies and contacts do not: the models leave them out) and `ability`
/// may read it.
async fn readable(
    uow: &Uow,
    ability: &Ability,
    subject_type: SubjectType,
    id: Uuid,
) -> Result<bool, OrmError> {
    Ok(match subject_type {
        SubjectType::Company => Company::find(uow, id)
            .await?
            .is_some_and(|company| ability.allows(Read, &company)),
        SubjectType::Contact => Contact::find(uow, id)
            .await?
            .is_some_and(|contact| ability.allows(Read, &contact)),
        SubjectType::Deal => Deal::find(uow, id)
            .await?
            .is_some_and(|deal| ability.allows(Read, &deal)),
    })
}
