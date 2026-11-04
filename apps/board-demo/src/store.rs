//! Reading the board and writing its moves. A move is checked and written in one
//! transaction, which SQLite starts with `BEGIN IMMEDIATE`: two moves at once queue
//! for the write lock, and the second is checked against what the first left.

use navree::db::{DbError, ErrorKind, Uow};
use navree::orm::{Model as _, OrmError};
use navree::ui::{Landing, ListState};

use crate::data::{Activity, Card, Check, Column, Comment, Person};
use crate::views::board::BoardData;
use crate::views::card::CardData;

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

/// Every card, column by column, in order.
pub async fn cards(uow: &Uow) -> Result<Vec<Card>, OrmError> {
    let mut cards: Vec<Card> = Card::query()
        .order_by("position")
        .order_by("id")
        .get(uow)
        .await?
        .into_vec()
        .into_iter()
        .map(|record| record.into_inner())
        .collect();
    cards.sort_by_key(|card| {
        Column::ALL
            .iter()
            .position(|c| c.key() == card.list)
            .unwrap_or(9)
    });
    Ok(cards)
}

/// What the board draws.
pub async fn board(uow: &Uow) -> Result<BoardData, OrmError> {
    Ok(BoardData {
        people: people(uow).await?,
        cards: cards(uow).await?,
        checks: Check::query()
            .get(uow)
            .await?
            .into_vec()
            .into_iter()
            .map(|record| record.into_inner())
            .collect(),
        comments: Comment::query()
            .get(uow)
            .await?
            .into_vec()
            .into_iter()
            .map(|record| record.into_inner())
            .collect(),
    })
}

/// What the drawer draws for card `id`, if it is on the board.
pub async fn card(uow: &Uow, id: i64) -> Result<Option<CardData>, OrmError> {
    let Some(card) = Card::query().find(uow, id).await? else {
        return Ok(None);
    };
    Ok(Some(CardData {
        card: card.into_inner(),
        checks: checks(uow, id).await?,
        comments: Comment::query()
            .where_(Comment::CARD_ID.eq(id))
            .order_by("at")
            .order_by("id")
            .get(uow)
            .await?
            .into_vec()
            .into_iter()
            .map(|record| record.into_inner())
            .collect(),
        activity: Activity::query()
            .where_(Activity::CARD_ID.eq(id))
            .order_by("at")
            .order_by("id")
            .get(uow)
            .await?
            .into_vec()
            .into_iter()
            .map(|record| record.into_inner())
            .collect(),
        people: people(uow).await?,
    }))
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

/// The columns as the server holds them now, with their limits, and Done closed
/// to anyone who isn't a lead: what a move is checked against.
pub async fn columns(uow: &Uow, viewer: &Person) -> Result<Vec<ListState>, OrmError> {
    let cards = cards(uow).await?;
    Ok(Column::ALL
        .iter()
        .map(|column| {
            let keys = cards
                .iter()
                .filter(|card| card.list == column.key())
                .map(|card| card.id.to_string());
            ListState::new(column.key(), column.title(), keys)
                .limit(column.limit())
                .refuse_if(
                    *column == Column::Done && !viewer.is_lead(),
                    "Only a lead can move cards to Done.",
                )
        })
        .collect())
}

/// Writes a checked move of a card: both columns' new orders, and a line in the
/// card's history when it changed column.
pub async fn move_card(uow: &Uow, landing: &Landing, by: i64, at: i64) -> Result<(), OrmError> {
    for (order, list) in [
        (&landing.order, &landing.to_list),
        (&landing.left, &landing.from_list),
    ] {
        for (position, key) in order.iter().enumerate() {
            let Ok(id) = key.parse::<i64>() else { continue };
            Card::query()
                .where_(Card::ID.eq(id))
                .update(
                    uow,
                    [
                        Card::LIST.set(list.clone()),
                        Card::POSITION.set(position as i64),
                    ],
                )
                .await?;
        }
    }
    if landing.from_list != landing.to_list {
        let title = |key: &str| Column::parse(key).map_or("", Column::title);
        record(
            uow,
            landing.item.parse().unwrap_or(0),
            by,
            at,
            "moved",
            format!(
                "{} to {}",
                title(&landing.from_list),
                title(&landing.to_list)
            ),
        )
        .await?;
    }
    Ok(())
}

/// Writes a checked move of a checklist line.
pub async fn move_check(uow: &Uow, landing: &Landing) -> Result<(), OrmError> {
    for (position, key) in landing.order.iter().enumerate() {
        let Ok(id) = key.parse::<i64>() else { continue };
        Check::query()
            .where_(Check::ID.eq(id))
            .update(uow, [Check::POSITION.set(position as i64)])
            .await?;
    }
    Ok(())
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
        .map_or(1, |last| last.id + 1);
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

/// Adds a comment, and says so in the card's history.
pub async fn comment(uow: &Uow, card: i64, by: i64, at: i64, body: String) -> Result<(), OrmError> {
    let id = Comment::query()
        .order_by_desc("id")
        .first(uow)
        .await?
        .map_or(1, |last| last.id + 1);
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
    record(uow, card, by, at, "commented", String::new()).await
}
