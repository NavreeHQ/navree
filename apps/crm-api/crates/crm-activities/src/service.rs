//! [`ActivityService`]: listing, creating, changing, completing and deleting
//! activities. Writes run in the caller's unit of work (the request's), so
//! the change and its audit rows commit together.

use std::sync::Arc;

use crm_core::entities::{activity, company, contact, deal};
use crm_core::{Activity, ActivityKind, SubjectType};
use keel::access::{Ability, Accessible, PrincipalAccess, READ, Read, record_json};
use keel::auth::user;
use keel::contracts::Clock;
use keel::db::paginate;
use keel::prelude::*;
use keel::validation::ValidationErrors;
use sea_orm::ActiveValue::{Set, Unchanged};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DbErr, EntityTrait, IntoActiveModel,
    PaginatorTrait, QueryFilter, QueryOrder,
};
use serde_json::Value;
use uuid::Uuid;

use crate::dto::{ActivityFilter, CreateActivity, UpdateActivity};
use crate::due::Today;
use crate::resource::{ActivityResource, present};

/// Activities; see the [module docs](self).
#[injectable]
pub struct ActivityService {
    clock: Arc<dyn Clock>,
}

impl ActivityService {
    /// One page of the activities `scope` allows that match `filter`:
    /// with `due`, earliest due first; with `completed=true`, most recently
    /// completed first; otherwise newest first.
    pub async fn list(
        &self,
        uow: &Uow,
        scope: Accessible<Activity, Read>,
        filter: ActivityFilter,
        page: PageQuery,
    ) -> Result<Page<ActivityResource>> {
        let mut select = scope.apply(activity::Entity::find());
        if let Some(subject_type) = filter.subject_type {
            select = select.filter(activity::Column::SubjectType.eq(subject_type));
        }
        if let Some(subject_id) = filter.subject_id {
            select = select.filter(activity::Column::SubjectId.eq(subject_id));
        }
        if let Some(kind) = filter.kind {
            select = select.filter(activity::Column::Kind.eq(kind));
        }
        if let Some(assignee) = filter.assignee_id {
            select = select.filter(activity::Column::AssigneeId.eq(assignee));
        }
        if let Some(due) = filter.due {
            select = select.filter(due.condition(Today::of(self.clock.now())));
        }
        select = match filter.completed {
            Some(true) => select.filter(activity::Column::CompletedAt.is_not_null()),
            Some(false) => select.filter(activity::Column::CompletedAt.is_null()),
            None => select,
        };
        let select = if filter.completed == Some(true) {
            select
                .order_by_desc(activity::Column::CompletedAt)
                .order_by_desc(activity::Column::Id)
        } else if filter.due.is_some() {
            select
                .order_by_asc(activity::Column::DueAt)
                .order_by_asc(activity::Column::Id)
        } else {
            select
                .order_by_desc(activity::Column::CreatedAt)
                .order_by_desc(activity::Column::Id)
        };
        let tx = uow.tx().await?;
        let page = paginate(select, &*tx, page).await?;
        let data = present(&*tx, page.data).await?;
        Ok(Page {
            data,
            meta: page.meta,
        })
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
        let now = self.clock.now();
        let model = Repo::<activity::Entity>::insert(
            uow,
            activity::ActiveModel {
                id: Set(Uuid::now_v7()),
                kind: Set(input.kind),
                subject: Set(input.subject),
                body: Set(non_blank(input.body)),
                due_at: Set(input.due_at),
                completed_at: Set(None),
                subject_type: Set(input.subject_type),
                subject_id: Set(input.subject_id),
                owner_id: Set(me),
                assignee_id: Set(input.assignee_id),
                created_at: Set(now),
                updated_at: Set(now),
            },
        )
        .await?;
        self.present_one(uow, model).await
    }

    /// Changes the fields present in `input` (checked like on create when
    /// they change what the activity points at); `ability` is the caller's.
    pub async fn update(
        &self,
        uow: &Uow,
        ability: &Ability,
        current: activity::Model,
        input: UpdateActivity,
    ) -> Result<ActivityResource> {
        let mut next = current.clone();
        if let Some(kind) = input.kind {
            next.kind = kind;
        }
        if let Some(subject) = input.subject {
            next.subject = subject;
        }
        if let Some(body) = input.body {
            next.body = non_blank(body);
        }
        if let Some(due_at) = input.due_at {
            next.due_at = due_at;
        }
        if let Some(subject_type) = input.subject_type {
            next.subject_type = subject_type;
        }
        if let Some(subject_id) = input.subject_id {
            next.subject_id = subject_id;
        }
        if let Some(assignee) = input.assignee_id {
            next.assignee_id = assignee;
        }
        let moved =
            (next.subject_type, next.subject_id) != (current.subject_type, current.subject_id);
        let reassigned = next
            .assignee_id
            .filter(|_| next.assignee_id != current.assignee_id);
        check_references(
            uow,
            ability,
            moved.then_some((next.subject_type, next.subject_id)),
            reassigned,
        )
        .await?;
        if next == current {
            return self.present_one(uow, current).await;
        }
        next.updated_at = self.clock.now();
        let model =
            Repo::<activity::Entity>::update(uow, next.into_active_model().reset_all()).await?;
        self.present_one(uow, model).await
    }

    /// Marks a task completed now; completing it again changes nothing.
    /// Other kinds of activity are not completed (a 400).
    pub async fn complete(&self, uow: &Uow, activity: activity::Model) -> Result<ActivityResource> {
        if activity.kind != ActivityKind::Task {
            return Err(AppError::bad_request("Only tasks can be completed"));
        }
        if activity.completed_at.is_some() {
            return self.present_one(uow, activity).await;
        }
        let now = self.clock.now();
        let model = Repo::<activity::Entity>::update(
            uow,
            activity::ActiveModel {
                id: Unchanged(activity.id),
                completed_at: Set(Some(now)),
                updated_at: Set(now),
                ..Default::default()
            },
        )
        .await?;
        self.present_one(uow, model).await
    }

    /// Deletes `activity`.
    pub async fn delete(&self, uow: &Uow, activity: activity::Model) -> Result<()> {
        Repo::<activity::Entity>::delete(uow, activity).await?;
        Ok(())
    }

    /// `activity` as a resource, read in `uow`'s transaction.
    pub async fn present_one(
        &self,
        uow: &Uow,
        activity: activity::Model,
    ) -> Result<ActivityResource> {
        let tx = uow.tx().await?;
        let mut resources = present(&*tx, vec![activity]).await?;
        resources
            .pop()
            .ok_or_else(|| AppError::internal("an activity was not presented"))
    }
}

/// `None` for a missing or blank text.
fn non_blank(text: Option<String>) -> Option<String> {
    text.filter(|text| !text.trim().is_empty())
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
    let tx = uow.tx().await?;
    let mut errors = ValidationErrors::new();
    if let Some((subject_type, id)) = subject {
        let readable = subject_record(&*tx, subject_type, id)
            .await?
            .is_some_and(|record| ability.can(READ, subject_type.subject(), Some(&record)));
        if !readable {
            errors.add("subject_id", "subject_id does not exist");
        }
    }
    if let Some(assignee) = assignee {
        let exists = user::Entity::find_by_id(assignee).count(&*tx).await? > 0;
        if !exists {
            errors.add("assignee_id", "assignee_id does not exist");
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.into_app_error())
    }
}

/// The record an activity would be attached to, as access conditions see
/// it; `None` when it does not exist or is soft-deleted.
async fn subject_record<C: ConnectionTrait>(
    conn: &C,
    subject_type: SubjectType,
    id: Uuid,
) -> Result<Option<Value>, DbErr> {
    Ok(match subject_type {
        SubjectType::Company => company::Entity::find_by_id(id)
            .filter(company::Column::DeletedAt.is_null())
            .one(conn)
            .await?
            .map(|model| record_json(&model)),
        SubjectType::Contact => contact::Entity::find_by_id(id)
            .filter(contact::Column::DeletedAt.is_null())
            .one(conn)
            .await?
            .map(|model| record_json(&model)),
        SubjectType::Deal => deal::Entity::find_by_id(id)
            .one(conn)
            .await?
            .map(|model| record_json(&model)),
    })
}
