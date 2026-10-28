//! Showcase: small surfaces so the protocol packages have something to
//! serve from the evidence data.
//!
//! - GraphQL (`/graphql`): `taskCounts` and `tasks(status, limit)` in the
//!   request's workspace (`X-Workspace`).
//! - MCP (`/mcp`): the `workspace_summary` tool, for a workspace the user
//!   is a member of.
//! - Feature flags: `board-view` (exposed at `GET /api/flags`) and
//!   `task-export` (on by default).

use async_graphql::{Context, SimpleObject};
use garde::Validate;
use navree::flags::flag;
use navree::graphql::{ContextExt, resolver};
use navree::mcp::{McpContext, mcp_server};
use navree::orm::prelude::*;
use navree::prelude::*;
use navree::workspace::Workspaces;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::modules::tasks::Task;

/// A task, as GraphQL serves it.
#[derive(Clone, Debug, SimpleObject)]
pub struct TaskObject {
    pub id: Uuid,
    pub project_id: Uuid,
    pub title: String,
    pub status: String,
    pub priority: i32,
}

/// How many tasks a workspace has, by status.
#[derive(Clone, Debug, Default, SimpleObject, Serialize, ToSchema)]
pub struct TaskCounts {
    pub todo: u64,
    pub doing: u64,
    pub done: u64,
}

async fn counts(db: &Uow, workspace: Uuid) -> std::result::Result<TaskCounts, BoxError> {
    let count = |status: &'static str| {
        Task::query()
            .for_workspace(workspace)
            .where_(Task::STATUS.eq(status))
            .count(db)
    };
    Ok(TaskCounts {
        todo: count("todo").await?,
        doing: count("doing").await?,
        done: count("done").await?,
    })
}

/// The GraphQL fields.
#[injectable]
pub struct EvidenceResolver;

#[resolver]
impl EvidenceResolver {
    /// The workspace's task counts by status.
    #[query]
    #[can(Read, Task)]
    async fn task_counts(&self, ctx: &Context<'_>) -> navree::graphql::Result<TaskCounts> {
        let workspace = ctx.workspace().await?;
        let uow = ctx.uow()?;
        Ok(counts(&uow, workspace.id()).await.map_err(|e| AppError::internal(e.to_string()))?)
    }

    /// The workspace's tasks, newest first, optionally one status.
    #[query]
    #[can(Read, Task)]
    async fn tasks(
        &self,
        ctx: &Context<'_>,
        status: Option<String>,
        #[graphql(default = 20, validator(maximum = 100))] limit: u64,
    ) -> navree::graphql::Result<Vec<TaskObject>> {
        let workspace = ctx.workspace().await?;
        let uow = ctx.uow()?;
        let tasks = Task::query()
            .for_workspace(workspace.id())
            .when_some(status, |query, status| query.where_(Task::STATUS.eq(status)))
            .latest()
            .limit(limit)
            .get(&uow)
            .await?;
        Ok(tasks
            .into_iter()
            .map(|task| TaskObject {
                id: task.id,
                project_id: task.project_id,
                title: task.title.clone(),
                status: task.status.clone(),
                priority: task.priority,
            })
            .collect())
    }
}

/// The arguments of `workspace_summary`.
#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct SummaryArgs {
    /// The workspace's slug, such as `acme`.
    #[garde(length(min = 1, max = 64))]
    pub workspace: String,
}

/// What `workspace_summary` answers.
#[derive(Debug, Serialize, ToSchema)]
pub struct WorkspaceSummary {
    pub workspace: String,
    pub name: String,
    pub tasks: TaskCounts,
}

/// The MCP tools.
#[injectable]
pub struct EvidenceMcp {
    workspaces: Arc<Workspaces>,
}

#[mcp_server]
impl EvidenceMcp {
    /// Summarises a workspace the user is a member of: its name and task
    /// counts by status.
    #[mcp_tool(read_only)]
    #[authenticated]
    async fn workspace_summary(&self, cx: &McpContext, args: SummaryArgs) -> Result<WorkspaceSummary> {
        let workspace = self
            .workspaces
            .find_by_slug(&args.workspace)
            .await
            .map_err(|e| AppError::internal(e.to_string()))?
            .ok_or_else(|| AppError::not_found())?;
        let member = self
            .workspaces
            .membership(workspace.id, cx.user_id())
            .await
            .map_err(|e| AppError::internal(e.to_string()))?;
        if member.is_none() {
            return Err(AppError::not_found());
        }
        let tasks = counts(cx.uow(), workspace.id)
            .await
            .map_err(|e| AppError::internal(e.to_string()))?;
        Ok(WorkspaceSummary {
            workspace: workspace.slug,
            name: workspace.name,
            tasks,
        })
    }
}

/// The kanban board view of a project's tasks.
#[flag(expose)]
pub struct BoardView;

/// The CSV export of tasks; on unless someone turns it off.
#[flag(default = true, expose)]
pub struct TaskExport;

/// The showcase surfaces.
#[module(
    imports = [FlagsModule::for_feature::<BoardView>(), FlagsModule::for_feature::<TaskExport>()],
    providers = [EvidenceResolver, EvidenceMcp],
    register = Self::wire,
)]
pub struct ShowcaseModule;

impl ShowcaseModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.resolver::<EvidenceResolver>();
        m.mcp_server::<EvidenceMcp>();
    }
}
