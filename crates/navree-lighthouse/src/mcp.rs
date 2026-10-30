//! [`LighthouseMcp`]: Lighthouse on the app's MCP server (feature `mcp`),
//! so coding agents find, read and resolve issues the way the JSON API
//! lets scripts.
//!
//! | Item | What it does | Ability |
//! |---|---|---|
//! | `lighthouse_list_issues` | A page of issues (open by default), filtered and sorted | `view` |
//! | `lighthouse_get_issue` | One issue: message, source chain, latest occurrence, timeline, alerts | `view` |
//! | `lighthouse_list_occurrences` | An issue's stored occurrences, newest first | `view` |
//! | `lighthouse_get_trace` | A stored trace with its spans | `view` |
//! | `lighthouse_get_overview` | The overview's figures, timeline and latest occurrences | `view` |
//! | `lighthouse_list_routes` | Every route with throughput, error rate and latency | `view` |
//! | `lighthouse_get_route` | One route's figures, timeline and slowest traces | `view` |
//! | `lighthouse_list_queries` | Normalized statements by total time | `view` |
//! | `lighthouse_list_jobs` | Job types with runtime and failures | `view` |
//! | `lighthouse_list_outgoing` | Hosts called through the HTTP client | `view` |
//! | `lighthouse_list_alerts` | The latest alerts and their delivery | `view` |
//! | `lighthouse_resolve_issue` | Resolves an issue, now or in the next release | `resolve` |
//! | `lighthouse_ignore_issue` | Ignores an issue: forever, until a time or for N occurrences | `resolve` |
//! | `lighthouse_reopen_issue` | Reopens an issue | `resolve` |
//! | resource `lighthouse://issues/{id}` | One issue, as `lighthouse_get_issue` returns it | `view` |
//! | prompt `fix-lighthouse-issue` | Asks the model to fix an issue, test it and resolve it | `view` |

use std::future::Future;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use navree_access::Subject as _;
use navree_http::AppError;
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree_mcp::{
    Call, McpItem, McpOutput, McpServer, PromptDef, PromptMessage, ResourceDef, ToolDef,
};
use navree_validation::Validate;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api::IgnoreInput;
use crate::model::{IssueKind, IssueStatus, Level};
use crate::module::{LighthouseSubject, RESOLVE, VIEW};
use crate::options::{Period, StoreKind};
use crate::service::Lighthouse;
use crate::store::{IssueDetail, IssueFilter, IssueSort, LighthouseStore};

/// The URI template of the issue resource.
pub const ISSUE_RESOURCE: &str = "lighthouse://issues/{id}";

/// The name of the prompt that asks the model to fix an issue.
pub const FIX_ISSUE_PROMPT: &str = "fix-lighthouse-issue";

/// Which statuses `lighthouse_list_issues` lists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum StatusFilter {
    /// Open issues (the default).
    #[default]
    Open,
    /// Resolved issues.
    Resolved,
    /// Issues resolved in the next release.
    ResolvedInNextRelease,
    /// Ignored issues.
    Ignored,
    /// Every status.
    Any,
}

impl StatusFilter {
    /// The status to filter on; `None` for every status.
    fn status(self) -> Option<IssueStatus> {
        match self {
            StatusFilter::Open => Some(IssueStatus::Open),
            StatusFilter::Resolved => Some(IssueStatus::Resolved),
            StatusFilter::ResolvedInNextRelease => Some(IssueStatus::ResolvedInNextRelease),
            StatusFilter::Ignored => Some(IssueStatus::Ignored),
            StatusFilter::Any => None,
        }
    }
}

/// The arguments of `lighthouse_list_issues`.
#[derive(Clone, Debug, Default, Deserialize, Validate, utoipa::ToSchema)]
pub struct ListIssuesArgs {
    /// Only issues with this status: `open` (the default), `resolved`,
    /// `resolved_in_next_release`, `ignored`, or `any` for every status.
    #[validate(skip)]
    #[serde(default)]
    pub status: Option<StatusFilter>,
    /// Only issues of this kind.
    #[validate(skip)]
    #[serde(default)]
    pub kind: Option<IssueKind>,
    /// Only issues of this level.
    #[validate(skip)]
    #[serde(default)]
    pub level: Option<Level>,
    /// Only issues with an occurrence in this release.
    #[validate(length(max = 200))]
    #[serde(default)]
    pub release: Option<String>,
    /// Only issues whose latest occurrence is in this environment.
    #[validate(length(max = 100))]
    #[serde(default)]
    pub environment: Option<String>,
    /// Only issues of routes containing this text (`POST /api/v1/invoices`).
    #[validate(length(max = 500))]
    #[serde(default)]
    pub route: Option<String>,
    /// Only issues of job types containing this text.
    #[validate(length(max = 200))]
    #[serde(default)]
    pub job: Option<String>,
    /// Only issues with an occurrence tagged so, written `key:value`.
    #[validate(length(max = 200))]
    #[serde(default)]
    pub tag: Option<String>,
    /// Only issues with an occurrence for this user id.
    #[validate(length(max = 100))]
    #[serde(default)]
    pub user: Option<String>,
    /// Only issues with an occurrence in this workspace id.
    #[validate(length(max = 100))]
    #[serde(default)]
    pub workspace: Option<String>,
    /// Text in the title.
    #[validate(length(max = 200))]
    #[serde(default)]
    pub search: Option<String>,
    /// The order: `last_seen` (the default), `first_seen` (newest issues
    /// first) or `count`.
    #[validate(skip)]
    #[serde(default)]
    pub sort: Option<IssueSort>,
    /// The page, from 1.
    #[validate(range(min = 1))]
    #[serde(default)]
    pub page: Option<u64>,
    /// Issues per page, 1 to 100 (25 by default).
    #[validate(range(min = 1, max = 100))]
    #[serde(default)]
    pub per_page: Option<u64>,
}

impl ListIssuesArgs {
    /// The store's filter.
    fn filter(self) -> IssueFilter {
        IssueFilter {
            status: self
                .status
                .unwrap_or_default()
                .status()
                .map(|status| status.as_str().to_owned()),
            kind: self.kind.map(|kind| kind.as_str().to_owned()),
            level: self.level.map(|level| level.as_str().to_owned()),
            release: self.release,
            environment: self.environment,
            route: self.route,
            job: self.job,
            tag: self.tag,
            user: self.user,
            workspace: self.workspace,
            search: self.search,
            sort: self.sort.unwrap_or_default(),
            page: self.page,
            per_page: self.per_page,
        }
    }
}

/// The arguments of the tools that take one issue.
#[derive(Clone, Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct IssueArgs {
    /// The issue's id.
    #[validate(skip)]
    pub issue_id: Uuid,
}

/// The arguments of `lighthouse_list_occurrences`.
#[derive(Clone, Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct OccurrencesArgs {
    /// The issue's id.
    #[validate(skip)]
    pub issue_id: Uuid,
    /// The page, from 1.
    #[validate(range(min = 1))]
    #[serde(default)]
    pub page: Option<u64>,
    /// Occurrences per page, 1 to 100 (25 by default).
    #[validate(range(min = 1, max = 100))]
    #[serde(default)]
    pub per_page: Option<u64>,
}

/// The arguments of `lighthouse_get_trace`.
#[derive(Clone, Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct TraceArgs {
    /// The stored trace's id (an occurrence's `sample_id`, or a route's
    /// slowest trace).
    #[validate(skip)]
    pub trace_id: Uuid,
}

/// The arguments of the tools that read figures over a period.
#[derive(Clone, Debug, Default, Deserialize, Validate, utoipa::ToSchema)]
pub struct PeriodArgs {
    /// The period: `1h`, `24h` (the default) or `7d`.
    #[validate(skip)]
    #[serde(default)]
    pub period: Option<Period>,
}

/// The arguments of `lighthouse_get_route`.
#[derive(Clone, Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct RouteArgs {
    /// The method and route template, as `lighthouse_list_routes` lists
    /// them (`GET /api/v1/deals/{id}`).
    #[validate(length(min = 1, max = 500))]
    pub endpoint: String,
    /// The period: `1h`, `24h` (the default) or `7d`.
    #[validate(skip)]
    #[serde(default)]
    pub period: Option<Period>,
}

/// The arguments of `lighthouse_resolve_issue`.
#[derive(Clone, Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct ResolveIssueArgs {
    /// The issue's id.
    #[validate(skip)]
    pub issue_id: Uuid,
    /// Resolve it in the next release: only an occurrence from another
    /// release than the current one reopens it. Use this when the fix is
    /// not deployed yet.
    #[validate(skip)]
    #[serde(default)]
    pub in_next_release: bool,
    /// A comment kept on the issue, such as the change that fixes it.
    #[validate(length(max = 2000))]
    #[serde(default)]
    pub comment: Option<String>,
}

/// The arguments of `lighthouse_ignore_issue`: forever, `until` a time or
/// for `occurrences` more occurrences.
#[derive(Clone, Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct IgnoreIssueArgs {
    /// The issue's id.
    #[validate(skip)]
    pub issue_id: Uuid,
    /// Ignore it until then (RFC 3339).
    #[validate(skip)]
    #[serde(default)]
    pub until: Option<DateTime<Utc>>,
    /// Ignore it for this many more occurrences (at least 1).
    #[validate(skip)]
    #[serde(default)]
    pub occurrences: Option<i64>,
    /// A comment kept on the issue.
    #[validate(length(max = 2000))]
    #[serde(default)]
    pub comment: Option<String>,
}

/// The arguments of the `fix-lighthouse-issue` prompt.
#[derive(Clone, Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct FixIssueArgs {
    /// The issue's id.
    #[validate(skip)]
    pub issue_id: Uuid,
}

/// Lighthouse's MCP server: registered by `LighthouseModule` when the
/// crate's `mcp` feature is on, and served by `McpModule` with the app's
/// other servers. It offers nothing when `lighthouse.mcp` is off or
/// `lighthouse.store` is `off`. Every item checks `view` or `resolve` on
/// the subject `Lighthouse`, so a client sees only what its user may do.
pub struct LighthouseMcp {
    lighthouse: Arc<Lighthouse>,
}

impl Injectable for LighthouseMcp {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Lighthouse>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(LighthouseMcp {
                lighthouse: r.get::<Lighthouse>().await?,
            })
        })
    }
}

/// The store, or a 503 when Lighthouse keeps nothing.
fn store(lighthouse: &Lighthouse) -> Result<&LighthouseStore, AppError> {
    lighthouse
        .store()
        .ok_or_else(|| AppError::service_unavailable("lighthouse.store is off"))
}

/// A value as a tool's result.
fn json<T: Serialize>(value: &T) -> Result<McpOutput, AppError> {
    serde_json::to_value(value)
        .map(McpOutput::Json)
        .map_err(AppError::internal)
}

/// A comment, trimmed; `None` when blank.
fn clean(comment: Option<String>) -> Option<String> {
    comment
        .map(|comment| comment.trim().to_owned())
        .filter(|comment| !comment.is_empty())
}

/// A tool whose arguments are a `T`, running `run` with the service.
fn tool<T, R, F, Fut>(lighthouse: &Arc<Lighthouse>, def: ToolDef, run: F) -> McpItem
where
    T: DeserializeOwned + Validate + utoipa::ToSchema + Send + 'static,
    R: Serialize,
    F: Fn(Arc<Lighthouse>, T) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Result<R, AppError>> + Send + 'static,
{
    let lighthouse = lighthouse.clone();
    McpItem::tool(def.input::<T>(), move |call: Call| {
        let (lighthouse, run) = (lighthouse.clone(), run.clone());
        async move {
            let args: T = call.args().await?;
            json(&run(lighthouse, args).await?)
        }
    })
}

/// A tool reading what everyone with `view` may read.
fn reads(name: &str, title: &str, description: &str) -> ToolDef {
    ToolDef::new(name)
        .title(title)
        .description(description)
        .read_only()
        .can(VIEW, LighthouseSubject::SUBJECT, None)
}

/// A tool changing an issue's status, for users with `resolve`.
fn changes(name: &str, title: &str, description: &str) -> ToolDef {
    ToolDef::new(name)
        .title(title)
        .description(description)
        .idempotent()
        .can(RESOLVE, LighthouseSubject::SUBJECT, None)
}

impl McpServer for LighthouseMcp {
    fn items(self: Arc<Self>) -> Vec<McpItem> {
        let options = self.lighthouse.options();
        if !options.mcp || options.store == StoreKind::Off || self.lighthouse.store().is_none() {
            return Vec::new();
        }
        let l = &self.lighthouse;
        vec![
            tool(
                l,
                reads(
                    "lighthouse_list_issues",
                    "List Lighthouse issues",
                    "Lists the app's issues (errors, slow requests and queries, failed jobs and \
                     scheduled tasks, failing outgoing calls, each grouped by fingerprint), open \
                     ones by default, most recently seen first. Each has its id, title, kind, \
                     level, status, culprit (the route, job, task or host), occurrence count, \
                     users affected, first and last seen, and a 24-hour sparkline. Returns \
                     {data, meta} with the page, per_page, total and last_page.",
                ),
                |l, args: ListIssuesArgs| async move { Ok(store(&l)?.issues(&args.filter()).await?) },
            ),
            tool(
                l,
                reads(
                    "lighthouse_get_issue",
                    "Get a Lighthouse issue",
                    "Returns everything about one issue: the full message, the error's source \
                     chain, its type and location, the latest occurrence (the request's method, \
                     route, path, status and masked headers, the user, workspace, release, \
                     server, tags, report context and backtrace), occurrences per hour over \
                     seven days, its status and comment, and the alerts sent about it.",
                ),
                |l, args: IssueArgs| async move { Ok(store(&l)?.issue_detail(args.issue_id).await?) },
            ),
            tool(
                l,
                reads(
                    "lighthouse_list_occurrences",
                    "List an issue's occurrences",
                    "Lists an issue's stored occurrences, newest first, each with its request, \
                     user, release, context and the id of its stored trace (sample_id). \
                     Returns {data, meta}.",
                ),
                |l, args: OccurrencesArgs| async move {
                    Ok(store(&l)?
                        .occurrences(
                            args.issue_id,
                            args.page.unwrap_or(1),
                            args.per_page.unwrap_or(25),
                        )
                        .await?)
                },
            ),
            tool(
                l,
                reads(
                    "lighthouse_get_trace",
                    "Get a Lighthouse trace",
                    "Returns a stored trace of a request or job: its duration, status and \
                     outcome, the user and release, and its spans (queries, outgoing calls, \
                     cache calls, mail, exceptions) in order, with their timings.",
                ),
                |l, args: TraceArgs| async move { Ok(store(&l)?.sample(args.trace_id).await?) },
            ),
            tool(
                l,
                reads(
                    "lighthouse_get_overview",
                    "Get the Lighthouse overview",
                    "Returns the overview for a period: requests, requests per minute, error \
                     rate, the 95th percentile latency, exceptions, failed jobs and open issues, \
                     a timeline of requests and errors, and the latest occurrences.",
                ),
                |l, args: PeriodArgs| async move {
                    let mut overview = store(&l)?.overview(args.period.unwrap_or_default()).await?;
                    overview.dropped = l.dropped();
                    Ok(overview)
                },
            ),
            tool(
                l,
                reads(
                    "lighthouse_list_routes",
                    "List routes",
                    "Lists every route (method and template) with its request count, \
                     throughput, error rate and p50, p95 and p99 latency in milliseconds over a \
                     period, slowest first.",
                ),
                |l, args: PeriodArgs| async move {
                    Ok(store(&l)?.routes(args.period.unwrap_or_default()).await?)
                },
            ),
            tool(
                l,
                reads(
                    "lighthouse_get_route",
                    "Get a route",
                    "Returns one route's figures over a period, its timeline of requests and \
                     errors, and its slowest stored traces (pass a trace's id to \
                     lighthouse_get_trace for its spans).",
                ),
                |l, args: RouteArgs| async move {
                    Ok(store(&l)?
                        .route(&args.endpoint, args.period.unwrap_or_default())
                        .await?)
                },
            ),
            tool(
                l,
                reads(
                    "lighthouse_list_queries",
                    "List queries",
                    "Lists normalized SQL statements over a period by total time: how often \
                     each ran, its errors, total, average, p95 and maximum time, and the routes \
                     and jobs that ran it.",
                ),
                |l, args: PeriodArgs| async move {
                    Ok(store(&l)?.queries(args.period.unwrap_or_default()).await?)
                },
            ),
            tool(
                l,
                reads(
                    "lighthouse_list_jobs",
                    "List jobs",
                    "Lists every job type over a period with its attempts, throughput, runtime, \
                     failed attempts and failed jobs.",
                ),
                |l, args: PeriodArgs| async move {
                    Ok(store(&l)?.jobs(args.period.unwrap_or_default()).await?)
                },
            ),
            tool(
                l,
                reads(
                    "lighthouse_list_outgoing",
                    "List outgoing calls",
                    "Lists every host the app called through its HTTP client over a period, \
                     with the call count, error rate, latency, slowest calls and the routes and \
                     jobs that made them.",
                ),
                |l, args: PeriodArgs| async move {
                    Ok(store(&l)?.outgoing(args.period.unwrap_or_default()).await?)
                },
            ),
            McpItem::tool(
                reads(
                    "lighthouse_list_alerts",
                    "List alerts",
                    "Lists the latest alerts Lighthouse sent (new issues, regressions, spikes, \
                     failed jobs) per channel, with whether each was delivered.",
                ),
                {
                    let l = l.clone();
                    move |_call: Call| {
                        let l = l.clone();
                        async move {
                            let alerts = store(&l)?
                                .alerts_for(None)
                                .await
                                .map_err(AppError::internal)?;
                            json(&alerts)
                        }
                    }
                },
            ),
            tool(
                l,
                changes(
                    "lighthouse_resolve_issue",
                    "Resolve a Lighthouse issue",
                    "Resolves an issue. With in_next_release, only an occurrence from a release \
                     other than the current one reopens it: use it when the fix is not deployed \
                     yet. Put the change that fixes it in comment. A resolved issue that happens \
                     again reopens as a regression. Returns the issue.",
                ),
                |l, args: ResolveIssueArgs| async move {
                    Ok(store(&l)?
                        .resolve(args.issue_id, args.in_next_release, clean(args.comment))
                        .await?)
                },
            ),
            tool(
                l,
                changes(
                    "lighthouse_ignore_issue",
                    "Ignore a Lighthouse issue",
                    "Ignores an issue: it is still counted but never alerts. Forever without \
                     until and occurrences, until a time with until, or for a number of further \
                     occurrences with occurrences (give at most one). Returns the issue.",
                ),
                |l, args: IgnoreIssueArgs| async move {
                    let ignore = IgnoreInput {
                        until: args.until,
                        occurrences: args.occurrences,
                        comment: None,
                    }
                    .ignore_for()?;
                    Ok(store(&l)?
                        .ignore(args.issue_id, ignore, clean(args.comment))
                        .await?)
                },
            ),
            tool(
                l,
                changes(
                    "lighthouse_reopen_issue",
                    "Reopen a Lighthouse issue",
                    "Reopens a resolved or ignored issue. Returns the issue.",
                ),
                |l, args: IssueArgs| async move { Ok(store(&l)?.reopen(args.issue_id).await?) },
            ),
            McpItem::resource(
                ResourceDef::new(ISSUE_RESOURCE, "lighthouse_issue")
                    .title("Lighthouse issue")
                    .description(
                        "One Lighthouse issue with its message, source chain, latest \
                         occurrence, timeline and alerts.",
                    )
                    .mime_type("application/json")
                    .can(VIEW, LighthouseSubject::SUBJECT, None),
                {
                    let l = l.clone();
                    move |call: Call| {
                        let l = l.clone();
                        async move {
                            let id: Uuid = call.param("id")?;
                            json(&store(&l)?.issue_detail(id).await?)
                        }
                    }
                },
            ),
            McpItem::prompt(
                PromptDef::new(FIX_ISSUE_PROMPT)
                    .title("Fix a Lighthouse issue")
                    .description(
                        "Asks the model to find the cause of an issue, fix it with a test and \
                         resolve the issue in the next release.",
                    )
                    .input::<FixIssueArgs>()
                    .can(VIEW, LighthouseSubject::SUBJECT, None),
                {
                    let l = l.clone();
                    move |call: Call| {
                        let l = l.clone();
                        async move {
                            let args: FixIssueArgs = call.args().await?;
                            let detail = store(&l)?.issue_detail(args.issue_id).await?;
                            Ok(McpOutput::Messages(vec![
                                PromptMessage::resource(format!(
                                    "lighthouse://issues/{}",
                                    args.issue_id
                                )),
                                PromptMessage::user(fix_instructions(&detail)),
                            ]))
                        }
                    }
                },
            ),
        ]
    }
}

/// The instructions of the `fix-lighthouse-issue` prompt.
fn fix_instructions(detail: &IssueDetail) -> String {
    let issue = &detail.issue;
    let mut facts = format!(
        "{} in {}, {} occurrence{} from {} to {}",
        issue.kind.label(),
        issue.culprit,
        issue.count,
        if issue.count == 1 { "" } else { "s" },
        issue.first_seen.format("%Y-%m-%d %H:%M UTC"),
        issue.last_seen.format("%Y-%m-%d %H:%M UTC"),
    );
    if let Some(error_type) = &detail.error_type {
        facts.push_str(&format!(", error type {error_type}"));
    }
    if let Some(location) = &detail.location {
        facts.push_str(&format!(", at {location}"));
    }
    if let Some(release) = &detail.last_release {
        facts.push_str(&format!(", latest in release {release}"));
    }
    format!(
        "Fix the Lighthouse issue \"{title}\" ({facts}). The issue above has the full \
         message, the source chain, the backtrace when there is one, and the latest \
         occurrence with its request, user and context.\n\n\
         1. Find the cause in the code.\n\
         2. Write a test that reproduces it and fails.\n\
         3. Fix it and run the tests.\n\
         4. Resolve the issue in the next release with the tool lighthouse_resolve_issue \
         (issue_id {id}, in_next_release true) and a comment naming the change.",
        title = issue.title,
        id = issue.id,
    )
}
