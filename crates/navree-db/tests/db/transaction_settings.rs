//! `database.transaction_settings`: the statements a unit of work sends to
//! set (and, on MySQL, clear) the actor and request id, checked on a
//! scripted connection (no server needed).

use navree_db::{Db, DbOptions};
use sea_orm::{ConnectionTrait, DbBackend, MockDatabase, MockExecResult};
use uuid::Uuid;

/// The actor the scripted units of work run for.
const ACTOR: Uuid = Uuid::from_u128(0x0199_a7a0_0000_7000_8000_0000_0000_0001);

/// A scripted connection to `backend` whose statements each affect a row.
fn scripted(backend: DbBackend, execs: usize) -> Db {
    let mock = MockDatabase::new(backend).append_exec_results(
        std::iter::repeat_with(|| MockExecResult {
            last_insert_id: 0,
            rows_affected: 1,
        })
        .take(execs),
    );
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

/// A unit of work for `ACTOR` that deletes every note and commits.
async fn delete_notes(db: &Db, table: &str) {
    let uow = db.uow(Some(ACTOR), Some("req-1".to_owned())).await;
    let tx = uow.tx().await.expect("begins");
    tx.execute_unprepared(&format!("DELETE FROM {table}"))
        .await
        .unwrap();
    drop(tx);
    uow.commit().await.expect("commits");
}

#[test]
fn transaction_settings_are_on_by_default() {
    assert!(DbOptions::default().transaction_settings);
    let options: DbOptions =
        serde_json::from_value(serde_json::json!({ "transaction_settings": false }))
            .expect("valid section");
    assert!(!options.transaction_settings);
    assert!(
        Db::from_connection(MockDatabase::new(DbBackend::Postgres).into_connection())
            .transaction_settings()
    );
}

#[tokio::test]
async fn postgres_transactions_set_the_actor_and_request_id_by_default() {
    let db = scripted(DbBackend::Postgres, 2);
    delete_notes(&db, "notes").await;
    assert_eq!(
        sent(&db),
        [
            "BEGIN",
            "SELECT set_config('navree.actor', '0199a7a0-0000-7000-8000-000000000001', true), \
             set_config('navree.request_id', 'req-1', true)",
            "DELETE FROM notes",
            "COMMIT",
        ]
    );
}

/// Without settings to read, a transaction is its own statements between
/// `BEGIN` and `COMMIT`: one round trip fewer per unit of work.
#[tokio::test]
async fn postgres_transactions_skip_the_settings_when_turned_off() {
    let db = scripted(DbBackend::Postgres, 1).with_transaction_settings(false);
    assert!(!db.transaction_settings());
    delete_notes(&db, "notes").await;
    assert_eq!(sent(&db), ["BEGIN", "DELETE FROM notes", "COMMIT"]);
}

#[tokio::test]
async fn mysql_transactions_skip_setting_and_clearing_the_variables_when_turned_off() {
    let db = scripted(DbBackend::MySql, 1).with_transaction_settings(false);
    delete_notes(&db, "`notes`").await;
    assert_eq!(sent(&db), ["BEGIN", "DELETE FROM `notes`", "COMMIT"]);
}
