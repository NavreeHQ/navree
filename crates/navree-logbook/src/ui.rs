//! The timeline UI at `/_logbook` and its JSON API.

use std::fmt::Write as _;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use http::header::{CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE};
use http::request::Parts;
use http::{HeaderName, HeaderValue, Method, StatusCode};
use navree_http::{AppError, ClientIp, Controller, Guard, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree_view::Views;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::batch::duplicates;
use crate::store::ListQuery;
use crate::{Batch, Content, Entry, EntryKind, Filter, Logbook, LogbookError, ReplayRequest, Term};

/// The permission every Logbook route needs when `logbook.allow_remote` is
/// on.
pub const VIEW_PERMISSION: &str = "logbook.view";

/// The UI's Content-Security-Policy: no inline scripts or styles.
const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; \
    connect-src 'self'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'";

/// A recorded mail's HTML runs sandboxed, without scripts.
const MAIL_CSP: &str =
    "default-src 'none'; img-src * data:; style-src 'unsafe-inline'; font-src * data:; sandbox";

/// Rows per page.
const PAGE: usize = 50;

const TEMPLATES: [(&str, &str); 4] = [
    ("logbook/layout.html", include_str!("ui/layout.html")),
    ("logbook/index.html", include_str!("ui/index.html")),
    ("logbook/rows.html", include_str!("ui/rows.html")),
    ("logbook/detail.html", include_str!("ui/detail.html")),
];

const STYLESHEET: &str = include_str!("ui/logbook.css");
const SCRIPT: &str = include_str!("ui/logbook.js");

/// Serves the UI and its API under `logbook.path` (only when Logbook is on).
pub(crate) struct LogbookController {
    logbook: Arc<Logbook>,
    views: Views,
}

impl Injectable for LogbookController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Logbook>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            let logbook = r.get::<Logbook>().await?;
            let views =
                Views::from_templates(TEMPLATES).map_err(|error| KernelError::Construction {
                    provider: "navree_logbook::LogbookController".to_owned(),
                    source: Box::new(error),
                })?;
            Ok(LogbookController { logbook, views })
        })
    }
}

/// Answers non-loopback clients with a 404 (`logbook.allow_remote = false`).
#[derive(Debug)]
pub(crate) struct LoopbackOnly;

impl Guard for LoopbackOnly {
    fn check<'a>(&'a self, parts: &'a mut Parts) -> BoxFuture<'a, Result<(), AppError>> {
        let local = parts
            .extensions
            .get::<ClientIp>()
            .is_some_and(|ip| ip.0.is_loopback());
        Box::pin(async move {
            if local {
                Ok(())
            } else {
                Err(AppError::not_found())
            }
        })
    }
}

/// The list's query parameters.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct ListParams {
    q: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    before: Option<u64>,
    after: Option<u64>,
    limit: Option<usize>,
}

impl Controller for LogbookController {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        if !self.logbook.is_enabled() {
            return Vec::new();
        }
        let base = self.logbook.options().path.clone();
        let path = |suffix: &str| format!("!{base}{suffix}");
        let mut routes = Vec::new();

        let this = self.clone();
        routes.push(RouteDef::on(
            Method::GET,
            path(""),
            move |Query(params): Query<ListParams>| async move { this.index(params).await },
        ));
        let this = self.clone();
        routes.push(RouteDef::on(
            Method::GET,
            path("/rows"),
            move |Query(params): Query<ListParams>| async move { this.rows(params).await },
        ));
        let this = self.clone();
        routes.push(RouteDef::on(
            Method::GET,
            path("/entries/{id}"),
            move |Path(id): Path<String>| async move { this.detail(id).await },
        ));
        let this = self.clone();
        routes.push(RouteDef::on(
            Method::GET,
            path("/mail/{id}"),
            move |Path(id): Path<String>| async move { this.mail_html(id).await },
        ));
        routes.push(RouteDef::on(
            Method::GET,
            path("/assets/logbook.css"),
            || async { asset("text/css; charset=utf-8", STYLESHEET) },
        ));
        routes.push(RouteDef::on(
            Method::GET,
            path("/assets/logbook.js"),
            || async { asset("text/javascript; charset=utf-8", SCRIPT) },
        ));
        let this = self.clone();
        routes.push(RouteDef::on(
            Method::GET,
            path("/api/entries"),
            move |Query(params): Query<ListParams>| async move { this.api_list(params).await },
        ));
        let this = self.clone();
        routes.push(RouteDef::on(
            Method::DELETE,
            path("/api/entries"),
            move || async move { this.api_clear().await },
        ));
        let this = self.clone();
        routes.push(RouteDef::on(
            Method::GET,
            path("/api/entries/{id}"),
            move |Path(id): Path<String>| async move { this.api_entry(id).await },
        ));
        let this = self.clone();
        routes.push(RouteDef::on(
            Method::GET,
            path("/api/batches/{id}"),
            move |Path(id): Path<String>| async move { this.api_batch(id).await },
        ));
        let this = self.clone();
        routes.push(RouteDef::on(
            Method::POST,
            path("/api/requests/{id}/replay"),
            move |Path(id): Path<String>, Json(edit): Json<ReplayRequest>| async move {
                this.api_replay(id, edit).await
            },
        ));
        let this = self.clone();
        routes.push(RouteDef::on(
            Method::GET,
            path("/api/requests/{id}/curl"),
            move |Path(id): Path<String>| async move { this.api_curl(id).await },
        ));

        let remote = self.logbook.options().allow_remote;
        routes
            .into_iter()
            .map(|route| {
                let route = route.name("logbook");
                if remote {
                    route.permissions(&[VIEW_PERMISSION])
                } else {
                    route.public().guard(Arc::new(LoopbackOnly))
                }
            })
            .collect()
    }
}

fn asset(content_type: &'static str, body: &'static str) -> Response {
    (
        [
            (CONTENT_TYPE, HeaderValue::from_static(content_type)),
            (CACHE_CONTROL, HeaderValue::from_static("no-cache")),
        ],
        body,
    )
        .into_response()
}

fn html(body: String) -> Response {
    (
        [
            (
                CONTENT_TYPE,
                HeaderValue::from_static("text/html; charset=utf-8"),
            ),
            (CONTENT_SECURITY_POLICY, HeaderValue::from_static(CSP)),
            (CACHE_CONTROL, HeaderValue::from_static("no-store")),
        ],
        body,
    )
        .into_response()
}

/// The filter of the list's parameters, or the parse error to show.
fn filter_of(params: &ListParams) -> Result<Filter, String> {
    let mut filter =
        Filter::parse(params.q.as_deref().unwrap_or_default()).map_err(|error| error.0)?;
    if let Some(kind) = params.kind.as_deref().filter(|kind| !kind.is_empty()) {
        let kind = EntryKind::parse(kind).ok_or_else(|| format!("unknown type `{kind}`"))?;
        filter = filter.and(Term::Type(kind));
    }
    Ok(filter)
}

/// `base?q=<q>` (and `&type=<kind>`).
fn list_href(base: &str, q: &str, kind: Option<&str>) -> String {
    let mut href = base.to_owned();
    let mut query = form_urlencoded::Serializer::new(String::new());
    if !q.is_empty() {
        query.append_pair("q", q);
    }
    if let Some(kind) = kind {
        query.append_pair("type", kind);
    }
    let query = query.finish();
    if !query.is_empty() {
        href.push('?');
        href.push_str(&query);
    }
    href
}

/// A duration for people: `0.42 ms`, `4.1 ms`, `64 ms`, `1.24 s`.
pub(crate) fn format_ms(ms: f64) -> String {
    if ms >= 1000.0 {
        format!("{:.2} s", ms / 1000.0)
    } else if ms >= 10.0 {
        format!("{ms:.0} ms")
    } else if ms >= 1.0 {
        format!("{ms:.1} ms")
    } else {
        format!("{ms:.2} ms")
    }
}

/// The singular label of a kind, for rows.
fn row_label(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::Request => "Request",
        EntryKind::Query => "Query",
        EntryKind::Job => "Job",
        EntryKind::Mail => "Mail",
        EntryKind::Event => "Event",
        EntryKind::Cache => "Cache",
        EntryKind::Log => "Log",
        EntryKind::Exception => "Exception",
        EntryKind::HttpClient => "HTTP",
        EntryKind::Notification => "Notification",
        EntryKind::Model => "Data change",
        EntryKind::Schedule => "Task",
        EntryKind::Dump => "Dump",
    }
}

fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut short: String = text.chars().take(max).collect();
    short.push('…');
    short
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

#[derive(Serialize)]
struct Counter {
    count: u32,
    short: &'static str,
    label: &'static str,
}

#[derive(Serialize)]
struct Row {
    id: String,
    seq: u64,
    href: String,
    kind: &'static str,
    kind_label: &'static str,
    title: String,
    status: String,
    duration: String,
    user: String,
    user_short: String,
    user_href: String,
    counters: Vec<Counter>,
    amber: bool,
    flags: Vec<&'static str>,
    at: String,
    time: String,
}

/// The flags of an entry: failed, slow, repeated queries.
fn flags(entry: &Entry) -> Vec<&'static str> {
    let mut flags = Vec::new();
    if entry.failed {
        flags.push("failed");
    }
    if entry.slow {
        flags.push("slow");
    }
    if entry.summary().is_some_and(|s| s.duplicate_queries > 0) {
        flags.push("repeated queries");
    }
    flags
}

fn row(base: &str, entry: &Entry) -> Row {
    let user = entry.user_id.clone().unwrap_or_default();
    let mut counters = Vec::new();
    if let Some(summary) = entry.summary() {
        for (count, short, label) in [
            (summary.queries, "q", "queries"),
            (summary.events, "ev", "events"),
            (summary.mail, "mail", "mail"),
            (summary.logs, "log", "log lines"),
            (summary.exceptions, "err", "exceptions"),
            (summary.http_client, "http", "outgoing HTTP calls"),
        ] {
            if count > 0 {
                counters.push(Counter {
                    count,
                    short,
                    label,
                });
            }
        }
    }
    let flags = flags(entry);
    Row {
        id: entry.id.clone(),
        seq: entry.seq,
        href: format!("{base}/entries/{}", entry.id),
        kind: entry.kind().as_str(),
        kind_label: row_label(entry.kind()),
        title: shorten(&entry.title(), 240),
        status: entry.status().map(|s| s.to_string()).unwrap_or_default(),
        duration: entry.duration_ms.map(format_ms).unwrap_or_default(),
        user_short: user.chars().take(8).collect(),
        user_href: list_href(base, &format!("user:{user}"), None),
        user,
        counters,
        amber: !flags.is_empty(),
        flags,
        at: entry.at.to_rfc3339(),
        time: entry.at.format("%H:%M:%S%.3f").to_string(),
    }
}

/// A labelled value of the summary, maybe linking to a filter.
#[derive(Serialize)]
struct Fact {
    label: &'static str,
    value: String,
    href: String,
}

#[derive(Serialize)]
struct Item {
    id: String,
    selected: bool,
    title: String,
    meta: String,
    body: String,
    href: String,
    html_href: String,
}

#[derive(Serialize)]
struct Section {
    id: &'static str,
    label: &'static str,
    items: Vec<Item>,
}

#[derive(Serialize)]
struct Bar {
    id: String,
    label: String,
    kind: &'static str,
    left: String,
    width: String,
    duration: String,
}

impl LogbookController {
    fn base(&self) -> &str {
        &self.logbook.options().path
    }

    fn render(&self, name: &str, context: Value) -> Result<String, AppError> {
        self.views.render(name, context).map_err(AppError::internal)
    }

    async fn index(&self, params: ListParams) -> Result<Response, AppError> {
        let base = self.base().to_owned();
        let q = params.q.clone().unwrap_or_default();
        let kind = params
            .kind
            .clone()
            .filter(|kind| EntryKind::parse(kind).is_some());
        let (rows, error) = match filter_of(&params) {
            Ok(filter) => {
                let query = ListQuery {
                    filter,
                    before: params.before,
                    after: None,
                    limit: PAGE,
                };
                (
                    self.logbook
                        .entries(&query)
                        .await
                        .map_err(LogbookError::into_app_error)?,
                    String::new(),
                )
            }
            Err(error) => (Vec::new(), error),
        };
        let latest = if params.before.is_none() {
            rows.first().map_or(0, |entry| entry.seq)
        } else {
            0
        };
        let next_href = if rows.len() == PAGE {
            let mut href = list_href(&base, &q, kind.as_deref());
            let last = rows.last().map_or(0, |entry| entry.seq);
            href.push(if href.contains('?') { '&' } else { '?' });
            let _ = write!(href, "before={last}");
            href
        } else {
            String::new()
        };
        let live_href = if params.before.is_none() && error.is_empty() {
            let mut href = list_href(&format!("{base}/rows"), &q, kind.as_deref());
            href.push(if href.contains('?') { '&' } else { '?' });
            href.push_str("live=1");
            href
        } else {
            String::new()
        };
        let buttons: Vec<Value> = EntryKind::ALL
            .iter()
            .map(|k| {
                json!({
                    "label": k.label(),
                    "href": list_href(&base, &q, Some(k.as_str())),
                    "on": kind.as_deref().and_then(EntryKind::parse) == Some(*k),
                })
            })
            .collect();
        let rows: Vec<Row> = rows.iter().map(|entry| row(&base, entry)).collect();
        let page = self.render(
            "logbook/index.html",
            json!({
                "base": base,
                "q": q,
                "kind": kind.clone().unwrap_or_default(),
                "all_href": list_href(&base, &q, None),
                "buttons": buttons,
                "error": error,
                "rows": rows,
                "latest": latest,
                "live_href": live_href,
                "next_href": next_href,
            }),
        )?;
        Ok(html(page))
    }

    /// New rows for the live list (`after`), as HTML.
    async fn rows(&self, params: ListParams) -> Result<Response, AppError> {
        let filter = filter_of(&params).map_err(AppError::bad_request)?;
        let after = params.after.unwrap_or(0);
        let query = ListQuery {
            filter,
            before: None,
            after: Some(after),
            limit: PAGE,
        };
        let entries = self
            .logbook
            .entries(&query)
            .await
            .map_err(LogbookError::into_app_error)?;
        let latest = entries.first().map_or(after, |entry| entry.seq);
        let base = self.base().to_owned();
        let rows: Vec<Row> = entries.iter().map(|entry| row(&base, entry)).collect();
        let body = self.render("logbook/rows.html", json!({ "rows": rows }))?;
        let mut response = html(body);
        if let Ok(value) = HeaderValue::from_str(&latest.to_string()) {
            response
                .headers_mut()
                .insert(HeaderName::from_static("x-logbook-latest"), value);
        }
        Ok(response)
    }

    /// An entry by id, or the head of a batch by batch id.
    async fn find(&self, id: &str) -> Result<Entry, AppError> {
        if let Some(entry) = self
            .logbook
            .entry(id)
            .await
            .map_err(LogbookError::into_app_error)?
        {
            return Ok(entry);
        }
        self.logbook
            .batch(id)
            .await
            .and_then(|batch| batch.head().cloned())
            .ok_or_else(AppError::not_found)
    }

    async fn detail(&self, id: String) -> Result<Response, AppError> {
        let selected = self.find(&id).await?;
        let batch = match &selected.batch_id {
            Some(batch_id) => self.logbook.batch(batch_id).await,
            None => None,
        };
        let entries: Vec<Entry> = batch
            .as_ref()
            .map_or_else(|| vec![selected.clone()], |batch| batch.entries().to_vec());
        let head = batch
            .as_ref()
            .and_then(Batch::head)
            .cloned()
            .unwrap_or_else(|| selected.clone());
        let context = self.detail_context(&selected, &head, &entries).await?;
        Ok(html(self.render("logbook/detail.html", context)?))
    }

    async fn detail_context(
        &self,
        selected: &Entry,
        head: &Entry,
        entries: &[Entry],
    ) -> Result<Value, AppError> {
        let base = self.base().to_owned();
        let filter = |q: String| list_href(&base, &q, None);
        let mut facts = Vec::new();
        let mut fact = |label: &'static str, value: String, href: String| {
            if !value.is_empty() {
                facts.push(Fact { label, value, href });
            }
        };
        let mut jobs_in_trace: Vec<Entry> = Vec::new();
        match &head.content {
            Content::Request(request) => {
                fact("Method", request.method.clone(), String::new());
                fact("Path", request.path.clone(), String::new());
                if let Some(route) = &request.route {
                    fact("Route", route.clone(), filter(format!("route:{route}")));
                }
                fact(
                    "Status",
                    request.status.to_string(),
                    filter(format!("status:{}", request.status)),
                );
                fact("Duration", format_ms(request.duration_ms), String::new());
                fact(
                    "Request id",
                    request.request_id.clone(),
                    filter(format!("batch:{}", request.request_id)),
                );
                fact(
                    "Client",
                    request.client_ip.clone().unwrap_or_default(),
                    String::new(),
                );
                if let Some(original) = &request.replay_of {
                    fact(
                        "Replay of",
                        original.clone(),
                        format!("{base}/entries/{original}"),
                    );
                }
            }
            Content::Job(job) => {
                fact(
                    "Type",
                    job.job_type.clone(),
                    filter(format!("tag:job:{}", job.job_type)),
                );
                fact(
                    "Queue",
                    job.queue.clone(),
                    filter(format!("tag:queue:{}", job.queue)),
                );
                fact(
                    "Job id",
                    job.job_id.clone(),
                    filter(format!("batch:{}", job.job_id)),
                );
                fact("Attempt", job.attempt.to_string(), String::new());
                fact("Outcome", job.outcome.as_str().to_owned(), String::new());
                fact("Duration", format_ms(job.duration_ms), String::new());
                fact(
                    "Error",
                    job.error.clone().unwrap_or_default(),
                    String::new(),
                );
            }
            Content::Schedule(task) => {
                fact("Task", task.task.clone(), String::new());
                fact("Due", task.tick.to_rfc3339(), String::new());
                fact("Duration", format_ms(task.duration_ms), String::new());
                fact(
                    "Error",
                    task.error.clone().unwrap_or_default(),
                    String::new(),
                );
            }
            _ => {
                fact("Recorded", head.at.to_rfc3339(), String::new());
                if let Some(ms) = head.duration_ms {
                    fact("Duration", format_ms(ms), String::new());
                }
            }
        }
        if let Some(user) = &head.user_id {
            fact("User", user.clone(), filter(format!("user:{user}")));
        }
        if let Some(workspace) = &head.workspace_id {
            fact("Workspace", workspace.clone(), String::new());
        }
        if let Some(trace) = &head.trace_id {
            fact("Trace", trace.clone(), filter(format!("trace:{trace}")));
            if head.kind() == EntryKind::Request {
                let query = ListQuery::new(
                    Filter::of([Term::Trace(trace.clone()), Term::Type(EntryKind::Job)]),
                    100,
                );
                jobs_in_trace = self
                    .logbook
                    .entries(&query)
                    .await
                    .map_err(LogbookError::into_app_error)?;
            } else if head.kind() == EntryKind::Job {
                let query = ListQuery::new(
                    Filter::of([Term::Trace(trace.clone()), Term::Type(EntryKind::Request)]),
                    1,
                );
                if let Some(request) = self
                    .logbook
                    .entries(&query)
                    .await
                    .map_err(LogbookError::into_app_error)?
                    .pop()
                {
                    fact(
                        "Dispatched by",
                        request.title(),
                        format!("{base}/entries/{}", request.id),
                    );
                }
            }
        }
        let tags: Vec<Value> = head
            .tags
            .iter()
            .map(|tag| json!({ "tag": tag, "href": filter(format!("tag:{tag}")) }))
            .collect();

        let mut tabs: Vec<Value> = Vec::new();
        let mut tab = |id: &str, label: &str, count: usize| {
            tabs.push(json!({ "id": id, "label": label, "count": count }));
        };

        let (request, response) = match &head.content {
            Content::Request(request) => (
                request.request.as_ref().map(message_view),
                request.response.as_ref().map(message_view),
            ),
            _ => (None, None),
        };
        if request.is_some() {
            tab("request", "Request", 0);
        }
        if response.is_some() {
            tab("response", "Response", 0);
        }

        let queries: Vec<&crate::QueryEntry> = entries
            .iter()
            .filter_map(|e| match &e.content {
                Content::Query(q) => Some(q),
                _ => None,
            })
            .collect();
        let repeated = duplicates(queries.clone(), self.logbook.options().duplicate_queries);
        let query_rows: Vec<Value> = queries
            .iter()
            .map(|q| {
                json!({
                    "sql": q.sql,
                    "duration": format_ms(q.duration_ms),
                    "slow": q.slow,
                    "duplicate": repeated.iter().any(|d| d.sql == q.sql),
                })
            })
            .collect();
        let duplicate_rows: Vec<Value> = repeated
            .iter()
            .map(|d| json!({ "sql": d.sql, "count": d.count, "total": format_ms(d.total_ms) }))
            .collect();
        if !queries.is_empty() {
            tab("queries", "Queries", queries.len());
        }

        let sections = self.sections(&base, selected, head, entries, &jobs_in_trace);
        for section in &sections {
            tab(section.id, section.label, section.items.len());
        }
        let timeline = timeline(head, entries);
        if !timeline.is_empty() {
            tab("timeline", "Timeline", 0);
        }
        let replay = match &head.content {
            Content::Request(request) if self.logbook.options().replay => {
                let message = request.request.as_ref();
                let headers: String = message
                    .map(|m| {
                        m.headers
                            .iter()
                            .filter(|(name, _)| {
                                !["host", "content-length", "x-request-id"]
                                    .contains(&name.to_ascii_lowercase().as_str())
                            })
                            .map(|(name, value)| format!("{name}: {value}\n"))
                            .collect()
                    })
                    .unwrap_or_default();
                let path = match message.and_then(|m| m.query.as_deref()) {
                    Some(query) if !query.is_empty() => format!("{}?{query}", request.path),
                    _ => request.path.clone(),
                };
                Some(json!({
                    "action": format!("{base}/api/requests/{}/replay", head.id),
                    "curl": format!("{base}/api/requests/{}/curl", head.id),
                    "method": request.method,
                    "path": path,
                    "headers": headers,
                    "body": message
                        .and_then(|m| m.body.as_ref())
                        .and_then(|b| b.text.clone())
                        .unwrap_or_default(),
                    "user": head.user_id.clone().unwrap_or_default(),
                }))
            }
            _ => None,
        };
        if replay.is_some() {
            tab("replay", "Replay", 0);
        }
        let head_flags = flags(head);
        Ok(json!({
            "base": base,
            "q": "",
            "kind": "",
            "head": {
                "title": shorten(&head.title(), 300),
                "kind": head.kind().as_str(),
                "kind_label": row_label(head.kind()),
                "rows": facts,
                "tags": tags,
                "amber": !head_flags.is_empty(),
                "flags": head_flags,
            },
            "tabs": tabs,
            "request": request,
            "response": response,
            "queries": query_rows,
            "duplicates": duplicate_rows,
            "sections": sections,
            "timeline": timeline,
            "replay": replay,
        }))
    }

    /// Every section but the request, response and queries.
    fn sections(
        &self,
        base: &str,
        selected: &Entry,
        head: &Entry,
        entries: &[Entry],
        jobs_in_trace: &[Entry],
    ) -> Vec<Section> {
        let order = [
            (EntryKind::Event, "events", "Events"),
            (EntryKind::Job, "jobs", "Jobs"),
            (EntryKind::Mail, "mail", "Mail"),
            (EntryKind::Cache, "cache", "Cache"),
            (EntryKind::Log, "logs", "Logs"),
            (EntryKind::Exception, "exceptions", "Exceptions"),
            (EntryKind::HttpClient, "http", "Outgoing HTTP"),
            (EntryKind::Notification, "notifications", "Notifications"),
            (EntryKind::Model, "models", "Data changes"),
            (EntryKind::Dump, "dumps", "Dumps"),
        ];
        let mut sections = Vec::new();
        for (kind, id, label) in order {
            let source: Vec<&Entry> = if kind == EntryKind::Job && head.kind() == EntryKind::Request
            {
                jobs_in_trace.iter().collect()
            } else {
                entries
                    .iter()
                    .filter(|e| e.kind() == kind && (e.id != head.id || !kind.is_batch()))
                    .collect()
            };
            if source.is_empty() && !(kind == selected.kind() && selected.id != head.id) {
                continue;
            }
            let items: Vec<Item> = source
                .into_iter()
                .map(|entry| item(base, entry, entry.id == selected.id, kind == EntryKind::Job))
                .collect();
            if !items.is_empty() {
                sections.push(Section { id, label, items });
            }
        }
        sections
    }

    async fn mail_html(&self, id: String) -> Result<Response, AppError> {
        let entry = self
            .logbook
            .entry(&id)
            .await
            .map_err(LogbookError::into_app_error)?
            .ok_or_else(AppError::not_found)?;
        let Content::Mail(mail) = &entry.content else {
            return Err(AppError::not_found());
        };
        let body = match (&mail.html, &mail.text) {
            (Some(html), _) => html.clone(),
            (None, Some(text)) => format!("<pre>{}</pre>", escape(text)),
            (None, None) => String::new(),
        };
        Ok((
            [
                (
                    CONTENT_TYPE,
                    HeaderValue::from_static("text/html; charset=utf-8"),
                ),
                (CONTENT_SECURITY_POLICY, HeaderValue::from_static(MAIL_CSP)),
                (CACHE_CONTROL, HeaderValue::from_static("no-store")),
            ],
            body,
        )
            .into_response())
    }

    async fn api_list(&self, params: ListParams) -> Result<Json<Value>, AppError> {
        let filter = filter_of(&params).map_err(AppError::bad_request)?;
        let limit = params.limit.unwrap_or(PAGE).clamp(1, 500);
        let query = ListQuery {
            filter,
            before: params.before,
            after: params.after,
            limit,
        };
        let entries = self
            .logbook
            .entries(&query)
            .await
            .map_err(LogbookError::into_app_error)?;
        let next = (entries.len() == limit)
            .then(|| entries.last().map(|entry| entry.seq))
            .flatten();
        Ok(Json(json!({ "data": entries, "next": next })))
    }

    async fn api_entry(&self, id: String) -> Result<Json<Entry>, AppError> {
        Ok(Json(self.find(&id).await?))
    }

    async fn api_batch(&self, id: String) -> Result<Json<Value>, AppError> {
        let batch = self
            .logbook
            .batch(&id)
            .await
            .ok_or_else(AppError::not_found)?;
        Ok(Json(json!({
            "id": batch.id(),
            "entries": batch.entries(),
            "duplicate_queries": batch.duplicate_queries(),
        })))
    }

    async fn api_clear(&self) -> Result<Json<Value>, AppError> {
        let deleted = self
            .logbook
            .clear()
            .await
            .map_err(LogbookError::into_app_error)?;
        Ok(Json(json!({ "deleted": deleted })))
    }

    async fn api_replay(&self, id: String, edit: ReplayRequest) -> Result<Response, AppError> {
        let replayed = self
            .logbook
            .replay(&id, edit)
            .await
            .map_err(LogbookError::into_app_error)?;
        Ok((StatusCode::OK, Json(replayed)).into_response())
    }

    async fn api_curl(&self, id: String) -> Result<Response, AppError> {
        let curl = self
            .logbook
            .curl(&id)
            .await
            .map_err(LogbookError::into_app_error)?;
        Ok((
            [(
                CONTENT_TYPE,
                HeaderValue::from_static("text/plain; charset=utf-8"),
            )],
            curl,
        )
            .into_response())
    }
}

/// The view of a captured request or response.
fn message_view(message: &crate::HttpMessage) -> Value {
    let (body, body_meta) = match &message.body {
        Some(body) => {
            let mut meta = Vec::new();
            if let Some(content_type) = &body.content_type {
                meta.push(content_type.clone());
            }
            if let Some(size) = body.size {
                meta.push(format!("{size} bytes"));
            }
            if body.truncated {
                meta.push("cut at logbook.capture.max_body_kb".to_owned());
            }
            if body.text.is_none() {
                meta.push("binary, not stored".to_owned());
            }
            let text = body.text.clone().unwrap_or_default();
            let text = match serde_json::from_str::<Value>(&text) {
                Ok(value) if !body.truncated => pretty(&value),
                _ => text,
            };
            (text, meta.join(" · "))
        }
        None => (String::new(), String::new()),
    };
    json!({
        "headers": message.headers,
        "query": message.query.clone().unwrap_or_default(),
        "body": body,
        "body_meta": body_meta,
    })
}

/// One entry in a section.
fn item(base: &str, entry: &Entry, selected: bool, link: bool) -> Item {
    let duration = entry.duration_ms.map(format_ms).unwrap_or_default();
    let (title, meta, body, html_href) = match &entry.content {
        Content::Event(e) => {
            let mut meta = Vec::new();
            if !e.listeners.is_empty() {
                meta.push(format!("ran: {}", e.listeners.join(", ")));
            }
            if !e.queued.is_empty() {
                meta.push(format!("queued: {}", e.queued.join(", ")));
            }
            if !e.failed.is_empty() {
                meta.push(format!("failed: {}", e.failed.join(", ")));
            }
            (
                e.name.clone(),
                meta.join(" · "),
                pretty(&e.payload),
                String::new(),
            )
        }
        Content::Job(j) => (
            j.job_type.clone(),
            format!(
                "attempt {} · {} · {} · queue {}",
                j.attempt,
                j.outcome.as_str(),
                format_ms(j.duration_ms),
                j.queue
            ),
            j.error.clone().unwrap_or_default(),
            String::new(),
        ),
        Content::Mail(m) => (
            m.subject.clone(),
            format!(
                "to {}{} · from {} · {}",
                m.to.join(", "),
                if m.cc.is_empty() {
                    String::new()
                } else {
                    format!(" · cc {}", m.cc.join(", "))
                },
                m.from.clone().unwrap_or_default(),
                m.transport
            ),
            m.text.clone().unwrap_or_default(),
            if m.html.is_some() || m.text.is_some() {
                format!("{base}/mail/{}", entry.id)
            } else {
                String::new()
            },
        ),
        Content::Cache(c) => (
            format!("{} {}", c.operation, c.key),
            format!("store {}", c.store),
            String::new(),
            String::new(),
        ),
        Content::Log(l) => (
            l.message.clone(),
            format!("{} {}", l.level, l.target),
            if l.fields.is_empty() {
                String::new()
            } else {
                pretty(&serde_json::to_value(&l.fields).unwrap_or_default())
            },
            String::new(),
        ),
        Content::Exception(x) => (
            x.message.clone(),
            [x.panic.then(|| "panic".to_owned()), x.location.clone()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · "),
            x.chain.join("\n  caused by: "),
            String::new(),
        ),
        Content::HttpClient(h) => (
            format!("{} {}", h.method, h.url),
            format!(
                "{} · {} attempt(s) · {duration}",
                h.status
                    .map_or_else(|| "no response".to_owned(), |s| s.to_string()),
                h.attempts
            ),
            h.error.clone().unwrap_or_default(),
            String::new(),
        ),
        Content::Notification(n) => (
            format!("{} via {}", n.notification_type, n.channel),
            format!(
                "to {} {} · {}",
                n.notifiable_type, n.notifiable_id, n.notification_id
            ),
            String::new(),
            String::new(),
        ),
        Content::Model(m) => (
            format!("{} {} {}", m.entity, m.op, m.id),
            entry.tags.join(" "),
            pretty(&json!({ "old": m.old, "new": m.new })),
            String::new(),
        ),
        Content::Dump(d) => (
            d.label.clone(),
            String::new(),
            pretty(&d.value),
            String::new(),
        ),
        other => (
            entry.title(),
            duration.clone(),
            pretty(&json!(other)),
            String::new(),
        ),
    };
    Item {
        id: entry.id.clone(),
        selected,
        title,
        meta,
        body,
        href: if link {
            format!("{base}/entries/{}", entry.id)
        } else {
            String::new()
        },
        html_href,
    }
}

/// The waterfall: every entry of the batch against the batch's duration.
fn timeline(head: &Entry, entries: &[Entry]) -> Vec<Bar> {
    if entries.len() < 2 && !head.kind().is_batch() {
        return Vec::new();
    }
    let start = head.at;
    let end_of = |e: &Entry| offset_ms(start, e.at) + e.duration_ms.unwrap_or(0.0);
    let total = head
        .duration_ms
        .unwrap_or(0.0)
        .max(entries.iter().map(end_of).fold(0.0, f64::max))
        .max(0.001);
    let mut ordered: Vec<&Entry> = entries.iter().collect();
    ordered.sort_by(|a, b| a.at.cmp(&b.at).then(a.seq.cmp(&b.seq)));
    ordered
        .into_iter()
        .map(|e| {
            let left = (offset_ms(start, e.at) / total * 100.0).clamp(0.0, 100.0);
            let width = (e.duration_ms.unwrap_or(0.0) / total * 100.0).clamp(0.0, 100.0 - left);
            Bar {
                id: e.id.clone(),
                label: shorten(&format!("{}: {}", row_label(e.kind()), e.title()), 120),
                kind: e.kind().as_str(),
                left: format!("{left:.2}"),
                width: format!("{width:.2}"),
                duration: e.duration_ms.map(format_ms).unwrap_or_default(),
            }
        })
        .collect()
}

fn offset_ms(start: DateTime<Utc>, at: DateTime<Utc>) -> f64 {
    (at - start)
        .num_microseconds()
        .map_or(0.0, |us| us as f64 / 1000.0)
        .max(0.0)
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_read_well() {
        assert_eq!(format_ms(0.421), "0.42 ms");
        assert_eq!(format_ms(4.12), "4.1 ms");
        assert_eq!(format_ms(64.4), "64 ms");
        assert_eq!(format_ms(1240.0), "1.24 s");
    }

    #[test]
    fn list_links_encode_the_filter() {
        assert_eq!(list_href("/_logbook", "", None), "/_logbook");
        assert_eq!(
            list_href("/_logbook", "user:a b", Some("query")),
            "/_logbook?q=user%3Aa+b&type=query"
        );
    }

    #[test]
    fn the_templates_compile() {
        let views = Views::from_templates(TEMPLATES).unwrap();
        assert!(views.exists("logbook/detail.html"));
    }
}
