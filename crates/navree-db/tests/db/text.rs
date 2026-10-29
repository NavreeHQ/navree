//! `contains_ignore_case` / `eq_ignore_case` against a real SQLite database,
//! whose own `LOWER` and `LIKE` fold ASCII letters only.

use navree_db::{Db, ExecutorExt, Statement, contains_ignore_case, eq_ignore_case, record};

use crate::fixtures::{new_note, notes_db};
use crate::support::TempDb;

/// The titles of the notes matching `condition`, in order.
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
async fn non_ascii_text_matches_in_any_case() {
    let temp = TempDb::new();
    let db = notes_db(&temp).await;
    for title in [
        "Øresund Énergie",
        "Équipe Ünited Straße",
        "Ölwechsel Rahmenvertrag",
        "Acme 100% *real*",
    ] {
        record::insert(&db, &new_note(title)).await.unwrap();
    }
    let backend = db.backend();
    let title = backend.quote_identifier("title");
    for (term, expected) in [
        ("Øresund", vec!["Øresund Énergie"]),
        ("øresund", vec!["Øresund Énergie"]),
        ("ÉNERGIE", vec!["Øresund Énergie"]),
        ("équipe", vec!["Équipe Ünited Straße"]),
        ("ünited", vec!["Équipe Ünited Straße"]),
        ("STRAßE", vec!["Équipe Ünited Straße"]),
        ("ölwechsel", vec!["Ölwechsel Rahmenvertrag"]),
        ("rahmenVERTRAG", vec!["Ölwechsel Rahmenvertrag"]),
        ("100%", vec!["Acme 100% *real*"]),
        ("*REAL*", vec!["Acme 100% *real*"]),
        ("*", vec!["Acme 100% *real*"]),
        ("e?u", vec![]),
    ] {
        let found = titles(&db, contains_ignore_case(backend, &title, term)).await;
        assert_eq!(found, expected, "search {term:?}");
    }
    let found = titles(&db, eq_ignore_case(backend, &title, "øresund énergie")).await;
    assert_eq!(found.len(), 1);
    let found = titles(&db, eq_ignore_case(backend, &title, "øresund")).await;
    assert!(found.is_empty());
}
