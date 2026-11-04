//! `/api/v1/projects`: the current workspace's projects.

use navree::orm::list::List;
use navree::prelude::*;

use super::dto::{CreateProject, ProjectList, ProjectPage, ProjectResource, UpdateProject};
use super::entity::Project;
use super::service::ProjectService;

/// The project routes. The `InWorkspace` guard resolves the workspace from
/// the request (`X-Workspace`) and checks that the user is a member before
/// the access rules run, so `#[can]` and `Accessible` decide with the
/// user's role in that workspace. Routes with `from = "id"` load the
/// record first: another workspace's project is a 404.
#[injectable]
pub struct ProjectsController {
    projects: Arc<ProjectService>,
}

#[controller("/projects", version = "1")]
#[guard(InWorkspace::guard())]
impl ProjectsController {
    /// `GET /api/v1/projects`: the current workspace's projects, newest
    /// first; `?filter[status]=active&search=web&sort=name&page=2&per_page=20`
    /// (see [`ProjectList`]).
    #[get("/")]
    #[can(Read, Project)]
    #[api(
        summary = "List projects",
        tag = "Projects",
        response = ProjectPage,
        responses(400, 401, 403)
    )]
    async fn index(
        &self,
        uow: WorkspaceUow,
        ws: CurrentWorkspace,
        scope: Accessible<Project, Read>,
        list: List<ProjectList>,
    ) -> Result<Page<ProjectResource>> {
        Ok(self.projects.list(&uow, ws.id(), scope, &list).await?)
    }

    /// `POST /api/v1/projects`
    #[post("/")]
    #[can(Create, Project)]
    #[api(
        summary = "Create a project",
        tag = "Projects",
        request = CreateProject,
        response = ProjectResource,
        status = 201,
        responses(400, 401, 403)
    )]
    async fn store(
        &self,
        uow: WorkspaceUow,
        ws: CurrentWorkspace,
        CurrentUser(user): CurrentUser,
        Valid(Json(input)): Valid<Json<CreateProject>>,
    ) -> Result<Created<ProjectResource>> {
        let project = self.projects.create(&uow, ws.id(), user.id, input).await?;
        Ok(Created(format!("/api/v1/projects/{}", project.id), project))
    }

    /// `GET /api/v1/projects/{id}`
    #[get("/{id}")]
    #[can(Read, Project, from = "id")]
    #[api(
        summary = "Show a project",
        tag = "Projects",
        response = ProjectResource,
        responses(401, 403, 404)
    )]
    async fn show(&self, project: Authorized<Project, Read>) -> Json<ProjectResource> {
        Json(ProjectResource::from(&*project.into_inner()))
    }

    /// `PATCH /api/v1/projects/{id}`: changes the fields sent.
    #[patch("/{id}")]
    #[can(Update, Project, from = "id")]
    #[api(
        summary = "Update a project",
        tag = "Projects",
        request = UpdateProject,
        response = ProjectResource,
        responses(400, 401, 403, 404)
    )]
    async fn update(
        &self,
        uow: WorkspaceUow,
        ws: CurrentWorkspace,
        project: Authorized<Project, Update>,
        Valid(Json(input)): Valid<Json<UpdateProject>>,
    ) -> Result<Json<ProjectResource>> {
        project.check_fields(&input.changed_fields())?;
        let project = self
            .projects
            .update(&uow, ws.id(), project.into_inner(), input)
            .await?;
        Ok(Json(project))
    }

    /// `DELETE /api/v1/projects/{id}`
    #[delete("/{id}")]
    #[can(Delete, Project, from = "id")]
    #[api(
        summary = "Delete a project",
        tag = "Projects",
        status = 204,
        responses(401, 403, 404)
    )]
    async fn destroy(
        &self,
        uow: WorkspaceUow,
        ws: CurrentWorkspace,
        project: Authorized<Project, Delete>,
    ) -> Result<NoContent> {
        self.projects
            .delete(&uow, ws.id(), project.into_inner())
            .await?;
        Ok(NoContent)
    }
}
