//! The statements navree-db sends to MySQL and MariaDB, checked on a
//! scripted connection (no server needed): the unit of work's session
//! variables, repositories, pagination, migrations and `migrate:fresh`.
//! `tests/mysql` runs the same paths against a real server.

use std::sync::Arc;

use navree_db::{
    ChangeOp, CursorQuery, DatabaseModule, Db, DbOptions, MigrationSet, Migrator,
    ModuleBuilderDbExt, PageMeta, PageQuery, Repo, cursor_paginate, cursor_paginate_desc, paginate,
};
use navree_kernel::{App, Module, ModuleBuilder, Navree};
use sea_orm::{
    ActiveValue::Set, ConnectionTrait, DbBackend, DbErr, EntityTrait, IntoActiveModel, IntoMockRow,
    MockDatabase, MockExecResult, MockRow, QueryOrder, Value,
};
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};
use uuid::Uuid;

use crate::fixtures::{CreateTable, note};

/// The actor the scripted units of work run for.
const ACTOR: Uuid = Uuid::from_u128(0x0199_a7a0_0000_7000_8000_0000_0000_0001);

/// The columns SeaORM selects for a `notes` row.
const NOTE_COLUMNS: &str = "`notes`.`id`, `notes`.`title`, `notes`.`body`";

/// A `notes` row.
fn a_note(n: u128, title: &str) -> note::Model {
    note::Model {
        id: Uuid::from_u128(0x0199_a7a0_0000_7000_8000_0000_0000_1000 + n),
        title: title.to_owned(),
        body: format!("body of {title}"),
    }
}

/// Every statement sent so far as `(sql, values)` (drains the log).
fn sent_raw(db: &Db) -> Vec<(String, Vec<Value>)> {
    db.conn()
        .clone()
        .into_transaction_log()
        .iter()
        .flat_map(|transaction| transaction.statements())
        .map(|statement| {
            let values = statement
                .values
                .as_ref()
                .map(|values| values.0.clone())
                .unwrap_or_default();
            (statement.sql.clone(), values)
        })
        .collect()
}

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

    let begin =
        "SET @navree_actor = '0199a7a0-0000-7000-8000-000000000001', @navree_request_id = NULL";
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
    assert_eq!(
        log[3],
        "SET @navree_actor = NULL, @navree_request_id = NULL"
    );
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

// --- Repositories ------------------------------------------------------------

/// MySQL has no `RETURNING`: SeaORM reads a row back by its key after
/// `INSERT` and `UPDATE`. `Repo::update` locks the row it reads first.
#[tokio::test]
async fn repo_writes_through_the_unit_of_work_with_mysql_statements() {
    let created = a_note(1, "a");
    let changed = note::Model {
        body: "changed".to_owned(),
        ..created.clone()
    };
    let db = scripted(
        5,
        vec![
            vec![created.clone().into_mock_row()],
            vec![created.clone().into_mock_row()],
            vec![changed.clone().into_mock_row()],
        ],
    );
    let uow = db.uow(None, None).await;
    let inserted = Repo::<note::Entity>::insert(
        &uow,
        note::ActiveModel {
            id: Set(created.id),
            title: Set(created.title.clone()),
            body: Set(created.body.clone()),
        },
    )
    .await
    .expect("inserts");
    let mut am = inserted.into_active_model();
    am.body = Set("changed".to_owned());
    let updated = Repo::<note::Entity>::update(&uow, am)
        .await
        .expect("updates");
    Repo::<note::Entity>::delete(&uow, updated)
        .await
        .expect("deletes");
    let ops: Vec<ChangeOp> = uow.changes().iter().map(|change| change.op).collect();
    assert_eq!(
        ops,
        [ChangeOp::Created, ChangeOp::Updated, ChangeOp::Deleted]
    );
    assert_eq!(
        uow.changes()[1].new,
        Some(serde_json::json!({"body": "changed"}))
    );
    uow.commit().await.expect("commits");

    let sql: Vec<String> = sent_raw(&db).into_iter().map(|(sql, _)| sql).collect();
    assert_eq!(
        sql,
        [
            "BEGIN".to_owned(),
            "SET @navree_actor = ?, @navree_request_id = ?".to_owned(),
            "INSERT INTO `notes` (`id`, `title`, `body`) VALUES (?, ?, ?)".to_owned(),
            format!("SELECT {NOTE_COLUMNS} FROM `notes` WHERE `notes`.`id` = ? LIMIT ?"),
            format!("SELECT {NOTE_COLUMNS} FROM `notes` WHERE `notes`.`id` = ? LIMIT ? FOR UPDATE"),
            "UPDATE `notes` SET `body` = ? WHERE `notes`.`id` = ?".to_owned(),
            format!("SELECT {NOTE_COLUMNS} FROM `notes` WHERE `notes`.`id` = ? LIMIT ?"),
            "DELETE FROM `notes` WHERE `notes`.`id` = ?".to_owned(),
            "SET @navree_actor = NULL, @navree_request_id = NULL".to_owned(),
            "COMMIT".to_owned(),
        ]
    );
}

#[tokio::test]
async fn repo_find_for_update_locks_the_row() {
    let found = a_note(2, "b");
    let db = scripted(2, vec![vec![found.clone().into_mock_row()]]);
    let uow = db.uow(None, None).await;
    let locked = Repo::<note::Entity>::find_for_update(&uow, found.id)
        .await
        .expect("reads");
    assert_eq!(locked, Some(found.clone()));
    // The log holds a transaction once it has ended.
    uow.rollback().await.expect("rolls back");
    let log = sent_raw(&db);
    assert_eq!(
        log[2],
        (
            format!("SELECT {NOTE_COLUMNS} FROM `notes` WHERE `notes`.`id` = ? LIMIT ? FOR UPDATE"),
            vec![found.id.into(), 1u64.into()]
        )
    );
}

// --- Pagination --------------------------------------------------------------

/// A `COUNT(*)` row as MySQL returns it.
fn count_row(n: i64) -> MockRow {
    std::collections::BTreeMap::from([("num_items", Value::BigInt(Some(n)))]).into_mock_row()
}

#[tokio::test]
async fn paginate_counts_then_reads_one_page() {
    let rows: Vec<MockRow> = (10..20)
        .map(|n| a_note(n, &format!("note-{n}")).into_mock_row())
        .collect();
    let db = scripted(0, vec![vec![count_row(23)], rows]);
    let page = paginate(
        note::Entity::find().order_by_asc(note::Column::Title),
        db.conn(),
        PageQuery::new(2, 10),
    )
    .await
    .expect("pages");
    assert_eq!(
        page.meta,
        PageMeta {
            current_page: 2,
            per_page: 10,
            total: 23,
            last_page: 3
        }
    );
    assert_eq!(page.data.len(), 10);
    assert_eq!(page.data[0].title, "note-10");
    assert_eq!(
        sent_raw(&db),
        [
            (
                format!(
                    "SELECT COUNT(*) AS num_items FROM (SELECT {NOTE_COLUMNS} FROM `notes`) AS `sub_query`"
                ),
                vec![]
            ),
            (
                format!(
                    "SELECT {NOTE_COLUMNS} FROM `notes` ORDER BY `notes`.`title` ASC LIMIT ? OFFSET ?"
                ),
                vec![10u64.into(), 10u64.into()]
            ),
        ]
    );
}

/// A page past the end is empty and its rows are not queried.
#[tokio::test]
async fn paginate_past_the_end_reads_only_the_count() {
    let db = scripted(0, vec![vec![count_row(3)]]);
    let page = paginate(
        note::Entity::find(),
        db.conn(),
        PageQuery::new(u64::MAX, 100),
    )
    .await
    .expect("pages");
    assert!(page.data.is_empty());
    assert_eq!(page.meta.total, 3);
    assert_eq!(sent_raw(&db).len(), 1, "the count only");
}

#[tokio::test]
async fn cursor_pages_filter_and_order_on_the_key() {
    let notes: Vec<note::Model> = (0..5).map(|n| a_note(n, &format!("note-{n}"))).collect();
    let rows = |range: std::ops::Range<usize>| -> Vec<MockRow> {
        notes[range]
            .iter()
            .cloned()
            .map(IntoMockRow::into_mock_row)
            .collect()
    };
    let db = scripted(0, vec![rows(0..3), rows(2..5), rows(0..2)]);
    let first = cursor_paginate(
        note::Entity::find(),
        note::Column::Id,
        db.conn(),
        CursorQuery::new(None, 2),
    )
    .await
    .unwrap();
    assert_eq!(first.data, notes[0..2]);
    let second = cursor_paginate(
        note::Entity::find(),
        note::Column::Id,
        db.conn(),
        CursorQuery::new(first.next_cursor.clone(), 2),
    )
    .await
    .unwrap();
    assert_eq!(second.data, notes[2..4]);
    let newest = cursor_paginate_desc(
        note::Entity::find(),
        note::Column::Id,
        db.conn(),
        CursorQuery::new(second.next_cursor.clone(), 2),
    )
    .await
    .unwrap();
    assert_eq!(newest.next_cursor, None);
    assert_eq!(
        sent_raw(&db),
        [
            (
                format!("SELECT {NOTE_COLUMNS} FROM `notes` ORDER BY `notes`.`id` ASC LIMIT ?"),
                vec![3u64.into()]
            ),
            (
                format!(
                    "SELECT {NOTE_COLUMNS} FROM `notes` WHERE `notes`.`id` > ? ORDER BY `notes`.`id` ASC LIMIT ?"
                ),
                vec![notes[1].id.into(), 3u64.into()]
            ),
            (
                format!(
                    "SELECT {NOTE_COLUMNS} FROM `notes` WHERE `notes`.`id` < ? ORDER BY `notes`.`id` DESC LIMIT ?"
                ),
                vec![notes[3].id.into(), 3u64.into()]
            ),
        ]
    );
}

// --- Migrations ----------------------------------------------------------------

/// Owns `users`; with `transactional`, its migration asks for a transaction.
struct UsersModule {
    transactional: Option<bool>,
}

impl Module for UsersModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.migrations(MigrationSet::new("users").migration(Transactional {
            inner: CreateTable {
                name: "m0001_create_users",
                table: "users",
                references: None,
            },
            transactional: self.transactional,
        }));
    }
}

/// A migration that overrides `use_transaction`.
struct Transactional<M> {
    inner: M,
    transactional: Option<bool>,
}

impl<M: MigrationName> MigrationName for Transactional<M> {
    fn name(&self) -> &str {
        self.inner.name()
    }
}

#[sea_orm_migration::async_trait::async_trait]
impl<M: MigrationTrait> MigrationTrait for Transactional<M> {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        self.inner.up(manager).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        self.inner.down(manager).await
    }

    fn use_transaction(&self) -> Option<bool> {
        self.transactional
    }
}

/// An app whose `Db` is `db` (a scripted connection).
async fn app_on(db: Db, transactional: Option<bool>) -> App {
    Navree::app()
        .module(DatabaseModule::for_root(DbOptions::default()))
        .module(UsersModule { transactional })
        .override_value(Arc::new(db))
        .boot()
        .await
        .expect("boots")
}

/// A `navree_migrations` row.
fn applied_row(module: &str, name: &str, batch: i64) -> MockRow {
    std::collections::BTreeMap::from([
        ("module", Value::from(module)),
        ("name", Value::from(name)),
        ("batch", Value::BigInt(Some(batch))),
        ("applied_at", Value::from(chrono::Utc::now())),
    ])
    .into_mock_row()
}

const CREATE_MIGRATIONS_TABLE: &str = "CREATE TABLE IF NOT EXISTS `navree_migrations` ( \
     `module` varchar(255) NOT NULL, `name` varchar(255) NOT NULL, \
     `batch` bigint NOT NULL, `applied_at` datetime NOT NULL, \
     PRIMARY KEY (`module`, `name`) )";

const SELECT_APPLIED: &str =
    "SELECT `module`, `name`, `batch`, `applied_at` FROM `navree_migrations`";

const INSERT_APPLIED: &str = "INSERT INTO `navree_migrations` \
     (`module`, `name`, `batch`, `applied_at`) VALUES (?, ?, ?, ?)";

/// MySQL commits DDL implicitly, so a migration runs outside a
/// transaction unless it asks for one; `applied_at` is a `DATETIME` (UTC,
/// no 2038 limit).
#[tokio::test]
async fn migrations_run_outside_a_transaction_on_mysql() {
    let db = scripted(3, vec![vec![]]);
    let app = app_on(db.clone(), None).await;
    let applied = Migrator::up(app.ctx()).await.expect("migrates");
    assert_eq!(applied, ["users/m0001_create_users"]);
    let log = sent_raw(&db);
    let sql: Vec<&str> = log.iter().map(|(sql, _)| sql.as_str()).collect();
    assert_eq!(
        sql,
        [
            CREATE_MIGRATIONS_TABLE,
            SELECT_APPLIED,
            "CREATE TABLE `users` ( `id` binary(16) NOT NULL PRIMARY KEY, `title` varchar(255) NOT NULL UNIQUE )",
            INSERT_APPLIED,
        ]
    );
    assert_eq!(
        log[3].1[..3],
        [
            Value::from("users"),
            Value::from("m0001_create_users"),
            Value::BigInt(Some(1))
        ]
    );
}

/// A migration that asks for a transaction gets one, with its
/// `navree_migrations` row (for data migrations; DDL still commits).
#[tokio::test]
async fn a_migration_can_ask_for_a_transaction_on_mysql() {
    let db = scripted(3, vec![vec![]]);
    let app = app_on(db.clone(), Some(true)).await;
    Migrator::up(app.ctx()).await.expect("migrates");
    let sql: Vec<String> = sent_raw(&db).into_iter().map(|(sql, _)| sql).collect();
    assert_eq!(
        sql[2..],
        ["BEGIN", sql[3].as_str(), INSERT_APPLIED, "COMMIT"]
    );
    assert!(sql[3].starts_with("CREATE TABLE `users`"), "{}", sql[3]);
}

#[tokio::test]
async fn rollback_reverts_the_last_batch_on_mysql() {
    let db = scripted(3, vec![vec![applied_row("users", "m0001_create_users", 1)]]);
    let app = app_on(db.clone(), None).await;
    let reverted = Migrator::rollback(app.ctx()).await.expect("rolls back");
    assert_eq!(reverted, ["users/m0001_create_users"]);
    let log = sent_raw(&db);
    let sql: Vec<&str> = log.iter().map(|(sql, _)| sql.as_str()).collect();
    assert_eq!(
        sql,
        [
            CREATE_MIGRATIONS_TABLE,
            SELECT_APPLIED,
            "DROP TABLE `users`",
            "DELETE FROM `navree_migrations` WHERE `module` = ? AND `name` = ?",
        ]
    );
    assert_eq!(
        log[3].1,
        [Value::from("users"), Value::from("m0001_create_users")]
    );
}

/// `migrate:fresh` drops every base table in one statement, on one
/// connection with foreign key checks off, and turns them back on before
/// the connection returns to the pool.
#[tokio::test]
async fn fresh_drops_every_table_with_foreign_key_checks_off() {
    let table = |name: &str| {
        std::collections::BTreeMap::from([("name", Value::from(name))]).into_mock_row()
    };
    let db = scripted(
        7,
        vec![
            vec![table("navree_migrations"), table("role`s"), table("users")],
            vec![],
        ],
    );
    let app = app_on(db.clone(), None).await;
    let applied = Migrator::fresh(app.ctx()).await.expect("fresh");
    assert_eq!(applied, ["users/m0001_create_users"]);
    let sql: Vec<String> = sent_raw(&db).into_iter().map(|(sql, _)| sql).collect();
    assert_eq!(
        sql[..6],
        [
            "SELECT CAST(table_name AS CHAR) AS name FROM information_schema.tables \
             WHERE table_schema = DATABASE() AND table_type IN ('BASE TABLE', 'SYSTEM VERSIONED') \
             ORDER BY table_name",
            "BEGIN",
            "SET FOREIGN_KEY_CHECKS = 0",
            "DROP TABLE IF EXISTS `navree_migrations`, `role``s`, `users`",
            "SET FOREIGN_KEY_CHECKS = 1",
            "COMMIT",
        ]
    );
    assert_eq!(sql[6], CREATE_MIGRATIONS_TABLE, "then migrates again");
}

/// A failed drop still turns foreign key checks back on.
#[tokio::test]
async fn fresh_turns_foreign_key_checks_back_on_when_the_drop_fails() {
    let mut mock = MockDatabase::new(DbBackend::MySql)
        .append_query_results([vec![
            std::collections::BTreeMap::from([("name", Value::from("users"))]).into_mock_row(),
        ]])
        .append_exec_results([MockExecResult::default()])
        .append_exec_errors([DbErr::Custom("lock wait timeout".to_owned())]);
    mock = mock.append_exec_results([MockExecResult::default()]);
    let db = Db::from_connection(mock.into_connection());
    let app = app_on(db.clone(), None).await;
    let error = Migrator::fresh(app.ctx()).await.unwrap_err().to_string();
    assert!(error.contains("lock wait timeout"), "{error}");
    let sql: Vec<String> = sent_raw(&db).into_iter().map(|(sql, _)| sql).collect();
    assert_eq!(
        sql[1..],
        [
            "BEGIN",
            "SET FOREIGN_KEY_CHECKS = 0",
            "DROP TABLE IF EXISTS `users`",
            "SET FOREIGN_KEY_CHECKS = 1",
            "ROLLBACK",
        ]
    );
}

/// An empty database has nothing to drop.
#[tokio::test]
async fn fresh_on_an_empty_mysql_database_only_migrates() {
    let db = scripted(3, vec![vec![], vec![]]);
    let app = app_on(db.clone(), None).await;
    Migrator::fresh(app.ctx()).await.expect("fresh");
    let sql: Vec<String> = sent_raw(&db).into_iter().map(|(sql, _)| sql).collect();
    assert!(
        sql[0].starts_with("SELECT CAST(table_name AS CHAR)"),
        "{}",
        sql[0]
    );
    assert_eq!(sql[1], CREATE_MIGRATIONS_TABLE);
    assert!(!sql.iter().any(|sql| sql.contains("FOREIGN_KEY_CHECKS")));
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
