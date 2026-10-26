//! The two directions between [`AppError`] and gRPC's [`Status`].

use http::StatusCode;
use navree_http::AppError;
use tonic::{Code, Status};
use tonic_types::{ErrorDetails, FieldViolation, StatusExt};

/// The gRPC status meaning the same as `error`, with the error's message.
///
/// | `AppError` status | gRPC code |
/// |---|---|
/// | 400 | `INVALID_ARGUMENT`, field errors in a `google.rpc.BadRequest` detail |
/// | 401 | `UNAUTHENTICATED` |
/// | 403 | `PERMISSION_DENIED` |
/// | 404 | `NOT_FOUND` |
/// | 409 | `ALREADY_EXISTS` |
/// | 412 | `FAILED_PRECONDITION` |
/// | 413, 429 | `RESOURCE_EXHAUSTED` |
/// | 501 | `UNIMPLEMENTED` |
/// | 503 | `UNAVAILABLE` |
/// | 504 | `DEADLINE_EXCEEDED` |
/// | anything else | `INTERNAL` |
///
/// A 500 keeps the message `"Internal server error"`: its source never
/// reaches the client. A 400 with field errors lists every message,
/// separated by `; `.
pub fn status_from_error(error: &AppError) -> Status {
    let code = match error.status.as_u16() {
        400 => Code::InvalidArgument,
        401 => Code::Unauthenticated,
        403 => Code::PermissionDenied,
        404 => Code::NotFound,
        409 => Code::AlreadyExists,
        412 => Code::FailedPrecondition,
        413 | 429 => Code::ResourceExhausted,
        501 => Code::Unimplemented,
        503 => Code::Unavailable,
        504 => Code::DeadlineExceeded,
        _ => Code::Internal,
    };
    let fields = error.errors.as_deref().filter(|errors| !errors.is_empty());
    let message = match fields {
        Some(errors) => errors
            .values()
            .flatten()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("; "),
        None => error.detail.clone().unwrap_or_else(|| error.title.clone()),
    };
    match fields {
        Some(errors) if code == Code::InvalidArgument => {
            let violations: Vec<FieldViolation> = errors
                .iter()
                .flat_map(|(field, messages)| {
                    messages
                        .iter()
                        .map(move |message| FieldViolation::new(field.clone(), message.clone()))
                })
                .collect();
            Status::with_error_details(code, message, ErrorDetails::with_bad_request(violations))
        }
        _ => Status::new(code, message),
    }
}

/// The [`AppError`] meaning the same as `status`, what `?` on a [`Status`]
/// gives in an HTTP handler (through the error mapper `GrpcModule`
/// registers): `NOT_FOUND` from another service is a 404 here.
///
/// Client errors and `UNIMPLEMENTED`, `UNAVAILABLE` and `DEADLINE_EXCEEDED`
/// keep the status message; every other code is a 500 whose message stays
/// `"Internal server error"` (the status is kept as its source, for logs).
pub fn error_from_status(status: &Status) -> AppError {
    let http = match status.code() {
        Code::Ok => StatusCode::OK,
        Code::InvalidArgument | Code::OutOfRange => StatusCode::BAD_REQUEST,
        Code::Unauthenticated => StatusCode::UNAUTHORIZED,
        Code::PermissionDenied => StatusCode::FORBIDDEN,
        Code::NotFound => StatusCode::NOT_FOUND,
        Code::AlreadyExists | Code::Aborted => StatusCode::CONFLICT,
        Code::FailedPrecondition => StatusCode::PRECONDITION_FAILED,
        Code::ResourceExhausted => StatusCode::TOO_MANY_REQUESTS,
        Code::Unimplemented => StatusCode::NOT_IMPLEMENTED,
        Code::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        Code::DeadlineExceeded => StatusCode::GATEWAY_TIMEOUT,
        Code::Cancelled | Code::Unknown | Code::Internal | Code::DataLoss => {
            StatusCode::INTERNAL_SERVER_ERROR
        }
    };
    let error = AppError::from_status(http);
    let keeps_message = http.is_client_error()
        || matches!(
            status.code(),
            Code::Unimplemented | Code::Unavailable | Code::DeadlineExceeded
        );
    if http == StatusCode::BAD_REQUEST
        && let Ok(details) = status.check_error_details()
        && let Some(bad_request) = details.bad_request()
        && !bad_request.field_violations.is_empty()
    {
        let mut fields = std::collections::BTreeMap::<String, Vec<String>>::new();
        for violation in &bad_request.field_violations {
            fields
                .entry(violation.field.clone())
                .or_default()
                .push(violation.description.clone());
        }
        return AppError::validation(fields);
    }
    if keeps_message && !status.message().is_empty() {
        error.with_detail(status.message())
    } else if keeps_message {
        error
    } else {
        error.with_source(status.clone())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn app_errors_become_the_status_of_the_same_meaning() {
        let cases = [
            (
                AppError::bad_request("bad id"),
                Code::InvalidArgument,
                "bad id",
            ),
            (
                AppError::unauthenticated(),
                Code::Unauthenticated,
                "Unauthorized",
            ),
            (AppError::forbidden(), Code::PermissionDenied, "Forbidden"),
            (AppError::not_found(), Code::NotFound, "Not Found"),
            (AppError::conflict("taken"), Code::AlreadyExists, "taken"),
            (
                AppError::from_status(StatusCode::PRECONDITION_FAILED),
                Code::FailedPrecondition,
                "Precondition Failed",
            ),
            (
                AppError::too_many_requests(std::time::Duration::from_secs(1)),
                Code::ResourceExhausted,
                "Too Many Requests",
            ),
            (
                AppError::from_status(StatusCode::NOT_IMPLEMENTED),
                Code::Unimplemented,
                "Not Implemented",
            ),
            (
                AppError::service_unavailable("db down"),
                Code::Unavailable,
                "db down",
            ),
            (
                AppError::from_status(StatusCode::GATEWAY_TIMEOUT),
                Code::DeadlineExceeded,
                "Gateway Timeout",
            ),
            (
                AppError::internal("secret connection string"),
                Code::Internal,
                "Internal server error",
            ),
            (
                AppError::from_status(StatusCode::IM_A_TEAPOT),
                Code::Internal,
                "I'm a teapot",
            ),
        ];
        for (error, code, message) in cases {
            let status = status_from_error(&error);
            assert_eq!(
                (status.code(), status.message()),
                (code, message),
                "{error}"
            );
        }
    }

    #[test]
    fn validation_errors_carry_a_bad_request_detail() {
        let mut fields = BTreeMap::new();
        fields.insert(
            "position".to_owned(),
            vec!["position must not be less than 0".to_owned()],
        );
        fields.insert(
            "stage".to_owned(),
            vec!["stage must be longer than or equal to 1 characters".to_owned()],
        );
        let status = status_from_error(&AppError::validation(fields));
        assert_eq!(status.code(), Code::InvalidArgument);
        assert_eq!(
            status.message(),
            "position must not be less than 0; stage must be longer than or equal to 1 characters"
        );
        let details = status.check_error_details().unwrap();
        let violations = &details.bad_request().unwrap().field_violations;
        assert_eq!(violations.len(), 2);
        assert_eq!(violations[0].field, "position");
        assert_eq!(
            violations[0].description,
            "position must not be less than 0"
        );

        // And back: an upstream INVALID_ARGUMENT with field violations is a
        // validation error with the same fields.
        let error = error_from_status(&status);
        assert_eq!(error.status, StatusCode::BAD_REQUEST);
        assert_eq!(
            error.errors.as_deref().unwrap()["position"],
            ["position must not be less than 0"]
        );
    }

    #[test]
    fn statuses_become_the_app_error_of_the_same_meaning() {
        let cases = [
            (Status::not_found("no deal"), 404, Some("no deal")),
            (Status::invalid_argument("bad"), 400, Some("bad")),
            (Status::unauthenticated("who"), 401, Some("who")),
            (Status::permission_denied("no"), 403, Some("no")),
            (Status::already_exists("dup"), 409, Some("dup")),
            (Status::failed_precondition("pre"), 412, Some("pre")),
            (Status::resource_exhausted("slow"), 429, Some("slow")),
            (Status::unimplemented("later"), 501, Some("later")),
            (Status::unavailable("down"), 503, Some("down")),
            (Status::deadline_exceeded("late"), 504, Some("late")),
            (Status::internal("stack trace"), 500, None),
            (Status::unknown("??"), 500, None),
        ];
        for (status, http, detail) in cases {
            let error = error_from_status(&status);
            assert_eq!(error.status.as_u16(), http, "{status:?}");
            assert_eq!(error.detail.as_deref(), detail, "{status:?}");
        }
        let internal = error_from_status(&Status::internal("stack trace"));
        assert_eq!(internal.body()["message"], "Internal server error");
        assert!(internal.source.is_some(), "the status is kept for the logs");
    }
}
