//! `contains_ignore_case` / `eq_ignore_case` against a real SQLite database,
//! whose own `LOWER` and `LIKE` fold ASCII letters only.

use navree_db::{contains_ignore_case, eq_ignore_case};
use sea_orm::sea_query::Expr;
use sea_orm::{ActiveModelTrait, EntityTrait, QueryFilter, QueryOrder};

use crate::fixtures::{new_note, note, notes_db};
use crate::support::TempDb;

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
        new_note(title).insert(db.conn()).await.unwrap();
    }
    let backend = db.backend();
    let titles = |condition| {
        note::Entity::find()
            .filter(condition)
            .order_by_asc(note::Column::Title)
            .all(db.conn())
    };
    let title = || Expr::col(note::Column::Title);
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
        let found: Vec<String> = titles(contains_ignore_case(backend, title(), term))
            .await
            .unwrap()
            .into_iter()
            .map(|note| note.title)
            .collect();
        assert_eq!(found, expected, "search {term:?}");
    }
    let found = titles(eq_ignore_case(backend, title(), "øresund énergie"))
        .await
        .unwrap();
    assert_eq!(found.len(), 1);
    let found = titles(eq_ignore_case(backend, title(), "øresund"))
        .await
        .unwrap();
    assert!(found.is_empty());
}
