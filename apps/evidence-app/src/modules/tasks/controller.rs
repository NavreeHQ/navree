//! `/api/v1/tasks`: the current workspace's tasks.

use chrono::NaiveDate;
use garde::Validate;
use navree::orm::access::AccessibleQuery;
use navree::orm::prelude::*;
use navree::prelude::*;
use serde::Deserialize;
use utoipa::ToSchema;
use uuid::Uuid;

use super::model::{Task, TaskStatus};

/// The body of `POST /api/v1/tasks`.
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct CreateTask {
    /// The project (in the same workspace).
    #[garde(skip)]
    pub project_id: Uuid,
    /// 1 to 200 characters.
    #[garde(length(min = 1, max = 200))]
    pub title: String,
    /// `todo` (the default), `doing` or `done`.
    #[garde(skip)]
    #[serde(default)]
    pub status: TaskStatus,
    /// 1 (highest) to 4; 3 by default.
    #[garde(range(min = 1, max = 4))]
    #[serde(default = "default_priority")]
    pub priority: i32,
    /// When it is due.
    #[garde(skip)]
    pub due_on: Option<NaiveDate>,
}

fn default_priority() -> i32 {
    3
}

/// The body of `PATCH /api/v1/tasks/{id}`: only the fields present change.
#[derive(Debug, Default, Deserialize, Validate, ToSchema)]
pub struct UpdateTask {
    /// 1 to 200 characters.
    #[garde(length(min = 1, max = 200))]
    pub title: Option<String>,
    /// `todo`, `doing` or `done`.
    #[garde(skip)]
    pub status: Option<TaskStatus>,
    /// 1 (highest) to 4.
    #[garde(range(min = 1, max = 4))]
    pub priority: Option<i32>,
}

/// The query of `GET /api/v1/tasks`: `?project_id=..&status=todo`.
#[derive(Debug, Default, Deserialize, Validate, ToSchema)]
pub struct TaskFilter {
    /// Only this project's tasks.
    #[garde(skip)]
    pub project_id: Option<Uuid>,
    /// Only tasks with this status.
    #[garde(skip)]
    pub status: Option<TaskStatus>,
}

/// The task routes, behind `InWorkspace`: the ORM scopes every query and
/// write to the request's workspace.
#[injectable]
pub struct TasksController;

#[controller("/tasks", version = "1")]
#[guard(InWorkspace::guard())]
impl TasksController {
    /// `GET /api/v1/tasks`: newest first.
    #[get("/")]
    #[can(Read, Task)]
    #[api(summary = "List tasks", tag = "Tasks", responses(400, 401, 403))]
    async fn index(
        &self,
        uow: Uow,
        scope: Accessible<Task, Read>,
        Valid(Query(filter)): Valid<Query<TaskFilter>>,
        page: PageQuery,
    ) -> Result<Page<Record<Task>>> {
        Ok(scope
            .query()
            .when_some(filter.project_id, |query, project| {
                query.where_(Task::PROJECT_ID.eq(project))
            })
            .when_some(filter.status, |query, status| {
                query.where_(Task::STATUS.eq(status.as_str()))
            })
            .latest()
            .paginate(&uow, page)
            .await?)
    }

    /// `POST /api/v1/tasks`
    #[post("/")]
    #[can(Create, Task)]
    #[api(
        summary = "Create a task",
        tag = "Tasks",
        request = CreateTask,
        status = 201,
        responses(400, 401, 403)
    )]
    async fn store(
        &self,
        uow: Uow,
        Valid(Json(input)): Valid<Json<CreateTask>>,
    ) -> Result<Created<Record<Task>>> {
        let task = Task::create(
            &uow,
            Task {
                id: Uuid::now_v7(),
                project_id: input.project_id,
                title: input.title,
                status: input.status.as_str().to_owned(),
                priority: input.priority,
                due_on: input.due_on,
                ..Default::default()
            },
        )
        .await?;
        Ok(Created(format!("/api/v1/tasks/{}", task.id), task))
    }

    /// `GET /api/v1/tasks/{id}`
    #[get("/{id}")]
    #[can(Read, Task, from = "id")]
    #[api(summary = "Show a task", tag = "Tasks", responses(401, 403, 404))]
    async fn show(&self, task: Authorized<Task, Read>) -> Json<Record<Task>> {
        Json(task.into_inner())
    }

    /// `PATCH /api/v1/tasks/{id}`
    #[patch("/{id}")]
    #[can(Update, Task, from = "id")]
    #[api(
        summary = "Update a task",
        tag = "Tasks",
        request = UpdateTask,
        responses(400, 401, 403, 404)
    )]
    async fn update(
        &self,
        uow: Uow,
        task: Authorized<Task, Update>,
        Valid(Json(input)): Valid<Json<UpdateTask>>,
    ) -> Result<Json<Record<Task>>> {
        let mut task = task.into_inner();
        task.update(&uow, |task| {
            if let Some(title) = input.title {
                task.title = title;
            }
            if let Some(status) = input.status {
                task.status = status.as_str().to_owned();
            }
            if let Some(priority) = input.priority {
                task.priority = priority;
            }
        })
        .await?;
        Ok(Json(task))
    }

    /// `DELETE /api/v1/tasks/{id}`
    #[delete("/{id}")]
    #[can(Delete, Task, from = "id")]
    #[api(
        summary = "Delete a task",
        tag = "Tasks",
        status = 204,
        responses(401, 403, 404)
    )]
    async fn destroy(&self, uow: Uow, task: Authorized<Task, Delete>) -> Result<NoContent> {
        task.into_inner().delete(&uow).await?;
        Ok(NoContent)
    }
}
