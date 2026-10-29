//! [`SearchError`] and its error responses.

use navree_db::DbError;
use navree_http::AppError;
use navree_kernel::BoxError;
use navree_orm::OrmError;

/// Everything that can go wrong while searching or indexing.
///
/// Returned from a handler with `?`, an invalid filter or sort field is a
/// 400, an unreachable or timed-out engine a 503 ("Search is unavailable.
/// Try again shortly."), and the rest a 500.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SearchError {
    /// The field is not marked `#[searchable(filter)]`.
    #[error("`{field}` is not a filterable field of the `{index}` index")]
    NotFilterable {
        /// The index (before the prefix).
        index: String,
        /// The field.
        field: String,
    },
    /// The field is not marked `#[searchable(sort)]`.
    #[error("`{field}` is not a sortable field of the `{index}` index")]
    NotSortable {
        /// The index (before the prefix).
        index: String,
        /// The field.
        field: String,
    },
    /// No registered model has this name (commands take the `name`).
    #[error("no searchable model is named `{0}`; register it with SearchModule::for_feature")]
    UnknownIndex(String),
    /// The model is not registered with `SearchModule::for_feature`.
    #[error("`{0}` is not registered with SearchModule::for_feature")]
    NotRegistered(&'static str),
    /// The index is scoped to workspaces and the query names none.
    #[error("the `{index}` index is scoped to workspaces: name one with `.workspace(id)`")]
    WorkspaceRequired {
        /// The index (before the prefix).
        index: String,
    },
    /// The engine could not be reached, timed out or failed on its side.
    #[error("the {engine} search engine is unavailable: {message}")]
    Unavailable {
        /// The engine.
        engine: &'static str,
        /// What happened.
        message: String,
    },
    /// The engine refused the request.
    #[error("the {engine} search engine refused the request: {message}")]
    Engine {
        /// The engine.
        engine: &'static str,
        /// The engine's answer.
        message: String,
    },
    /// The engine cannot do what the query asks.
    #[error("the {engine} search engine does not support {feature}")]
    Unsupported {
        /// The engine.
        engine: &'static str,
        /// What it cannot do.
        feature: String,
    },
    /// The index settings are invalid.
    #[error("invalid settings for the `{index}` index: {message}")]
    InvalidSettings {
        /// The index (before the prefix).
        index: String,
        /// What is wrong.
        message: String,
    },
    /// A vector query without `search.vectors.store`.
    #[error("vector search is off: set `search.vectors.store`")]
    VectorsOff,
    /// Vector search needs a `dyn Embeddings` provider in the app.
    #[error(
        "vector search needs an embeddings provider: register an `Arc<dyn navree_search::Embeddings>`"
    )]
    NoEmbeddings,
    /// The embeddings provider failed.
    #[error("cannot embed text: {0}")]
    Embeddings(#[source] BoxError),
    /// `Searchable::document` failed.
    #[error("cannot build the `{index}` document of {id}: {source}")]
    Document {
        /// The index (before the prefix).
        index: String,
        /// The record's primary key.
        id: String,
        /// The error.
        #[source]
        source: BoxError,
    },
    /// A database error.
    #[error(transparent)]
    Db(#[from] DbError),
    /// An ORM error (reading the records).
    #[error(transparent)]
    Orm(#[from] OrmError),
    /// A queue error (queued syncs).
    #[error(transparent)]
    Queue(#[from] navree_queue::QueueError),
    /// Anything else.
    #[error("{0}")]
    Other(String),
}

impl SearchError {
    /// The error response (also what `map_error` registers).
    pub fn to_app_error(&self) -> AppError {
        map_search_error(self).unwrap_or_else(|| AppError::internal(self.to_string()))
    }

    /// Whether the engine was unreachable (worth retrying later).
    pub fn is_unavailable(&self) -> bool {
        matches!(self, SearchError::Unavailable { .. })
    }
}

/// The error response of a [`SearchError`]; `None` keeps the 500.
pub(crate) fn map_search_error(error: &SearchError) -> Option<AppError> {
    match error {
        SearchError::NotFilterable { .. }
        | SearchError::NotSortable { .. }
        | SearchError::Unsupported { .. } => Some(AppError::bad_request(error.to_string())),
        SearchError::Unavailable { .. } => Some(
            AppError::service_unavailable("Search is unavailable. Try again shortly.")
                .with_source(error.to_string()),
        ),
        SearchError::Db(db) => navree_db::map_db_error(db),
        SearchError::Orm(OrmError::Db(db)) => navree_db::map_db_error(db),
        SearchError::Orm(OrmError::ModelNotFound { .. }) => Some(AppError::not_found()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_map_to_statuses() {
        let cases = [
            (
                SearchError::NotFilterable {
                    index: "deals".into(),
                    field: "title".into(),
                },
                400,
            ),
            (
                SearchError::NotSortable {
                    index: "deals".into(),
                    field: "title".into(),
                },
                400,
            ),
            (
                SearchError::Unavailable {
                    engine: "meilisearch",
                    message: "connection refused".into(),
                },
                503,
            ),
            (SearchError::Other("boom".into()), 500),
            (SearchError::Db(DbError::not_found("deals 1")), 404),
        ];
        for (error, status) in cases {
            assert_eq!(error.to_app_error().status.as_u16(), status, "{error}");
        }
        let body = SearchError::Unavailable {
            engine: "meilisearch",
            message: "down".into(),
        }
        .to_app_error()
        .body();
        assert_eq!(body["message"], "Search is unavailable. Try again shortly.");
        assert_eq!(body["error"], "Service Unavailable");
        assert_eq!(body["statusCode"], 503);
    }
}
