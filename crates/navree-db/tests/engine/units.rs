//! Units of work on navree-db's transactions, the settings they send,
//! change observers, and where a `Db` runs a statement.

use std::sync::{Arc, Mutex};

use navree_db::{
    Backend, ChangeEvent, ChangeObserver, ChangeOp, Db, DbError, Executor, ExecutorExt, QueryLog,
    Statement, Uow,
};
use navree_kernel::BoxFuture;
use uuid::Uuid;

use crate::support::{Scratch, script, serial};

const ACTOR: Uuid = Uuid::from_u128(0x0199_a7a0_0000_7000_8000_0000_0000_0007);

async fn tables(db: &impl Executor, backend: Backend) {
    script(
        db,
        &format!(
            "CREATE TABLE deals (id {serial}, title VARCHAR(100) NOT NULL);
             CREATE TABLE audit (id {serial}, entity VARCHAR(50) NOT NULL, op VARCHAR(10) NOT NULL, actor VARCHAR(40) NULL)",
            serial = serial(backend)
        ),
    )
    .await;
}

fn add_deal(title: &str) -> Statement {
    Statement::portable("INSERT INTO deals (title) VALUES (?)", [title.into()])
}

async fn titles(db: &impl Executor) -> Vec<String> {
    db.fetch_column(&Statement::text("SELECT title FROM deals ORDER BY title"))
        .await
        .unwrap()
}

/// A unit of work: lazy, the actor and request id set for SQL, commit and
/// rollback, after-commit hooks.
pub async fn units_of_work(scratch: Scratch) {
    let db = scratch.db().await;
    let backend = scratch.server.backend();
    tables(&db, backend).await;

    let uow = db.uow(Some(ACTOR), Some("req-7".to_owned())).await;
    assert!(!uow.has_open_transaction().await);
    uow.execute(&add_deal("Renewal")).await.unwrap();
    assert!(uow.has_open_transaction().await);
    assert!(uow.has_modified_records());
    match backend {
        Backend::Postgres => {
            let row = uow
                .fetch_one(&Statement::text(
                    "SELECT current_setting('navree.actor', true) AS actor, current_setting('navree.request_id', true) AS request",
                ))
                .await
                .unwrap();
            assert_eq!(row.get::<String>("actor").unwrap(), ACTOR.to_string());
            assert_eq!(row.get::<String>("request").unwrap(), "req-7");
        }
        Backend::MySql => {
            let row = uow
                .fetch_one(&Statement::text(
                    "SELECT @navree_actor AS actor, @navree_request_id AS request",
                ))
                .await
                .unwrap();
            assert_eq!(row.get::<String>("actor").unwrap(), ACTOR.to_string());
            assert_eq!(row.get::<String>("request").unwrap(), "req-7");
        }
        Backend::Sqlite => {}
    }
    let hooks = Arc::new(Mutex::new(Vec::new()));
    let seen = hooks.clone();
    uow.after_commit(move || {
        Box::pin(async move {
            seen.lock().unwrap().push("after commit");
        })
    });
    if backend != Backend::Sqlite {
        assert!(
            titles(&db.pool().clone()).await.is_empty(),
            "not committed yet"
        );
    }
    uow.commit().await.unwrap();
    assert_eq!(*hooks.lock().unwrap(), ["after commit"]);
    assert_eq!(titles(&db).await, ["Renewal"]);
    let finished = uow.execute(&add_deal("late")).await.unwrap_err();
    assert_eq!(finished.kind(), navree_db::ErrorKind::TransactionDone);

    // The MySQL session variables are cleared before the connection goes
    // back to the pool.
    if backend == Backend::MySql {
        let single = Db::connect(&navree_db::DbOptions {
            max_connections: 1,
            ..scratch.options.clone()
        })
        .await
        .unwrap();
        let uow = single.uow(Some(ACTOR), None).await;
        uow.execute(&add_deal("Upsell")).await.unwrap();
        uow.commit().await.unwrap();
        let actor: Option<String> = single
            .fetch_scalar(&Statement::text("SELECT @navree_actor"))
            .await
            .unwrap();
        assert_eq!(actor, None);
    }

    // Rollback discards the work and the hooks.
    let uow = db.uow(None, None).await;
    uow.execute(&add_deal("Discarded")).await.unwrap();
    let ran = Arc::new(Mutex::new(false));
    let flag = ran.clone();
    uow.after_commit(move || Box::pin(async move { *flag.lock().unwrap() = true }));
    uow.rollback().await.unwrap();
    assert!(!*ran.lock().unwrap());
    assert!(!titles(&db).await.contains(&"Discarded".to_owned()));
}

/// `database.transaction_settings`: on by default, a unit of work sets the
/// actor and request id right after `BEGIN` (PostgreSQL, MySQL; MySQL also
/// clears them before the transaction ends); turned off, its transaction
/// is its own statements alone, one round trip fewer.
pub async fn transaction_settings_follow_the_option(scratch: Scratch) {
    assert!(navree_db::DbOptions::default().transaction_settings);
    let options: navree_db::DbOptions =
        serde_json::from_value(serde_json::json!({ "transaction_settings": false }))
            .expect("valid section");
    assert!(!options.transaction_settings);

    let backend = scratch.server.backend();
    let db = scratch.db().await;
    assert!(db.transaction_settings());
    tables(&db, backend).await;
    let settings = |sql: &str| {
        sql.contains("set_config('navree.actor'") || sql.starts_with("SET @navree_actor")
    };

    for enabled in [true, false] {
        let db = db.with_transaction_settings(enabled);
        assert_eq!(db.transaction_settings(), enabled);
        let ((), sent) = QueryLog::capture(async {
            let uow = db.uow(Some(ACTOR), Some("req-1".to_owned())).await;
            uow.execute(&Statement::text("DELETE FROM deals"))
                .await
                .unwrap();
            if backend == Backend::Postgres {
                let actor: Option<String> = uow
                    .fetch_scalar(&Statement::text(
                        "SELECT NULLIF(current_setting('navree.actor', true), '')",
                    ))
                    .await
                    .unwrap();
                assert_eq!(actor.is_some(), enabled);
            }
            if backend == Backend::MySql {
                let actor: Option<String> = uow
                    .fetch_scalar(&Statement::text("SELECT @navree_actor"))
                    .await
                    .unwrap();
                assert_eq!(actor.is_some(), enabled);
            }
            uow.commit().await.unwrap();
        })
        .await;
        let sql: Vec<&str> = sent.iter().map(|query| query.sql.as_str()).collect();
        let expected = match backend {
            Backend::Sqlite => 0,
            Backend::Postgres if enabled => 1,
            Backend::MySql if enabled => 2,
            _ => 0,
        };
        assert_eq!(
            sql.iter().filter(|sql| settings(sql)).count(),
            expected,
            "settings {enabled} on {backend:?}: {sql:?}"
        );
        assert!(
            sql.iter().any(|sql| sql.contains("DELETE FROM deals")),
            "{sql:?}"
        );
    }
}

/// Savepoints and transaction-local settings inside a unit of work.
pub async fn savepoints_and_settings_in_units_of_work(scratch: Scratch) {
    let db = scratch.db().await;
    let backend = scratch.server.backend();
    tables(&db, backend).await;

    let uow = db.uow(None, None).await;
    uow.execute(&add_deal("kept")).await.unwrap();
    let attempt = uow.savepoint().await.unwrap();
    attempt.execute(&add_deal("tried")).await.unwrap();
    attempt.rollback().await.unwrap();
    let applied = uow.set_local("navree.workspace", "agra").await.unwrap();
    assert_eq!(applied, backend != Backend::Sqlite);
    match backend {
        Backend::Postgres => {
            let workspace: String = uow
                .fetch_scalar(&Statement::text(
                    "SELECT current_setting('navree.workspace', true)",
                ))
                .await
                .unwrap();
            assert_eq!(workspace, "agra");
        }
        Backend::MySql => {
            let workspace: String = uow
                .fetch_scalar(&Statement::text("SELECT @navree_workspace"))
                .await
                .unwrap();
            assert_eq!(workspace, "agra");
        }
        Backend::Sqlite => {}
    }
    assert!(uow.set_local("bad name; DROP", "x").await.is_err());
    uow.commit().await.unwrap();
    assert_eq!(titles(&db).await, ["kept"]);
}

/// The changes the auditor saw.
#[derive(Default)]
struct Seen(Mutex<Vec<ChangeEvent>>);

/// An observer writing an audit row through the unit of work it is handed;
/// it refuses a change to `refused`.
struct Auditor {
    seen: Arc<Seen>,
}

impl navree_kernel::Injectable for Auditor {
    fn dependencies() -> Vec<navree_kernel::ProviderKey> {
        vec![navree_kernel::ProviderKey::of::<Seen>()]
    }

    fn construct(
        r: navree_kernel::Resolver,
    ) -> BoxFuture<'static, Result<Self, navree_kernel::KernelError>> {
        Box::pin(async move {
            Ok(Auditor {
                seen: r.get::<Seen>().await?,
            })
        })
    }
}

impl ChangeObserver for Auditor {
    fn observe<'a>(
        &'a self,
        uow: &'a Uow,
        changes: &'a [ChangeEvent],
    ) -> BoxFuture<'a, Result<(), DbError>> {
        Box::pin(async move {
            for change in changes {
                self.seen.0.lock().unwrap().push(change.clone());
                uow.execute(&Statement::portable(
                    "INSERT INTO audit (entity, op, actor) VALUES (?, ?, ?)",
                    [
                        change.entity.clone().into(),
                        change.op.as_str().into(),
                        uow.actor().map(|actor| actor.to_string()).into(),
                    ],
                ))
                .await?;
                if change.entity == "refused" {
                    return Err(DbError::invalid("refused"));
                }
            }
            Ok(())
        })
    }
}

fn change(entity: &str) -> ChangeEvent {
    ChangeEvent {
        entity: entity.into(),
        id: "1".into(),
        op: ChangeOp::Created,
        old: None,
        new: Some(serde_json::json!({"title": "x"})),
    }
}

/// An app with an observer: it runs inside the transaction at commit; a
/// failing observer rolls everything back.
pub async fn observers_run_inside_the_commit(scratch: Scratch) {
    let backend = scratch.server.backend();
    let app = navree_kernel::Navree::app()
        .module(navree_db::DatabaseModule::for_root(scratch.options.clone()))
        .module(ObserverModule)
        .boot()
        .await
        .expect("boots");
    let db = app.ctx().get::<Db>().await.expect("a Db").as_ref().clone();
    tables(&db, backend).await;
    let uow = db.uow(Some(ACTOR), None).await;
    uow.execute(&add_deal("observed")).await.unwrap();
    uow.record_change(change("deals"));
    uow.commit().await.unwrap();
    let seen = app.ctx().get::<Seen>().await.unwrap();
    assert_eq!(seen.0.lock().unwrap().len(), 1);
    let rows = db
        .fetch_all(&Statement::text("SELECT entity, op, actor FROM audit"))
        .await
        .unwrap();
    let last = rows.last().expect("an audit row");
    assert_eq!(last.get::<String>("entity").unwrap(), "deals");
    assert_eq!(last.get::<String>("op").unwrap(), "created");
    assert_eq!(last.get::<String>("actor").unwrap(), ACTOR.to_string());

    // A refusing observer: the business row and the audit row roll back
    // together, and the commit's error is the observer's.
    let uow = db.uow(Some(ACTOR), None).await;
    uow.execute(&add_deal("refused")).await.unwrap();
    uow.record_change(change("refused"));
    let error = uow.commit().await.unwrap_err();
    assert!(error.to_string().contains("refused"), "{error}");
    assert_eq!(titles(&db).await, ["observed"]);
    let audit: i64 = db
        .fetch_scalar(&Statement::text("SELECT COUNT(*) FROM audit"))
        .await
        .unwrap();
    assert_eq!(audit, 1);
}

struct ObserverModule;

impl navree_kernel::Module for ObserverModule {
    fn register(&self, m: &mut navree_kernel::ModuleBuilder) {
        use navree_db::ModuleBuilderDbExt;
        m.value(Seen::default()).change_observer::<Auditor>();
    }
}

/// Statements on a `Db` join the task's unit of work: writes always (they
/// begin it), reads once it is open.
pub async fn db_statements_join_the_current_unit_of_work(scratch: Scratch) {
    let db = scratch.db().await;
    let backend = scratch.server.backend();
    tables(&db, backend).await;
    let uow = db.uow(None, None).await;
    uow.scope(async {
        // A read before the unit of work began runs on the pool.
        assert!(titles(&db).await.is_empty());
        assert!(!uow.has_open_transaction().await);
        // A write joins (and begins) it.
        db.execute(&add_deal("joined")).await.unwrap();
        assert!(uow.has_open_transaction().await);
        // Reads now see the uncommitted write.
        assert_eq!(titles(&db).await, ["joined"]);
    })
    .await;
    uow.rollback().await.unwrap();
    assert!(
        titles(&db).await.is_empty(),
        "the write was the unit of work's"
    );
}
