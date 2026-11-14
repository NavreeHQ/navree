//! Moves at the same moment, on PostgreSQL, MySQL and SQLite: `Move::place` and
//! `Move::check` in the transaction the toolkit owns (`Move::transaction`, which
//! begins at READ COMMITTED, locks the lists a move touches and only then hands
//! the unit of work over), the lock itself (`navree::ui::sortable::lock_lists`) and
//! what it refuses, and a whole list's fresh ranks in one statement
//! (`renew_ranks`). Each race also runs without the lock: then every move waits
//! between its reads and its write until all have read, which is what two moves at
//! once do when the timing goes against them.
//!
//! The SQLite tests always run. The PostgreSQL and MySQL ones run against real
//! servers, at each isolation level a pool can be set to: build with their
//! features and set `NAVREE_TEST_POSTGRES_URL` and `NAVREE_TEST_MYSQL_URL` (or have
//! `target/test-databases.env`). Each test works in a database of its own.
//!
//! ```text
//! cargo test -p navree --features sqlite,postgres,mysql --test sortable_locks -- --nocapture
//! ```

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use navree::db::{
    Db, DbError, DbOptions, ErrorKind, Executor as _, IsolationLevel, MySqlOptions,
    PostgresOptions, Uow,
};
use navree::orm::testing::database::TestDb;
use navree::orm::{Dialect, Model as _, OrmError, raw, scalar};
use navree::prelude::*;
use navree::ui::sortable::{self, ListScope, Placement, RANK_GAP, RankedLists, Spot};
use navree::ui::{Landing, ListState, Move, Refusal};
use tokio::sync::Barrier;

/// A lane of a board: one row each, the row a move locks.
#[derive(Model, Clone, Debug, Default, serde::Serialize)]
#[model(table = "lanes", primary_key = "key")]
struct Lane {
    key: String,
    title: String,
    max_items: Option<i64>,
}

/// A task in a lane, at a rank. `rank` is a reserved word on MySQL: the lock and
/// the renewal quote it.
#[derive(Model, Clone, Debug, Default, serde::Serialize)]
#[model(table = "tasks")]
struct Task {
    id: i64,
    lane: String,
    rank: i64,
}

/// The tables, in `db`'s own quoting.
async fn schema(db: &Db) {
    let dialect = Dialect::from(db.backend());
    let q = |name: &str| dialect.quote_identifier(name);
    for sql in [
        format!(
            "CREATE TABLE {} ({} VARCHAR(64) PRIMARY KEY, {} VARCHAR(64) NOT NULL, {} BIGINT NULL)",
            q("lanes"),
            q("key"),
            q("title"),
            q("max_items")
        ),
        format!(
            "CREATE TABLE {} ({} BIGINT PRIMARY KEY, {} VARCHAR(64) NOT NULL, {} BIGINT NOT NULL)",
            q("tasks"),
            q("id"),
            q("lane"),
            q("rank")
        ),
        format!(
            "CREATE INDEX tasks_place ON {} ({}, {}, {})",
            q("tasks"),
            q("lane"),
            q("rank"),
            q("id")
        ),
    ] {
        db.pool().execute_script(&sql).await.expect("schema");
    }
}

/// Lanes `(key, limit, how many tasks)`, their tasks numbered from 1 in order and
/// ranked [`RANK_GAP`] apart. The ids of each lane's tasks.
async fn seed(db: &Db, lanes: &[(&str, Option<i64>, usize)]) -> Vec<Vec<i64>> {
    Lane::query()
        .insert(
            db,
            lanes.iter().map(|(key, limit, _)| Lane {
                key: (*key).to_owned(),
                title: key.to_uppercase(),
                max_items: *limit,
            }),
        )
        .await
        .expect("lanes");
    let mut next = 1;
    let mut ids = Vec::new();
    for (key, _, count) in lanes {
        let tasks: Vec<Task> = (0..*count)
            .map(|at| {
                next += 1;
                Task {
                    id: next - 1,
                    lane: (*key).to_owned(),
                    rank: at as i64 * RANK_GAP,
                }
            })
            .collect();
        ids.push(tasks.iter().map(|task| task.id).collect());
        for chunk in tasks.chunks(1_000) {
            Task::query()
                .insert(db, chunk.to_vec())
                .await
                .expect("tasks");
        }
    }
    ids
}

/// The tasks of `lane`, in order: `(id, rank)`.
async fn order(db: &Db, lane: &str) -> Vec<(i64, i64)> {
    Task::query()
        .where_(Task::LANE.eq(lane))
        .order_by("rank")
        .order_by("id")
        .get(db)
        .await
        .expect("order")
        .into_vec()
        .into_iter()
        .map(|record| {
            let task = record.into_inner();
            (task.id, task.rank)
        })
        .collect()
}

fn id_of(key: &str) -> i64 {
    key.parse().unwrap_or(0)
}

fn db_error(error: OrmError) -> DbError {
    DbError::new(ErrorKind::Other, error.to_string())
}

/// The board as `Move::place` reads it, in one unit of work: one task, one count
/// or one neighbour at a time. `locking` false leaves out the lock.
struct Board<'a> {
    uow: &'a Uow,
    locking: bool,
}

impl RankedLists for Board<'_> {
    type Error = OrmError;

    fn lock(&self, lists: &[&str]) -> impl Future<Output = Result<(), OrmError>> + Send {
        let (uow, locking) = (self.uow, self.locking);
        let lists: Vec<String> = lists.iter().map(|key| (*key).to_owned()).collect();
        async move {
            if locking {
                sortable::lock_lists(uow, &Lane::KEY, lists).await
            } else {
                Ok(())
            }
        }
    }

    fn spot(&self, key: &str) -> impl Future<Output = Result<Option<Spot>, OrmError>> + Send {
        let (uow, id) = (self.uow, id_of(key));
        async move {
            Ok(Task::query().find(uow, id).await?.map(|record| {
                let task = record.into_inner();
                Spot {
                    list: task.lane,
                    rank: task.rank,
                }
            }))
        }
    }

    fn list(
        &self,
        key: &str,
        item: &str,
    ) -> impl Future<Output = Result<Option<(ListState, u64)>, OrmError>> + Send {
        let (uow, key, item) = (self.uow, key.to_owned(), id_of(item));
        async move {
            let Some(lane) = Lane::query().find(uow, key.clone()).await? else {
                return Ok(None);
            };
            let lane = lane.into_inner();
            let count = Task::query()
                .where_(Task::LANE.eq(key))
                .where_(Task::ID.ne(item))
                .count(uow)
                .await?;
            let limit = lane.max_items.and_then(|n| u32::try_from(n).ok());
            let state = ListState::new(lane.key, lane.title, Vec::<String>::new()).limit(limit);
            Ok(Some((state, count)))
        }
    }

    fn beside(
        &self,
        list: &str,
        anchor: &str,
        rank: i64,
        lower: bool,
        item: &str,
    ) -> impl Future<Output = Result<Option<i64>, OrmError>> + Send {
        let (uow, list, anchor, item) = (self.uow, list.to_owned(), id_of(anchor), id_of(item));
        async move {
            let query = Task::query()
                .where_(Task::LANE.eq(list))
                .where_(Task::ID.ne(item));
            // By rank, then by id: a task tied with the anchor is its neighbour too.
            let found = if lower {
                query
                    .where_(
                        Task::RANK
                            .lt(rank)
                            .or(Task::RANK.eq(rank).and(Task::ID.lt(anchor))),
                    )
                    .order_by_desc("rank")
                    .order_by_desc("id")
            } else {
                query
                    .where_(
                        Task::RANK
                            .gt(rank)
                            .or(Task::RANK.eq(rank).and(Task::ID.gt(anchor))),
                    )
                    .order_by("rank")
                    .order_by("id")
            };
            Ok(found.first(uow).await?.map(|r| r.into_inner().rank))
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
            Ok(Task::query()
                .where_(Task::LANE.eq(list))
                .where_(Task::ID.ne(item))
                .order_by("rank")
                .order_by("id")
                .offset(index)
                .first(uow)
                .await?
                .map(|r| r.into_inner().rank))
        }
    }

    fn last(
        &self,
        list: &str,
        item: &str,
    ) -> impl Future<Output = Result<Option<i64>, OrmError>> + Send {
        let (uow, list, item) = (self.uow, list.to_owned(), id_of(item));
        async move {
            Ok(Task::query()
                .where_(Task::LANE.eq(list))
                .where_(Task::ID.ne(item))
                .order_by_desc("rank")
                .order_by_desc("id")
                .first(uow)
                .await?
                .map(|r| r.into_inner().rank))
        }
    }
}

/// How a race's moves run.
#[derive(Clone, Copy, Debug)]
struct Way {
    /// In the toolkit's transaction (`Move::transaction`): READ COMMITTED, the
    /// lists locked first, then the move. Otherwise in a unit of work of the
    /// move's own, as an app writes it by hand.
    toolkit: bool,
    /// The lock is taken (in a unit of work of the move's own; the toolkit's
    /// always takes it).
    locking: bool,
    /// Every move waits, once it has read what it needs and before it writes,
    /// until all the race's moves have read: the worst timing for moves without a
    /// lock. (With the lock the moves run one after another, so nothing waits.)
    hold: bool,
    /// Each transaction reads something before it moves, as a handler reading
    /// its user first does. In a unit of work of the move's own, every move waits
    /// after that read until all have made it; in the toolkit's, the read comes
    /// after the lock, which is the only place it can go.
    read_first: bool,
}

const TOOLKIT: Way = Way {
    toolkit: true,
    locking: true,
    hold: false,
    read_first: true,
};

#[cfg(any(feature = "postgres", feature = "mysql"))]
const UNLOCKED: Way = Way {
    toolkit: false,
    locking: false,
    hold: true,
    read_first: false,
};

/// By hand: the move's own unit of work, the lock taken in `place`, a read before it.
#[cfg(any(feature = "postgres", feature = "mysql"))]
const BY_HAND: Way = Way {
    toolkit: false,
    locking: true,
    hold: false,
    read_first: true,
};

type Outcome<T> = Result<Result<T, Refusal>, OrmError>;

/// Places `mv` by rank in `uow`, renewing the lane's ranks once when its new
/// neighbours have no room between them, and writes it.
async fn place_and_write(uow: &Uow, mv: &Move, way: Way, gate: &Barrier) -> Outcome<Placement> {
    let board = Board {
        uow,
        locking: way.locking,
    };
    let mut placed = mv.place(&board).await?;
    if let Ok(Placement {
        rank: None,
        to_list,
        ..
    }) = &placed
    {
        sortable::renew_ranks(uow, &Task::LANE, &Task::RANK, &Task::ID, to_list.clone()).await?;
        placed = mv.place(&board).await?;
    }
    if way.hold {
        gate.wait().await;
    }
    if let Ok(placement) = &placed
        && placement.moved
        && let Some(rank) = placement.rank
    {
        Task::query()
            .where_(Task::ID.eq(id_of(&placement.item)))
            .update(
                uow,
                [
                    Task::LANE.set(placement.to_list.clone()),
                    Task::RANK.set(rank),
                ],
            )
            .await?;
    }
    Ok(placed)
}

/// Places `mv` and writes it, in the toolkit's transaction or a unit of work of
/// its own.
async fn placed(db: &Db, mv: Move, way: Way, gate: Arc<Barrier>) -> Outcome<Placement> {
    if way.toolkit {
        return mv
            .transaction(db, &Lane::KEY, |uow| {
                let (mv, gate) = (mv.clone(), gate.clone());
                async move {
                    if way.read_first {
                        Lane::query().get(&uow).await?;
                    }
                    place_and_write(&uow, &mv, way, &gate).await
                }
            })
            .await;
    }
    db.transaction(|uow| async move {
        if way.read_first {
            Lane::query().get(&uow).await.map_err(db_error)?;
            gate.wait().await;
        }
        place_and_write(&uow, &mv, way, &gate)
            .await
            .map_err(db_error)
    })
    .await
    .map_err(OrmError::from)
}

/// Checks `mv` against the lanes read whole and writes their new order: the way
/// for lists short enough to read.
async fn check_and_write(uow: &Uow, mv: &Move, way: Way, gate: &Barrier) -> Outcome<Landing> {
    let lanes = Lane::query().order_by("key").get(uow).await?;
    let mut lists = Vec::new();
    for lane in lanes.into_vec() {
        let lane = lane.into_inner();
        let keys: Vec<String> = Task::query()
            .where_(Task::LANE.eq(lane.key.clone()))
            .order_by("rank")
            .order_by("id")
            .get(uow)
            .await?
            .into_vec()
            .into_iter()
            .map(|r| r.into_inner().id.to_string())
            .collect();
        let limit = lane.max_items.and_then(|n| u32::try_from(n).ok());
        lists.push(ListState::new(lane.key, lane.title, keys).limit(limit));
    }
    let landing = mv.check(&lists);
    if way.hold {
        gate.wait().await;
    }
    if let Ok(landing) = &landing
        && landing.moved
    {
        for (at, key) in landing.order.iter().enumerate() {
            Task::query()
                .where_(Task::ID.eq(id_of(key)))
                .update(
                    uow,
                    [
                        Task::LANE.set(landing.to_list.clone()),
                        Task::RANK.set(at as i64 * RANK_GAP),
                    ],
                )
                .await?;
        }
    }
    Ok(landing)
}

/// Checks `mv` and writes it, in the toolkit's transaction or a unit of work of
/// its own.
async fn checked(db: &Db, mv: Move, way: Way, gate: Arc<Barrier>) -> Outcome<Landing> {
    if way.toolkit {
        return mv
            .transaction(db, &Lane::KEY, |uow| {
                let (mv, gate) = (mv.clone(), gate.clone());
                async move { check_and_write(&uow, &mv, way, &gate).await }
            })
            .await;
    }
    db.transaction(|uow| async move {
        if way.locking {
            mv.lock(&uow, &Lane::KEY).await.map_err(db_error)?;
        }
        check_and_write(&uow, &mv, way, &gate)
            .await
            .map_err(db_error)
    })
    .await
    .map_err(OrmError::from)
}

/// Runs `moves` at once, each through `run` in a task of its own; their results
/// in order.
async fn race<T, F, Fut>(db: &Db, moves: Vec<Move>, way: Way, run: F) -> Vec<T>
where
    T: Send + 'static,
    F: Fn(Db, Move, Way, Arc<Barrier>) -> Fut,
    Fut: Future<Output = T> + Send + 'static,
{
    let gate = Arc::new(Barrier::new(moves.len()));
    let tasks: Vec<_> = moves
        .into_iter()
        .map(|mv| tokio::spawn(run(db.clone(), mv, way, gate.clone())))
        .collect();
    let mut out = Vec::new();
    for task in tasks {
        out.push(task.await.expect("the move's task"));
    }
    out
}

/// What a race for one free place came to.
#[derive(Debug, PartialEq, Eq)]
struct Race {
    /// Moves taken.
    taken: usize,
    /// Moves refused as full.
    full: usize,
    /// Moves that failed, with why.
    errors: Vec<String>,
    /// Tasks the lane holds after, at most 3.
    held: usize,
}

/// Twenty tasks of the backlog race for the one free place in a lane that holds
/// at most three and has two.
async fn one_free_place(db: &Db, way: Way, by_rank: bool) -> Race {
    let ids = seed(db, &[("backlog", None, 20), ("doing", Some(3), 2)]).await;
    let moves: Vec<Move> = ids[0]
        .iter()
        .map(|id| Move::new(id.to_string(), "backlog", "doing"))
        .collect();
    let outcomes: Vec<Result<bool, String>> = if by_rank {
        race(db, moves, way, |db, mv, way, gate| async move {
            placed(&db, mv, way, gate).await
        })
        .await
        .into_iter()
        .map(tally)
        .collect()
    } else {
        race(db, moves, way, |db, mv, way, gate| async move {
            checked(&db, mv, way, gate).await
        })
        .await
        .into_iter()
        .map(tally)
        .collect()
    };
    Race {
        taken: outcomes.iter().filter(|o| matches!(o, Ok(true))).count(),
        full: outcomes.iter().filter(|o| matches!(o, Ok(false))).count(),
        errors: outcomes.into_iter().filter_map(Result::err).collect(),
        held: order(db, "doing").await.len(),
    }
}

/// Taken (`true`), refused as full (`false`), or what failed.
fn tally<T: std::fmt::Debug>(outcome: Outcome<T>) -> Result<bool, String> {
    match outcome {
        Ok(Ok(_)) => Ok(true),
        Ok(Err(Refusal::Full { limit: 3, .. })) => Ok(false),
        Ok(Err(other)) => Err(format!("refused: {other:?}")),
        Err(error) => Err(error.to_string()),
    }
}

/// One move taken, nineteen refused as full, none failed, three held.
fn exactly_one() -> Race {
    Race {
        taken: 1,
        full: 19,
        errors: Vec::new(),
        held: 3,
    }
}

/// Two tasks of one lane move into the same gap of another, before the same task:
/// the ranks they get, and the lane's order after.
async fn one_gap(db: &Db, way: Way) -> (i64, i64, Vec<(i64, i64)>) {
    let ids = seed(db, &[("todo", None, 4), ("doing", None, 2)]).await;
    let before = ids[0][1].to_string();
    let moves: Vec<Move> = ids[1]
        .iter()
        .map(|id| Move::new(id.to_string(), "doing", "todo").before(before.clone()))
        .collect();
    let out = race(db, moves, way, |db, mv, way, gate| async move {
        placed(&db, mv, way, gate).await
    })
    .await;
    let ranks: Vec<i64> = out
        .iter()
        .map(|r| {
            r.as_ref()
                .expect("no error")
                .as_ref()
                .expect("both moves are taken")
                .rank
                .expect("a rank")
        })
        .collect();
    (ranks[0], ranks[1], order(db, "todo").await)
}

/// A pool on the test database at `url`, with room for every move of a race at
/// once, its transactions at `level` by default on PostgreSQL and MySQL when given.
async fn pool(url: &str, level: Option<IsolationLevel>) -> Db {
    Db::connect(&DbOptions {
        url: url.to_owned(),
        max_connections: 32,
        mysql: MySqlOptions {
            isolation_level: level,
            ..MySqlOptions::default()
        },
        postgres: PostgresOptions {
            isolation_level: level,
            ..PostgresOptions::default()
        },
        ..DbOptions::default()
    })
    .await
    .expect("connects")
}

/// A database of its own on `server`'s test server, its tables made, and a pool
/// on it; `None` when the server isn't configured.
async fn fresh(server: &str, level: Option<IsolationLevel>) -> Option<(TestDb, Db)> {
    let test = match server {
        "sqlite" => TestDb::sqlite().await.expect("sqlite"),
        "postgres" => TestDb::postgres().await.expect("postgres")?,
        _ => TestDb::mysql().await.expect("mysql")?,
    };
    let db = pool(test.url(), level).await;
    schema(&db).await;
    Some((test, db))
}

async fn done(test: TestDb, db: Db) {
    db.close().await.expect("closes");
    test.close().await.expect("drops the database");
}

/// The levels a pool can begin its transactions at: the server's own, and each one
/// set. SQLite's transactions are serializable whatever is set.
fn levels(server: &str) -> Vec<Option<IsolationLevel>> {
    if server == "sqlite" {
        return vec![None];
    }
    vec![
        None,
        Some(IsolationLevel::ReadCommitted),
        Some(IsolationLevel::RepeatableRead),
        Some(IsolationLevel::Serializable),
    ]
}

/// Through the toolkit's transaction, at every level the pool may be set to, one
/// of twenty moves takes the one free place, by rank and read whole, with a read
/// in each move; two moves into one gap get ranks of their own.
async fn moves_at_once(server: &str) {
    for level in levels(server) {
        for by_rank in [true, false] {
            let Some((test, db)) = fresh(server, level).await else {
                return;
            };
            let race = one_free_place(&db, TOOLKIT, by_rank).await;
            eprintln!("{server} at {level:?}, by rank {by_rank}: {race:?}");
            assert_eq!(
                race,
                exactly_one(),
                "{server} at {level:?}, by rank {by_rank}"
            );
            done(test, db).await;
        }
    }
    let Some((test, db)) = fresh(server, None).await else {
        return;
    };
    let (a, b, todo) = one_gap(&db, TOOLKIT).await;
    assert_ne!(
        a, b,
        "{server}: two moves into one gap get ranks of their own"
    );
    let ranks: Vec<i64> = todo.iter().map(|(_, rank)| *rank).collect();
    assert!(ranks.windows(2).all(|w| w[0] < w[1]), "{server}: {todo:?}");
    // Both land between the first task and the one they were dropped before.
    let ids: Vec<i64> = todo.iter().map(|(id, _)| *id).collect();
    assert_eq!(ids[0], 1, "{server}: {ids:?}");
    assert_eq!(ids[3], 2, "{server}: {ids:?}");
    done(test, db).await;
}

#[cfg(any(feature = "postgres", feature = "mysql"))]
async fn moves_at_once_without_the_lock(server: &str) {
    let Some((test, db)) = fresh(server, None).await else {
        return;
    };
    let race = one_free_place(&db, UNLOCKED, true).await;
    assert!(
        race.taken > 1 && race.held > 3,
        "{server}: by rank, unlocked: {race:?}"
    );
    eprintln!(
        "{server}: without the lock, {} of 20 moves took the one free place; the lane holds {}, at most 3",
        race.taken, race.held
    );
    done(test, db).await;

    let Some((test, db)) = fresh(server, None).await else {
        return;
    };
    let race = one_free_place(&db, UNLOCKED, false).await;
    assert!(
        race.taken > 1 && race.held > 3,
        "{server}: read whole, unlocked: {race:?}"
    );
    done(test, db).await;

    let Some((test, db)) = fresh(server, None).await else {
        return;
    };
    let (a, b, _) = one_gap(&db, UNLOCKED).await;
    assert_eq!(
        a, b,
        "{server}: without the lock both moves take the same rank"
    );
    eprintln!("{server}: without the lock, two moves into one gap both took rank {a}");
    done(test, db).await;
}

/// By hand, a lock outside READ COMMITTED is refused: a transaction whose reads
/// keep an older snapshot can't be held to the lock. Through the toolkit, the
/// same pool takes exactly one.
#[cfg(any(feature = "postgres", feature = "mysql"))]
async fn a_lock_outside_read_committed_is_refused(server: &str) {
    // With a read before the lock, and with the lock first (at PostgreSQL's
    // REPEATABLE READ the snapshot comes from the first statement, so even the lock
    // first let 18 of 20 through).
    for (level, read_first) in [
        (IsolationLevel::RepeatableRead, true),
        (IsolationLevel::RepeatableRead, false),
        (IsolationLevel::Serializable, true),
        (IsolationLevel::Serializable, false),
    ] {
        let Some((test, db)) = fresh(server, Some(level)).await else {
            return;
        };
        let way = Way {
            read_first,
            ..BY_HAND
        };
        let race = one_free_place(&db, way, true).await;
        eprintln!("{server} at {level:?}, by hand, a read first {read_first}: {race:?}");
        assert_eq!(race.taken, 0, "{server} at {level:?}: {race:?}");
        assert_eq!(race.errors.len(), 20, "{server} at {level:?}: {race:?}");
        assert!(
            race.errors.iter().all(|e| e.contains("READ COMMITTED")),
            "{server} at {level:?}: {race:?}"
        );
        assert_eq!(race.held, 2, "nothing moved");
        done(test, db).await;
    }
    // At READ COMMITTED, by hand, with the lock first, it holds.
    let Some((test, db)) = fresh(server, Some(IsolationLevel::ReadCommitted)).await else {
        return;
    };
    let lock_first = Way {
        read_first: false,
        ..BY_HAND
    };
    assert_eq!(
        one_free_place(&db, lock_first, true).await,
        exactly_one(),
        "{server}"
    );
    // Outside a unit of work a lock would end with its statement: refused too.
    let outside = sortable::lock_lists(&db, &Lane::KEY, ["doing"])
        .await
        .expect_err("outside a unit of work");
    assert!(outside.to_string().contains("unit of work"), "{outside}");
    done(test, db).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sqlite_runs_moves_at_once_one_after_the_other() {
    moves_at_once("sqlite").await;
}

#[cfg(feature = "postgres")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn postgres_moves_at_once_take_turns_at_every_level() {
    moves_at_once("postgres").await;
}

#[cfg(feature = "postgres")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn postgres_moves_at_once_without_the_lock_break_the_limit_and_share_a_rank() {
    moves_at_once_without_the_lock("postgres").await;
}

#[cfg(feature = "postgres")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn postgres_refuses_a_lock_outside_read_committed() {
    a_lock_outside_read_committed_is_refused("postgres").await;
}

#[cfg(feature = "mysql")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mysql_moves_at_once_take_turns_at_every_level() {
    moves_at_once("mysql").await;
}

#[cfg(feature = "mysql")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mysql_moves_at_once_without_the_lock_break_the_limit_and_share_a_rank() {
    moves_at_once_without_the_lock("mysql").await;
}

#[cfg(feature = "mysql")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mysql_refuses_a_lock_outside_read_committed() {
    a_lock_outside_read_committed_is_refused("mysql").await;
}

/// The servers configured for this run, SQLite always.
fn servers() -> Vec<&'static str> {
    let mut found = vec!["sqlite"];
    if cfg!(feature = "postgres") {
        found.push("postgres");
    }
    if cfg!(feature = "mysql") {
        found.push("mysql");
    }
    found
}

/// A list with no row to lock is an error, not a silent pass: the move it names
/// would run with nothing holding it to the moves beside it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_list_with_no_row_is_not_locked_silently() {
    for server in servers() {
        let Some((test, db)) = fresh(server, None).await else {
            continue;
        };
        seed(&db, &[("todo", None, 2)]).await;
        let missing = Move::new("1", "todo", "nowhere")
            .transaction(&db, &Lane::KEY, |_uow| async { Ok(()) })
            .await
            .expect_err("no row for nowhere");
        assert!(missing.is_not_found(), "{server}: {missing}");
        assert!(
            missing.to_string().contains("nowhere"),
            "{server}: {missing}"
        );
        // The one that is there is locked, and the work runs.
        let ran = Move::new("1", "todo", "todo")
            .transaction(&db, &Lane::KEY, |_uow| async { Ok(7) })
            .await
            .expect("todo has its row");
        assert_eq!(ran, 7);
        done(test, db).await;
    }
}

/// The toolkit's transaction runs again when the database asks for it (a deadlock,
/// a serialization failure, a lock not free in time), at most three times; it
/// gives up on any other error at once; and it is READ COMMITTED whatever the
/// pool's own level.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_toolkits_transaction_runs_again_when_asked_and_is_read_committed() {
    for server in servers() {
        let level = (server != "sqlite").then_some(IsolationLevel::Serializable);
        let Some((test, db)) = fresh(server, level).await else {
            continue;
        };
        seed(&db, &[("todo", None, 2)]).await;
        let lists = vec!["todo"];
        for (kind, fails, runs, ok) in [
            (ErrorKind::SerializationFailure, 1, 2, true),
            (ErrorKind::Deadlock, 2, 3, true),
            (ErrorKind::LockUnavailable, 5, 3, false),
            (ErrorKind::UniqueViolation, 1, 1, false),
        ] {
            let attempts = AtomicUsize::new(0);
            let result = sortable::transaction(&db, &Lane::KEY, lists.clone(), |_uow| {
                let n = attempts.fetch_add(1, Ordering::SeqCst);
                async move {
                    if n < fails {
                        Err(OrmError::Db(DbError::new(kind, "asked to run again")))
                    } else {
                        Ok(n)
                    }
                }
            })
            .await;
            assert_eq!(attempts.load(Ordering::SeqCst), runs, "{server}: {kind:?}");
            assert_eq!(result.is_ok(), ok, "{server}: {kind:?}: {result:?}");
        }
        let seen = sortable::transaction(&db, &Lane::KEY, lists.clone(), |uow| async move {
            Ok(match server {
                "postgres" => scalar::<String, _>(
                    &uow,
                    raw("SELECT current_setting('transaction_isolation')", Vec::<i64>::new()),
                )
                .await?
                .unwrap_or_default(),
                "mysql" => scalar::<String, _>(
                    &uow,
                    raw(
                        "SELECT trx_isolation_level FROM information_schema.innodb_trx WHERE trx_mysql_thread_id = CONNECTION_ID()",
                        Vec::<i64>::new(),
                    ),
                )
                .await?
                .unwrap_or_default(),
                _ => "serializable".to_owned(),
            })
        })
        .await
        .expect("reads its own level");
        let want = match server {
            "postgres" => "read committed",
            "mysql" => "READ COMMITTED",
            _ => "serializable",
        };
        assert_eq!(seen, want, "{server}, the pool at {level:?}");
        done(test, db).await;
    }
}

/// Tied ranks, which only a writer that skips the lock leaves: a move dropped
/// between two tied tasks lands between them, the lane given fresh ranks first.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_move_between_tied_ranks_lands_between_them() {
    for server in servers() {
        for mv in [
            Move::new("5", "b", "a").before("3"),
            Move::new("5", "b", "a").after("2"),
        ] {
            let Some((test, db)) = fresh(server, None).await else {
                continue;
            };
            seed(&db, &[("a", None, 4), ("b", None, 1)]).await;
            for (id, rank) in [(1_i64, 0_i64), (2, 100), (3, 100), (4, 200)] {
                Task::query()
                    .where_(Task::ID.eq(id))
                    .update(&db, [Task::RANK.set(rank)])
                    .await
                    .unwrap();
            }
            let gate = Arc::new(Barrier::new(1));
            let label = format!("{mv:?}");
            placed(&db, mv, TOOLKIT, gate)
                .await
                .expect("no error")
                .expect("taken");
            let ids: Vec<i64> = order(&db, "a")
                .await
                .into_iter()
                .map(|(id, _)| id)
                .collect();
            assert_eq!(ids, [1, 2, 5, 3, 4], "{server}: {label}");
            done(test, db).await;
        }
    }
}

/// Fresh ranks for a 12,000-task lane, in one statement, against one `UPDATE` a
/// task: the best of three runs of each way, and the order kept.
async fn renewal(server: &str) {
    const TASKS: usize = 12_000;
    let Some((test, db)) = fresh(server, None).await else {
        return;
    };
    seed(&db, &[("long", None, TASKS), ("other", None, 3)]).await;
    // Ranks crowded together, some the same, so the order leans on the id too.
    let crowd = |db: Db| async move {
        let dialect = Dialect::from(db.backend());
        let q = |name: &str| dialect.quote_identifier(name);
        let sql = format!(
            "UPDATE {t} SET {r} = ({i} * 7) % 5000 WHERE {l} = 'long'",
            t = q("tasks"),
            r = q("rank"),
            i = q("id"),
            l = q("lane")
        );
        db.pool()
            .execute_script(&sql)
            .await
            .expect("crowds the ranks");
    };
    crowd(db.clone()).await;
    let expected: Vec<i64> = order(&db, "long")
        .await
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    let other = order(&db, "other").await;

    let mut one = Duration::MAX;
    let mut each = Duration::MAX;
    for _ in 0..3 {
        crowd(db.clone()).await;
        let start = Instant::now();
        let changed = db
            .transaction(|uow| async move {
                sortable::renew_ranks(&uow, &Task::LANE, &Task::RANK, &Task::ID, "long")
                    .await
                    .map_err(db_error)
            })
            .await
            .expect("renews");
        one = one.min(start.elapsed());
        // MySQL counts the rows whose value changed; a renewal changes every one.
        assert!(changed > 0, "{server}: {changed} rows changed");
        let now = order(&db, "long").await;
        assert_eq!(
            now.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            expected,
            "{server}: the order is kept"
        );
        assert!(
            now.iter()
                .enumerate()
                .all(|(at, (_, rank))| *rank == at as i64 * RANK_GAP),
            "{server}: fresh ranks, RANK_GAP apart"
        );
        assert_eq!(
            order(&db, "other").await,
            other,
            "{server}: other lanes are left alone"
        );

        crowd(db.clone()).await;
        let start = Instant::now();
        db.transaction(|uow| async move {
            let ids: Vec<i64> = Task::query()
                .where_(Task::LANE.eq("long"))
                .order_by("rank")
                .order_by("id")
                .get(&uow)
                .await
                .map_err(db_error)?
                .into_vec()
                .into_iter()
                .map(|r| r.into_inner().id)
                .collect();
            for (at, id) in ids.into_iter().enumerate() {
                Task::query()
                    .where_(Task::ID.eq(id))
                    .update(&uow, [Task::RANK.set(at as i64 * RANK_GAP)])
                    .await
                    .map_err(db_error)?;
            }
            Ok(())
        })
        .await
        .expect("renews a task at a time");
        each = each.min(start.elapsed());
    }
    eprintln!(
        "{server}: fresh ranks for {TASKS} tasks: one statement {:.1} ms, one UPDATE a task {:.1} ms (best of 3 each)",
        one.as_secs_f64() * 1e3,
        each.as_secs_f64() * 1e3
    );
    assert!(
        one * 3 < each,
        "{server}: one statement {one:?}, a task at a time {each:?}"
    );
    assert!(one < Duration::from_secs(2), "{server}: {one:?}");
    done(test, db).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sqlite_renews_a_long_lane_in_one_statement() {
    renewal("sqlite").await;
}

#[cfg(feature = "postgres")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn postgres_renews_a_long_lane_in_one_statement() {
    renewal("postgres").await;
}

#[cfg(feature = "mysql")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mysql_renews_a_long_lane_in_one_statement() {
    renewal("mysql").await;
}

/// A move whose neighbours have no room left renews the lane and lands where it
/// was dropped.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_tight_gap_renews_the_lane_and_the_move_lands() {
    for server in servers() {
        let Some((test, db)) = fresh(server, None).await else {
            continue;
        };
        seed(&db, &[("todo", None, 3)]).await;
        // 1 and 2 next to each other: nothing fits between them.
        Task::query()
            .where_(Task::ID.eq(2_i64))
            .update(&db, [Task::RANK.set(1_i64)])
            .await
            .unwrap();
        let gate = Arc::new(Barrier::new(1));
        let placement = placed(
            &db,
            Move::new("3", "todo", "todo").before("2"),
            TOOLKIT,
            gate,
        )
        .await
        .expect("no error")
        .expect("taken");
        assert!(placement.moved);
        let ids: Vec<i64> = order(&db, "todo")
            .await
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        assert_eq!(ids, [1, 3, 2], "{server}");
        done(test, db).await;
    }
}

/// A lane of a board that more than one organisation uses: the key names it, and `org`
/// says whose it is.
#[derive(Model, Clone, Debug, Default, serde::Serialize)]
#[model(table = "shared_lanes")]
struct SharedLane {
    id: i64,
    org: i64,
    key: String,
}

/// A card in a shared lane: the lane is named by its key, which two organisations
/// can both have.
#[derive(Model, Clone, Debug, Default, serde::Serialize)]
#[model(table = "shared_cards")]
struct SharedCard {
    id: i64,
    org: i64,
    lane: String,
    rank: i64,
}

async fn shared_schema(db: &Db) {
    let dialect = Dialect::from(db.backend());
    let q = |name: &str| dialect.quote_identifier(name);
    for sql in [
        format!(
            "CREATE TABLE {} ({} BIGINT PRIMARY KEY, {} BIGINT NOT NULL, {} VARCHAR(64) NOT NULL)",
            q("shared_lanes"),
            q("id"),
            q("org"),
            q("key")
        ),
        format!(
            "CREATE TABLE {} ({} BIGINT PRIMARY KEY, {} BIGINT NOT NULL, {} VARCHAR(64) NOT NULL, {} BIGINT NOT NULL)",
            q("shared_cards"),
            q("id"),
            q("org"),
            q("lane"),
            q("rank")
        ),
    ] {
        db.pool().execute_script(&sql).await.expect("schema");
    }
}

/// A database of its own with the shared tables; `None` when `server` isn't configured.
async fn fresh_shared(server: &str) -> Option<(TestDb, Db)> {
    let test = match server {
        "sqlite" => TestDb::sqlite().await.expect("sqlite"),
        "postgres" => TestDb::postgres().await.expect("postgres")?,
        _ => TestDb::mysql().await.expect("mysql")?,
    };
    let db = pool(test.url(), None).await;
    shared_schema(&db).await;
    Some((test, db))
}

fn shared_lane(id: i64, org: i64, key: &str) -> SharedLane {
    SharedLane {
        id,
        org,
        key: key.to_owned(),
    }
}

/// `(id, rank)` of the cards of `org`, by id.
async fn cards_of(db: &Db, org: i64) -> Vec<(i64, i64)> {
    SharedCard::query()
        .where_(SharedCard::ORG.eq(org))
        .order_by("id")
        .get(db)
        .await
        .expect("cards")
        .into_vec()
        .into_iter()
        .map(|record| {
            let card = record.into_inner();
            (card.id, card.rank)
        })
        .collect()
}

/// A list is found only among the rows of the caller's scope: the key of another
/// organisation's lane, posted by a browser, is a lane that isn't there, and is not
/// locked. Without a scope the key finds any row of the table.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_scoped_lock_finds_a_list_only_among_the_callers_rows() {
    for server in servers() {
        let Some((test, db)) = fresh_shared(server).await else {
            continue;
        };
        SharedLane::query()
            .insert(
                &db,
                vec![shared_lane(1, 1, "todo"), shared_lane(2, 2, "doing")],
            )
            .await
            .expect("lanes");
        let mine = ListScope::new().and(&SharedLane::ORG, 1_i64);
        let theirs = ListScope::new().and(&SharedLane::ORG, 2_i64);

        // Their lane, named by a caller of organisation 1.
        let missing = sortable::transaction_scoped(
            &db,
            &SharedLane::KEY,
            vec!["doing"],
            &mine,
            |_uow| async { Ok(()) },
        )
        .await
        .expect_err("not among organisation 1's rows");
        assert!(missing.is_not_found(), "{server}: {missing}");
        assert!(missing.to_string().contains("doing"), "{server}: {missing}");
        let missing = Move::new("1", "todo", "doing")
            .transaction_scoped(&db, &SharedLane::KEY, &mine, |_uow| async { Ok(()) })
            .await
            .expect_err("the move reaches for a lane that is not theirs");
        assert!(missing.is_not_found(), "{server}: {missing}");

        // Their own lanes lock, and the work runs.
        let ran = sortable::transaction_scoped(
            &db,
            &SharedLane::KEY,
            vec!["doing"],
            &theirs,
            |_uow| async { Ok(7) },
        )
        .await
        .expect("organisation 2's lane");
        assert_eq!(ran, 7, "{server}");
        let ran = Move::new("1", "todo", "todo")
            .transaction_scoped(&db, &SharedLane::KEY, &mine, |_uow| async { Ok(8) })
            .await
            .expect("organisation 1's lane");
        assert_eq!(ran, 8, "{server}");

        // Without a scope the key finds the row of another organisation, as it did.
        let ran =
            sortable::transaction(&db, &SharedLane::KEY, vec!["doing"], |_uow| async { Ok(9) })
                .await
                .expect("any row of the table");
        assert_eq!(ran, 9, "{server}");

        // A scope has to be of the lanes' own table.
        let wrong = ListScope::new().and(&Task::ID, 1_i64);
        let refused = sortable::transaction_scoped(
            &db,
            &SharedLane::KEY,
            vec!["todo"],
            &wrong,
            |_uow| async { Ok(()) },
        )
        .await
        .expect_err("a column of another table");
        assert!(refused.to_string().contains("tasks"), "{server}: {refused}");
        done(test, db).await;
    }
}

/// Where two organisations have a list of the same name, a renewal for one leaves the
/// other's ranks as they are. Without a scope it numbers both organisations' rows
/// together by the one name.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_scoped_renewal_leaves_the_ranks_of_the_same_named_list_of_another_organisation() {
    for server in servers() {
        let Some((test, db)) = fresh_shared(server).await else {
            continue;
        };
        let card = |id: i64, org: i64, rank: i64| SharedCard {
            id,
            org,
            lane: "todo".to_owned(),
            rank,
        };
        SharedCard::query()
            .insert(
                &db,
                vec![
                    card(1, 1, 5),
                    card(2, 1, 5),
                    card(3, 1, 5),
                    card(11, 2, 7),
                    card(12, 2, 7),
                    card(13, 2, 7),
                ],
            )
            .await
            .expect("cards");
        let mine = ListScope::new().and(&SharedCard::ORG, 1_i64);
        let changed = db
            .transaction(|uow| {
                let mine = mine.clone();
                async move {
                    sortable::renew_ranks_scoped(
                        &uow,
                        &SharedCard::LANE,
                        &SharedCard::RANK,
                        &SharedCard::ID,
                        "todo",
                        &mine,
                    )
                    .await
                    .map_err(db_error)
                }
            })
            .await
            .expect("renews");
        assert!(changed > 0, "{server}: {changed} rows changed");
        assert_eq!(
            cards_of(&db, 1).await,
            [(1, 0), (2, RANK_GAP), (3, 2 * RANK_GAP)],
            "{server}: organisation 1's list has fresh ranks"
        );
        assert_eq!(
            cards_of(&db, 2).await,
            [(11, 7), (12, 7), (13, 7)],
            "{server}: organisation 2's list is as it was"
        );

        // The scope's own value is bound, not written into the statement.
        let nobody = ListScope::new().and(&SharedCard::ORG, 99_i64);
        let changed = db
            .transaction(|uow| {
                let nobody = nobody.clone();
                async move {
                    sortable::renew_ranks_scoped(
                        &uow,
                        &SharedCard::LANE,
                        &SharedCard::RANK,
                        &SharedCard::ID,
                        "todo",
                        &nobody,
                    )
                    .await
                    .map_err(db_error)
                }
            })
            .await
            .expect("renews nothing");
        assert_eq!(changed, 0, "{server}: no row is of organisation 99");

        // Without a scope the one name is every organisation's rows.
        db.transaction(|uow| async move {
            sortable::renew_ranks(
                &uow,
                &SharedCard::LANE,
                &SharedCard::RANK,
                &SharedCard::ID,
                "todo",
            )
            .await
            .map_err(db_error)
        })
        .await
        .expect("renews");
        assert_ne!(
            cards_of(&db, 2).await,
            [(11, 7), (12, 7), (13, 7)],
            "{server}: the unscoped form rewrites the rows of both"
        );
        done(test, db).await;
    }
}
