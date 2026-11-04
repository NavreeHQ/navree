//! The routes: what each page loads, what each action checks and changes, and what it
//! answers, with JavaScript and without.
//!
//! | Route | Answers |
//! |---|---|
//! | `GET /inbox` | the panes with no conversation open; the view and filters in the URL |
//! | `GET /inbox/{id}` | the panes with a conversation open (`?pane=customer` or `conversations` for phones) |
//! | `GET /inbox/fragments/list`, `/inbox/{id}/fragments/{head,messages,customer}` | what the live parts fetch |
//! | `GET /inbox/events` | the server-sent event stream for one page |
//! | `POST /inbox/{id}/reply` | a reply or a note, or a saved reply put in the composer |
//! | `POST /inbox/{id}/assign`, `/assign/me`; `GET /inbox/{id}/assign` | assigning, and the page that does it with no script |
//! | `POST /inbox/{id}/status/{status}`, `/priority/{priority}`, `/snooze`, `/snooze/{preset}` | the rest of what an agent does |
//! | `GET /inbox/agents/options` | the team, searched, for the assign combobox |
//! | `POST /inbox/panes` | the sizes of the agent's panes |
//! | `GET /inbox/{id}/files/{file}` | a file sent with a message |
//! | `POST /as/{agent}` | work as someone else on the team |

use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::NaiveDate;
use http::header::{CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_TYPE, COOKIE, REFERER, SET_COOKIE};
use http::request::Parts;
use http::{HeaderMap, HeaderValue, Method, StatusCode};
use navree::db::{Db, Uow};
use navree::http::extract::{Json, Path, Query};
use navree::http::response::{IntoResponse, Response};
use navree::http::{AppError, Controller, RouteDef};
use navree::orm::{Filter, Model as _};
use navree::prelude::*;
use navree::ui::form::{FieldErrors, Rejected};
use navree::ui::*;
use serde::Deserialize;

use crate::Settings;
use crate::data::{
    Agent, Attachment, Author, Conversation, Customer, CustomerEvent, DEFAULT_AGENT, EventKind,
    Layout, Message, Priority, Status, day_of, preview_of, saved,
};
use crate::live::{Change, Happening, Hub, now};
use crate::views::customer::CUSTOMER;
use crate::views::inbox::{
    Inbox, OpenThread, SPLIT, assign_page, customer_region, head_region, inbox_split, layout,
    messages_region, page_actions, palette, shortcuts,
};
use crate::views::list::{LIST, ListRow, list_region};
use crate::views::thread::{
    AssignForm, ActForm, COMPOSER, COMPOSER_BODY, HEAD, MESSAGES, Open, ReplyForm, SNOOZE_DAYS,
    SnoozeForm, composer,
};
use crate::views::{Filters, View, ViewCounts, shell};

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
    open: Option<String>,
    customer: Option<String>,
    pane: Option<String>,
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

    fn number(text: Option<&str>) -> Option<i64> {
        text.and_then(|text| text.parse().ok()).filter(|n| *n > 0)
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

/// A response the browser keeps no copy of.
fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// The agent a visitor works as: the cookie's, if they are on the team, or the default.
async fn who(uow: &Uow, headers: &HeaderMap) -> Result<Agent> {
    let wanted = cookie(headers, AGENT_COOKIE)
        .and_then(|value| value.parse::<i64>().ok())
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

async fn team(uow: &Uow) -> Result<Vec<Agent>> {
    Ok(Agent::query()
        .order_by("id")
        .get(uow)
        .await?
        .into_vec()
        .into_iter()
        .map(|record| record.into_inner())
        .collect())
}

/// The conversations the list shows, newest first.
async fn rows(uow: &Uow, filters: &Filters, me: i64) -> Result<Vec<ListRow>> {
    let open_or_pending = [Status::Open.key(), Status::Pending.key()];
    let mut query = Conversation::query();
    query = match filters.view {
        View::All => query.where_(Conversation::STATUS.is_in(open_or_pending)),
        View::Mine => query
            .where_(Conversation::STATUS.is_in(open_or_pending))
            .where_(Conversation::ASSIGNEE_ID.eq(Some(me))),
        View::Unassigned => query
            .where_(Conversation::STATUS.eq(Status::Open.key()))
            .where_(Conversation::ASSIGNEE_ID.is_null()),
        View::Snoozed => query.where_(Conversation::STATUS.eq(Status::Snoozed.key())),
        View::Closed => query.where_(Conversation::STATUS.eq(Status::Closed.key())),
    };
    if !filters.channel.is_empty() {
        query = query.where_(Conversation::CHANNEL.eq(filters.channel.clone()));
    }
    if !filters.priority.is_empty() {
        query = query.where_(Conversation::PRIORITY.eq(filters.priority.clone()));
    }
    if !filters.q.is_empty() {
        let found: Vec<i64> = Customer::query()
            .where_(Filter::any([
                Customer::NAME.contains_ignore_case(filters.q.clone()),
                Customer::COMPANY.contains_ignore_case(filters.q.clone()),
            ]))
            .get(uow)
            .await?
            .into_vec()
            .into_iter()
            .map(|record| record.id)
            .collect();
        query = query.where_(Filter::any([
            Conversation::SUBJECT.contains_ignore_case(filters.q.clone()),
            Conversation::CUSTOMER_ID.is_in(found),
        ]));
    }
    let conversations: Vec<Conversation> = query
        .order_by_desc("last_at")
        .order_by_desc("id")
        .limit(200)
        .get(uow)
        .await?
        .into_vec()
        .into_iter()
        .map(|record| record.into_inner())
        .collect();
    let customers: Vec<Customer> = Customer::query()
        .find_many(uow, conversations.iter().map(|c| c.customer_id).collect::<Vec<i64>>())
        .await?
        .into_vec()
        .into_iter()
        .map(|record| record.into_inner())
        .collect();
    let agents = team(uow).await?;
    Ok(conversations
        .into_iter()
        .map(|conversation| {
            let customer = customers.iter().find(|c| c.id == conversation.customer_id);
            ListRow {
                customer: customer.map_or_else(|| "Someone".to_owned(), |c| c.name.clone()),
                company: customer.map_or_else(String::new, |c| c.company.clone()),
                assignee: conversation
                    .assignee_id
                    .and_then(|id| agents.iter().find(|agent| agent.id == id))
                    .map(|agent| agent.name.clone()),
                conversation,
            }
        })
        .collect())
}

/// How many conversations each view holds.
async fn counts(uow: &Uow, me: i64) -> Result<ViewCounts> {
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

async fn conversation(uow: &Uow, id: i64) -> Result<Option<Conversation>> {
    Ok(Conversation::query()
        .find(uow, id)
        .await?
        .map(|record| record.into_inner()))
}

/// Everything an open conversation shows.
async fn open_thread(uow: &Uow, id: i64) -> Result<Option<OpenThread>> {
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
    let messages = Message::query()
        .where_(Message::CONVERSATION_ID.eq(id))
        .order_by("at")
        .order_by("id")
        .get(uow)
        .await?
        .into_vec()
        .into_iter()
        .map(|record| record.into_inner())
        .collect();
    let files = Attachment::query()
        .where_(Attachment::CONVERSATION_ID.eq(id))
        .order_by("id")
        .get(uow)
        .await?
        .into_vec()
        .into_iter()
        .map(|record| record.into_inner())
        .collect();
    let history = CustomerEvent::query()
        .where_(CustomerEvent::CUSTOMER_ID.eq(customer.id))
        .order_by("at")
        .order_by("id")
        .get(uow)
        .await?
        .into_vec()
        .into_iter()
        .map(|record| record.into_inner())
        .collect();
    let theirs = Conversation::query()
        .where_(Conversation::CUSTOMER_ID.eq(customer.id))
        .order_by_desc("last_at")
        .order_by_desc("id")
        .limit(20)
        .get(uow)
        .await?
        .into_vec()
        .into_iter()
        .map(|record| record.into_inner())
        .collect();
    Ok(Some(OpenThread {
        conversation,
        customer,
        assignee,
        messages,
        files,
        history,
        theirs,
    }))
}

/// The sizes the agent saved for their panes.
async fn saved_sizes(uow: &Uow, me: i64) -> Result<Option<SplitSizes>> {
    let kept = Layout::query()
        .where_(Layout::AGENT_ID.eq(me))
        .where_(Layout::SPLIT.eq(SPLIT))
        .first(uow)
        .await?;
    Ok(kept.and_then(|record| SplitSizes::from_text(&record.sizes, &layout())))
}

/// What the page needs to draw a conversation's heading.
fn open_of<'a>(thread: &'a OpenThread, filters: &'a Filters) -> Open<'a> {
    Open {
        conversation: &thread.conversation,
        customer: &thread.customer,
        assignee: thread.assignee.clone(),
        filters,
        now: now(),
        today: today(),
    }
}

/// The note in the thread that says what happened.
fn event_line(hub: &Hub, conversation: i64, me: &Agent, text: String, at: i64) -> Message {
    Message {
        id: hub.message_id(),
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
        .toast(ToastTone::Warning, "That conversation isn't in the inbox any more.")
        .into_response()
}

/// A form's refusal for a reason found after it decoded.
fn refuse<T: navree::ui::form::Form>(value: &T, field: &str, message: &str, parts: &Parts) -> Rejected<T> {
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
}

impl Pages {
    /// The inbox page, drawn for `input`.
    async fn page(&self, uow: &Uow, input: PageInput<'_>) -> Result<Response> {
        let me = who(uow, input.headers).await?;
        let team = team(uow).await?;
        let counts = counts(uow, me.id).await?;
        let rows = rows(uow, &input.filters, me.id).await?;
        let thread = match input.open {
            Some(id) => match open_thread(uow, id).await? {
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
        let now = now();
        let page = Inbox {
            filters: &input.filters,
            rows: &rows,
            open: thread.as_ref(),
            pane,
            now,
            today: today(),
            draft: &input.draft,
            kind: &input.kind,
            error: input.error.as_deref(),
        };
        let split = inbox_split(&page, sizes.as_ref());
        let title = input.filters.view.label().to_owned();
        let shell = shell(&me, &team, counts, input.filters.view).for_request(input.path, input.headers);
        let view = shell
            .page(title)
            .actions(page_actions())
            .body((split, palette(&page), shortcuts()))
            .to_view();
        Ok(no_store(view.into_response()))
    }

    fn missing(&self, me: &Agent, team: &[Agent], counts: ViewCounts, input: &PageInput<'_>) -> Response {
        let shell = shell(me, team, counts, input.filters.view).for_request(input.path.clone(), input.headers);
        let view = shell
            .page("No such conversation")
            .body(
                Empty::new("That conversation isn't here")
                    .icon(icons::MESSAGE_SQUARE_X)
                    .description("It may have been merged or removed. The list has everything that is open.")
                    .actions(Button::outline("Back to the inbox").href(input.filters.href("/inbox"))),
            )
            .to_view()
            .status(StatusCode::NOT_FOUND);
        view.into_response()
    }

    /// `GET /inbox/{id}`: also marks the conversation read, unless the browser is only
    /// fetching ahead.
    async fn show(&self, uow: Uow, headers: HeaderMap, path: String, id: i64, query: ListQuery) -> Result<Response> {
        if !is_prefetch(&headers)
            && let Some(found) = conversation(&uow, id).await?
            && found.unread > 0
        {
            Conversation::query()
                .where_(Conversation::ID.eq(id))
                .update(&uow, [Conversation::UNREAD.set(0_i64)])
                .await?;
            self.hub.tell(Change {
                conversation: id,
                customer: found.customer_id,
            });
        }
        self.page(
            &uow,
            PageInput {
                headers: &headers,
                path,
                filters: query.filters(),
                open: Some(id),
                pane: query.pane.clone(),
                draft: String::new(),
                kind: "reply".to_owned(),
                error: None,
            },
        )
        .await
    }

    /// What an action answers: the heading and the messages drawn again, the list asked
    /// to fetch itself, and a toast; with no script, the conversation's page.
    async fn after(
        &self,
        uow: &Uow,
        headers: &HeaderMap,
        id: i64,
        toast: Toast,
        messages: bool,
    ) -> Result<Reply> {
        let filters = filters_of_referer(headers);
        let Some(thread) = open_thread(uow, id).await? else {
            return Ok(Reply::ops_or_redirect("/inbox").toast_with(toast));
        };
        let mut reply = Reply::ops_or_redirect(filters.href(&format!("/inbox/{id}")))
            .morph(HEAD, head_region(&open_of(&thread, &filters)))
            .morph(CUSTOMER, customer_region(&thread, &filters, now()))
            .op(Op::Invalidate {
                target: LIST.to_owned(),
            });
        if messages {
            reply = reply.morph(MESSAGES, messages_region(&thread, today()));
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
        let me = who(&uow, &headers).await?;
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
                return Ok(refuse(&form, "body", "That saved reply isn't there any more.", &parts).into_response());
            };
            let mut draft = body.trim_end().to_owned();
            if !draft.is_empty() {
                draft.push_str("\n\n");
            }
            draft.push_str(chosen.body);
            let draft: String = draft.chars().take(LONGEST).collect();
            if from_runtime {
                return Ok(Reply::ops(vec![
                    Op::replace(COMPOSER, &composer(&found, &draft, kind, None)),
                    Op::Focus {
                        target: COMPOSER_BODY.to_owned(),
                    },
                ])
                .toast(ToastTone::Info, format!("Added “{}”. Edit it, then send.", chosen.title))
                .into_response());
            }
            return self
                .page(
                    &uow,
                    PageInput {
                        headers: &headers,
                        path: format!("/inbox/{id}"),
                        filters,
                        open: Some(id),
                        pane: None,
                        draft,
                        kind: kind.to_owned(),
                        error: None,
                    },
                )
                .await;
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
            let mut response = self
                .page(
                    &uow,
                    PageInput {
                        headers: &headers,
                        path: format!("/inbox/{id}"),
                        filters,
                        open: Some(id),
                        pane: None,
                        draft: body.chars().take(LONGEST).collect(),
                        kind: kind.to_owned(),
                        error: Some(problem.to_owned()),
                    },
                )
                .await?;
            *response.status_mut() = StatusCode::UNPROCESSABLE_ENTITY;
            return Ok(response);
        }
        let at = now();
        let author = if kind == "note" { Author::Note } else { Author::Agent };
        Message::query()
            .insert(
                &uow,
                [Message {
                    id: self.hub.message_id(),
                    conversation_id: id,
                    kind: author.key().to_owned(),
                    author: me.name.clone(),
                    agent_id: Some(me.id),
                    body: text.to_owned(),
                    at,
                }],
            )
            .await?;
        let mut sets = vec![Conversation::LAST_AT.set(at)];
        if author == Author::Agent {
            // An answer stops the clock and leaves the next word to the customer.
            sets.push(Conversation::SLA_DUE.set(None::<i64>));
            sets.push(Conversation::SLA_FROM.set(None::<i64>));
            sets.push(Conversation::UNREAD.set(0_i64));
            sets.push(Conversation::PREVIEW.set(preview_of(text)));
            if found.status == Status::Open.key() {
                sets.push(Conversation::STATUS.set(Status::Pending.key().to_owned()));
            }
            if found.assignee_id.is_none() {
                sets.push(Conversation::ASSIGNEE_ID.set(Some(me.id)));
            }
        }
        Conversation::query()
            .where_(Conversation::ID.eq(id))
            .update(&uow, sets)
            .await?;
        self.hub.tell(Change {
            conversation: id,
            customer: found.customer_id,
        });
        let customer = Customer::query()
            .find(&uow, found.customer_id)
            .await?
            .map_or_else(|| "the customer".to_owned(), |record| record.name.clone());
        let toast = if author == Author::Agent {
            Toast::new(ToastTone::Success, format!("Your reply is on its way to {customer}."))
                .title("Reply sent")
        } else {
            Toast::new(ToastTone::Success, "Only the team can see it.").title("Note added")
        };
        let reply = self
            .after(&uow, &headers, id, toast, true)
            .await?
            .op(Op::replace(COMPOSER, &composer(&found, "", kind, None)));
        Ok(reply.into_response())
    }

    /// Assigns conversation `id` to `agent`, or to nobody.
    async fn assign_to(&self, uow: &Uow, headers: &HeaderMap, id: i64, agent: Option<Agent>) -> Result<Response> {
        let Some(found) = conversation(uow, id).await? else {
            return Ok(missing_reply());
        };
        let me = who(uow, headers).await?;
        let at = now();
        let line = match &agent {
            Some(agent) if agent.id == me.id => format!("{} took the conversation", me.name),
            Some(agent) => format!("{} assigned it to {}", me.name, agent.name),
            None => format!("{} unassigned it", me.name),
        };
        Conversation::query()
            .where_(Conversation::ID.eq(id))
            .update(
                uow,
                [
                    Conversation::ASSIGNEE_ID.set(agent.as_ref().map(|agent| agent.id)),
                    Conversation::LAST_AT.set(at),
                ],
            )
            .await?;
        Message::query()
            .insert(uow, [event_line(&self.hub, id, &me, line, at)])
            .await?;
        self.hub.tell(Change {
            conversation: id,
            customer: found.customer_id,
        });
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
            .all_pickable(active.clone(), [picked.as_str()], &uow, OptionSource::as_int)
            .await?
        {
            return Ok(refuse(&form, "agent", "Choose someone on the team who is in today.", &parts).into_response());
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
                .toast(ToastTone::Warning, "That isn't a status a conversation can have.")
                .status(StatusCode::UNPROCESSABLE_ENTITY)
                .into_response());
        };
        let Some(found) = conversation(&uow, id).await? else {
            return Ok(missing_reply());
        };
        let me = who(&uow, &headers).await?;
        let at = now();
        let mut sets = vec![
            Conversation::STATUS.set(status.key().to_owned()),
            Conversation::LAST_AT.set(at),
            Conversation::SNOOZED_UNTIL.set(None::<NaiveDate>),
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
            .insert(&uow, [event_line(&self.hub, id, &me, line, at)])
            .await?;
        if status == Status::Closed {
            CustomerEvent::query()
                .insert(
                    &uow,
                    [CustomerEvent {
                        id: self.hub.event_id(),
                        customer_id: found.customer_id,
                        kind: EventKind::TicketClosed.key().to_owned(),
                        at,
                        title: format!("Resolved: {}", found.subject),
                        detail: format!("Closed by {}", me.name),
                    }],
                )
                .await?;
        }
        self.hub.tell(Change {
            conversation: id,
            customer: found.customer_id,
        });
        let toast = match status {
            Status::Closed => Toast::new(ToastTone::Success, format!("“{}” is closed.", found.subject)).title("Closed"),
            Status::Open => Toast::new(ToastTone::Info, "It's back in the open list.").title("Reopened"),
            _ => Toast::new(ToastTone::Info, "Waiting for the customer.").title("Marked pending"),
        };
        Ok(self.after(&uow, &headers, id, toast, true).await?.into_response())
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
        let me = who(&uow, &headers).await?;
        let at = now();
        let mut sets = vec![Conversation::PRIORITY.set(priority.key().to_owned())];
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
                    &self.hub,
                    id,
                    &me,
                    format!("{} set the priority to {}", me.name, priority.label().to_lowercase()),
                    at,
                )],
            )
            .await?;
        self.hub.tell(Change {
            conversation: id,
            customer: found.customer_id,
        });
        let toast = Toast::new(ToastTone::Info, format!("Priority is {} now.", priority.label().to_lowercase()));
        Ok(self.after(&uow, &headers, id, toast, true).await?.into_response())
    }

    /// Snoozes conversation `id` to `day`, which the caller has checked.
    async fn snooze_to(&self, uow: &Uow, headers: &HeaderMap, id: i64, day: NaiveDate) -> Result<Response> {
        let Some(found) = conversation(uow, id).await? else {
            return Ok(missing_reply());
        };
        let me = who(uow, headers).await?;
        let at = now();
        Conversation::query()
            .where_(Conversation::ID.eq(id))
            .update(
                uow,
                [
                    Conversation::STATUS.set(Status::Snoozed.key().to_owned()),
                    Conversation::SNOOZED_UNTIL.set(Some(day)),
                    Conversation::SLA_DUE.set(None::<i64>),
                    Conversation::SLA_FROM.set(None::<i64>),
                    Conversation::LAST_AT.set(at),
                ],
            )
            .await?;
        let label = day.format("%A, %-d %B").to_string();
        Message::query()
            .insert(
                uow,
                [event_line(&self.hub, id, &me, format!("{} snoozed it until {label}", me.name), at)],
            )
            .await?;
        self.hub.tell(Change {
            conversation: id,
            customer: found.customer_id,
        });
        let toast = Toast::new(ToastTone::Info, format!("It comes back on {label}.")).title("Snoozed");
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
            Some(day) if day > today && day <= last => self.snooze_to(&uow, &headers, id, day).await,
            _ => Ok(refuse(&form, "until", "Pick a day from tomorrow to 90 days ahead.", &parts).into_response()),
        }
    }

    /// `POST /inbox/panes`: the agent's pane sizes, held to the layout's rules.
    async fn panes(&self, uow: Uow, headers: HeaderMap, save: SplitSave) -> Result<Response> {
        let Some(sizes) = save.sizes_for(&layout()) else {
            return Ok(StatusCode::UNPROCESSABLE_ENTITY.into_response());
        };
        let me = who(&uow, &headers).await?;
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
                let id = Layout::query().count(&uow).await? as i64 + me.id * 1000;
                Layout::query()
                    .insert(
                        &uow,
                        [Layout {
                            id,
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
    async fn file(&self, uow: Uow, id: i64, file: i64) -> Result<Response> {
        let Some(found) = Attachment::query()
            .where_(Attachment::CONVERSATION_ID.eq(id))
            .find(&uow, file)
            .await?
            .map(|record| record.into_inner())
        else {
            return Ok(StatusCode::NOT_FOUND.into_response());
        };
        // Only letters, digits, dots and dashes in the name a browser saves as.
        let safe: String = found
            .name
            .chars()
            .map(|ch| if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') { ch } else { '-' })
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
            .toast(ToastTone::Info, format!("You're working as {}.", found.name))
            .into_response();
        if let Ok(value) = HeaderValue::from_str(&format!(
            "{AGENT_COOKIE}={}; Path=/; Max-Age=31536000; SameSite=Lax; HttpOnly",
            found.id
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
            route!(me, Method::GET, "!/inbox", pages, [uow: Uow, headers: HeaderMap, Query(query): Query<ListQuery>] => {
                pages
                    .page(
                        &uow,
                        PageInput {
                            headers: &headers,
                            path: "/inbox".to_owned(),
                            filters: query.filters(),
                            open: None,
                            pane: None,
                            draft: String::new(),
                            kind: "reply".to_owned(),
                            error: None,
                        },
                    )
                    .await
            }),
            route!(me, Method::GET, "!/inbox/{id}", pages, [uow: Uow, headers: HeaderMap, Path(id): Path<i64>, Query(query): Query<ListQuery>] => {
                pages.show(uow, headers, format!("/inbox/{id}"), id, query).await
            }),
            route!(me, Method::GET, "!/inbox/fragments/list", pages, [uow: Uow, headers: HeaderMap, Query(query): Query<ListQuery>] => {
                let me = who(&uow, &headers).await?;
                let filters = query.filters();
                let open = ListQuery::number(query.open.as_deref());
                let rows = rows(&uow, &filters, me.id).await?;
                let stream = match open {
                    Some(id) => match conversation(&uow, id).await? {
                        Some(found) => format!("/inbox/events?open={id}&customer={}", found.customer_id),
                        None => "/inbox/events".to_owned(),
                    },
                    None => "/inbox/events".to_owned(),
                };
                let _ = &pages;
                Ok::<_, AppError>(no_store(fragment("Conversations", list_region(&rows, &filters, open, now(), &stream)).into_response()))
            }),
            route!(me, Method::GET, "!/inbox/{id}/fragments/head", pages, [uow: Uow, headers: HeaderMap, Path(id): Path<i64>] => {
                let _ = &pages;
                let filters = filters_of_referer(&headers);
                match open_thread(&uow, id).await? {
                    Some(thread) => Ok::<_, AppError>(no_store(fragment("Conversation", head_region(&open_of(&thread, &filters))).into_response())),
                    None => Ok(StatusCode::NOT_FOUND.into_response()),
                }
            }),
            route!(me, Method::GET, "!/inbox/{id}/fragments/messages", pages, [uow: Uow, Path(id): Path<i64>] => {
                let _ = &pages;
                match open_thread(&uow, id).await? {
                    Some(thread) => Ok::<_, AppError>(no_store(fragment("Messages", messages_region(&thread, today())).into_response())),
                    None => Ok(StatusCode::NOT_FOUND.into_response()),
                }
            }),
            route!(me, Method::GET, "!/inbox/{id}/fragments/customer", pages, [uow: Uow, headers: HeaderMap, Path(id): Path<i64>] => {
                let _ = &pages;
                let filters = filters_of_referer(&headers);
                match open_thread(&uow, id).await? {
                    Some(thread) => Ok::<_, AppError>(no_store(fragment("Customer", customer_region(&thread, &filters, now())).into_response())),
                    None => Ok(StatusCode::NOT_FOUND.into_response()),
                }
            }),
            route!(me, Method::GET, "!/inbox/events", pages, [headers: HeaderMap, Query(query): Query<ListQuery>] => {
                let open = ListQuery::number(query.open.as_deref()).unwrap_or(0);
                let customer = ListQuery::number(query.customer.as_deref()).unwrap_or(0);
                pages.hub.stream(&headers, open, customer)
            }),
            route!(me, Method::GET, "!/inbox/agents/options", pages, [uow: Uow, query: OptionQuery] => {
                let _ = &pages;
                TEAM.find(
                    Agent::query().where_(Agent::ACTIVE.eq(true)),
                    &query,
                    &uow,
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
                let me = who(&uow, &headers).await?;
                pages.assign_to(&uow, &headers, id, Some(me)).await
            }),
            route!(me, Method::GET, "!/inbox/{id}/assign", pages, [uow: Uow, headers: HeaderMap, Path(id): Path<i64>, Query(query): Query<OptionQueryText>] => {
                let me = who(&uow, &headers).await?;
                let Some(found) = conversation(&uow, id).await? else {
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
                    .get(&uow)
                    .await?
                    .into_vec()
                    .into_iter()
                    .map(|record| record.into_inner())
                    .collect();
                let everyone = team(&uow).await?;
                let counts = counts(&uow, me.id).await?;
                let _ = &pages;
                let page = shell(&me, &everyone, counts, View::All)
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
            route!(me, Method::GET, "!/inbox/{id}/files/{file}", pages, [uow: Uow, Path((id, file)): Path<(i64, i64)>] => {
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
                .map_err(|error| AppError::internal(error.to_string()))?;
            Ok::<_, AppError>(change.map_or_else(String::new, |change| {
                format!("{} {}", change.conversation, change.customer)
            }))
        }),
        route!(me, Method::POST, "!/__test/tick", pages, [] => {
            pages.hub.tell(Change::TICK);
            StatusCode::NO_CONTENT
        }),
        route!(me, Method::POST, "!/__test/sla", pages, [uow: Uow, Query(query): Query<StepQuery>] => {
            let id = query.on.unwrap_or(0);
            let left = query.left.unwrap_or(600);
            let at = now();
            Conversation::query()
                .where_(Conversation::ID.eq(id))
                .update(
                    &uow,
                    [
                        Conversation::STATUS.set(Status::Open.key().to_owned()),
                        Conversation::SLA_DUE.set(Some(at + left)),
                        Conversation::SLA_FROM.set(Some(at + left - 3600)),
                    ],
                )
                .await?;
            pages.hub.tell(Change::TICK);
            Ok::<_, AppError>(StatusCode::NO_CONTENT)
        }),
        route!(me, Method::POST, "!/__test/reset", pages, [] => {
            pages.hub.seed(&pages.db, now()).await.map_err(|error| {
                eprintln!("inbox-demo: reset failed: {error}");
                AppError::internal(error.to_string())
            })?;
            pages.hub.tell(Change::TICK);
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

/// What the tests read of a conversation: its own row and its latest message.
#[derive(serde::Serialize)]
struct Snapshot<'a> {
    conversation: &'a Conversation,
    last: &'a Message,
}

fn serde_json_lite<'a>(conversation: &'a Conversation, last: &'a Message) -> Snapshot<'a> {
    Snapshot { conversation, last }
}
