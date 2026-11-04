//! What the pages read, at the desk's real size: a page of the list by keyset (never
//! an offset), the conversations either side of an open one, what changed since a
//! revision, a window of a conversation's messages, and the customer pane. Every query
//! reads by an index; `EXPLAIN QUERY PLAN` of each is in the browser suite's numbers.
//!
//! Each takes any executor: a page reads through the database handle, outside a unit of
//! work, so on SQLite's write-ahead log a reader never waits for a writer, and an action
//! reads through its unit of work, whose `BEGIN IMMEDIATE` makes read-then-write safe.

use navree::http::AppError;
use navree::orm::{Builder, Executor, Filter, Model as _, Value};
use navree::ui::LazyAsk;

use crate::data::{
    Agent, Attachment, Conversation, Customer, CustomerEvent, Message, Status, read_cursor,
    read_message_cursor,
};
use crate::scale::match_query;
use crate::views::inbox::CustomerView;
use crate::views::list::{ListRow, PAGE, Summary};
use crate::views::{Filters, View};

type Result<T> = std::result::Result<T, AppError>;

/// How many messages a page of a conversation holds.
pub const MESSAGES: u64 = 30;
/// The most changes one live answer carries.
const MOST_CHANGES: u64 = 300;

/// The search as a filter on conversations: their ids in the full-text index.
fn searched(filters: &Filters) -> Option<Filter> {
    let query = match_query(&filters.q)?;
    Some(Filter::raw(
        "\"id\" IN (SELECT rowid FROM conversations_fts WHERE conversations_fts MATCH ?)",
        [Value::String(query)],
    ))
}

/// The conversations in `filters`' view, for the agent `me`, each condition written the
/// way its index reads it.
pub fn in_view(filters: &Filters, me: i64) -> Builder<Conversation> {
    let mut query = Conversation::query();
    query = match filters.view {
        View::All => query.where_(Conversation::INBOX.eq("open")),
        View::Mine => query
            .where_(Conversation::ASSIGNEE_ID.eq(Some(me)))
            .where_(Conversation::INBOX.eq("open")),
        View::Unassigned => query
            .where_(Conversation::STATUS.eq(Status::Open.key()))
            .where_(Conversation::ASSIGNEE_ID.is_null()),
        View::Snoozed => query.where_(Conversation::INBOX.eq("snoozed")),
        View::Closed => query.where_(Conversation::INBOX.eq("closed")),
    };
    if !filters.channel.is_empty() {
        query = query.where_(Conversation::CHANNEL.eq(filters.channel.clone()));
    }
    if !filters.priority.is_empty() {
        query = query.where_(Conversation::PRIORITY.eq(filters.priority.clone()));
    }
    if let Some(search) = searched(filters) {
        query = query.where_(search);
    }
    query
}

/// Whether a conversation belongs in the view, but for the search, which the index
/// answers.
fn fits(conversation: &Conversation, filters: &Filters, me: i64) -> bool {
    let view = match filters.view {
        View::All => conversation.inbox == "open",
        View::Mine => conversation.inbox == "open" && conversation.assignee_id == Some(me),
        View::Unassigned => {
            conversation.status == Status::Open.key() && conversation.assignee_id.is_none()
        }
        View::Snoozed => conversation.inbox == "snoozed",
        View::Closed => conversation.inbox == "closed",
    };
    view && (filters.channel.is_empty() || conversation.channel == filters.channel)
        && (filters.priority.is_empty() || conversation.priority == filters.priority)
}

fn inner(records: navree::orm::Collection<Conversation>) -> Vec<Conversation> {
    records
        .into_vec()
        .into_iter()
        .map(|record| record.into_inner())
        .collect()
}

/// After the cursor `(last_at, id)` in the list: older activity.
pub(crate) fn after(at: i64, id: i64) -> Filter {
    Filter::raw(
        "(\"last_at\", \"id\") < (?, ?)",
        [Value::I64(at), Value::I64(id)],
    )
}

/// Before the cursor in the list: newer activity.
pub(crate) fn before(at: i64, id: i64) -> Filter {
    Filter::raw(
        "(\"last_at\", \"id\") > (?, ?)",
        [Value::I64(at), Value::I64(id)],
    )
}

/// A page of the list: the rows, whether more come after them, and whether more come
/// before them. A cursor that doesn't read is the first page.
pub async fn list_page(
    uow: &(impl Executor + ?Sized),
    filters: &Filters,
    me: i64,
    ask: &LazyAsk,
) -> Result<(Vec<ListRow>, bool, bool)> {
    let query = in_view(filters, me);
    let cursor = |text: &str| read_cursor(text);
    let (found, more_after, more_before) = match ask {
        LazyAsk::After(text) if cursor(text).is_some() => {
            let (at, id) = cursor(text).unwrap_or_default();
            let mut found = inner(
                query
                    .where_(after(at, id))
                    .order_by_desc("last_at")
                    .order_by_desc("id")
                    .limit(PAGE + 1)
                    .get(uow)
                    .await?,
            );
            let more = found.len() as u64 > PAGE;
            found.truncate(PAGE as usize);
            (found, more, true)
        }
        LazyAsk::Before(text) if cursor(text).is_some() => {
            let (at, id) = cursor(text).unwrap_or_default();
            let mut found = inner(
                query
                    .where_(before(at, id))
                    .order_by("last_at")
                    .order_by("id")
                    .limit(PAGE + 1)
                    .get(uow)
                    .await?,
            );
            let more = found.len() as u64 > PAGE;
            found.truncate(PAGE as usize);
            found.reverse();
            (found, true, more)
        }
        _ => {
            let mut found = inner(
                query
                    .order_by_desc("last_at")
                    .order_by_desc("id")
                    .limit(PAGE + 1)
                    .get(uow)
                    .await?,
            );
            let more = found.len() as u64 > PAGE;
            found.truncate(PAGE as usize);
            (found, more, false)
        }
    };
    Ok((hydrate(uow, found).await?, more_after, more_before))
}

/// Rows with the names they show: the customers and agents of these conversations only.
pub async fn hydrate(
    uow: &(impl Executor + ?Sized),
    conversations: Vec<Conversation>,
) -> Result<Vec<ListRow>> {
    let mut customer_ids: Vec<i64> = conversations.iter().map(|c| c.customer_id).collect();
    customer_ids.sort_unstable();
    customer_ids.dedup();
    let customers: Vec<Customer> = Customer::query()
        .find_many(uow, customer_ids)
        .await?
        .into_vec()
        .into_iter()
        .map(|record| record.into_inner())
        .collect();
    let mut agent_ids: Vec<i64> = conversations.iter().filter_map(|c| c.assignee_id).collect();
    agent_ids.sort_unstable();
    agent_ids.dedup();
    let agents: Vec<Agent> = Agent::query()
        .find_many(uow, agent_ids)
        .await?
        .into_vec()
        .into_iter()
        .map(|record| record.into_inner())
        .collect();
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

/// Conversations with something new, written as the partial index's own condition so
/// the count reads only them.
pub(crate) fn something_new() -> Filter {
    Filter::raw("\"unread\" > 0", Vec::<Value>::new())
}

/// The count over the list: how many, how many with something new, how many past due.
pub async fn summary(
    uow: &(impl Executor + ?Sized),
    filters: &Filters,
    me: i64,
    now: i64,
) -> Result<Summary> {
    // The closed list only grows: an agent reads it newest first, and counting it all on
    // every page would read every closed row. Filters narrow it enough to count.
    let total = if filters.view == View::Closed && !filters.narrowed() {
        None
    } else {
        Some(in_view(filters, me).count(uow).await?)
    };
    // Closing a conversation reads what was new in it: the closed list has nothing new,
    // and counting it would read every closed row.
    let unread = if filters.view == View::Closed {
        0
    } else {
        in_view(filters, me)
            .where_(something_new())
            .count(uow)
            .await?
    };
    let overdue = if matches!(filters.view, View::Snoozed | View::Closed) {
        0
    } else {
        in_view(filters, me)
            .where_(Conversation::STATUS.eq(Status::Open.key()))
            .where_(Conversation::SLA_DUE.lt(Some(now)))
            .count(uow)
            .await?
    };
    Ok(Summary {
        total,
        unread,
        overdue,
    })
}

/// The conversations either side of `conversation` in the view: the newer one above it
/// and the older one below, for `k` and `j`.
pub async fn neighbours(
    uow: &(impl Executor + ?Sized),
    filters: &Filters,
    me: i64,
    conversation: &Conversation,
) -> Result<(Option<i64>, Option<i64>)> {
    let (at, id) = (conversation.last_at, conversation.id);
    let newer = in_view(filters, me)
        .where_(before(at, id))
        .order_by("last_at")
        .order_by("id")
        .first(uow)
        .await?
        .map(|record| record.id);
    let older = in_view(filters, me)
        .where_(after(at, id))
        .order_by_desc("last_at")
        .order_by_desc("id")
        .first(uow)
        .await?
        .map(|record| record.id);
    Ok((newer, older))
}

/// What changed since revision `since`: the conversations that fit the view now, the
/// keys of those that left it, and the revision the answer is at.
pub async fn changes(
    uow: &(impl Executor + ?Sized),
    filters: &Filters,
    me: i64,
    since: i64,
    window: Option<(&str, &str)>,
) -> Result<(Vec<ListRow>, Vec<String>, i64)> {
    let mut changed = inner(
        Conversation::query()
            .where_(Conversation::REV.gt(since))
            .order_by("rev")
            .limit(MOST_CHANGES)
            .get(uow)
            .await?,
    );
    let newest = changed.iter().map(|c| c.rev).max().unwrap_or(since);
    if let Some(window) = window {
        for clock in clocks_in(uow, filters, me, window).await? {
            if !changed.iter().any(|c| c.id == clock.id) {
                changed.push(clock);
            }
        }
    }
    let searched_ids: Option<Vec<i64>> = match (searched(filters), changed.is_empty()) {
        (Some(search), false) => Some(
            Conversation::query()
                .where_(Conversation::ID.is_in(changed.iter().map(|c| c.id).collect::<Vec<i64>>()))
                .where_(search)
                .get(uow)
                .await?
                .into_vec()
                .into_iter()
                .map(|record| record.id)
                .collect(),
        ),
        _ => None,
    };
    let mut keep = Vec::new();
    let mut gone = Vec::new();
    for conversation in changed {
        let found = searched_ids
            .as_ref()
            .is_none_or(|ids| ids.contains(&conversation.id));
        if found && fits(&conversation, filters, me) {
            keep.push(conversation);
        } else {
            gone.push(conversation.id.to_string());
        }
    }
    Ok((hydrate(uow, keep).await?, gone, newest))
}

/// The conversations in a loaded window of the list whose clock is running: their
/// countdown changes with no write, so a live answer draws them again. At most what the
/// page holds, read by the view's index between the window's two cursors.
pub async fn clocks_in(
    uow: &(impl Executor + ?Sized),
    filters: &Filters,
    me: i64,
    window: (&str, &str),
) -> Result<Vec<Conversation>> {
    let (Some((top_at, top_id)), Some((bottom_at, bottom_id))) =
        (read_cursor(window.0), read_cursor(window.1))
    else {
        return Ok(Vec::new());
    };
    if matches!(filters.view, View::Snoozed | View::Closed) {
        return Ok(Vec::new());
    }
    Ok(inner(
        in_view(filters, me)
            .where_(Filter::raw(
                "(\"last_at\", \"id\") <= (?, ?) AND (\"last_at\", \"id\") >= (?, ?)",
                [
                    Value::I64(top_at),
                    Value::I64(top_id),
                    Value::I64(bottom_at),
                    Value::I64(bottom_id),
                ],
            ))
            .where_(Conversation::STATUS.eq(Status::Open.key()))
            .where_(Conversation::SLA_DUE.is_not_null())
            .order_by_desc("last_at")
            .order_by_desc("id")
            .limit(400)
            .get(uow)
            .await?,
    ))
}

/// A window of a conversation's messages, oldest first, and their files: the newest page,
/// the page before or after a cursor, or everything after a live revision. Then whether
/// older and newer messages lie beyond it.
pub async fn thread_window(
    uow: &(impl Executor + ?Sized),
    id: i64,
    ask: &LazyAsk,
) -> Result<(Vec<Message>, Vec<Attachment>, bool, bool)> {
    let query = Message::query().where_(Message::CONVERSATION_ID.eq(id));
    let rows = |records: navree::orm::Collection<Message>| -> Vec<Message> {
        records
            .into_vec()
            .into_iter()
            .map(|record| record.into_inner())
            .collect()
    };
    let (messages, older, newer) = match ask {
        LazyAsk::Before(text) if read_message_cursor(text).is_some() => {
            let cursor = read_message_cursor(text).unwrap_or_default();
            let mut found = rows(
                query
                    .where_(Message::ID.lt(cursor))
                    .order_by_desc("id")
                    .limit(MESSAGES + 1)
                    .get(uow)
                    .await?,
            );
            let more = found.len() as u64 > MESSAGES;
            found.truncate(MESSAGES as usize);
            found.reverse();
            (found, more, true)
        }
        LazyAsk::After(text) if read_message_cursor(text).is_some() => {
            let cursor = read_message_cursor(text).unwrap_or_default();
            let mut found = rows(
                query
                    .where_(Message::ID.gt(cursor))
                    .order_by("id")
                    .limit(MESSAGES + 1)
                    .get(uow)
                    .await?,
            );
            let more = found.len() as u64 > MESSAGES;
            found.truncate(MESSAGES as usize);
            (found, true, more)
        }
        LazyAsk::Since(text) => {
            let since = text.parse::<i64>().unwrap_or(i64::MAX);
            let found = rows(
                query
                    .where_(Message::ID.gt(since))
                    .order_by("id")
                    .limit(200)
                    .get(uow)
                    .await?,
            );
            (found, false, false)
        }
        _ => {
            let mut found = rows(
                query
                    .order_by_desc("id")
                    .limit(MESSAGES + 1)
                    .get(uow)
                    .await?,
            );
            let more = found.len() as u64 > MESSAGES;
            found.truncate(MESSAGES as usize);
            found.reverse();
            (found, more, false)
        }
    };
    let files = if messages.is_empty() {
        Vec::new()
    } else {
        Attachment::query()
            .where_(
                Attachment::MESSAGE_ID.is_in(messages.iter().map(|m| m.id).collect::<Vec<i64>>()),
            )
            .order_by("id")
            .get(uow)
            .await?
            .into_vec()
            .into_iter()
            .map(|record| record.into_inner())
            .collect()
    };
    Ok((messages, files, older, newer))
}

/// The customer pane: the customer, their latest history, their latest conversations,
/// and how many are open.
pub async fn customer_view(
    uow: &(impl Executor + ?Sized),
    customer: Customer,
    now: i64,
) -> Result<CustomerView> {
    let mut history: Vec<CustomerEvent> = CustomerEvent::query()
        .where_(CustomerEvent::CUSTOMER_ID.eq(customer.id))
        .order_by_desc("at")
        .order_by_desc("id")
        .limit(30)
        .get(uow)
        .await?
        .into_vec()
        .into_iter()
        .map(|record| record.into_inner())
        .collect();
    history.reverse();
    let theirs = inner(
        Conversation::query()
            .where_(Conversation::CUSTOMER_ID.eq(customer.id))
            .order_by_desc("last_at")
            .order_by_desc("id")
            .limit(7)
            .get(uow)
            .await?,
    );
    let open = Conversation::query()
        .where_(Conversation::CUSTOMER_ID.eq(customer.id))
        .where_(Conversation::INBOX.eq("open"))
        .count(uow)
        .await?;
    let overdue = Conversation::query()
        .where_(Conversation::CUSTOMER_ID.eq(customer.id))
        .where_(Conversation::STATUS.eq(Status::Open.key()))
        .where_(Conversation::SLA_DUE.lt(Some(now)))
        .count(uow)
        .await?;
    Ok(CustomerView {
        customer,
        history,
        theirs,
        open,
        overdue,
    })
}
