//! A small BM25 index over docs sections.

use std::collections::HashMap;

use serde::Serialize;

use super::{DocPage, DocsBundle, SITE};

/// BM25 parameters.
const K1: f64 = 1.2;
const B: f64 = 0.75;

/// Words too common to rank by.
const STOP_WORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "by", "can", "do", "does", "for", "from", "how", "i",
    "if", "in", "into", "is", "it", "its", "my", "of", "on", "or", "so", "that", "the", "their",
    "then", "this", "to", "what", "when", "where", "which", "who", "why", "with", "you", "your",
];

/// One section in the index.
#[derive(Debug)]
struct Entry {
    page: usize,
    section: usize,
    terms: HashMap<String, u32>,
    length: u32,
}

/// The index of a [`DocsBundle`]: one document per section, with the page
/// title (three times) and the section heading (twice) counted in.
#[derive(Debug)]
pub struct SearchIndex {
    entries: Vec<Entry>,
    document_frequency: HashMap<String, u32>,
    average_length: f64,
}

/// One search result.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SearchHit {
    /// The page slug (`ai/navigator`), for `read_doc`.
    pub slug: String,
    /// The page title.
    pub title: String,
    /// The section heading (empty for the text before the first heading).
    pub section: String,
    /// The section's address on the site.
    pub url: String,
    /// The passage that matched best.
    pub excerpt: String,
    /// Whether the page documents something not built yet.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub planned: bool,
    /// The BM25 score.
    pub score: f64,
}

/// Lower-cased words of letters, digits and `_`; `snake_case` words also
/// count their parts; a trailing plural `s` is dropped.
pub fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    for word in text
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|w| !w.is_empty())
    {
        let word = word.to_lowercase();
        if word.contains('_') {
            for part in word.split('_').filter(|p| p.len() > 1) {
                push_token(&mut tokens, part);
            }
        }
        push_token(&mut tokens, word.trim_matches('_'));
    }
    tokens
}

fn push_token(tokens: &mut Vec<String>, word: &str) {
    if word.is_empty() || STOP_WORDS.contains(&word) {
        return;
    }
    let stemmed = if word.len() > 3 && word.ends_with('s') && !word.ends_with("ss") {
        &word[..word.len() - 1]
    } else {
        word
    };
    tokens.push(stemmed.to_owned());
}

impl SearchIndex {
    /// Indexes every section of `bundle`.
    pub fn new(bundle: &DocsBundle) -> SearchIndex {
        let mut entries = Vec::new();
        let mut document_frequency: HashMap<String, u32> = HashMap::new();
        for (p, page) in bundle.pages.iter().enumerate() {
            let title = tokenize(&page.title);
            for (s, section) in page.sections.iter().enumerate() {
                let mut all = Vec::new();
                for _ in 0..3 {
                    all.extend(title.iter().cloned());
                }
                let heading = tokenize(&section.heading);
                for _ in 0..2 {
                    all.extend(heading.iter().cloned());
                }
                if s == 0 {
                    all.extend(tokenize(&page.description));
                }
                all.extend(tokenize(&section.markdown));
                let mut terms: HashMap<String, u32> = HashMap::new();
                for term in &all {
                    *terms.entry(term.clone()).or_default() += 1;
                }
                for term in terms.keys() {
                    *document_frequency.entry(term.clone()).or_default() += 1;
                }
                entries.push(Entry {
                    page: p,
                    section: s,
                    length: u32::try_from(all.len()).unwrap_or(u32::MAX),
                    terms,
                });
            }
        }
        let average_length = if entries.is_empty() {
            1.0
        } else {
            entries.iter().map(|e| f64::from(e.length)).sum::<f64>() / entries.len() as f64
        };
        SearchIndex {
            entries,
            document_frequency,
            average_length,
        }
    }

    /// The best `limit` sections for `query`, at most two per page.
    pub fn search(&self, bundle: &DocsBundle, query: &str, limit: usize) -> Vec<SearchHit> {
        let mut terms = tokenize(query);
        terms.sort();
        terms.dedup();
        if terms.is_empty() {
            return Vec::new();
        }
        let count = self.entries.len() as f64;
        let mut scored: Vec<(f64, &Entry)> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let mut score = 0.0;
                for term in &terms {
                    let Some(&tf) = entry.terms.get(term) else {
                        continue;
                    };
                    let df = f64::from(self.document_frequency[term]);
                    let idf = ((count - df + 0.5) / (df + 0.5) + 1.0).ln();
                    let tf = f64::from(tf);
                    let norm = 1.0 - B + B * f64::from(entry.length) / self.average_length;
                    score += idf * tf * (K1 + 1.0) / (tf + K1 * norm);
                }
                (score > 0.0).then_some((score, entry))
            })
            .collect();
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        let mut per_page: HashMap<usize, usize> = HashMap::new();
        let mut hits = Vec::new();
        for (score, entry) in scored {
            let seen = per_page.entry(entry.page).or_default();
            if *seen >= 2 {
                continue;
            }
            *seen += 1;
            hits.push(hit(&bundle.pages[entry.page], entry.section, &terms, score));
            if hits.len() == limit {
                break;
            }
        }
        hits
    }
}

fn hit(page: &DocPage, section: usize, terms: &[String], score: f64) -> SearchHit {
    let section = &page.sections[section];
    let anchor = if section.anchor.is_empty() {
        String::new()
    } else {
        format!("#{}", section.anchor)
    };
    SearchHit {
        slug: page.slug.clone(),
        title: page.title.clone(),
        section: section.heading.clone(),
        url: format!("{SITE}/docs/{}/{anchor}", page.slug),
        excerpt: excerpt(&section.markdown, terms),
        planned: page.planned,
        score: (score * 1000.0).round() / 1000.0,
    }
}

/// About 300 characters of `markdown` around the first line that holds a
/// query term (whole lines, code fences skipped).
fn excerpt(markdown: &str, terms: &[String]) -> String {
    let lines: Vec<&str> = markdown
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with("```"))
        .collect();
    let start = lines
        .iter()
        .position(|line| {
            let tokens = tokenize(line);
            terms.iter().any(|t| tokens.contains(t))
        })
        .unwrap_or(0);
    let mut out = String::new();
    for line in &lines[start..] {
        if out.len() > 300 {
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(line.trim());
    }
    if out.chars().count() > 400 {
        out = out.chars().take(400).collect::<String>() + "...";
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docs::{DocSection, DocsBundle};

    fn bundle() -> DocsBundle {
        let page = |slug: &str, title: &str, sections: &[(&str, &str)]| DocPage {
            slug: slug.to_owned(),
            title: title.to_owned(),
            description: String::new(),
            planned: false,
            sections: sections
                .iter()
                .map(|(heading, markdown)| DocSection {
                    heading: (*heading).to_owned(),
                    anchor: crate::docs::mdx::slugify(heading),
                    markdown: (*markdown).to_owned(),
                })
                .collect(),
        };
        DocsBundle {
            version: "0.1.0".to_owned(),
            pages: vec![
                page(
                    "basics/validation",
                    "Validation",
                    &[
                        ("Introduction", "Validate request bodies with garde DTOs."),
                        ("Unique rules", "Use `AsyncRule::unique` to check the database."),
                    ],
                ),
                page(
                    "database/migrations",
                    "Migrations",
                    &[
                        ("Introduction", "Migrations change the schema."),
                        ("Running migrations", "Run `navree migrate` to apply pending migrations."),
                    ],
                ),
                page("queues/jobs", "Queues", &[("Workers", "Run `navree queue:work`.")]),
            ],
        }
    }

    #[test]
    fn tokens_are_lower_case_words_with_snake_case_parts() {
        assert_eq!(tokenize("The list_routes Tools!"), ["list", "route", "list_route", "tool"]);
        assert_eq!(tokenize("class"), ["class"]);
    }

    #[test]
    fn the_best_sections_come_first() {
        let bundle = bundle();
        let index = SearchIndex::new(&bundle);
        let hits = index.search(&bundle, "how do I run pending migrations", 5);
        assert_eq!(hits[0].slug, "database/migrations");
        assert_eq!(hits[0].section, "Running migrations");
        assert_eq!(
            hits[0].url,
            "https://navree.com/docs/database/migrations/#running-migrations"
        );
        assert!(hits[0].excerpt.contains("navree migrate"), "{}", hits[0].excerpt);

        let hits = index.search(&bundle, "unique validation", 5);
        assert_eq!(hits[0].slug, "basics/validation");
        assert!(index.search(&bundle, "the of", 5).is_empty());
        assert!(index.search(&bundle, "kubernetes", 5).is_empty());
    }

    #[test]
    fn at_most_two_sections_per_page() {
        let bundle = bundle();
        let index = SearchIndex::new(&bundle);
        let hits = index.search(&bundle, "migrations", 10);
        assert_eq!(hits.iter().filter(|h| h.slug == "database/migrations").count(), 2);
        assert_eq!(index.search(&bundle, "migrations", 1).len(), 1);
    }
}
