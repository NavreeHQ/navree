//! The `search` migration set: the `postgres` engine's tables.

use navree_db::MigrationSet;
use sea_orm::{ConnectionTrait, DbBackend, DbErr};
use sea_orm_migration::async_trait::async_trait;
use sea_orm_migration::{MigrationName, MigrationTrait, SchemaManager};

/// The name of the migration set `SearchModule` contributes.
pub const SEARCH_MIGRATIONS: &str = "search";

/// The documents of the `postgres` engine.
pub const DOCUMENTS_TABLE: &str = "search_documents";

/// The FTS5 table of the `postgres` engine on SQLite.
pub const FTS_TABLE: &str = "search_documents_fts";

pub(crate) fn migrations() -> MigrationSet {
    MigrationSet::new(SEARCH_MIGRATIONS)
        .migration(CreateSearchDocuments)
        .migration(KeyFtsByRowid)
}

/// SQLite: gives each FTS row the rowid of its document, so the engine
/// finds and deletes it through the documents' primary key instead of
/// scanning the FTS table's `UNINDEXED` columns (which made an import
/// quadratic). Rows written before this migration are copied over; there is
/// nothing to do on PostgreSQL.
struct KeyFtsByRowid;

impl MigrationName for KeyFtsByRowid {
    fn name(&self) -> &str {
        "m20261028_000002_key_search_fts_by_rowid"
    }
}

#[async_trait]
impl MigrationTrait for KeyFtsByRowid {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        if manager.get_database_backend() != DbBackend::Sqlite {
            return Ok(());
        }
        let conn = manager.get_connection();
        let old = format!("{FTS_TABLE}_unkeyed");
        conn.execute_unprepared(&format!("ALTER TABLE {FTS_TABLE} RENAME TO {old}"))
            .await?;
        conn.execute_unprepared(&format!(
            "CREATE VIRTUAL TABLE {FTS_TABLE} USING fts5(\
               index_name UNINDEXED, id UNINDEXED, \
               b1, b2, b3, b4, b5, b6, b7, b8, b9, b10, \
               tokenize = 'unicode61 remove_diacritics 2')"
        ))
        .await?;
        // Table names here are module constants (and a suffix of one); no input reaches this SQL.
        // nosemgrep: navree-sql-built-from-strings
        conn.execute_unprepared(&format!(
            "INSERT INTO {FTS_TABLE} (rowid, index_name, id, b1, b2, b3, b4, b5, b6, b7, b8, b9, b10) \
             SELECT d.rowid, o.index_name, o.id, o.b1, o.b2, o.b3, o.b4, o.b5, o.b6, o.b7, o.b8, o.b9, o.b10 \
             FROM {old} o JOIN {DOCUMENTS_TABLE} d ON d.index_name = o.index_name AND d.id = o.id"
        ))
        .await?;
        conn.execute_unprepared(&format!("DROP TABLE {old}"))
            .await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // The keyed table works with the previous engine too.
        Ok(())
    }
}

/// Creates `search_documents`: on PostgreSQL with a weighted `tsvector`
/// and its GIN index; on SQLite with an FTS5 table beside it.
struct CreateSearchDocuments;

impl MigrationName for CreateSearchDocuments {
    fn name(&self) -> &str {
        "m20261026_000001_create_search_documents"
    }
}

#[async_trait]
impl MigrationTrait for CreateSearchDocuments {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        match manager.get_database_backend() {
            DbBackend::Postgres => {
                conn.execute_unprepared(&format!(
                    "CREATE TABLE {DOCUMENTS_TABLE} (\
                       index_name VARCHAR(191) NOT NULL, \
                       id VARCHAR(191) NOT NULL, \
                       fields JSONB NOT NULL DEFAULT '{{}}', \
                       content TEXT NOT NULL DEFAULT '', \
                       search_vector TSVECTOR NOT NULL DEFAULT ''::tsvector, \
                       PRIMARY KEY (index_name, id))"
                ))
                .await?;
                conn.execute_unprepared(&format!(
                    "CREATE INDEX {DOCUMENTS_TABLE}_search_vector ON {DOCUMENTS_TABLE} USING GIN (search_vector)"
                ))
                .await?;
            }
            DbBackend::Sqlite => {
                conn.execute_unprepared(&format!(
                    "CREATE TABLE {DOCUMENTS_TABLE} (\
                       index_name TEXT NOT NULL, \
                       id TEXT NOT NULL, \
                       fields TEXT NOT NULL DEFAULT '{{}}', \
                       PRIMARY KEY (index_name, id))"
                ))
                .await?;
                conn.execute_unprepared(&format!(
                    "CREATE VIRTUAL TABLE {FTS_TABLE} USING fts5(\
                       index_name UNINDEXED, id UNINDEXED, \
                       b1, b2, b3, b4, b5, b6, b7, b8, b9, b10, \
                       tokenize = 'unicode61 remove_diacritics 2')"
                ))
                .await?;
            }
            other => {
                return Err(DbErr::BackendNotSupported {
                    db: if other == DbBackend::MySql {
                        "MySQL"
                    } else {
                        "this database"
                    },
                    ctx: "navree-search",
                });
            }
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let conn = manager.get_connection();
        if manager.get_database_backend() == DbBackend::Sqlite {
            conn.execute_unprepared(&format!("DROP TABLE IF EXISTS {FTS_TABLE}"))
                .await?;
        }
        conn.execute_unprepared(&format!("DROP TABLE IF EXISTS {DOCUMENTS_TABLE}"))
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{Database, DatabaseConnection, Statement};

    async fn scalar(conn: &DatabaseConnection, sql: &str) -> i64 {
        conn.query_one_raw(Statement::from_string(DbBackend::Sqlite, sql))
            .await
            .unwrap()
            .unwrap()
            .try_get_by_index(0)
            .unwrap()
    }

    #[tokio::test]
    async fn rows_written_before_the_rowid_migration_move_to_their_documents_rowids() {
        let conn = Database::connect("sqlite::memory:").await.unwrap();
        let manager = SchemaManager::new(&conn);
        CreateSearchDocuments.up(&manager).await.unwrap();
        // Three documents whose FTS rows have unrelated rowids, and an FTS
        // row without a document.
        for (id, title) in [("a", "alpha"), ("b", "beta"), ("c", "gamma")] {
            // Test data: literals from the loop above.
            // nosemgrep: navree-sql-built-from-strings
            conn.execute_unprepared(&format!(
                "INSERT INTO {DOCUMENTS_TABLE} (index_name, id, fields) VALUES ('notes', '{id}', '{{}}')"
            ))
            .await
            .unwrap();
            conn.execute_unprepared(&format!(
                "INSERT INTO {FTS_TABLE} (rowid, index_name, id, b1) VALUES ({}, 'notes', '{id}', '{title}')",
                100 - i64::from(id.as_bytes()[0])
            ))
            .await
            .unwrap();
        }
        conn.execute_unprepared(&format!(
            "INSERT INTO {FTS_TABLE} (rowid, index_name, id, b1) VALUES (500, 'notes', 'orphan', 'ghost')"
        ))
        .await
        .unwrap();

        KeyFtsByRowid.up(&manager).await.unwrap();

        assert_eq!(
            scalar(&conn, &format!("SELECT COUNT(*) FROM {FTS_TABLE}")).await,
            3,
            "the row without a document is dropped"
        );
        let aligned = scalar(
            &conn,
            &format!(
                "SELECT COUNT(*) FROM {FTS_TABLE} f JOIN {DOCUMENTS_TABLE} d ON d.rowid = f.rowid \
                 WHERE d.id = f.id"
            ),
        )
        .await;
        assert_eq!(aligned, 3);
        let found = scalar(
            &conn,
            &format!("SELECT COUNT(*) FROM {FTS_TABLE} WHERE {FTS_TABLE} MATCH 'beta'"),
        )
        .await;
        assert_eq!(found, 1);
    }
}
