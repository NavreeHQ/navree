//! Pagination of model queries.
//!
//! | Method | Returns | Queries | JSON |
//! |---|---|---|---|
//! | [`paginate`](Builder::paginate) | [`Page`] (navree-db's) | a count and a page | `{"data", "meta": {"current_page", "per_page", "total", "last_page"}}` |
//! | [`simple_paginate`](Builder::simple_paginate) | [`SimplePage`] | one (a row more than the page) | `{"data", "meta": {"current_page", "per_page", "has_more"}}` |
//! | [`cursor_paginate`](Builder::cursor_paginate) | [`CursorPage`] | one, by the query's order | `{"data", "next_cursor", "prev_cursor"}` |
//!
//! Handlers take the page from the request with navree-db's
//! [`PageQuery`] (`?page=&per_page=`) or this module's [`CursorRequest`]
//! (`?cursor=&limit=`); both answer malformed parameters with a 400.
//!
//! ```no_run
//! # use navree_orm::__doc_models::*;
//! use std::sync::Arc;
//!
//! use navree_db::{Db, PageQuery, Uow};
//! use navree_orm::{CursorPage, CursorRequest, Page, Record};
//!
//! async fn index(uow: Uow, page: PageQuery) -> navree_http::Result<Page<Record<Deal>>> {
//!     Ok(Deal::query().latest().paginate(&uow, page).await?)
//! }
//!
//! async fn feed(db: Arc<Db>, cursor: CursorRequest) -> navree_http::Result<CursorPage<Record<Event>>> {
//!     Ok(Event::query().latest().cursor_paginate(&db, cursor).await?)
//! }
//! ```

use std::collections::BTreeMap;

use axum::extract::{FromRequestParts, Query};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use http::request::Parts;
use navree_http::{AppError, Json};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

pub use navree_db::{MAX_PER_PAGE, Page, PageMeta, PageQuery};

use crate::error::{OrmError, Result};
use crate::exec::{Access, Executor};
use crate::ir::{Boolean, ColumnRef, Direction, Expr, Operator, Order, Where, WhereClause};
use crate::model::Model;
use crate::query::Builder;
use crate::record::Record;
use crate::value::{Decimal, Value};

/// Which page [`paginate`](Builder::paginate) and
/// [`simple_paginate`](Builder::simple_paginate) load: a page number (with
/// the model's page size), a `(page, per_page)` pair, or navree-db's
/// [`PageQuery`] (the request's `?page=&per_page=`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageRequest {
    /// The 1-based page number.
    pub page: u64,
    /// The page size; the model's when `None`.
    pub per_page: Option<u64>,
}

impl PageRequest {
    /// The page and page size, the model's size by default.
    fn resolve(self, default_per_page: u64) -> PageQuery {
        PageQuery {
            page: self.page.max(1),
            per_page: self.per_page.unwrap_or(default_per_page).max(1),
        }
    }
}

impl From<u64> for PageRequest {
    fn from(page: u64) -> Self {
        PageRequest {
            page,
            per_page: None,
        }
    }
}

impl From<(u64, u64)> for PageRequest {
    fn from((page, per_page): (u64, u64)) -> Self {
        PageRequest {
            page,
            per_page: Some(per_page),
        }
    }
}

impl From<PageQuery> for PageRequest {
    fn from(query: PageQuery) -> Self {
        PageRequest {
            page: query.page,
            per_page: Some(query.per_page),
        }
    }
}

/// The metadata of a [`SimplePage`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimplePageMeta {
    /// The page number.
    pub current_page: u64,
    /// The page size.
    pub per_page: u64,
    /// Whether a next page has records.
    pub has_more: bool,
}

/// A page without a total (one query instead of two):
/// `{"data": [...], "meta": {"current_page", "per_page", "has_more"}}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SimplePage<T> {
    /// The items.
    pub data: Vec<T>,
    /// The pagination metadata.
    pub meta: SimplePageMeta,
}

impl<T> SimplePage<T> {
    /// Converts every item, keeping the metadata.
    pub fn map<U>(self, f: impl FnMut(T) -> U) -> SimplePage<U> {
        SimplePage {
            data: self.data.into_iter().map(f).collect(),
            meta: self.meta,
        }
    }
}

impl<T: Serialize> IntoResponse for SimplePage<T> {
    fn into_response(self) -> Response {
        Json(self).into_response()
    }
}

/// A page of a cursor-paginated query: `{"data": [...], "next_cursor",
/// "prev_cursor"}`. Pass a cursor back as `?cursor=` for the next or the
/// previous page; `null` means there is none.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CursorPage<T> {
    /// The items.
    pub data: Vec<T>,
    /// The cursor of the next page.
    pub next_cursor: Option<String>,
    /// The cursor of the previous page.
    pub prev_cursor: Option<String>,
}

impl<T> CursorPage<T> {
    /// Converts every item, keeping the cursors.
    pub fn map<U>(self, f: impl FnMut(T) -> U) -> CursorPage<U> {
        CursorPage {
            data: self.data.into_iter().map(f).collect(),
            next_cursor: self.next_cursor,
            prev_cursor: self.prev_cursor,
        }
    }
}

impl<T: Serialize> IntoResponse for CursorPage<T> {
    fn into_response(self) -> Response {
        Json(self).into_response()
    }
}

/// Which cursor page to load: `?cursor=&limit=` (the limit defaults to the
/// model's page size and is clamped to 100 as a request parameter).
///
/// As an extractor it answers a malformed cursor or a limit that is not a
/// positive integer with a 400.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CursorRequest {
    /// A `next_cursor` or `prev_cursor` of an earlier page; `None` for the
    /// first page.
    pub cursor: Option<String>,
    /// The page size; the model's when `None`.
    pub limit: Option<u64>,
}

impl CursorRequest {
    /// The first page.
    pub fn first() -> Self {
        CursorRequest::default()
    }

    /// The page `cursor` points to.
    pub fn after(cursor: impl Into<String>) -> Self {
        CursorRequest {
            cursor: Some(cursor.into()),
            limit: None,
        }
    }

    /// The same page with at most `limit` items (at least 1).
    pub fn limit(mut self, limit: u64) -> Self {
        self.limit = Some(limit.max(1));
        self
    }
}

impl From<Option<String>> for CursorRequest {
    fn from(cursor: Option<String>) -> Self {
        CursorRequest {
            cursor,
            limit: None,
        }
    }
}

#[derive(Deserialize)]
struct RawCursorRequest {
    cursor: Option<String>,
    limit: Option<String>,
}

impl<S: Send + Sync> FromRequestParts<S> for CursorRequest {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let Query(raw) = Query::<RawCursorRequest>::try_from_uri(&parts.uri)
            .map_err(|rejection| AppError::bad_request(rejection.body_text()))?;
        let mut errors = BTreeMap::new();
        let limit = match raw.limit.as_deref().map(str::trim) {
            None | Some("") => None,
            Some(text) => match text.parse::<u64>() {
                Ok(limit) if limit >= 1 => Some(limit.min(MAX_PER_PAGE)),
                _ => {
                    errors.insert(
                        "limit".to_owned(),
                        vec!["limit must be a positive integer".to_owned()],
                    );
                    None
                }
            },
        };
        let cursor = raw.cursor.filter(|cursor| !cursor.trim().is_empty());
        if let Some(cursor) = &cursor
            && Cursor::decode(cursor).is_err()
        {
            errors.insert("cursor".to_owned(), vec!["cursor is invalid".to_owned()]);
        }
        if errors.is_empty() {
            Ok(CursorRequest { cursor, limit })
        } else {
            Err(AppError::validation(errors))
        }
    }
}

/// Where a cursor points from its record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Towards {
    /// The records after it.
    Next,
    /// The records before it.
    Previous,
}

/// A decoded cursor: the order columns and their values at one record.
#[derive(Clone, Debug, PartialEq)]
struct Cursor {
    towards: Towards,
    columns: Vec<String>,
    values: Vec<Value>,
}

impl Cursor {
    /// The opaque text: base64url of a small JSON document whose values
    /// keep their types (so a date-time binds as a date-time).
    fn encode(&self) -> String {
        let document = json!({
            "d": match self.towards { Towards::Next => "n", Towards::Previous => "p" },
            "c": self.columns,
            "v": self.values.iter().map(tag).collect::<Vec<_>>(),
        });
        URL_SAFE_NO_PAD.encode(document.to_string())
    }

    fn decode(text: &str) -> Result<Cursor> {
        let invalid = || OrmError::invalid("the pagination cursor is invalid");
        let bytes = URL_SAFE_NO_PAD.decode(text.trim()).map_err(|_| invalid())?;
        let document: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        let towards = match document.get("d").and_then(serde_json::Value::as_str) {
            Some("n") => Towards::Next,
            Some("p") => Towards::Previous,
            _ => return Err(invalid()),
        };
        let columns: Vec<String> = document
            .get("c")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(invalid)?
            .iter()
            .map(|column| column.as_str().map(str::to_owned).ok_or_else(invalid))
            .collect::<Result<_>>()?;
        let values: Vec<Value> = document
            .get("v")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(invalid)?
            .iter()
            .map(untag)
            .collect::<Result<_>>()?;
        if columns.is_empty() || columns.len() != values.len() {
            return Err(invalid());
        }
        Ok(Cursor {
            towards,
            columns,
            values,
        })
    }
}

/// A value with its type: `["i64", 42]`, `["utc", "2026-10-27T08:30:00Z"]`.
fn tag(value: &Value) -> serde_json::Value {
    fn text<T: ToString>(name: &str, value: &Option<T>) -> serde_json::Value {
        match value {
            Some(value) => json!([name, value.to_string()]),
            None => json!(["null"]),
        }
    }
    match value {
        Value::Bool(v) => v.map_or(json!(["null"]), |v| json!(["b", v])),
        Value::TinyInt(v) => v.map_or(json!(["null"]), |v| json!(["i8", v])),
        Value::SmallInt(v) => v.map_or(json!(["null"]), |v| json!(["i16", v])),
        Value::Int(v) => v.map_or(json!(["null"]), |v| json!(["i32", v])),
        Value::BigInt(v) => v.map_or(json!(["null"]), |v| json!(["i64", v])),
        Value::TinyUnsigned(v) => v.map_or(json!(["null"]), |v| json!(["u8", v])),
        Value::SmallUnsigned(v) => v.map_or(json!(["null"]), |v| json!(["u16", v])),
        Value::Unsigned(v) => v.map_or(json!(["null"]), |v| json!(["u32", v])),
        Value::BigUnsigned(v) => v.map_or(json!(["null"]), |v| json!(["u64", v])),
        Value::Float(v) => v.map_or(json!(["null"]), |v| json!(["f32", v])),
        Value::Double(v) => v.map_or(json!(["null"]), |v| json!(["f64", v])),
        Value::String(v) => text("s", v),
        Value::Char(v) => text("s", v),
        Value::ChronoDate(v) => text("date", v),
        Value::ChronoTime(v) => text("time", v),
        Value::ChronoDateTime(v) => text("naive", &v.map(|v| v.format("%Y-%m-%dT%H:%M:%S%.f"))),
        Value::ChronoDateTimeUtc(v) => text("utc", &v.map(|v| v.to_rfc3339())),
        Value::ChronoDateTimeLocal(v) => {
            text("utc", &v.map(|v| v.with_timezone(&Utc).to_rfc3339()))
        }
        Value::ChronoDateTimeWithTimeZone(v) => text("tz", &v.map(|v| v.to_rfc3339())),
        Value::Uuid(v) => text("uuid", v),
        Value::Decimal(v) => text("dec", v),
        // A driver's typed text (a MongoDB ObjectId): its type and text.
        Value::Enum(sea_query::OptionEnum::Some(value)) => {
            json!(["enum", &*value.type_name, value.value])
        }
        Value::Enum(sea_query::OptionEnum::None(_)) => json!(["null"]),
        other => json!(["s", crate::value::to_json(other).to_string()]),
    }
}

/// The value [`tag`] wrote.
fn untag(tagged: &serde_json::Value) -> Result<Value> {
    let invalid = || OrmError::invalid("the pagination cursor is invalid");
    let parts = tagged.as_array().ok_or_else(invalid)?;
    let name = parts
        .first()
        .and_then(serde_json::Value::as_str)
        .ok_or_else(invalid)?;
    if name == "null" {
        return Ok(Value::String(None));
    }
    let raw = parts.get(1).ok_or_else(invalid)?;
    let int = || raw.as_i64().ok_or_else(invalid);
    let unsigned = || raw.as_u64().ok_or_else(invalid);
    let text = || raw.as_str().ok_or_else(invalid);
    Ok(match name {
        "b" => Value::Bool(Some(raw.as_bool().ok_or_else(invalid)?)),
        "i8" => Value::TinyInt(Some(i8::try_from(int()?).map_err(|_| invalid())?)),
        "i16" => Value::SmallInt(Some(i16::try_from(int()?).map_err(|_| invalid())?)),
        "i32" => Value::Int(Some(i32::try_from(int()?).map_err(|_| invalid())?)),
        "i64" => Value::BigInt(Some(int()?)),
        "u8" => Value::TinyUnsigned(Some(u8::try_from(unsigned()?).map_err(|_| invalid())?)),
        "u16" => Value::SmallUnsigned(Some(u16::try_from(unsigned()?).map_err(|_| invalid())?)),
        "u32" => Value::Unsigned(Some(u32::try_from(unsigned()?).map_err(|_| invalid())?)),
        "u64" => Value::BigUnsigned(Some(unsigned()?)),
        // Narrowing back to the precision the value was written with.
        #[allow(clippy::cast_possible_truncation)]
        "f32" => Value::Float(Some(raw.as_f64().ok_or_else(invalid)? as f32)),
        "f64" => Value::Double(Some(raw.as_f64().ok_or_else(invalid)?)),
        "s" => Value::String(Some(text()?.to_owned())),
        "date" => Value::ChronoDate(Some(text()?.parse::<NaiveDate>().map_err(|_| invalid())?)),
        "time" => Value::ChronoTime(Some(text()?.parse::<NaiveTime>().map_err(|_| invalid())?)),
        "naive" => Value::ChronoDateTime(Some(
            NaiveDateTime::parse_from_str(text()?, "%Y-%m-%dT%H:%M:%S%.f")
                .map_err(|_| invalid())?,
        )),
        "utc" => Value::ChronoDateTimeUtc(Some(
            DateTime::parse_from_rfc3339(text()?)
                .map_err(|_| invalid())?
                .with_timezone(&Utc),
        )),
        "tz" => Value::ChronoDateTimeWithTimeZone(Some(
            DateTime::<FixedOffset>::parse_from_rfc3339(text()?).map_err(|_| invalid())?,
        )),
        "uuid" => Value::Uuid(Some(Uuid::parse_str(text()?).map_err(|_| invalid())?)),
        "dec" => Value::Decimal(Some(text()?.parse::<Decimal>().map_err(|_| invalid())?)),
        "enum" => {
            let value = parts
                .get(2)
                .and_then(serde_json::Value::as_str)
                .ok_or_else(invalid)?;
            Value::Enum(sea_query::OptionEnum::Some(Box::new(sea_query::Enum {
                type_name: text()?.into(),
                value: value.to_owned().into(),
            })))
        }
        _ => return Err(invalid()),
    })
}

/// The column of an order a cursor can follow: a plain column with a
/// direction.
fn cursor_column(order: &Order) -> Result<(ColumnRef, Direction)> {
    match order {
        Order::By {
            expr: Expr::Column(column),
            direction,
            ..
        } if column.path.is_empty() => Ok((column.clone(), *direction)),
        other => Err(OrmError::invalid(format!(
            "cursor pagination orders by columns only, not {other:?}"
        ))),
    }
}

/// `(c1, c2, ...) > (v1, v2, ...)` in the query's directions: for each
/// column, the earlier ones equal and this one past its value.
fn after(columns: &[(ColumnRef, Direction)], values: &[Value], towards: Towards) -> Where {
    let mut alternatives = Vec::with_capacity(columns.len());
    for (index, (column, direction)) in columns.iter().enumerate() {
        let mut clauses: Vec<WhereClause> = columns[..index]
            .iter()
            .zip(values)
            .map(|((earlier, _), value)| {
                WhereClause::and(Where::Compare {
                    left: Expr::Column(earlier.clone()),
                    op: Operator::Eq,
                    right: Expr::Value(value.clone()),
                })
            })
            .collect();
        let ascending = (*direction == Direction::Asc) == (towards == Towards::Next);
        clauses.push(WhereClause::and(Where::Compare {
            left: Expr::Column(column.clone()),
            op: if ascending {
                Operator::Gt
            } else {
                Operator::Lt
            },
            right: Expr::Value(values[index].clone()),
        }));
        alternatives.push(WhereClause {
            boolean: Boolean::Or,
            negated: false,
            filter: Where::Group(clauses),
        });
    }
    Where::Group(alternatives)
}

impl<M: Model> Builder<M> {
    /// Page `page` of the query and the total number of records: one count
    /// and one page query. Order the query for stable pages. A page past
    /// the end is empty (its rows are not queried).
    ///
    /// `page` is a page number (with the model's page size, 15 unless the
    /// model sets `per_page`), a `(page, per_page)` pair, or navree-db's
    /// [`PageQuery`] extractor.
    pub async fn paginate<E: Executor + ?Sized>(
        self,
        exec: &E,
        page: impl Into<PageRequest>,
    ) -> Result<Page<Record<M>>> {
        let page = page.into().resolve(M::meta().default_per_page());
        let target = exec.target(Access::Read).await?;
        let total = self.clone().count_on(&target).await?;
        let past_the_end = (page.page - 1)
            .checked_mul(page.per_page)
            .is_none_or(|offset| offset >= total);
        let data = if past_the_end {
            Vec::new()
        } else {
            self.offset((page.page - 1) * page.per_page)
                .limit(page.per_page)
                .fetch(&target)
                .await?
                .into_vec()
        };
        Ok(Page::new(data, page, total))
    }

    /// Page `page` of the query without a total: one query for a record
    /// more than the page, which tells whether another page follows.
    pub async fn simple_paginate<E: Executor + ?Sized>(
        self,
        exec: &E,
        page: impl Into<PageRequest>,
    ) -> Result<SimplePage<Record<M>>> {
        let page = page.into().resolve(M::meta().default_per_page());
        let target = exec.target(Access::Read).await?;
        let offset = (page.page - 1).saturating_mul(page.per_page);
        let mut data = self
            .offset(offset)
            .limit(page.per_page.saturating_add(1))
            .fetch(&target)
            .await?
            .into_vec();
        let has_more = data.len() as u64 > page.per_page;
        data.truncate(usize::try_from(page.per_page).unwrap_or(usize::MAX));
        Ok(SimplePage {
            data,
            meta: SimplePageMeta {
                current_page: page.page,
                per_page: page.per_page,
                has_more,
            },
        })
    }

    /// The page of records after (or before) a cursor, by the query's
    /// order: no count and no offset, so pages stay fast and stable while
    /// records are added. The key is appended to the order as a
    /// tie-breaker; an unordered query pages by key.
    ///
    /// The order must be by plain, non-NULL columns (model columns or
    /// selected ones), in any mix of directions.
    pub async fn cursor_paginate<E: Executor + ?Sized>(
        self,
        exec: &E,
        request: impl Into<CursorRequest>,
    ) -> Result<CursorPage<Record<M>>> {
        let request = request.into();
        let meta = M::meta();
        let per_page = request.limit.unwrap_or(meta.default_per_page()).max(1);
        let mut query = self;
        let key = ColumnRef::new(Some(meta.table()), meta.key_column());
        let has_key = query.ir.orders.iter().any(|order| {
            matches!(order, Order::By { expr: Expr::Column(column), .. }
                if column.name == key.name && column.table.as_deref().is_none_or(|table| table == meta.table()))
        });
        if !has_key {
            query.ir.orders.push(Order::asc(Expr::Column(key)));
        }
        let columns = query
            .ir
            .orders
            .iter()
            .map(cursor_column)
            .collect::<Result<Vec<_>>>()?;
        let names: Vec<String> = columns
            .iter()
            .map(|(column, _)| column.name.to_string())
            .collect();

        let cursor = request.cursor.as_deref().map(Cursor::decode).transpose()?;
        if let Some(cursor) = &cursor {
            if cursor.columns != names {
                return Err(OrmError::invalid(format!(
                    "the pagination cursor is for columns {:?}, but the query orders by {names:?}",
                    cursor.columns
                )));
            }
            let condition = after(&columns, &cursor.values, cursor.towards);
            query = query.where_(crate::column::Filter::from(condition));
            if cursor.towards == Towards::Previous {
                for order in &mut query.ir.orders {
                    if let Order::By { direction, .. } = order {
                        *direction = direction.reversed();
                    }
                }
            }
        }

        let target = exec.target(Access::Read).await?;
        let mut data = query
            .limit(per_page.saturating_add(1))
            .fetch(&target)
            .await?
            .into_vec();
        let has_more = data.len() as u64 > per_page;
        data.truncate(usize::try_from(per_page).unwrap_or(usize::MAX));
        let backwards = cursor
            .as_ref()
            .is_some_and(|c| c.towards == Towards::Previous);
        if backwards {
            data.reverse();
        }

        let cursor_at = |record: &Record<M>, towards: Towards| -> Result<String> {
            let values = names
                .iter()
                .map(|name| match record.model.value(name)? {
                    Some(value) => Ok(value),
                    None => record
                        .extras()
                        .iter()
                        .find(|(column, _)| column == name)
                        .map(|(_, value)| value.clone())
                        .ok_or_else(|| {
                            OrmError::invalid(format!(
                                "cursor pagination orders by `{name}`, which the query does not select"
                            ))
                        }),
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(Cursor {
                towards,
                columns: names.clone(),
                values,
            }
            .encode())
        };

        let next_cursor = match (&cursor, data.last()) {
            (None, _) if !has_more => None,
            (Some(c), _) if c.towards == Towards::Next && !has_more => None,
            (_, None) => None,
            (_, Some(last)) => Some(cursor_at(last, Towards::Next)?),
        };
        let prev_cursor = match (&cursor, data.first()) {
            (None, _) => None,
            (Some(c), _) if c.towards == Towards::Previous && !has_more => None,
            (_, None) => None,
            (_, Some(first)) => Some(cursor_at(first, Towards::Previous)?),
        };
        Ok(CursorPage {
            data,
            next_cursor,
            prev_cursor,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursors_round_trip_with_their_types() {
        let at = DateTime::parse_from_rfc3339("2026-10-27T08:30:00.123456Z")
            .unwrap()
            .with_timezone(&Utc);
        let cursor = Cursor {
            towards: Towards::Previous,
            columns: vec![
                "created_at".into(),
                "id".into(),
                "score".into(),
                "amount".into(),
            ],
            values: vec![
                Value::ChronoDateTimeUtc(Some(at)),
                Value::Uuid(Some(Uuid::nil())),
                Value::Double(Some(2.5)),
                Value::Decimal(Some(Decimal::new(1250, 2))),
            ],
        };
        let text = cursor.encode();
        assert!(
            text.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        );
        assert_eq!(Cursor::decode(&text).unwrap(), cursor);
    }

    /// Typed values a driver carries as enums (a MongoDB ObjectId) keep
    /// their type through a cursor, so the next page compares like with
    /// like.
    #[test]
    fn cursors_keep_enum_values() {
        let object_id = Value::Enum(sea_query::OptionEnum::Some(Box::new(sea_query::Enum {
            type_name: "ObjectId".into(),
            value: "65f1c0ffee0000000000002a".into(),
        })));
        let cursor = Cursor {
            towards: Towards::Next,
            columns: vec!["_id".into(), "kind".into()],
            values: vec![
                object_id,
                Value::Enum(sea_query::OptionEnum::None("ObjectId".into())),
            ],
        };
        let decoded = Cursor::decode(&cursor.encode()).unwrap();
        assert_eq!(decoded.values[0], cursor.values[0]);
        assert!(crate::is_null(&decoded.values[1]));
    }

    #[test]
    fn malformed_cursors_are_rejected() {
        for text in [
            "",
            "!!",
            "e30",
            &URL_SAFE_NO_PAD.encode(r#"{"d":"n","c":["a"],"v":[]}"#),
        ] {
            assert!(Cursor::decode(text).is_err(), "{text}");
        }
        let forged = URL_SAFE_NO_PAD.encode(r#"{"d":"n","c":["a"],"v":[["utc","yesterday"]]}"#);
        assert!(Cursor::decode(&forged).is_err());
    }

    #[test]
    fn page_requests_default_to_the_model_page_size() {
        assert_eq!(
            PageRequest::from(3).resolve(25),
            PageQuery {
                page: 3,
                per_page: 25
            }
        );
        assert_eq!(
            PageRequest::from((2, 10)).resolve(25),
            PageQuery {
                page: 2,
                per_page: 10
            }
        );
        assert_eq!(PageRequest::from(0).resolve(25).page, 1);
    }
}
