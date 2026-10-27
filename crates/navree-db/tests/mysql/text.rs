//! `contains_ignore_case` / `eq_ignore_case` on a real MySQL server: case
//! is ignored and accents are not, whatever the column's collation (the
//! same results as on SQLite and PostgreSQL).

use navree_db::{contains_ignore_case, eq_ignore_case};
use sea_orm::sea_query::{Alias, Expr, Query};
use sea_orm::{ActiveModelTrait, ConnectionTrait, EntityTrait, QueryFilter, QueryOrder};

use crate::fixtures::{create_notes, new_note, note};
use crate::support::Scratch;

#[tokio::test]
async fn non_ascii_text_matches_in_any_case_but_not_without_its_accents() {
    let Some(scratch) =
        Scratch::new("non_ascii_text_matches_in_any_case_but_not_without_its_accents").await
    else {
        return;
    };
    let db = scratch.db().await;
    create_notes(&db).await;
    for title in [
        "Øresund Énergie",
        "Équipe Ünited Straße",
        "Ölwechsel Rahmenvertrag",
        "Acme 100% *real*",
        "ΟΔΟΣ ΑΘΗΝΑΣ",
        "under_score 50%off",
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
        ("ΟΔΟΣ", vec!["ΟΔΟΣ ΑΘΗΝΑΣ"]),
        ("αθηνασ", vec!["ΟΔΟΣ ΑΘΗΝΑΣ"]),
        ("r_s", vec!["under_score 50%off"]),
        ("50%OFF", vec!["under_score 50%off"]),
        // Wildcards match themselves.
        ("e?u", vec![]),
        ("50_off", vec![]),
        // The default collation would ignore these accents; the helper does not.
        ("equipe", vec![]),
        ("energie", vec![]),
        ("oresund", vec![]),
    ] {
        let found: Vec<String> = titles(contains_ignore_case(backend, title(), term))
            .await
            .unwrap()
            .into_iter()
            .map(|note| note.title)
            .collect();
        assert_eq!(found, expected, "search {term:?}");
    }
    for (value, matches) in [
        ("øresund énergie", 1),
        ("ØRESUND ÉNERGIE", 1),
        ("oresund energie", 0),
        ("øresund", 0),
        ("øresund énergie ", 0),
    ] {
        let found = titles(eq_ignore_case(backend, title(), value))
            .await
            .unwrap();
        assert_eq!(found.len(), matches, "equals {value:?}");
    }
}

/// A `_bin` or case-sensitive collation does not make the helpers
/// case-sensitive.
#[tokio::test]
async fn case_is_ignored_under_binary_and_case_sensitive_collations() {
    let Some(scratch) =
        Scratch::new("case_is_ignored_under_binary_and_case_sensitive_collations").await
    else {
        return;
    };
    let db = scratch.db().await;
    db.conn()
        .execute_unprepared(
            "CREATE TABLE labels (\
                 bin VARCHAR(64) COLLATE utf8mb4_bin NOT NULL, \
                 cs VARCHAR(64) COLLATE utf8mb4_0900_as_cs NOT NULL)",
        )
        .await
        .unwrap();
    db.conn()
        .execute_unprepared("INSERT INTO labels (bin, cs) VALUES ('Équipe Nord', 'Équipe Nord')")
        .await
        .unwrap();
    let backend = db.backend();
    for column in ["bin", "cs"] {
        for (condition, matches) in [
            (
                contains_ignore_case(backend, Expr::col(Alias::new(column)), "ÉQUIPE"),
                1,
            ),
            (
                eq_ignore_case(backend, Expr::col(Alias::new(column)), "équipe nord"),
                1,
            ),
            (
                contains_ignore_case(backend, Expr::col(Alias::new(column)), "equipe"),
                0,
            ),
        ] {
            let rows = db
                .conn()
                .query_all(
                    Query::select()
                        .column(Alias::new(column))
                        .from(Alias::new("labels"))
                        .and_where(condition),
                )
                .await
                .unwrap();
            assert_eq!(rows.len(), matches, "{column}");
        }
    }
}
