//! Text handling shared by the engines: tokenizing, typo budgets, term
//! matching and highlighting.
//!
//! Every engine highlights through [`highlight`], so `Hit::highlights` has
//! the same shape (HTML-escaped text with the configured tags around the
//! matches) whichever engine answered.

use std::collections::BTreeMap;

/// A word of a text: its lower-cased form and its byte span in the text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Token {
    /// Lower-cased.
    pub(crate) text: String,
    /// Byte offset of the first character in the original text.
    pub(crate) start: usize,
    /// Byte offset after the last character.
    pub(crate) end: usize,
}

/// Splits `text` into words: runs of alphanumeric characters, lower-cased.
pub(crate) fn tokenize(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut start = None;
    for (i, c) in text.char_indices() {
        if c.is_alphanumeric() {
            start.get_or_insert(i);
        } else if let Some(s) = start.take() {
            tokens.push(token(text, s, i));
        }
    }
    if let Some(s) = start {
        tokens.push(token(text, s, text.len()));
    }
    tokens
}

fn token(text: &str, start: usize, end: usize) -> Token {
    Token {
        text: text[start..end].to_lowercase(),
        start,
        end,
    }
}

/// The lower-cased words of `text`.
pub(crate) fn words(text: &str) -> Vec<String> {
    tokenize(text).into_iter().map(|t| t.text).collect()
}

/// How many typos a query word of `len` characters may contain: none below
/// 5 characters, one up to 8, two from 9.
pub(crate) fn typo_budget(len: usize) -> usize {
    match len {
        0..=4 => 0,
        5..=8 => 1,
        _ => 2,
    }
}

/// The Levenshtein distance between `a` and `b` (by characters), or `None`
/// when it is larger than `max`.
pub(crate) fn distance_within(a: &str, b: &str, max: usize) -> Option<usize> {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len().abs_diff(b.len()) > max {
        return None;
    }
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        current[0] = i + 1;
        let mut row_min = current[0];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            current[j + 1] = (previous[j] + cost)
                .min(previous[j + 1] + 1)
                .min(current[j] + 1);
            row_min = row_min.min(current[j + 1]);
        }
        if row_min > max {
            return None;
        }
        std::mem::swap(&mut previous, &mut current);
    }
    let d = previous[b.len()];
    (d <= max).then_some(d)
}

/// How well a document word matches a query word, from 0 (not at all) to 1
/// (exactly).
///
/// - exact: 1.0;
/// - `prefix` and the word starts with the query word: 0.9;
/// - within the typo budget: 0.7 for one typo, 0.6 for two;
/// - `prefix` and a prefix of the word within the budget: 0.5.
pub(crate) fn match_quality(query: &str, word: &str, prefix: bool) -> f64 {
    if query == word {
        return 1.0;
    }
    if prefix && word.starts_with(query) {
        return 0.9;
    }
    let budget = typo_budget(query.chars().count());
    if budget == 0 {
        return 0.0;
    }
    if let Some(d) = distance_within(query, word, budget) {
        return if d <= 1 { 0.7 } else { 0.6 };
    }
    if prefix {
        let len = query.chars().count();
        let head: String = word.chars().take(len).collect();
        if head.chars().count() == len && distance_within(query, &head, budget).is_some() {
            return 0.5;
        }
    }
    0.0
}

/// A parsed query text: its words (stop words removed), each with the
/// alternatives its synonyms give.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct QueryTerms {
    /// One entry per query word, in order.
    pub(crate) terms: Vec<Term>,
}

/// One query word and its synonyms.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Term {
    /// The word, lower-cased.
    pub(crate) word: String,
    /// Whether it may match as a prefix (the last word of the query).
    pub(crate) prefix: bool,
    /// Alternatives from the index's synonyms, each a list of words that
    /// must all match.
    pub(crate) synonyms: Vec<Vec<String>>,
}

impl QueryTerms {
    /// Parses `text` with the index's `stop_words` and `synonyms`.
    pub(crate) fn parse(
        text: &str,
        stop_words: &[String],
        synonyms: &BTreeMap<String, Vec<String>>,
    ) -> QueryTerms {
        let stop: Vec<String> = stop_words.iter().map(|w| w.to_lowercase()).collect();
        let all = words(text);
        let kept: Vec<String> = all.iter().filter(|w| !stop.contains(w)).cloned().collect();
        // A query made only of stop words still searches for them.
        let kept = if kept.is_empty() { all } else { kept };
        let last = kept.len().saturating_sub(1);
        let terms = kept
            .into_iter()
            .enumerate()
            .map(|(i, word)| Term {
                synonyms: synonyms_of(&word, synonyms),
                prefix: i == last && !text.ends_with(char::is_whitespace),
                word,
            })
            .collect();
        QueryTerms { terms }
    }

    /// Whether there is nothing to match (an empty query matches everything).
    pub(crate) fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }
}

/// The synonyms of `word`: the configured alternatives, and every key that
/// lists `word` among its alternatives (synonyms work both ways).
fn synonyms_of(word: &str, synonyms: &BTreeMap<String, Vec<String>>) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = Vec::new();
    for (key, alternatives) in synonyms {
        let key_words = words(key);
        if key_words.len() == 1 && key_words[0] == word {
            for alternative in alternatives {
                let alt = words(alternative);
                if !alt.is_empty() && !out.contains(&alt) && alt != [word] {
                    out.push(alt);
                }
            }
        }
        if alternatives
            .iter()
            .any(|a| words(a).len() == 1 && words(a)[0] == word)
            && !key_words.is_empty()
            && key_words != [word]
            && !out.contains(&key_words)
        {
            out.push(key_words);
        }
    }
    out
}

/// The best quality with which `term` (or one of its synonyms) matches
/// `tokens`, and the indexes of the tokens that matched.
pub(crate) fn term_match(term: &Term, tokens: &[Token]) -> (f64, Vec<usize>) {
    let mut best = 0.0;
    let mut matched = Vec::new();
    for (i, token) in tokens.iter().enumerate() {
        let q = match_quality(&term.word, &token.text, term.prefix);
        if q > 0.0 {
            matched.push(i);
            if q > best {
                best = q;
            }
        }
    }
    for alternative in &term.synonyms {
        let mut indexes: Vec<usize> = Vec::new();
        let all = alternative.iter().all(|word| {
            let found: Vec<usize> = tokens
                .iter()
                .enumerate()
                .filter(|(_, t)| t.text == *word)
                .map(|(i, _)| i)
                .collect();
            indexes.extend(&found);
            !found.is_empty()
        });
        if all {
            matched.extend(indexes);
            if 0.95 > best {
                best = 0.95;
            }
        }
    }
    matched.sort_unstable();
    matched.dedup();
    (best, matched)
}

/// The tags put around matches in highlights.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HighlightTags {
    /// Before a match (default `<mark>`).
    pub open: String,
    /// After a match (default `</mark>`).
    pub close: String,
}

impl Default for HighlightTags {
    fn default() -> Self {
        Self {
            open: "<mark>".to_owned(),
            close: "</mark>".to_owned(),
        }
    }
}

/// `text` HTML-escaped, with `tags` around every word that matches one of
/// `terms`; `None` when nothing matches.
pub(crate) fn highlight(text: &str, terms: &QueryTerms, tags: &HighlightTags) -> Option<String> {
    let tokens = tokenize(text);
    let mut marked = vec![false; tokens.len()];
    for term in &terms.terms {
        for i in term_match(term, &tokens).1 {
            marked[i] = true;
        }
    }
    if !marked.iter().any(|m| *m) {
        return None;
    }
    let mut out = String::with_capacity(text.len() + 16);
    let mut at = 0;
    for (token, marked) in tokens.iter().zip(marked) {
        if !marked {
            continue;
        }
        out.push_str(&escape_html(&text[at..token.start]));
        out.push_str(&tags.open);
        out.push_str(&escape_html(&text[token.start..token.end]));
        out.push_str(&tags.close);
        at = token.end;
    }
    out.push_str(&escape_html(&text[at..]));
    Some(out)
}

/// Escapes `&`, `<`, `>`, `"` and `'`.
pub(crate) fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// Private-use characters that engines put around matches, so the text can
/// be escaped before the real tags go in.
pub(crate) const MARK_OPEN: &str = "\u{E000}";
/// See [`MARK_OPEN`].
pub(crate) const MARK_CLOSE: &str = "\u{E001}";

/// An engine's highlight made with [`MARK_OPEN`] / [`MARK_CLOSE`]: escaped,
/// with `tags`; `None` when it has no match.
pub(crate) fn from_marked(marked: &str, tags: &HighlightTags) -> Option<String> {
    if !marked.contains(MARK_OPEN) {
        return None;
    }
    Some(
        escape_html(marked)
            .replace(MARK_OPEN, &tags.open)
            .replace(MARK_CLOSE, &tags.close),
    )
}

/// The text of a document field for matching: strings as they are, numbers
/// and booleans in their JSON form, arrays joined by spaces; `None` for
/// null and objects.
pub(crate) fn field_text(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        serde_json::Value::Array(items) => {
            let parts: Vec<String> = items.iter().filter_map(field_text).collect();
            (!parts.is_empty()).then(|| parts.join(" "))
        }
        serde_json::Value::Null | serde_json::Value::Object(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terms(text: &str) -> QueryTerms {
        QueryTerms::parse(text, &[], &BTreeMap::new())
    }

    #[test]
    fn tokenize_splits_on_non_alphanumerics_and_lowercases() {
        let tokens = tokenize("Ada Lovelace, ada@lovelace.io");
        let words: Vec<&str> = tokens.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(words, ["ada", "lovelace", "ada", "lovelace", "io"]);
        assert_eq!((tokens[1].start, tokens[1].end), (4, 12));
        assert_eq!(words_of("Émile Zoë"), ["émile", "zoë"]);
    }

    fn words_of(text: &str) -> Vec<String> {
        words(text)
    }

    #[test]
    fn typo_budget_follows_word_length() {
        assert_eq!(typo_budget(4), 0);
        assert_eq!(typo_budget(5), 1);
        assert_eq!(typo_budget(8), 1);
        assert_eq!(typo_budget(9), 2);
    }

    #[test]
    fn distance_is_bounded() {
        assert_eq!(distance_within("lovlace", "lovelace", 1), Some(1));
        assert_eq!(distance_within("kitten", "sitting", 3), Some(3));
        assert_eq!(distance_within("kitten", "sitting", 2), None);
        assert_eq!(distance_within("a", "abcd", 2), None);
    }

    #[test]
    fn match_quality_ranks_exact_prefix_and_typos() {
        assert_eq!(match_quality("ada", "ada", false), 1.0);
        assert_eq!(match_quality("lov", "lovelace", true), 0.9);
        assert_eq!(match_quality("lov", "lovelace", false), 0.0);
        assert_eq!(match_quality("lovlace", "lovelace", false), 0.7);
        assert_eq!(
            match_quality("ada", "adb", false),
            0.0,
            "short words need exact"
        );
        assert_eq!(match_quality("lovek", "lovelace", true), 0.5);
    }

    #[test]
    fn the_last_word_is_a_prefix_unless_followed_by_a_space() {
        let parsed = terms("ada lov");
        assert!(!parsed.terms[0].prefix);
        assert!(parsed.terms[1].prefix);
        assert!(!terms("ada lov ").terms[1].prefix);
    }

    #[test]
    fn stop_words_are_dropped_and_synonyms_go_both_ways() {
        let synonyms = BTreeMap::from([("acme".to_owned(), vec!["acme corp".to_owned()])]);
        let parsed = QueryTerms::parse("the acme", &["the".to_owned()], &synonyms);
        assert_eq!(parsed.terms.len(), 1);
        assert_eq!(parsed.terms[0].synonyms, vec![vec!["acme", "corp"]]);

        let synonyms = BTreeMap::from([("nyc".to_owned(), vec!["manhattan".to_owned()])]);
        let parsed = QueryTerms::parse("manhattan", &[], &synonyms);
        assert_eq!(parsed.terms[0].synonyms, vec![vec!["nyc"]]);
        let only_stop = QueryTerms::parse("the", &["the".to_owned()], &BTreeMap::new());
        assert_eq!(only_stop.terms.len(), 1, "a query of stop words keeps them");
    }

    #[test]
    fn highlight_marks_matches_and_escapes_the_rest() {
        let tags = HighlightTags::default();
        assert_eq!(
            highlight("Lovelace", &terms("lovlace"), &tags).as_deref(),
            Some("<mark>Lovelace</mark>")
        );
        assert_eq!(
            highlight("Ada <b> & Lovelace", &terms("ada"), &tags).as_deref(),
            Some("<mark>Ada</mark> &lt;b&gt; &amp; Lovelace")
        );
        assert_eq!(highlight("Grace", &terms("ada"), &tags), None);
        let em = HighlightTags {
            open: "<em>".into(),
            close: "</em>".into(),
        };
        assert_eq!(
            highlight("ada lovelace", &terms("lov"), &em).as_deref(),
            Some("ada <em>lovelace</em>")
        );
    }

    #[test]
    fn engine_marks_become_escaped_tags() {
        let marked = format!("{MARK_OPEN}Ada{MARK_CLOSE} <3");
        assert_eq!(
            from_marked(&marked, &HighlightTags::default()).as_deref(),
            Some("<mark>Ada</mark> &lt;3")
        );
        assert_eq!(from_marked("plain", &HighlightTags::default()), None);
    }

    #[test]
    fn field_text_flattens_scalars_and_arrays() {
        use serde_json::json;
        assert_eq!(field_text(&json!("a")).as_deref(), Some("a"));
        assert_eq!(field_text(&json!(42)).as_deref(), Some("42"));
        assert_eq!(field_text(&json!(["a", "b"])).as_deref(), Some("a b"));
        assert_eq!(field_text(&json!(null)), None);
    }
}
