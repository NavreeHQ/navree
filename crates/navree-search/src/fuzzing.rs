//! Entry points for the fuzz targets in `fuzz/` (feature `fuzzing`): the
//! query text parser and its renderings for the SQL engines, highlighting,
//! typo matching, and the filters' renderings for each engine. Not part of
//! the API.

use std::collections::BTreeMap;

use sea_orm::DbBackend;
use serde_json::Value;

use crate::engine::database::{fts5_query, tsquery, tsquery_any};
use crate::engine::meilisearch::filter_expression;
use crate::engine::typesense::filter_by;
use crate::filter::{CmpOp, Filter};
use crate::sql::Sql;
use crate::text::{
    HighlightTags, QueryTerms, distance_within, escape_html, from_marked, highlight,
    match_quality, term_match, tokenize,
};

/// Reads `text` as a user's search query and renders it for the SQL engines
/// and highlights `document` with it.
///
/// With no synonyms (they are developer settings, and may hold anything),
/// the renderings contain only the query's words: letters and digits inside
/// double quotes for FTS5, bare words for `to_tsquery`.
///
/// # Panics
///
/// When an invariant breaks.
pub fn query(
    text: &str,
    document: &str,
    stop_words: &[String],
    synonyms: &BTreeMap<String, Vec<String>>,
) {
    let terms = QueryTerms::parse(text, stop_words, synonyms);
    let _ = terms.is_empty();
    let tags = HighlightTags::default();
    let _ = highlight(document, &terms, &tags);
    let _ = fts5_query(&terms);
    let _ = tsquery(&terms);
    let _ = tsquery_any(&terms);

    let plain = QueryTerms::parse(text, &[], &BTreeMap::new());
    let fts5 = fts5_query(&plain);
    // Every quoted run is a word of letters and digits.
    let mut inside = false;
    for c in fts5.chars() {
        match c {
            '"' => inside = !inside,
            c if inside => assert!(c.is_alphanumeric(), "{c:?} inside quotes in {fts5:?}"),
            c => assert!(
                matches!(c, '*' | ' ' | '(' | ')' | 'A' | 'N' | 'D' | 'O' | 'R'),
                "{c:?} outside quotes in {fts5:?}"
            ),
        }
    }
    assert!(!inside, "unbalanced quotes in {fts5:?}");
    for rendering in [tsquery(&plain), tsquery_any(&plain)] {
        assert!(
            rendering
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, ':' | '*' | ' ' | '&' | '|')),
            "{rendering:?} is not a plain tsquery"
        );
    }

    // Highlights are the document escaped, with only our tags added.
    let document_escaped = escape_html(document);
    if let Some(marked) = highlight(document, &plain, &tags) {
        let stripped = marked.replace(&tags.open, "").replace(&tags.close, "");
        assert_eq!(stripped, document_escaped, "a highlight changed the text");
        assert!(!marked.contains(['<', '>']) || marked.contains("<mark>"));
    }
    assert!(!document_escaped.contains(['<', '>', '"', '\'']));
    let _ = from_marked(text, &tags);

    // Typo matching: symmetric, and bounded by the budget.
    let tokens = tokenize(document);
    for term in &plain.terms {
        let _ = term_match(term, &tokens);
        for token in tokens.iter().take(8) {
            let quality = match_quality(&term.word, &token.text, term.prefix);
            assert!((0.0..=1.0).contains(&quality));
            if let Some(distance) = distance_within(&term.word, &token.text, 2) {
                assert_eq!(
                    distance_within(&token.text, &term.word, 2),
                    Some(distance),
                    "the edit distance is not symmetric"
                );
            }
        }
    }
}

/// Renders filters on `value` (a value from a request) for every engine and
/// evaluates them.
///
/// # Panics
///
/// When an invariant breaks: a value must be bound in SQL (never written
/// into the statement) and escaped in Meilisearch's and Typesense's filter
/// strings.
pub fn filters(value: &Value) {
    let filters = shapes(value);
    // The SQL does not depend on the value, only on its type: whatever the
    // value holds, it is bound.
    let probe = match value {
        Value::String(_) => Some(Value::String("x".to_owned())),
        Value::Number(_) => Some(Value::from(1)),
        _ => None,
    };
    let probes = probe.as_ref().map(shapes);
    let fields: BTreeMap<String, Value> =
        BTreeMap::from([("status".to_owned(), value.clone())]);
    for (n, filter) in filters.into_iter().enumerate() {
        let _ = filter.fields();
        let _ = filter.matches(&fields);
        let filter = filter.normalized();
        let _ = filter.matches(&fields);

        let meilisearch = filter_expression(&filter);
        if let Filter::Cmp {
            op: CmpOp::Eq,
            value: Value::String(text),
            ..
        } = &filter
        {
            let expected = format!(
                "status = \"{}\"",
                text.replace('\\', "\\\\").replace('"', "\\\"")
            );
            assert_eq!(meilisearch, expected);
            // The literal ends where the filter ends: no unescaped quote
            // inside it.
            let body = &meilisearch["status = \"".len()..meilisearch.len() - 1];
            let mut escaped = false;
            for c in body.chars() {
                assert!(escaped || c != '"', "unescaped quote in {meilisearch:?}");
                escaped = !escaped && c == '\\';
            }
            assert!(!escaped, "a dangling backslash in {meilisearch:?}");
        }
        if let Ok(typesense) = filter_by(&filter)
            && let Filter::Cmp {
                op: CmpOp::Eq,
                value: Value::String(text),
                ..
            } = &filter
        {
            assert_eq!(typesense, format!("status:=`{text}`"));
            assert!(!text.contains('`'));
        }

        for backend in [DbBackend::Sqlite, DbBackend::Postgres] {
            let mut sql = Sql::new(backend);
            sql.filter(&filter, "fields");
            let placeholders = match backend {
                DbBackend::Postgres => (1..=sql.bound())
                    .all(|n| sql.text().contains(&format!("${n}"))),
                _ => sql.text().matches('?').count() == sql.bound(),
            };
            assert!(placeholders, "placeholders and values differ: {}", sql.text());
            if let Some(probes) = &probes {
                let mut expected = Sql::new(backend);
                expected.filter(&probes[n].clone().normalized(), "fields");
                assert_eq!(
                    sql.text(),
                    expected.text(),
                    "the SQL depends on the value {value}"
                );
            }
        }
    }
}

/// Filters on `value`, one of each shape.
fn shapes(value: &Value) -> [Filter; 8] {
    [
        Filter::eq("status", value),
        Filter::ne("status", value),
        Filter::is_in("status", [value, value]),
        Filter::range::<Value>("amount", value.clone()..),
        Filter::All(vec![
            Filter::eq("status", value),
            Filter::Any(vec![Filter::null("owner"), Filter::ne("stage", value)]),
        ]),
        Filter::Not(Box::new(Filter::eq("status", value))),
        Filter::NotIn {
            field: "status".to_owned(),
            values: vec![value.clone()],
        },
        Filter::Cmp {
            field: "amount".to_owned(),
            op: CmpOp::Lt,
            value: value.clone(),
        },
    ]
}
