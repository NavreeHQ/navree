//! A custom page in the panel: the operations overview at
//! `/admin/pages/ops-overview`, a table per workspace and the latest
//! activity, drawn from `templates/ops-overview.html`.

use chrono::Utc;
use navree::helm::{HelmPage, PageContext};
use navree::orm::prelude::*;
use navree::prelude::*;
use navree::workspace::{Member, Workspace};
use serde_json::{Value, json};

use super::activity::AuditEntry;
use crate::modules::projects::Project;
use crate::modules::reports::Report;
use crate::modules::tasks::Task;

/// The template of the page.
pub const TEMPLATE: (&str, &str) = (
    "admin/ops-overview.html",
    include_str!("templates/ops-overview.html"),
);

/// The page: shown to users who may read tasks.
pub fn operations_overview() -> HelmPage {
    HelmPage::new("ops-overview", "Operations overview")
        .group("Overview")
        .icon("gauge")
        .template("admin/ops-overview")
        .can("read", "Task")
        .data(overview)
}

/// What the page shows.
async fn overview(ctx: PageContext) -> Result<Value> {
    let uow = ctx.uow();
    let today = Utc::now().date_naive();
    let workspaces = Workspace::query()
        .order_by(Workspace::NAME)
        .get(uow)
        .await?;
    let mut rows = Vec::new();
    for workspace in workspaces.iter() {
        let id = workspace.id;
        let members = Member::query()
            .where_(Member::WORKSPACE_ID.eq(id))
            .count(uow)
            .await?;
        let projects = Project::query().for_workspace(id).count(uow).await?;
        let open = Task::query()
            .for_workspace(id)
            .where_(Task::STATUS.ne("done"))
            .count(uow)
            .await?;
        let overdue = Task::query()
            .for_workspace(id)
            .where_(Task::STATUS.ne("done"))
            .where_(Task::DUE_ON.lt(today))
            .count(uow)
            .await?;
        let reports = Report::query().for_workspace(id).count(uow).await?;
        let last = Report::query()
            .for_workspace(id)
            .order_by_desc(Report::CREATED_AT)
            .first(uow)
            .await?
            .map(|report| report.created_at.format("%-d %b %Y %H:%M").to_string());
        rows.push(json!({
            "name": workspace.name,
            "slug": workspace.slug,
            "members": members,
            "projects": projects,
            "open_tasks": open,
            "overdue": overdue,
            "reports": reports,
            "last_report": last,
        }));
    }

    let mut counts = Vec::new();
    for (status, label) in [
        ("todo", "To do"),
        ("doing", "In progress"),
        ("done", "Done"),
    ] {
        let count = Task::query()
            .without_workspace_scope()
            .where_(Task::STATUS.eq(status))
            .count(uow)
            .await?;
        counts.push((label, count));
    }
    let most = counts
        .iter()
        .map(|(_, count)| *count)
        .max()
        .unwrap_or(1)
        .max(1);
    let statuses: Vec<Value> = counts
        .iter()
        .map(|(label, count)| {
            json!({
                "label": label,
                "count": count,
                "percent": count * 100 / most,
            })
        })
        .collect();

    let recent = AuditEntry::query()
        .order_by_desc(AuditEntry::CREATED_AT)
        .limit(8)
        .get(uow)
        .await?;
    let activity: Vec<Value> = recent
        .iter()
        .map(|entry| {
            json!({
                "when": entry.created_at.format("%-d %b %H:%M").to_string(),
                "action": entry.action,
                "subject": entry.subject_type,
            })
        })
        .collect();

    Ok(json!({ "workspaces": rows, "statuses": statuses, "activity": activity }))
}
