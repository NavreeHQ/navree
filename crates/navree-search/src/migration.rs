//! The `search` migration set: the `postgres` engine's tables.
//!
//! The names are the ones the module's migrations always had, so a
//! database migrated by an earlier version finds nothing pending.

use navree_db::MigrationSet;
use navree_db::navree_schema::{Dialect, Migration, Schema, SchemaMigration, SchemaResult};

/// The name of the migration set `SearchModule` contributes.
pub const SEARCH_MIGRATIONS: &str = "search";

/// The documents of the `postgres` engine.
pub const DOCUMENTS_TABLE: &str = "search_documents";

/// The FTS5 table of the `postgres` engine on SQLite.
pub const FTS_TABLE: &str = "search_documents_fts";

pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new(SEARCH_MIGRATIONS)
        .migration(SchemaMigration::new(
            "m20261026_000001_create_search_documents",
            CreateSearchDocuments,
        ))
        .migration(SchemaMigration::new(
            "m20261028_000002_key_search_fts_by_rowid",
            KeyFtsByRowid,
        ))
}

/// The FTS5 table: one column per boost, the index name and id unindexed.
fn create_fts_table() -> String {
    format!(
        "CREATE VIRTUAL TABLE {FTS_TABLE} USING fts5(\
           index_name UNINDEXED, id UNINDEXED, \
           b1, b2, b3, b4, b5, b6, b7, b8, b9, b10, \
           tokenize = 'unicode61 remove_diacritics 2')"
    )
}

/// SQLite: gives each FTS row the rowid of its document, so the engine
/// finds and deletes it through the documents' primary key instead of
/// scanning the FTS table's `UNINDEXED` columns (which made an import
/// quadratic). Rows written before this migration are copied over; there is
/// nothing to do on PostgreSQL or MySQL.
struct KeyFtsByRowid;

impl Migration for KeyFtsByRowid {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        if schema.target().await?.dialect() != Dialect::Sqlite {
            return Ok(());
        }
        let old = format!("{FTS_TABLE}_unkeyed");
        schema
            .statement(&format!("ALTER TABLE {FTS_TABLE} RENAME TO {old}"))
            .await?;
        schema.statement(&create_fts_table()).await?;
        // Table names here are module constants (and a suffix of one); no input reaches this SQL.
        // nosemgrep: navree-sql-built-from-strings
        schema
            .statement(&format!(
                "INSERT INTO {FTS_TABLE} (rowid, index_name, id, b1, b2, b3, b4, b5, b6, b7, b8, b9, b10) \
                 SELECT d.rowid, o.index_name, o.id, o.b1, o.b2, o.b3, o.b4, o.b5, o.b6, o.b7, o.b8, o.b9, o.b10 \
                 FROM {old} o JOIN {DOCUMENTS_TABLE} d ON d.index_name = o.index_name AND d.id = o.id"
            ))
            .await?;
        schema.statement(&format!("DROP TABLE {old}")).await
    }

    async fn down(&self, _schema: &Schema<'_>) -> SchemaResult {
        // The keyed table works with the previous engine too.
        Ok(())
    }
}

/// Creates `search_documents`: on PostgreSQL with a weighted `tsvector`
/// and its GIN index; on SQLite with an FTS5 table beside it. On MySQL
/// it creates nothing: the `postgres` engine does not run there (the app
/// refuses it at boot), the other engines keep no tables.
pub(crate) struct CreateSearchDocuments;

impl Migration for CreateSearchDocuments {
    async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
        match schema.target().await?.dialect() {
            Dialect::Postgres => {
                schema
                    .statement(&format!(
                        "CREATE TABLE {DOCUMENTS_TABLE} (\
                           index_name VARCHAR(191) NOT NULL, \
                           id VARCHAR(191) NOT NULL, \
                           fields JSONB NOT NULL DEFAULT '{{}}', \
                           content TEXT NOT NULL DEFAULT '', \
                           search_vector TSVECTOR NOT NULL DEFAULT ''::tsvector, \
                           PRIMARY KEY (index_name, id))"
                    ))
                    .await?;
                schema
                    .statement(&format!(
                        "CREATE INDEX {DOCUMENTS_TABLE}_search_vector ON {DOCUMENTS_TABLE} USING GIN (search_vector)"
                    ))
                    .await
            }
            Dialect::Sqlite => {
                schema
                    .statement(&format!(
                        "CREATE TABLE {DOCUMENTS_TABLE} (\
                           index_name TEXT NOT NULL, \
                           id TEXT NOT NULL, \
                           fields TEXT NOT NULL DEFAULT '{{}}', \
                           PRIMARY KEY (index_name, id))"
                    ))
                    .await?;
                schema.statement(&create_fts_table()).await
            }
            Dialect::MySql => Ok(()),
        }
    }

    async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
        match schema.target().await?.dialect() {
            Dialect::Sqlite => {
                schema
                    .statement(&format!("DROP TABLE IF EXISTS {FTS_TABLE}"))
                    .await?;
            }
            Dialect::MySql => return Ok(()),
            Dialect::Postgres => {}
        }
        schema
            .statement(&format!("DROP TABLE IF EXISTS {DOCUMENTS_TABLE}"))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use navree_db::{DbOptions, ExecutorExt, Pool, Statement};

    async fn scalar(pool: &Pool, sql: &str) -> i64 {
        pool.fetch_scalar(&Statement::text(sql)).await.unwrap()
    }

    async fn run(pool: &Pool, sql: &str) {
        navree_db::Executor::execute_script(pool, sql)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn rows_written_before_the_rowid_migration_move_to_their_documents_rowids() {
        let pool = Pool::connect(&DbOptions {
            url: "sqlite::memory:".to_owned(),
            max_connections: 1,
            ..DbOptions::default()
        })
        .await
        .unwrap();
        let schema = Schema::new(&pool);
        CreateSearchDocuments.up(&schema).await.unwrap();
        // Three documents whose FTS rows have unrelated rowids, and an FTS
        // row without a document.
        for (id, title) in [("a", "alpha"), ("b", "beta"), ("c", "gamma")] {
            // Test data: literals from the loop above.
            // nosemgrep: navree-sql-built-from-strings
            run(
                &pool,
                &format!(
                    "INSERT INTO {DOCUMENTS_TABLE} (index_name, id, fields) VALUES ('notes', '{id}', '{{}}')"
                ),
            )
            .await;
            run(
                &pool,
                &format!(
                    "INSERT INTO {FTS_TABLE} (rowid, index_name, id, b1) VALUES ({}, 'notes', '{id}', '{title}')",
                    100 - i64::from(id.as_bytes()[0])
                ),
            )
            .await;
        }
        run(
            &pool,
            &format!(
                "INSERT INTO {FTS_TABLE} (rowid, index_name, id, b1) VALUES (500, 'notes', 'orphan', 'ghost')"
            ),
        )
        .await;

        KeyFtsByRowid.up(&schema).await.unwrap();

        assert_eq!(
            scalar(&pool, &format!("SELECT COUNT(*) FROM {FTS_TABLE}")).await,
            3,
            "the row without a document is dropped"
        );
        let aligned = scalar(
            &pool,
            &format!(
                "SELECT COUNT(*) FROM {FTS_TABLE} f JOIN {DOCUMENTS_TABLE} d ON d.rowid = f.rowid \
                 WHERE d.id = f.id"
            ),
        )
        .await;
        assert_eq!(aligned, 3);
        let found = scalar(
            &pool,
            &format!("SELECT COUNT(*) FROM {FTS_TABLE} WHERE {FTS_TABLE} MATCH 'beta'"),
        )
        .await;
        assert_eq!(found, 1);
    }
}
