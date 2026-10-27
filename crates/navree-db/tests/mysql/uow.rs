//! The unit of work on MySQL: the `@navree_actor` and `@navree_request_id`
//! session variables (and a trigger reading them), commit and rollback,
//! and change observers inside the transaction.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use navree_db::{
    ChangeEvent, ChangeObserver, DatabaseModule, Db, MigrationSet, ModuleBuilderDbExt, Repo, Uow,
};
use navree_kernel::{
    BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Navree, ProviderKey, Resolver,
};
use sea_orm::sea_query::Query;
use sea_orm::{ActiveModelTrait, ConnectionTrait, DatabaseTransaction, DbErr};
use uuid::Uuid;

use crate::fixtures::{CreateNotes, create_notes, new_note, note};
use crate::support::{Scratch, count, string, strings};

type Notes = Repo<note::Entity>;

/// `actor|request_id`, with `NULL` for an unset variable.
const VARIABLES: &str = "SELECT CONCAT(IFNULL(@navree_actor, 'NULL'), '|', \
     IFNULL(@navree_request_id, 'NULL'))";

#[tokio::test]
async fn variables_are_set_inside_the_unit_of_work_and_cleared_after_it() {
    let Some(scratch) =
        Scratch::new("variables_are_set_inside_the_unit_of_work_and_cleared_after_it").await
    else {
        return;
    };
    // One connection: every unit of work and every pool read shares it.
    let db = scratch.db_with(|options| options.max_connections = 1).await;
    let actor = Uuid::now_v7();

    let committed = db.uow(Some(actor), Some("req-1".to_owned())).await;
    let tx = committed.tx().await.unwrap();
    assert_eq!(string(&*tx, VARIABLES).await, format!("{actor}|req-1"));
    drop(tx);
    committed.commit().await.unwrap();
    assert_eq!(string(db.conn(), VARIABLES).await, "NULL|NULL");

    let rolled_back = db.uow(Some(actor), None).await;
    let tx = rolled_back.tx().await.unwrap();
    assert_eq!(string(&*tx, VARIABLES).await, format!("{actor}|NULL"));
    drop(tx);
    rolled_back.rollback().await.unwrap();
    assert_eq!(string(db.conn(), VARIABLES).await, "NULL|NULL");

    // A unit of work without an actor overwrites whatever is there.
    db.conn()
        .execute_unprepared("SET @navree_actor = 'stale', @navree_request_id = 'stale'")
        .await
        .unwrap();
    let anonymous = db.uow(None, None).await;
    let tx = anonymous.tx().await.unwrap();
    assert_eq!(string(&*tx, VARIABLES).await, "NULL|NULL");
    drop(tx);
    anonymous.commit().await.unwrap();
}

/// What the variables are for: a trigger records who made the change and
/// in which request. Writes outside a unit of work record no actor, even on
/// a connection a unit of work used before.
#[tokio::test]
async fn triggers_read_the_actor_and_request_id() {
    let Some(scratch) = Scratch::new("triggers_read_the_actor_and_request_id").await else {
        return;
    };
    let db = scratch.db_with(|options| options.max_connections = 1).await;
    create_notes(&db).await;
    db.conn()
        .execute_unprepared(
            "CREATE TRIGGER notes_created AFTER INSERT ON notes FOR EACH ROW \
             INSERT INTO audit (entity, entity_id, op, actor) \
             VALUES ('notes', HEX(NEW.id), IFNULL(@navree_request_id, '-'), @navree_actor)",
        )
        .await
        .expect("creates the trigger");

    let actor = Uuid::now_v7();
    let uow = db.uow(Some(actor), Some("req-7".to_owned())).await;
    Notes::insert(&uow, new_note("by the actor")).await.unwrap();
    uow.commit().await.unwrap();
    new_note("outside any unit of work")
        .insert(db.conn())
        .await
        .unwrap();

    assert_eq!(
        strings(
            db.conn(),
            "SELECT CONCAT(op, '|', IFNULL(actor, 'NULL')) FROM audit ORDER BY id"
        )
        .await,
        [format!("req-7|{actor}"), "-|NULL".to_owned()]
    );
}

/// The trigger the database docs show: `updated_by`, a `BINARY(16)` UUID
/// column like every Navree id on MySQL, stamped from `@navree_actor` with
/// functions MySQL and MariaDB both have.
#[tokio::test]
async fn a_trigger_stamps_the_actor_into_a_binary_uuid_column() {
    let Some(scratch) =
        Scratch::new("a_trigger_stamps_the_actor_into_a_binary_uuid_column").await
    else {
        return;
    };
    let db = scratch.db_with(|options| options.max_connections = 1).await;
    for sql in [
        "CREATE TABLE deals (id INT PRIMARY KEY, stage VARCHAR(32) NOT NULL, \
         updated_by BINARY(16) NULL)",
        "CREATE TRIGGER deals_updated_by BEFORE UPDATE ON deals FOR EACH ROW \
         SET NEW.updated_by = UNHEX(REPLACE(@navree_actor, '-', ''))",
        "INSERT INTO deals (id, stage) VALUES (1, 'lead')",
    ] {
        db.conn()
            .execute_unprepared(sql)
            .await
            .unwrap_or_else(|error| panic!("{sql}: {error}"));
    }
    let updated_by = || async {
        string(
            db.conn(),
            "SELECT IFNULL(LOWER(HEX(updated_by)), 'NULL') FROM deals",
        )
        .await
    };

    let actor = Uuid::now_v7();
    let uow = db.uow(Some(actor), None).await;
    uow.tx()
        .await
        .unwrap()
        .execute_unprepared("UPDATE deals SET stage = 'won' WHERE id = 1")
        .await
        .unwrap();
    uow.commit().await.unwrap();
    assert_eq!(updated_by().await, actor.simple().to_string());

    // Outside a unit of work there is no actor.
    db.conn()
        .execute_unprepared("UPDATE deals SET stage = 'lost' WHERE id = 1")
        .await
        .unwrap();
    assert_eq!(updated_by().await, "NULL");
}

/// What the database docs warn about: a deadlock (error 1213) makes MySQL
/// roll back the victim's whole transaction at once, and what the victim's
/// unit of work runs after it commits on its own, so the unit of work's
/// rollback cannot undo it. Errors must end the unit of work.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn after_a_deadlock_the_victims_statements_commit_on_their_own() {
    let Some(scratch) =
        Scratch::new("after_a_deadlock_the_victims_statements_commit_on_their_own").await
    else {
        return;
    };
    let db = scratch.db().await;
    for sql in [
        "CREATE TABLE counters (id INT PRIMARY KEY, n INT NOT NULL)",
        "INSERT INTO counters (id, n) VALUES (1, 0), (2, 0)",
    ] {
        db.conn().execute_unprepared(sql).await.unwrap();
    }
    let bump = |id: i32| format!("UPDATE counters SET n = n + 1 WHERE id = {id}");

    let first = db.uow(None, None).await;
    let second = db.uow(None, None).await;
    let first_tx = first.tx().await.unwrap();
    let second_tx = second.tx().await.unwrap();
    first_tx.execute_unprepared(&bump(1)).await.unwrap();
    second_tx.execute_unprepared(&bump(2)).await.unwrap();
    // Each now waits for the row the other holds; InnoDB picks a victim.
    let crossing = tokio::spawn({
        let first_tx = first_tx.clone();
        let sql = bump(2);
        async move { first_tx.execute_unprepared(&sql).await.map(drop) }
    });
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let second_result = second_tx.execute_unprepared(&bump(1)).await.map(drop);
    let first_result = crossing.await.unwrap();
    let (victim, victim_tx, survivor, survivor_tx, error) = match (first_result, second_result) {
        (Err(error), Ok(())) => (first, first_tx, second, second_tx, error),
        (Ok(()), Err(error)) => (second, second_tx, first, first_tx, error),
        other => panic!("one deadlock victim expected: {other:?}"),
    };
    assert!(error.to_string().contains("Deadlock"), "{error}");

    victim_tx
        .execute_unprepared("INSERT INTO counters (id, n) VALUES (3, 0)")
        .await
        .expect("runs, outside any transaction");
    drop(victim_tx);
    victim.rollback().await.unwrap();
    drop(survivor_tx);
    survivor.commit().await.unwrap();
    assert_eq!(
        strings(db.conn(), "SELECT CAST(id AS CHAR) FROM counters ORDER BY id").await,
        ["1", "2", "3"],
        "the victim's insert survived its rollback"
    );
}

#[tokio::test]
async fn writes_are_visible_after_commit_and_gone_after_rollback() {
    let Some(scratch) =
        Scratch::new("writes_are_visible_after_commit_and_gone_after_rollback").await
    else {
        return;
    };
    let db = scratch.db().await;
    create_notes(&db).await;

    let uow = db.uow(None, None).await;
    new_note("kept")
        .insert(&*uow.tx().await.unwrap())
        .await
        .unwrap();
    assert_eq!(count(db.conn(), "notes").await, 0, "not visible before commit");
    uow.commit().await.unwrap();
    assert_eq!(count(db.conn(), "notes").await, 1);

    let uow = db.uow(None, None).await;
    new_note("dropped")
        .insert(&*uow.tx().await.unwrap())
        .await
        .unwrap();
    uow.rollback().await.unwrap();
    assert_eq!(count(db.conn(), "notes").await, 1);

    let error = db
        .transaction(|uow| async move {
            new_note("also dropped").insert(&*uow.tx().await?).await?;
            Err::<(), _>(DbErr::Custom("changed my mind".to_owned()))
        })
        .await
        .unwrap_err();
    assert!(error.to_string().contains("changed my mind"), "{error}");
    assert_eq!(count(db.conn(), "notes").await, 1);
}

/// Makes [`FailingObserver`] fail.
#[derive(Default)]
struct FailSwitch(AtomicBool);

/// Writes one `audit` row per change, inside the unit of work's transaction.
struct AuditObserver;

impl Injectable for AuditObserver {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(AuditObserver) })
    }
}

impl ChangeObserver for AuditObserver {
    fn observe<'a>(
        &'a self,
        tx: &'a DatabaseTransaction,
        uow: &'a Uow,
        changes: &'a [ChangeEvent],
    ) -> BoxFuture<'a, Result<(), DbErr>> {
        Box::pin(async move {
            for change in changes {
                tx.execute(
                    Query::insert()
                        .into_table("audit")
                        .columns(["entity", "entity_id", "op", "actor"])
                        .values_panic([
                            change.entity.clone().into(),
                            change.id.clone().into(),
                            change.op.as_str().into(),
                            uow.actor().map(|a| a.to_string()).into(),
                        ]),
                )
                .await?;
            }
            Ok(())
        })
    }
}

/// Fails while the [`FailSwitch`] is on.
struct FailingObserver(Arc<FailSwitch>);

impl Injectable for FailingObserver {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<FailSwitch>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move { Ok(FailingObserver(r.get::<FailSwitch>().await?)) })
    }
}

impl ChangeObserver for FailingObserver {
    fn observe<'a>(
        &'a self,
        _tx: &'a DatabaseTransaction,
        _uow: &'a Uow,
        _changes: &'a [ChangeEvent],
    ) -> BoxFuture<'a, Result<(), DbErr>> {
        Box::pin(async move {
            if self.0.0.load(Ordering::SeqCst) {
                Err(DbErr::Custom("observer refused".to_owned()))
            } else {
                Ok(())
            }
        })
    }
}

struct NotesModule;

impl Module for NotesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(MigrationSet::new("notes").migration(CreateNotes))
            .value(FailSwitch::default())
            .change_observer::<AuditObserver>()
            .change_observer::<FailingObserver>();
    }
}

#[tokio::test]
async fn observers_write_in_the_transaction_and_their_errors_roll_it_back() {
    let Some(scratch) =
        Scratch::new("observers_write_in_the_transaction_and_their_errors_roll_it_back").await
    else {
        return;
    };
    let mut options = scratch.options();
    options.migrate_on_boot = true;
    options.max_connections = 1;
    let app = Navree::app()
        .module(DatabaseModule::for_root(options))
        .module(NotesModule)
        .boot()
        .await
        .expect("boots and migrates");
    let db = app.ctx().get::<Db>().await.unwrap();
    let actor = Uuid::now_v7();

    let uow = db.uow(Some(actor), Some("req".to_owned())).await;
    Notes::insert(&uow, new_note("a")).await.unwrap();
    Notes::insert(&uow, new_note("b")).await.unwrap();
    uow.commit().await.expect("commits");
    assert_eq!(count(db.conn(), "notes").await, 2);
    assert_eq!(
        strings(db.conn(), "SELECT CONCAT(op, '|', actor) FROM audit ORDER BY id").await,
        [format!("created|{actor}"), format!("created|{actor}")]
    );

    app.ctx()
        .get::<FailSwitch>()
        .await
        .unwrap()
        .0
        .store(true, Ordering::SeqCst);
    let uow = db.uow(Some(actor), None).await;
    Notes::insert(&uow, new_note("c")).await.unwrap();
    let error = uow.commit().await.unwrap_err();
    assert!(error.to_string().contains("observer refused"), "{error}");
    assert_eq!(count(db.conn(), "notes").await, 2, "the change is rolled back");
    assert_eq!(count(db.conn(), "audit").await, 2, "and its audit row with it");
    assert_eq!(string(db.conn(), VARIABLES).await, "NULL|NULL");
    app.shutdown().await.unwrap();
}
