//! Rendering errors: every GraphQL error gets the rest of Navree's JSON
//! error shape in its `extensions` (`code`, `statusCode`, `error` and, for
//! validation failures, `errors`), with the status the same error has on a
//! route.

use std::collections::BTreeMap;
use std::error::Error as StdError;

use async_graphql::{ErrorExtensionValues, PathSegment, Response, ServerError, Value};
use http::StatusCode;
use navree_http::{AppError, ErrorMapper};
use navree_kernel::BoxError;

/// The message of a server error: internals never reach the client.
const INTERNAL_MESSAGE: &str = "Internal server error";

/// What an error is on HTTP.
struct Classified {
    status: StatusCode,
    /// The message to show (`None` keeps the error's own).
    message: Option<String>,
    /// Field errors (validation).
    errors: Option<BTreeMap<String, Vec<String>>>,
    /// The real error, logged for server errors.
    log: Option<String>,
}

/// `extensions.code` for `status`: the reason phrase in upper snake case
/// (`NOT_FOUND`), and `UNAUTHENTICATED` for 401.
pub(crate) fn code(status: StatusCode) -> String {
    if status == StatusCode::UNAUTHORIZED {
        return "UNAUTHENTICATED".to_owned();
    }
    status
        .canonical_reason()
        .unwrap_or("Error")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect()
}

/// Adds Navree's extensions to every error of `response`, and logs server
/// errors with `request_id`.
pub(crate) fn render_response(response: &mut Response, mappers: &[ErrorMapper], request_id: &str) {
    for error in &mut response.errors {
        render(error, mappers, request_id);
    }
}

/// Adds Navree's extensions to `error` (keeping any the resolver set) and
/// logs it when it is a server error.
pub(crate) fn render(error: &mut ServerError, mappers: &[ErrorMapper], request_id: &str) {
    let classified = classify(error, mappers);
    if classified.status.is_server_error() {
        tracing::error!(
            target: "navree::error",
            request_id,
            path = %path_text(&error.path),
            error = %classified.log.as_deref().unwrap_or(&error.message),
            "GraphQL field failed"
        );
    }
    if let Some(message) = classified.message {
        error.message = message;
    }
    let extensions = error
        .extensions
        .get_or_insert_with(ErrorExtensionValues::default);
    let status = classified.status;
    if extensions.get("code").is_none() {
        extensions.set("code", code(status));
    }
    if extensions.get("statusCode").is_none() {
        extensions.set("statusCode", i32::from(status.as_u16()));
    }
    if extensions.get("error").is_none() {
        extensions.set("error", status.canonical_reason().unwrap_or("Error"));
    }
    if let Some(errors) = classified.errors {
        let value = serde_json::to_value(errors).unwrap_or_default();
        extensions.set("errors", Value::from_json(value).unwrap_or(Value::Null));
    }
}

/// A 400 GraphQL error (a request that does not parse or is not allowed).
pub(crate) fn request_error(message: impl Into<String>) -> ServerError {
    let mut error = ServerError::new(message, None);
    let extensions = error
        .extensions
        .get_or_insert_with(ErrorExtensionValues::default);
    extensions.set("code", code(StatusCode::BAD_REQUEST));
    extensions.set("statusCode", 400);
    extensions.set("error", "Bad Request");
    error
}

fn path_text(path: &[PathSegment]) -> String {
    path.iter()
        .map(|segment| match segment {
            PathSegment::Field(name) => name.clone(),
            PathSegment::Index(index) => index.to_string(),
        })
        .collect::<Vec<_>>()
        .join(".")
}

fn classify(error: &ServerError, mappers: &[ErrorMapper]) -> Classified {
    let Some(source) = error.source.as_deref() else {
        // async-graphql's own errors (parsing, validation, limits, argument
        // types) and messages built with `Error::new`.
        return client(StatusCode::BAD_REQUEST, None);
    };
    if let Some(app) = source.downcast_ref::<AppError>() {
        return from_app_error(app, mappers);
    }
    if let Some(db) = source.downcast_ref::<navree_db::DbError>() {
        return match navree_db::map_db_error(db).or_else(|| ErrorMapper::apply_any(mappers, source))
        {
            Some(mapped) => from_app_error(&mapped, mappers),
            None => internal(db.to_string()),
        };
    }
    if let Some(boxed) = source.downcast_ref::<BoxError>() {
        let error: &(dyn StdError + 'static) = &**boxed;
        return match ErrorMapper::apply_to(mappers, error) {
            Some(mapped) => from_app_error(&mapped, mappers),
            None => internal(chain(error)),
        };
    }
    if source.is::<String>() || source.is::<&'static str>() {
        // `Err(format!(..))?`: a message written for the client.
        return client(StatusCode::BAD_REQUEST, None);
    }
    match ErrorMapper::apply_any(mappers, source) {
        Some(mapped) => from_app_error(&mapped, mappers),
        None => internal(error.message.clone()),
    }
}

fn from_app_error(error: &AppError, mappers: &[ErrorMapper]) -> Classified {
    if error.status == StatusCode::INTERNAL_SERVER_ERROR
        && error.source.is_some()
        && let Some(mapped) = ErrorMapper::apply(mappers, error)
        && mapped.status != StatusCode::INTERNAL_SERVER_ERROR
    {
        return from_app_error(&mapped, mappers);
    }
    let errors = error.errors.as_deref().filter(|errors| !errors.is_empty());
    let message = match errors {
        Some(errors) => errors
            .values()
            .flatten()
            .cloned()
            .collect::<Vec<_>>()
            .join("; "),
        None if error.status == StatusCode::INTERNAL_SERVER_ERROR => INTERNAL_MESSAGE.to_owned(),
        None => error.detail.clone().unwrap_or_else(|| error.title.clone()),
    };
    let log = error.status.is_server_error().then(|| {
        let mut text = error.to_string();
        if let Some(source) = error.source.as_deref() {
            text.push_str(": ");
            text.push_str(&chain(source));
        }
        text
    });
    Classified {
        status: error.status,
        message: Some(message),
        errors: errors.cloned(),
        log,
    }
}

fn client(status: StatusCode, message: Option<String>) -> Classified {
    Classified {
        status,
        message,
        errors: None,
        log: None,
    }
}

fn internal(log: String) -> Classified {
    Classified {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: Some(INTERNAL_MESSAGE.to_owned()),
        errors: None,
        log: Some(log),
    }
}

/// `error: cause: cause`, for logs.
fn chain(error: &(dyn StdError + 'static)) -> String {
    let mut out = error.to_string();
    let mut next = error.source();
    while let Some(cause) = next {
        out.push_str(": ");
        out.push_str(&cause.to_string());
        next = cause.source();
    }
    out
}

/// Whether `response` failed as a whole before running: no data and only
/// errors without a path (it did not parse, broke the schema or went over a
/// limit). Such a response is a 400 on HTTP.
pub(crate) fn is_request_error(response: &Response) -> bool {
    response.data == Value::Null
        && !response.errors.is_empty()
        && response.errors.iter().all(|error| error.path.is_empty())
}

#[cfg(test)]
mod tests {
    use async_graphql::ErrorExtensions;

    use super::*;

    #[test]
    fn codes_are_upper_snake_reason_phrases() {
        assert_eq!(code(StatusCode::BAD_REQUEST), "BAD_REQUEST");
        assert_eq!(code(StatusCode::UNAUTHORIZED), "UNAUTHENTICATED");
        assert_eq!(code(StatusCode::FORBIDDEN), "FORBIDDEN");
        assert_eq!(code(StatusCode::NOT_FOUND), "NOT_FOUND");
        assert_eq!(code(StatusCode::CONFLICT), "CONFLICT");
        assert_eq!(code(StatusCode::TOO_MANY_REQUESTS), "TOO_MANY_REQUESTS");
        assert_eq!(
            code(StatusCode::INTERNAL_SERVER_ERROR),
            "INTERNAL_SERVER_ERROR"
        );
    }

    fn rendered(error: async_graphql::Error) -> ServerError {
        let mut error = error.into_server_error(async_graphql::Pos::default());
        render(&mut error, &[], "test");
        error
    }

    fn extension(error: &ServerError, key: &str) -> Value {
        error
            .extensions
            .as_ref()
            .and_then(|e| e.get(key))
            .cloned()
            .unwrap_or(Value::Null)
    }

    #[test]
    fn app_errors_keep_their_status_and_message() {
        let error = rendered(AppError::not_found().with_detail("Deal not found").into());
        assert_eq!(error.message, "Deal not found");
        assert_eq!(extension(&error, "code"), Value::from("NOT_FOUND"));
        assert_eq!(extension(&error, "statusCode"), Value::from(404));
        assert_eq!(extension(&error, "error"), Value::from("Not Found"));
    }

    #[test]
    fn server_errors_hide_their_message() {
        let error = rendered(std::io::Error::other("disk on fire").into());
        assert_eq!(error.message, "Internal server error");
        assert_eq!(extension(&error, "statusCode"), Value::from(500));
        let error = rendered(AppError::internal("secret detail").into());
        assert_eq!(error.message, "Internal server error");
    }

    #[test]
    fn database_errors_map_like_on_routes() {
        let error = rendered(navree_db::DbError::not_found("deal").into());
        assert_eq!(extension(&error, "code"), Value::from("NOT_FOUND"));
        assert_eq!(extension(&error, "statusCode"), Value::from(404));
    }

    #[test]
    fn plain_messages_are_bad_requests() {
        let error = rendered(async_graphql::Error::new("Stage is closed"));
        assert_eq!(error.message, "Stage is closed");
        assert_eq!(extension(&error, "code"), Value::from("BAD_REQUEST"));
        let error = rendered(String::from("Pick a stage").into());
        assert_eq!(error.message, "Pick a stage");
        assert_eq!(extension(&error, "statusCode"), Value::from(400));
    }

    #[test]
    fn validation_errors_carry_field_errors() {
        let mut errors = BTreeMap::new();
        errors.insert(
            "input.position".to_owned(),
            vec!["position must not be less than 0".to_owned()],
        );
        let error = rendered(AppError::validation(errors).into());
        assert_eq!(error.message, "position must not be less than 0");
        assert_eq!(extension(&error, "code"), Value::from("BAD_REQUEST"));
        assert_eq!(
            extension(&error, "errors").into_json().unwrap(),
            serde_json::json!({ "input.position": ["position must not be less than 0"] })
        );
    }

    #[test]
    fn a_code_the_resolver_set_is_kept() {
        let error =
            async_graphql::Error::new("Slow down").extend_with(|_, e| e.set("code", "RATE"));
        let error = rendered(error);
        assert_eq!(extension(&error, "code"), Value::from("RATE"));
        assert_eq!(extension(&error, "statusCode"), Value::from(400));
    }
}
