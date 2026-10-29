//! The business logic of projects. Every read and write goes through
//! `WorkspaceRepo`, so it only ever touches the current workspace's rows.

use chrono::Utc;
use navree::access::{Accessible, Read};
use navree::contracts::Broadcaster;
use navree::db::{Page, PageQuery, Uow, paginate};
use navree::injectable;
// COMPAT (data-layer port): the SeaORM forms of the workspace repositories.
use navree::workspace::sea_compat::WorkspaceRepo;
use sea_orm::{ActiveValue::Set, ColumnTrait, DbErr, QueryFilter, QueryOrder};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

use super::dto::{CreateProject, ProjectFilter, ProjectResource, UpdateProject};
use super::entity::{ActiveModel, Column, Model, Project};

/// The real-time event a project change is broadcast as, on the
/// workspace's channel `private-workspace.{id}`:
/// `{"action": "created" | "updated" | "deleted", "project": {..}}`.
pub const PROJECT_CHANGED: &str = "project.changed";

/// Lists, creates, updates and deletes the projects of a workspace.
///
/// Writes run in the caller's unit of work: they commit with the request
/// (or roll back with it), are recorded in the audit log, and are
/// broadcast to the workspace's members once committed.
#[injectable]
pub struct ProjectService {
    broadcaster: Arc<dyn Broadcaster>,
}

impl ProjectService {
    /// One page of the workspace's projects that `scope` lets the user
    /// read, newest first.
    pub async fn list(
        &self,
        uow: &Uow,
        workspace: Uuid,
        scope: Accessible<Project, Read>,
        filter: ProjectFilter,
        page: PageQuery,
    ) -> Result<Page<ProjectResource>, DbErr> {
        let mut select =
            WorkspaceRepo::<Project>::select(workspace).order_by_desc(Column::CreatedAt);
        if let Some(status) = filter.status {
            select = select.filter(Column::Status.eq(status.as_str()));
        }
        if let Some(search) = filter.search.as_deref().map(str::trim)
            && !search.is_empty()
        {
            select = select.filter(Column::Name.contains(search));
        }
        let page = paginate(scope.apply(select), &*uow.tx().await?, page).await?;
        Ok(page.map(ProjectResource::from))
    }

    /// Creates a project in `workspace`, created by `user`.
    pub async fn create(
        &self,
        uow: &Uow,
        workspace: Uuid,
        user: Uuid,
        input: CreateProject,
    ) -> Result<ProjectResource, DbErr> {
        let now = Utc::now();
        let project = WorkspaceRepo::<Project>::insert(
            uow,
            workspace,
            ActiveModel {
                id: Set(Uuid::now_v7()),
                name: Set(input.name),
                description: Set(input.description),
                status: Set(input.status.as_str().to_owned()),
                created_by: Set(user),
                created_at: Set(now),
                updated_at: Set(now),
                // Set by the repository.
                workspace_id: sea_orm::ActiveValue::NotSet,
            },
        )
        .await?;
        let project = ProjectResource::from(project);
        self.broadcast(uow, "created", &project);
        Ok(project)
    }

    /// Applies the fields present in `input`.
    pub async fn update(
        &self,
        uow: &Uow,
        workspace: Uuid,
        project: Model,
        input: UpdateProject,
    ) -> Result<ProjectResource, DbErr> {
        let mut active: ActiveModel = project.into();
        if let Some(name) = input.name {
            active.name = Set(name);
        }
        if let Some(description) = input.description {
            active.description = Set(Some(description));
        }
        if let Some(status) = input.status {
            active.status = Set(status.as_str().to_owned());
        }
        active.updated_at = Set(Utc::now());
        let project =
            ProjectResource::from(WorkspaceRepo::<Project>::update(uow, workspace, active).await?);
        self.broadcast(uow, "updated", &project);
        Ok(project)
    }

    /// Deletes `project`.
    pub async fn delete(&self, uow: &Uow, workspace: Uuid, project: Model) -> Result<(), DbErr> {
        let resource = ProjectResource::from(project.clone());
        WorkspaceRepo::<Project>::delete(uow, workspace, project).await?;
        self.broadcast(uow, "deleted", &resource);
        Ok(())
    }

    /// Broadcasts the change to the workspace's members once the unit of
    /// work commits (never for a change that rolls back). A failed
    /// broadcast is logged: the change itself stands.
    fn broadcast(&self, uow: &Uow, action: &'static str, project: &ProjectResource) {
        let broadcaster = self.broadcaster.clone();
        let channel = format!("private-workspace.{}", project.workspace_id);
        let data = json!({ "action": action, "project": project });
        uow.after_commit(move || {
            Box::pin(async move {
                if let Err(error) = broadcaster
                    .broadcast(vec![channel], PROJECT_CHANGED.to_owned(), data)
                    .await
                {
                    tracing::warn!(%error, "the project.changed broadcast failed");
                }
            })
        });
    }
}
