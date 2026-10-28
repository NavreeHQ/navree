//! The admin panel's resources (Helm at `/admin`): users, workspaces and
//! projects, with dashboard metrics, so the panel shows the evidence data.

use navree::BoxFuture;
use navree::auth::user;
use navree::helm::{Field, FormData, Metric, Resource, SaveContext};
use navree::prelude::*;
use navree::workspace::entity::workspace;

use crate::modules::projects::entity as project;

/// Users: every account, searchable by name and email.
#[injectable]
pub struct UserAdmin;

impl Resource for UserAdmin {
    type Entity = user::Entity;

    const LABEL: &'static str = "Users";
    const SINGULAR: &'static str = "User";
    const GROUP: &'static str = "Accounts";
    const ICON: &'static str = "users";
    const TITLE: &'static str = "name";
    const SEARCH: &'static [&'static str] = &["name", "email"];

    fn fields(&self) -> Vec<Field> {
        vec![
            Field::text("name").required().max_length(100).sortable(),
            Field::email("email").readonly().sortable(),
            Field::datetime("email_verified_at")
                .label("Verified")
                .readonly(),
            Field::datetime("created_at").readonly().sortable(),
        ]
    }

    fn metrics(&self) -> Vec<Metric<user::Entity>> {
        vec![
            Metric::value("Users").count().on_dashboard(),
            Metric::trend("Sign-ups")
                .count()
                .by_day(user::Column::CreatedAt, 30),
        ]
    }

    /// Accounts sign up through the API (`POST /api/auth/register`).
    fn create<'a>(
        &'a self,
        _ctx: &'a SaveContext,
        _form: FormData,
    ) -> BoxFuture<'a, Result<user::Model>> {
        Box::pin(async move { Err(AppError::bad_request("Accounts sign up in the app.")) })
    }
}

/// Workspaces.
#[injectable]
pub struct WorkspaceAdmin;

impl Resource for WorkspaceAdmin {
    type Entity = workspace::Entity;

    const LABEL: &'static str = "Workspaces";
    const SINGULAR: &'static str = "Workspace";
    const GROUP: &'static str = "Accounts";
    const ICON: &'static str = "building-2";
    const TITLE: &'static str = "name";
    const SEARCH: &'static [&'static str] = &["name", "slug"];

    fn fields(&self) -> Vec<Field> {
        vec![
            Field::text("name").required().max_length(100).sortable(),
            Field::text("slug").readonly().sortable(),
            Field::belongs_to::<UserAdmin>("owner_id")
                .label("Owner")
                .readonly(),
            Field::datetime("created_at").readonly().sortable(),
        ]
    }

    fn metrics(&self) -> Vec<Metric<workspace::Entity>> {
        vec![Metric::value("Workspaces").count().on_dashboard()]
    }

    /// Workspaces are created by their owners (`POST /api/workspaces`).
    fn create<'a>(
        &'a self,
        _ctx: &'a SaveContext,
        _form: FormData,
    ) -> BoxFuture<'a, Result<workspace::Model>> {
        Box::pin(async move {
            Err(AppError::bad_request(
                "Workspaces are created by their owners in the app.",
            ))
        })
    }
}

/// Projects, across every workspace.
#[injectable]
pub struct ProjectAdmin;

impl Resource for ProjectAdmin {
    type Entity = project::Entity;

    const LABEL: &'static str = "Projects";
    const SINGULAR: &'static str = "Project";
    const GROUP: &'static str = "Work";
    const ICON: &'static str = "folder";
    const TITLE: &'static str = "name";
    const SEARCH: &'static [&'static str] = &["name", "description"];

    fn fields(&self) -> Vec<Field> {
        vec![
            Field::text("name").required().max_length(120).sortable(),
            Field::belongs_to::<WorkspaceAdmin>("workspace_id")
                .label("Workspace")
                .readonly(),
            Field::select("status")
                .options([("active", "Active"), ("archived", "Archived")])
                .sortable(),
            Field::textarea("description").hide_on_index(),
            Field::datetime("created_at").readonly().sortable(),
        ]
    }

    fn metrics(&self) -> Vec<Metric<project::Entity>> {
        vec![
            Metric::value("Projects").count().on_dashboard(),
            Metric::partition("By status")
                .count()
                .group_by(project::Column::Status)
                .on_dashboard(),
        ]
    }

    /// Projects are created in their workspace (`POST /api/v1/projects`).
    fn create<'a>(
        &'a self,
        _ctx: &'a SaveContext,
        _form: FormData,
    ) -> BoxFuture<'a, Result<project::Model>> {
        Box::pin(async move {
            Err(AppError::bad_request(
                "Projects are created in their workspace.",
            ))
        })
    }
}

/// The admin panel's resources.
#[module(register = Self::wire)]
pub struct AdminModule;

impl AdminModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.resource::<UserAdmin>()
            .resource::<WorkspaceAdmin>()
            .resource::<ProjectAdmin>();
    }
}
