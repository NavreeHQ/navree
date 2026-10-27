//! Databases and apps for the attribute tests: a fresh SQLite file per
//! test with the tables a test file declares.

#![allow(dead_code)]

use std::sync::Arc;

use navree_crypto::{CryptoModule, CryptoOptions, HashingOptions, generate_key};
use navree_db::sea_orm::ConnectionTrait;
use navree_db::{DatabaseModule, Db, DbOptions};
use navree_testing::TestApp;
use tempfile::TempDir;

/// A SQLite database of its own. Keep it alive for the test: dropping it
/// deletes the file.
pub struct Sqlite {
    pub db: Db,
    _dir: TempDir,
}

/// Runs each statement of `schema`.
pub async fn create(db: &Db, schema: &str) {
    for statement in schema.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        db.conn()
            .execute_unprepared(statement)
            .await
            .unwrap_or_else(|error| panic!("{statement}: {error}"));
    }
}

/// A fresh SQLite database (no app) with `schema`'s tables.
pub async fn sqlite(schema: &str) -> Sqlite {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let url = format!(
        "sqlite://{}?mode=rwc",
        dir.path().join("attributes.db").display()
    );
    let db = Db::connect(&DbOptions {
        url,
        ..DbOptions::default()
    })
    .await
    .expect("connects");
    create(&db, schema).await;
    Sqlite { db, _dir: dir }
}

/// Fast hashing for tests (the defaults cost 19 MiB and two passes).
pub fn fast_hashing() -> HashingOptions {
    HashingOptions {
        memory_kib: 1024,
        iterations: 1,
        parallelism: 1,
        max_concurrent_hashes: Some(4),
    }
}

/// An app with `CryptoModule` (key `key`, previous keys `previous`, fast
/// hashing) and `DatabaseModule` on a fresh SQLite file with `schema`'s
/// tables.
pub async fn crypto_app_with(schema: &str, key: String, previous: &[String]) -> (TestApp, Arc<Db>) {
    let mut options = CryptoOptions::new(key).hashing(fast_hashing());
    for old in previous {
        options = options.previous_key(old.clone());
    }
    let app = TestApp::builder()
        .temp_database()
        .module(CryptoModule::for_root(options))
        .module(DatabaseModule::from_config())
        .build()
        .await;
    let db = app.get::<Db>().await;
    create(&db, schema).await;
    (app, db)
}

/// An app with `CryptoModule` (a fresh key, fast hashing) and
/// `DatabaseModule` on a fresh SQLite file with `schema`'s tables.
pub async fn crypto_app(schema: &str) -> (TestApp, Arc<Db>) {
    crypto_app_with(schema, generate_key(), &[]).await
}

/// The text stored in `column` of the only row of `table`.
pub async fn stored_text(db: &Db, table: &str, column: &str) -> Option<String> {
    navree_orm::table(table)
        .select([column])
        .first(db)
        .await
        .unwrap()
        .expect("a row")
        .get::<Option<String>>(column)
        .unwrap()
}
