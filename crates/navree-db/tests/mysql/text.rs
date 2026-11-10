//! `contains_ignore_case` / `eq_ignore_case` on a real MySQL server: case
//! is ignored and accents are not, whatever the column's collation (the
//! same results as on SQLite and PostgreSQL).

use navree_db::{
    Db, Executor, ExecutorExt, Statement, contains_ignore_case, eq_ignore_case, record,
};

use crate::fixtures::{create_notes, new_note};
use crate::support::Scratch;

/// The values of `column` in `table` where `condition` holds, in order.
async fn matching(db: &Db, table: &str, column: &str, condition: Statement) -> Vec<String> {
    db.fetch_column(&Statement::portable(
        format!(
            "SELECT {column} FROM {table} WHERE {} ORDER BY {column}",
            condition.sql()
        ),
        condition.values().to_vec(),
    ))
    .await
    .unwrap()
}

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
        "Agra 100% *real*",
        "ΟΔΟΣ ΑΘΗΝΑΣ",
        "under_score 50%off",
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
        ("100%", vec!["Agra 100% *real*"]),
        ("*REAL*", vec!["Agra 100% *real*"]),
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
        let found = matching(
            &db,
            "notes",
            "title",
            contains_ignore_case(backend, &title, term),
        )
        .await;
        assert_eq!(found, expected, "search {term:?}");
    }
    for (value, matches) in [
        ("øresund énergie", 1),
        ("ØRESUND ÉNERGIE", 1),
        ("oresund energie", 0),
        ("øresund", 0),
        ("øresund énergie ", 0),
    ] {
        let found = matching(
            &db,
            "notes",
            "title",
            eq_ignore_case(backend, &title, value),
        )
        .await;
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
    // MySQL 8's case-sensitive collation, MariaDB 10.10's, or on an older
    // MariaDB (which has none for `utf8mb4`) its binary one without padding.
    let mut cs = "utf8mb4_nopad_bin";
    for known in ["utf8mb4_0900_as_cs", "utf8mb4_uca1400_as_cs"] {
        let count: i64 = db
            .fetch_scalar(&navree_db::Statement::text(format!(
                "SELECT COUNT(*) FROM information_schema.COLLATIONS WHERE COLLATION_NAME = '{known}'"
            )))
            .await
            .unwrap();
        if count > 0 {
            cs = known;
            break;
        }
    }
    db.execute_script(&format!(
        "CREATE TABLE labels (\
             bin VARCHAR(64) COLLATE utf8mb4_bin NOT NULL, \
             cs VARCHAR(64) COLLATE {cs} NOT NULL)"
    ))
    .await
    .unwrap();
    db.execute_script("INSERT INTO labels (bin, cs) VALUES ('Équipe Nord', 'Équipe Nord')")
        .await
        .unwrap();
    let backend = db.backend();
    for column in ["bin", "cs"] {
        let quoted = backend.quote_identifier(column);
        for (condition, matches) in [
            (contains_ignore_case(backend, &quoted, "ÉQUIPE"), 1),
            (eq_ignore_case(backend, &quoted, "équipe nord"), 1),
            (contains_ignore_case(backend, &quoted, "equipe"), 0),
        ] {
            let rows = matching(&db, "labels", &quoted, condition).await;
            assert_eq!(rows.len(), matches, "{column}");
        }
    }
}
