//! Route model binding: [`Bound<M>`] loads the record a route's path
//! parameter names, and [`BoundChild<P, C>`] a record under its parent
//! (`/companies/{company}/deals/{deal}`), as handler arguments.
//!
//! ```no_run
//! # use navree_orm::__doc_factories::*;
//! use navree_http::{AppError, Json};
//! use navree_orm::binding::{Bound, WithTrashed};
//!
//! // GET /deals/{deal}: the deal whose route key (its slug) is in the path;
//! // a 404 in Navree's error shape when there is none.
//! async fn show(deal: Bound<Deal>) -> Result<Json<Record<Deal>>, AppError> {
//!     Ok(Json(deal.into_inner()))
//! }
//!
//! // Soft-deleted deals bind too on this route.
//! async fn restore(Bound(deal, _): Bound<Deal, WithTrashed>) -> Result<Json<Record<Deal>>, AppError> {
//!     Ok(Json(deal))
//! }
//! ```
//!
//! **Which parameter.** A binding reads the path parameter named after the
//! model in snake case (`{deal}` for `Deal`, `{user_account}` for
//! `UserAccount`), else `{id}`, else the route's only parameter; a
//! [`RouteBinding`] can name another one.
//!
//! **Which column.** The model's route key (`#[model(route_key = "slug")]`,
//! else its key), unless the route's binding says otherwise
//! (`Bound<Deal, ByNumber>` with your own [`RouteBinding`]). The text is
//! read as the column's type first: a malformed UUID, ULID or number names
//! no record, so it is a 404 without a query.
//!
//! **Which records.** Bindings run the model's query with its global
//! scopes, in the request's unit of work: soft-deleted records are a 404
//! unless the route binds [`WithTrashed`], and a workspace-scoped model
//! only binds records of the request's workspace (another workspace's
//! record is a 404).

use std::fmt;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::str::FromStr;

use http::request::Parts;
use navree_db::{Db, Uow};
use navree_http::extract::FromRequestParts;
use navree_http::{AppError, PathParams};
use navree_kernel::AppContext;
use uuid::Uuid;

use crate::column::col;
use crate::error::OrmError;
use crate::model::{ColumnMeta, Model};
use crate::query::Builder;
use crate::record::Record;
use crate::ulid::Ulid;
use crate::value::Value;

/// How a [`Bound`] finds its record: the path parameter it reads, whether
/// soft-deleted records bind, and the query for the parameter's text.
///
/// [`RouteKey`] (the default) and [`WithTrashed`] cover the usual cases;
/// implement it on a marker type of your own for another column or a
/// custom query:
///
/// ```
/// # use navree_orm::__doc_factories::*;
/// use navree_orm::binding::{Bound, RouteBinding};
///
/// /// Deals by title, case-insensitively, from `{title}`.
/// pub struct ByTitle;
///
/// impl RouteBinding<Deal> for ByTitle {
///     const PARAM: Option<&'static str> = Some("title");
///
///     fn query(query: Builder<Deal>, value: &str) -> Option<Builder<Deal>> {
///         Some(query.where_like(Deal::TITLE, value.to_owned()))
///     }
/// }
///
/// async fn show(deal: Bound<Deal, ByTitle>) -> String {
///     deal.slug.clone()
/// }
/// ```
pub trait RouteBinding<M: Model>: Send + Sync + 'static {
    /// The path parameter to read; `None` for the default (the model's
    /// name in snake case, else `id`, else the only parameter).
    const PARAM: Option<&'static str> = None;

    /// Whether soft-deleted records bind.
    const WITH_TRASHED: bool = false;

    /// `query` narrowed to the record `value` names; `None` when `value`
    /// cannot name one (the binding answers 404 without a query).
    fn query(query: Builder<M>, value: &str) -> Option<Builder<M>>;
}

/// The default binding: the model's route key
/// ([`Model::route_key_name`]), the text read as the column's type.
#[derive(Clone, Copy, Debug, Default)]
pub struct RouteKey;

impl<M: Model> RouteBinding<M> for RouteKey {
    fn query(query: Builder<M>, value: &str) -> Option<Builder<M>> {
        let column = M::route_key_name();
        let meta = M::meta().column(column)?;
        let value = route_value(meta, value)?;
        Some(query.where_(col(format!("{}.{column}", M::meta().table())).eq(value)))
    }
}

/// Binding `B` (the route key by default) with soft-deleted records too:
/// `Bound<Deal, WithTrashed>`, `Bound<Deal, WithTrashed<ByNumber>>`.
pub struct WithTrashed<B = RouteKey>(PhantomData<fn() -> B>);

impl<B> fmt::Debug for WithTrashed<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("WithTrashed")
    }
}

impl<M: Model, B: RouteBinding<M>> RouteBinding<M> for WithTrashed<B> {
    const PARAM: Option<&'static str> = B::PARAM;
    const WITH_TRASHED: bool = true;

    fn query(query: Builder<M>, value: &str) -> Option<Builder<M>> {
        B::query(query, value)
    }
}

/// The text of a path parameter as a value of column `meta`'s type; `None`
/// when it cannot be one (a malformed UUID, ULID, number or boolean).
pub(crate) fn route_value(meta: &ColumnMeta, text: &str) -> Option<Value> {
    let rust_type = meta.rust_type();
    let inner = rust_type
        .strip_prefix("Option<")
        .and_then(|rest| rest.strip_suffix('>'))
        .unwrap_or(rust_type);
    let last = inner.rsplit("::").next().unwrap_or(inner);
    match last {
        "Uuid" => Uuid::parse_str(text).ok().map(Value::from),
        "Ulid" => Ulid::from_str(text)
            .ok()
            .map(|ulid| Value::from(ulid.to_string())),
        "i8" | "i16" | "i32" | "i64" | "isize" => text.parse::<i64>().ok().map(Value::from),
        "u8" | "u16" | "u32" | "u64" | "usize" => text.parse::<u64>().ok().map(Value::from),
        "bool" => text.parse::<bool>().ok().map(Value::from),
        _ => Some(Value::from(text.to_owned())),
    }
}

/// The record a route's path parameter names: a handler argument that
/// loads it in the request's unit of work, or answers 404 in Navree's error
/// shape. It dereferences to the [`Record`] (or destructure it:
/// `Bound(deal, _)`); see the [module documentation](crate::binding) for
/// the parameter, the column and the scopes.
pub struct Bound<M: Model, B = RouteKey>(pub Record<M>, pub PhantomData<fn() -> B>);

impl<M: Model, B> Bound<M, B> {
    /// The record.
    pub fn into_inner(self) -> Record<M> {
        self.0
    }
}

impl<M: Model, B> Deref for Bound<M, B> {
    type Target = Record<M>;

    fn deref(&self) -> &Record<M> {
        &self.0
    }
}

impl<M: Model, B> DerefMut for Bound<M, B> {
    fn deref_mut(&mut self) -> &mut Record<M> {
        &mut self.0
    }
}

impl<M: Model + fmt::Debug, B> fmt::Debug for Bound<M, B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Bound").field(&self.0).finish()
    }
}

impl<M, B> FromRequestParts for Bound<M, B>
where
    M: Model,
    B: RouteBinding<M>,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        let params = path_params(parts);
        let param = B::PARAM
            .map(str::to_owned)
            .or_else(|| default_param::<M>(&params, Position::Only))
            .ok_or_else(|| missing_param::<M>(&params))?;
        let text = param_value(&params, &param).ok_or_else(|| missing_param::<M>(&params))?;
        let record = bind::<M, B>(parts, M::query(), text).await?;
        Ok(Bound(record, PhantomData))
    }
}

/// How a child model's records are found under a parent record in nested
/// routes (`/teams/{team}/members/{member}`): implement it once per pair,
/// narrowing the child's query to the parent's children.
///
/// ```
/// use navree_orm::binding::ChildOf;
/// use navree_orm::prelude::*;
///
/// #[derive(Model, Clone, Debug, Default, serde::Serialize)]
/// pub struct Team {
///     id: i64,
///     name: String,
/// }
///
/// #[derive(Model, Clone, Debug, Default, serde::Serialize)]
/// pub struct Member {
///     id: i64,
///     team_id: i64,
///     name: String,
/// }
///
/// impl ChildOf<Team> for Member {
///     fn children_of(team: &Record<Team>, query: Builder<Member>) -> Builder<Member> {
///         query.where_(Member::TEAM_ID.eq(team.id))
///     }
/// }
/// ```
pub trait ChildOf<P: Model>: Model {
    /// `query` narrowed to the children of `parent`.
    fn children_of(parent: &Record<P>, query: Builder<Self>) -> Builder<Self>;
}

/// A parent record and one of its children from a nested route
/// (`/companies/{company}/deals/{deal}`): the parent is bound like
/// [`Bound<P>`], then the child among the parent's children only
/// ([`ChildOf`]), so a deal of another company is a 404. The parent comes
/// from the parameter named after `P` (else the first parameter), the
/// child from the one named after `C` (else the last).
///
/// ```no_run
/// # use navree_orm::__doc_factories::*;
/// // With `impl ChildOf<Company> for Deal` (deals of the company).
/// use navree_orm::binding::BoundChild;
///
/// async fn show(deal: BoundChild<Company, Deal>) -> String {
///     format!("{} at {}", deal.child().title, deal.parent().name)
/// }
/// ```
pub struct BoundChild<P: Model, C: Model, B = RouteKey> {
    parent: Record<P>,
    child: Record<C>,
    _binding: PhantomData<fn() -> B>,
}

impl<P: Model, C: Model, B> BoundChild<P, C, B> {
    /// The parent record.
    pub fn parent(&self) -> &Record<P> {
        &self.parent
    }

    /// The child record.
    pub fn child(&self) -> &Record<C> {
        &self.child
    }

    /// The parent and the child.
    pub fn into_parts(self) -> (Record<P>, Record<C>) {
        (self.parent, self.child)
    }
}

impl<P: Model + fmt::Debug, C: Model + fmt::Debug, B> fmt::Debug for BoundChild<P, C, B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BoundChild")
            .field("parent", &self.parent)
            .field("child", &self.child)
            .finish()
    }
}

impl<P, C, B> FromRequestParts for BoundChild<P, C, B>
where
    P: Model,
    C: ChildOf<P>,
    B: RouteBinding<C>,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        let params = path_params(parts);
        let parent_param = default_param::<P>(&params, Position::First)
            .ok_or_else(|| missing_param::<P>(&params))?;
        let child_param = B::PARAM
            .map(str::to_owned)
            .or_else(|| default_param::<C>(&params, Position::Last))
            .filter(|param| *param != parent_param)
            .ok_or_else(|| missing_param::<C>(&params))?;
        let parent_text =
            param_value(&params, &parent_param).ok_or_else(|| missing_param::<P>(&params))?;
        let child_text =
            param_value(&params, &child_param).ok_or_else(|| missing_param::<C>(&params))?;
        let parent = bind::<P, RouteKey>(parts, P::query(), parent_text).await?;
        let child = bind::<C, B>(parts, C::children_of(&parent, C::query()), child_text).await?;
        Ok(BoundChild {
            parent,
            child,
            _binding: PhantomData,
        })
    }
}

/// The route's path parameters, in route order (the router has decoded
/// them, and refused a value that is not valid percent-encoded UTF-8).
fn path_params(parts: &Parts) -> Vec<(String, String)> {
    parts
        .extensions
        .get::<PathParams>()
        .map_or_else(Vec::new, |params| {
            params
                .iter()
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
                .collect()
        })
}

/// The text of parameter `name`.
fn param_value<'a>(params: &'a [(String, String)], name: &str) -> Option<&'a str> {
    params
        .iter()
        .find(|(param, _)| param == name)
        .map(|(_, value)| value.as_str())
}

/// Where a parameter sits when none is named after the model.
#[derive(Clone, Copy)]
enum Position {
    Only,
    First,
    Last,
}

/// The parameter of model `M`: named after it, else `id` (single
/// bindings) or the only one, or the first or last of a nested route.
fn default_param<M: Model>(params: &[(String, String)], position: Position) -> Option<String> {
    let named = snake_case(M::meta().name());
    if param_value(params, &named).is_some() {
        return Some(named);
    }
    match position {
        Position::Only => {
            if param_value(params, "id").is_some() {
                return Some("id".to_owned());
            }
            match params {
                [(only, _)] => Some(only.clone()),
                _ => None,
            }
        }
        Position::First => params.first().map(|(name, _)| name.clone()),
        Position::Last => params.last().map(|(name, _)| name.clone()),
    }
}

/// A 500 for a route whose parameters a binding cannot use.
fn missing_param<M: Model>(params: &[(String, String)]) -> AppError {
    let names: Vec<&str> = params.iter().map(|(name, _)| name.as_str()).collect();
    AppError::internal(format!(
        "cannot bind {}: the route has no `{{{}}}` parameter (it has: {})",
        M::meta().name(),
        snake_case(M::meta().name()),
        if names.is_empty() {
            "none".to_owned()
        } else {
            names.join(", ")
        }
    ))
}

/// Loads the record of `M` that `text` names through binding `B`, on the
/// request's unit of work.
async fn bind<M: Model, B: RouteBinding<M>>(
    parts: &Parts,
    query: Builder<M>,
    text: &str,
) -> Result<Record<M>, AppError> {
    let Some(query) = B::query(query, text) else {
        return Err(AppError::not_found());
    };
    let query = if B::WITH_TRASHED {
        query.with_trashed()
    } else {
        query
    };
    let found = match parts.extensions.get::<Uow>() {
        Some(uow) => query.first(uow).await,
        None => {
            let ctx = parts.extensions.get::<AppContext>().ok_or_else(|| {
                AppError::internal("route model binding needs the navree request pipeline")
            })?;
            let db = ctx.get::<Db>().await.map_err(|_| {
                AppError::internal(format!(
                    "cannot bind {}: add DatabaseModule to the app",
                    M::meta().name()
                ))
            })?;
            query.first(&*db).await
        }
    };
    match found {
        Ok(Some(record)) => Ok(record),
        Ok(None) => Err(AppError::not_found()),
        Err(error) if error.is_not_found() => Err(AppError::not_found()),
        Err(error) => Err(binding_error::<M>(error)),
    }
}

/// A 500 for a binding query that failed.
fn binding_error<M: Model>(error: OrmError) -> AppError {
    AppError::internal(format!("cannot bind {}: {error}", M::meta().name()))
}

/// `UserAccount` → `user_account`.
pub(crate) fn snake_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    let chars: Vec<char> = name.chars().collect();
    for (index, c) in chars.iter().enumerate() {
        if c.is_uppercase() {
            let previous_lower =
                index > 0 && !chars[index - 1].is_uppercase() && chars[index - 1] != '_';
            let next_lower = chars.get(index + 1).is_some_and(|next| next.is_lowercase());
            let previous_upper = index > 0 && chars[index - 1].is_uppercase();
            if index > 0 && (previous_lower || (previous_upper && next_lower)) {
                out.push('_');
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(*c);
        }
    }
    out
}

impl<M: Model> Record<M> {
    /// The value of the model's route key column
    /// ([`Model::route_key_name`]): what a URL to the record carries.
    ///
    /// ```
    /// # use navree_orm::__doc_factories::*;
    /// let deal = Record::new(Deal { slug: "agra-renewal".into(), ..Deal::default() });
    /// assert_eq!(deal.route_key()?, Value::from("agra-renewal"));
    /// # Ok::<(), navree_orm::OrmError>(())
    /// ```
    pub fn route_key(&self) -> crate::Result<Value> {
        let column = M::route_key_name();
        self.model
            .value(column)?
            .ok_or_else(|| crate::model::derive_support::unknown_column::<M>(column))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_names_become_parameter_names() {
        assert_eq!(snake_case("Deal"), "deal");
        assert_eq!(snake_case("UserAccount"), "user_account");
        assert_eq!(snake_case("HTTPRequest"), "http_request");
        assert_eq!(snake_case("OAuthClient"), "o_auth_client");
        assert_eq!(snake_case("Tag2"), "tag2");
    }

    #[test]
    fn path_text_is_read_as_the_columns_type() {
        let uuid = ColumnMeta::new("id", "id", "Uuid");
        let id = Uuid::now_v7();
        assert_eq!(route_value(&uuid, &id.to_string()), Some(Value::from(id)));
        assert_eq!(route_value(&uuid, "not-a-uuid"), None);
        let int = ColumnMeta::new("id", "id", "i64");
        assert_eq!(route_value(&int, "42"), Some(Value::from(42_i64)));
        assert_eq!(route_value(&int, "4x2"), None);
        let unsigned = ColumnMeta::new("id", "id", "u32");
        assert_eq!(route_value(&unsigned, "-1"), None);
        let ulid = ColumnMeta::new("id", "id", "navree_orm::Ulid");
        assert_eq!(route_value(&ulid, "nope"), None);
        let text = ColumnMeta::new("slug", "slug", "Option<String>");
        assert_eq!(route_value(&text, "agra"), Some(Value::from("agra")));
    }
}
