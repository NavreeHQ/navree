//! The routes: what each page loads, what each action checks and changes, and what it
//! answers, with JavaScript and without.
//!
//! | Route | Answers |
//! |---|---|
//! | `GET /inbox` | the panes with no conversation open; the view and filters in the URL |
//! | `GET /inbox/{id}` | the panes with a conversation open (`?pane=customer` or `conversations` for phones); it changes nothing |
//! | `GET /inbox/rows`, `/inbox/{id}/messages` | the list's and the thread's pages, and what changed since a revision |
//! | `GET /inbox/fragments/{summary,views}`, `/inbox/{id}/fragments/{head,customer}` | what the live parts fetch |
//! | `GET /broadcasting/stream` | the live hub's stream (`RealtimeModule`): `conversations`, and with a conversation open `conversation.<id>`, `customer.<id>` and `clock` |
//! | `POST /inbox/{id}/reply` | a reply or a note, or a saved reply put in the composer |
//! | `POST /inbox/{id}/assign`, `/assign/me`; `GET /inbox/{id}/assign` | assigning, and the page that does it with no script |
//! | `POST /inbox/{id}/status/{status}`, `/priority/{priority}`, `/snooze`, `/snooze/{preset}` | the rest of what an agent does |
//! | `GET /inbox/agents/options` | the team, searched, for the assign combobox |
//! | `POST /inbox/{id}/seen` | the agent saw what is new: marks the conversation read |
//! | `POST /inbox/panes` | the sizes of the agent's panes |
//! | `POST /inbox/keys` | single-key shortcuts on or off, for the agent |
//! | `GET /inbox/{id}/files/{file}` | a file sent with a message |
//! | `POST /as/{agent}` | work as someone else on the team |
//!
//! **There is no sign-in.** A visitor works as one of the team, named by a cookie the
//! server signs, and can switch from the account menu. The signature only stops a
//! visitor naming someone they didn't choose here; it is no sign-in. A real app puts
//! the inbox behind navree-auth, and checks on every route that the person may see and
//! change the conversation it names.

use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::{DateTime, NaiveDate, Utc};
use http::header::{CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_TYPE, COOKIE, REFERER, SET_COOKIE};
use http::request::Parts;
use http::{HeaderMap, HeaderValue, Method, StatusCode};
use navree::db::{Db, Executor as DbExecutor, Uow};
use navree::http::extract::{Json, Path, Query};
use navree::http::response::{IntoResponse, Response};
use navree::http::{AppError, Controller, RouteDef, UrlSigner};
use navree::orm::{Executor, Filter, Model as _};
use navree::prelude::*;
use navree::ui::form::{FieldErrors, Rejected};
use navree::ui::*;
use serde::Deserialize;

use crate::Settings;
use crate::data::{
    Agent, Attachment, Author, Conversation, Customer, CustomerEvent, DEFAULT_AGENT, EventKind,
    Layout, Message, Priority, Status, day_of, inbox_of, preview_of, saved,
};
use crate::live::{Change, Happening, Hub, current_rev, next_rev, now, plus_one};
use crate::queries::{self, Since};
use crate::views::customer::CUSTOMER;
use crate::views::inbox::{
    Inbox, KeysForm, OpenThread, SPLIT, assign_page, customer_region, head_region, inbox_split,
    layout, page_actions, page_title, palette, shortcuts, stream_url,
};
use crate::views::list::{LIST, ListPage, conversation_item, summary_region};
use crate::views::thread::{
    ActForm, AssignForm, COMPOSER, COMPOSER_BODY, HEAD, MESSAGES, Open, ReplyForm, SNOOZE_DAYS,
    SnoozeForm, composer, message_item,
};
use crate::views::web_only;
use crate::views::{Filters, View, ViewCounts, shell, views_src};

/// The cookie that says which agent a visitor works as.
const AGENT_COOKIE: &str = "inbox_agent";
/// The longest message the composer takes, in characters.
const LONGEST: usize = 5000;
/// The team, searched by name or team, for the assign combobox.
const TEAM: OptionSource = OptionSource::new("id", "name").search_in(&["name", "team"]);

/// The pages, with the hub that knows when the desk moves.
#[injectable]
pub struct Pages {
    hub: Arc<Hub>,
    settings: Arc<Settings>,
    db: Arc<Db>,
    signer: Arc<UrlSigner>,
}

type Result<T> = std::result::Result<T, AppError>;

/// Today in the desk's zone.
fn today() -> NaiveDate {
    day_of(now()).unwrap_or_default()
}

#[derive(Deserialize, Default)]
struct ListQuery {
    view: Option<String>,
    q: Option<String>,
    channel: Option<String>,
    priority: Option<String>,
    pane: Option<String>,
    /// With no script: the list from after this cursor, or up to it.
    rows_after: Option<String>,
    rows_before: Option<String>,
    /// With no script: the messages before this cursor, or after it.
    older: Option<String>,
    newer: Option<String>,
}

impl ListQuery {
    fn filters(&self) -> Filters {
        Filters::read(
            self.view.as_deref(),
            self.q.as_deref(),
            self.channel.as_deref(),
            self.priority.as_deref(),
        )
    }

    /// Which part of the list the page draws.
    fn list_ask(&self) -> LazyAsk {
        match (&self.rows_after, &self.rows_before) {
            (Some(cursor), _) => LazyAsk::After(cursor.chars().take(64).collect()),
            (None, Some(cursor)) => LazyAsk::Before(cursor.chars().take(64).collect()),
            _ => LazyAsk::First,
        }
    }

    /// Which part of the conversation the page draws.
    fn thread_ask(&self) -> LazyAsk {
        match (&self.older, &self.newer) {
            (Some(cursor), _) => LazyAsk::Before(cursor.chars().take(64).collect()),
            (None, Some(cursor)) => LazyAsk::After(cursor.chars().take(64).collect()),
            _ => LazyAsk::First,
        }
    }
}

#[derive(Deserialize, Default)]
struct StepQuery {
    what: Option<String>,
    on: Option<i64>,
    left: Option<i64>,
}

/// The value of the cookie `name`.
fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value)
}

/// The filters of the page an action was taken on, from its address: a form posts to
/// its action, but the page it answers into keeps its view.
fn filters_of_referer(headers: &HeaderMap) -> Filters {
    let Some(referer) = headers.get(REFERER).and_then(|value| value.to_str().ok()) else {
        return Filters::default();
    };
    let query = referer.split_once('?').map_or("", |(_, query)| query);
    let query: ListQuery = serde_urlencoded_lite(query);
    query.filters()
}

/// The conversation open in the page a request came from, from its address.
fn filters_of_referer_open(headers: &HeaderMap) -> Option<i64> {
    let referer = headers.get(REFERER)?.to_str().ok()?;
    let path = referer.split_once("://").map_or(referer, |(_, rest)| rest);
    let path = path.split_once('/').map_or("", |(_, path)| path);
    let path = path.split(['?', '#']).next().unwrap_or("");
    path.strip_prefix("inbox/")?.parse().ok()
}

/// A query string read into a `ListQuery`, leniently.
fn serde_urlencoded_lite(query: &str) -> ListQuery {
    let mut out = ListQuery::default();
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let value = decode(value);
        match key {
            "view" => out.view = Some(value),
            "q" => out.q = Some(value),
            "channel" => out.channel = Some(value),
            "priority" => out.priority = Some(value),
            _ => {}
        }
    }
    out
}

/// Decodes `+` and `%XX` in a query value.
fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let byte = bytes[at];
        if byte == b'+' {
            out.push(b' ');
        } else if byte == b'%'
            && let Some(hex) = bytes.get(at + 1..at + 3)
            && let Some(value) = std::str::from_utf8(hex)
                .ok()
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(value);
            at += 2;
        } else {
            out.push(byte);
        }
        at += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// An HTML fragment for a region: the region itself, so the runtime finds it by id.
fn fragment(title: &'static str, body: impl IntoNodes) -> navree::ui::View {
    navree::ui::View::new(title).body(body)
}

/// The desk's next revision, inside the request's unit of work (see [`crate::live`]).
async fn revision(uow: &Uow) -> Result<i64> {
    next_rev(uow)
        .await
        .map_err(|error| AppError::internal(error.to_string()))
}

/// Where an action taken on a page goes back to: that page, when it is one of the
/// inbox's, else the inbox.
fn back_to(headers: &HeaderMap) -> String {
    let Some(referer) = headers.get(REFERER).and_then(|value| value.to_str().ok()) else {
        return "/inbox".to_owned();
    };
    let rest = referer.split_once("://").map_or(referer, |(_, rest)| rest);
    let path = rest.find('/').map_or("/", |at| &rest[at..]);
    let path = path.split('#').next().unwrap_or("/");
    if path.starts_with("/inbox") {
        path.to_owned()
    } else {
        "/inbox".to_owned()
    }
}

/// A response the browser keeps no copy of.
fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// The cookie's value that says a visitor works as `agent`, signed with the app's key and
/// good for a year: `7.1825329600.<signature>`. The signature is the app's own
/// [`UrlSigner`]'s, over `/as/7` and the expiry.
pub fn agent_cookie(signer: &UrlSigner, agent: i64, now: DateTime<Utc>) -> String {
    let signed = signer.sign(&format!("/as/{agent}"), now + chrono::Duration::days(365));
    let query = signed.split_once('?').map_or("", |(_, query)| query);
    let mut expires = "";
    let mut signature = "";
    for pair in query.split('&') {
        match pair.split_once('=') {
            Some(("expires", value)) => expires = value,
            Some(("signature", value)) => signature = value,
            _ => {}
        }
    }
    format!("{agent}.{expires}.{signature}")
}

/// The agent a signed cookie's value names, if the app signed it and it hasn't expired at
/// `now`: anything else names nobody.
pub fn agent_of_cookie(signer: &UrlSigner, value: &str, now: DateTime<Utc>) -> Option<i64> {
    let mut parts = value.splitn(3, '.');
    let (agent, expires, signature) = (parts.next()?, parts.next()?, parts.next()?);
    let digits = |text: &str| {
        !text.is_empty() && text.len() <= 18 && text.bytes().all(|b| b.is_ascii_digit())
    };
    if !digits(agent)
        || !digits(expires)
        || signature.is_empty()
        || signature.len() > 128
        || !signature.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return None;
    }
    let agent: i64 = agent.parse().ok()?;
    let uri: http::Uri = format!("/as/{agent}?expires={expires}&signature={signature}")
        .parse()
        .ok()?;
    signer.verify(&uri, now).then_some(agent)
}

/// The agent a visitor works as: the one their signed cookie names, if they are on the
/// team, or the default. An unsigned or forged cookie names nobody.
async fn who(
    uow: &(impl Executor + ?Sized),
    signer: &UrlSigner,
    headers: &HeaderMap,
) -> Result<Agent> {
    let wanted = cookie(headers, AGENT_COOKIE)
        .and_then(|value| agent_of_cookie(signer, value, Utc::now()))
        .unwrap_or(DEFAULT_AGENT);
    if let Some(agent) = Agent::query()
        .where_(Agent::ACTIVE.eq(true))
        .find(uow, wanted)
        .await?
    {
        return Ok(agent.into_inner());
    }
    Ok(Agent::query()
        .find(uow, DEFAULT_AGENT)
        .await?
        .map(|record| record.into_inner())
        .unwrap_or_default())
}

/// The shell of a page of an error (a missing page, a refused request): the one every
/// page draws, for the agent the visitor works as, so a native client has the app's
/// destinations to draw its chrome from. `None` leaves the page on its own.
pub async fn error_shell(request: navree::http::ErrorContext) -> Option<Shell> {
    let db = request.ctx.get::<Db>().await.ok()?;
    let signer = request.ctx.get::<UrlSigner>().await.ok()?;
    let me = who(&*db, &signer, &request.headers).await.ok()?;
    let team = team(&*db).await.ok()?;
    let counts = counts(&*db, me.id).await.ok()?;
    Some(shell(&me, &team, counts, View::All, None))
}

async fn team(uow: &(impl Executor + ?Sized)) -> Result<Vec<Agent>> {
    Ok(Agent::query()
        .order_by("id")
        .get(uow)
        .await?
        .into_vec()
        .into_iter()
        .map(|record| record.into_inner())
        .collect())
}

/// How many conversations each view holds.
async fn counts(uow: &(impl Executor + ?Sized), me: i64) -> Result<ViewCounts> {
    let open_or_pending = [Status::Open.key(), Status::Pending.key()];
    Ok(ViewCounts {
        all: Conversation::query()
            .where_(Conversation::STATUS.is_in(open_or_pending))
            .count(uow)
            .await?,
        mine: Conversation::query()
            .where_(Conversation::STATUS.is_in(open_or_pending))
            .where_(Conversation::ASSIGNEE_ID.eq(Some(me)))
            .count(uow)
            .await?,
        unassigned: Conversation::query()
            .where_(Conversation::STATUS.eq(Status::Open.key()))
            .where_(Conversation::ASSIGNEE_ID.is_null())
            .count(uow)
            .await?,
        snoozed: Conversation::query()
            .where_(Conversation::STATUS.eq(Status::Snoozed.key()))
            .count(uow)
            .await?,
    })
}

async fn conversation(uow: &(impl Executor + ?Sized), id: i64) -> Result<Option<Conversation>> {
    Ok(Conversation::query()
        .find(uow, id)
        .await?
        .map(|record| record.into_inner()))
}

/// An open conversation: the window of messages `ask` names, and its neighbours in the view.
async fn open_thread(
    uow: &(impl Executor + ?Sized),
    id: i64,
    ask: &LazyAsk,
    filters: &Filters,
    me: i64,
) -> Result<Option<OpenThread>> {
    let Some(conversation) = conversation(uow, id).await? else {
        return Ok(None);
    };
    let Some(customer) = Customer::query()
        .find(uow, conversation.customer_id)
        .await?
        .map(|record| record.into_inner())
    else {
        return Ok(None);
    };
    let assignee = match conversation.assignee_id {
        Some(agent) => Agent::query()
            .find(uow, agent)
            .await?
            .map(|record| (record.id, record.name.clone())),
        None => None,
    };
    let (messages, files, older, newer) = queries::thread_window(uow, id, ask).await?;
    let neighbours = queries::neighbours(uow, filters, me, &conversation).await?;
    Ok(Some(OpenThread {
        conversation,
        customer,
        assignee,
        messages,
        files,
        older,
        newer,
        neighbours,
    }))
}

/// The customer pane of conversation `id`.
async fn customer_of(
    uow: &(impl Executor + ?Sized),
    id: i64,
) -> Result<Option<crate::views::inbox::CustomerView>> {
    let Some(found) = conversation(uow, id).await? else {
        return Ok(None);
    };
    let Some(customer) = Customer::query()
        .find(uow, found.customer_id)
        .await?
        .map(|record| record.into_inner())
    else {
        return Ok(None);
    };
    Ok(Some(queries::customer_view(uow, customer, now()).await?))
}

/// The sizes the agent saved for their panes.
async fn saved_sizes(uow: &(impl Executor + ?Sized), me: i64) -> Result<Option<SplitSizes>> {
    let kept = Layout::query()
        .where_(Layout::AGENT_ID.eq(me))
        .where_(Layout::SPLIT.eq(SPLIT))
        .first(uow)
        .await?;
    Ok(kept.and_then(|record| SplitSizes::from_text(&record.sizes, &layout())))
}

/// What the page needs to draw a conversation's heading.
fn open_of<'a>(thread: &'a OpenThread, filters: &'a Filters, keys: bool) -> Open<'a> {
    Open {
        conversation: &thread.conversation,
        customer: &thread.customer,
        assignee: thread.assignee.clone(),
        filters,
        now: now(),
        today: today(),
        neighbours: thread.neighbours,
        keys,
    }
}

/// The note in the thread that says what happened. Its id is the database's, given
/// where it commits.
fn event_line(conversation: i64, me: &Agent, text: String, at: i64) -> Message {
    Message {
        id: 0,
        conversation_id: conversation,
        kind: Author::Event.key().to_owned(),
        author: me.name.clone(),
        agent_id: Some(me.id),
        body: text,
        at,
    }
}

/// A conversation that isn't there: a page, or a toast for the runtime.
fn missing_reply() -> Response {
    Reply::ops_or_redirect("/inbox")
        .toast(
            ToastTone::Warning,
            "That conversation isn't in the inbox any more.",
        )
        .into_response()
}

/// A form's refusal for a reason found after it decoded.
fn refuse<T: navree::ui::form::Form>(
    value: &T,
    field: &str,
    message: &str,
    parts: &Parts,
) -> Rejected<T> {
    let errors: FieldErrors = BTreeMap::from([(field.to_owned(), vec![message.to_owned()])]);
    Rejected::after_decode(value, errors, parts)
}

struct PageInput<'a> {
    headers: &'a HeaderMap,
    path: String,
    filters: Filters,
    open: Option<i64>,
    pane: Option<String>,
    draft: String,
    kind: String,
    error: Option<String>,
    list: LazyAsk,
    thread: LazyAsk,
}

impl<'a> PageInput<'a> {
    /// A page of conversation `open` (or none), the list and the thread from the start.
    fn new(headers: &'a HeaderMap, path: String, filters: Filters, open: Option<i64>) -> Self {
        Self {
            headers,
            path,
            filters,
            open,
            pane: None,
            draft: String::new(),
            kind: "reply".to_owned(),
            error: None,
            list: LazyAsk::First,
            thread: LazyAsk::First,
        }
    }
}

impl Pages {
    /// The agent the request works as.
    async fn who(&self, uow: &(impl Executor + ?Sized), headers: &HeaderMap) -> Result<Agent> {
        who(uow, &self.signer, headers).await
    }

    /// The inbox page, drawn for `input`.
    async fn page<E>(&self, uow: &E, input: PageInput<'_>) -> Result<Response>
    where
        E: Executor + DbExecutor + ?Sized,
    {
        let me = self.who(uow, input.headers).await?;
        let team = team(uow).await?;
        let counts = counts(uow, me.id).await?;
        let now = now();
        // The revision before the rows: a change that commits between the two is asked
        // for again, never missed.
        let since = Since {
            rev: current_rev(uow)
                .await
                .map_err(|e| AppError::internal(e.to_string()))?,
            at: now,
        }
        .text();
        let (rows, more_after, more_before) =
            queries::list_page(uow, &input.filters, me.id, &input.list).await?;
        let summary = queries::summary(uow, &input.filters, me.id, now).await?;
        let thread = match input.open {
            Some(id) => match open_thread(uow, id, &input.thread, &input.filters, me.id).await? {
                Some(thread) => Some(thread),
                None => return Ok(self.missing(&me, &team, counts, &input)),
            },
            None => None,
        };
        let sizes = saved_sizes(uow, me.id).await?;
        let pane = match (input.pane.as_deref(), &thread) {
            (Some("customer"), Some(_)) => "customer",
            (Some("conversations"), _) | (_, None) => "conversations",
            _ => "thread",
        };
        let page = Inbox {
            filters: &input.filters,
            list: ListPage {
                rows: &rows,
                more_after,
                more_before,
                since,
            },
            summary,
            open: thread.as_ref(),
            customer: None,
            pane,
            now,
            today: today(),
            draft: &input.draft,
            kind: &input.kind,
            error: input.error.as_deref(),
            me: me.id,
            keys: me.single_keys,
        };
        let split = inbox_split(&page, sizes.as_ref());
        let stream = stream_url(page.open);
        let shell = shell(&me, &team, counts, input.filters.view, Some(&stream))
            .for_request(input.path.clone(), input.headers);
        let mut app_page = shell.page(page_title(&page));
        if page.open.is_some() {
            app_page = app_page.crumb(input.filters.view.label(), input.filters.href("/inbox"));
        }
        let view = app_page
            .actions(page_actions(me.single_keys))
            // The palette is for a keyboard: a native client has its own way to search and act.
            .body((
                split,
                web_only(palette(&page)),
                web_only(shortcuts(me.single_keys)),
            ))
            .to_view();
        Ok(no_store(view.into_response()))
    }

    fn missing(
        &self,
        me: &Agent,
        team: &[Agent],
        counts: ViewCounts,
        input: &PageInput<'_>,
    ) -> Response {
        let shell = shell(me, team, counts, input.filters.view, None)
            .for_request(input.path.clone(), input.headers);
        let view = shell
            .page("No such conversation")
            .body(
                Empty::new("That conversation isn't here")
                    .icon(icons::MESSAGE_SQUARE_X)
                    .description(
                        "It may have been merged or removed. The list has everything that is open.",
                    )
                    .actions(
                        Button::outline("Back to the inbox").href(input.filters.href("/inbox")),
                    ),
            )
            .to_view()
            .status(StatusCode::NOT_FOUND);
        view.into_response()
    }

    /// `GET /inbox/{id}`: the conversation, changing nothing. Its messages say when they
    /// were seen (`POST /inbox/{id}/seen`), and with no script a button does.
    async fn show(
        &self,
        headers: HeaderMap,
        path: String,
        id: i64,
        query: ListQuery,
    ) -> Result<Response> {
        let db = &*self.db;
        let mut input = PageInput::new(&headers, path, query.filters(), Some(id));
        input.pane = query.pane.clone();
        input.list = query.list_ask();
        input.thread = query.thread_ask();
        self.page(db, input).await
    }

    /// What an action answers: the heading and the customer drawn again, the list and the
    /// messages asked for what changed, and a toast; with no script, the conversation's page.
    async fn after(
        &self,
        uow: &Uow,
        headers: &HeaderMap,
        id: i64,
        toast: Toast,
        messages: bool,
    ) -> Result<Reply> {
        let filters = filters_of_referer(headers);
        let me = self.who(uow, headers).await?;
        let Some(thread) = open_thread(uow, id, &LazyAsk::First, &filters, me.id).await? else {
            return Ok(Reply::ops_or_redirect("/inbox").toast_with(toast));
        };
        let view = customer_of(uow, id).await?;
        let mut reply = Reply::ops_or_redirect(filters.href(&format!("/inbox/{id}")))
            .morph(
                HEAD,
                head_region(&open_of(&thread, &filters, me.single_keys)),
            )
            .morph(
                CUSTOMER,
                customer_region(id, thread.customer.id, &filters, view.as_ref(), now()),
            )
            .op(Op::Invalidate {
                target: LIST.to_owned(),
            });
        if messages {
            reply = reply.op(Op::Invalidate {
                target: MESSAGES.to_owned(),
            });
        }
        Ok(reply.toast_with(toast))
    }

    /// `POST /inbox/{id}/reply`.
    async fn reply(
        &self,
        uow: Uow,
        headers: HeaderMap,
        parts: Parts,
        id: i64,
        sent: std::result::Result<Submit<ReplyForm>, Rejected<ReplyForm>>,
    ) -> Result<Response> {
        let form = match sent {
            Ok(Submit(form)) => form,
            Err(rejected) => return Ok(rejected.into_response()),
        };
        let Some(found) = conversation(&uow, id).await? else {
            return Ok(missing_reply());
        };
        let me = self.who(&uow, &headers).await?;
        let body = form.body.clone().unwrap_or_default();
        let kind = if form.kind.as_deref() == Some("note") {
            "note"
        } else {
            "reply"
        };
        let filters = filters_of_referer(&headers);
        let from_runtime = parts.headers.contains_key("x-navree-ui");
        // A saved reply goes into the text; nothing is sent.
        let pick = form
            .insert
            .as_deref()
            .filter(|key| !key.is_empty())
            .or(form.saved.as_deref().filter(|key| !key.is_empty()));
        if let Some(key) = pick {
            let Some(chosen) = saved(key) else {
                return Ok(refuse(
                    &form,
                    "body",
                    "That saved reply isn't there any more.",
                    &parts,
                )
                .into_response());
            };
            let mut draft = body.trim_end().to_owned();
            if !draft.is_empty() {
                draft.push_str("\n\n");
            }
            draft.push_str(chosen.body);
            let draft: String = draft.chars().take(LONGEST).collect();
            if from_runtime {
                return Ok(Reply::ops(vec![
                    Op::replace(
                        COMPOSER,
                        &composer(&found, &draft, kind, None, me.single_keys),
                    ),
                    Op::Focus {
                        target: COMPOSER_BODY.to_owned(),
                    },
                ])
                .toast(
                    ToastTone::Info,
                    format!("Added “{}”. Edit it, then send.", chosen.title),
                )
                .into_response());
            }
            let mut input = PageInput::new(&headers, format!("/inbox/{id}"), filters, Some(id));
            input.draft = draft;
            input.kind = kind.to_owned();
            return self.page(&uow, input).await;
        }
        let text = body.trim();
        let problem = if text.is_empty() {
            Some("Write a message first.")
        } else if text.chars().count() > LONGEST {
            Some("A message can be at most 5,000 characters.")
        } else {
            None
        };
        if let Some(problem) = problem {
            if from_runtime {
                return Ok(refuse(&form, "body", problem, &parts).into_response());
            }
            let mut input = PageInput::new(&headers, format!("/inbox/{id}"), filters, Some(id));
            input.draft = body.chars().take(LONGEST).collect();
            input.kind = kind.to_owned();
            input.error = Some(problem.to_owned());
            let mut response = self.page(&uow, input).await?;
            *response.status_mut() = StatusCode::UNPROCESSABLE_ENTITY;
            return Ok(response);
        }
        let at = now();
        let author = if kind == "note" {
            Author::Note
        } else {
            Author::Agent
        };
        Message::query()
            .insert(
                &uow,
                [Message {
                    id: 0,
                    conversation_id: id,
                    kind: author.key().to_owned(),
                    author: me.name.clone(),
                    agent_id: Some(me.id),
                    body: text.to_owned(),
                    at,
                }],
            )
            .await?;
        let rev = revision(&uow).await?;
        let mut sets = vec![
            Conversation::LAST_AT.set(at),
            Conversation::LAST_REV.set(rev),
            Conversation::REV.set(rev),
            Conversation::MESSAGES.set_expr(plus_one("messages")),
        ];
        if author == Author::Agent {
            // An answer stops the clock and leaves the next word to the customer.
            sets.push(Conversation::SLA_DUE.set(None::<i64>));
            sets.push(Conversation::SLA_FROM.set(None::<i64>));
            sets.push(Conversation::UNREAD.set(0_i64));
            sets.push(Conversation::PREVIEW.set(preview_of(text)));
            if found.status == Status::Open.key() {
                sets.push(Conversation::STATUS.set(Status::Pending.key().to_owned()));
                sets.push(Conversation::INBOX.set("open".to_owned()));
            }
            if found.assignee_id.is_none() {
                sets.push(Conversation::ASSIGNEE_ID.set(Some(me.id)));
            }
        }
        Conversation::query()
            .where_(Conversation::ID.eq(id))
            .update(&uow, sets)
            .await?;
        self.hub.tell(Change::of(id, found.customer_id)).await;
        let customer = Customer::query()
            .find(&uow, found.customer_id)
            .await?
            .map_or_else(|| "the customer".to_owned(), |record| record.name.clone());
        let toast = if author == Author::Agent {
            Toast::new(
                ToastTone::Success,
                format!("Your reply is on its way to {customer}."),
            )
            .title("Reply sent")
        } else {
            Toast::new(ToastTone::Success, "Only the team can see it.").title("Note added")
        };
        let reply = self
            .after(&uow, &headers, id, toast, true)
            .await?
            .op(Op::replace(
                COMPOSER,
                &composer(&found, "", kind, None, me.single_keys),
            ));
        Ok(reply.into_response())
    }

    /// Assigns conversation `id` to `agent`, or to nobody.
    async fn assign_to(
        &self,
        uow: &Uow,
        headers: &HeaderMap,
        id: i64,
        agent: Option<Agent>,
    ) -> Result<Response> {
        let Some(found) = conversation(uow, id).await? else {
            return Ok(missing_reply());
        };
        let me = self.who(uow, headers).await?;
        let at = now();
        let line = match &agent {
            Some(agent) if agent.id == me.id => format!("{} took the conversation", me.name),
            Some(agent) => format!("{} assigned it to {}", me.name, agent.name),
            None => format!("{} unassigned it", me.name),
        };
        let rev = revision(uow).await?;
        Conversation::query()
            .where_(Conversation::ID.eq(id))
            .update(
                uow,
                [
                    Conversation::ASSIGNEE_ID.set(agent.as_ref().map(|agent| agent.id)),
                    Conversation::LAST_AT.set(at),
                    Conversation::LAST_REV.set(rev),
                    Conversation::REV.set(rev),
                    Conversation::MESSAGES.set_expr(plus_one("messages")),
                ],
            )
            .await?;
        Message::query()
            .insert(uow, [event_line(id, &me, line, at)])
            .await?;
        self.hub.tell(Change::of(id, found.customer_id)).await;
        let toast = match &agent {
            Some(agent) => Toast::new(ToastTone::Success, format!("{} has it now.", agent.name))
                .title("Assigned"),
            None => Toast::new(ToastTone::Info, "Nobody has it now.").title("Unassigned"),
        };
        let reply = self
            .after(uow, headers, id, toast, true)
            .await?
            .op(Op::Close {
                target: "assign-popover".to_owned(),
            });
        Ok(reply.into_response())
    }

    /// `POST /inbox/{id}/assign`: the agent comes from a search, so the server checks
    /// they are someone the conversation can go to.
    async fn assign(
        &self,
        uow: Uow,
        headers: HeaderMap,
        parts: Parts,
        id: i64,
        sent: std::result::Result<Submit<AssignForm>, Rejected<AssignForm>>,
    ) -> Result<Response> {
        let form = match sent {
            Ok(Submit(form)) => form,
            Err(rejected) => return Ok(rejected.into_response()),
        };
        let picked = form.agent.clone().unwrap_or_default();
        if picked.trim().is_empty() {
            return self.assign_to(&uow, &headers, id, None).await;
        }
        let active = Agent::query().where_(Agent::ACTIVE.eq(true));
        if !TEAM
            .all_pickable(
                active.clone(),
                [picked.as_str()],
                &uow,
                OptionSource::as_int,
            )
            .await?
        {
            return Ok(refuse(
                &form,
                "agent",
                "Choose someone on the team who is in today.",
                &parts,
            )
            .into_response());
        }
        let agent = active
            .find(&uow, picked.parse::<i64>().unwrap_or(0))
            .await?
            .map(|record| record.into_inner());
        self.assign_to(&uow, &headers, id, agent).await
    }

    /// `POST /inbox/{id}/status/{status}`.
    async fn status(&self, uow: Uow, headers: HeaderMap, id: i64, to: &str) -> Result<Response> {
        let Some(status) = Status::parse(to).filter(|status| *status != Status::Snoozed) else {
            return Ok(Reply::ops_or_redirect(format!("/inbox/{id}"))
                .toast(
                    ToastTone::Warning,
                    "That isn't a status a conversation can have.",
                )
                .status(StatusCode::UNPROCESSABLE_ENTITY)
                .into_response());
        };
        let Some(found) = conversation(&uow, id).await? else {
            return Ok(missing_reply());
        };
        let me = self.who(&uow, &headers).await?;
        let at = now();
        let rev = revision(&uow).await?;
        let mut sets = vec![
            Conversation::STATUS.set(status.key().to_owned()),
            Conversation::INBOX.set(inbox_of(status.key()).to_owned()),
            Conversation::LAST_AT.set(at),
            Conversation::LAST_REV.set(rev),
            Conversation::SNOOZED_UNTIL.set(None::<NaiveDate>),
            Conversation::REV.set(rev),
            Conversation::MESSAGES.set_expr(plus_one("messages")),
        ];
        if status != Status::Open {
            sets.push(Conversation::SLA_DUE.set(None::<i64>));
            sets.push(Conversation::SLA_FROM.set(None::<i64>));
        }
        if status == Status::Closed {
            sets.push(Conversation::UNREAD.set(0_i64));
        }
        Conversation::query()
            .where_(Conversation::ID.eq(id))
            .update(&uow, sets)
            .await?;
        let line = match status {
            Status::Closed => format!("{} closed the conversation", me.name),
            Status::Open => format!("{} reopened the conversation", me.name),
            _ => format!("{} marked it pending", me.name),
        };
        Message::query()
            .insert(&uow, [event_line(id, &me, line, at)])
            .await?;
        if status == Status::Closed {
            CustomerEvent::query()
                .insert(
                    &uow,
                    [CustomerEvent {
                        id: 0,
                        customer_id: found.customer_id,
                        kind: EventKind::TicketClosed.key().to_owned(),
                        at,
                        title: format!("Resolved: {}", found.subject),
                        detail: format!("Closed by {}", me.name),
                    }],
                )
                .await?;
        }
        self.hub.tell(Change::of(id, found.customer_id)).await;
        let toast = match status {
            Status::Closed => Toast::new(
                ToastTone::Success,
                format!("“{}” is closed.", found.subject),
            )
            .title("Closed"),
            Status::Open => {
                Toast::new(ToastTone::Info, "It's back in the open list.").title("Reopened")
            }
            _ => Toast::new(ToastTone::Info, "Waiting for the customer.").title("Marked pending"),
        };
        Ok(self
            .after(&uow, &headers, id, toast, true)
            .await?
            .into_response())
    }

    /// `POST /inbox/{id}/priority/{priority}`: a running clock is held to the new target.
    async fn priority(&self, uow: Uow, headers: HeaderMap, id: i64, to: &str) -> Result<Response> {
        let Some(priority) = Priority::parse(to) else {
            return Ok(Reply::ops_or_redirect(format!("/inbox/{id}"))
                .toast(ToastTone::Warning, "That isn't a priority.")
                .status(StatusCode::UNPROCESSABLE_ENTITY)
                .into_response());
        };
        let Some(found) = conversation(&uow, id).await? else {
            return Ok(missing_reply());
        };
        let me = self.who(&uow, &headers).await?;
        let at = now();
        let mut sets = vec![
            Conversation::PRIORITY.set(priority.key().to_owned()),
            Conversation::REV.set(revision(&uow).await?),
            Conversation::MESSAGES.set_expr(plus_one("messages")),
        ];
        if let (Some(_), Some(from)) = (found.sla_due, found.sla_from) {
            sets.push(Conversation::SLA_DUE.set(Some(from + priority.target())));
        }
        Conversation::query()
            .where_(Conversation::ID.eq(id))
            .update(&uow, sets)
            .await?;
        Message::query()
            .insert(
                &uow,
                [event_line(
                    id,
                    &me,
                    format!(
                        "{} set the priority to {}",
                        me.name,
                        priority.label().to_lowercase()
                    ),
                    at,
                )],
            )
            .await?;
        self.hub.tell(Change::of(id, found.customer_id)).await;
        let toast = Toast::new(
            ToastTone::Info,
            format!("Priority is {} now.", priority.label().to_lowercase()),
        );
        Ok(self
            .after(&uow, &headers, id, toast, true)
            .await?
            .into_response())
    }

    /// Snoozes conversation `id` to `day`, which the caller has checked.
    async fn snooze_to(
        &self,
        uow: &Uow,
        headers: &HeaderMap,
        id: i64,
        day: NaiveDate,
    ) -> Result<Response> {
        let Some(found) = conversation(uow, id).await? else {
            return Ok(missing_reply());
        };
        let me = self.who(uow, headers).await?;
        let at = now();
        let rev = revision(uow).await?;
        Conversation::query()
            .where_(Conversation::ID.eq(id))
            .update(
                uow,
                [
                    Conversation::STATUS.set(Status::Snoozed.key().to_owned()),
                    Conversation::INBOX.set("snoozed".to_owned()),
                    Conversation::SNOOZED_UNTIL.set(Some(day)),
                    Conversation::SLA_DUE.set(None::<i64>),
                    Conversation::SLA_FROM.set(None::<i64>),
                    Conversation::LAST_AT.set(at),
                    Conversation::LAST_REV.set(rev),
                    Conversation::REV.set(rev),
                    Conversation::MESSAGES.set_expr(plus_one("messages")),
                ],
            )
            .await?;
        let label = day.format("%A, %-d %B").to_string();
        Message::query()
            .insert(
                uow,
                [event_line(
                    id,
                    &me,
                    format!("{} snoozed it until {label}", me.name),
                    at,
                )],
            )
            .await?;
        self.hub.tell(Change::of(id, found.customer_id)).await;
        let toast =
            Toast::new(ToastTone::Info, format!("It comes back on {label}.")).title("Snoozed");
        let reply = self
            .after(uow, headers, id, toast, true)
            .await?
            .op(Op::Close {
                target: "snooze-popover".to_owned(),
            });
        Ok(reply.into_response())
    }

    /// The day a quick choice means.
    fn preset_day(preset: &str) -> Option<NaiveDate> {
        let today = today();
        match preset {
            "tomorrow" => today.succ_opt(),
            "next-week" => today.checked_add_signed(chrono::Duration::days(7)),
            _ => None,
        }
    }

    /// `POST /inbox/{id}/snooze`: a day from tomorrow to 90 days ahead, held here as the
    /// calendar holds it.
    async fn snooze(
        &self,
        uow: Uow,
        headers: HeaderMap,
        parts: Parts,
        id: i64,
        sent: std::result::Result<Submit<SnoozeForm>, Rejected<SnoozeForm>>,
    ) -> Result<Response> {
        let form = match sent {
            Ok(Submit(form)) => form,
            Err(rejected) => return Ok(rejected.into_response()),
        };
        let today = today();
        let last = today
            .checked_add_signed(chrono::Duration::days(SNOOZE_DAYS))
            .unwrap_or(today);
        let day = match form.preset.as_deref().filter(|preset| !preset.is_empty()) {
            Some(preset) => Self::preset_day(preset),
            None => form.until,
        };
        match day {
            Some(day) if day > today && day <= last => {
                self.snooze_to(&uow, &headers, id, day).await
            }
            _ => Ok(refuse(
                &form,
                "until",
                "Pick a day from tomorrow to 90 days ahead.",
                &parts,
            )
            .into_response()),
        }
    }

    /// `POST /inbox/panes`: the agent's pane sizes, held to the layout's rules.
    async fn panes(&self, uow: Uow, headers: HeaderMap, save: SplitSave) -> Result<Response> {
        let Some(sizes) = save.sizes_for(&layout()) else {
            return Ok(StatusCode::BAD_REQUEST.into_response());
        };
        let me = self.who(&uow, &headers).await?;
        let text = sizes.to_text();
        let kept = Layout::query()
            .where_(Layout::AGENT_ID.eq(me.id))
            .where_(Layout::SPLIT.eq(SPLIT))
            .first(&uow)
            .await?;
        match kept {
            Some(record) => {
                Layout::query()
                    .where_(Layout::ID.eq(record.id))
                    .update(&uow, [Layout::SIZES.set(text)])
                    .await?;
            }
            None => {
                // The database gives the id, where it commits: no two saves share one.
                Layout::query()
                    .insert(
                        &uow,
                        [Layout {
                            id: 0,
                            agent_id: me.id,
                            split: SPLIT.to_owned(),
                            sizes: text,
                        }],
                    )
                    .await?;
            }
        }
        Ok(StatusCode::NO_CONTENT.into_response())
    }

    /// `GET /inbox/{id}/files/{file}`: a file sent with a message. The example keeps no
    /// files, so it answers a short text that says so, under the file's name.
    async fn file(&self, uow: &(impl Executor + ?Sized), id: i64, file: i64) -> Result<Response> {
        let Some(found) = Attachment::query()
            .where_(Attachment::CONVERSATION_ID.eq(id))
            .find(uow, file)
            .await?
            .map(|record| record.into_inner())
        else {
            return Ok(StatusCode::NOT_FOUND.into_response());
        };
        // Only letters, digits, dots and dashes in the name a browser saves as.
        let safe: String = found
            .name
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                    ch
                } else {
                    '-'
                }
            })
            .collect();
        let disposition = HeaderValue::from_str(&format!("attachment; filename=\"{safe}\""))
            .unwrap_or(HeaderValue::from_static("attachment"));
        Ok((
            [
                (CONTENT_TYPE, HeaderValue::from_static("text/plain; charset=utf-8")),
                (CONTENT_DISPOSITION, disposition),
                (CACHE_CONTROL, HeaderValue::from_static("no-store")),
            ],
            format!(
                "{} ({} bytes) was sent with conversation #{id}.\nThis example app keeps the names of files, not the files.\n",
                found.name, found.bytes
            ),
        )
            .into_response())
    }

    /// `POST /inbox/{id}/seen`: the agent saw what is new in the conversation, so it is
    /// read. The runtime posts it when the messages show; with no script, a button does.
    /// It reads first outside a transaction, and writes only when there is something to
    /// mark, so a page view takes no write lock.
    async fn seen(&self, uow: Uow, headers: &HeaderMap, id: i64) -> Result<Response> {
        let Some(found) = conversation(&*self.db, id).await? else {
            return Ok(missing_reply());
        };
        if found.unread > 0
            && let Some(found) = conversation(&uow, id).await?
            && found.unread > 0
        {
            Conversation::query()
                .where_(Conversation::ID.eq(id))
                .update(
                    &uow,
                    [
                        Conversation::UNREAD.set(0_i64),
                        Conversation::REV.set(revision(&uow).await?),
                    ],
                )
                .await?;
            // Only the lists show what is read: the page's own heading, messages and
            // customer stay as they are, with whatever the agent has open in them.
            self.hub.tell(Change::read(id, found.customer_id)).await;
        }
        if headers.contains_key("x-navree-ui") {
            return Ok(StatusCode::NO_CONTENT.into_response());
        }
        Ok(
            Reply::redirect(filters_of_referer(headers).href(&format!("/inbox/{id}")))
                .into_response(),
        )
    }

    /// `POST /inbox/keys`: turns single-key shortcuts on or off for the agent. The page
    /// then draws no hotkeys at all for them.
    async fn keys(&self, uow: Uow, headers: &HeaderMap, form: &KeysForm) -> Result<Response> {
        let me = self.who(&uow, headers).await?;
        let on = form.from.as_deref() == Some("on");
        Agent::query()
            .where_(Agent::ID.eq(me.id))
            .update(&uow, [Agent::SINGLE_KEYS.set(on)])
            .await?;
        let (tone, text) = if on {
            (ToastTone::Success, "Single-key shortcuts are on.")
        } else {
            (
                ToastTone::Info,
                "Single-key shortcuts are off. Ctrl+K still opens the command palette.",
            )
        };
        Ok(Reply::redirect(back_to(headers))
            .toast(tone, text)
            .into_response())
    }

    /// `POST /as/{agent}`: work as someone else on the team.
    async fn work_as(&self, uow: Uow, agent: i64) -> Result<Response> {
        let Some(found) = Agent::query()
            .where_(Agent::ACTIVE.eq(true))
            .find(&uow, agent)
            .await?
            .map(|record| record.into_inner())
        else {
            return Ok(Reply::redirect("/inbox")
                .toast(ToastTone::Warning, "That person isn't on the team today.")
                .into_response());
        };
        let mut response = Reply::redirect("/inbox")
            .toast(
                ToastTone::Info,
                format!("You're working as {}.", found.name),
            )
            .into_response();
        if let Ok(value) = HeaderValue::from_str(&format!(
            "{AGENT_COOKIE}={}; Path=/; Max-Age=31536000; SameSite=Lax; HttpOnly",
            agent_cookie(&self.signer, found.id, Utc::now())
        )) {
            response.headers_mut().append(SET_COOKIE, value);
        }
        Ok(response)
    }
}

/// A route that needs the pages, wired once: `route!(me, GET, "/x", pages, [uow: Uow] => { ... })`.
macro_rules! route {
    ($me:expr, $method:expr, $path:expr, $pages:ident, [$($args:tt)*] => $body:block) => {{
        let me = $me.clone();
        RouteDef::on($method, $path, move |$($args)*| {
            let $pages = me.clone();
            async move $body
        })
        .public()
    }};
}

impl Controller for Pages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let me = self.clone();
        let mut routes = vec![
            RouteDef::on(Method::GET, "!/", || async { Reply::redirect("/inbox") }).public(),
            route!(me, Method::GET, "!/inbox", pages, [headers: HeaderMap, Query(query): Query<ListQuery>] => {
                let uow = &*pages.db;
                let mut input = PageInput::new(&headers, "/inbox".to_owned(), query.filters(), None);
                input.list = query.list_ask();
                pages.page(uow, input).await
            }),
            route!(me, Method::GET, "!/inbox/{id}", pages, [headers: HeaderMap, Path(id): Path<i64>, Query(query): Query<ListQuery>] => {
                pages.show(headers, format!("/inbox/{id}"), id, query).await
            }),
            route!(me, Method::GET, "!/inbox/rows", pages, [headers: HeaderMap, ask: LazyQuery, Query(query): Query<ListQuery>] => {
                let uow = &*pages.db;
                let me = pages.who(uow, &headers).await?;
                let filters = query.filters();
                let now = now();
                let open = filters_of_referer_open(&headers);
                if let LazyAsk::Since(since) = ask.ask() {
                    // A revision is taken where it commits (see `live`), so the newest one
                    // this answer holds is where the next ask starts: nothing can commit
                    // below it later.
                    let since = Since::read(since).unwrap_or(Since { rev: i64::MAX, at: now });
                    let (rows, gone, newest, more) = queries::changes(uow, &filters, me.id, since, ask.window(), now).await?;
                    let items: Vec<LazyItem> = rows.iter().map(|row| conversation_item(row, &filters, open, now, None)).collect();
                    let next = Since { rev: newest, at: now }.text();
                    return Ok::<_, AppError>(LazyPage::changes(items, next, gone).more(more).into_response());
                }
                let (rows, more_after, more_before) = queries::list_page(uow, &filters, me.id, ask.ask()).await?;
                let more = match ask.ask() {
                    LazyAsk::Before(_) => more_before,
                    _ => more_after,
                };
                Ok(LazyPage::new(rows.iter().map(|row| conversation_item(row, &filters, open, now, None)), more).into_response())
            }),
            route!(me, Method::GET, "!/inbox/{id}/messages", pages, [headers: HeaderMap, ask: LazyQuery, Path(id): Path<i64>] => {
                let uow = &*pages.db;
                let now = now();
                let me = pages.who(uow, &headers).await?;
                let (messages, files, older, newer) = queries::thread_window(uow, id, ask.ask()).await?;
                if let LazyAsk::Since(since) = ask.ask() {
                    // An id is the database's, given where it commits (see `live`): the
                    // newest this answer holds is where the next ask starts. A GET changes
                    // nothing; the list says what was seen with a POST.
                    let last = messages.last().map_or_else(|| since.clone(), |m| m.id.to_string());
                    let items: Vec<LazyItem> = messages.iter().map(|m| message_item(m, &files, now, me.id)).collect();
                    return Ok::<_, AppError>(LazyPage::changes(items, last, Vec::new()).more(newer).into_response());
                }
                let more = match ask.ask() {
                    LazyAsk::After(_) => newer,
                    _ => older,
                };
                Ok(LazyPage::new(messages.iter().map(|m| message_item(m, &files, now, me.id)), more).into_response())
            }),
            route!(me, Method::GET, "!/inbox/fragments/summary", pages, [headers: HeaderMap, Query(query): Query<ListQuery>] => {
                let uow = &*pages.db;
                let me = pages.who(uow, &headers).await?;
                let filters = query.filters();
                let summary = queries::summary(uow, &filters, me.id, now()).await?;
                // The stream of the page it is drawn into: another would take a second of
                // the six connections a browser keeps to a host.
                let open = match filters_of_referer_open(&headers) {
                    Some(id) => conversation(uow, id).await?,
                    None => None,
                };
                let stream = crate::live::page_stream(open.map(|found| (found.id, found.customer_id)))
                    .href()
                    .as_str()
                    .to_owned();
                Ok::<_, AppError>(no_store(fragment("Count", summary_region(summary, &filters, &stream)).into_response()))
            }),
            route!(me, Method::GET, "!/inbox/fragments/views", pages, [headers: HeaderMap, Query(query): Query<ListQuery>] => {
                // The sidebar's views with their counts, in a page with nothing else: the
                // group's region takes itself from it.
                let uow = &*pages.db;
                let me = pages.who(uow, &headers).await?;
                let team = team(uow).await?;
                let counts = counts(uow, me.id).await?;
                let view = query.filters().view;
                let open = match filters_of_referer_open(&headers) {
                    Some(id) => conversation(uow, id).await?,
                    None => None,
                };
                let stream = crate::live::page_stream(open.map(|found| (found.id, found.customer_id)))
                    .href()
                    .as_str()
                    .to_owned();
                let page = shell(&me, &team, counts, view, Some(&stream))
                    .for_request(views_src(view), &headers)
                    .page(view.label())
                    .to_view();
                Ok::<_, AppError>(no_store(page.into_response()))
            }),
            route!(me, Method::GET, "!/inbox/{id}/fragments/head", pages, [headers: HeaderMap, Path(id): Path<i64>] => {
                let uow = &*pages.db;
                let filters = filters_of_referer(&headers);
                let me = pages.who(uow, &headers).await?;
                match open_thread(uow, id, &LazyAsk::First, &filters, me.id).await? {
                    Some(thread) => Ok::<_, AppError>(no_store(fragment("Conversation", head_region(&open_of(&thread, &filters, me.single_keys))).into_response())),
                    None => Ok(StatusCode::NOT_FOUND.into_response()),
                }
            }),
            route!(me, Method::GET, "!/inbox/{id}/fragments/customer", pages, [headers: HeaderMap, Path(id): Path<i64>] => {
                let uow = &*pages.db;
                let _ = &pages;
                let filters = filters_of_referer(&headers);
                match customer_of(uow, id).await? {
                    Some(view) => {
                        let customer = view.customer.id;
                        Ok::<_, AppError>(no_store(fragment("Customer", customer_region(id, customer, &filters, Some(&view), now())).into_response()))
                    }
                    None => Ok(StatusCode::NOT_FOUND.into_response()),
                }
            }),
            route!(me, Method::GET, "!/inbox/agents/options", pages, [query: OptionQuery] => {
                let uow = &*pages.db;
                let _ = &pages;
                TEAM.find(
                    Agent::query().where_(Agent::ACTIVE.eq(true)),
                    &query,
                    uow,
                    "/inbox/agents/options",
                    |record| {
                        let agent = record.into_inner();
                        Choice::new(agent.id.to_string(), agent.name).description(agent.team)
                    },
                )
                .await
                .map_err(AppError::from)
            }),
            route!(me, Method::POST, "!/inbox/{id}/reply", pages, [uow: Uow, headers: HeaderMap, parts: Parts, Path(id): Path<i64>, sent: std::result::Result<Submit<ReplyForm>, Rejected<ReplyForm>>] => {
                pages.reply(uow, headers, parts, id, sent).await
            }),
            route!(me, Method::POST, "!/inbox/{id}/assign", pages, [uow: Uow, headers: HeaderMap, parts: Parts, Path(id): Path<i64>, sent: std::result::Result<Submit<AssignForm>, Rejected<AssignForm>>] => {
                pages.assign(uow, headers, parts, id, sent).await
            }),
            route!(me, Method::POST, "!/inbox/{id}/assign/me", pages, [uow: Uow, headers: HeaderMap, Path(id): Path<i64>, sent: std::result::Result<Submit<ActForm>, Rejected<ActForm>>] => {
                if let Err(rejected) = sent {
                    return Ok(rejected.into_response());
                }
                let me = pages.who(&uow, &headers).await?;
                pages.assign_to(&uow, &headers, id, Some(me)).await
            }),
            route!(me, Method::POST, "!/inbox/{id}/seen", pages, [uow: Uow, headers: HeaderMap, Path(id): Path<i64>, sent: std::result::Result<Submit<ActForm>, Rejected<ActForm>>] => {
                if let Err(rejected) = sent {
                    return Ok(rejected.into_response());
                }
                pages.seen(uow, &headers, id).await
            }),
            route!(me, Method::POST, "!/inbox/keys", pages, [uow: Uow, headers: HeaderMap, sent: std::result::Result<Submit<KeysForm>, Rejected<KeysForm>>] => {
                match sent {
                    Ok(Submit(form)) => pages.keys(uow, &headers, &form).await,
                    Err(rejected) => Ok(rejected.into_response()),
                }
            }),
            route!(me, Method::GET, "!/inbox/{id}/assign", pages, [headers: HeaderMap, Path(id): Path<i64>, Query(query): Query<OptionQueryText>] => {
                let uow = &*pages.db;
                let me = pages.who(uow, &headers).await?;
                let Some(found) = conversation(uow, id).await? else {
                    return Ok::<_, AppError>(missing_reply());
                };
                let q: String = query.q.unwrap_or_default().trim().chars().take(100).collect();
                let mut team_query = Agent::query().where_(Agent::ACTIVE.eq(true));
                if !q.is_empty() {
                    team_query = team_query.where_(Filter::any([
                        Agent::NAME.contains_ignore_case(q.clone()),
                        Agent::TEAM.contains_ignore_case(q.clone()),
                    ]));
                }
                let found_team: Vec<Agent> = team_query
                    .order_by("name")
                    .get(uow)
                    .await?
                    .into_vec()
                    .into_iter()
                    .map(|record| record.into_inner())
                    .collect();
                let everyone = team(uow).await?;
                let counts = counts(uow, me.id).await?;
                let page = shell(&me, &everyone, counts, View::All, None)
                    .for_request(format!("/inbox/{id}/assign"), &headers)
                    .page("Assign the conversation")
                    .crumb("Inbox", "/inbox")
                    .crumb(found.subject.clone(), format!("/inbox/{id}"))
                    .body(assign_page(&found, &found_team, &q, &format!("/inbox/{id}")));
                Ok(page.into_response())
            }),
            route!(me, Method::POST, "!/inbox/{id}/status/{status}", pages, [uow: Uow, headers: HeaderMap, Path((id, status)): Path<(i64, String)>, sent: std::result::Result<Submit<ActForm>, Rejected<ActForm>>] => {
                if let Err(rejected) = sent {
                    return Ok(rejected.into_response());
                }
                pages.status(uow, headers, id, &status).await
            }),
            route!(me, Method::POST, "!/inbox/{id}/priority/{priority}", pages, [uow: Uow, headers: HeaderMap, Path((id, priority)): Path<(i64, String)>, sent: std::result::Result<Submit<ActForm>, Rejected<ActForm>>] => {
                if let Err(rejected) = sent {
                    return Ok(rejected.into_response());
                }
                pages.priority(uow, headers, id, &priority).await
            }),
            route!(me, Method::POST, "!/inbox/{id}/snooze", pages, [uow: Uow, headers: HeaderMap, parts: Parts, Path(id): Path<i64>, sent: std::result::Result<Submit<SnoozeForm>, Rejected<SnoozeForm>>] => {
                pages.snooze(uow, headers, parts, id, sent).await
            }),
            route!(me, Method::POST, "!/inbox/{id}/snooze/{preset}", pages, [uow: Uow, headers: HeaderMap, Path((id, preset)): Path<(i64, String)>, sent: std::result::Result<Submit<ActForm>, Rejected<ActForm>>] => {
                if let Err(rejected) = sent {
                    return Ok(rejected.into_response());
                }
                match Pages::preset_day(&preset) {
                    Some(day) => pages.snooze_to(&uow, &headers, id, day).await,
                    None => Ok(Reply::ops_or_redirect(format!("/inbox/{id}"))
                        .toast(ToastTone::Warning, "That isn't a time to snooze to.")
                        .status(StatusCode::UNPROCESSABLE_ENTITY)
                        .into_response()),
                }
            }),
            route!(me, Method::POST, "!/inbox/panes", pages, [uow: Uow, headers: HeaderMap, save: SplitSave] => {
                pages.panes(uow, headers, save).await
            }),
            route!(me, Method::GET, "!/inbox/{id}/files/{file}", pages, [Path((id, file)): Path<(i64, i64)>] => {
                let uow = &*pages.db;
                pages.file(uow, id, file).await
            }),
            route!(me, Method::POST, "!/as/{agent}", pages, [uow: Uow, Path(agent): Path<i64>, sent: std::result::Result<Submit<ActForm>, Rejected<ActForm>>] => {
                if let Err(rejected) = sent {
                    return Ok(rejected.into_response());
                }
                pages.work_as(uow, agent).await
            }),
        ];
        if self.settings.test_hooks {
            routes.extend(test_hooks(&me));
        }
        routes
    }
}

#[derive(Deserialize, Default)]
struct OptionQueryText {
    q: Option<String>,
}

/// What the browser tests drive the app with, when `INBOX_TEST_HOOKS` is set: one
/// happening they choose, the clock, a clock they set, the desk written afresh, and
/// what the server holds, to check an action against.
fn test_hooks(me: &Arc<Pages>) -> Vec<RouteDef> {
    vec![
        route!(me, Method::POST, "!/__test/step", pages, [Query(query): Query<StepQuery>] => {
            let what = query.what.as_deref().and_then(Happening::parse);
            let change = pages
                .hub
                .step(&pages.db, now(), what, query.on)
                .await
                .map_err(|error| {
                    eprintln!("inbox-demo: a step failed: {error}");
                    AppError::internal(error.to_string())
                })?;
            Ok::<_, AppError>(change.map_or_else(String::new, |change| {
                format!("{} {}", change.conversation, change.customer)
            }))
        }),
        route!(me, Method::POST, "!/__test/tick", pages, [] => {
            pages.hub.tell(Change::TICK).await;
            StatusCode::NO_CONTENT
        }),
        route!(me, Method::POST, "!/__test/sla", pages, [uow: Uow, Query(query): Query<StepQuery>] => {
            let id = query.on.unwrap_or(0);
            let left = query.left.unwrap_or(600);
            let at = now();
            let found = conversation(&uow, id).await?;
            Conversation::query()
                .where_(Conversation::ID.eq(id))
                .update(
                    &uow,
                    [
                        Conversation::STATUS.set(Status::Open.key().to_owned()),
                        Conversation::SLA_DUE.set(Some(at + left)),
                        Conversation::SLA_FROM.set(Some(at + left - 3600)),
                        Conversation::REV.set(revision(&uow).await?),
                    ],
                )
                .await?;
            if let Some(found) = found {
                pages.hub.tell(Change::of(id, found.customer_id)).await;
            }
            Ok::<_, AppError>(StatusCode::NO_CONTENT)
        }),
        route!(me, Method::POST, "!/__test/wake", pages, [Query(query): Query<StepQuery>] => {
            // Snoozed conversations come back as they would `left` days from now.
            let at = now() + query.left.unwrap_or(0).clamp(0, 400) * 86_400;
            let _one = pages.hub.writing.lock().await;
            let woken = pages.hub.wake(&pages.db, at).await.map_err(|error| AppError::internal(error.to_string()))?;
            Ok::<_, AppError>(woken.to_string())
        }),
        route!(me, Method::POST, "!/__test/reset", pages, [] => {
            // On its own task, one at a time: a client that gives up can't stop a write
            // half way and leave the database locked.
            let (hub, db, scale) = (pages.hub.clone(), pages.db.clone(), pages.settings.scale);
            let written = tokio::spawn(async move {
                let _one = hub.writing.lock().await;
                hub.seed(&db, now(), scale).await.map_err(|error| error.to_string())
            })
            .await
            .map_err(|error| AppError::internal(error.to_string()))?;
            written.map_err(|error| {
                eprintln!("inbox-demo: reset failed: {error}");
                AppError::internal(error)
            })?;
            pages.hub.tell(Change::TICK).await;
            Ok::<_, AppError>(StatusCode::NO_CONTENT)
        }),
        route!(me, Method::GET, "!/__test/conversation/{id}", pages, [uow: Uow, Path(id): Path<i64>] => {
            let _ = &pages;
            let Some(found) = conversation(&uow, id).await? else {
                return Ok::<_, AppError>(StatusCode::NOT_FOUND.into_response());
            };
            let last = Message::query()
                .where_(Message::CONVERSATION_ID.eq(id))
                .order_by_desc("id")
                .first(&uow)
                .await?
                .map(|record| record.into_inner())
                .unwrap_or_default();
            Ok(Json(serde_json_lite(&found, &last)).into_response())
        }),
        route!(me, Method::GET, "!/__test/explain", pages, [uow: Uow] => {
            let _ = &pages;
            explain(&uow).await
        }),
        route!(me, Method::GET, "!/__test/layout/{agent}", pages, [uow: Uow, Path(agent): Path<i64>] => {
            let _ = &pages;
            let kept = Layout::query()
                .where_(Layout::AGENT_ID.eq(agent))
                .where_(Layout::SPLIT.eq(SPLIT))
                .first(&uow)
                .await?
                .map_or_else(String::new, |record| record.sizes.clone());
            Ok::<_, AppError>(kept)
        }),
    ]
}

/// The plans SQLite makes for the queries the pages run, as the ORM writes them: each
/// query's name, the columns its index must match on (`expect:`, with `|` between
/// indexes that are each good enough, so a plan that walks a whole list and checks each
/// row fails the scale suite), its SQL, and `EXPLAIN QUERY PLAN`'s lines. For the
/// numbers in the report and the scale suite.
///
/// The lists are every view the sidebar offers under each filter the list offers: a
/// channel, a priority, both. "Mine" and "Unassigned" are read by their own small sets.
pub async fn explain(
    uow: &(impl navree::db::Executor + ?Sized),
) -> std::result::Result<String, AppError> {
    use navree::db::Statement;
    use navree::orm::Dialect;

    let at = now() - 30 * 86_400;
    let read = |view: &str, channel: Option<&str>, priority: Option<&str>| {
        Filters::read(Some(view), None, channel, priority)
    };
    let first_page =
        |filters: &Filters| queries::newest_first(queries::in_view(filters, 1)).limit(51);
    let cursor = (at, 0, 50_000);
    let mut conversations = vec![
        (
            "list: the first page of all open".to_owned(),
            "inbox",
            first_page(&Filters::default()),
        ),
        (
            "list: the page after a cursor, closed".to_owned(),
            "inbox",
            queries::newest_first(
                queries::in_view(&Filters::with_view(View::Closed), 1)
                    .where_(queries::after(cursor)),
            )
            .limit(51),
        ),
        (
            "list: the page before a cursor, closed".to_owned(),
            "inbox",
            queries::oldest_first(
                queries::in_view(&Filters::with_view(View::Closed), 1)
                    .where_(queries::before(cursor)),
            )
            .limit(51),
        ),
        (
            "list: the page after a cursor, closed, api and urgent".to_owned(),
            "inbox channel priority",
            queries::newest_first(
                queries::in_view(&read("closed", Some("api"), Some("urgent")), 1)
                    .where_(queries::after(cursor)),
            )
            .limit(51),
        ),
        (
            "list: assigned to me".to_owned(),
            "assignee_id inbox",
            first_page(&Filters::with_view(View::Mine)),
        ),
        (
            "list: assigned to me, chat and urgent".to_owned(),
            "assignee_id inbox | inbox channel priority",
            first_page(&read("mine", Some("chat"), Some("urgent"))),
        ),
        (
            "list: unassigned".to_owned(),
            "status assignee_id",
            first_page(&Filters::with_view(View::Unassigned)),
        ),
        (
            "list: unassigned, api and urgent".to_owned(),
            "status assignee_id | inbox channel priority",
            first_page(&read("unassigned", Some("api"), Some("urgent"))),
        ),
        (
            "search: closed, \"booking page\"".to_owned(),
            "",
            first_page(&Filters::read(
                Some("closed"),
                Some("booking page"),
                None,
                None,
            )),
        ),
        (
            "live: what changed since a revision".to_owned(),
            "",
            Conversation::query()
                .where_(Conversation::REV.gt(1_i64))
                .order_by("rev")
                .limit(301),
        ),
        (
            "count: all open".to_owned(),
            "inbox",
            queries::in_view(&Filters::default(), 1),
        ),
        (
            "count: something new in all open".to_owned(),
            "inbox",
            queries::in_view(&Filters::default(), 1).where_(queries::something_new()),
        ),
        (
            "count: closed, api and urgent".to_owned(),
            "inbox channel priority",
            queries::in_view(&read("closed", Some("api"), Some("urgent")), 1),
        ),
    ];
    // Every list of the sidebar under each filter: the view's index and the filter's.
    for view in ["all", "snoozed", "closed"] {
        for (channel, priority, expect) in [
            (Some("api"), None, "inbox channel"),
            (None, Some("urgent"), "inbox priority"),
            (Some("api"), Some("urgent"), "inbox channel priority"),
        ] {
            let filters = read(view, channel, priority);
            conversations.push((
                format!(
                    "list: {view}, {}",
                    [channel, priority]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                        .join(" and ")
                ),
                expect,
                first_page(&filters),
            ));
        }
    }
    let messages = [
        (
            "thread: the newest thirty",
            "conversation_id",
            Message::query()
                .where_(Message::CONVERSATION_ID.eq(1_008_i64))
                .order_by_desc("id")
                .limit(31),
        ),
        (
            "thread: thirty before a cursor",
            "conversation_id",
            Message::query()
                .where_(Message::CONVERSATION_ID.eq(1_008_i64))
                .where_(Message::ID.lt(500_i64))
                .order_by_desc("id")
                .limit(31),
        ),
        (
            "thread: what is new since a message",
            "conversation_id",
            Message::query()
                .where_(Message::CONVERSATION_ID.eq(1_008_i64))
                .where_(Message::ID.gt(500_i64))
                .order_by("id")
                .limit(201),
        ),
    ];
    let mut statements = Vec::new();
    for (name, expect, builder) in conversations {
        statements.push((
            name,
            expect,
            builder
                .to_sql(Dialect::Sqlite)
                .map_err(|e| AppError::internal(e.to_string()))?,
        ));
    }
    for (name, expect, builder) in messages {
        statements.push((
            name.to_owned(),
            expect,
            builder
                .to_sql(Dialect::Sqlite)
                .map_err(|e| AppError::internal(e.to_string()))?,
        ));
    }
    let mut out = String::new();
    for (name, expect, statement) in statements {
        let plan = uow
            .fetch_all(&Statement::portable(
                format!("EXPLAIN QUERY PLAN {}", statement.sql),
                statement.values.clone(),
            ))
            .await
            .map_err(|e| AppError::internal(e.to_string()))?;
        out.push_str(&format!("## {name}\nexpect: {expect}\n{}\n", statement.sql));
        for row in plan {
            let detail: String = row.get("detail").unwrap_or_default();
            out.push_str(&format!("  {detail}\n"));
        }
        out.push('\n');
    }
    Ok(out)
}

/// What the tests read of a conversation: its own row and its latest message.
#[derive(serde::Serialize)]
struct Snapshot<'a> {
    conversation: &'a Conversation,
    last: &'a Message,
}

fn serde_json_lite<'a>(conversation: &'a Conversation, last: &'a Message) -> Snapshot<'a> {
    Snapshot { conversation, last }
}
