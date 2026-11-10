//! `navree_search`: what a user types into a search box (query text) and
//! the values they put in filters, rendered for each engine (FTS5,
//! `to_tsquery`, Meilisearch and Typesense filter strings, SQL).
//!
//! Input: the query text, a NUL, the document text, a NUL, a JSON filter
//! value (optional), a NUL, stop words and synonyms (`word=a,b` lines).
//!
//! Invariants: without synonyms a rendered query holds only the user's
//! words, quoted for FTS5; a highlight is the document escaped with only the
//! configured tags added; filter values are bound in SQL and escaped in the
//! engines' filter languages; edit distance is symmetric.
#![no_main]

use std::collections::BTreeMap;

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let mut parts = text.split('\0');
    let mut next = || parts.next().unwrap_or("");
    let (query, document, value, settings) = (next(), next(), next(), next());

    let mut stop_words = Vec::new();
    let mut synonyms: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for line in settings.lines() {
        match line.split_once('=') {
            Some((word, alternatives)) => {
                synonyms.insert(
                    word.to_owned(),
                    alternatives.split(',').map(str::to_owned).collect(),
                );
            }
            None => stop_words.push(line.to_owned()),
        }
    }
    navree_search::fuzzing::query(query, document, &stop_words, &synonyms);
    if let Ok(value) = serde_json::from_str(value) {
        navree_search::fuzzing::filters(&value);
    }
    navree_search::fuzzing::filters(&serde_json::Value::String(query.to_owned()));
});
