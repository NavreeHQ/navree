//! The tests' own connections: a [`SchemaConnection`] on an SQLx pool or
//! transaction (SQLite files; PostgreSQL and MySQL with the `postgres` and
//! `mysql` features). navree-schema talks to databases only through the
//! trait; navree-db implements it for the app's connections.

#![allow(dead_code)]

use std::str::FromStr;
use std::sync::Arc;

use navree_schema::{BoxFuture, Dialect, SchemaConnection, SchemaError, SchemaRow, SchemaValue};
use sqlx::{AssertSqlSafe, Column, Row, TypeInfo, ValueRef};
use tokio::sync::Mutex;

/// A pool of one connection (session settings stick).
#[derive(Clone, Debug)]
pub enum TestConn {
    /// An SQLite file.
    Sqlite(sqlx::SqlitePool),
    /// A PostgreSQL database.
    #[cfg(feature = "postgres")]
    Postgres(sqlx::PgPool),
    /// A MySQL or MariaDB database.
    #[cfg(feature = "mysql")]
    MySql(sqlx::MySqlPool),
}

/// A transaction on a [`TestConn`].
pub struct TestTx {
    dialect: Dialect,
    tx: Mutex<Option<Tx>>,
}

enum Tx {
    Sqlite(sqlx::Transaction<'static, sqlx::Sqlite>),
    #[cfg(feature = "postgres")]
    Postgres(sqlx::Transaction<'static, sqlx::Postgres>),
    #[cfg(feature = "mysql")]
    MySql(sqlx::Transaction<'static, sqlx::MySql>),
}

impl TestConn {
    /// The SQLite database at `url`, foreign keys on or off.
    pub async fn sqlite(url: &str, foreign_keys: bool) -> TestConn {
        let options = sqlx::sqlite::SqliteConnectOptions::from_str(url)
            .expect("an SQLite URL")
            .foreign_keys(foreign_keys);
        let pool = sqlx::pool::PoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("sqlite connects");
        TestConn::Sqlite(pool)
    }

    /// The server database at `url` (`postgres://` or `mysql://`).
    #[allow(unused_variables, clippy::unused_async)]
    pub async fn server(url: &str) -> TestConn {
        #[cfg(feature = "postgres")]
        if url.starts_with("postgres") {
            let pool = sqlx::pool::PoolOptions::new()
                .max_connections(1)
                .connect(url)
                .await
                .expect("postgres connects");
            return TestConn::Postgres(pool);
        }
        #[cfg(feature = "mysql")]
        if url.starts_with("mysql") || url.starts_with("mariadb") {
            let pool = sqlx::pool::PoolOptions::new()
                .max_connections(1)
                .connect(&url.replacen("mariadb://", "mysql://", 1))
                .await
                .expect("mysql connects");
            return TestConn::MySql(pool);
        }
        panic!("no test driver for {url}")
    }

    /// Runs `sql` (several statements allowed).
    pub async fn execute_unprepared(&self, sql: &str) -> Result<(), String> {
        let sql = AssertSqlSafe(sql.to_owned());
        match self {
            TestConn::Sqlite(pool) => sqlx::raw_sql(sql).execute(pool).await.map(drop),
            #[cfg(feature = "postgres")]
            TestConn::Postgres(pool) => sqlx::raw_sql(sql).execute(pool).await.map(drop),
            #[cfg(feature = "mysql")]
            TestConn::MySql(pool) => sqlx::raw_sql(sql).execute(pool).await.map(drop),
        }
        .map_err(|error| error.to_string())
    }

    /// The rows of `sql`.
    pub async fn rows(&self, sql: &str) -> Vec<SchemaRow> {
        self.query(sql, &[])
            .await
            .unwrap_or_else(|error| panic!("{sql}: {error}"))
    }

    /// Begins a transaction.
    pub async fn begin(&self) -> Result<TestTx, String> {
        let tx = match self {
            TestConn::Sqlite(pool) => pool.begin().await.map(Tx::Sqlite),
            #[cfg(feature = "postgres")]
            TestConn::Postgres(pool) => pool.begin().await.map(Tx::Postgres),
            #[cfg(feature = "mysql")]
            TestConn::MySql(pool) => pool.begin().await.map(Tx::MySql),
        }
        .map_err(|error| error.to_string())?;
        Ok(TestTx {
            dialect: self.backend(),
            tx: Mutex::new(Some(tx)),
        })
    }

    /// Closes the pool.
    pub async fn close(&self) -> Result<(), String> {
        match self {
            TestConn::Sqlite(pool) => pool.close().await,
            #[cfg(feature = "postgres")]
            TestConn::Postgres(pool) => pool.close().await,
            #[cfg(feature = "mysql")]
            TestConn::MySql(pool) => pool.close().await,
        }
        Ok(())
    }
}

impl TestTx {
    /// Commits.
    pub async fn commit(self) -> Result<(), String> {
        match self.tx.into_inner().expect("open") {
            Tx::Sqlite(tx) => tx.commit().await,
            #[cfg(feature = "postgres")]
            Tx::Postgres(tx) => tx.commit().await,
            #[cfg(feature = "mysql")]
            Tx::MySql(tx) => tx.commit().await,
        }
        .map_err(|error| error.to_string())
    }

    /// Rolls back.
    pub async fn rollback(self) -> Result<(), String> {
        match self.tx.into_inner().expect("open") {
            Tx::Sqlite(tx) => tx.rollback().await,
            #[cfg(feature = "postgres")]
            Tx::Postgres(tx) => tx.rollback().await,
            #[cfg(feature = "mysql")]
            Tx::MySql(tx) => tx.rollback().await,
        }
        .map_err(|error| error.to_string())
    }

    /// Runs `sql` (several statements allowed).
    pub async fn execute_unprepared(&self, sql: &str) -> Result<(), String> {
        SchemaConnection::execute(self, sql)
            .await
            .map_err(|error| error.to_string())
    }
}

/// Binds `params` to `sql` and fetches every row, decoded.
macro_rules! fetch {
    ($db:ty, $executor:expr, $sql:expr, $params:expr, $other:expr) => {{
        let mut query = sqlx::query::<$db>(AssertSqlSafe($sql.to_owned()));
        for param in $params {
            query = match param {
                SchemaValue::Null => query.bind(None::<String>),
                SchemaValue::Int(v) => query.bind(*v),
                SchemaValue::Float(v) => query.bind(*v),
                SchemaValue::Text(v) => query.bind(v.clone()),
                SchemaValue::Bytes(v) => query.bind(v.clone()),
                SchemaValue::Bool(v) => query.bind(*v),
            };
        }
        query.fetch_all($executor).await.map(|rows| {
            rows.iter()
                .map(|row| decode!(row, $other))
                .collect::<Vec<_>>()
        })
    }};
}

/// One row as a [`SchemaRow`]: each column decoded as the first type that
/// fits.
macro_rules! decode {
    ($row:expr, $other:expr) => {{
        let row = $row;
        let columns: Arc<[String]> = row
            .columns()
            .iter()
            .map(|c| c.name().to_owned())
            .collect::<Vec<_>>()
            .into();
        let mut values = Vec::with_capacity(columns.len());
        for at in 0..columns.len() {
            values.push(decode_value!(row, at, $other));
        }
        SchemaRow::new(columns, values)
    }};
}

macro_rules! decode_value {
    ($row:expr, $at:expr, $other:expr) => {{
        let row = $row;
        let at = $at;
        let raw = row.try_get_raw(at).expect("a column");
        let type_name = raw.type_info().name().to_owned();
        if raw.is_null() {
            SchemaValue::Null
        } else if let Ok(v) = row.try_get::<i64, _>(at) {
            SchemaValue::Int(v)
        } else if let Ok(v) = row.try_get::<i32, _>(at) {
            SchemaValue::Int(i64::from(v))
        } else if let Ok(v) = row.try_get::<i16, _>(at) {
            SchemaValue::Int(i64::from(v))
        } else if let Ok(v) = row.try_get::<i8, _>(at) {
            SchemaValue::Int(i64::from(v))
        } else if let Ok(v) = row.try_get::<bool, _>(at) {
            SchemaValue::Bool(v)
        } else if let Ok(v) = row.try_get::<f64, _>(at) {
            SchemaValue::Float(v)
        } else if let Ok(v) = row.try_get::<f32, _>(at) {
            SchemaValue::Float(f64::from(v))
        } else if let Ok(v) = row.try_get::<String, _>(at) {
            SchemaValue::Text(v)
        } else if let Ok(v) = row.try_get::<Vec<u8>, _>(at) {
            SchemaValue::Bytes(v)
        } else {
            $other(row, at).unwrap_or(SchemaValue::Text(format!("<{type_name}>")))
        }
    }};
}

/// What SQLite reads beyond the common types: nothing.
fn sqlite_other(_row: &sqlx::sqlite::SqliteRow, _at: usize) -> Option<SchemaValue> {
    None
}

/// PostgreSQL's decimals and object ids.
#[cfg(feature = "postgres")]
fn postgres_other(row: &sqlx::postgres::PgRow, at: usize) -> Option<SchemaValue> {
    if let Ok(v) = row.try_get::<sqlx::types::Decimal, _>(at) {
        return Some(SchemaValue::Text(v.to_string()));
    }
    row.try_get::<sqlx::postgres::types::Oid, _>(at)
        .ok()
        .map(|oid| SchemaValue::Int(i64::from(oid.0)))
}

/// MySQL's decimals.
#[cfg(feature = "mysql")]
fn mysql_other(row: &sqlx::mysql::MySqlRow, at: usize) -> Option<SchemaValue> {
    row.try_get::<sqlx::types::Decimal, _>(at)
        .ok()
        .map(|v| SchemaValue::Text(v.to_string()))
}

fn database_error(sql: &str, error: sqlx::Error) -> SchemaError {
    SchemaError::database(sql, error)
}

impl SchemaConnection for TestConn {
    fn backend(&self) -> Dialect {
        match self {
            TestConn::Sqlite(_) => Dialect::Sqlite,
            #[cfg(feature = "postgres")]
            TestConn::Postgres(_) => Dialect::Postgres,
            #[cfg(feature = "mysql")]
            TestConn::MySql(_) => Dialect::MySql,
        }
    }

    fn execute<'a>(&'a self, sql: &'a str) -> BoxFuture<'a, Result<(), SchemaError>> {
        Box::pin(async move {
            let statement = AssertSqlSafe(sql.to_owned());
            match self {
                TestConn::Sqlite(pool) => sqlx::raw_sql(statement).execute(pool).await.map(drop),
                #[cfg(feature = "postgres")]
                TestConn::Postgres(pool) => sqlx::raw_sql(statement).execute(pool).await.map(drop),
                #[cfg(feature = "mysql")]
                TestConn::MySql(pool) => sqlx::raw_sql(statement).execute(pool).await.map(drop),
            }
            .map_err(|error| database_error(sql, error))
        })
    }

    fn query<'a>(
        &'a self,
        sql: &'a str,
        params: &'a [SchemaValue],
    ) -> BoxFuture<'a, Result<Vec<SchemaRow>, SchemaError>> {
        Box::pin(async move {
            match self {
                TestConn::Sqlite(pool) => fetch!(sqlx::Sqlite, pool, sql, params, sqlite_other),
                #[cfg(feature = "postgres")]
                TestConn::Postgres(pool) => {
                    fetch!(sqlx::Postgres, pool, sql, params, postgres_other)
                }
                #[cfg(feature = "mysql")]
                TestConn::MySql(pool) => fetch!(sqlx::MySql, pool, sql, params, mysql_other),
            }
            .map_err(|error| database_error(sql, error))
        })
    }
}

impl SchemaConnection for TestTx {
    fn backend(&self) -> Dialect {
        self.dialect
    }

    fn execute<'a>(&'a self, sql: &'a str) -> BoxFuture<'a, Result<(), SchemaError>> {
        Box::pin(async move {
            let statement = AssertSqlSafe(sql.to_owned());
            let mut tx = self.tx.lock().await;
            match tx.as_mut().expect("open") {
                Tx::Sqlite(tx) => sqlx::raw_sql(statement).execute(&mut **tx).await.map(drop),
                #[cfg(feature = "postgres")]
                Tx::Postgres(tx) => sqlx::raw_sql(statement).execute(&mut **tx).await.map(drop),
                #[cfg(feature = "mysql")]
                Tx::MySql(tx) => sqlx::raw_sql(statement).execute(&mut **tx).await.map(drop),
            }
            .map_err(|error| database_error(sql, error))
        })
    }

    fn query<'a>(
        &'a self,
        sql: &'a str,
        params: &'a [SchemaValue],
    ) -> BoxFuture<'a, Result<Vec<SchemaRow>, SchemaError>> {
        Box::pin(async move {
            let mut tx = self.tx.lock().await;
            match tx.as_mut().expect("open") {
                Tx::Sqlite(tx) => fetch!(sqlx::Sqlite, &mut **tx, sql, params, sqlite_other),
                #[cfg(feature = "postgres")]
                Tx::Postgres(tx) => fetch!(sqlx::Postgres, &mut **tx, sql, params, postgres_other),
                #[cfg(feature = "mysql")]
                Tx::MySql(tx) => fetch!(sqlx::MySql, &mut **tx, sql, params, mysql_other),
            }
            .map_err(|error| database_error(sql, error))
        })
    }
}

/// Every row of `sql`, each column as text (`NULL` as `None`).
pub async fn text_rows(conn: &impl SchemaConnection, sql: &str) -> Vec<Vec<Option<String>>> {
    conn.query(sql, &[])
        .await
        .unwrap_or_else(|error| panic!("{sql}: {error}"))
        .iter()
        .map(|row| row.values().iter().map(SchemaValue::as_text).collect())
        .collect()
}
