//! The `search` migration set on SQLite: `search_documents` and its FTS5
//! table, through `migrate:fresh` and rollbacks. (PostgreSQL runs it in
//! `postgres.rs`, and every database in the facade's
//! `production_migrations`.)

use navree_db::{Db, Executor, ExecutorExt, Statement};
use navree_search::{DOCUMENTS_TABLE, FTS_TABLE, Search};

use crate::servers::Server;
use crate::support::{self, Contact, insert_contacts, new_contact};

/// Whether the suite runs on SQLite (else the test is skipped).
fn on_sqlite(test: &str) -> bool {
    support::runs_on(&[Server::Sqlite], test)
}

async fn tables(db: &Db) -> Vec<String> {
    db.fetch_column::<String>(&Statement::text(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name LIKE 'search_%' ORDER BY name",
    ))
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_search_tables_survive_fresh_and_rollback() {
    if !on_sqlite("the_search_tables_survive_fresh_and_rollback") {
        return;
    }
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

/// `(documents.rowid, fts.rowid)` pairs per id: they must be equal, so a
/// document's FTS row is found (and deleted) by rowid, not by scanning the
/// `UNINDEXED` columns.
async fn rowids(db: &Db) -> Vec<(String, i64, Option<i64>)> {
    db.fetch_all(&Statement::text(format!(
        "SELECT d.id AS id, d.rowid AS doc, f.rowid AS fts FROM {DOCUMENTS_TABLE} d \
         LEFT JOIN {FTS_TABLE} f ON f.rowid = d.rowid ORDER BY d.id"
    )))
    .await
    .unwrap()
    .iter()
    .map(|row| {
        (
            row.get("id").unwrap(),
            row.get("doc").unwrap(),
            row.get("fts").unwrap(),
        )
    })
    .collect()
}

async fn fts_rows(db: &Db) -> i64 {
    db.fetch_scalar(&Statement::text(format!(
        "SELECT COUNT(*) AS n FROM {FTS_TABLE}"
    )))
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fts_rows_are_keyed_by_the_documents_rowid() {
    use navree_search::{Document, IndexSettings};

    if !on_sqlite("fts_rows_are_keyed_by_the_documents_rowid") {
        return;
    }

    let app = support::app("postgres", "off").await;
    let db = app.get::<Db>().await;
    let search = app.get::<Search>().await;
    let engine = search.engine().clone();
    let settings = IndexSettings::new("Note", "notes").text("title", 1);
    let note = |id: &str, title: &str| Document::new(id).field("title", title);
    engine.prepare("notes", &settings).await.unwrap();
    engine.prepare("staged", &settings).await.unwrap();

    engine
        .upsert(
            "notes",
            &settings,
            vec![note("a", "alpha"), note("b", "beta"), note("c", "gamma")],
        )
        .await
        .unwrap();
    // Re-upserting replaces the row; the FTS row follows it.
    engine
        .upsert("notes", &settings, vec![note("b", "bravo")])
        .await
        .unwrap();
    for (id, doc, fts) in rowids(&db).await {
        assert_eq!(
            Some(doc),
            fts,
            "document {id} and its FTS row share a rowid"
        );
    }
    assert_eq!(fts_rows(&db).await, 3);

    engine.delete("notes", &["a".to_owned()]).await.unwrap();
    assert_eq!(
        fts_rows(&db).await,
        2,
        "deleting a document deletes its FTS row"
    );

    // Swapping a staged index in replaces the live one, FTS rows included.
    engine
        .upsert("staged", &settings, vec![note("z", "zulu")])
        .await
        .unwrap();
    engine.swap("notes", "staged").await.unwrap();
    assert_eq!(fts_rows(&db).await, 1);
    for (id, doc, fts) in rowids(&db).await {
        assert_eq!(Some(doc), fts, "document {id} after the swap");
    }

    engine.flush("notes").await.unwrap();
    assert_eq!(fts_rows(&db).await, 0);
    assert!(rowids(&db).await.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn importing_many_documents_keeps_one_fts_row_for_each_document() {
    use navree_search::{Document, IndexSettings};

    if !on_sqlite("importing_many_documents_keeps_one_fts_row_for_each_document") {
        return;
    }
    let app = support::app("postgres", "off").await;
    let db = app.get::<Db>().await;
    let search = app.get::<Search>().await;
    let engine = search.engine().clone();
    let settings = IndexSettings::new("Note", "notes").text("title", 1);
    engine.prepare("notes", &settings).await.unwrap();

    // Deleting a document's old FTS row by its two UNINDEXED columns scans
    // the whole table, so the import grows with the square of the index. The engine's
    // unit test reads the plan of every statement an import runs and fails on a scan; this
    // runs a whole import, with replaced documents among the new ones, and checks that it
    // leaves exactly one FTS row for each document, keyed by the document's rowid.
    for chunk in 0..4 {
        let documents = (0..500)
            .map(|n| Document::new(format!("n{chunk}-{n}")).field("title", format!("note {n}")))
            .collect();
        engine.upsert("notes", &settings, documents).await.unwrap();
    }
    assert_eq!(engine.count("notes").await.unwrap(), 2_000);
    let replaced = (0..500)
        .map(|n| Document::new(format!("n0-{n}")).field("title", format!("renamed {n}")))
        .collect();
    engine.upsert("notes", &settings, replaced).await.unwrap();
    assert_eq!(engine.count("notes").await.unwrap(), 2_000);
    assert_eq!(fts_rows(&db).await, 2_000);
    assert!(
        rowids(&db)
            .await
            .iter()
            .all(|(_, doc, fts)| Some(*doc) == *fts),
        "every document has its FTS row, under its own rowid"
    );
}
