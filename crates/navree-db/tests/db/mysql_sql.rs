//! The statements navree-db sends to MySQL and MariaDB, checked on a
//! scripted connection (no server needed): the unit of work's session
//! variables, repositories, pagination, migrations and `migrate:fresh`.
//! `tests/mysql` runs the same paths against a real server.

use navree_db::Db;
use sea_orm::{ConnectionTrait, DbBackend, DbErr, MockDatabase, MockExecResult, MockRow};
use uuid::Uuid;

/// The actor the scripted units of work run for.
const ACTOR: Uuid = Uuid::from_u128(0x0199_a7a0_0000_7000_8000_0000_0000_0001);

/// A scripted MySQL connection: statements answer with `execs` (one
/// affected row each, in order) and queries with `queries` (in order).
fn scripted(execs: usize, queries: Vec<Vec<MockRow>>) -> Db {
    let mut mock = MockDatabase::new(DbBackend::MySql).append_exec_results(
        std::iter::repeat_with(|| MockExecResult {
            last_insert_id: 0,
            rows_affected: 1,
        })
        .take(execs),
    );
    for rows in queries {
        mock = mock.append_query_results([rows]);
    }
    Db::from_connection(mock.into_connection())
}

/// A scripted MySQL connection whose statements answer with `execs`, in
/// order.
fn scripted_with(execs: Vec<Result<u64, DbErr>>) -> Db {
    let mut mock = MockDatabase::new(DbBackend::MySql);
    for exec in execs {
        mock = match exec {
            Ok(rows_affected) => mock.append_exec_results([MockExecResult {
                last_insert_id: 0,
                rows_affected,
            }]),
            Err(error) => mock.append_exec_errors([error]),
        };
    }
    Db::from_connection(mock.into_connection())
}

/// Every statement sent so far, values in place (drains the log).
fn sent(db: &Db) -> Vec<String> {
    db.conn()
        .clone()
        .into_transaction_log()
        .iter()
        .flat_map(|transaction| transaction.statements())
        .map(ToString::to_string)
        .collect()
}

// --- The unit of work ------------------------------------------------------

#[tokio::test]
async fn uow_sets_its_session_variables_after_begin_and_clears_them_before_commit() {
    let db = scripted(3, vec![]);
    let uow = db.uow(Some(ACTOR), Some("req-1".to_owned())).await;
    let tx = uow.tx().await.expect("begins");
    tx.execute_unprepared("DELETE FROM `notes`").await.unwrap();
    drop(tx);
    uow.commit().await.expect("commits");
    assert_eq!(
        sent(&db),
        [
            "BEGIN",
            "SET @navree_actor = '0199a7a0-0000-7000-8000-000000000001', @navree_request_id = 'req-1'",
            "DELETE FROM `notes`",
            "SET @navree_actor = NULL, @navree_request_id = NULL",
            "COMMIT",
        ]
    );
}

#[tokio::test]
async fn uow_binds_the_variables_as_values() {
    let db = scripted(2, vec![]);
    let uow = db.uow(Some(ACTOR), Some("it's".to_owned())).await;
    uow.tx().await.expect("begins");
    uow.rollback().await.expect("rolls back");
    let log = db.conn().clone().into_transaction_log();
    let set = &log[0].statements()[1];
    assert_eq!(set.sql, "SET @navree_actor = ?, @navree_request_id = ?");
    assert_eq!(
        set.values.as_ref().map(|values| values.0.clone()),
        Some(vec![ACTOR.to_string().into(), "it's".into()])
    );
}

#[tokio::test]
async fn uow_without_an_actor_sets_null() {
    let db = scripted(2, vec![]);
    let uow = db.uow(None, None).await;
    uow.tx().await.expect("begins");
    uow.commit().await.expect("commits");
    assert_eq!(
        sent(&db),
        [
            "BEGIN",
            "SET @navree_actor = NULL, @navree_request_id = NULL",
            "SET @navree_actor = NULL, @navree_request_id = NULL",
            "COMMIT",
        ]
    );
}

#[tokio::test]
async fn uow_clears_its_session_variables_before_rollback() {
    let db = scripted(3, vec![]);
    let uow = db.uow(Some(ACTOR), None).await;
    uow.tx()
        .await
        .unwrap()
        .execute_unprepared("DELETE FROM `notes`")
        .await
        .unwrap();
    uow.rollback().await.expect("rolls back");
    assert_eq!(
        sent(&db),
        [
            "BEGIN",
            "SET @navree_actor = '0199a7a0-0000-7000-8000-000000000001', @navree_request_id = NULL",
            "DELETE FROM `notes`",
            "SET @navree_actor = NULL, @navree_request_id = NULL",
            "ROLLBACK",
        ]
    );
}

/// A transaction still held elsewhere rolls back when its last holder
/// drops it; the variables are cleared at once.
#[tokio::test]
async fn uow_clears_its_session_variables_when_the_transaction_is_still_held() {
    let db = scripted(4, vec![]);
    let committed = db.uow(Some(ACTOR), None).await;
    let held = committed.tx().await.unwrap();
    let error = committed.commit().await.unwrap_err();
    assert!(error.to_string().contains("still in use"), "{error}");
    drop(held);

    let rolled_back = db.uow(Some(ACTOR), None).await;
    let held = rolled_back.tx().await.unwrap();
    rolled_back.rollback().await.expect("rolls back");
    drop(held);

    let begin = "SET @navree_actor = '0199a7a0-0000-7000-8000-000000000001', @navree_request_id = NULL";
    let clear = "SET @navree_actor = NULL, @navree_request_id = NULL";
    assert_eq!(
        sent(&db),
        [
            "BEGIN", begin, clear, "ROLLBACK", "BEGIN", begin, clear, "ROLLBACK"
        ]
    );
}

/// A failure setting the variables fails `tx()` and ends the transaction:
/// no half-begun unit of work is left behind.
#[tokio::test]
async fn uow_begin_fails_when_the_variables_cannot_be_set() {
    let db = scripted_with(vec![Err(DbErr::Custom("connection lost".to_owned()))]);
    let uow = db.uow(Some(ACTOR), None).await;
    let error = uow.tx().await.unwrap_err();
    assert!(error.to_string().contains("connection lost"), "{error}");
    let log = sent(&db);
    assert_eq!(log.first().map(String::as_str), Some("BEGIN"));
    assert_eq!(log.last().map(String::as_str), Some("ROLLBACK"));
}

/// Clearing the variables is part of the commit: when it fails, the unit
/// of work rolls back rather than leave them on a pooled connection.
#[tokio::test]
async fn uow_rolls_back_when_the_variables_cannot_be_cleared() {
    let db = scripted_with(vec![
        Ok(0),
        Ok(1),
        Err(DbErr::Custom("connection lost".to_owned())),
    ]);
    let uow = db.uow(Some(ACTOR), None).await;
    uow.tx()
        .await
        .unwrap()
        .execute_unprepared("DELETE FROM `notes`")
        .await
        .unwrap();
    let error = uow.commit().await.unwrap_err();
    assert!(error.to_string().contains("connection lost"), "{error}");
    let log = sent(&db);
    assert_eq!(log.len(), 5, "{log:#?}");
    assert_eq!(log[3], "SET @navree_actor = NULL, @navree_request_id = NULL");
    assert_eq!(log[4], "ROLLBACK");
    assert!(uow.is_finished().await);
}

#[tokio::test]
async fn uow_locks_rows_for_update_on_mysql() {
    let db = scripted(0, vec![]);
    let uow = db.uow(None, None).await;
    assert!(uow.lock_for_update(), "MySQL has SELECT ... FOR UPDATE");
    assert_eq!(uow.db().backend(), DbBackend::MySql);
    // Nothing was sent: the transaction is lazy.
    assert!(sent(&db).is_empty());
}

#[tokio::test]
async fn db_transaction_runs_in_a_unit_of_work_with_null_variables() {
    let db = scripted(3, vec![]);
    let rows = db
        .transaction(|uow| async move {
            let tx = uow.tx().await?;
            let result = tx.execute_unprepared("DELETE FROM `sessions`").await?;
            Ok(result.rows_affected())
        })
        .await
        .expect("commits");
    assert_eq!(rows, 1);
    assert_eq!(
        sent(&db),
        [
            "BEGIN",
            "SET @navree_actor = NULL, @navree_request_id = NULL",
            "DELETE FROM `sessions`",
            "SET @navree_actor = NULL, @navree_request_id = NULL",
            "COMMIT",
        ]
    );
}
