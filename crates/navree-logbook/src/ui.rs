//! The timeline UI at `/_logbook` and its JSON API.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use http::header::{CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE};
use http::request::Parts;
use http::{HeaderName, HeaderValue, Method, StatusCode};
use navree_http::response::{IntoResponse, Response};
use navree_http::{AppError, ClientIp, Controller, Guard, Json, Path, Query, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
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

#[path = "ui/detail_page.rs"]
mod detail_page;
#[path = "ui/list_page.rs"]
mod list_page;
#[path = "ui/screen.rs"]
mod screen;

/// Serves the UI and its API under `logbook.path` (only when Logbook is on).
pub(crate) struct LogbookController {
    logbook: Arc<Logbook>,
}

impl Injectable for LogbookController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Logbook>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            let logbook = r.get::<Logbook>().await?;
            Ok(LogbookController { logbook })
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

/// Refuses a write that a browser says came from another site.
///
/// A plain form can post to this app from any page the developer has open, and
/// Logbook can clear itself and replay requests. Browsers name the origin of a
/// request in `Sec-Fetch-Site`, or in `Origin` when they predate it; a client that
/// sends neither (curl, a test) is not a browser acting for a page, and passes.
#[derive(Debug)]
pub(crate) struct SameOrigin;

impl SameOrigin {
    /// Whether a request with these headers may write.
    pub(crate) fn allows(headers: &http::HeaderMap) -> bool {
        if let Some(site) = headers.get("sec-fetch-site") {
            return matches!(site.as_bytes(), b"same-origin" | b"none");
        }
        let Some(origin) = headers.get(http::header::ORIGIN) else {
            return true;
        };
        let host = headers
            .get(http::header::HOST)
            .and_then(|host| host.to_str().ok());
        let authority = origin
            .to_str()
            .ok()
            .and_then(|origin| origin.split_once("://"))
            .map(|(_, authority)| authority);
        matches!((host, authority), (Some(host), Some(authority)) if host.eq_ignore_ascii_case(authority))
    }
}

impl Guard for SameOrigin {
    fn check<'a>(&'a self, parts: &'a mut Parts) -> BoxFuture<'a, Result<(), AppError>> {
        let allowed = Self::allows(&parts.headers);
        Box::pin(async move {
            if allowed {
                Ok(())
            } else {
                Err(AppError::forbidden())
            }
        })
    }
}

/// The replay form's fields.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct ReplayForm {
    method: String,
    path: String,
    headers: String,
    body: String,
    as_user: Option<String>,
    confirm: Option<String>,
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
        routes.push(
            RouteDef::on(
                Method::GET,
                path(""),
                move |Query(params): Query<ListParams>, parts: Parts| async move {
                    this.index(params, parts).await
                },
            )
            .name("logbook.index"),
        );
        let this = self.clone();
        routes.push(
            RouteDef::on(
                Method::GET,
                path("/rows"),
                move |Query(params): Query<ListParams>| async move { this.rows(params).await },
            )
            .name("logbook.rows"),
        );
        let this = self.clone();
        routes.push(
            RouteDef::on(
                Method::GET,
                path("/entries/{id}"),
                move |Path(id): Path<String>, Query(params): Query<ListParams>, parts: Parts| async move {
                this.detail(id, params, parts).await
            },
            )
            .name("logbook.entry"),
        );
        let this = self.clone();
        routes.push(
            RouteDef::on(
                Method::POST,
                path("/entries/{id}/replay"),
                move |Path(id): Path<String>,
                      navree_http::Form(form): navree_http::Form<ReplayForm>| async move {
                    this.replay_form(id, form).await
                },
            )
            .name("logbook.replay"),
        );
        let this = self.clone();
        routes.push(
            RouteDef::on(
                Method::GET,
                path("/mail/{id}"),
                move |Path(id): Path<String>| async move { this.mail_html(id).await },
            )
            .name("logbook.mail"),
        );
        let this = self.clone();
        routes.push(
            RouteDef::on(
                Method::GET,
                path("/api/entries"),
                move |Query(params): Query<ListParams>| async move { this.api_list(params).await },
            )
            .name("logbook.api.entries"),
        );
        let this = self.clone();
        routes.push(
            RouteDef::on(Method::POST, path("/clear"), move || async move {
                this.clear().await
            })
            .name("logbook.clear"),
        );
        let this = self.clone();
        routes.push(
            RouteDef::on(Method::DELETE, path("/api/entries"), move || async move {
                this.api_clear().await
            })
            .name("logbook.api.clear"),
        );
        let this = self.clone();
        routes.push(
            RouteDef::on(
                Method::GET,
                path("/api/entries/{id}"),
                move |Path(id): Path<String>| async move { this.api_entry(id).await },
            )
            .name("logbook.api.entry"),
        );
        let this = self.clone();
        routes.push(
            RouteDef::on(
                Method::GET,
                path("/api/batches/{id}"),
                move |Path(id): Path<String>| async move { this.api_batch(id).await },
            )
            .name("logbook.api.batch"),
        );
        let this = self.clone();
        routes.push(
            RouteDef::on(
                Method::POST,
                path("/api/requests/{id}/replay"),
                move |Path(id): Path<String>, Json(edit): Json<ReplayRequest>| async move {
                    this.api_replay(id, edit).await
                },
            )
            .name("logbook.api.replay"),
        );
        let this = self.clone();
        routes.push(
            RouteDef::on(
                Method::GET,
                path("/api/requests/{id}/curl"),
                move |Path(id): Path<String>| async move { this.api_curl(id).await },
            )
            .name("logbook.api.curl"),
        );

        let remote = self.logbook.options().allow_remote;
        routes
            .into_iter()
            .map(|mut route| {
                if route.method != Method::GET {
                    route = route.guard(Arc::new(SameOrigin));
                }
                if remote {
                    route.permissions(&[VIEW_PERMISSION])
                } else {
                    route.public().guard(Arc::new(LoopbackOnly))
                }
            })
            .collect()
    }
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
    href_with(base, q, kind, None)
}

/// `path?q=<q>&type=<kind>&before=<seq>`: a page with the state of the list in its address.
fn href_with(path: &str, q: &str, kind: Option<&str>, before: Option<u64>) -> String {
    let mut href = path.to_owned();
    let mut query = form_urlencoded::Serializer::new(String::new());
    if !q.is_empty() {
        query.append_pair("q", q);
    }
    if let Some(kind) = kind {
        query.append_pair("type", kind);
    }
    if let Some(before) = before {
        query.append_pair("before", &before.to_string());
    }
    let query = query.finish();
    if !query.is_empty() {
        href.push('?');
        href.push_str(&query);
    }
    href
}

/// Gives a long run of text without a space somewhere to break, so that a path or
/// an identifier a phone can't fit wraps instead of widening the page. Text that
/// already has room to wrap is left as it is.
pub(crate) fn breakable(text: &str) -> String {
    break_runs(text, 16, 40)
}

/// [`breakable`] for text set large or in a narrow column (a heading, a row of the
/// list), where a run has to be shorter to fit a phone.
pub(crate) fn tight(text: &str) -> String {
    break_runs(text, 12, 24)
}

/// Puts a break in a run of `hard` characters without a space, and after
/// punctuation once it is `soft` long.
fn break_runs(text: &str, soft: usize, hard: usize) -> String {
    let mut out = String::with_capacity(text.len() + text.len() / soft);
    let mut run = 0;
    for c in text.chars() {
        out.push(c);
        if c.is_whitespace() || c == '\u{200b}' {
            run = 0;
            continue;
        }
        run += 1;
        // A hyphen is a place to break already, and an identifier's hyphens are
        // left alone so that copying one copies it whole.
        let punctuation = matches!(c, '/' | '?' | '&' | '=' | '_' | '.' | ':' | ',' | ';');
        if (punctuation && run >= soft) || run >= hard {
            out.push('\u{200b}');
            run = 0;
        }
    }
    out
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
        EntryKind::Ai => "AI call",
        EntryKind::Broadcast => "Broadcast",
        EntryKind::Storage => "Storage",
        EntryKind::Webhook => "Webhook",
        EntryKind::Search => "Search",
        EntryKind::SignIn => "Sign-in",
        EntryKind::Ui => "UI",
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

/// One entry of the list.
#[derive(Serialize)]
pub(crate) struct Row {
    id: String,
    seq: u64,
    href: String,
    /// What kind of entry, for those that aren't requests.
    prefix: Option<&'static str>,
    title: String,
    /// A request's status, a job's outcome.
    status: String,
    duration: String,
    counters: Vec<Counter>,
    flags: Vec<&'static str>,
    at: String,
    time: String,
    selected: bool,
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

/// The word before a title that says what it is, for entries that aren't requests.
fn prefix_of(kind: EntryKind) -> Option<&'static str> {
    match kind {
        EntryKind::Request => None,
        EntryKind::Query => Some("query"),
        EntryKind::Job => Some("job"),
        EntryKind::Mail => Some("mail"),
        EntryKind::Event => Some("event"),
        EntryKind::Cache => Some("cache"),
        EntryKind::Log => Some("log"),
        EntryKind::Exception => Some("error"),
        EntryKind::HttpClient => Some("http"),
        EntryKind::Notification => Some("notification"),
        EntryKind::Model => Some("change"),
        EntryKind::Schedule => Some("task"),
        EntryKind::Dump => Some("dump"),
        EntryKind::Ai => Some("ai"),
        EntryKind::Broadcast => Some("broadcast"),
        EntryKind::Storage => Some("storage"),
        EntryKind::Webhook => Some("webhook"),
        EntryKind::Search => Some("search"),
        EntryKind::SignIn => Some("sign-in"),
        EntryKind::Ui => Some("ui"),
    }
}

/// What the list says of an entry's result: the status of a request, the outcome
/// of a job, whether a task ran or failed.
fn status_of(entry: &Entry) -> String {
    match &entry.content {
        Content::Request(request) => request.status.to_string(),
        Content::Job(job) => job.outcome.as_str().to_owned(),
        Content::Schedule(task) if task.error.is_some() => "failed".to_owned(),
        Content::Schedule(_) => "ran".to_owned(),
        Content::SignIn(sign_in) => sign_in.outcome.clone(),
        Content::Webhook(hook) => hook
            .status
            .map_or_else(|| "no answer".to_owned(), |status| status.to_string()),
        Content::Ui(op) => op.status.to_string(),
        _ => String::new(),
    }
}

/// How a row's links keep the list the user is looking at.
struct ListState<'a> {
    base: &'a str,
    q: &'a str,
    kind: Option<&'a str>,
    /// The ids of the entry that is open: its row is marked.
    open: &'a [String],
}

fn row(state: &ListState<'_>, entry: &Entry) -> Row {
    let mut counters = Vec::new();
    if let Some(summary) = entry.summary() {
        for (count, short, label) in [
            (summary.queries, "q", "queries"),
            (summary.events, "ev", "events"),
            (summary.jobs, "job", "jobs"),
            (summary.mail, "mail", "mail"),
            (summary.cache, "cache", "cache calls"),
            (summary.logs, "log", "log lines"),
            (summary.exceptions, "err", "exceptions"),
            (summary.http_client, "http", "outgoing HTTP calls"),
            (summary.notifications, "notif", "notifications"),
            (summary.models, "chg", "data changes"),
            (summary.dumps, "dump", "dumps"),
            (summary.ai_calls, "ai", "AI calls"),
            (summary.broadcasts, "bcast", "broadcasts"),
            (summary.storage_ops, "file", "file operations"),
            (summary.webhooks, "hook", "webhook attempts"),
            (summary.searches, "srch", "searches"),
            (summary.sign_ins, "auth", "sign-ins"),
            (summary.ui_ops, "ui", "UI operations"),
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
    Row {
        id: entry.id.clone(),
        seq: entry.seq,
        href: href_with(
            &format!("{}/entries/{}", state.base, entry.id),
            state.q,
            state.kind,
            None,
        ),
        prefix: prefix_of(entry.kind()),
        title: shorten(&entry.title(), 240),
        status: status_of(entry),
        duration: entry.duration_ms.map(format_ms).unwrap_or_default(),
        counters,
        flags: flags(entry),
        at: entry.at.to_rfc3339(),
        time: entry.at.format("%H:%M:%S%.3f").to_string(),
        selected: state.open.contains(&entry.id),
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

    /// The list, with nothing open beside it.
    async fn index(&self, params: ListParams, parts: Parts) -> Result<Response, AppError> {
        self.screen(params, None, &parts).await
    }

    /// The list, and beside it what the entry `id` did.
    async fn detail(
        &self,
        id: String,
        params: ListParams,
        parts: Parts,
    ) -> Result<Response, AppError> {
        self.screen(params, Some(id), &parts).await
    }

    /// One page of the tool: the list the address asks for and, when an entry is
    /// named, the entry beside it.
    async fn screen(
        &self,
        params: ListParams,
        open: Option<String>,
        parts: &Parts,
    ) -> Result<Response, AppError> {
        let base = self.base().to_owned();
        let q = params.q.clone().unwrap_or_default();
        let kind = params
            .kind
            .clone()
            .filter(|kind| EntryKind::parse(kind).is_some());

        // What is open beside the list: the entry, with the ids that mark its row.
        let (beside, open_ids) = match &open {
            None => (screen::Beside::Nothing, Vec::new()),
            Some(id) => match self.find(id).await {
                Ok(selected) => {
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
                    let detail = self.detail_context(&selected, &head, &entries).await?;
                    (
                        screen::Beside::Entry(Box::new(detail)),
                        vec![selected.id.clone(), head.id.clone()],
                    )
                }
                Err(_) => (screen::Beside::Missing, Vec::new()),
            },
        };

        let (entries, error) = match filter_of(&params) {
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
        let path = parts.uri.path().to_owned();
        let next_href = (entries.len() == PAGE).then(|| {
            href_with(
                &path,
                &q,
                kind.as_deref(),
                Some(entries.last().map_or(0, |entry| entry.seq)),
            )
        });
        let latest = entries.first().map_or(0, |entry| entry.seq);
        let live_href = (params.before.is_none() && error.is_empty())
            .then(|| list_href(&format!("{base}/rows"), &q, kind.as_deref()));
        let state = ListState {
            base: &base,
            q: &q,
            kind: kind.as_deref(),
            open: &open_ids,
        };
        let rows: Vec<Row> = entries.iter().map(|entry| row(&state, entry)).collect();
        let counts = self
            .logbook
            .counts()
            .await
            .map_err(LogbookError::into_app_error)?;
        let options = self.logbook.options();
        let store = match options.store {
            crate::StoreKind::File => options.file.clone(),
            crate::StoreKind::Memory => "in memory".to_owned(),
        };
        let screen = screen::Screen {
            list: list_page::List {
                q: q.clone(),
                kind: kind.clone(),
                error,
                rows,
                live_href,
                latest,
                next_href,
                entry_open: false,
            },
            kind: kind.as_deref().and_then(EntryKind::parse),
            base,
            q,
            counts,
            store,
            path,
            beside,
        };
        let app = screen::shell(&screen).for_request(parts.uri.path(), &parts.headers);
        Ok(screen::page(&app, screen).into_response())
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
        let q = params.q.clone().unwrap_or_default();
        let kind = params
            .kind
            .clone()
            .filter(|kind| EntryKind::parse(kind).is_some());
        let state = ListState {
            base: &base,
            q: &q,
            kind: kind.as_deref(),
            open: &[],
        };
        let rows: Vec<Row> = entries.iter().map(|entry| row(&state, entry)).collect();
        let mut response = html(list_page::rows_fragment(rows));
        if let Ok(value) = HeaderValue::from_str(&latest.to_string()) {
            response
                .headers_mut()
                .insert(HeaderName::from_static("x-ui-cursor"), value);
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

    async fn detail_context(
        &self,
        selected: &Entry,
        head: &Entry,
        entries: &[Entry],
    ) -> Result<detail_page::Detail, AppError> {
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
        let tags: Vec<(String, String)> = head
            .tags
            .iter()
            .map(|tag| (tag.clone(), filter(format!("tag:{tag}"))))
            .collect();

        let (request, response) = match &head.content {
            Content::Request(request) => (
                request.request.as_ref().map(message_view),
                request.response.as_ref().map(message_view),
            ),
            _ => (None, None),
        };

        let queries: Vec<&crate::QueryEntry> = entries
            .iter()
            .filter_map(|e| match &e.content {
                Content::Query(q) => Some(q),
                _ => None,
            })
            .collect();
        let repeated = duplicates(queries.clone(), self.logbook.options().duplicate_queries);
        let statements: Vec<detail_page::Statement> = queries
            .iter()
            .map(|q| detail_page::Statement {
                sql: q.sql.clone(),
                duration: format_ms(q.duration_ms),
                slow: q.slow,
                duplicate: repeated.iter().any(|d| d.sql == q.sql),
            })
            .collect();
        let duplicate_rows: Vec<detail_page::Repeated> = repeated
            .iter()
            .map(|d| detail_page::Repeated {
                sql: d.sql.clone(),
                count: u32::try_from(d.count).unwrap_or(u32::MAX),
                total: format_ms(d.total_ms),
            })
            .collect();

        let sections = self.sections(&base, selected, head, entries, &jobs_in_trace);
        let (timeline, timeline_total) = timeline(head, entries);
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
                Some(detail_page::Replay {
                    action: format!("{base}/entries/{}/replay", head.id),
                    curl: format!("{base}/api/requests/{}/curl", head.id),
                    method: request.method.clone(),
                    path,
                    headers,
                    body: message
                        .and_then(|m| m.body.as_ref())
                        .and_then(|b| b.text.clone())
                        .unwrap_or_default(),
                    user: head.user_id.clone().unwrap_or_default(),
                })
            }
            _ => None,
        };
        Ok(detail_page::Detail {
            title: shorten(&head.title(), 300),
            kind_label: row_label(head.kind()),
            status: status_of(head),
            duration: head.duration_ms.map(format_ms).unwrap_or_default(),
            flags: flags(head),
            facts,
            tags,
            request,
            response,
            statements,
            repeated: duplicate_rows,
            sections,
            timeline,
            timeline_total: format_ms(timeline_total),
            replay,
        })
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
            (EntryKind::Ai, "ai", "AI calls"),
            (EntryKind::Broadcast, "broadcasts", "Broadcasts"),
            (EntryKind::Storage, "storage", "Storage"),
            (EntryKind::Webhook, "webhooks", "Webhooks"),
            (EntryKind::Search, "searches", "Searches"),
            (EntryKind::SignIn, "sign-ins", "Sign-ins"),
            (EntryKind::Ui, "ui", "UI operations"),
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

    /// `POST /clear`: the page's Clear button. A form can only post, so this stays
    /// beside `DELETE /api/entries` (the API's way) to keep the button working with
    /// JavaScript off. Like every write it refuses a request a browser says came
    /// from another site ([`SameOrigin`]).
    async fn clear(&self) -> Result<Response, AppError> {
        let _ = self.api_clear().await?;
        Ok(navree_ui::Reply::redirect(self.base().to_owned()).into_response())
    }

    async fn api_clear(&self) -> Result<Json<Value>, AppError> {
        let deleted = self
            .logbook
            .clear()
            .await
            .map_err(LogbookError::into_app_error)?;
        Ok(Json(json!({ "deleted": deleted })))
    }

    /// `POST /entries/{id}/replay`: the replay form. It answers where the replay
    /// was recorded, or the page again with what went wrong.
    async fn replay_form(&self, id: String, form: ReplayForm) -> Result<Response, AppError> {
        let base = self.base().to_owned();
        let headers: Vec<(String, String)> = form
            .headers
            .lines()
            .filter_map(|line| {
                let (name, value) = line.split_once(':')?;
                let name = name.trim();
                (!name.is_empty()).then(|| (name.to_owned(), value.trim().to_owned()))
            })
            .collect();
        let edit = ReplayRequest {
            method: Some(form.method.trim().to_ascii_uppercase()),
            path: Some(form.path.trim().to_owned()),
            headers: Some(headers),
            body: Some(form.body),
            as_guest: form.as_user.is_none(),
            confirm: form.confirm.is_some(),
        };
        let reply = match self.logbook.replay(&id, edit).await {
            Ok(replayed) => {
                let at = replayed.entry_id.unwrap_or(replayed.batch_id);
                navree_ui::Reply::redirect(format!("{base}/entries/{at}")).toast(
                    navree_ui::ToastTone::Success,
                    format!("Replayed: {}", replayed.status),
                )
            }
            Err(error) => navree_ui::Reply::redirect(format!("{base}/entries/{id}"))
                .toast(navree_ui::ToastTone::Danger, error.to_string()),
        };
        Ok(reply.into_response())
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
fn message_view(message: &crate::HttpMessage) -> detail_page::Message {
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
    detail_page::Message {
        headers: message.headers.clone(),
        query: message.query.clone().unwrap_or_default(),
        body,
        body_meta,
    }
}

/// `1,200 in, 340 out tokens`, with what the provider said.
fn tokens_text(input: Option<u64>, output: Option<u64>) -> Option<String> {
    match (input, output) {
        (None, None) => None,
        (input, output) => Some(format!(
            "{} in, {} out tokens",
            input.map_or_else(|| "?".to_owned(), |n| n.to_string()),
            output.map_or_else(|| "?".to_owned(), |n| n.to_string()),
        )),
    }
}

/// A cost kept in millionths of a US dollar, as `$0.0054`.
fn cost_text(micros: u64) -> String {
    format!("${}.{:06}", micros / 1_000_000, micros % 1_000_000)
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
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
        Content::Ai(a) => (
            format!("{} {}", a.model, a.operation),
            [
                Some(a.provider.clone()),
                tokens_text(a.input_tokens, a.output_tokens),
                a.cached_input_tokens
                    .map(|cached| format!("{cached} from the prompt cache")),
                a.cost_micros.map(cost_text),
                Some(duration.clone()),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · "),
            a.error.clone().unwrap_or_default(),
            String::new(),
        ),
        Content::Broadcast(b) => (
            format!("{} to {}", b.event, b.channels.join(", ")),
            format!("{} delivered · {duration}", b.delivered),
            b.error.clone().unwrap_or_default(),
            String::new(),
        ),
        Content::Storage(s) => (
            format!("{} {}", s.operation, s.path),
            [
                Some(format!("disk {} ({})", s.disk, s.driver)),
                s.bytes.map(|bytes| format!("{bytes} bytes")),
                Some(duration.clone()),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · "),
            s.error.clone().unwrap_or_default(),
            String::new(),
        ),
        Content::Webhook(w) => (
            format!("{} to {}", w.event, w.endpoint),
            format!(
                "attempt {} · {} · {duration}",
                w.attempt,
                w.status
                    .map_or_else(|| "no answer".to_owned(), |status| status.to_string())
            ),
            w.error.clone().unwrap_or_default(),
            String::new(),
        ),
        Content::Search(s) => (
            format!("{} search of {}", s.mode, s.index),
            [
                Some(s.engine.clone()),
                Some(format!("a term of {} characters", s.term_length)),
                s.hits.map(|hits| format!("{hits} hits")),
                Some(duration.clone()),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · "),
            s.error.clone().unwrap_or_default(),
            String::new(),
        ),
        Content::SignIn(s) => (
            format!("{} sign-in {}", s.method, s.outcome),
            [
                s.provider.clone(),
                s.reason.clone(),
                entry.user_id.as_ref().map(|user| format!("user {user}")),
                Some(duration.clone()),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · "),
            String::new(),
            String::new(),
        ),
        Content::Ui(u) => (
            format!("{} {}", u.kind, u.name),
            format!("{} · {} operation(s) · {duration}", u.status, u.ops),
            String::new(),
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
fn timeline(head: &Entry, entries: &[Entry]) -> (Vec<Bar>, f64) {
    if entries.len() < 2 && !head.kind().is_batch() {
        return (Vec::new(), 0.0);
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
    let bars = ordered
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
        .collect();
    (bars, total)
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
    fn long_runs_get_somewhere_to_break_and_text_with_room_is_left_alone() {
        assert_eq!(breakable("GET /api/v1/deals"), "GET /api/v1/deals");
        let long = format!("/{}", "a".repeat(100));
        let broken = breakable(&long);
        assert!(broken.contains('\u{200b}'), "{broken}");
        assert_eq!(
            broken.replace('\u{200b}', ""),
            long,
            "only a break is added"
        );
        let path = "/api/v1/companies/0199c1d2-6f7e-7a31-b5d4-2f0c8e9a1b23/contacts/0199c1d2-6f7e";
        assert!(breakable(path).contains('\u{200b}'));
        assert!(!breakable("short/word").contains('\u{200b}'));
        let long = format!("GET /{}", "a".repeat(160));
        assert!(
            tight(&long)
                .split(|c: char| c.is_whitespace() || c == '\u{200b}')
                .all(|run| run.chars().count() <= 24),
            "a heading breaks within a phone's width"
        );
        // An identifier is copied whole.
        for id in [
            "01a19b71a3e975e68382cea88c0d064b",
            "0199c1d2-6f7e-7a31-b5d4-2f0c8e9a1b23",
        ] {
            assert_eq!(breakable(id), id);
        }
    }

    #[test]
    fn tokens_and_costs_read_well() {
        assert_eq!(
            tokens_text(Some(1200), Some(340)).as_deref(),
            Some("1200 in, 340 out tokens")
        );
        assert_eq!(
            tokens_text(Some(10), None).as_deref(),
            Some("10 in, ? out tokens")
        );
        assert_eq!(tokens_text(None, None), None);
        assert_eq!(cost_text(5_400), "$0.0054");
        assert_eq!(cost_text(2_000_000), "$2");
        assert_eq!(cost_text(1_500_000), "$1.5");
    }

    #[test]
    fn list_links_encode_the_filter() {
        assert_eq!(list_href("/_logbook", "", None), "/_logbook");
        assert_eq!(
            list_href("/_logbook", "user:a b", Some("query")),
            "/_logbook?q=user%3Aa+b&type=query"
        );
    }
}
