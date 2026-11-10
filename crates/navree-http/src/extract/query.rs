//! [`Query`] and [`RawQuery`]: the query string.

use std::convert::Infallible;
use std::ops::{Deref, DerefMut};

use http::request::Parts;
use serde::de::DeserializeOwned;

use super::rejection::QueryRejection;
use super::{FromRequestParts, urlencoded};

/// The query string, deserialized into `T` (see
/// [`urlencoded`](super::urlencoded) for how pairs become values). A
/// missing query string is an empty one, so a `T` whose fields are all
/// `Option` or have defaults always reads.
///
/// ```
/// use http::Method;
/// use navree_http::{Query, RouteDef};
/// use serde::Deserialize;
///
/// #[derive(Deserialize)]
/// struct Page {
///     page: Option<u32>,
///     per_page: Option<u32>,
/// }
///
/// async fn index(Query(page): Query<Page>) -> String {
///     format!("{:?}", page.page)
/// }
///
/// let route = RouteDef::on(Method::GET, "/deals", index).public();
/// # let _ = route;
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Query<T>(pub T);

impl<T> Deref for Query<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> DerefMut for Query<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl<T: DeserializeOwned> Query<T> {
    /// Deserializes `T` from the query string of `uri`.
    pub fn try_from_uri(uri: &http::Uri) -> Result<Self, QueryRejection> {
        let query = uri.query().unwrap_or_default();
        serde_path_to_error::deserialize(urlencoded::Deserializer::new(query.as_bytes()))
            .map(Query)
            .map_err(|error| QueryRejection(error.to_string()))
    }
}

impl<T: DeserializeOwned + Send> FromRequestParts for Query<T> {
    type Rejection = QueryRejection;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        Self::try_from_uri(&parts.uri)
    }
}

/// The query string as sent (still percent-encoded), `None` without one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RawQuery(pub Option<String>);

impl FromRequestParts for RawQuery {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        Ok(RawQuery(parts.uri.query().map(str::to_owned)))
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    #[derive(Debug, Deserialize, PartialEq)]
    struct Filter {
        owner_id: Option<uuid::Uuid>,
        page: Option<u32>,
    }

    fn query(uri: &str) -> Result<Filter, String> {
        Query::<Filter>::try_from_uri(&uri.parse().unwrap())
            .map(|Query(filter)| filter)
            .map_err(|rejection| rejection.to_string())
    }

    #[test]
    fn the_query_string_deserializes_with_paths_in_errors() {
        assert_eq!(
            query("/deals"),
            Ok(Filter {
                owner_id: None,
                page: None
            })
        );
        assert_eq!(query("/deals?page=3").unwrap().page, Some(3));
        assert_eq!(
            query("/deals?page=x").unwrap_err(),
            "Failed to deserialize query string: page: invalid digit found in string"
        );
        let error = query("/deals?owner_id=not-a-uuid").unwrap_err();
        assert!(
            error.starts_with("Failed to deserialize query string: owner_id: UUID parsing failed"),
            "{error}"
        );
    }

    #[tokio::test]
    async fn raw_query_is_the_query_as_sent() {
        let request = http::Request::get("/x?a=%20b&c").body(()).unwrap();
        let (mut parts, ()) = request.into_parts();
        let RawQuery(query) = RawQuery::from_request_parts(&mut parts).await.unwrap();
        assert_eq!(query.as_deref(), Some("a=%20b&c"));
    }
}
