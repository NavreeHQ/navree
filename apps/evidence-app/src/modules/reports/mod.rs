//! Reports: `POST /api/v1/reports` stores a `queued` report and queues the
//! `reports.generate` job in the same unit of work; a worker counts the
//! workspace's tasks by status, marks the report `computed` and emits the
//! `report.computed` event over NATS JetStream (navree-micro); the event
//! handler, in the `serve` process, marks it `done`. The load profiles poll
//! `GET /api/v1/reports/{id}` until it is `done`, which proves the queue and
//! the broker both deliver under load.

mod migrations;

use chrono::{DateTime, Utc};
use navree::contracts::{Job, JobContext, JobOptions};
use navree::db::Db;
use navree::micro::ServiceClient;
use navree::orm::prelude::*;
use navree::prelude::*;
use navree::queue::Queue;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::modules::tasks::Task;

/// A report: what was asked, where it stands and, once done, the result.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "reports", timestamps, workspace, subject)]
pub struct Report {
    /// A UUID v7.
    pub id: Uuid,
    /// The workspace it belongs to.
    pub workspace_id: Uuid,
    /// `task-summary`.
    pub kind: String,
    /// `queued`, `computed` (by the worker) or `done` (once the
    /// `report.computed` event came back over NATS).
    pub status: String,
    /// The result, once done.
    pub result: Option<serde_json::Value>,
    /// The job's attempt that finished it.
    pub attempts: i32,
    /// When the worker finished it.
    pub finished_at: Option<DateTime<Utc>>,
    /// When it was asked for.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

/// The queued job: counts the workspace's tasks by status into the report.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GenerateReport {
    /// The report to fill.
    pub report_id: Uuid,
    /// Its workspace (jobs run outside any request).
    pub workspace_id: Uuid,
}

impl Job for GenerateReport {
    const NAME: &'static str = "reports.generate";

    fn options() -> JobOptions {
        JobOptions {
            queue: "default".into(),
            max_attempts: 5,
            timeout_secs: 60,
            ..JobOptions::default()
        }
    }

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        let db = ctx.app.get::<Db>().await?;
        let client = ctx.app.get::<ServiceClient>().await?;
        let attempt = ctx.attempt;
        let event = ReportComputed {
            report_id: self.report_id,
            workspace_id: self.workspace_id,
        };
        navree::orm::workspace::scope(self.workspace_id, async move {
            let Some(mut report) = Report::find(&db, self.report_id).await? else {
                // Deleted since: nothing to do.
                return Ok(());
            };
            let mut counts = serde_json::Map::new();
            for status in ["todo", "doing", "done"] {
                let count = Task::query()
                    .where_(Task::STATUS.eq(status))
                    .count(&db)
                    .await?;
                counts.insert(status.to_owned(), json!(count));
            }
            report
                .update(&db, |report| {
                    report.status = "computed".to_owned();
                    report.result = Some(json!({ "tasks": counts }));
                    report.attempts = i32::try_from(attempt).unwrap_or(i32::MAX);
                })
                .await?;
            Ok::<_, BoxError>(())
        })
        .await?;
        client
            .emit(REPORT_COMPUTED, &event)
            .key(event.report_id)
            .await?;
        Ok(())
    }
}

/// The event a computed report is announced with, on NATS JetStream.
pub const REPORT_COMPUTED: &str = "report.computed";

/// `report.computed`: which report, in which workspace.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReportComputed {
    /// The report.
    pub report_id: Uuid,
    /// Its workspace.
    pub workspace_id: Uuid,
}

/// The `report.computed` handler: marks the report `done`.
#[injectable]
pub struct ReportEvents {
    db: Arc<Db>,
}

#[message_handlers]
impl ReportEvents {
    /// Marks the report `done` (idempotent: a redelivered event changes
    /// nothing).
    #[event_pattern("report.computed")]
    async fn computed(&self, Payload(event): Payload<ReportComputed>) -> Result<()> {
        let db = self.db.clone();
        navree::orm::workspace::scope(event.workspace_id, async move {
            let Some(mut report) = Report::find(&*db, event.report_id).await? else {
                return Ok(());
            };
            if report.status != "done" {
                report
                    .update(&*db, |report| {
                        report.status = "done".to_owned();
                        report.finished_at = Some(Utc::now());
                    })
                    .await?;
            }
            Ok(())
        })
        .await
    }
}

/// The report routes, behind `InWorkspace`.
#[injectable]
pub struct ReportsController {
    queue: Arc<Queue>,
}

#[controller("/reports", version = "1")]
#[guard(InWorkspace::guard())]
impl ReportsController {
    /// `POST /api/v1/reports`: queues a task summary; answers `202` with
    /// the report, `queued` until a worker runs it.
    #[post("/")]
    #[can(Create, Report)]
    #[api(
        summary = "Queue a report",
        tag = "Reports",
        status = 201,
        responses(401, 403)
    )]
    async fn store(&self, uow: Uow, ws: CurrentWorkspace) -> Result<Created<Record<Report>>> {
        let report = Report::create(
            &uow,
            Report {
                id: Uuid::now_v7(),
                kind: "task-summary".to_owned(),
                status: "queued".to_owned(),
                ..Default::default()
            },
        )
        .await?;
        self.queue
            .dispatch_in(
                &uow,
                GenerateReport {
                    report_id: report.id,
                    workspace_id: ws.id(),
                },
            )
            .await
            .map_err(|error| AppError::internal(error.to_string()))?;
        Ok(Created(format!("/api/v1/reports/{}", report.id), report))
    }

    /// `GET /api/v1/reports/{id}`
    #[get("/{id}")]
    #[can(Read, Report, from = "id")]
    #[api(summary = "Show a report", tag = "Reports", responses(401, 403, 404))]
    async fn show(&self, report: Authorized<Report, Read>) -> Json<Record<Report>> {
        Json(report.into_inner())
    }
}

/// Reports: the model, the job and the routes.
#[module(
    imports = [AccessModule::for_feature::<Report>()],
    controllers = [ReportsController],
    register = Self::wire,
)]
pub struct ReportsModule;

impl ReportsModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.migrations(migrations::migrations());
        m.model::<Report>();
        m.job::<GenerateReport>();
        m.message_handlers::<ReportEvents>();
    }
}
