//! Reading the board and writing its moves. Every read is a page by an index:
//! a column's cards by `(list, rank, id)`, a card's comments and history by
//! `(card_id, at, id)`. A move is checked and written in one transaction, which
//! SQLite starts with `BEGIN IMMEDIATE`: two moves at once queue for the write lock,
//! and the second is checked against what the first left.

use std::future::Future;

use chrono::{Days, NaiveDate};
use navree::db::{DbError, ErrorKind, Uow};
use navree::orm::{Model as _, OrmError};
use navree::ui::ListState;
use navree::ui::sortable::{Placement, RANK_GAP, RankedLists, Spot};

use crate::data::{Activity, Card, Check, Column, Comment, Person};
use crate::views::board::{BoardData, CAP, ColumnPage, Due, Filters, PAGE};
use crate::views::card::{COMMENT_PAGE, CardData, HISTORY_PAGE};

/// An ORM error as the transaction's own.
pub fn db(error: OrmError) -> DbError {
    DbError::new(ErrorKind::Other, error.to_string())
}

/// Everyone on the team.
pub async fn people(uow: &Uow) -> Result<Vec<Person>, OrmError> {
    Ok(Person::query()
        .order_by("id")
        .get(uow)
        .await?
        .into_vec()
        .into_iter()
        .map(|record| record.into_inner())
        .collect())
}

/// The person with `id`, or the lead when there is none: who the board is shown as.
pub async fn viewer(uow: &Uow, id: Option<i64>) -> Result<Person, OrmError> {
    let people = people(uow).await?;
    Ok(id
        .and_then(|id| people.iter().find(|person| person.id == id).cloned())
        .or_else(|| people.into_iter().next())
        .unwrap_or_default())
}

/// `rank.id`: where a page of a column ends.
pub fn cursor(card: &Card) -> String {
    format!("{}.{}", card.rank, card.id)
}

/// A cursor read back, or `None` for one that doesn't parse.
pub fn parse_cursor(text: &str) -> Option<(i64, i64)> {
    let (rank, id) = text.rsplit_once('.')?;
    Some((rank.parse().ok()?, id.parse().ok()?))
}

/// The cards of `column` that pass `filters`, after `after` when given, at most
/// `limit` of them, and whether more follow.
pub async fn column_cards(
    uow: &Uow,
    column: Column,
    filters: &Filters,
    today: NaiveDate,
    after: Option<(i64, i64)>,
    limit: usize,
) -> Result<(Vec<Card>, bool), OrmError> {
    let mut query = Card::query().where_(Card::LIST.eq(column.key()));
    if let Some((rank, id)) = after {
        query = query.where_(
            Card::RANK
                .gt(rank)
                .or(Card::RANK.eq(rank).and(Card::ID.gt(id))),
        );
    }
    if let Some(id) = filters.assignee {
        query = query.where_(Card::ASSIGNEE.eq(id));
    }
    if let Some(label) = filters.label {
        query = query.where_(Card::LABELS.like(format!("%{}%", label.key())));
    }
    query = match filters.due {
        Due::Any => query,
        Due::Late => query.where_(Card::DUE.lt(today)),
        Due::Week => query
            .where_(Card::DUE.gte(today))
            .where_(Card::DUE.lt(today.checked_add_days(Days::new(7)).unwrap_or(today))),
        Due::Undated => query.where_(Card::DUE.is_null()),
    };
    let mut cards: Vec<Card> = query
        .order_by("rank")
        .order_by("id")
        .limit(u64::try_from(limit + 1).unwrap_or(u64::MAX))
        .get(uow)
        .await?
        .into_vec()
        .into_iter()
        .map(|record| record.into_inner())
        .collect();
    let more = cards.len() > limit;
    cards.truncate(limit);
    Ok((cards, more))
}

/// How many cards `column` holds: an index-only count.
pub async fn column_total(uow: &Uow, column: Column) -> Result<u64, OrmError> {
    Card::query()
        .where_(Card::LIST.eq(column.key()))
        .count(uow)
        .await
}

/// How many cards each column shows: `show.<column>` from the query, a page at
/// least and the cap at most.
pub fn shown(column: Column, show: impl Fn(&str) -> Option<String>) -> usize {
    show(&format!("show.{}", column.key()))
        .and_then(|n| n.parse::<usize>().ok())
        .unwrap_or(PAGE)
        .clamp(PAGE, CAP)
}

/// What the board draws: each column's first cards, as many as `show` asks.
pub async fn board(
    uow: &Uow,
    filters: &Filters,
    today: NaiveDate,
    show: impl Fn(&str) -> Option<String>,
) -> Result<BoardData, OrmError> {
    let mut columns = Vec::with_capacity(Column::ALL.len());
    for column in Column::ALL {
        let (cards, more) =
            column_cards(uow, column, filters, today, None, shown(column, &show)).await?;
        columns.push(ColumnPage {
            next: more.then(|| cards.last().map(cursor)).flatten(),
            total: column_total(uow, column).await?,
            start: 0,
            column,
            cards,
        });
    }
    Ok(BoardData {
        people: people(uow).await?,
        columns,
    })
}

/// The cards a palette search finds: titles that hold `text`, at most 20.
pub async fn search(uow: &Uow, text: &str) -> Result<Vec<Card>, OrmError> {
    let text: String = text
        .chars()
        .filter(|c| *c != '%' && *c != '_')
        .take(80)
        .collect();
    Ok(Card::query()
        .where_(Card::TITLE.like(format!("%{}%", text.trim())))
        .order_by_desc("id")
        .limit(20)
        .get(uow)
        .await?
        .into_vec()
        .into_iter()
        .map(|record| record.into_inner())
        .collect())
}

/// The board as [`Move::place`](navree::ui::Move::place) reads it: one card, one
/// count or one neighbour at a time, by index.
pub struct Ranks<'a> {
    pub uow: &'a Uow,
    pub viewer: &'a Person,
}

/// The card `key` names, when it is a number.
fn id_of(key: &str) -> i64 {
    key.parse().unwrap_or(0)
}

impl RankedLists for Ranks<'_> {
    type Error = OrmError;

    fn spot(&self, key: &str) -> impl Future<Output = Result<Option<Spot>, OrmError>> + Send {
        let (uow, id) = (self.uow, id_of(key));
        async move {
            Ok(Card::query().find(uow, id).await?.map(|record| {
                let card = record.into_inner();
                Spot {
                    list: card.list,
                    rank: card.rank,
                }
            }))
        }
    }

    fn list(
        &self,
        key: &str,
        item: &str,
    ) -> impl Future<Output = Result<Option<(ListState, u64)>, OrmError>> + Send {
        let (uow, column, item) = (self.uow, Column::parse(key), id_of(item));
        let lead = self.viewer.is_lead();
        async move {
            let Some(column) = column else {
                return Ok(None);
            };
            let count = Card::query()
                .where_(Card::LIST.eq(column.key()))
                .where_(Card::ID.ne(item))
                .count(uow)
                .await?;
            let list = ListState::new(column.key(), column.title(), Vec::<String>::new())
                .limit(column.limit())
                .refuse_if(
                    column == Column::Done && !lead,
                    "Only a lead can move cards to Done.",
                );
            Ok(Some((list, count)))
        }
    }

    fn beside(
        &self,
        list: &str,
        rank: i64,
        lower: bool,
        item: &str,
    ) -> impl Future<Output = Result<Option<i64>, OrmError>> + Send {
        let (uow, list, item) = (self.uow, list.to_owned(), id_of(item));
        async move {
            let query = Card::query()
                .where_(Card::LIST.eq(list))
                .where_(Card::ID.ne(item));
            let found = if lower {
                query
                    .where_(Card::RANK.lt(rank))
                    .order_by_desc("rank")
                    .order_by_desc("id")
            } else {
                query
                    .where_(Card::RANK.gt(rank))
                    .order_by("rank")
                    .order_by("id")
            };
            Ok(found
                .first(uow)
                .await?
                .map(|record| record.into_inner().rank))
        }
    }

    fn nth(
        &self,
        list: &str,
        index: u64,
        item: &str,
    ) -> impl Future<Output = Result<Option<i64>, OrmError>> + Send {
        let (uow, list, item) = (self.uow, list.to_owned(), id_of(item));
        async move {
            Ok(Card::query()
                .where_(Card::LIST.eq(list))
                .where_(Card::ID.ne(item))
                .order_by("rank")
                .order_by("id")
                .offset(index)
                .first(uow)
                .await?
                .map(|record| record.into_inner().rank))
        }
    }

    fn last(
        &self,
        list: &str,
        item: &str,
    ) -> impl Future<Output = Result<Option<i64>, OrmError>> + Send {
        let (uow, list, item) = (self.uow, list.to_owned(), id_of(item));
        async move {
            Ok(Card::query()
                .where_(Card::LIST.eq(list))
                .where_(Card::ID.ne(item))
                .order_by_desc("rank")
                .order_by_desc("id")
                .first(uow)
                .await?
                .map(|record| record.into_inner().rank))
        }
    }
}

/// Gives every card of `list` a fresh rank, [`RANK_GAP`] apart, in its order: for
/// when two neighbours have no room left between them.
pub async fn renew_ranks(uow: &Uow, list: &str) -> Result<(), OrmError> {
    let ids: Vec<i64> = Card::query()
        .where_(Card::LIST.eq(list))
        .order_by("rank")
        .order_by("id")
        .get(uow)
        .await?
        .into_vec()
        .into_iter()
        .map(|record| record.into_inner().id)
        .collect();
    for (at, id) in ids.into_iter().enumerate() {
        Card::query()
            .where_(Card::ID.eq(id))
            .update(
                uow,
                [Card::RANK.set(i64::try_from(at).unwrap_or(0) * RANK_GAP)],
            )
            .await?;
    }
    Ok(())
}

/// Writes a placed move of a card: its column and rank, and a line in its history
/// when it changed column.
pub async fn move_card(
    uow: &Uow,
    placement: &Placement,
    rank: i64,
    by: i64,
    at: i64,
) -> Result<(), OrmError> {
    let id = id_of(&placement.item);
    Card::query()
        .where_(Card::ID.eq(id))
        .update(
            uow,
            [
                Card::LIST.set(placement.to_list.clone()),
                Card::RANK.set(rank),
            ],
        )
        .await?;
    if placement.from_list != placement.to_list {
        let title = |key: &str| Column::parse(key).map_or("", Column::title);
        record(
            uow,
            id,
            by,
            at,
            "moved",
            format!(
                "{} to {}",
                title(&placement.from_list),
                title(&placement.to_list)
            ),
        )
        .await?;
    }
    Ok(())
}

/// A card's checklist, in order.
pub async fn checks(uow: &Uow, card: i64) -> Result<Vec<Check>, OrmError> {
    Ok(Check::query()
        .where_(Check::CARD_ID.eq(card))
        .order_by("position")
        .order_by("id")
        .get(uow)
        .await?
        .into_vec()
        .into_iter()
        .map(|record| record.into_inner())
        .collect())
}

/// Writes a checked move of a checklist line: a card's few lines, renumbered.
pub async fn move_check(uow: &Uow, order: &[String]) -> Result<(), OrmError> {
    for (position, key) in order.iter().enumerate() {
        Check::query()
            .where_(Check::ID.eq(id_of(key)))
            .update(
                uow,
                [Check::POSITION.set(i64::try_from(position).unwrap_or(0))],
            )
            .await?;
    }
    Ok(())
}

/// Ticks a line or not, and keeps its card's count true.
pub async fn tick(uow: &Uow, line: &Check, done: bool) -> Result<(), OrmError> {
    Check::query()
        .where_(Check::ID.eq(line.id))
        .update(uow, [Check::DONE.set(i64::from(done))])
        .await?;
    let ticked = Check::query()
        .where_(Check::CARD_ID.eq(line.card_id))
        .where_(Check::DONE.eq(1))
        .count(uow)
        .await?;
    Card::query()
        .where_(Card::ID.eq(line.card_id))
        .update(
            uow,
            [Card::CHECKS_DONE.set(i64::try_from(ticked).unwrap_or(0))],
        )
        .await?;
    Ok(())
}

/// `at.id`: where a page of comments or history ends.
fn stamp(at: i64, id: i64) -> String {
    format!("{at}.{id}")
}

/// A card's comments, newest first, older than `before` when given: a page, and
/// the cursor for the next when there is one.
pub async fn comments(
    uow: &Uow,
    card: i64,
    before: Option<(i64, i64)>,
) -> Result<(Vec<Comment>, Option<String>), OrmError> {
    let mut query = Comment::query().where_(Comment::CARD_ID.eq(card));
    if let Some((at, id)) = before {
        query = query.where_(
            Comment::AT
                .lt(at)
                .or(Comment::AT.eq(at).and(Comment::ID.lt(id))),
        );
    }
    let mut page: Vec<Comment> = query
        .order_by_desc("at")
        .order_by_desc("id")
        .limit(u64::try_from(COMMENT_PAGE + 1).unwrap_or(u64::MAX))
        .get(uow)
        .await?
        .into_vec()
        .into_iter()
        .map(|record| record.into_inner())
        .collect();
    let more = page.len() > COMMENT_PAGE;
    page.truncate(COMMENT_PAGE);
    let next = more
        .then(|| page.last().map(|c| stamp(c.at, c.id)))
        .flatten();
    Ok((page, next))
}

/// What happened to a card, newest first, older than `before` when given.
pub async fn history(
    uow: &Uow,
    card: i64,
    before: Option<(i64, i64)>,
) -> Result<(Vec<Activity>, Option<String>), OrmError> {
    let mut query = Activity::query().where_(Activity::CARD_ID.eq(card));
    if let Some((at, id)) = before {
        query = query.where_(
            Activity::AT
                .lt(at)
                .or(Activity::AT.eq(at).and(Activity::ID.lt(id))),
        );
    }
    let mut page: Vec<Activity> = query
        .order_by_desc("at")
        .order_by_desc("id")
        .limit(u64::try_from(HISTORY_PAGE + 1).unwrap_or(u64::MAX))
        .get(uow)
        .await?
        .into_vec()
        .into_iter()
        .map(|record| record.into_inner())
        .collect();
    let more = page.len() > HISTORY_PAGE;
    page.truncate(HISTORY_PAGE);
    let next = more
        .then(|| page.last().map(|a| stamp(a.at, a.id)))
        .flatten();
    Ok((page, next))
}

/// What the drawer draws for card `id`, if it is on the board: the card, its
/// checklist, and the newest page of its comments and of its history.
pub async fn card(uow: &Uow, id: i64) -> Result<Option<CardData>, OrmError> {
    let Some(card) = Card::query().find(uow, id).await? else {
        return Ok(None);
    };
    let (comments, older_comments) = comments(uow, id, None).await?;
    let (activity, older_history) = history(uow, id, None).await?;
    Ok(Some(CardData {
        card: card.into_inner(),
        checks: checks(uow, id).await?,
        comments,
        older_comments,
        activity,
        older_history,
        people: people(uow).await?,
    }))
}

/// Adds a line to a card's history.
pub async fn record(
    uow: &Uow,
    card: i64,
    by: i64,
    at: i64,
    kind: &str,
    detail: String,
) -> Result<(), OrmError> {
    let id = Activity::query()
        .order_by_desc("id")
        .first(uow)
        .await?
        .map_or(1, |last| last.into_inner().id + 1);
    Activity::query()
        .insert(
            uow,
            [Activity {
                id,
                card_id: card,
                person_id: by,
                at,
                kind: kind.to_owned(),
                detail,
            }],
        )
        .await?;
    Ok(())
}

/// Adds a comment, counts it on its card, and says so in the card's history.
pub async fn comment(uow: &Uow, card: i64, by: i64, at: i64, body: String) -> Result<(), OrmError> {
    let id = Comment::query()
        .order_by_desc("id")
        .first(uow)
        .await?
        .map_or(1, |last| last.into_inner().id + 1);
    Comment::query()
        .insert(
            uow,
            [Comment {
                id,
                card_id: card,
                person_id: by,
                at,
                body,
            }],
        )
        .await?;
    let count = Comment::query()
        .where_(Comment::CARD_ID.eq(card))
        .count(uow)
        .await?;
    Card::query()
        .where_(Card::ID.eq(card))
        .update(uow, [Card::COMMENTS.set(i64::try_from(count).unwrap_or(0))])
        .await?;
    record(uow, card, by, at, "commented", String::new()).await
}

/// Places `mv` and writes it, renewing the ranks of its list once if its new
/// neighbours have no room between them. The placement, or why it was refused.
pub async fn place_and_write(
    uow: &Uow,
    viewer: &Person,
    mv: &navree::ui::Move,
    at: i64,
) -> Result<Result<Placement, navree::ui::Refusal>, OrmError> {
    let ranks = Ranks { uow, viewer };
    let mut placed = mv.place(&ranks).await?;
    if let Ok(Placement {
        rank: None,
        to_list,
        ..
    }) = &placed
    {
        renew_ranks(uow, to_list).await?;
        placed = mv.place(&ranks).await?;
    }
    if let Ok(placement) = &placed
        && placement.moved
        && let Some(rank) = placement.rank
    {
        move_card(uow, placement, rank, viewer.id, at).await?;
    }
    Ok(placed)
}
