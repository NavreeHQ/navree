//! The business logic of projects. Every read and write goes through
//! `WorkspaceRepo`, so it only ever touches the named workspace's rows.

use navree::access::{Accessible, Read};
use navree::contracts::Broadcaster;
use navree::db::{Page, PageQuery, Uow};
use navree::injectable;
use navree::orm::access::AccessibleQuery;
use navree::orm::prelude::*;
use navree::workspace::WorkspaceRepo;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

use super::dto::{CreateProject, ProjectFilter, ProjectResource, UpdateProject};
use super::entity::Project;

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
    ) -> Result<Page<ProjectResource>, OrmError> {
        let mut query = WorkspaceRepo::<Project>::query(workspace).order_by_desc(Project::CREATED_AT);
        if let Some(status) = filter.status {
            query = query.where_(Project::STATUS.eq(status.as_str()));
        }
        if let Some(search) = filter.search.as_deref().map(str::trim)
            && !search.is_empty()
        {
            query = query.where_(Project::NAME.like(format!("%{search}%")));
        }
        let page = scope.apply(query).paginate(uow, page).await?;
        Ok(page.map(ProjectResource::from))
    }

    /// Creates a project in `workspace`, created by `user`.
    pub async fn create(
        &self,
        uow: &Uow,
        workspace: Uuid,
        user: Uuid,
        input: CreateProject,
    ) -> Result<ProjectResource, OrmError> {
        let project = WorkspaceRepo::<Project>::insert(
            uow,
            workspace,
            Project {
                id: Uuid::now_v7(),
                name: input.name,
                description: input.description,
                status: input.status.as_str().to_owned(),
                created_by: user,
                // The workspace is set by the repository, the times by the
                // model.
                ..Project::default()
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
        mut project: Record<Project>,
        input: UpdateProject,
    ) -> Result<ProjectResource, OrmError> {
        if let Some(name) = input.name {
            project.name = name;
        }
        if let Some(description) = input.description {
            project.description = Some(description);
        }
        if let Some(status) = input.status {
            project.status = status.as_str().to_owned();
        }
        WorkspaceRepo::<Project>::update(uow, workspace, &mut project).await?;
        let project = ProjectResource::from(project);
        self.broadcast(uow, "updated", &project);
        Ok(project)
    }

    /// Deletes `project`.
    pub async fn delete(
        &self,
        uow: &Uow,
        workspace: Uuid,
        project: Record<Project>,
    ) -> Result<(), OrmError> {
        let resource = ProjectResource::from(project.model().clone());
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
