//! `contains_ignore_case` / `eq_ignore_case` on a real PostgreSQL server:
//! case is ignored for every letter and accents are kept, whatever the
//! database's locale. A database whose character type is `C` (what
//! `initdb --locale=C` gives every database, a common production setting)
//! folds ASCII letters only in `LOWER`, `UPPER` and `ILIKE`, so the
//! helpers must not rely on them.
//!
//! Opt-in: build with `--features postgres` and set
//! `NAVREE_TEST_POSTGRES_URL` to a URL of a throwaway server whose user may
//! create databases, e.g.
//! `postgres://postgres:secret@127.0.0.1:55432/postgres`. Each test creates
//! its own database and drops it at the end.
#![cfg(feature = "postgres")]

use std::sync::atomic::{AtomicU32, Ordering};

use navree_db::{
    Db, DbOptions, Executor, ExecutorExt, Pool, Statement, contains_ignore_case, eq_ignore_case,
};

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A one-connection pool on `url`.
async fn connect(url: &str) -> Pool {
    Pool::connect(&DbOptions {
        url: url.to_owned(),
        max_connections: 1,
        ..DbOptions::default()
    })
    .await
    .unwrap()
}

/// A database of its own on the test server, with the `C` locale.
struct Scratch {
    admin: String,
    name: String,
    url: String,
}

impl Scratch {
    async fn new() -> Option<Scratch> {
        let Ok(admin) = std::env::var("NAVREE_TEST_POSTGRES_URL") else {
            eprintln!("NAVREE_TEST_POSTGRES_URL is not set: skipping the PostgreSQL test");
            return None;
        };
        let name = format!(
            "navree_text_{}_{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        );
        let conn = connect(&admin).await;
        conn.execute_script(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"))
            .await
            .unwrap();
        conn.execute_script(&format!(
            "CREATE DATABASE {name} TEMPLATE template0 ENCODING 'UTF8' LC_COLLATE 'C' LC_CTYPE 'C'"
        ))
        .await
        .unwrap();
        conn.close().await;
        let (base, _) = admin.rsplit_once('/').unwrap();
        let url = format!("{base}/{name}");
        Some(Scratch { admin, name, url })
    }

    async fn db(&self) -> Db {
        Db::connect(&DbOptions {
            url: self.url.clone(),
            ..DbOptions::default()
        })
        .await
        .unwrap()
    }

    async fn drop(self) {
        let conn = connect(&self.admin).await;
        conn.execute_script(&format!(
            "DROP DATABASE IF EXISTS {} WITH (FORCE)",
            self.name
        ))
        .await
        .unwrap();
        conn.close().await;
    }
}

/// The titles of `notes` matching `condition`, in title order.
async fn titles(db: &Db, condition: Statement) -> Vec<String> {
    db.fetch_column(&Statement::portable(
        format!(
            "SELECT title FROM notes WHERE {} ORDER BY title",
            condition.sql()
        ),
        condition.values().to_vec(),
    ))
    .await
    .unwrap()
}

#[tokio::test]
async fn non_ascii_text_matches_in_any_case_but_not_without_its_accents() {
    let Some(scratch) = Scratch::new().await else {
        return;
    };
    let db = scratch.db().await;
    // Checked in a task of its own, so the database is dropped even when
    // an assertion fails.
    let checked = tokio::spawn(case_is_ignored_and_accents_kept(db.clone())).await;
    db.close().await.unwrap();
    scratch.drop().await;
    if let Err(failure) = checked {
        std::panic::resume_unwind(failure.into_panic());
    }
}

/// The searches and comparisons, on a database with the `C` locale.
async fn case_is_ignored_and_accents_kept(db: Db) {
    db.execute_script("CREATE TABLE notes (title TEXT NOT NULL)")
        .await
        .unwrap();
    for title in [
        "Øresund Énergie",
        "Équipe Ünited Straße",
        "Ölwechsel Rahmenvertrag",
        "Acme 100% *real* (v2.0)",
        "ΟΔΟΣ ΑΘΗΝΑΣ",
        "under_score 50%off",
        "Release v2x0",
        "a.b|c^d$e+f{g}h[i]j\\k",
    ] {
        db.execute(&Statement::portable(
            "INSERT INTO notes (title) VALUES (?)",
            [title.into()],
        ))
        .await
        .unwrap();
    }
    // The database folds ASCII letters only: `LOWER` alone would miss these.
    let backend = db.backend();
    let column = backend.quote_identifier("title");
    let title = || column.as_str();
    for (term, expected) in [
        ("Øresund", vec!["Øresund Énergie"]),
        ("øresund", vec!["Øresund Énergie"]),
        ("ÉNERGIE", vec!["Øresund Énergie"]),
        ("équipe", vec!["Équipe Ünited Straße"]),
        ("ünited", vec!["Équipe Ünited Straße"]),
        ("STRAßE", vec!["Équipe Ünited Straße"]),
        ("ölwechsel", vec!["Ölwechsel Rahmenvertrag"]),
        ("rahmenVERTRAG", vec!["Ölwechsel Rahmenvertrag"]),
        ("ΟΔΟΣ", vec!["ΟΔΟΣ ΑΘΗΝΑΣ"]),
        ("αθηνασ", vec!["ΟΔΟΣ ΑΘΗΝΑΣ"]),
        // Wildcards and regular-expression characters match themselves.
        ("100%", vec!["Acme 100% *real* (v2.0)"]),
        ("*REAL* (V2.0)", vec!["Acme 100% *real* (v2.0)"]),
        ("r_s", vec!["under_score 50%off"]),
        ("50%OFF", vec!["under_score 50%off"]),
        ("a.b|c^d$e+f{g}h[i]j\\k", vec!["a.b|c^d$e+f{g}h[i]j\\k"]),
        ("e?u", vec![]),
        ("50_off", vec![]),
        ("v2.0", vec!["Acme 100% *real* (v2.0)"]),
        // Accents are kept.
        ("equipe", vec![]),
        ("energie", vec![]),
        ("oresund", vec![]),
    ] {
        let found = titles(&db, contains_ignore_case(backend, title(), term)).await;
        assert_eq!(found, expected, "search {term:?}");
    }
    for (value, matches) in [
        ("øresund énergie", 1),
        ("ØRESUND ÉNERGIE", 1),
        ("oresund energie", 0),
        ("øresund", 0),
        ("øresund énergie ", 0),
        ("ACME 100% *REAL* (V2.0)", 1),
    ] {
        let found = titles(&db, eq_ignore_case(backend, title(), value)).await;
        assert_eq!(found.len(), matches, "equals {value:?}");
    }
}
