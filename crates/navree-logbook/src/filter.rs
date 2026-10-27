//! The filter language of the search box: [`Filter`] and its [`Term`]s.

use std::fmt;

use crate::{Entry, EntryKind};

/// A parsed search: every term must match (`type:` terms are alternatives).
///
/// ```
/// use navree_logbook::{Filter, Term, StatusMatch};
///
/// let filter = Filter::parse("user:0192f0c1 status:4xx route:/api/v1/deals/{id}").unwrap();
/// assert_eq!(filter.terms()[1], Term::Status(StatusMatch::Class(4)));
/// assert_eq!(filter.to_string(), "user:0192f0c1 status:4xx route:/api/v1/deals/{id}");
/// ```
///
/// | Term | Matches |
/// |---|---|
/// | free text, or `"quoted text"` | paths, SQL, job types, event names, mail subjects and recipients, log messages |
/// | `user:<id>` | entries of one user (a prefix of the id is enough) |
/// | `tag:<tag>` | entries whose batch has a tag starting with `<tag>` |
/// | `status:500`, `status:4xx` | requests by status or status class |
/// | `route:<template>` | requests to one route |
/// | `type:<kind>` | one kind of entry (several `type:` terms: any of them) |
/// | `slow` | slow queries, and batches that ran one |
/// | `failed` | failed requests (5xx), jobs, queries and tasks |
/// | `duration>200`, `duration<5` | entries that took longer (shorter) than this many ms |
/// | `batch:<id>` | everything inside one batch (a prefix is enough) |
/// | `trace:<id>` | everything in one trace (a prefix is enough) |
///
/// Without `type:`, `batch:` or `trace:` the list shows only batches and
/// entries outside a batch.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Filter {
    terms: Vec<Term>,
}

/// One term of a [`Filter`].
#[derive(Clone, Debug, PartialEq)]
pub enum Term {
    /// Free text (case-insensitive).
    Text(String),
    /// `user:<prefix>`.
    User(String),
    /// `tag:<prefix>`.
    Tag(String),
    /// `status:<code>` or `status:<n>xx`.
    Status(StatusMatch),
    /// `route:<template>`.
    Route(String),
    /// `type:<kind>`.
    Type(EntryKind),
    /// `slow`.
    Slow,
    /// `failed`.
    Failed,
    /// `duration>ms` or `duration<ms`.
    Duration(Comparison, f64),
    /// `batch:<prefix>`.
    Batch(String),
    /// `trace:<prefix>`.
    Trace(String),
}

/// A status term.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusMatch {
    /// One status (`status:404`).
    Exact(u16),
    /// A class (`status:4xx` is `Class(4)`).
    Class(u8),
}

impl StatusMatch {
    /// Whether `status` matches.
    pub fn matches(self, status: u16) -> bool {
        match self {
            StatusMatch::Exact(code) => status == code,
            StatusMatch::Class(class) => status / 100 == u16::from(class),
        }
    }
}

/// The comparison of a duration term.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Comparison {
    /// `duration>ms`.
    Longer,
    /// `duration<ms`.
    Shorter,
}

/// A term that does not parse; the message names it.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct FilterError(pub String);

impl Filter {
    /// No terms: the unfiltered list.
    pub fn all() -> Filter {
        Filter::default()
    }

    /// A filter of these terms.
    pub fn of(terms: impl IntoIterator<Item = Term>) -> Filter {
        Filter {
            terms: terms.into_iter().collect(),
        }
    }

    /// Parses the search box's text.
    pub fn parse(input: &str) -> Result<Filter, FilterError> {
        let mut terms = Vec::new();
        let mut rest = input.trim_start();
        while !rest.is_empty() {
            if let Some(quoted) = rest.strip_prefix('"') {
                let (text, after) = match quoted.find('"') {
                    Some(end) => (&quoted[..end], &quoted[end + 1..]),
                    None => (quoted, ""),
                };
                if !text.trim().is_empty() {
                    terms.push(Term::Text(text.to_owned()));
                }
                rest = after.trim_start();
                continue;
            }
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            terms.push(parse_term(&rest[..end])?);
            rest = rest[end..].trim_start();
        }
        Ok(Filter { terms })
    }

    /// The terms.
    pub fn terms(&self) -> &[Term] {
        &self.terms
    }

    /// Whether there is no term.
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    /// This filter plus `term`.
    #[must_use]
    pub fn and(mut self, term: Term) -> Filter {
        self.terms.push(term);
        self
    }

    /// The kinds of the `type:` terms (empty: any kind).
    pub fn kinds(&self) -> Vec<EntryKind> {
        self.terms
            .iter()
            .filter_map(|term| match term {
                Term::Type(kind) => Some(*kind),
                _ => None,
            })
            .collect()
    }

    /// Whether only batches and entries outside a batch match: no
    /// `type:`, `batch:` or `trace:` term.
    pub fn top_level_only(&self) -> bool {
        !self
            .terms
            .iter()
            .any(|term| matches!(term, Term::Type(_) | Term::Batch(_) | Term::Trace(_)))
    }

    /// Whether `entry` matches (its `tags` are its batch's tags).
    pub fn matches(&self, entry: &Entry) -> bool {
        if self.top_level_only() && !entry.is_top_level() {
            return false;
        }
        let kinds = self.kinds();
        if !kinds.is_empty() && !kinds.contains(&entry.kind()) {
            return false;
        }
        self.terms.iter().all(|term| term_matches(term, entry))
    }
}

/// Whether `entry` matches one term (`type:` terms are checked together).
fn term_matches(term: &Term, entry: &Entry) -> bool {
    match term {
        Term::Text(text) => entry
            .search_text()
            .to_lowercase()
            .contains(&text.to_lowercase()),
        Term::User(prefix) => starts_with_ci(entry.user_id.as_deref(), prefix),
        Term::Tag(prefix) => entry
            .tags
            .iter()
            .any(|tag| starts_with_ci(Some(tag), prefix)),
        Term::Status(status) => entry.status().is_some_and(|code| status.matches(code)),
        Term::Route(route) => entry.route() == Some(route.as_str()),
        Term::Type(_) => true,
        Term::Slow => entry.slow,
        Term::Failed => entry.failed,
        Term::Duration(Comparison::Longer, ms) => entry.duration_ms.is_some_and(|d| d > *ms),
        Term::Duration(Comparison::Shorter, ms) => entry.duration_ms.is_some_and(|d| d < *ms),
        Term::Batch(prefix) => starts_with_ci(entry.batch_id.as_deref(), prefix),
        Term::Trace(prefix) => starts_with_ci(entry.trace_id.as_deref(), prefix),
    }
}

/// Whether `value` starts with `prefix`, ASCII case-insensitively.
pub(crate) fn starts_with_ci(value: Option<&str>, prefix: &str) -> bool {
    value.is_some_and(|value| {
        value.len() >= prefix.len()
            && value.is_char_boundary(prefix.len())
            && value[..prefix.len()].eq_ignore_ascii_case(prefix)
    })
}

/// One unquoted token.
fn parse_term(token: &str) -> Result<Term, FilterError> {
    let lower = token.to_ascii_lowercase();
    if lower == "slow" {
        return Ok(Term::Slow);
    }
    if lower == "failed" {
        return Ok(Term::Failed);
    }
    for (op, comparison) in [(">", Comparison::Longer), ("<", Comparison::Shorter)] {
        if let Some(value) = lower.strip_prefix(&format!("duration{op}")) {
            let number = value.strip_suffix("ms").unwrap_or(value);
            return match number.parse::<f64>() {
                Ok(ms) if ms.is_finite() && ms >= 0.0 => Ok(Term::Duration(comparison, ms)),
                _ => Err(FilterError(format!(
                    "`{token}`: a duration is a number of milliseconds, like duration>200"
                ))),
            };
        }
    }
    let Some((key, value)) = token.split_once(':') else {
        return Ok(Term::Text(token.to_owned()));
    };
    let key = key.to_ascii_lowercase();
    if !KEYS.contains(&key.as_str()) {
        return Ok(Term::Text(token.to_owned()));
    }
    if value.is_empty() {
        return Err(FilterError(format!(
            "`{token}` needs a value after `{key}:`"
        )));
    }
    let value = value.to_owned();
    Ok(match key.as_str() {
        "user" => Term::User(value),
        "tag" => Term::Tag(value),
        "route" => Term::Route(value),
        "batch" => Term::Batch(value),
        "trace" => Term::Trace(value),
        "type" => Term::Type(EntryKind::parse(&value).ok_or_else(|| {
            FilterError(format!(
                "`{token}`: unknown type; use one of {}",
                EntryKind::ALL.map(EntryKind::as_str).join(", ")
            ))
        })?),
        _ => Term::Status(parse_status(&value).ok_or_else(|| {
            FilterError(format!(
                "`{token}`: a status is a code like 404 or a class like 4xx"
            ))
        })?),
    })
}

/// The term keys (`key:value`).
const KEYS: [&str; 7] = ["user", "tag", "status", "route", "type", "batch", "trace"];

fn parse_status(value: &str) -> Option<StatusMatch> {
    let lower = value.to_ascii_lowercase();
    if let Some(class) = lower.strip_suffix("xx") {
        let class: u8 = class.parse().ok()?;
        return (1..=5)
            .contains(&class)
            .then_some(StatusMatch::Class(class));
    }
    let code: u16 = lower.parse().ok()?;
    (100..=599)
        .contains(&code)
        .then_some(StatusMatch::Exact(code))
}

impl fmt::Display for Filter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, term) in self.terms.iter().enumerate() {
            if i > 0 {
                f.write_str(" ")?;
            }
            write!(f, "{term}")?;
        }
        Ok(())
    }
}

impl fmt::Display for Term {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Term::Text(text) => {
                let plain = !text.is_empty()
                    && !text.contains(char::is_whitespace)
                    && !text.starts_with('"')
                    && matches!(parse_term(text), Ok(Term::Text(ref t)) if t == text);
                if plain {
                    f.write_str(text)
                } else {
                    write!(f, "\"{text}\"")
                }
            }
            Term::User(v) => write!(f, "user:{v}"),
            Term::Tag(v) => write!(f, "tag:{v}"),
            Term::Status(StatusMatch::Exact(code)) => write!(f, "status:{code}"),
            Term::Status(StatusMatch::Class(class)) => write!(f, "status:{class}xx"),
            Term::Route(v) => write!(f, "route:{v}"),
            Term::Type(kind) => write!(f, "type:{kind}"),
            Term::Slow => f.write_str("slow"),
            Term::Failed => f.write_str("failed"),
            Term::Duration(Comparison::Longer, ms) => write!(f, "duration>{ms}"),
            Term::Duration(Comparison::Shorter, ms) => write!(f, "duration<{ms}"),
            Term::Batch(v) => write!(f, "batch:{v}"),
            Term::Trace(v) => write!(f, "trace:{v}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_documented_term() {
        let filter = Filter::parse(
            "user:0192f0c1 tag:deal:0199c1d2 status:500 status:4xx route:/api/v1/deals/{id} \
             type:query slow failed duration>200 batch:01a17749 trace:4bf9 \"two words\" deals",
        )
        .unwrap();
        assert_eq!(
            filter.terms(),
            [
                Term::User("0192f0c1".into()),
                Term::Tag("deal:0199c1d2".into()),
                Term::Status(StatusMatch::Exact(500)),
                Term::Status(StatusMatch::Class(4)),
                Term::Route("/api/v1/deals/{id}".into()),
                Term::Type(EntryKind::Query),
                Term::Slow,
                Term::Failed,
                Term::Duration(Comparison::Longer, 200.0),
                Term::Batch("01a17749".into()),
                Term::Trace("4bf9".into()),
                Term::Text("two words".into()),
                Term::Text("deals".into()),
            ]
        );
    }

    #[test]
    fn bad_terms_name_themselves() {
        assert!(
            Filter::parse("status:abc")
                .unwrap_err()
                .0
                .contains("status:abc")
        );
        assert!(Filter::parse("status:999").is_err());
        assert!(Filter::parse("status:9xx").is_err());
        assert!(
            Filter::parse("type:nope")
                .unwrap_err()
                .0
                .contains("unknown type")
        );
        assert!(Filter::parse("duration>soon").is_err());
        assert!(
            Filter::parse("user:")
                .unwrap_err()
                .0
                .contains("needs a value")
        );
    }

    #[test]
    fn unknown_keys_are_free_text() {
        assert_eq!(
            Filter::parse("http://x deal.won").unwrap().terms(),
            [Term::Text("http://x".into()), Term::Text("deal.won".into())]
        );
        assert_eq!(
            Filter::parse("DURATION<5ms Slow").unwrap().terms(),
            [Term::Duration(Comparison::Shorter, 5.0), Term::Slow]
        );
    }

    #[test]
    fn text_that_looks_like_a_term_is_quoted_when_printed() {
        let filter = Filter::of([Term::Text("slow".into()), Term::Text("user:x".into())]);
        assert_eq!(filter.to_string(), "\"slow\" \"user:x\"");
        assert_eq!(Filter::parse(&filter.to_string()).unwrap(), filter);
    }

    #[test]
    fn the_top_level_restriction_lifts_with_type_batch_or_trace() {
        assert!(Filter::all().top_level_only());
        assert!(Filter::parse("slow user:1").unwrap().top_level_only());
        assert!(!Filter::parse("type:log").unwrap().top_level_only());
        assert!(!Filter::parse("batch:1").unwrap().top_level_only());
        assert!(!Filter::parse("trace:1").unwrap().top_level_only());
    }

    #[test]
    fn prefixes_compare_ascii_case_insensitively() {
        assert!(starts_with_ci(Some("0192F0C1-aa"), "0192f0c1"));
        assert!(!starts_with_ci(Some("01"), "0192"));
        assert!(!starts_with_ci(None, ""));
        assert!(!starts_with_ci(Some("é1"), "e"));
    }
}
