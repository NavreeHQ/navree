//! Work in the panel: projects, tasks and reports.
//!
//! These models belong to workspaces, and the panel is not inside any one
//! workspace, so each resource works across every workspace
//! (`Workspaces::Every`) and runs its writes inside the workspace of the
//! record it changes. The connection's role decides what it sees: the
//! compose database's role is a superuser, so it sees every workspace.

use chrono::Utc;
use navree::BoxFuture;
use navree::helm::{
    Action, ActionResult, Field, Filter, FormData, Metric, RecordOf, Resource, SaveContext,
    Workspaces,
};
use navree::orm::prelude::*;
use navree::prelude::*;
use navree::queue::Queue;
use serde_json::json;
use uuid::Uuid;

use super::accounts::{UserAdmin, WorkspaceAdmin};
use super::form;
use crate::modules::projects::Project;
use crate::modules::reports::{GenerateReport, Report};
use crate::modules::tasks::Task;

/// The statuses a task has.
const TASK_STATUSES: [(&str, &str); 3] = [
    ("todo", "To do"),
    ("doing", "In progress"),
    ("done", "Done"),
];

/// The priorities a task has.
const PRIORITIES: [(&str, &str); 4] = [
    ("1", "1 - Urgent"),
    ("2", "2 - High"),
    ("3", "3 - Normal"),
    ("4", "4 - Low"),
];

/// The statuses a project has.
const PROJECT_STATUSES: [(&str, &str); 2] = [("active", "Active"), ("archived", "Archived")];

/// The statuses a report goes through.
const REPORT_STATUSES: [(&str, &str); 3] = [
    ("queued", "Queued"),
    ("computed", "Computed"),
    ("done", "Done"),
];

/// Projects, across every workspace.
#[injectable]
pub struct ProjectAdmin;

impl Resource for ProjectAdmin {
    type Model = Project;

    const LABEL: &'static str = "Projects";
    const SINGULAR: &'static str = "Project";
    const GROUP: &'static str = "Work";
    const ICON: &'static str = "folder";
    const TITLE: &'static str = "name";
    const SEARCH: &'static [&'static str] = &["name", "description"];
    const ORDER: i32 = 1;
    const WORKSPACES: Workspaces = Workspaces::Every;

    fn fields(&self) -> Vec<Field> {
        vec![
            Field::text("name").required().max_length(120).sortable(),
            Field::belongs_to::<WorkspaceAdmin>("workspace_id")
                .label("Workspace")
                .readonly(),
            Field::select("status")
                .options(PROJECT_STATUSES)
                .badge()
                .sortable(),
            Field::textarea("description").hide_on_index(),
            Field::belongs_to::<UserAdmin>("created_by")
                .label("Created by")
                .readonly()
                .hide_on_index(),
            Field::datetime("created_at").readonly().sortable(),
            Field::has_many::<TaskAdmin>("project_id").label("Tasks"),
        ]
    }

    fn filters(&self) -> Vec<Filter<Project>> {
        vec![
            Filter::select("status").options(PROJECT_STATUSES),
            Filter::belongs_to::<WorkspaceAdmin>("workspace_id").label("Workspace"),
            Filter::belongs_to::<UserAdmin>("created_by").label("Created by"),
            Filter::date_range("created_at").label("Created"),
            Filter::custom("name_contains", "Name contains", |query, text| {
                query.where_(Project::NAME.like(format!("%{text}%")))
            }),
        ]
    }

    fn actions(&self) -> Vec<Action<Project>> {
        vec![
            Action::<Project>::new("archive", "Archive projects")
                .confirm("Archive the selected projects? Their tasks stay as they are.")
                .handle(|ctx, projects| async move {
                    let mut archived = 0;
                    for mut project in projects {
                        if project.status == "archived" {
                            continue;
                        }
                        let workspace = project.workspace_id;
                        let (id, name) = (project.id, project.name.clone());
                        navree::orm::workspace::scope(
                            workspace,
                            project.update(ctx.uow(), |project| {
                                project.status = "archived".to_owned();
                            }),
                        )
                        .await?;
                        form::announce(
                            &ctx,
                            "project.archived",
                            workspace,
                            json!({ "project_id": id, "name": name, "by": ctx.user_id() }),
                        )
                        .await;
                        archived += 1;
                    }
                    Ok(ActionResult::message(format!(
                        "{archived} projects archived."
                    )))
                }),
            Action::<Project>::new("reactivate", "Reactivate projects").handle(
                |ctx, projects| async move {
                    let mut reactivated = 0;
                    for mut project in projects {
                        if project.status == "active" {
                            continue;
                        }
                        let workspace = project.workspace_id;
                        navree::orm::workspace::scope(
                            workspace,
                            project.update(ctx.uow(), |project| {
                                project.status = "active".to_owned();
                            }),
                        )
                        .await?;
                        reactivated += 1;
                    }
                    Ok(ActionResult::message(format!(
                        "{reactivated} projects reactivated."
                    )))
                },
            ),
            Action::<Project>::new("add-note", "Add a note")
                .fields(vec![
                    Field::textarea("note")
                        .label("Note")
                        .required()
                        .max_length(500)
                        .help("Added to the end of the description."),
                ])
                .only_on_detail()
                .handle(|ctx, projects| async move {
                    let note: String = ctx.input("note")?;
                    let today = Utc::now().format("%-d %b %Y");
                    let mut changed = 0;
                    for mut project in projects {
                        let workspace = project.workspace_id;
                        let entry = format!("Note, {today}: {}", note.trim());
                        navree::orm::workspace::scope(
                            workspace,
                            project.update(ctx.uow(), |project| {
                                project.description = Some(match project.description.take() {
                                    Some(text) if !text.trim().is_empty() => {
                                        format!("{}\n\n{entry}", text.trim_end())
                                    }
                                    _ => entry,
                                });
                            }),
                        )
                        .await?;
                        changed += 1;
                    }
                    Ok(ActionResult::message(format!(
                        "Note added to {changed} project."
                    )))
                }),
        ]
    }

    fn metrics(&self) -> Vec<Metric<Project>> {
        vec![
            Metric::value("Projects").count().on_dashboard(),
            Metric::value("Active projects")
                .count()
                .filter(Project::STATUS.eq("active")),
            Metric::trend("New projects")
                .count()
                .by_day(Project::CREATED_AT, 30)
                .on_dashboard(),
            Metric::partition("By status")
                .count()
                .group_by(Project::STATUS)
                .on_dashboard(),
            Metric::partition("By workspace")
                .count()
                .group_by(Project::WORKSPACE_ID),
        ]
    }

    /// Projects are created in their workspace (`POST /api/v1/projects`).
    fn create<'a>(
        &'a self,
        _ctx: &'a SaveContext,
        _form: FormData,
    ) -> BoxFuture<'a, Result<RecordOf<Self>>> {
        Box::pin(async move {
            Err(AppError::bad_request(
                "Projects are created in their workspace.",
            ))
        })
    }

    /// Deletes the project from inside its workspace.
    fn delete<'a>(
        &'a self,
        ctx: &'a SaveContext,
        mut record: RecordOf<Self>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let workspace = record.workspace_id;
            navree::orm::workspace::scope(workspace, record.delete(ctx.uow())).await?;
            Ok(())
        })
    }
}

/// Tasks, across every workspace: filters, row and bulk actions, metrics,
/// and forms that create and edit.
#[injectable]
pub struct TaskAdmin;

impl TaskAdmin {
    /// The project a form names, which must exist.
    async fn project_of(ctx: &SaveContext, id: Uuid) -> Result<Project> {
        let found = Project::query()
            .without_workspace_scope()
            .where_key(id)
            .first(ctx.uow())
            .await?;
        found
            .map(Record::into_inner)
            .ok_or_else(|| form::invalid("project_id", "The project does not exist."))
    }
}

impl Resource for TaskAdmin {
    type Model = Task;

    const LABEL: &'static str = "Tasks";
    const SINGULAR: &'static str = "Task";
    const GROUP: &'static str = "Work";
    const ICON: &'static str = "list-checks";
    const TITLE: &'static str = "title";
    const SEARCH: &'static [&'static str] = &["title"];
    const ORDER: i32 = 2;
    const WORKSPACES: Workspaces = Workspaces::Every;

    fn fields(&self) -> Vec<Field> {
        vec![
            Field::text("title").required().max_length(200).sortable(),
            Field::belongs_to::<ProjectAdmin>("project_id")
                .label("Project")
                .required(),
            Field::belongs_to::<WorkspaceAdmin>("workspace_id")
                .label("Workspace")
                .readonly(),
            Field::select("status")
                .options(TASK_STATUSES)
                .default(json!("todo"))
                .badge()
                .sortable(),
            Field::select("priority")
                .options(PRIORITIES)
                .default(json!("3"))
                .badge()
                .sortable(),
            Field::date("due_on").label("Due").sortable(),
            Field::belongs_to::<UserAdmin>("assignee_id").label("Assignee"),
            Field::datetime("created_at").readonly().sortable(),
            Field::datetime("updated_at").readonly().hide_on_index(),
        ]
    }

    fn filters(&self) -> Vec<Filter<Task>> {
        vec![
            Filter::select("status").options(TASK_STATUSES),
            Filter::select("priority").options(PRIORITIES),
            Filter::belongs_to::<WorkspaceAdmin>("workspace_id").label("Workspace"),
            Filter::belongs_to::<UserAdmin>("assignee_id").label("Assignee"),
            Filter::date_range("due_on").label("Due"),
            Filter::date_range("created_at").label("Created"),
            Filter::boolean("overdue", "Overdue", |query, on| {
                if on {
                    query
                        .where_(Task::DUE_ON.lt(Utc::now().date_naive()))
                        .where_(Task::STATUS.ne("done"))
                } else {
                    query
                }
            }),
            Filter::boolean("unassigned", "Unassigned", |query, on| {
                if on {
                    query.where_(Task::ASSIGNEE_ID.is_null())
                } else {
                    query
                }
            }),
        ]
    }

    fn actions(&self) -> Vec<Action<Task>> {
        vec![
            Action::<Task>::new("mark-done", "Mark tasks done")
                .confirm("Mark the selected tasks as done?")
                .handle(|ctx, tasks| async move {
                    let mut done = 0;
                    for mut task in tasks {
                        if task.status == "done" {
                            continue;
                        }
                        let workspace = task.workspace_id;
                        let (id, title) = (task.id, task.title.clone());
                        navree::orm::workspace::scope(
                            workspace,
                            task.update(ctx.uow(), |task| task.status = "done".to_owned()),
                        )
                        .await?;
                        form::announce(
                            &ctx,
                            "task.completed",
                            workspace,
                            json!({ "task_id": id, "title": title, "by": ctx.user_id() }),
                        )
                        .await;
                        done += 1;
                    }
                    Ok(ActionResult::message(format!(
                        "{done} tasks marked as done."
                    )))
                }),
            Action::<Task>::new("reopen", "Reopen tasks").handle(|ctx, tasks| async move {
                let mut reopened = 0;
                for mut task in tasks {
                    if task.status != "done" {
                        continue;
                    }
                    let workspace = task.workspace_id;
                    navree::orm::workspace::scope(
                        workspace,
                        task.update(ctx.uow(), |task| task.status = "doing".to_owned()),
                    )
                    .await?;
                    reopened += 1;
                }
                Ok(ActionResult::message(format!("{reopened} tasks reopened.")))
            }),
            Action::<Task>::new("reprioritize", "Reprioritize")
                .fields(vec![
                    Field::select("priority")
                        .label("New priority")
                        .options(PRIORITIES)
                        .required(),
                ])
                .handle(|ctx, tasks| async move {
                    let value: serde_json::Value = ctx.input("priority")?;
                    let priority = form::number_of("priority", &value)?
                        .flatten()
                        .filter(|priority| (1..=4).contains(priority))
                        .ok_or_else(|| form::invalid("priority", "priority must be from 1 to 4"))?;
                    let mut changed = 0;
                    for mut task in tasks {
                        if task.priority == priority {
                            continue;
                        }
                        let workspace = task.workspace_id;
                        navree::orm::workspace::scope(
                            workspace,
                            task.update(ctx.uow(), |task| task.priority = priority),
                        )
                        .await?;
                        changed += 1;
                    }
                    Ok(ActionResult::message(format!(
                        "{changed} tasks moved to priority {priority}."
                    )))
                }),
            Action::<Task>::new("assign-to-me", "Assign to me").handle(|ctx, tasks| async move {
                let me = ctx.user_id();
                let mut assigned = 0;
                for mut task in tasks {
                    if task.assignee_id == Some(me) {
                        continue;
                    }
                    let workspace = task.workspace_id;
                    navree::orm::workspace::scope(
                        workspace,
                        task.update(ctx.uow(), |task| task.assignee_id = Some(me)),
                    )
                    .await?;
                    assigned += 1;
                }
                Ok(ActionResult::message(format!(
                    "{assigned} tasks assigned to you."
                )))
            }),
        ]
    }

    fn metrics(&self) -> Vec<Metric<Task>> {
        vec![
            Metric::value("Open tasks")
                .count()
                .filter(Task::STATUS.ne("done"))
                .on_dashboard(),
            Metric::value("Tasks created")
                .count()
                .over(Task::CREATED_AT)
                .on_dashboard(),
            Metric::trend("Tasks per day")
                .count()
                .by_day(Task::CREATED_AT, 30)
                .on_dashboard(),
            Metric::trend("Completed per day")
                .count()
                .filter(Task::STATUS.eq("done"))
                .by_day(Task::UPDATED_AT, 30),
            Metric::partition("Tasks by status")
                .count()
                .group_by(Task::STATUS)
                .on_dashboard(),
            Metric::partition("Tasks by priority")
                .count()
                .group_by(Task::PRIORITY),
            Metric::partition("Tasks by workspace")
                .count()
                .group_by(Task::WORKSPACE_ID),
        ]
    }

    /// Creates a task in the workspace of its project. A task whose title
    /// another open task of the project already has is refused.
    fn create<'a>(
        &'a self,
        ctx: &'a SaveContext,
        data: FormData,
    ) -> BoxFuture<'a, Result<RecordOf<Self>>> {
        Box::pin(async move {
            let title = form::text(&data, "title")
                .flatten()
                .ok_or_else(|| form::invalid("title", "title should not be empty"))?;
            let project_id = form::uuid(&data, "project_id")?
                .flatten()
                .ok_or_else(|| form::invalid("project_id", "project_id should not be empty"))?;
            let project = Self::project_of(ctx, project_id).await?;
            let workspace = project.workspace_id;
            let uow = ctx.uow();
            let open_with_title = Task::query()
                .for_workspace(workspace)
                .where_(Task::PROJECT_ID.eq(project.id))
                .where_(Task::TITLE.eq(title.clone()))
                .where_(Task::STATUS.ne("done"))
                .count(uow)
                .await?;
            if open_with_title > 0 {
                return Err(form::invalid(
                    "title",
                    "An open task with this title already exists in the project.",
                ));
            }
            let status = form::text(&data, "status")
                .flatten()
                .unwrap_or_else(|| "todo".to_owned());
            let priority = form::number(&data, "priority")?.flatten().unwrap_or(3);
            let due_on = form::date(&data, "due_on")?.flatten();
            let assignee_id = form::uuid(&data, "assignee_id")?.flatten();
            let task = navree::orm::workspace::scope(
                workspace,
                Task::create(
                    uow,
                    Task {
                        id: Uuid::now_v7(),
                        workspace_id: workspace,
                        project_id: project.id,
                        title,
                        status,
                        priority,
                        due_on,
                        assignee_id,
                        ..Task::default()
                    },
                ),
            )
            .await?;
            Ok(task)
        })
    }

    /// Changes the fields the form sent. A task stays in its workspace: its
    /// project must be one of that workspace's.
    fn update<'a>(
        &'a self,
        ctx: &'a SaveContext,
        mut record: RecordOf<Self>,
        data: FormData,
    ) -> BoxFuture<'a, Result<RecordOf<Self>>> {
        Box::pin(async move {
            let title = form::text(&data, "title");
            if let Some(None) = title {
                return Err(form::invalid("title", "title should not be empty"));
            }
            let project_id = form::uuid(&data, "project_id")?;
            if let Some(Some(id)) = project_id {
                let project = Self::project_of(ctx, id).await?;
                if project.workspace_id != record.workspace_id {
                    return Err(form::invalid(
                        "project_id",
                        "The project must belong to the task's workspace.",
                    ));
                }
            }
            let status = form::text(&data, "status");
            let priority = form::number(&data, "priority")?;
            let due_on = form::date(&data, "due_on")?;
            let assignee_id = form::uuid(&data, "assignee_id")?;
            let workspace = record.workspace_id;
            navree::orm::workspace::scope(
                workspace,
                record.update(ctx.uow(), |task| {
                    if let Some(Some(title)) = title {
                        task.title = title;
                    }
                    if let Some(Some(id)) = project_id {
                        task.project_id = id;
                    }
                    if let Some(Some(status)) = status {
                        task.status = status;
                    }
                    if let Some(Some(priority)) = priority {
                        task.priority = priority;
                    }
                    if let Some(due_on) = due_on {
                        task.due_on = due_on;
                    }
                    if let Some(assignee_id) = assignee_id {
                        task.assignee_id = assignee_id;
                    }
                }),
            )
            .await?;
            Ok(record)
        })
    }

    /// Deletes the task from inside its workspace.
    fn delete<'a>(
        &'a self,
        ctx: &'a SaveContext,
        mut record: RecordOf<Self>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let workspace = record.workspace_id;
            navree::orm::workspace::scope(workspace, record.delete(ctx.uow())).await?;
            Ok(())
        })
    }
}

/// Reports: the task summaries workspaces asked for.
#[injectable]
pub struct ReportAdmin;

impl Resource for ReportAdmin {
    type Model = Report;

    const LABEL: &'static str = "Reports";
    const SINGULAR: &'static str = "Report";
    const GROUP: &'static str = "Work";
    const ICON: &'static str = "chart-column";
    const SEARCH: &'static [&'static str] = &["kind"];
    const ORDER: i32 = 3;
    const WORKSPACES: Workspaces = Workspaces::Every;

    fn fields(&self) -> Vec<Field> {
        vec![
            Field::select("kind")
                .options([("task-summary", "Task summary")])
                .readonly(),
            Field::belongs_to::<WorkspaceAdmin>("workspace_id")
                .label("Workspace")
                .readonly(),
            Field::select("status")
                .options(REPORT_STATUSES)
                .badge()
                .readonly()
                .sortable(),
            Field::computed("tasks", |report: &Report| {
                let count = |status: &str| {
                    report
                        .result
                        .as_ref()
                        .and_then(|result| result.pointer(&format!("/tasks/{status}")))
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(0)
                };
                if report.result.is_none() {
                    return "waiting".to_owned();
                }
                format!(
                    "{} to do, {} in progress, {} done",
                    count("todo"),
                    count("doing"),
                    count("done")
                )
            }),
            Field::number("attempts").readonly(),
            Field::datetime("finished_at").readonly().sortable(),
            Field::datetime("created_at")
                .label("Requested")
                .readonly()
                .sortable(),
        ]
    }

    fn filters(&self) -> Vec<Filter<Report>> {
        vec![
            Filter::select("status").options(REPORT_STATUSES),
            Filter::belongs_to::<WorkspaceAdmin>("workspace_id").label("Workspace"),
            Filter::date_range("created_at").label("Requested"),
        ]
    }

    fn actions(&self) -> Vec<Action<Report>> {
        vec![
            Action::<Report>::new("request-again", "Request a new report")
                .confirm("Request a new task summary for the workspace of each selected report?")
                .handle(|ctx, reports| async move {
                    let queue = ctx.get::<Queue>().await?;
                    let mut workspaces: Vec<Uuid> =
                        reports.iter().map(|report| report.workspace_id).collect();
                    workspaces.sort();
                    workspaces.dedup();
                    for workspace in &workspaces {
                        let report = navree::orm::workspace::scope(
                            *workspace,
                            Report::create(
                                ctx.uow(),
                                Report {
                                    id: Uuid::now_v7(),
                                    kind: "task-summary".to_owned(),
                                    status: "queued".to_owned(),
                                    ..Report::default()
                                },
                            ),
                        )
                        .await?;
                        queue
                            .dispatch_in(
                                ctx.uow(),
                                GenerateReport {
                                    report_id: report.id,
                                    workspace_id: *workspace,
                                },
                            )
                            .await
                            .map_err(AppError::internal)?;
                        form::announce(
                            &ctx,
                            "report.requested",
                            *workspace,
                            json!({ "report_id": report.id, "by": ctx.user_id() }),
                        )
                        .await;
                    }
                    Ok(ActionResult::message(format!(
                        "{} reports requested.",
                        workspaces.len()
                    )))
                }),
        ]
    }

    fn metrics(&self) -> Vec<Metric<Report>> {
        vec![
            Metric::value("Reports").count().on_dashboard(),
            Metric::trend("Reports requested")
                .count()
                .by_day(Report::CREATED_AT, 30),
            Metric::partition("Reports by status")
                .count()
                .group_by(Report::STATUS)
                .on_dashboard(),
        ]
    }

    /// Reports are requested through the API or with the action above.
    fn create<'a>(
        &'a self,
        _ctx: &'a SaveContext,
        _form: FormData,
    ) -> BoxFuture<'a, Result<RecordOf<Self>>> {
        Box::pin(async move {
            Err(AppError::bad_request(
                "Reports are requested with the \"Request a new report\" action.",
            ))
        })
    }

    /// A report's numbers come from its job.
    fn update<'a>(
        &'a self,
        _ctx: &'a SaveContext,
        _record: RecordOf<Self>,
        _form: FormData,
    ) -> BoxFuture<'a, Result<RecordOf<Self>>> {
        Box::pin(async move { Err(AppError::bad_request("A report's numbers are computed.")) })
    }

    /// Deletes the report from inside its workspace.
    fn delete<'a>(
        &'a self,
        ctx: &'a SaveContext,
        mut record: RecordOf<Self>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let workspace = record.workspace_id;
            navree::orm::workspace::scope(workspace, record.delete(ctx.uow())).await?;
            Ok(())
        })
    }
}
