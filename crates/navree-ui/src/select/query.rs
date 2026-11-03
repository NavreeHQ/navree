//! The server side of a searched list: [`OptionQuery`] reads what the runtime
//! asked, [`ChoiceList`] answers with the options as markup.
//!
//! ```
//! use navree_ui::{ChoiceList, OptionQuery};
//!
//! // GET /people/options?q=ada
//! let query = OptionQuery::parse("q=ada");
//! let list = ChoiceList::new()
//!     .option("7", "Ada Lovelace")
//!     .option("9", "Ada Smith");
//! assert_eq!(query.text(), "ada");
//! assert!(list.to_html().contains("Ada Lovelace"));
//! ```

use std::collections::HashSet;

use http::HeaderValue;
use http::header::{CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE};
use http::request::Parts;
use navree_http::extract::{FromRequestParts, urlencoded};
use navree_http::response::{IntoResponse, Response};

use super::choice::{Choice, Entries};
use super::list::rows;
use crate::html::LocalHref;
use crate::node::{Node, el};
use crate::render::render;
use crate::view::PAGE_CSP;

/// The longest search text read, in bytes.
pub const MAX_QUERY_TEXT: usize = 256;
/// The furthest offset read.
pub const MAX_OFFSET: u64 = 1_000_000;
/// The most options one answer carries, whatever was asked.
const MAX_LIMIT: usize = 50;
/// How many a plain request gets.
const DEFAULT_LIMIT: usize = 20;

/// What the runtime asks of a searched list: `?q=text&limit=20&after=40`.
///
/// It never fails: a value that doesn't parse is dropped, control characters are
/// taken out of the text, the text is clipped to
/// [`MAX_QUERY_TEXT`] bytes on a character boundary, the limit is capped at 50 and the
/// offset at [`MAX_OFFSET`]. It carries text, never a column name: which columns are
/// searched is the endpoint's own list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptionQuery {
    text: String,
    limit: usize,
    offset: u64,
}

impl OptionQuery {
    /// Reads a query string.
    pub fn parse(query: &str) -> Self {
        let mut found = Self {
            text: String::new(),
            limit: DEFAULT_LIMIT,
            offset: 0,
        };
        for (name, value) in urlencoded::parse(query.as_bytes()) {
            match name.as_ref() {
                "q" => found.text = clip(&without_controls(&value)),
                "limit" => {
                    if let Ok(limit) = value.parse::<usize>() {
                        found.limit = limit.clamp(1, MAX_LIMIT);
                    }
                }
                "after" => {
                    if let Ok(offset) = value.parse::<u64>() {
                        found.offset = offset.min(MAX_OFFSET);
                    }
                }
                _ => {}
            }
        }
        found
    }

    /// The text typed, trimmed. Empty when nothing was.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// How many options to answer with, at most 50.
    pub fn limit(&self) -> usize {
        self.limit
    }

    /// How many to skip: where the "Show more" of the last answer left off.
    pub fn offset(&self) -> u64 {
        self.offset
    }

    /// Whether nothing was typed.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// The query string for the page after this one, `offset` further on.
    pub fn next_url(&self, path: &str, taken: usize) -> String {
        let mut url = String::with_capacity(path.len() + self.text.len() + 32);
        url.push_str(path);
        // A path that already has a query goes on with `&`.
        url.push_str(if path.contains('?') { "&q=" } else { "?q=" });
        crate::form::encode_into(&mut url, &self.text);
        url.push_str("&limit=");
        url.push_str(&self.limit.to_string());
        url.push_str("&after=");
        url.push_str(
            &self
                .offset
                .saturating_add(taken as u64)
                .min(MAX_OFFSET)
                .to_string(),
        );
        url
    }
}

/// The text without control characters, trimmed. A NUL in a bound text makes
/// PostgreSQL refuse the whole query, and no control character is a search.
fn without_controls(text: &str) -> String {
    let kept: String = text.chars().filter(|c| !c.is_control()).collect();
    kept.trim().to_owned()
}

fn clip(text: &str) -> String {
    let mut end = text.len().min(MAX_QUERY_TEXT);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

impl FromRequestParts for OptionQuery {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        Ok(Self::parse(parts.uri.query().unwrap_or_default()))
    }
}

/// An answer to a search: options, groups and, when there are more, where to get
/// them. The markup is the same an inline list has, so a searched list and a fixed
/// one look and behave alike.
///
/// It answers `text/html` that is never stored. It is not a page: the runtime reads
/// it into the open list.
#[derive(Clone, Debug, Default)]
#[must_use]
pub struct ChoiceList {
    entries: Entries,
    more: Option<String>,
}

impl ChoiceList {
    /// An empty answer: the list shows its "no results" text.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an option.
    pub fn option(mut self, value: impl Into<String>, label: impl Into<String>) -> Self {
        self.entries.choice(Choice::new(value, label));
        self
    }

    /// Adds a choice.
    pub fn choice(mut self, choice: Choice) -> Self {
        self.entries.choice(choice);
        self
    }

    /// Adds choices.
    pub fn choices<C: Into<Choice>>(mut self, choices: impl IntoIterator<Item = C>) -> Self {
        for choice in choices {
            self.entries.choice(choice.into());
        }
        self
    }

    /// Adds a labelled group.
    pub fn group<C: Into<Choice>>(
        mut self,
        label: impl Into<String>,
        choices: impl IntoIterator<Item = C>,
    ) -> Self {
        self.entries
            .group(label.into(), choices.into_iter().map(Into::into).collect());
        self
    }

    /// Says there is a next page: the URL, on this site, that answers it. The list
    /// then ends with "Show more". [`OptionQuery::next_url`] builds it.
    pub fn more(mut self, url: impl Into<String>) -> Self {
        let url = url.into();
        self.more = LocalHref::parse(&url)
            .ok()
            .map(|href| href.as_str().to_owned());
        self
    }

    /// The markup the runtime reads.
    pub fn into_node(self) -> Node {
        let chosen = HashSet::new();
        el("div")
            .slot("option-page")
            .attr_opt("data-more", self.more)
            .child(rows(&self.entries, &chosen, false, false))
            .into()
    }

    /// The markup as text.
    pub fn to_html(&self) -> String {
        render(&self.clone().into_node())
    }
}

impl From<ChoiceList> for Node {
    fn from(list: ChoiceList) -> Self {
        list.into_node()
    }
}

impl IntoResponse for ChoiceList {
    fn into_response(self) -> Response {
        let mut response = (
            [
                (
                    CONTENT_TYPE,
                    HeaderValue::from_static("text/html; charset=utf-8"),
                ),
                (CACHE_CONTROL, HeaderValue::from_static("no-store")),
                (CONTENT_SECURITY_POLICY, HeaderValue::from_static(PAGE_CSP)),
            ],
            self.to_html(),
        )
            .into_response();
        response.headers_mut().insert(
            http::header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        );
        response
    }
}
