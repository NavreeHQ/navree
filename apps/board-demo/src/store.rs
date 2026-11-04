//! Reading the board and writing its moves. Every read is a page by an index:
//! a column's cards by `(list, rank, id)`, a card's comments and history by
//! `(card_id, at, id)`. A move is checked and written in one transaction that
//! first locks the columns it touches: `SELECT ... FOR UPDATE` on their rows on
//! PostgreSQL and MySQL, and on SQLite the write lock `BEGIN IMMEDIATE` takes. Two
//! moves at once queue, and the second is checked against what the first left.

use std::collections::HashMap;
use std::future::Future;

use chrono::{Days, NaiveDate};
use navree::db::{DbError, ErrorKind, Uow};
use navree::orm::{Model as _, OrmError, Value, raw};
use navree::ui::ListState;
use navree::ui::sortable::{self, Placement, RankedLists, Spot};

use crate::data::{Activity, Card, Check, Column, ColumnRow, Comment, Person};
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

/// The person with `id`, or the first member when there is none: who the board is
/// shown as. Nobody is a lead until they choose to be.
pub async fn viewer(uow: &Uow, id: Option<i64>) -> Result<Person, OrmError> {
    let people = people(uow).await?;
    let named = id.and_then(|id| people.iter().find(|person| person.id == id).cloned());
    Ok(named
        .or_else(|| people.iter().find(|person| !person.is_lead()).cloned())
        .unwrap_or_default())
}

/// Where a page of a column ends: the last card drawn, by its id. The next page
/// starts after that card wherever it is now, so fresh ranks between two pages
/// skip nothing.
pub fn cursor(card: &Card) -> String {
    card.id.to_string()
}

/// A card's id from a cursor: digits only, or `None`.
pub fn parse_card(text: &str) -> Option<i64> {
    let plain = !text.is_empty() && text.len() <= 18 && text.bytes().all(|b| b.is_ascii_digit());
    plain.then(|| text.parse().ok()).flatten()
}

/// `at.id`, a comment's or a line of history's place, read back, or `None` for
/// one that doesn't parse.
pub fn parse_cursor(text: &str) -> Option<(i64, i64)> {
    let (rank, id) = text.rsplit_once('.')?;
    Some((rank.parse().ok()?, id.parse().ok()?))
}

/// Where a page of a column starts: after the card `card`, wherever it stands in
/// the column now. When that card has left the column, after the first `skip`
/// cards that pass the filters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct After {
    pub card: i64,
    pub skip: usize,
}

/// The cards of `column` that pass `filters`, after `after` when given, at most
/// `limit` of them, and whether more follow.
pub async fn column_cards(
    uow: &Uow,
    column: Column,
    filters: &Filters,
    today: NaiveDate,
    after: Option<After>,
    limit: usize,
) -> Result<(Vec<Card>, bool), OrmError> {
    let mut query = Card::query().where_(Card::LIST.eq(column.key()));
    let mut skip = 0;
    if let Some(after) = after {
        match Card::query().find(uow, after.card).await? {
            Some(card) if card.list == column.key() => {
                let (rank, id) = (card.rank, card.id);
                query = query.where_(
                    Card::RANK
                        .gt(rank)
                        .or(Card::RANK.eq(rank).and(Card::ID.gt(id))),
                );
            }
            _ => skip = after.skip,
        }
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
        .offset(u64::try_from(skip).unwrap_or(u64::MAX))
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

/// Where each of `cards` stands in the whole of `column`, 1 being first: one walk
/// of the column's index, numbered by a window. For a filter, whose cards are some
/// of the column's, so their move forms count in the column the server places by.
pub async fn places(uow: &Uow, column: Column, cards: &[Card]) -> Result<Vec<usize>, OrmError> {
    if cards.is_empty() {
        return Ok(Vec::new());
    }
    let marks = vec!["?"; cards.len()].join(", ");
    let sql = format!(
        "SELECT id, n FROM (SELECT id, ROW_NUMBER() OVER (ORDER BY rank, id) AS n FROM cards WHERE list = ?) AS ranked WHERE id IN ({marks})"
    );
    let mut values = vec![Value::from(column.key())];
    values.extend(cards.iter().map(|card| Value::from(card.id)));
    let found: HashMap<i64, usize> = navree::orm::select(uow, raw(sql, values))
        .await?
        .iter()
        .filter_map(|row| {
            let id = row.get::<i64>("id").ok()?;
            let n = usize::try_from(row.get::<i64>("n").ok()?).ok()?;
            Some((id, n))
        })
        .collect();
    Ok(cards
        .iter()
        .map(|card| found.get(&card.id).copied().unwrap_or(1))
        .collect())
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
            places: if filters.is_empty() {
                Vec::new()
            } else {
                places(uow, column, &cards).await?
            },
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

/// The card `key` names: digits only, no sign and no leading zero, so one card has
/// one key. Anything else names no card.
fn id_of(key: &str) -> i64 {
    let plain = !key.is_empty()
        && key.len() <= 18
        && key.bytes().all(|b| b.is_ascii_digit())
        && !key.starts_with('0');
    if plain { key.parse().unwrap_or(0) } else { 0 }
}

impl RankedLists for Ranks<'_> {
    type Error = OrmError;

    fn lock(&self, lists: &[&str]) -> impl Future<Output = Result<(), OrmError>> + Send {
        let (uow, lists) = (self.uow, lists.iter().map(|key| (*key).to_owned()));
        let lists: Vec<String> = lists.collect();
        async move { sortable::lock_lists(uow, &ColumnRow::KEY, lists).await }
    }

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

/// Gives every card of `list` a fresh rank, [`RANK_GAP`](sortable::RANK_GAP) apart,
/// in its order, in one statement: for when two neighbours have no room left
/// between them.
pub async fn renew_ranks(uow: &Uow, list: &str) -> Result<(), OrmError> {
    sortable::renew_ranks(uow, &Card::LIST, &Card::RANK, &Card::ID, list.to_owned()).await?;
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
    // A card in Done is a lead's to reopen, as it was a lead's to close.
    if !viewer.is_lead() && mv.from_list == Column::Done.key() && mv.to_list != Column::Done.key() {
        return Ok(Err(navree::ui::Refusal::Refused {
            reason: "Only a lead can reopen a card from Done.".to_owned(),
        }));
    }
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
