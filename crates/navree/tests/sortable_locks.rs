//! Moves at the same moment, on PostgreSQL, MySQL and SQLite: `Move::place` and
//! `Move::check` with the lock the toolkit takes on the lists a move touches
//! (`navree::ui::sortable::lock_lists`), and a whole list's fresh ranks in one
//! statement (`renew_ranks`). Each race runs with the lock and without it: without
//! it, every move waits between its reads and its write until all have read, which
//! is what two moves at once do when the timing goes against them.
//!
//! The SQLite tests always run. The PostgreSQL and MySQL ones run against real
//! servers: build with their features and set `NAVREE_TEST_POSTGRES_URL` and
//! `NAVREE_TEST_MYSQL_URL` (or have `target/test-databases.env`). Each test works
//! in a database of its own.
//!
//! ```text
//! cargo test -p navree --features sqlite,postgres,mysql --test sortable_locks -- --nocapture
//! ```
#![cfg(feature = "sqlite")]

use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use navree::db::{Db, DbError, DbOptions, ErrorKind, Executor as _, MySqlOptions, Uow};
use navree::orm::testing::database::TestDb;
use navree::orm::{Dialect, Model as _, OrmError};
use navree::prelude::*;
use navree::ui::sortable::{self, Placement, RANK_GAP, RankedLists, Spot};
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
            Task::query().insert(db, chunk.to_vec()).await.expect("tasks");
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
        rank: i64,
        lower: bool,
        item: &str,
    ) -> impl Future<Output = Result<Option<i64>, OrmError>> + Send {
        let (uow, list, item) = (self.uow, list.to_owned(), id_of(item));
        async move {
            let query = Task::query()
                .where_(Task::LANE.eq(list))
                .where_(Task::ID.ne(item));
            let found = if lower {
                query
                    .where_(Task::RANK.lt(rank))
                    .order_by_desc("rank")
                    .order_by_desc("id")
            } else {
                query
                    .where_(Task::RANK.gt(rank))
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
    /// The lock is taken.
    locking: bool,
    /// Every move waits, once it has read what it needs and before it writes,
    /// until all the race's moves have read: the worst timing for moves without a
    /// lock. (With the lock the moves run one after another, so nothing waits.)
    hold: bool,
    /// Each transaction reads something before it moves, as a handler reading
    /// its user first does; every move waits after that read until all have made
    /// it.
    read_first: bool,
}

const LOCKED: Way = Way {
    locking: true,
    hold: false,
    read_first: false,
};

#[cfg(any(feature = "postgres", feature = "mysql"))]
const UNLOCKED: Way = Way {
    locking: false,
    hold: true,
    read_first: false,
};

/// Places `mv` by rank and writes it, in a transaction of its own, renewing the
/// lane's ranks once when its new neighbours have no room between them.
async fn placed(db: &Db, mv: Move, way: Way, gate: Arc<Barrier>) -> Result<Placement, Refusal> {
    db.transaction(|uow| async move {
        if way.read_first {
            Lane::query().get(&uow).await.map_err(db_error)?;
            gate.wait().await;
        }
        let board = Board {
            uow: &uow,
            locking: way.locking,
        };
        let mut placed = mv.place(&board).await.map_err(db_error)?;
        if let Ok(Placement {
            rank: None,
            to_list,
            ..
        }) = &placed
        {
            sortable::renew_ranks(&uow, &Task::LANE, &Task::RANK, &Task::ID, to_list.clone())
                .await
                .map_err(db_error)?;
            placed = mv.place(&board).await.map_err(db_error)?;
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
                    &uow,
                    [
                        Task::LANE.set(placement.to_list.clone()),
                        Task::RANK.set(rank),
                    ],
                )
                .await
                .map_err(db_error)?;
        }
        Ok(placed)
    })
    .await
    .expect("the move's transaction")
}

/// Checks `mv` against the lanes read whole and writes their new order, in a
/// transaction of its own: the way for lists short enough to read.
async fn checked(db: &Db, mv: Move, way: Way, gate: Arc<Barrier>) -> Result<Landing, Refusal> {
    db.transaction(|uow| async move {
        if way.locking {
            mv.lock(&uow, &Lane::KEY).await.map_err(db_error)?;
        }
        let lanes = Lane::query().order_by("key").get(&uow).await.map_err(db_error)?;
        let mut lists = Vec::new();
        for lane in lanes.into_vec() {
            let lane = lane.into_inner();
            let keys: Vec<String> = Task::query()
                .where_(Task::LANE.eq(lane.key.clone()))
                .order_by("rank")
                .order_by("id")
                .get(&uow)
                .await
                .map_err(db_error)?
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
                        &uow,
                        [
                            Task::LANE.set(landing.to_list.clone()),
                            Task::RANK.set(at as i64 * RANK_GAP),
                        ],
                    )
                    .await
                    .map_err(db_error)?;
            }
        }
        Ok(landing)
    })
    .await
    .expect("the move's transaction")
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

/// Twenty tasks of the backlog race for the one free place in a lane that holds
/// at most three and has two: how many moves were accepted, and how many tasks the
/// lane holds after.
async fn one_free_place(db: &Db, way: Way, by_rank: bool) -> (usize, usize) {
    let ids = seed(db, &[("backlog", None, 20), ("doing", Some(3), 2)]).await;
    let moves: Vec<Move> = ids[0]
        .iter()
        .map(|id| Move::new(id.to_string(), "backlog", "doing"))
        .collect();
    let accepted = if by_rank {
        let out = race(db, moves, way, |db, mv, way, gate| async move {
            placed(&db, mv, way, gate).await
        })
        .await;
        for refused in out.iter().filter_map(|r| r.as_ref().err()) {
            assert!(matches!(refused, Refusal::Full { limit: 3, .. }), "{refused:?}");
        }
        out.iter().filter(|r| r.is_ok()).count()
    } else {
        let out = race(db, moves, way, |db, mv, way, gate| async move {
            checked(&db, mv, way, gate).await
        })
        .await;
        for refused in out.iter().filter_map(|r| r.as_ref().err()) {
            assert!(matches!(refused, Refusal::Full { limit: 3, .. }), "{refused:?}");
        }
        out.iter().filter(|r| r.is_ok()).count()
    };
    (accepted, order(db, "doing").await.len())
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
        .map(|r| r.as_ref().expect("both moves are taken").rank.expect("a rank"))
        .collect();
    (ranks[0], ranks[1], order(db, "todo").await)
}

/// A pool on the test database at `url`, with room for every move of a race at
/// once, at MySQL's `isolation` when given.
async fn pool(url: &str, isolation: Option<navree::db::IsolationLevel>) -> Db {
    Db::connect(&DbOptions {
        url: url.to_owned(),
        max_connections: 32,
        mysql: MySqlOptions {
            isolation_level: isolation,
            ..MySqlOptions::default()
        },
        ..DbOptions::default()
    })
    .await
    .expect("connects")
}

/// A database of its own on `server`'s test server, its tables made, and a pool
/// on it; `None` when the server isn't configured.
async fn fresh(server: &str, isolation: Option<navree::db::IsolationLevel>) -> Option<(TestDb, Db)> {
    let test = match server {
        "sqlite" => TestDb::sqlite().await.expect("sqlite"),
        "postgres" => TestDb::postgres().await.expect("postgres")?,
        _ => TestDb::mysql().await.expect("mysql")?,
    };
    let db = pool(test.url(), isolation).await;
    schema(&db).await;
    Some((test, db))
}

async fn done(test: TestDb, db: Db) {
    db.close().await.expect("closes");
    test.close().await.expect("drops the database");
}

/// The lock holds every database to the limit and to distinct ranks; on PostgreSQL
/// and MySQL, the same moves without it break both.
async fn moves_at_once(server: &str) {
    let Some((test, db)) = fresh(server, None).await else {
        return;
    };
    assert_eq!(one_free_place(&db, LOCKED, true).await, (1, 3), "{server}: by rank, locked");
    done(test, db).await;

    let Some((test, db)) = fresh(server, None).await else {
        return;
    };
    assert_eq!(one_free_place(&db, LOCKED, false).await, (1, 3), "{server}: read whole, locked");
    done(test, db).await;

    let Some((test, db)) = fresh(server, None).await else {
        return;
    };
    let (a, b, todo) = one_gap(&db, LOCKED).await;
    assert_ne!(a, b, "{server}: two moves into one gap get ranks of their own");
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
    let (accepted, held) = one_free_place(&db, UNLOCKED, true).await;
    assert!(accepted > 1 && held > 3, "{server}: by rank, unlocked: {accepted} taken, {held} held");
    eprintln!("{server}: without the lock, {accepted} of 20 moves took the one free place; the lane holds {held}, at most 3");
    done(test, db).await;

    let Some((test, db)) = fresh(server, None).await else {
        return;
    };
    let (accepted, held) = one_free_place(&db, UNLOCKED, false).await;
    assert!(accepted > 1 && held > 3, "{server}: read whole, unlocked: {accepted} taken, {held} held");
    done(test, db).await;

    let Some((test, db)) = fresh(server, None).await else {
        return;
    };
    let (a, b, _) = one_gap(&db, UNLOCKED).await;
    assert_eq!(a, b, "{server}: without the lock both moves take the same rank");
    eprintln!("{server}: without the lock, two moves into one gap both took rank {a}");
    done(test, db).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sqlite_runs_moves_at_once_one_after_the_other() {
    moves_at_once("sqlite").await;
}

#[cfg(feature = "postgres")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn postgres_moves_at_once_take_turns_under_the_lock() {
    moves_at_once("postgres").await;
}

#[cfg(feature = "postgres")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn postgres_moves_at_once_without_the_lock_break_the_limit_and_share_a_rank() {
    moves_at_once_without_the_lock("postgres").await;
}

#[cfg(feature = "mysql")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mysql_moves_at_once_take_turns_under_the_lock() {
    moves_at_once("mysql").await;
}

#[cfg(feature = "mysql")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mysql_moves_at_once_without_the_lock_break_the_limit_and_share_a_rank() {
    moves_at_once_without_the_lock("mysql").await;
}

/// On MySQL a transaction keeps reading what was there at its first read
/// (`REPEATABLE READ`): one that read before it took the lock is checked against
/// that, and the lock can't help. At `READ COMMITTED`, and on PostgreSQL (whose
/// default it is), every read sees the moves that went first.
#[cfg(feature = "mysql")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mysql_moves_that_read_before_the_lock_need_read_committed() {
    use navree::db::IsolationLevel;
    let late = Way {
        locking: true,
        hold: false,
        read_first: true,
    };
    let Some((test, db)) = fresh("mysql", None).await else {
        return;
    };
    let (accepted, held) = one_free_place(&db, late, true).await;
    assert!(accepted > 1, "REPEATABLE READ: {accepted} taken, {held} held");
    eprintln!("mysql, REPEATABLE READ, a read before the lock: {accepted} of 20 taken");
    done(test, db).await;
    let Some((test, db)) = fresh("mysql", Some(IsolationLevel::ReadCommitted)).await else {
        return;
    };
    assert_eq!(one_free_place(&db, late, true).await, (1, 3), "READ COMMITTED");
    done(test, db).await;
}

#[cfg(feature = "postgres")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn postgres_moves_that_read_before_the_lock_still_take_turns() {
    let late = Way {
        locking: true,
        hold: false,
        read_first: true,
    };
    let Some((test, db)) = fresh("postgres", None).await else {
        return;
    };
    assert_eq!(one_free_place(&db, late, true).await, (1, 3));
    done(test, db).await;
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
        db.pool().execute_script(&sql).await.expect("crowds the ranks");
    };
    crowd(db.clone()).await;
    let expected: Vec<i64> = order(&db, "long").await.into_iter().map(|(id, _)| id).collect();
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
        assert_eq!(now.iter().map(|(id, _)| *id).collect::<Vec<_>>(), expected, "{server}: the order is kept");
        assert!(
            now.iter().enumerate().all(|(at, (_, rank))| *rank == at as i64 * RANK_GAP),
            "{server}: fresh ranks, RANK_GAP apart"
        );
        assert_eq!(order(&db, "other").await, other, "{server}: other lanes are left alone");

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
    assert!(one * 3 < each, "{server}: one statement {one:?}, a task at a time {each:?}");
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
/// was dropped; the lock is a no-op where the database serialises already, and a
/// column with no table is an error, not a silent pass.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_tight_gap_renews_the_lane_and_the_move_lands() {
    for server in ["sqlite", "postgres", "mysql"] {
        if (server == "postgres" && !cfg!(feature = "postgres"))
            || (server == "mysql" && !cfg!(feature = "mysql"))
        {
            continue;
        }
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
        let placement = placed(&db, Move::new("3", "todo", "todo").before("2"), LOCKED, gate)
            .await
            .expect("taken");
        assert!(placement.moved);
        let ids: Vec<i64> = order(&db, "todo").await.into_iter().map(|(id, _)| id).collect();
        assert_eq!(ids, [1, 3, 2], "{server}");
        done(test, db).await;
    }
}
