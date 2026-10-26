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
    MigrationSet::new(SEARCH_MIGRATIONS).migration(CreateSearchDocuments)
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
