//! Offset pagination ([`PageQuery`], [`Page`], [`paginate`]) and cursor
//! pagination ([`CursorQuery`], [`CursorPage`], [`cursor_paginate`]).

use std::collections::BTreeMap;

use axum::extract::{FromRequestParts, Query};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use http::request::Parts;
use navree_http::{AppError, Json};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DbErr, EntityTrait, ModelTrait, Order, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect, Select, Value,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The default page size.
pub const DEFAULT_PER_PAGE: u64 = 15;

/// The largest page size; larger requests are clamped.
pub const MAX_PER_PAGE: u64 = 100;

/// `?page=&per_page=` (defaults 1 and 15; `per_page` is clamped to 100).
///
/// As an extractor it ignores other query parameters and rejects a `page`
/// or `per_page` that is not a positive integer with a 400.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PageQuery {
    /// The 1-based page number.
    pub page: u64,
    /// The page size.
    pub per_page: u64,
}

impl Default for PageQuery {
    fn default() -> Self {
        Self {
            page: 1,
            per_page: DEFAULT_PER_PAGE,
        }
    }
}

impl PageQuery {
    /// Page `page` of `per_page` items (`page` at least 1, `per_page`
    /// within 1..=100).
    pub fn new(page: u64, per_page: u64) -> Self {
        Self {
            page: page.max(1),
            per_page: per_page.clamp(1, MAX_PER_PAGE),
        }
    }

    /// The number of rows before this page.
    pub fn offset(&self) -> u64 {
        (self.page.max(1) - 1).saturating_mul(self.per_page)
    }
}

#[derive(Deserialize)]
struct RawPageQuery {
    page: Option<String>,
    per_page: Option<String>,
}

impl<S: Send + Sync> FromRequestParts<S> for PageQuery {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let Query(raw) = Query::<RawPageQuery>::try_from_uri(&parts.uri)
            .map_err(|rejection| AppError::bad_request(rejection.body_text()))?;
        let mut errors = BTreeMap::new();
        let page = positive(
            raw.page.as_deref(),
            1,
            "page",
            "page must be a positive integer",
            &mut errors,
        );
        let per_page = positive(
            raw.per_page.as_deref(),
            DEFAULT_PER_PAGE,
            "per_page",
            "per_page must be a positive integer",
            &mut errors,
        );
        if errors.is_empty() {
            Ok(PageQuery::new(page, per_page))
        } else {
            Err(AppError::validation(errors))
        }
    }
}

/// Parses an optional positive integer parameter, recording a field error.
fn positive(
    raw: Option<&str>,
    default: u64,
    field: &str,
    message: &str,
    errors: &mut BTreeMap<String, Vec<String>>,
) -> u64 {
    match raw.map(str::trim) {
        None | Some("") => default,
        Some(value) => match value.parse::<u64>() {
            Ok(n) if n >= 1 => n,
            _ => {
                errors.insert(field.to_owned(), vec![message.to_owned()]);
                default
            }
        },
    }
}

/// The pagination metadata of a [`Page`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageMeta {
    /// The page number.
    pub current_page: u64,
    /// The page size.
    pub per_page: u64,
    /// The number of matching rows.
    pub total: u64,
    /// The last page number (at least 1).
    pub last_page: u64,
}

/// One page of results: `{"data": [...], "meta": {"current_page",
/// "per_page", "total", "last_page"}}`. Handlers can return it directly.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Page<T> {
    /// The items.
    pub data: Vec<T>,
    /// The pagination metadata.
    pub meta: PageMeta,
}

impl<T> Page<T> {
    /// A page of `data` for query `q` out of `total` rows.
    pub fn new(data: Vec<T>, q: PageQuery, total: u64) -> Self {
        let per_page = q.per_page.max(1);
        Self {
            data,
            meta: PageMeta {
                current_page: q.page.max(1),
                per_page,
                total,
                last_page: total.div_ceil(per_page).max(1),
            },
        }
    }

    /// Converts every item (e.g. models into DTOs), keeping the metadata.
    pub fn map<U>(self, f: impl FnMut(T) -> U) -> Page<U> {
        Page {
            data: self.data.into_iter().map(f).collect(),
            meta: self.meta,
        }
    }
}

impl<T: Serialize> IntoResponse for Page<T> {
    fn into_response(self) -> Response {
        Json(self).into_response()
    }
}

/// Runs `select` for page `q` (one `COUNT(*)` and one `LIMIT/OFFSET`
/// query). Order `select` yourself for stable pages. A page past the end
/// (any page number, however large) is empty; its rows are not queried.
pub async fn paginate<E, C>(
    select: Select<E>,
    conn: &C,
    q: PageQuery,
) -> Result<Page<E::Model>, DbErr>
where
    E: EntityTrait,
    E::Model: Send + Sync,
    C: ConnectionTrait,
{
    let q = PageQuery::new(q.page, q.per_page);
    let paginator = select.paginate(conn, q.per_page);
    let total = paginator.num_items().await?;
    // Only offsets below `total` reach SeaORM, whose `page * per_page`
    // would overflow (and whose SQL bind panics above `i64::MAX`).
    let past_the_end = (q.page - 1)
        .checked_mul(q.per_page)
        .is_none_or(|offset| offset >= total);
    let data = if past_the_end {
        Vec::new()
    } else {
        paginator.fetch_page(q.page - 1).await?
    };
    Ok(Page::new(data, q, total))
}

/// `?cursor=&limit=` for cursor pagination (`limit` defaults to 15 and is
/// clamped to 100). As an extractor it rejects a malformed cursor or a
/// non-positive limit with a 400.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CursorQuery {
    /// The `next_cursor` of the previous page; `None` for the first page.
    pub cursor: Option<String>,
    /// The page size.
    pub limit: u64,
}

impl Default for CursorQuery {
    fn default() -> Self {
        Self {
            cursor: None,
            limit: DEFAULT_PER_PAGE,
        }
    }
}

impl CursorQuery {
    /// The page after `cursor` with at most `limit` items (1..=100).
    pub fn new(cursor: Option<String>, limit: u64) -> Self {
        Self {
            cursor,
            limit: limit.clamp(1, MAX_PER_PAGE),
        }
    }
}

#[derive(Deserialize)]
struct RawCursorQuery {
    cursor: Option<String>,
    limit: Option<String>,
}

impl<S: Send + Sync> FromRequestParts<S> for CursorQuery {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let Query(raw) = Query::<RawCursorQuery>::try_from_uri(&parts.uri)
            .map_err(|rejection| AppError::bad_request(rejection.body_text()))?;
        let mut errors = BTreeMap::new();
        let limit = positive(
            raw.limit.as_deref(),
            DEFAULT_PER_PAGE,
            "limit",
            "limit must be a positive integer",
            &mut errors,
        );
        let cursor = raw.cursor.filter(|c| !c.trim().is_empty());
        if let Some(cursor) = &cursor
            && decode_cursor(cursor).is_none()
        {
            errors.insert("cursor".to_owned(), vec!["cursor is invalid".to_owned()]);
        }
        if errors.is_empty() {
            Ok(CursorQuery::new(cursor, limit))
        } else {
            Err(AppError::validation(errors))
        }
    }
}

/// One page of cursor-paginated results: `{"data": [...], "next_cursor"}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CursorPage<T> {
    /// The items.
    pub data: Vec<T>,
    /// Pass as `cursor` to get the next page; `None` on the last page.
    pub next_cursor: Option<String>,
}

impl<T> CursorPage<T> {
    /// Converts every item, keeping the cursor.
    pub fn map<U>(self, f: impl FnMut(T) -> U) -> CursorPage<U> {
        CursorPage {
            data: self.data.into_iter().map(f).collect(),
            next_cursor: self.next_cursor,
        }
    }
}

impl<T: Serialize> IntoResponse for CursorPage<T> {
    fn into_response(self) -> Response {
        Json(self).into_response()
    }
}

/// Cursor pagination over a UUID v7 `column` (usually the primary key),
/// oldest first. `select` may filter but must not be ordered.
pub async fn cursor_paginate<E, C>(
    select: Select<E>,
    column: E::Column,
    conn: &C,
    q: CursorQuery,
) -> Result<CursorPage<E::Model>, DbErr>
where
    E: EntityTrait,
    E::Model: Send + Sync,
    C: ConnectionTrait,
{
    cursor_page(select, column, conn, q, Order::Asc).await
}

/// Like [`cursor_paginate`], newest first.
pub async fn cursor_paginate_desc<E, C>(
    select: Select<E>,
    column: E::Column,
    conn: &C,
    q: CursorQuery,
) -> Result<CursorPage<E::Model>, DbErr>
where
    E: EntityTrait,
    E::Model: Send + Sync,
    C: ConnectionTrait,
{
    cursor_page(select, column, conn, q, Order::Desc).await
}

async fn cursor_page<E, C>(
    select: Select<E>,
    column: E::Column,
    conn: &C,
    q: CursorQuery,
    order: Order,
) -> Result<CursorPage<E::Model>, DbErr>
where
    E: EntityTrait,
    E::Model: Send + Sync,
    C: ConnectionTrait,
{
    let limit = q.limit.clamp(1, MAX_PER_PAGE);
    let mut select = select;
    if let Some(cursor) = q.cursor.as_deref() {
        let after = decode_cursor(cursor)
            .ok_or_else(|| DbErr::Type("invalid pagination cursor".to_owned()))?;
        select = match order {
            Order::Desc => select.filter(column.lt(after)),
            _ => select.filter(column.gt(after)),
        };
    }
    let mut data = select
        .order_by(column, order)
        .limit(limit + 1)
        .all(conn)
        .await?;
    let next_cursor = if data.len() as u64 > limit {
        data.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
        match data.last() {
            Some(last) => Some(encode_cursor(&last.get(column))?),
            None => None,
        }
    } else {
        None
    };
    Ok(CursorPage { data, next_cursor })
}

/// The opaque cursor for a UUID column value.
fn encode_cursor(value: &Value) -> Result<String, DbErr> {
    let id = match value {
        Value::Uuid(Some(id)) => *id,
        Value::String(Some(s)) => Uuid::parse_str(s)
            .map_err(|_| DbErr::Type("the cursor column must hold UUIDs".to_owned()))?,
        _ => {
            return Err(DbErr::Type("the cursor column must hold UUIDs".to_owned()));
        }
    };
    Ok(URL_SAFE_NO_PAD.encode(id.as_bytes()))
}

fn decode_cursor(cursor: &str) -> Option<Uuid> {
    let bytes = URL_SAFE_NO_PAD.decode(cursor.trim()).ok()?;
    Uuid::from_slice(&bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursors_round_trip() {
        let id = Uuid::now_v7();
        let cursor = encode_cursor(&Value::Uuid(Some(id))).unwrap();
        assert_eq!(decode_cursor(&cursor), Some(id));
        assert_eq!(decode_cursor("!!"), None);
        assert_eq!(decode_cursor("AAAA"), None, "wrong length");
    }

    #[test]
    fn offsets() {
        assert_eq!(PageQuery::new(1, 15).offset(), 0);
        assert_eq!(PageQuery::new(3, 10).offset(), 20);
        assert_eq!(PageQuery::new(0, 0), PageQuery::new(1, 1));
    }
}
