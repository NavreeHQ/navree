//! The lifecycle on real servers: events after the commit, soft deletes,
//! restores and force deletes (one by one and in bulk), quiet writes,
//! stepping several columns, and pruning.

use std::sync::Mutex;

use chrono::{DateTime, Duration, TimeZone, Utc};
use navree_db::Db;
use navree_db::Executor as _;
use navree_orm::hooks::{Flow, ModelEvent};
use navree_orm::prune::{MassPrunable, Prunable};
use navree_orm::{
    Builder, ExecContext, Model, ModelLifecycle, Observer, Record, Result, without_events,
};
use serde::Serialize;
use uuid::Uuid;

use crate::support::Server;

/// The tables of this scenario.
fn schema(server: Server) -> &'static [&'static str] {
    match server {
        Server::Postgres => &[
            "CREATE TABLE memos (id UUID PRIMARY KEY, body TEXT NOT NULL, hits BIGINT NOT NULL DEFAULT 0, likes BIGINT NOT NULL DEFAULT 0, created_at TIMESTAMPTZ NOT NULL, updated_at TIMESTAMPTZ NOT NULL, deleted_at TIMESTAMPTZ NULL)",
            "CREATE TABLE beats (id BIGSERIAL PRIMARY KEY, recorded_at TIMESTAMPTZ NOT NULL)",
        ],
        Server::MySql => &[
            "CREATE TABLE memos (id BINARY(16) PRIMARY KEY, body VARCHAR(255) NOT NULL, hits BIGINT NOT NULL DEFAULT 0, likes BIGINT NOT NULL DEFAULT 0, created_at DATETIME(6) NOT NULL, updated_at DATETIME(6) NOT NULL, deleted_at DATETIME(6) NULL) ENGINE = InnoDB",
            "CREATE TABLE beats (id BIGINT AUTO_INCREMENT PRIMARY KEY, recorded_at DATETIME(6) NOT NULL) ENGINE = InnoDB",
        ],
    }
}

/// What the memo observer heard, across the scenario's servers (each
/// server's memos carry its name).
static HEARD: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn heard(server: Server) -> Vec<String> {
    let tag = format!("{server:?}");
    let mut heard = HEARD.lock().unwrap();
    let (mine, others): (Vec<String>, Vec<String>) =
        heard.drain(..).partition(|line| line.contains(&tag));
    *heard = others;
    mine
}

fn cutoff() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap()
}

/// A memo: UUID-keyed, soft-deleting, observed, prunable.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(timestamps, soft_deletes, prunable, observers(MemoObserver))]
struct Memo {
    id: Uuid,
    body: String,
    hits: i64,
    likes: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
}

impl Prunable for Memo {
    fn prunable(query: Builder<Self>) -> Builder<Self> {
        query.where_(Memo::CREATED_AT.lt(cutoff()))
    }
}

/// Notes the write events of memos, and whether their unit of work had
/// committed.
struct MemoObserver;

async fn note(event: &str, memo: &Record<Memo>, ctx: &ExecContext) {
    let committed = match ctx.uow() {
        Some(uow) if uow.is_finished().await => " (committed)",
        _ => "",
    };
    HEARD
        .lock()
        .unwrap()
        .push(format!("{event} {}{committed}", memo.body));
}

impl Observer<Memo> for MemoObserver {
    async fn created(&self, memo: &Record<Memo>, ctx: &ExecContext) -> Result<()> {
        note("created", memo, ctx).await;
        Ok(())
    }

    async fn trashed(&self, memo: &Record<Memo>, ctx: &ExecContext) -> Result<()> {
        note("trashed", memo, ctx).await;
        Ok(())
    }

    async fn restored(&self, memo: &Record<Memo>, ctx: &ExecContext) -> Result<()> {
        note("restored", memo, ctx).await;
        Ok(())
    }

    async fn force_deleted(&self, memo: &Record<Memo>, ctx: &ExecContext) -> Result<()> {
        note("force_deleted", memo, ctx).await;
        Ok(())
    }

    async fn restoring(&self, memo: &mut Record<Memo>, _ctx: &ExecContext) -> Result<Flow> {
        Ok(if memo.body.contains("stay gone") {
            Flow::Cancel
        } else {
            Flow::Continue
        })
    }
}

/// A heartbeat, pruned a chunk per statement.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(mass_prunable)]
struct Beat {
    id: i64,
    recorded_at: DateTime<Utc>,
}

impl MassPrunable for Beat {
    fn prunable(query: Builder<Self>) -> Builder<Self> {
        query.where_(Beat::RECORDED_AT.lt(cutoff()))
    }
}

fn memo(server: Server, body: &str) -> Memo {
    Memo {
        body: format!("{body} [{server:?}]"),
        ..Memo::default()
    }
}

/// The lifecycle on `server`.
pub async fn scenario(db: Db, server: Server) {
    for statement in schema(server) {
        db.pool()
            .execute_script(statement)
            .await
            .unwrap_or_else(|error| panic!("{statement}: {error}"));
    }
    heard(server);

    // "-ed" events after the commit; nothing for a rollback.
    let uow = db.uow(None, None).await;
    let mut kept = Memo::create(&uow, memo(server, "kept")).await.unwrap();
    assert!(heard(server).is_empty());
    uow.commit().await.unwrap();
    assert_eq!(
        heard(server),
        [format!("created kept [{server:?}] (committed)")]
    );
    let uow = db.uow(None, None).await;
    Memo::create(&uow, memo(server, "ghost")).await.unwrap();
    uow.rollback().await.unwrap();
    assert!(heard(server).is_empty());
    assert_eq!(Memo::query().count(&db).await.unwrap(), 1);

    // Trash, restore (cancellable), force delete.
    kept.delete(&db).await.unwrap();
    assert_eq!(Memo::query().count(&db).await.unwrap(), 0);
    assert_eq!(Memo::query().only_trashed().count(&db).await.unwrap(), 1);
    assert!(kept.restore(&db).await.unwrap());
    assert!(Memo::find(&db, kept.id).await.unwrap().is_some());
    let mut gone = Memo::create(&db, memo(server, "stay gone")).await.unwrap();
    gone.delete(&db).await.unwrap();
    assert!(!gone.restore(&db).await.unwrap(), "the observer refused");
    assert!(gone.force_delete(&db).await.unwrap());
    assert_eq!(
        heard(server),
        [
            format!("trashed kept [{server:?}]"),
            format!("restored kept [{server:?}]"),
            format!("created stay gone [{server:?}]"),
            format!("trashed stay gone [{server:?}]"),
            format!("force_deleted stay gone [{server:?}]"),
        ]
    );
    assert!(
        Memo::query()
            .with_trashed()
            .find(&db, gone.id)
            .await
            .unwrap()
            .is_none()
    );

    // In bulk: restore trashed matches, force delete with a limit (a keyed
    // subquery MySQL accepts).
    for n in 0..4 {
        let mut bulk = Memo::create_quietly(&db, memo(server, &format!("bulk {n}")))
            .await
            .unwrap();
        bulk.delete_quietly(&db).await.unwrap();
    }
    assert_eq!(
        Memo::query()
            .where_(Memo::BODY.like("bulk 0%"))
            .restore(&db)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        Memo::query()
            .only_trashed()
            .latest()
            .limit(2)
            .force_delete(&db)
            .await
            .unwrap(),
        2
    );
    assert_eq!(Memo::query().only_trashed().count(&db).await.unwrap(), 1);
    assert_eq!(Memo::query().count(&db).await.unwrap(), 2);
    assert!(heard(server).is_empty(), "bulk writes fire no events");

    // Several columns in one statement, quietly.
    kept.increment_each(&db, [(Memo::HITS, 3), (Memo::LIKES, 1)])
        .await
        .unwrap();
    without_events(kept.decrement_each(&db, [(Memo::HITS, 1)]))
        .await
        .unwrap();
    let fresh = kept.fresh(&db).await.unwrap().unwrap();
    assert_eq!((fresh.hits, fresh.likes), (2, 1));

    // Pruning: record by record (trashed rows included), then in chunks.
    for n in 0..3 {
        let mut old = Memo::create(
            &db,
            Memo {
                created_at: cutoff() - Duration::days(1 + n),
                ..memo(server, &format!("old {n}"))
            },
        )
        .await
        .unwrap();
        if n == 1 {
            old.delete(&db).await.unwrap();
        }
    }
    heard(server);
    assert_eq!(Memo::prune_all(&db, 2).await.unwrap(), 3);
    assert_eq!(heard(server).len(), 3, "a force_deleted per record");
    for n in 0..5 {
        Beat::create(
            &db,
            Beat {
                recorded_at: cutoff() - Duration::hours(n + 1),
                ..Beat::default()
            },
        )
        .await
        .unwrap();
    }
    Beat::create(
        &db,
        Beat {
            recorded_at: cutoff() + Duration::hours(1),
            ..Beat::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(Beat::prune_all(&db, 2).await.unwrap(), 5);
    assert_eq!(Beat::query().count(&db).await.unwrap(), 1);
    assert_eq!(ModelEvent::Created.name(), "created");
}
