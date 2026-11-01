//! `/api/demo/...`: routes that fail, run slowly, run slow or repeated
//! queries, or call out to other services, so monitoring has real
//! problems to group and show. They need a bearer token and an
//! `X-Workspace` header, like the rest of the API, so what they record is
//! attributed to a user and a workspace.
//!
//! - `GET /api/demo/errors/{card-declined, stage-probability,
//!   duplicate-domain, mail-refused, template-missing, currency-parse}`:
//!   a typed error with a cause, answered as a 500.
//! - `GET /api/demo/errors/panic`: a handler that panics.
//! - `GET /api/demo/slow-request/{board, import, export}`: a response
//!   after a random wait of 1.3 to 2.2, 2.5 to 4 or 1.1 to 1.6 seconds.
//! - `GET /api/demo/slow-query/{contacts-by-id, activities-by-deal,
//!   report-rollup}`: one SQL statement that takes 250 to 700 ms.
//! - `GET /api/demo/n-plus-one`: a query repeated 10 to 15 times.
//! - `GET /api/demo/outgoing/{exchange-timeout, payments-503, refused, dns,
//!   ok-mailpit, ok-search}`: a call through the app's HTTP client.
//!
//! Each route has its own path, so each is its own route in monitoring.

use std::time::{Duration, Instant};

use navree::db::{Executor as _, Statement, Value};
use navree::http_client::{HttpClient, HttpClientError};
use navree::orm::prelude::*;
use navree::prelude::*;
use navree::view::Views;
use serde::Serialize;
use serde_json::json;
use utoipa::ToSchema;

use super::faults::{Fault, UpstreamStatus};
use super::options::DemoOptions;
use super::random::between;
use crate::modules::projects::Project;
use crate::modules::tasks::Task;

/// What a failure route answers when it does not fail.
#[derive(Debug, Serialize, ToSchema)]
pub struct DemoDone {
    /// What happened.
    pub message: String,
}

/// What a slow route answers.
#[derive(Debug, Serialize, ToSchema)]
pub struct DemoWaited {
    /// Which route.
    pub kind: &'static str,
    /// How long it took, in milliseconds.
    pub took_ms: u64,
}

/// What a slow query route answers.
#[derive(Debug, Serialize, ToSchema)]
pub struct DemoQueryRan {
    /// Which statement.
    pub kind: &'static str,
    /// How many rows it returned.
    pub rows: usize,
    /// How long it took, in milliseconds.
    pub took_ms: u64,
}

/// What the repeated-query route answers.
#[derive(Debug, Serialize, ToSchema)]
pub struct DemoRepeated {
    /// How many projects it looked at.
    pub projects: usize,
    /// How many times it ran the same query.
    pub queries: usize,
    /// How many tasks it found.
    pub tasks: usize,
}

/// What an outgoing call route answers.
#[derive(Debug, Serialize, ToSchema)]
pub struct DemoCallMade {
    /// The service that was called.
    pub upstream: &'static str,
    /// `completed`, `timed_out` or `unresolved`.
    pub outcome: &'static str,
    /// The status the service answered with, if it answered.
    pub status: Option<u16>,
    /// How long the call took, in milliseconds.
    pub elapsed_ms: u64,
    /// More about what happened.
    pub detail: Option<String>,
}

/// The query of a contact lookup by id: the pause is a CTE that runs once.
const CONTACTS_BY_ID: &str = "\
WITH pause AS MATERIALIZED (SELECT pg_sleep(?) AS slept) \
SELECT u.id, u.name, u.email, u.created_at \
FROM pause CROSS JOIN users u \
WHERE u.id = ?";

/// The open work of a workspace, soonest due first.
const ACTIVITIES_BY_DEAL: &str = "\
WITH pause AS MATERIALIZED (SELECT pg_sleep(?) AS slept) \
SELECT t.id, t.title, t.status, t.priority, t.due_on \
FROM pause CROSS JOIN tasks t \
WHERE t.workspace_id = ? AND t.status <> 'done' \
ORDER BY t.due_on NULLS LAST, t.priority, t.created_at DESC \
LIMIT 50";

/// Projects and their tasks, rolled up by project status.
const REPORT_ROLLUP: &str = "\
WITH pause AS MATERIALIZED (SELECT pg_sleep(?) AS slept) \
SELECT p.status, COUNT(DISTINCT p.id) AS projects, COUNT(t.id) AS tasks, \
COUNT(t.id) FILTER (WHERE t.status = 'done') AS done \
FROM pause CROSS JOIN projects p LEFT JOIN tasks t ON t.project_id = p.id \
WHERE p.workspace_id = ? \
GROUP BY p.status \
ORDER BY p.status";

/// The failure routes.
#[injectable]
pub struct FailuresController {
    views: Arc<Views>,
    http: Arc<HttpClient>,
    options: Arc<DemoOptions>,
}

#[controller("/demo")]
#[guard(InWorkspace::guard())]
#[authenticated]
impl FailuresController {
    /// `GET /api/demo/errors/card-declined`
    #[get("/errors/card-declined")]
    #[api(
        summary = "Fail like a declined card charge",
        tag = "Demo",
        response = DemoDone,
        responses(401, 403, 500)
    )]
    async fn card_declined(&self) -> Result<Json<DemoDone>> {
        Err(Fault::charge_declined().into())
    }

    /// `GET /api/demo/errors/stage-probability`
    #[get("/errors/stage-probability")]
    #[api(
        summary = "Fail like a forecast with an impossible stage probability",
        tag = "Demo",
        response = DemoDone,
        responses(401, 403, 500)
    )]
    async fn stage_probability(&self) -> Result<Json<DemoDone>> {
        Err(Fault::forecast_range().into())
    }

    /// `GET /api/demo/errors/duplicate-domain`
    #[get("/errors/duplicate-domain")]
    #[api(
        summary = "Fail like a company domain that is taken",
        tag = "Demo",
        response = DemoDone,
        responses(401, 403, 500)
    )]
    async fn duplicate_domain(&self) -> Result<Json<DemoDone>> {
        Err(Fault::domain_taken().into())
    }

    /// `GET /api/demo/errors/mail-refused`
    #[get("/errors/mail-refused")]
    #[api(
        summary = "Fail like a mail server that refuses a connection",
        tag = "Demo",
        response = DemoDone,
        responses(401, 403, 500)
    )]
    async fn mail_refused(&self) -> Result<Json<DemoDone>> {
        Err(Fault::reminder_refused().into())
    }

    /// `GET /api/demo/errors/template-missing`: renders a template that
    /// does not exist, and fails with the error the view layer returns.
    #[get("/errors/template-missing")]
    #[api(
        summary = "Fail like an email whose template is missing",
        tag = "Demo",
        response = DemoDone,
        responses(401, 403, 500)
    )]
    async fn template_missing(&self) -> Result<Json<DemoDone>> {
        let rendered = self
            .views
            .render("mail/weekly-digest-v2", &json!({ "name": "Priya" }));
        match rendered {
            Ok(html) => Ok(Json(DemoDone {
                message: format!("rendered {} bytes", html.len()),
            })),
            Err(source) => Err(Fault::digest_template(source).into()),
        }
    }

    /// `GET /api/demo/errors/currency-parse`
    #[get("/errors/currency-parse")]
    #[api(
        summary = "Fail like an invoice line with an unreadable amount",
        tag = "Demo",
        response = DemoDone,
        responses(401, 403, 500)
    )]
    async fn currency_parse(&self) -> Result<Json<DemoDone>> {
        Err(Fault::invoice_line().into())
    }

    /// `GET /api/demo/errors/panic`: the handler panics.
    #[get("/errors/panic")]
    #[api(
        summary = "Fail like a handler that panics",
        tag = "Demo",
        response = DemoDone,
        responses(401, 403, 500)
    )]
    async fn panics(&self) -> Result<Json<DemoDone>> {
        let columns = ["lead", "qualified", "proposal", "negotiation"];
        let position = columns.len() + 2;
        let column = columns
            .get(position)
            .expect("the deal board has no column at this position");
        Ok(Json(DemoDone {
            message: (*column).to_owned(),
        }))
    }

    /// `GET /api/demo/slow-request/board`: 1.3 to 2.2 seconds.
    #[get("/slow-request/board")]
    #[api(
        summary = "Load the board slowly",
        tag = "Demo",
        response = DemoWaited,
        responses(401, 403)
    )]
    async fn slow_board(&self) -> Json<DemoWaited> {
        wait("board", 1_300, 2_200).await
    }

    /// `GET /api/demo/slow-request/import`: 2.5 to 4 seconds.
    #[get("/slow-request/import")]
    #[api(
        summary = "Run an import slowly",
        tag = "Demo",
        response = DemoWaited,
        responses(401, 403)
    )]
    async fn slow_import(&self) -> Json<DemoWaited> {
        wait("import", 2_500, 4_000).await
    }

    /// `GET /api/demo/slow-request/export`: 1.1 to 1.6 seconds.
    #[get("/slow-request/export")]
    #[api(
        summary = "Run an export slowly",
        tag = "Demo",
        response = DemoWaited,
        responses(401, 403)
    )]
    async fn slow_export(&self) -> Json<DemoWaited> {
        wait("export", 1_100, 1_600).await
    }

    /// `GET /api/demo/slow-query/contacts-by-id`
    #[get("/slow-query/contacts-by-id")]
    #[api(
        summary = "Look an account up with a slow query",
        tag = "Demo",
        response = DemoQueryRan,
        responses(401, 403)
    )]
    async fn slow_contacts(
        &self,
        uow: WorkspaceUow,
        CurrentUser(user): CurrentUser,
    ) -> Result<Json<DemoQueryRan>> {
        let pause = pause_seconds();
        let statement =
            Statement::portable(CONTACTS_BY_ID, [Value::from(pause), Value::from(user.id)]);
        run_query(&uow, "contacts-by-id", &statement).await
    }

    /// `GET /api/demo/slow-query/activities-by-deal`
    #[get("/slow-query/activities-by-deal")]
    #[api(
        summary = "List a workspace's open work with a slow query",
        tag = "Demo",
        response = DemoQueryRan,
        responses(401, 403)
    )]
    async fn slow_activities(
        &self,
        uow: WorkspaceUow,
        ws: CurrentWorkspace,
    ) -> Result<Json<DemoQueryRan>> {
        let pause = pause_seconds();
        let statement = Statement::portable(
            ACTIVITIES_BY_DEAL,
            [Value::from(pause), Value::from(ws.id())],
        );
        run_query(&uow, "activities-by-deal", &statement).await
    }

    /// `GET /api/demo/slow-query/report-rollup`
    #[get("/slow-query/report-rollup")]
    #[api(
        summary = "Roll projects and tasks up with a slow query",
        tag = "Demo",
        response = DemoQueryRan,
        responses(401, 403)
    )]
    async fn slow_rollup(
        &self,
        uow: WorkspaceUow,
        ws: CurrentWorkspace,
    ) -> Result<Json<DemoQueryRan>> {
        let pause = pause_seconds();
        let statement =
            Statement::portable(REPORT_ROLLUP, [Value::from(pause), Value::from(ws.id())]);
        run_query(&uow, "report-rollup", &statement).await
    }

    /// `GET /api/demo/n-plus-one`: loads the latest task of 10 to 15
    /// projects with one query each.
    #[get("/n-plus-one")]
    #[api(
        summary = "Load each project's latest task with its own query",
        tag = "Demo",
        response = DemoRepeated,
        responses(401, 403)
    )]
    async fn n_plus_one(&self, uow: WorkspaceUow) -> Result<Json<DemoRepeated>> {
        let uow = uow.into_inner();
        let wanted = between(10, 15);
        let projects = Project::query()
            .order_by_desc(Project::CREATED_AT)
            .limit(wanted)
            .get(&uow)
            .await?;
        let mut tasks = 0;
        let mut queries = 0;
        for project in projects.iter() {
            let latest = Task::query()
                .where_(Task::PROJECT_ID.eq(project.id))
                .order_by_desc(Task::CREATED_AT)
                .first(&uow)
                .await?;
            queries += 1;
            tasks += usize::from(latest.is_some());
        }
        Ok(Json(DemoRepeated {
            projects: projects.len(),
            queries,
            tasks,
        }))
    }

    /// `GET /api/demo/outgoing/exchange-timeout`: an upstream that answers
    /// after 2.3 to 3.6 seconds, for a client that waits 3.
    #[get("/outgoing/exchange-timeout")]
    #[api(
        summary = "Call a slow exchange-rate service",
        tag = "Demo",
        response = DemoCallMade,
        responses(401, 403, 502)
    )]
    async fn exchange_timeout(&self) -> Result<Json<DemoCallMade>> {
        let url = self.options.stub_url(200, between(2_300, 3_600));
        let (result, elapsed_ms) = self.call(&url).await;
        match result {
            Ok(response) => Ok(made(
                "exchange",
                "completed",
                Some(&response),
                elapsed_ms,
                None,
            )),
            Err(HttpClientError::Timeout { .. }) => Ok(made(
                "exchange",
                "timed_out",
                None,
                elapsed_ms,
                Some("the exchange-rate service did not answer in time".to_owned()),
            )),
            Err(error) => Err(bad_gateway(
                "The exchange-rate service is unreachable.",
                error,
            )),
        }
    }

    /// `GET /api/demo/outgoing/payments-503`: an upstream that answers 503.
    #[get("/outgoing/payments-503")]
    #[api(
        summary = "Call a payments service that is unavailable",
        tag = "Demo",
        response = DemoCallMade,
        responses(401, 403, 502)
    )]
    async fn payments_unavailable(&self) -> Result<Json<DemoCallMade>> {
        let url = self.options.stub_url(503, 120);
        let (result, elapsed_ms) = self.call(&url).await;
        match result {
            Ok(response) if response.status().is_server_error() => Err(bad_gateway(
                "The payments service is unavailable.",
                UpstreamStatus {
                    service: "payments service",
                    status: response.status().as_u16(),
                },
            )),
            Ok(response) => Ok(made(
                "payments",
                "completed",
                Some(&response),
                elapsed_ms,
                None,
            )),
            Err(error) => Err(bad_gateway("The payments service is unreachable.", error)),
        }
    }

    /// `GET /api/demo/outgoing/refused`: a connection nothing accepts.
    #[get("/outgoing/refused")]
    #[api(
        summary = "Call a ledger service that refuses the connection",
        tag = "Demo",
        response = DemoCallMade,
        responses(401, 403, 502)
    )]
    async fn refused(&self) -> Result<Json<DemoCallMade>> {
        let (result, elapsed_ms) = self.call(&self.options.refused_url).await;
        match result {
            Ok(response) => Ok(made(
                "ledger",
                "completed",
                Some(&response),
                elapsed_ms,
                None,
            )),
            Err(error) => Err(bad_gateway(
                "The ledger service refused the connection.",
                error,
            )),
        }
    }

    /// `GET /api/demo/outgoing/dns`: a host name that does not resolve.
    #[get("/outgoing/dns")]
    #[api(
        summary = "Call a ledger host that does not resolve",
        tag = "Demo",
        response = DemoCallMade,
        responses(401, 403)
    )]
    async fn unresolvable(&self) -> Result<Json<DemoCallMade>> {
        let (result, elapsed_ms) = self.call(&self.options.unresolvable_url).await;
        Ok(match result {
            Ok(response) => made("ledger", "completed", Some(&response), elapsed_ms, None),
            Err(error) => made(
                "ledger",
                "unresolved",
                None,
                elapsed_ms,
                Some(error.to_string()),
            ),
        })
    }

    /// `GET /api/demo/outgoing/ok-mailpit`: the mail catcher's information.
    #[get("/outgoing/ok-mailpit")]
    #[api(
        summary = "Call the mail catcher",
        tag = "Demo",
        response = DemoCallMade,
        responses(401, 403, 502)
    )]
    async fn mail_catcher(&self) -> Result<Json<DemoCallMade>> {
        let (result, elapsed_ms) = self.call(&self.options.mail_info_url).await;
        match result {
            Ok(response) => Ok(made(
                "mailpit",
                "completed",
                Some(&response),
                elapsed_ms,
                None,
            )),
            Err(error) => Err(bad_gateway("The mail catcher is unreachable.", error)),
        }
    }

    /// `GET /api/demo/outgoing/ok-search`: the search engine's health.
    #[get("/outgoing/ok-search")]
    #[api(
        summary = "Call the search engine",
        tag = "Demo",
        response = DemoCallMade,
        responses(401, 403, 502)
    )]
    async fn search_engine(&self) -> Result<Json<DemoCallMade>> {
        let (result, elapsed_ms) = self.call(&self.options.search_health_url).await;
        match result {
            Ok(response) => Ok(made(
                "search",
                "completed",
                Some(&response),
                elapsed_ms,
                None,
            )),
            Err(error) => Err(bad_gateway("The search engine is unreachable.", error)),
        }
    }
}

impl FailuresController {
    /// A GET through the HTTP client, once, with the configured time limit;
    /// the result and how long the call took.
    async fn call(
        &self,
        url: &str,
    ) -> (Result<navree::http_client::Response, HttpClientError>, u64) {
        let started = Instant::now();
        let result = self
            .http
            .get(url)
            .retries(0)
            .timeout(Duration::from_millis(self.options.call_timeout_ms))
            .send()
            .await;
        (result, millis(started.elapsed()))
    }
}

/// Sleeps from `low` to `high` milliseconds and says how long.
async fn wait(kind: &'static str, low: u64, high: u64) -> Json<DemoWaited> {
    let took_ms = between(low, high);
    tokio::time::sleep(Duration::from_millis(took_ms)).await;
    Json(DemoWaited { kind, took_ms })
}

/// How long a slow statement pauses: 250 to 700 ms, in seconds.
fn pause_seconds() -> f64 {
    f64::from(u32::try_from(between(250, 700)).unwrap_or(250)) / 1_000.0
}

/// Runs `statement` and reports its row count and duration.
async fn run_query(
    uow: &WorkspaceUow,
    kind: &'static str,
    statement: &Statement,
) -> Result<Json<DemoQueryRan>> {
    let started = Instant::now();
    let rows = uow.fetch_all(statement).await?;
    Ok(Json(DemoQueryRan {
        kind,
        rows: rows.len(),
        took_ms: millis(started.elapsed()),
    }))
}

/// The answer of an outgoing call that went through.
fn made(
    upstream: &'static str,
    outcome: &'static str,
    response: Option<&navree::http_client::Response>,
    elapsed_ms: u64,
    detail: Option<String>,
) -> Json<DemoCallMade> {
    Json(DemoCallMade {
        upstream,
        outcome,
        status: response.map(|response| response.status().as_u16()),
        elapsed_ms,
        detail,
    })
}

/// A 502 for a service the app depends on, with the failure below it.
fn bad_gateway(detail: &str, source: impl Into<BoxError>) -> AppError {
    AppError::new(
        502u16.try_into().expect("502 is a status code"),
        "bad_gateway",
        "Bad Gateway",
    )
    .with_detail(detail)
    .with_source(source)
}

fn millis(elapsed: Duration) -> u64 {
    u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
}
