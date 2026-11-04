//! List queries on models: [`Builder::list`] applies a [`ListQuery`] that
//! navree-http read against a [`ListSpec`] (the filters, the search and the
//! order), and [`ListQueryExt`] turns its paging into [`PageRequest`] or
//! [`CursorRequest`] for [`paginate`](Builder::paginate) and
//! [`cursor_paginate`](Builder::cursor_paginate).
//!
//! ```no_run
//! # use navree_orm::__doc_models::*;
//! use navree_db::{Page, Uow};
//! use navree_http::list::{Field, List, ListResource, ListSpec};
//! use navree_orm::list::ListQueryExt;
//! use navree_orm::Record;
//!
//! struct Deals;
//!
//! impl ListResource for Deals {
//!     const SPEC: ListSpec = ListSpec::new(&[
//!         Field::one_of("stage", &["lead", "won", "lost"]),
//!         Field::int("amount_cents"),
//!     ])
//!     .sortable(&["amount_cents", "created_at"])
//!     .default_sort(&["-created_at"])
//!     .searchable(&["title"]);
//! }
//!
//! async fn index(uow: Uow, list: List<Deals>) -> navree_http::Result<Page<Record<Deal>>> {
//!     Ok(Deal::query()
//!         .list(&list, &Deals::SPEC)?
//!         .paginate(&uow, list.page_request(&Deals::SPEC))
//!         .await?)
//! }
//! ```
//!
//! What each operator does to the query:
//!
//! | Operator | Condition |
//! |---|---|
//! | `eq`, `ne`, `gt`, `gte`, `lt`, `lte` | the comparison, with the value read as the field's type |
//! | `in`, `not_in` | `IN (...)`, `NOT IN (...)` |
//! | `contains` | contains the text, ignoring case (like [`Column::contains_ignore_case`](crate::Column::contains_ignore_case)) |
//! | `starts_with` | `LIKE 'text%'`, ignoring case |
//! | `null` | `IS NULL` (`true`) or `IS NOT NULL` (`false`) |
//!
//! `ne` and `not_in` are SQL's: a row whose value is NULL matches neither.
//! A date-and-time field takes a plain date as the whole day in UTC:
//! `filter[created_at][lte]=2026-11-05` includes everything on the 5th.

use chrono::{DateTime, NaiveDate, Utc};
use uuid::Uuid;

use crate::column::{Filter, col, col_as};
use crate::error::{OrmError, Result};
use crate::model::Model;
use crate::pagination::{CursorRequest, PageRequest};
use crate::query::Builder;
use crate::value::Value;

pub use navree_http::list::*;

impl<M: Model> Builder<M> {
    /// Applies a list query: its filters and search as conditions, and its
    /// sort (or the list's default) as the order, with the primary key last
    /// so rows with equal values keep one order from page to page. A sort
    /// replaces any order the query had; a list with no sort at all leaves
    /// it.
    ///
    /// `list` must have been read against `spec` (by [`ListSpec::parse`] or
    /// the [`List`] extractor): a field `spec` does not declare, or a value
    /// of the wrong type, is an [`OrmError::InvalidQuery`], never SQL.
    pub fn list(mut self, list: &ListQuery, spec: &ListSpec) -> Result<Self> {
        for condition in &list.filter {
            let field = spec.field(&condition.field).ok_or_else(|| {
                OrmError::InvalidQuery(format!(
                    "`{}` is not a filter of this list",
                    condition.field
                ))
            })?;
            if !field.by_app {
                self = self.where_(filter_of(field, condition)?);
            }
        }
        if let (Some(text), Search::Columns(columns)) = (&list.search, spec.search) {
            let matches: Vec<Filter> = columns
                .iter()
                .map(|column| col_as::<String>(*column).contains_ignore_case(text.clone()))
                .collect();
            if !matches.is_empty() {
                self = self.where_(Filter::any(matches));
            }
        }
        let sorts = spec.sort_of(list);
        if !sorts.is_empty() {
            self = self.reorder();
            let mut last_descending = false;
            for key in &sorts {
                let name = sort_column(spec, &key.field)?;
                self = self.order(if key.descending {
                    col(name).desc()
                } else {
                    col(name).asc()
                });
                last_descending = key.descending;
            }
            let primary = M::meta().key_column();
            if !sorts.iter().any(|key| key.field == primary) {
                self = self.order(if last_descending {
                    col(primary).desc()
                } else {
                    col(primary).asc()
                });
            }
        }
        Ok(self)
    }
}

/// The column a sort field names: the field itself, which the list's
/// sortable names are (`created_at`, `amount_cents`); a name no `Field`
/// declares can only come from the default sort.
fn sort_column(spec: &ListSpec, name: &str) -> Result<&'static str> {
    if let Some(sort) = spec.sorts.iter().copied().find(|sort| *sort == name) {
        return Ok(sort);
    }
    spec.default_sort
        .iter()
        .map(|term| term.strip_prefix('-').unwrap_or(term))
        .find(|sort| *sort == name)
        .ok_or_else(|| OrmError::InvalidQuery(format!("`{name}` is not a sort of this list")))
}

/// What the paging of a [`ListQuery`] asks of the ORM.
pub trait ListQueryExt {
    /// The page for [`Builder::paginate`] and [`Builder::simple_paginate`]:
    /// the query's page and page size, the list's default size otherwise.
    fn page_request(&self, spec: &ListSpec) -> PageRequest;

    /// The page for [`Builder::cursor_paginate`].
    fn cursor_request(&self, spec: &ListSpec) -> CursorRequest;
}

impl ListQueryExt for ListQuery {
    fn page_request(&self, spec: &ListSpec) -> PageRequest {
        match spec.paging(self) {
            Paging::Pages { page, per_page } => PageRequest {
                page,
                per_page: Some(per_page),
            },
            Paging::Cursor { limit, .. } => PageRequest {
                page: 1,
                per_page: Some(limit),
            },
        }
    }

    fn cursor_request(&self, spec: &ListSpec) -> CursorRequest {
        match spec.paging(self) {
            Paging::Cursor { cursor, limit, .. } => CursorRequest {
                cursor,
                limit: Some(limit),
            },
            Paging::Pages { per_page, .. } => CursorRequest {
                cursor: None,
                limit: Some(per_page),
            },
        }
    }
}

fn invalid(field: &Field, text: &str, what: &str) -> OrmError {
    OrmError::InvalidQuery(format!(
        "`{text}` is not {what} (filter `{}`)",
        field.name
    ))
}

/// A value from a query string, bound as what the field holds.
fn bind(field: &Field, text: &str) -> Result<Value> {
    Ok(match field.kind {
        Kind::Text | Kind::OneOf(_) => Value::String(text.to_owned()),
        Kind::Int => Value::I64(
            text.parse()
                .map_err(|_| invalid(field, text, "a whole number"))?,
        ),
        Kind::Decimal => Value::Decimal(
            text.parse()
                .map_err(|_| invalid(field, text, "a decimal number"))?,
        ),
        Kind::Bool => match text {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            _ => return Err(invalid(field, text, "true or false")),
        },
        Kind::Date => Value::Date(date(field, text)?),
        Kind::DateTime => match DateTime::parse_from_rfc3339(text) {
            Ok(at) => Value::Timestamp(at.with_timezone(&Utc)),
            Err(_) => Value::Timestamp(start_of(date(field, text)?)),
        },
        Kind::Uuid => Value::Uuid(
            Uuid::parse_str(text).map_err(|_| invalid(field, text, "a UUID"))?,
        ),
    })
}

fn date(field: &Field, text: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").map_err(|_| invalid(field, text, "a date"))
}

/// Midnight UTC at the start of `day`.
fn start_of(day: NaiveDate) -> DateTime<Utc> {
    day.and_time(chrono::NaiveTime::MIN).and_utc()
}

/// Whether `text` is a plain date where a date-and-time is expected.
fn is_whole_day(field: &Field, text: &str) -> bool {
    field.kind == Kind::DateTime && DateTime::parse_from_rfc3339(text).is_err()
}

/// The filter of one condition.
fn filter_of(field: &Field, condition: &Condition) -> Result<Filter> {
    let source = col(field.source());
    let text = condition.value();
    let compare = |op: Op| -> Result<Filter> {
        if is_whole_day(field, text) {
            // A date stands for its whole day.
            let start = start_of(date(field, text)?);
            let next = start + chrono::Duration::days(1);
            let (start, next) = (Value::Timestamp(start), Value::Timestamp(next));
            return Ok(match op {
                Op::Eq => Filter::all([source.gte(start), source.lt(next)]),
                Op::Ne => Filter::any([source.lt(start), source.gte(next)]),
                Op::Gt => source.gte(next),
                Op::Gte => source.gte(start),
                Op::Lt => source.lt(start),
                _ => source.lt(next),
            });
        }
        let value = bind(field, text)?;
        Ok(match op {
            Op::Eq => source.eq(value),
            Op::Ne => source.ne(value),
            Op::Gt => source.gt(value),
            Op::Gte => source.gte(value),
            Op::Lt => source.lt(value),
            _ => source.lte(value),
        })
    };
    Ok(match condition.op {
        Op::Eq | Op::Ne | Op::Gt | Op::Gte | Op::Lt | Op::Lte => compare(condition.op)?,
        Op::In | Op::NotIn => {
            let values = condition
                .values
                .iter()
                .map(|text| bind(field, text))
                .collect::<Result<Vec<Value>>>()?;
            if condition.op == Op::In {
                source.is_in(values)
            } else {
                source.not_in(values)
            }
        }
        Op::Contains => col_as::<String>(field.source()).contains_ignore_case(text.to_owned()),
        Op::StartsWith => {
            if text.contains(['%', '_', '\\']) {
                return Err(OrmError::InvalidQuery(format!(
                    "a `starts_with` value for `{}` cannot contain %, _ or \\",
                    field.name
                )));
            }
            col_as::<String>(field.source()).like(format!("{text}%"))
        }
        Op::Null => {
            if condition.null_flag() == Some(true) {
                source.is_null()
            } else {
                source.is_not_null()
            }
        }
    })
}
