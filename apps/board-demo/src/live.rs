//! The board as it changes: the nudge that tells every open page, through the
//! app's live hub (`RealtimeModule`), and a teammate who moves a card now and then
//! while the app runs.
//!
//! A nudge says only that the board changed. The page never reads it: the board
//! fetches itself again, so a nudge can't put anything in a page that the server
//! didn't draw.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use navree::db::{Db, Executor as _};
use navree::orm::Model as _;
use navree::prelude::BoxError;
use navree::realtime::{Live, Topic};
use navree::ui::{LiveStream, Move, sortable};

use crate::data::{self, Activity, Card, Check, Column, ColumnRow, Comment, Person};
use crate::store;

/// The live topic of the board. The board signs nobody in, so it is public.
pub const TOPIC: &str = "board";

/// The stream a board page listens on.
pub fn stream() -> LiveStream {
    LiveStream::topics([TOPIC])
}

/// Who hears about changes, and the teammate's dice.
pub struct Hub {
    live: Option<Arc<Live>>,
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
    /// A hub that tells nobody (the tests of the data).
    pub fn new() -> Self {
        Self {
            live: None,
            dice: Mutex::new(0x2026_1104),
            delay_ms: AtomicU64::new(0),
        }
    }

    /// Tells the pages through the app's live hub.
    pub fn with_live(mut self, live: Arc<Live>) -> Self {
        self.live = Some(live);
        self
    }

    /// Tells every open board that it changed. In a unit of work, once it commits.
    pub async fn nudge(&self) {
        let Some(live) = &self.live else {
            return;
        };
        if let Err(error) = live.nudge([Topic::public(TOPIC)]).await {
            eprintln!("board-demo: the pages could not be told: {error}");
        }
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
            self.nudge().await;
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
}

/// The time the clock is held at, in unix seconds; 0 when it runs.
static HELD: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

/// Holds the clock at `at` (unix seconds) for every page and everything the
/// app writes: for a test that compares two runs of the app.
pub fn hold_clock(at: i64) {
    HELD.store(at, std::sync::atomic::Ordering::Relaxed);
}

/// Now, in unix seconds.
pub fn now() -> i64 {
    match HELD.load(std::sync::atomic::Ordering::Relaxed) {
        0 => chrono::Utc::now().timestamp(),
        held => held,
    }
}
