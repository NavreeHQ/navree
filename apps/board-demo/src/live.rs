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
use navree::ui::Move;
use tokio::sync::broadcast;

use crate::data::{self, Activity, Card, Check, Column, Comment, Person};
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
            "DROP TABLE IF EXISTS people",
            "CREATE TABLE people (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT NOT NULL, job TEXT NOT NULL, role TEXT NOT NULL)",
            "CREATE TABLE cards (id INTEGER PRIMARY KEY, list TEXT NOT NULL, position INTEGER NOT NULL, title TEXT NOT NULL, description TEXT NOT NULL, assignee INTEGER, labels TEXT NOT NULL, due DATE, created_at INTEGER NOT NULL)",
            "CREATE INDEX cards_place ON cards (list, position, id)",
            "CREATE TABLE checks (id INTEGER PRIMARY KEY, card_id INTEGER NOT NULL, position INTEGER NOT NULL, text TEXT NOT NULL, done INTEGER NOT NULL)",
            "CREATE INDEX checks_card ON checks (card_id, position)",
            "CREATE TABLE comments (id INTEGER PRIMARY KEY, card_id INTEGER NOT NULL, person_id INTEGER NOT NULL, at INTEGER NOT NULL, body TEXT NOT NULL)",
            "CREATE INDEX comments_card ON comments (card_id, at)",
            "CREATE TABLE activity (id INTEGER PRIMARY KEY, card_id INTEGER NOT NULL, person_id INTEGER NOT NULL, at INTEGER NOT NULL, kind TEXT NOT NULL, detail TEXT NOT NULL)",
            "CREATE INDEX activity_card ON activity (card_id, at)",
        ] {
            pool.execute_script(statement).await?;
        }
        Ok(())
    }

    /// Writes the board afresh, as it stands at `now`.
    pub async fn seed(&self, db: &Db, now: i64) -> Result<(), BoxError> {
        Activity::query().delete(db).await?;
        Comment::query().delete(db).await?;
        Check::query().delete(db).await?;
        Card::query().delete(db).await?;
        Person::query().delete(db).await?;
        let seed = data::seed(now);
        Person::query().insert(db, seed.people).await?;
        Card::query().insert(db, seed.cards).await?;
        Check::query().insert(db, seed.checks).await?;
        Comment::query().insert(db, seed.comments).await?;
        for chunk in seed.activity.chunks(100) {
            Activity::query().insert(db, chunk.to_vec()).await?;
        }
        self.delay_ms.store(0, Ordering::SeqCst);
        Ok(())
    }

    /// Moves `item` to the end of `to`, as `by`, checked like any other move.
    /// Returns whether it moved; `nudge` false leaves every open page as it was,
    /// out of date, which the tests use to make a stale move.
    pub async fn move_as(
        &self,
        db: &Db,
        by: i64,
        item: i64,
        to: Column,
        nudge: bool,
    ) -> Result<bool, BoxError> {
        let moved = db
            .transaction(|uow| async move {
                let viewer = store::viewer(&uow, Some(by)).await.map_err(store::db)?;
                let lists = store::columns(&uow, &viewer).await.map_err(store::db)?;
                let Some(from) = lists
                    .iter()
                    .find(|list| list.items.contains(&item.to_string()))
                else {
                    return Ok(false);
                };
                let mv = Move::new(item.to_string(), from.key.clone(), to.key());
                match mv.check(&lists) {
                    Ok(landing) => {
                        store::move_card(&uow, &landing, by, now())
                            .await
                            .map_err(store::db)?;
                        Ok(landing.moved)
                    }
                    Err(_) => Ok(false),
                }
            })
            .await?;
        if moved && nudge {
            self.nudge();
        }
        Ok(moved)
    }

    /// One step of the teammate: a card in Ready or In progress moves one column
    /// on, if the next column has room.
    pub async fn step(&self, db: &Db) -> Result<bool, BoxError> {
        let cards = Card::query()
            .where_(Card::LIST.is_in(["ready", "doing", "backlog"]))
            .get(db)
            .await?
            .into_vec();
        if cards.is_empty() {
            return Ok(false);
        }
        let at = usize::try_from(self.roll(cards.len() as u64)).unwrap_or(0);
        let card = cards[at].clone().into_inner();
        let next = match Column::parse(&card.list) {
            Some(Column::Backlog) => Column::Ready,
            Some(Column::Ready) => Column::Doing,
            Some(Column::Doing) => Column::Review,
            _ => return Ok(false),
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
