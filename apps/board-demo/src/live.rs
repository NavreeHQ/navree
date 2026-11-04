//! The board as it changes: the event stream that tells every open page, and a
//! teammate who moves a card now and then while the app runs.
//!
//! An event on the stream is only a nudge. The page never reads what it says: the
//! board fetches itself again, so the stream can't put anything in a page that the
//! server didn't draw.

use std::collections::VecDeque;
use std::convert::Infallible;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use http::HeaderMap;
use http::header::CONTENT_TYPE;
use navree::db::{Db, Executor as _};
use navree::http::response::sse::{Event as Sse, KeepAlive, Sse as SseResponse};
use navree::http::response::{IntoResponse, Response};
use navree::orm::Model as _;
use navree::prelude::BoxError;
use navree::ui::{Move, sortable};
use tokio::sync::broadcast;

use crate::data::{self, Activity, Card, Check, Column, ColumnRow, Comment, Person};
use crate::store;

/// Who hears about changes, and the teammate's dice.
pub struct Hub {
    tx: broadcast::Sender<()>,
    dice: Mutex<u64>,
    /// How long the next moves wait before they are answered, for the browser tests.
    pub delay_ms: AtomicU64,
}

impl Default for Hub {
    fn default() -> Self {
        Self::new()
    }
}

impl Hub {
    pub fn new() -> Self {
        Self {
            tx: broadcast::channel(64).0,
            dice: Mutex::new(0x2026_1104),
            delay_ms: AtomicU64::new(0),
        }
    }

    /// Tells every open stream that the board changed.
    pub fn nudge(&self) {
        let _ = self.tx.send(());
    }

    /// A number below `n`, from a small fixed sequence, so a run can be replayed.
    fn roll(&self, n: u64) -> u64 {
        let mut state = self.dice.lock().unwrap_or_else(PoisonError::into_inner);
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        if n == 0 { 0 } else { *state % n }
    }

    /// Creates the tables, empty.
    pub async fn create(&self, db: &Db) -> Result<(), BoxError> {
        let pool = db.pool();
        for statement in [
            "DROP TABLE IF EXISTS activity",
            "DROP TABLE IF EXISTS comments",
            "DROP TABLE IF EXISTS checks",
            "DROP TABLE IF EXISTS cards",
            "DROP TABLE IF EXISTS columns",
            "DROP TABLE IF EXISTS people",
            "CREATE TABLE columns (key TEXT PRIMARY KEY, position INTEGER NOT NULL)",
            "CREATE TABLE people (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT NOT NULL, job TEXT NOT NULL, role TEXT NOT NULL)",
            "CREATE TABLE cards (id INTEGER PRIMARY KEY, list TEXT NOT NULL, rank INTEGER NOT NULL, title TEXT NOT NULL, description TEXT NOT NULL, assignee INTEGER, labels TEXT NOT NULL, due DATE, created_at INTEGER NOT NULL, checks_done INTEGER NOT NULL, checks_total INTEGER NOT NULL, comments INTEGER NOT NULL)",
            // A column's page, its count and a move's neighbours are each one walk of this index.
            "CREATE INDEX cards_place ON cards (list, rank, id)",
            "CREATE INDEX cards_assignee ON cards (list, assignee, rank, id)",
            "CREATE TABLE checks (id INTEGER PRIMARY KEY, card_id INTEGER NOT NULL, position INTEGER NOT NULL, text TEXT NOT NULL, done INTEGER NOT NULL)",
            "CREATE INDEX checks_card ON checks (card_id, position)",
            "CREATE TABLE comments (id INTEGER PRIMARY KEY, card_id INTEGER NOT NULL, person_id INTEGER NOT NULL, at INTEGER NOT NULL, body TEXT NOT NULL)",
            "CREATE INDEX comments_card ON comments (card_id, at, id)",
            "CREATE TABLE activity (id INTEGER PRIMARY KEY, card_id INTEGER NOT NULL, person_id INTEGER NOT NULL, at INTEGER NOT NULL, kind TEXT NOT NULL, detail TEXT NOT NULL)",
            "CREATE INDEX activity_card ON activity (card_id, at, id)",
        ] {
            pool.execute_script(statement).await?;
        }
        Ok(())
    }

    /// Writes the board afresh, as it stands at `now`: twenty thousand cards and
    /// their comments, lines and history, in one transaction.
    pub async fn seed(&self, db: &Db, now: i64) -> Result<(), BoxError> {
        let seed = data::seed(now);
        db.transaction(|uow| async move {
            let write = async {
                Activity::query().delete(&uow).await?;
                Comment::query().delete(&uow).await?;
                Check::query().delete(&uow).await?;
                Card::query().delete(&uow).await?;
                Person::query().delete(&uow).await?;
                ColumnRow::query().delete(&uow).await?;
                ColumnRow::query().insert(&uow, seed.columns).await?;
                Person::query().insert(&uow, seed.people).await?;
                // Rows of a few columns each, a few hundred at a time: well under
                // SQLite's limit of bound values in one statement.
                for chunk in seed.cards.chunks(500) {
                    Card::query().insert(&uow, chunk.to_vec()).await?;
                }
                for chunk in seed.checks.chunks(1_000) {
                    Check::query().insert(&uow, chunk.to_vec()).await?;
                }
                for chunk in seed.comments.chunks(1_000) {
                    Comment::query().insert(&uow, chunk.to_vec()).await?;
                }
                for chunk in seed.activity.chunks(1_000) {
                    Activity::query().insert(&uow, chunk.to_vec()).await?;
                }
                Ok::<_, navree::orm::OrmError>(())
            };
            write.await.map_err(store::db)
        })
        .await?;
        self.delay_ms.store(0, Ordering::SeqCst);
        Ok(())
    }

    /// Moves `item` to the end of `to`, as `by`, placed and checked like any other
    /// move. Returns whether it moved; `nudge` false leaves every open page as it
    /// was, out of date, which the tests use to make a stale move.
    pub async fn move_as(
        &self,
        db: &Db,
        by: i64,
        item: i64,
        to: Column,
        nudge: bool,
    ) -> Result<bool, BoxError> {
        // Where the card is isn't known until it is read, so every column is locked
        // first, in the toolkit's transaction, as a move from the page locks its two.
        let columns = Column::ALL.map(Column::key).to_vec();
        let moved = sortable::transaction(db, &ColumnRow::KEY, columns, |uow| async move {
            let Some(card) = Card::query().find(&uow, item).await? else {
                return Ok(false);
            };
            let mv = Move::new(item.to_string(), card.into_inner().list, to.key());
            let viewer = store::viewer(&uow, Some(by)).await?;
            let placed = store::place_and_write(&uow, &viewer, &mv, now()).await?;
            Ok(placed.is_ok_and(|placement| placement.moved))
        })
        .await?;
        if moved && nudge {
            self.nudge();
        }
        Ok(moved)
    }

    /// One step of the teammate: a card at the top of the backlog, Ready or In
    /// progress moves one column on, if the next column has room.
    pub async fn step(&self, db: &Db) -> Result<bool, BoxError> {
        let column = [Column::Backlog, Column::Ready, Column::Doing]
            [usize::try_from(self.roll(3)).unwrap_or(0)];
        let cards = Card::query()
            .where_(Card::LIST.eq(column.key()))
            .order_by("rank")
            .limit(5)
            .get(db)
            .await?
            .into_vec();
        if cards.is_empty() {
            return Ok(false);
        }
        let at = usize::try_from(self.roll(cards.len() as u64)).unwrap_or(0);
        let card = cards[at].clone().into_inner();
        let next = match column {
            Column::Backlog => Column::Ready,
            Column::Ready => Column::Doing,
            _ => Column::Review,
        };
        self.move_as(db, card.assignee.unwrap_or(2), card.id, next, true)
            .await
    }

    /// Runs the teammate: a move every `every`, give or take a little, until the
    /// task holding it is dropped.
    pub async fn simulate(self: Arc<Self>, db: Arc<Db>, every: Duration) {
        loop {
            let jitter = Duration::from_millis(self.roll(2_000));
            tokio::time::sleep(every + jitter).await;
            if let Err(error) = self.step(&db).await {
                eprintln!("board-demo: the teammate could not move a card: {error}");
            }
        }
    }

    /// `GET /events`: the stream. Each change of the board is one event.
    pub fn stream(&self, headers: &HeaderMap) -> Response {
        // The runtime asks once what a failed stream answers: say it's a stream.
        if headers.contains_key("x-ui-probe") {
            return ([(CONTENT_TYPE, "text/event-stream")], "").into_response();
        }
        let rx = self.tx.subscribe();
        // A first comment goes out at once: Firefox reports a stream open only when
        // its first bytes arrive.
        let queue = VecDeque::from([Sse::default().comment("ready")]);
        let events = futures_util::stream::unfold((rx, queue), |(mut rx, mut queue)| async move {
            loop {
                if let Some(event) = queue.pop_front() {
                    return Some((Ok::<_, Infallible>(event), (rx, queue)));
                }
                match rx.recv().await {
                    Ok(()) | Err(broadcast::error::RecvError::Lagged(_)) => {
                        queue.push_back(Sse::default().data("changed"));
                    }
                    Err(broadcast::error::RecvError::Closed) => return None,
                }
            }
        });
        SseResponse::new(events)
            .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
            .into_response()
    }
}

/// Now, in unix seconds.
pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}
