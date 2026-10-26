//! The `search` migration set on SQLite: `search_documents` and its FTS5
//! table, through `migrate:fresh` and rollbacks.

use navree_db::Db;
use navree_search::{DOCUMENTS_TABLE, FTS_TABLE, Search};
use sea_orm::{ConnectionTrait, DbBackend, Statement};

use crate::support::{self, Contact, insert_contacts, new_contact};

async fn tables(db: &Db) -> Vec<String> {
    db.conn()
        .query_all_raw(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name LIKE 'search_%' ORDER BY name",
        ))
        .await
        .unwrap()
        .iter()
        .map(|row| row.try_get::<String>("", "name").unwrap())
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_search_tables_survive_fresh_and_rollback() {
    let app = support::app("postgres", "inline").await;
    let db = app.get::<Db>().await;
    let found = tables(&db).await;
    assert!(found.contains(&DOCUMENTS_TABLE.to_owned()), "{found:?}");
    assert!(found.contains(&FTS_TABLE.to_owned()), "{found:?}");

    insert_contacts(&db, vec![new_contact("Ada", "Lovelace")]).await;
    support::settle(&app).await;
    let search = app.get::<Search>().await;
    assert_eq!(search.query::<Contact>("ada").count().await.unwrap(), 1);

    app.run_command("migrate:fresh", &[]).await.unwrap();
    assert!(tables(&db).await.contains(&FTS_TABLE.to_owned()));
    assert_eq!(search.query::<Contact>("ada").count().await.unwrap(), 0);

    app.run_command("migrate:rollback", &[]).await.unwrap();
    assert!(tables(&db).await.is_empty(), "{:?}", tables(&db).await);
}
