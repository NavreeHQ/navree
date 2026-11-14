//! [`BulkRequest`]: what a bulk action posts, read back against the definition.
//!
//! The browser holds the selection. It posts either the keys, or "all that match
//! this state, except these, and I saw this many". The server runs the same
//! definition against the same scoped query, so the action can't reach a row the
//! user couldn't see, and refuses when the set isn't what was shown.

use http::StatusCode;
use navree_http::body::{BodyLimit, read_body};
use navree_http::extract::{FromRequest, Request, urlencoded};
use serde::de::DeserializeOwned;

use super::query::Shape;
use super::{FilterBar, NoFilter, TableQuery, TableRow};
use crate::op::{Op, Reply, ToastTone};

/// The most keys one post can name: selected, or excluded from "all". A larger
/// post is refused whole: cutting it would act on rows the user had excluded.
pub const MAX_SELECTED_IDS: usize = 10_000;

/// The field a native client posts, in place of the keys, to choose every row that
/// matches the table's query: its value is how many matched when the table was drawn,
/// which the server holds the action to as `_expected` is held.
pub(crate) const ALL_FIELD: &str = "_all";

/// What a bulk action selects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BulkMode {
    /// These keys.
    Ids,
    /// All rows matching the state, except these keys.
    All,
}

/// A bulk action's post. Take it as an extractor, check the user may do this, and
/// resolve it (with the `orm` feature, `BulkRequest::resolve`) to the query of
/// exactly those rows.
#[derive(Clone, Debug)]
pub struct BulkRequest<R, F = NoFilter> {
    action: String,
    mode: BulkMode,
    ids: Vec<String>,
    excluded: Vec<String>,
    expected: Option<u64>,
    version: Option<String>,
    table: String,
    pub(crate) query: TableQuery<R, F>,
}

impl<R: TableRow, F: FilterBar + Default + DeserializeOwned> BulkRequest<R, F> {
    /// Reads a posted form.
    pub fn parse(body: &[u8]) -> Result<Self, StatusCode> {
        Self::parse_shaped(body, Shape::of::<R>())
    }

    /// Reads a posted form against `shape`: the columns of a table described at run
    /// time.
    pub(crate) fn parse_shaped(body: &[u8], shape: Shape) -> Result<Self, StatusCode> {
        let mut action = String::new();
        let mut mode = BulkMode::Ids;
        let mut ids = Vec::new();
        let mut excluded = Vec::new();
        let mut expected = None;
        let mut version = None;
        let mut table = String::new();
        let mut path = String::new();
        let mut state = String::new();
        let key_kind = shape.config.key_kind;
        for (name, value) in urlencoded::parse(body) {
            match name.as_ref() {
                "action" => action = value.into_owned(),
                "_mode" if value == "all" => mode = BulkMode::All,
                // A native client's way of saying the same: all, and how many it was told.
                ALL_FIELD => {
                    mode = BulkMode::All;
                    expected = value.parse().ok();
                }
                "ids" | "ex" => {
                    let list = if name == "ids" {
                        &mut ids
                    } else {
                        &mut excluded
                    };
                    if list.len() >= MAX_SELECTED_IDS {
                        return Err(StatusCode::PAYLOAD_TOO_LARGE);
                    }
                    // Keys that can't be keys can't name a row: refuse rather than
                    // quietly act on the rest.
                    if !key_kind.accepts(&value) {
                        return Err(StatusCode::UNPROCESSABLE_ENTITY);
                    }
                    list.push(value.into_owned());
                }
                "_expected" => expected = value.parse().ok(),
                "_version" if !value.is_empty() => version = Some(value.into_owned()),
                "_table" => table = value.into_owned(),
                "_path" => path = value.into_owned(),
                "_q" => state = value.into_owned(),
                _ => {}
            }
        }
        // A path only ever goes back to this site.
        if !path.starts_with('/') || path.starts_with("//") || path.contains(['\\', '\r', '\n']) {
            path = "/".to_owned();
        }
        if !table
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            table.clear();
        }
        if mode == BulkMode::All && expected.is_none() {
            return Err(StatusCode::UNPROCESSABLE_ENTITY);
        }
        Ok(Self {
            action,
            mode,
            ids,
            excluded,
            expected,
            version,
            table,
            query: TableQuery::parse_shaped(path, &state, shape),
        })
    }
}

impl<R: TableRow, F: FilterBar> BulkRequest<R, F> {
    /// Takes columns away from the person asking, as
    /// [`TableQuery::without_columns`] does for the table they were drawn in: "all that
    /// match" then selects by the search and filters the table was allowed to have, and
    /// the count it is held to is that of the same rows. Call it with the same names.
    #[must_use]
    pub fn without_columns<S: AsRef<str>>(mut self, names: impl IntoIterator<Item = S>) -> Self {
        self.query = self.query.without_columns(names);
        self
    }

    /// Lets the person asking use only these columns, as [`TableQuery::only_columns`]
    /// does for the table they were drawn in.
    #[must_use]
    pub fn only_columns<S: AsRef<str>>(mut self, names: impl IntoIterator<Item = S>) -> Self {
        self.query = self.query.only_columns(names);
        self
    }

    /// Which action.
    pub fn action(&self) -> &str {
        &self.action
    }

    /// What it selects.
    pub fn mode(&self) -> &BulkMode {
        &self.mode
    }

    /// The keys selected, in [`BulkMode::Ids`].
    pub fn ids(&self) -> &[String] {
        &self.ids
    }

    /// The keys left out, in [`BulkMode::All`].
    pub fn excluded(&self) -> &[String] {
        &self.excluded
    }

    /// How many rows the user saw selected.
    pub fn expected(&self) -> Option<u64> {
        self.expected
    }

    /// The version the user saw.
    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    /// The table's state when the action was posted.
    pub fn query(&self) -> &TableQuery<R, F> {
        &self.query
    }

    /// Where a page with JavaScript off goes back to.
    pub fn back(&self) -> String {
        self.query.href()
    }

    /// The action worked on `count` rows: says so, and redraws the table. A
    /// successful reply is also what ends the selection in the browser.
    pub fn done(&self, text: impl Into<String>) -> Reply {
        Reply::ops_or_redirect(self.back())
            .toast(ToastTone::Success, text)
            .op(Op::Invalidate {
                target: self.table.clone(),
            })
    }

    /// Refuses, keeping the selection: says why, and redraws the table.
    pub fn refused(&self, status: StatusCode, text: impl Into<String>) -> Reply {
        Reply::ops_or_redirect(self.back())
            .toast(ToastTone::Danger, text)
            .op(Op::Invalidate {
                target: self.table.clone(),
            })
            .status(status)
    }
}

/// Why a bulk action didn't run: the rows matching now aren't the rows shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BulkRefusal {
    /// How many the user saw selected.
    pub shown: u64,
    /// How many are selected now.
    pub now: u64,
}

impl BulkRefusal {
    /// The refusal's words.
    pub fn message(&self) -> String {
        if self.shown == self.now {
            format!(
                "Nothing changed: rows were edited since you selected {}. Check them and try again.",
                super::view::grouped(self.shown)
            )
        } else {
            format!(
                "Nothing changed: you selected {}, but {} match now. Check them and try again.",
                super::view::grouped(self.shown),
                super::view::grouped(self.now)
            )
        }
    }
}

impl<R, F> FromRequest for BulkRequest<R, F>
where
    R: TableRow + Send + 'static,
    F: FilterBar + Default + DeserializeOwned + Send + 'static,
{
    type Rejection = StatusCode;

    async fn from_request(request: Request) -> Result<Self, Self::Rejection> {
        let (parts, body) = request.into_parts();
        // The same doors as any form: not another site, not a GET, not another body.
        if let Some(status) = crate::form::refusal(&parts).await {
            return Err(status);
        }
        let limit = BodyLimit::of(&parts.extensions);
        let bytes = read_body(body, limit)
            .await
            .map_err(|_| StatusCode::PAYLOAD_TOO_LARGE)?;
        Self::parse(&bytes)
    }
}
