//! List queries: the one grammar every Navree list speaks.
//!
//! A list is a page of rows that can be filtered, searched, sorted and paged:
//! a JSON endpoint, an admin panel's table, a Navree UI table, a Next.js or
//! Nuxt data table. All of them write and read the same query string:
//!
//! ```text
//! filter[status]=active                  equal
//! filter[created_at][gte]=2026-10-01     an operator
//! filter[stage][in]=lead,won             one of several values
//! filter[owner_id][null]=true            is NULL (false: is not NULL)
//! sort=-created_at,name                  descending with a leading "-"
//! search=acme
//! page=2&per_page=25                     numbered pages, or
//! cursor=eyJpZCI6NDJ9&limit=50           cursor pages
//! ```
//!
//! | Operator | Means | Value |
//! |---|---|---|
//! | `eq` (or none) | equals | one value |
//! | `ne` | does not equal | one value |
//! | `gt`, `gte`, `lt`, `lte` | greater than, at least, less than, at most | one value |
//! | `in`, `not_in` | is one of, is none of | a comma list; the key may repeat |
//! | `contains` | contains this text, ignoring case | text |
//! | `starts_with` | starts with this text, ignoring case | text |
//! | `null` | is NULL (`true`) or is not NULL (`false`) | `true`, `false`, `1` or `0` |
//!
//! Anything else in the query string (`owner_id=3`, `range=30d`) is not the
//! list query's: it is left for the handler's own extractors.
//!
//! There are two ways to read it:
//!
//! - [`ListQuery::parse_lenient`] reads the syntax and reports what is wrong
//!   as [`Problem`]s while keeping everything that is right. Screens use it:
//!   an old or edited address should still show a page.
//! - [`ListSpec::parse`] is strict. A resource declares which fields can be
//!   filtered, how, and which can be sorted or searched, in a [`ListSpec`];
//!   anything outside that list, a value of the wrong type, an operator the
//!   field does not take or a page size past the limit is a 400 that names
//!   the parameter. The [`List`] extractor does this for a handler.
//!
//! ```
//! use navree_http::list::{Field, ListResource, ListSpec, Op};
//!
//! /// `GET /api/v1/deals`.
//! struct Deals;
//!
//! impl ListResource for Deals {
//!     const SPEC: ListSpec = ListSpec::new(&[
//!         Field::one_of("stage", &["lead", "won", "lost"]),
//!         Field::int("amount_cents"),
//!         Field::date("expected_close_date"),
//!         Field::uuid("owner_id"),
//!     ])
//!     .sortable(&["amount_cents", "created_at"])
//!     .searchable(&["title"]);
//! }
//!
//! let query = Deals::SPEC
//!     .parse("filter[stage][in]=lead,won&filter[amount_cents][gte]=100000&sort=-amount_cents")
//!     .unwrap();
//! assert_eq!(query.filter[0].op, Op::In);
//! assert_eq!(query.filter[0].values, ["lead", "won"]);
//!
//! // The 400 names the parameter.
//! let error = Deals::SPEC.parse("filter[password][contains]=x").unwrap_err();
//! assert_eq!(error.problems[0].param, "filter[password][contains]");
//! ```

use std::collections::BTreeMap;
use std::fmt;
use std::marker::PhantomData;
use std::ops::Deref;

use http::request::Parts;

use crate::AppError;
use crate::extract::{FromRequestParts, urlencoded};

/// The most bytes of a query string read.
pub const MAX_QUERY_BYTES: usize = 16 * 1024;
/// The most parameters read from a query string.
pub const MAX_PARAMS: usize = 1000;
/// The most filter conditions kept.
pub const MAX_CONDITIONS: usize = 64;
/// The most values one `in` or `not_in` condition keeps.
pub const MAX_VALUES: usize = 200;
/// The longest filter value or search text, in bytes.
pub const MAX_TEXT: usize = 256;
/// The longest cursor, in bytes.
pub const MAX_CURSOR: usize = 1024;
/// The most sort fields kept.
pub const MAX_SORTS: usize = 4;
/// The longest field name, in characters.
pub const MAX_FIELD: usize = 64;
/// The highest page number.
pub const MAX_PAGE: u64 = 10_000_000;
/// The largest page size the syntax allows; a [`ListSpec`] sets a lower one.
pub const MAX_PAGE_SIZE: u64 = 10_000;

/// What a condition compares with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Op {
    /// Equals (`filter[f]=v`, or `filter[f][eq]=v`).
    Eq,
    /// Does not equal.
    Ne,
    /// Greater than.
    Gt,
    /// Greater than or equal to.
    Gte,
    /// Less than.
    Lt,
    /// Less than or equal to.
    Lte,
    /// Is one of a list.
    In,
    /// Is none of a list.
    NotIn,
    /// Contains this text, ignoring case.
    Contains,
    /// Starts with this text, ignoring case.
    StartsWith,
    /// Is NULL (`true`) or is not NULL (`false`).
    Null,
}

impl Op {
    /// Every operator.
    pub const ALL: [Op; 11] = [
        Op::Eq,
        Op::Ne,
        Op::Gt,
        Op::Gte,
        Op::Lt,
        Op::Lte,
        Op::In,
        Op::NotIn,
        Op::Contains,
        Op::StartsWith,
        Op::Null,
    ];

    /// The operator as the query string spells it (`gte`, `not_in`).
    pub const fn name(self) -> &'static str {
        match self {
            Op::Eq => "eq",
            Op::Ne => "ne",
            Op::Gt => "gt",
            Op::Gte => "gte",
            Op::Lt => "lt",
            Op::Lte => "lte",
            Op::In => "in",
            Op::NotIn => "not_in",
            Op::Contains => "contains",
            Op::StartsWith => "starts_with",
            Op::Null => "null",
        }
    }

    /// The operator spelled `name`, exactly (`gte` and not `GTE`).
    pub fn parse(name: &str) -> Option<Op> {
        Op::ALL.into_iter().find(|op| op.name() == name)
    }

    /// Whether the operator takes a list (`in`, `not_in`).
    pub const fn takes_list(self) -> bool {
        matches!(self, Op::In | Op::NotIn)
    }
}

impl fmt::Display for Op {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// One filter: a field, an operator and its values (one value, except for
/// `in` and `not_in`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Condition {
    /// The field's name.
    pub field: String,
    /// The operator.
    pub op: Op,
    /// The values, never empty. For `null`: `"true"` or `"false"`.
    pub values: Vec<String>,
}

impl Condition {
    /// The query parameter that carries it: `filter[stage]` for equality,
    /// `filter[amount][gte]` for the rest.
    pub fn param(&self) -> String {
        param_of(&self.field, self.op)
    }

    /// The first value.
    pub fn value(&self) -> &str {
        self.values.first().map_or("", String::as_str)
    }

    /// For `null`: whether the field must be NULL.
    pub fn null_flag(&self) -> Option<bool> {
        (self.op == Op::Null).then(|| self.value() == "true")
    }
}

/// The query parameter of a condition on `field` with operator `op`.
fn param_of(field: &str, op: Op) -> String {
    if op == Op::Eq {
        format!("filter[{field}]")
    } else {
        format!("filter[{field}][{op}]")
    }
}

/// One sort field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SortKey {
    /// The field's name.
    pub field: String,
    /// Whether the order is descending (`-field`).
    pub descending: bool,
}

impl SortKey {
    /// `-field` or `field`.
    pub fn term(&self) -> String {
        if self.descending {
            format!("-{}", self.field)
        } else {
            self.field.clone()
        }
    }
}

/// Something wrong in a query string: the parameter, and what is wrong with
/// it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    /// The parameter, as written (`filter[stage][in]`, `per_page`).
    pub param: String,
    /// What is wrong, in a sentence.
    pub message: String,
}

impl Problem {
    fn new(param: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            param: param.into(),
            message: message.into(),
        }
    }
}

/// A list query, as data. Every member is optional.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ListQuery {
    /// The filters, all of which must hold.
    pub filter: Vec<Condition>,
    /// The fields to sort by, in order.
    pub sort: Vec<SortKey>,
    /// The text to search for.
    pub search: Option<String>,
    /// The page number, from 1 (numbered pages).
    pub page: Option<u64>,
    /// The page size (numbered pages).
    pub per_page: Option<u64>,
    /// The page after this position (cursor pages): the `next_cursor` of the
    /// page before.
    pub cursor: Option<String>,
    /// The page before this position (cursor pages that go both ways).
    pub before: Option<String>,
    /// The page size (cursor pages).
    pub limit: Option<u64>,
}

/// How a list is paged, with the defaults filled in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Paging {
    /// Numbered pages.
    Pages {
        /// The page number, from 1.
        page: u64,
        /// The page size.
        per_page: u64,
    },
    /// Cursor pages.
    Cursor {
        /// The page after this position; the first page without one.
        cursor: Option<String>,
        /// The page before this position.
        before: Option<String>,
        /// The page size.
        limit: u64,
    },
}

impl ListQuery {
    /// An empty query.
    pub fn new() -> Self {
        Self::default()
    }

    /// Reads the list query's parameters from `query` (the query string, with
    /// or without its `?`), keeping everything that is right and reporting
    /// the rest. It reads at most [`MAX_QUERY_BYTES`] bytes and
    /// [`MAX_PARAMS`] parameters.
    ///
    /// ```
    /// use navree_http::list::ListQuery;
    ///
    /// let (query, problems) = ListQuery::parse_lenient("filter[stage]=won&page=0&owner_id=3");
    /// assert_eq!(query.filter.len(), 1);
    /// assert_eq!(query.page, None);
    /// assert_eq!(problems[0].param, "page");
    /// ```
    pub fn parse_lenient(query: &str) -> (ListQuery, Vec<Problem>) {
        let mut reader = Reader::default();
        reader.read(query.strip_prefix('?').unwrap_or(query));
        (reader.list, reader.problems)
    }

    /// The condition on `field` with operator `op`.
    pub fn condition(&self, field: &str, op: Op) -> Option<&Condition> {
        self.filter
            .iter()
            .find(|condition| condition.field == field && condition.op == op)
    }

    /// The value of `filter[field]=value`.
    pub fn eq(&self, field: &str) -> Option<&str> {
        self.condition(field, Op::Eq).map(Condition::value)
    }

    /// Writes the query back as a query string: the parameters a list query
    /// owns, in the order filter, sort, search, page, per_page, cursor,
    /// before, limit, with a field's conditions together and `eq` written
    /// plain. The encoding is `URLSearchParams`' (a space is `+`, a bracket
    /// is `%5B`), so the same query is the same string in the browser.
    ///
    /// ```
    /// use navree_http::list::ListQuery;
    ///
    /// let (query, _) = ListQuery::parse_lenient("sort=-name&filter[stage][in]=a,b&page=2");
    /// assert_eq!(query.to_query_string(), "filter%5Bstage%5D%5Bin%5D=a%2Cb&sort=-name&page=2");
    /// ```
    pub fn to_query_string(&self) -> String {
        let mut out = String::new();
        let mut done: Vec<&str> = Vec::new();
        for first in &self.filter {
            if done.contains(&first.field.as_str()) {
                continue;
            }
            done.push(&first.field);
            for condition in self.filter.iter().filter(|c| c.field == first.field) {
                pair(&mut out, &condition.param(), &condition.values.join(","));
            }
        }
        if !self.sort.is_empty() {
            let terms: Vec<String> = self.sort.iter().map(SortKey::term).collect();
            pair(&mut out, "sort", &terms.join(","));
        }
        if let Some(search) = &self.search {
            pair(&mut out, "search", search);
        }
        if let Some(page) = self.page {
            pair(&mut out, "page", &page.to_string());
        }
        if let Some(per_page) = self.per_page {
            pair(&mut out, "per_page", &per_page.to_string());
        }
        if let Some(cursor) = &self.cursor {
            pair(&mut out, "cursor", cursor);
        }
        if let Some(before) = &self.before {
            pair(&mut out, "before", before);
        }
        if let Some(limit) = self.limit {
            pair(&mut out, "limit", &limit.to_string());
        }
        out
    }

    /// The same query on page `page` (page 1 leaves `page` out), without a
    /// cursor.
    pub fn on_page(mut self, page: u64) -> Self {
        self.page = (page > 1).then_some(page);
        self.cursor = None;
        self.before = None;
        self
    }

    /// Whether the query pages by cursor (`cursor`, `before` or `limit`).
    pub fn is_cursor(&self) -> bool {
        self.cursor.is_some() || self.before.is_some() || self.limit.is_some()
    }
}

/// Appends `name=value` to a query string being written.
fn pair(out: &mut String, name: &str, value: &str) {
    if !out.is_empty() {
        out.push('&');
    }
    encode_component_into(out, name);
    out.push('=');
    encode_component_into(out, value);
}

const HEX: &[u8; 16] = b"0123456789ABCDEF";

/// Encodes `text` the way `URLSearchParams` does: letters, digits and
/// `*-._` stay, a space is `+`, every other byte is `%XX`.
///
/// ```
/// use navree_http::list::encode_component;
///
/// assert_eq!(encode_component("filter[a b]"), "filter%5Ba+b%5D");
/// ```
pub fn encode_component(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    encode_component_into(&mut out, text);
    out
}

fn encode_component_into(out: &mut String, text: &str) {
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'*' | b'-' | b'.' | b'_' => {
                out.push(char::from(byte));
            }
            b' ' => out.push('+'),
            _ => {
                out.push('%');
                out.push(char::from(HEX[usize::from(byte >> 4)]));
                out.push(char::from(HEX[usize::from(byte & 15)]));
            }
        }
    }
}

/// A field or sort name: a letter or `_`, then letters, digits, `_` and `.`,
/// at most [`MAX_FIELD`] long.
fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(first) if first.is_ascii_alphabetic() || first == '_')
        && name.len() <= MAX_FIELD
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
}

/// `text` cut to at most `max` bytes, at a character boundary.
fn clip(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// `filter[field]` or `filter[field][op]` as (field, op).
fn split_filter_key(key: &str) -> Option<(&str, Option<&str>)> {
    let rest = key.strip_prefix("filter[")?;
    let close = rest.find(']')?;
    let field = &rest[..close];
    let after = &rest[close + 1..];
    if after.is_empty() {
        return Some((field, None));
    }
    let op = after.strip_prefix('[')?.strip_suffix(']')?;
    if op.contains(['[', ']']) {
        return None;
    }
    Some((field, Some(op)))
}

/// A positive whole number of digits only, at most `max`.
fn count(value: &str, max: u64) -> Result<u64, &'static str> {
    if !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("must be a positive whole number");
    }
    match value.parse::<u64>() {
        Ok(0) => Err("must be at least 1"),
        Ok(n) if n > max => Err("is too large"),
        Ok(n) => Ok(n),
        Err(_) => Err("is too large"),
    }
}

/// The reader behind [`ListQuery::parse_lenient`].
#[derive(Default)]
struct Reader {
    list: ListQuery,
    problems: Vec<Problem>,
    sort_full: bool,
}

impl Reader {
    fn problem(&mut self, param: impl Into<String>, message: impl Into<String>) {
        self.problems.push(Problem::new(param, message));
    }

    fn read(&mut self, query: &str) {
        let mut bytes = query.as_bytes();
        if bytes.len() > MAX_QUERY_BYTES {
            self.problem(
                "query",
                format!("the query string must not exceed {MAX_QUERY_BYTES} bytes"),
            );
            bytes = &bytes[..MAX_QUERY_BYTES];
        }
        for (read, (name, value)) in urlencoded::parse(bytes).enumerate() {
            if read >= MAX_PARAMS {
                self.problem(
                    "query",
                    format!("a list query takes at most {MAX_PARAMS} parameters"),
                );
                break;
            }
            let (name, value) = (&*name, &*value);
            match name {
                "sort" => self.sort(value),
                "search" => self.search(value),
                "page" => self.number(name, value, MAX_PAGE),
                "per_page" | "limit" => self.number(name, value, MAX_PAGE_SIZE),
                "cursor" | "before" => self.cursor(name, value),
                "filter" => self.problem(
                    name,
                    "write a filter as filter[field]=value or filter[field][operator]=value",
                ),
                _ if name.starts_with("filter[") => self.filter(name, value),
                _ => {}
            }
        }
        let numbered = self.list.page.is_some() || self.list.per_page.is_some();
        if numbered && self.list.is_cursor() {
            let param = if self.list.cursor.is_some() {
                "cursor"
            } else if self.list.before.is_some() {
                "before"
            } else {
                "limit"
            };
            self.problem(
                param,
                "use page and per_page, or cursor and limit, not both",
            );
        }
    }

    fn sort(&mut self, value: &str) {
        for term in value.split(',').map(str::trim).filter(|t| !t.is_empty()) {
            let (field, descending) = term
                .strip_prefix('-')
                .map_or((term, false), |field| (field, true));
            if !valid_name(field) {
                self.problem("sort", format!("`{term}` is not a field name"));
            } else if self.list.sort.iter().any(|key| key.field == field) {
                self.problem("sort", format!("`{field}` is sorted twice"));
            } else if self.list.sort.len() >= MAX_SORTS {
                if !self.sort_full {
                    self.sort_full = true;
                    self.problem("sort", format!("sort by at most {MAX_SORTS} fields"));
                }
            } else {
                self.list.sort.push(SortKey {
                    field: field.to_owned(),
                    descending,
                });
            }
        }
    }

    fn search(&mut self, value: &str) {
        let value = value.trim();
        if value.is_empty() {
            return;
        }
        if value.contains('\0') {
            self.problem("search", "must not contain a NUL character");
            return;
        }
        if value.len() > MAX_TEXT {
            self.problem("search", format!("must not exceed {MAX_TEXT} bytes"));
        }
        self.list.search = Some(clip(value, MAX_TEXT).to_owned());
    }

    fn number(&mut self, name: &str, value: &str, max: u64) {
        if value.is_empty() {
            return;
        }
        match count(value, max) {
            Ok(n) => match name {
                "page" => self.list.page = Some(n),
                "per_page" => self.list.per_page = Some(n),
                _ => self.list.limit = Some(n),
            },
            Err(why) => self.problem(name, format!("{name} {why}")),
        }
    }

    fn cursor(&mut self, name: &str, value: &str) {
        if value.is_empty() {
            return;
        }
        if value.contains('\0') {
            self.problem(name, "must not contain a NUL character");
        } else if value.len() > MAX_CURSOR {
            self.problem(name, format!("must not exceed {MAX_CURSOR} bytes"));
        } else if name == "cursor" {
            self.list.cursor = Some(value.to_owned());
        } else {
            self.list.before = Some(value.to_owned());
        }
    }

    fn filter(&mut self, key: &str, value: &str) {
        let Some((field, op)) = split_filter_key(key) else {
            self.problem(
                key,
                "write a filter as filter[field]=value or filter[field][operator]=value",
            );
            return;
        };
        if !valid_name(field) {
            self.problem(
                key,
                format!("`{field}` is not a field name (letters, digits, _ and . only)"),
            );
            return;
        }
        let op = match op {
            None => Op::Eq,
            Some(name) => match Op::parse(name) {
                Some(op) => op,
                None => {
                    self.problem(
                        key,
                        format!(
                            "`{name}` is not an operator; use one of: {}",
                            Op::ALL.map(Op::name).join(", ")
                        ),
                    );
                    return;
                }
            },
        };
        if value.trim().is_empty() {
            return;
        }
        if value.contains('\0') {
            self.problem(key, "must not contain a NUL character");
            return;
        }
        let param = param_of(field, op);
        let values: Vec<String> = match op {
            Op::In | Op::NotIn => value
                .split(',')
                .filter(|part| !part.is_empty())
                .map(str::to_owned)
                .collect(),
            Op::Null => match value {
                "true" | "1" => vec!["true".to_owned()],
                "false" | "0" => vec!["false".to_owned()],
                _ => {
                    self.problem(param, "must be true or false");
                    return;
                }
            },
            _ => vec![value.to_owned()],
        };
        if values.iter().any(|v| v.len() > MAX_TEXT) {
            self.problem(param, format!("a value must not exceed {MAX_TEXT} bytes"));
            return;
        }
        if values.is_empty() {
            return;
        }
        let at = self
            .list
            .filter
            .iter()
            .position(|c| c.field == field && c.op == op);
        match at {
            Some(at) if op.takes_list() => {
                for value in values {
                    if self.list.filter[at].values.len() >= MAX_VALUES {
                        self.problem(param, format!("takes at most {MAX_VALUES} values"));
                        return;
                    }
                    self.list.filter[at].values.push(value);
                }
            }
            Some(_) => self.problem(param, "is given twice; the first is used"),
            None if self.list.filter.len() >= MAX_CONDITIONS => self.problem(
                "filter",
                format!("a list query takes at most {MAX_CONDITIONS} filters"),
            ),
            None => {
                let mut values = values;
                if values.len() > MAX_VALUES {
                    values.truncate(MAX_VALUES);
                    self.problem(param, format!("takes at most {MAX_VALUES} values"));
                }
                self.list.filter.push(Condition {
                    field: field.to_owned(),
                    op,
                    values,
                });
            }
        }
    }
}

// ---- What a resource allows --------------------------------------------------------

/// What a filter field holds, and so what a value may look like.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Text.
    Text,
    /// A whole number (`i64`).
    Int,
    /// A decimal number, written with digits and one point (`12.50`).
    Decimal,
    /// `true` or `false`.
    Bool,
    /// A date, `YYYY-MM-DD`.
    Date,
    /// A date, or a date and time with an offset (RFC 3339).
    DateTime,
    /// A UUID, in its hyphenated form.
    Uuid,
    /// One of these words.
    OneOf(&'static [&'static str]),
}

const TEXT_OPS: &[Op] = &[
    Op::Eq,
    Op::Ne,
    Op::Contains,
    Op::StartsWith,
    Op::In,
    Op::NotIn,
    Op::Null,
];
const ORDERED_OPS: &[Op] = &[
    Op::Eq,
    Op::Ne,
    Op::Gt,
    Op::Gte,
    Op::Lt,
    Op::Lte,
    Op::In,
    Op::NotIn,
    Op::Null,
];
const SET_OPS: &[Op] = &[Op::Eq, Op::Ne, Op::In, Op::NotIn, Op::Null];
const BOOL_OPS: &[Op] = &[Op::Eq, Op::Ne, Op::Null];

impl Kind {
    /// The operators a field of this kind takes unless it says otherwise.
    pub const fn default_ops(self) -> &'static [Op] {
        match self {
            Kind::Text => TEXT_OPS,
            Kind::Int | Kind::Decimal | Kind::Date | Kind::DateTime => ORDERED_OPS,
            Kind::Uuid | Kind::OneOf(_) => SET_OPS,
            Kind::Bool => BOOL_OPS,
        }
    }

    /// Whether `text` is a value of this kind.
    pub fn check(self, text: &str) -> Result<(), String> {
        let wrong = |what: &str| Err(format!("`{}` is not {what}", clip(text, 40)));
        match self {
            Kind::Text => Ok(()),
            Kind::Int => match text.parse::<i64>() {
                Ok(_) => Ok(()),
                Err(_) => wrong("a whole number"),
            },
            Kind::Decimal => {
                let digits = text.strip_prefix('-').unwrap_or(text);
                let (whole, fraction) = digits.split_once('.').unwrap_or((digits, "0"));
                let ok = !whole.is_empty()
                    && !fraction.is_empty()
                    && whole.len() + fraction.len() <= 38
                    && whole.bytes().chain(fraction.bytes()).all(|b| b.is_ascii_digit());
                if ok { Ok(()) } else { wrong("a decimal number") }
            }
            Kind::Bool => match text {
                "true" | "false" => Ok(()),
                _ => wrong("true or false"),
            },
            Kind::Date => {
                if is_date(text) {
                    Ok(())
                } else {
                    wrong("a date (YYYY-MM-DD)")
                }
            }
            Kind::DateTime => {
                if is_date(text) || chrono::DateTime::parse_from_rfc3339(text).is_ok() {
                    Ok(())
                } else {
                    wrong("a date or a date and time (2026-11-05 or 2026-11-05T10:30:00Z)")
                }
            }
            Kind::Uuid => {
                if text.len() == 36 && uuid::Uuid::parse_str(text).is_ok() {
                    Ok(())
                } else {
                    wrong("a UUID")
                }
            }
            Kind::OneOf(words) => {
                if words.contains(&text) {
                    Ok(())
                } else {
                    Err(format!("`{}` is not one of: {}", clip(text, 40), words.join(", ")))
                }
            }
        }
    }
}

fn is_date(text: &str) -> bool {
    text.len() == 10 && chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").is_ok()
}

/// One field a list can be filtered by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Field {
    /// The name in the query string.
    pub name: &'static str,
    /// What it holds.
    pub kind: Kind,
    /// The operators it takes; those of its kind when `None`.
    pub ops: Option<&'static [Op]>,
    /// The column or document field it reads, when that is not its name.
    pub column: Option<&'static str>,
    /// Whether the handler applies it (a computed filter such as
    /// `due=overdue`): the query is checked as for any field, and
    /// `Builder::list` leaves the condition to the handler.
    pub by_app: bool,
}

impl Field {
    /// A field of `kind`.
    pub const fn new(name: &'static str, kind: Kind) -> Self {
        Self {
            name,
            kind,
            ops: None,
            column: None,
            by_app: false,
        }
    }

    /// A text field.
    pub const fn text(name: &'static str) -> Self {
        Self::new(name, Kind::Text)
    }

    /// A whole-number field.
    pub const fn int(name: &'static str) -> Self {
        Self::new(name, Kind::Int)
    }

    /// A decimal field.
    pub const fn decimal(name: &'static str) -> Self {
        Self::new(name, Kind::Decimal)
    }

    /// A true-or-false field.
    pub const fn boolean(name: &'static str) -> Self {
        Self::new(name, Kind::Bool)
    }

    /// A date field.
    pub const fn date(name: &'static str) -> Self {
        Self::new(name, Kind::Date)
    }

    /// A date-and-time field.
    pub const fn datetime(name: &'static str) -> Self {
        Self::new(name, Kind::DateTime)
    }

    /// A UUID field.
    pub const fn uuid(name: &'static str) -> Self {
        Self::new(name, Kind::Uuid)
    }

    /// A field that holds one of `words`.
    pub const fn one_of(name: &'static str, words: &'static [&'static str]) -> Self {
        Self::new(name, Kind::OneOf(words))
    }

    /// Narrows (or widens) the operators the field takes.
    pub const fn ops(mut self, ops: &'static [Op]) -> Self {
        self.ops = Some(ops);
        self
    }

    /// Reads the column `column` (`companies.name`) instead of a column named
    /// like the field.
    pub const fn column(mut self, column: &'static str) -> Self {
        self.column = Some(column);
        self
    }

    /// The handler applies the field, not `Builder::list`.
    pub const fn by_app(mut self) -> Self {
        self.by_app = true;
        self
    }

    /// The operators the field takes.
    pub const fn allowed_ops(&self) -> &'static [Op] {
        match self.ops {
            Some(ops) => ops,
            None => self.kind.default_ops(),
        }
    }

    /// The column the field reads.
    pub const fn source(&self) -> &'static str {
        match self.column {
            Some(column) => column,
            None => self.name,
        }
    }
}

/// Whether and how a list is searched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Search {
    /// `search` is refused.
    Off,
    /// `search` matches text in any of these columns, ignoring case.
    Columns(&'static [&'static str]),
    /// `search` is accepted and the handler applies it (a search that
    /// matches words, or several tables).
    ByApp,
}

/// What a list allows in its query: the allow-list of one resource. Build it
/// as a constant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListSpec {
    /// The fields that can be filtered.
    pub fields: &'static [Field],
    /// The fields that can be sorted by.
    pub sorts: &'static [&'static str],
    /// The sort when the query gives none (`-created_at`).
    pub default_sort: &'static [&'static str],
    /// Searching.
    pub search: Search,
    /// The page size when the query gives none.
    pub default_per_page: u64,
    /// The largest page size.
    pub max_per_page: u64,
    /// Whether `before` is accepted (a list that pages both ways by cursor).
    pub two_way_cursors: bool,
}

impl ListSpec {
    /// A list filterable by `fields`: no sorting, no search, pages of 15 up
    /// to 100.
    pub const fn new(fields: &'static [Field]) -> Self {
        Self {
            fields,
            sorts: &[],
            default_sort: &[],
            search: Search::Off,
            default_per_page: 15,
            max_per_page: 100,
            two_way_cursors: false,
        }
    }

    /// The fields the list can be sorted by.
    pub const fn sortable(mut self, sorts: &'static [&'static str]) -> Self {
        self.sorts = sorts;
        self
    }

    /// The sort when the query gives none: field names, `-` in front for
    /// descending. The fields need not be sortable by the client.
    pub const fn default_sort(mut self, sorts: &'static [&'static str]) -> Self {
        self.default_sort = sorts;
        self
    }

    /// `search` matches these columns.
    pub const fn searchable(mut self, columns: &'static [&'static str]) -> Self {
        self.search = Search::Columns(columns);
        self
    }

    /// `search` is accepted, and the handler applies it.
    pub const fn searchable_by_app(mut self) -> Self {
        self.search = Search::ByApp;
        self
    }

    /// The default and the largest page size.
    pub const fn page_size(mut self, default: u64, max: u64) -> Self {
        self.default_per_page = default;
        self.max_per_page = max;
        self
    }

    /// Accepts `before`.
    pub const fn two_way_cursors(mut self) -> Self {
        self.two_way_cursors = true;
        self
    }

    /// The filter field called `name`.
    pub fn field(&self, name: &str) -> Option<&'static Field> {
        self.fields.iter().find(|field| field.name == name)
    }

    /// Reads `query` strictly: the list query's syntax, then this list's
    /// allow-list. A problem anywhere is an error naming every problem.
    pub fn parse(&self, query: &str) -> Result<ListQuery, ListError> {
        let (list, mut problems) = ListQuery::parse_lenient(query);
        problems.extend(self.check(&list));
        if problems.is_empty() {
            Ok(list)
        } else {
            Err(ListError { problems })
        }
    }

    /// What in `list` this list does not allow.
    pub fn check(&self, list: &ListQuery) -> Vec<Problem> {
        let mut problems = Vec::new();
        for condition in &list.filter {
            let param = condition.param();
            let Some(field) = self.field(&condition.field) else {
                problems.push(Problem::new(
                    param,
                    format!(
                        "`{}` is not a filter of this list{}",
                        condition.field,
                        names(self.fields.iter().map(|field| field.name))
                    ),
                ));
                continue;
            };
            if !field.allowed_ops().contains(&condition.op) {
                let ops: Vec<&str> = field.allowed_ops().iter().map(|op| op.name()).collect();
                problems.push(Problem::new(
                    param,
                    format!(
                        "`{}` does not take `{}`; it takes: {}",
                        field.name,
                        condition.op,
                        ops.join(", ")
                    ),
                ));
                continue;
            }
            if condition.op == Op::Null {
                continue;
            }
            if condition.op == Op::StartsWith
                && condition.values.iter().any(|v| v.contains(['%', '_', '\\']))
            {
                problems.push(Problem::new(
                    param,
                    "a starts_with value cannot contain %, _ or a backslash",
                ));
                continue;
            }
            for value in &condition.values {
                if let Err(message) = field.kind.check(value) {
                    problems.push(Problem::new(param.clone(), message));
                    break;
                }
            }
        }
        for key in &list.sort {
            if !self.sorts.contains(&key.field.as_str()) {
                problems.push(Problem::new(
                    "sort",
                    format!(
                        "`{}` is not a sort of this list{}",
                        key.field,
                        names(self.sorts.iter().copied())
                    ),
                ));
            }
        }
        if list.search.is_some() && self.search == Search::Off {
            problems.push(Problem::new("search", "this list cannot be searched"));
        }
        for (param, size) in [("per_page", list.per_page), ("limit", list.limit)] {
            if let Some(size) = size.filter(|size| *size > self.max_per_page) {
                problems.push(Problem::new(
                    param,
                    format!("{param} must not exceed {} (got {size})", self.max_per_page),
                ));
            }
        }
        if list.before.is_some() && !self.two_way_cursors {
            problems.push(Problem::new("before", "this list pages forward only"));
        }
        problems
    }

    /// How `list` pages, the page size defaulted.
    pub fn paging(&self, list: &ListQuery) -> Paging {
        if list.is_cursor() {
            Paging::Cursor {
                cursor: list.cursor.clone(),
                before: list.before.clone(),
                limit: list.limit.unwrap_or(self.default_per_page),
            }
        } else {
            Paging::Pages {
                page: list.page.unwrap_or(1),
                per_page: list.per_page.unwrap_or(self.default_per_page),
            }
        }
    }

    /// The sort to apply: the query's, else the list's default.
    pub fn sort_of(&self, list: &ListQuery) -> Vec<SortKey> {
        if !list.sort.is_empty() {
            return list.sort.clone();
        }
        self.default_sort
            .iter()
            .map(|term| {
                term.strip_prefix('-').map_or_else(
                    || SortKey {
                        field: (*term).to_owned(),
                        descending: false,
                    },
                    |field| SortKey {
                        field: field.to_owned(),
                        descending: true,
                    },
                )
            })
            .collect()
    }
}

/// `; use one of: a, b` for a non-empty list of names.
fn names<'a>(names: impl Iterator<Item = &'a str>) -> String {
    let names: Vec<&str> = names.collect();
    if names.is_empty() {
        String::new()
    } else {
        format!("; use one of: {}", names.join(", "))
    }
}

/// A query a list does not allow: every [`Problem`] found. It converts into
/// a 400 [`AppError`] that names each parameter (`?` in a handler does it).
/// It is not a `std::error::Error`, like `AppError`, so the conversion is
/// not the 500 every other error becomes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListError {
    /// The problems, in the order found. Never empty.
    pub problems: Vec<Problem>,
}

impl fmt::Display for ListError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (at, problem) in self.problems.iter().enumerate() {
            if at > 0 {
                f.write_str("; ")?;
            }
            write!(f, "{}: {}", problem.param, problem.message)?;
        }
        Ok(())
    }
}

impl From<ListError> for AppError {
    /// A 400 whose `errors` map each parameter to its messages.
    fn from(error: ListError) -> Self {
        let mut errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for problem in error.problems {
            errors.entry(problem.param).or_default().push(problem.message);
        }
        AppError::validation(errors)
    }
}

// ---- The extractor -----------------------------------------------------------------

/// A resource that is listed: its [`ListSpec`].
pub trait ListResource {
    /// What the list allows in its query.
    const SPEC: ListSpec;
}

/// The list query of a request, read strictly against `R`'s [`ListSpec`]: a
/// handler that takes `List<Deals>` gets a query it can trust, or the request
/// ends in a 400 that names the parameter.
///
/// ```
/// use navree_http::list::{Field, List, ListResource, ListSpec};
///
/// struct Deals;
///
/// impl ListResource for Deals {
///     const SPEC: ListSpec = ListSpec::new(&[Field::int("amount_cents")]);
/// }
///
/// async fn index(list: List<Deals>) -> String {
///     format!("{} filters", list.filter.len())
/// }
/// # let _ = index;
/// ```
pub struct List<R>(pub ListQuery, PhantomData<fn() -> R>);

impl<R> List<R> {
    /// The query.
    pub fn into_inner(self) -> ListQuery {
        self.0
    }
}

impl<R> Deref for List<R> {
    type Target = ListQuery;

    fn deref(&self) -> &ListQuery {
        &self.0
    }
}

impl<R> Clone for List<R> {
    fn clone(&self) -> Self {
        List(self.0.clone(), PhantomData)
    }
}

impl<R> fmt::Debug for List<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("List").field(&self.0).finish()
    }
}

impl<R: ListResource> List<R> {
    /// Reads `uri`'s query string against `R`.
    pub fn from_uri(uri: &http::Uri) -> Result<Self, ListError> {
        R::SPEC
            .parse(uri.query().unwrap_or_default())
            .map(|query| List(query, PhantomData))
    }
}

impl<R: ListResource> FromRequestParts for List<R> {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, AppError> {
        Self::from_uri(&parts.uri).map_err(AppError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Deals;

    impl ListResource for Deals {
        const SPEC: ListSpec = ListSpec::new(&[
            Field::one_of("stage", &["lead", "won", "lost"]),
            Field::int("amount_cents"),
            Field::decimal("rate"),
            Field::boolean("hot"),
            Field::date("closes_on"),
            Field::datetime("created_at"),
            Field::uuid("owner_id"),
            Field::text("title"),
            Field::text("company_name").column("companies.name"),
            Field::text("code").ops(&[Op::Eq]),
        ])
        .sortable(&["amount_cents", "created_at", "title"])
        .default_sort(&["-created_at"])
        .searchable(&["title"])
        .page_size(15, 100);
    }

    fn strict(query: &str) -> Result<ListQuery, ListError> {
        Deals::SPEC.parse(query)
    }

    fn params(error: ListError) -> Vec<String> {
        error.problems.into_iter().map(|p| p.param).collect()
    }

    #[test]
    fn reads_every_part() {
        let list = strict(
            "filter[stage][in]=lead,won&filter[amount_cents][gte]=1000&filter[hot]=true\
             &filter[owner_id][null]=false&sort=-amount_cents,title&search=Sutlej&page=3&per_page=50",
        )
        .unwrap();
        assert_eq!(list.filter.len(), 4);
        assert_eq!(list.filter[0].op, Op::In);
        assert_eq!(list.filter[0].values, ["lead", "won"]);
        assert_eq!(list.filter[1].field, "amount_cents");
        assert_eq!(list.filter[1].op, Op::Gte);
        assert_eq!(list.eq("hot"), Some("true"));
        assert_eq!(list.filter[3].null_flag(), Some(false));
        assert_eq!(list.sort[0].term(), "-amount_cents");
        assert_eq!(list.search.as_deref(), Some("Sutlej"));
        assert_eq!(
            Deals::SPEC.paging(&list),
            Paging::Pages {
                page: 3,
                per_page: 50
            }
        );
    }

    #[test]
    fn an_empty_query_is_an_empty_list() {
        let list = strict("").unwrap();
        assert_eq!(list, ListQuery::default());
        assert_eq!(
            Deals::SPEC.paging(&list),
            Paging::Pages {
                page: 1,
                per_page: 15
            }
        );
        assert_eq!(Deals::SPEC.sort_of(&list)[0].term(), "-created_at");
    }

    #[test]
    fn cursor_pages_default_their_size() {
        let list = strict("cursor=abc").unwrap();
        assert_eq!(
            Deals::SPEC.paging(&list),
            Paging::Cursor {
                cursor: Some("abc".to_owned()),
                before: None,
                limit: 15
            }
        );
        let list = strict("limit=20").unwrap();
        assert!(list.is_cursor());
    }

    #[test]
    fn a_field_outside_the_allow_list_is_named() {
        let error = strict("filter[password][contains]=x&filter[stage]=won").unwrap_err();
        assert_eq!(error.problems.len(), 1);
        assert_eq!(error.problems[0].param, "filter[password][contains]");
        assert!(error.problems[0].message.contains("use one of: stage"), "{error}");
    }

    #[test]
    fn an_operator_the_field_does_not_take_is_refused() {
        assert_eq!(params(strict("filter[hot][gte]=true").unwrap_err()), ["filter[hot][gte]"]);
        assert_eq!(params(strict("filter[code][contains]=x").unwrap_err()), ["filter[code][contains]"]);
        assert_eq!(params(strict("filter[stage][starts_with]=l").unwrap_err()), ["filter[stage][starts_with]"]);
        assert!(strict("filter[title][starts_with]=Su").is_ok());
        // A prefix is matched with LIKE, so its wildcards are not allowed.
        assert!(strict("filter[title][starts_with]=Su%25").is_err());
        assert!(strict("filter[title][starts_with]=S_").is_err());
        assert!(strict("filter[closes_on][lte]=2026-11-05").is_ok());
    }

    #[test]
    fn values_are_checked_against_the_kind() {
        for (query, param) in [
            ("filter[amount_cents]=1.5", "filter[amount_cents]"),
            ("filter[amount_cents][in]=1,x", "filter[amount_cents][in]"),
            ("filter[rate]=1e5", "filter[rate]"),
            ("filter[rate]=.5", "filter[rate]"),
            ("filter[hot]=yes", "filter[hot]"),
            ("filter[closes_on]=2026-13-01", "filter[closes_on]"),
            ("filter[closes_on]=2026-1-1", "filter[closes_on]"),
            ("filter[created_at][gte]=yesterday", "filter[created_at][gte]"),
            ("filter[owner_id]=not-a-uuid", "filter[owner_id]"),
            ("filter[owner_id]=00000000000000000000000000000000", "filter[owner_id]"),
            ("filter[stage]=won,lost", "filter[stage]"),
            ("filter[stage]=archived", "filter[stage]"),
        ] {
            assert_eq!(params(strict(query).unwrap_err()), [param], "{query}");
        }
        for query in [
            "filter[amount_cents]=-5",
            "filter[rate]=-12.50",
            "filter[hot]=false",
            "filter[closes_on]=2026-02-28",
            "filter[created_at][lt]=2026-11-05T10:30:00%2B05:30",
            "filter[created_at][lt]=2026-11-05",
            "filter[owner_id]=0190a5c4-7d2e-7c3a-9d1e-0f4b6a7c8d9e",
            "filter[stage][in]=lead,won",
        ] {
            assert!(strict(query).is_ok(), "{query}");
        }
    }

    #[test]
    fn sorts_search_and_page_sizes_are_limited() {
        assert_eq!(params(strict("sort=password").unwrap_err()), ["sort"]);
        assert_eq!(params(strict("per_page=101").unwrap_err()), ["per_page"]);
        assert_eq!(params(strict("limit=101").unwrap_err()), ["limit"]);
        assert_eq!(params(strict("before=abc").unwrap_err()), ["before"]);
        assert!(strict("per_page=100").is_ok());
        struct Quiet;
        impl ListResource for Quiet {
            const SPEC: ListSpec = ListSpec::new(&[]);
        }
        assert_eq!(params(Quiet::SPEC.parse("search=x").unwrap_err()), ["search"]);
    }

    #[test]
    fn every_problem_is_reported_at_once() {
        let error = strict("filter[x]=1&sort=y&page=0&filter[hot]=maybe").unwrap_err();
        assert_eq!(
            params(error),
            ["page", "filter[x]", "filter[hot]", "sort"]
        );
    }

    #[test]
    fn problems_become_a_400_naming_the_parameters() {
        let error = AppError::from(strict("filter[x]=1&page=0").unwrap_err());
        assert_eq!(error.status.as_u16(), 400);
        let errors = error.errors.unwrap();
        assert!(errors.contains_key("filter[x]"));
        assert!(errors.contains_key("page"));
    }

    #[test]
    fn the_extractor_reads_the_uri() {
        let uri: http::Uri = "/deals?filter[stage]=won&page=2".parse().unwrap();
        let list = List::<Deals>::from_uri(&uri).unwrap();
        assert_eq!(list.eq("stage"), Some("won"));
        assert_eq!(list.page, Some(2));
        let uri: http::Uri = "/deals?filter[nope]=1".parse().unwrap();
        assert!(List::<Deals>::from_uri(&uri).is_err());
    }

    #[test]
    fn the_query_string_round_trips() {
        let text = "filter%5Bstage%5D%5Bin%5D=lead%2Cwon&filter%5Bamount_cents%5D%5Bgte%5D=100&sort=-title&search=Sutlej+Supply&page=2&per_page=25";
        let (list, problems) = ListQuery::parse_lenient(text);
        assert!(problems.is_empty());
        let (again, _) = ListQuery::parse_lenient(&list.to_query_string());
        assert_eq!(list, again);
        assert_eq!(list.clone().on_page(1).page, None);
        assert_eq!(list.on_page(3).page, Some(3));
    }

    #[test]
    fn a_query_string_may_start_with_a_question_mark() {
        let (list, problems) = ListQuery::parse_lenient("?search=a");
        assert!(problems.is_empty());
        assert_eq!(list.search.as_deref(), Some("a"));
    }

    #[test]
    fn the_whole_query_is_clipped_not_trusted() {
        let long = format!("search={}", "a".repeat(MAX_QUERY_BYTES * 2));
        let (list, problems) = ListQuery::parse_lenient(&long);
        assert!(problems.iter().any(|p| p.param == "query"));
        assert!(list.search.is_none() || list.search.as_ref().unwrap().len() <= MAX_TEXT);
    }

    #[test]
    fn search_is_cut_on_a_character_boundary() {
        let text = "ਪ".repeat(200);
        let (list, problems) = ListQuery::parse_lenient(&format!("search={text}"));
        assert_eq!(problems.len(), 1);
        let search = list.search.unwrap();
        assert!(search.len() <= MAX_TEXT);
        assert!(search.chars().all(|c| c == 'ਪ'));
    }

    #[test]
    fn the_numbers_of_conditions_and_values_are_capped() {
        let many: String = (0..MAX_CONDITIONS + 10)
            .map(|n| format!("filter[f{n}]=1"))
            .collect::<Vec<_>>()
            .join("&");
        let (list, problems) = ListQuery::parse_lenient(&many);
        assert_eq!(list.filter.len(), MAX_CONDITIONS);
        assert!(problems.iter().any(|p| p.param == "filter"));

        let values: String = (0..MAX_VALUES + 50)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let (list, problems) = ListQuery::parse_lenient(&format!("filter[id][in]={values}"));
        assert_eq!(list.filter[0].values.len(), MAX_VALUES);
        assert_eq!(problems.len(), 1);
    }

    #[test]
    fn many_parameters_stop_the_read() {
        let many: String = (0..MAX_PARAMS + 5)
            .map(|n| format!("x{n}=1"))
            .collect::<Vec<_>>()
            .join("&");
        let (_, problems) = ListQuery::parse_lenient(&many);
        assert!(problems.iter().any(|p| p.param == "query"));
    }

    #[test]
    fn encoding_matches_url_search_params() {
        assert_eq!(encode_component("a b"), "a+b");
        assert_eq!(encode_component("a~b"), "a%7Eb");
        assert_eq!(encode_component("a*b-c.d_e"), "a*b-c.d_e");
        assert_eq!(encode_component("ਪ"), "%E0%A8%AA");
        assert_eq!(encode_component("a,b[c]"), "a%2Cb%5Bc%5D");
    }

    #[test]
    fn operators_know_their_names() {
        for op in Op::ALL {
            assert_eq!(Op::parse(op.name()), Some(op));
        }
        assert_eq!(Op::parse("GTE"), None);
        assert_eq!(Op::parse(""), None);
    }
}
