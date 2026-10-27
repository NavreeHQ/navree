//! The SCIM 2.0 server (RFC 7643 and RFC 7644): filters, PATCH, the
//! resources and their routes.

pub(crate) mod controller;
pub mod filter;
pub(crate) mod patch;
pub(crate) mod resources;
pub(crate) mod server;

use axum::response::{IntoResponse, Response};
use http::{HeaderValue, StatusCode, header};
use serde_json::{Value, json};

pub use filter::{AttrPath, CompareOp, Filter, FilterError, PatchPath};

/// The content type of SCIM responses.
pub const SCIM_CONTENT_TYPE: &str = "application/scim+json";

/// A SCIM error response (RFC 7644 §3.12): `{"schemas": [...Error],
/// "status": "400", "scimType": "invalidFilter", "detail": "..."}`, sent
/// as `application/scim+json`. SCIM clients read this shape, so the SCIM
/// routes answer with it instead of Navree's JSON error shape.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScimError {
    /// The HTTP status.
    pub status: u16,
    /// The SCIM error type (`invalidFilter`, `uniqueness`, ...), for 400
    /// and 409 answers.
    pub scim_type: Option<&'static str>,
    /// What went wrong, for people.
    pub detail: String,
}

impl ScimError {
    /// An error with a status, a SCIM type and a detail.
    pub fn new(status: u16, scim_type: Option<&'static str>, detail: impl Into<String>) -> Self {
        Self {
            status,
            scim_type,
            detail: detail.into(),
        }
    }

    pub(crate) fn invalid_syntax(detail: impl Into<String>) -> Self {
        Self::new(400, Some("invalidSyntax"), detail)
    }

    pub(crate) fn invalid_value(detail: impl Into<String>) -> Self {
        Self::new(400, Some("invalidValue"), detail)
    }

    pub(crate) fn invalid_filter(detail: impl Into<String>) -> Self {
        Self::new(400, Some("invalidFilter"), detail)
    }

    pub(crate) fn uniqueness(detail: impl Into<String>) -> Self {
        Self::new(409, Some("uniqueness"), detail)
    }

    pub(crate) fn not_found(detail: impl Into<String>) -> Self {
        Self::new(404, None, detail)
    }

    pub(crate) fn unauthorized() -> Self {
        Self::new(401, None, "A valid bearer token is required")
    }

    pub(crate) fn forbidden(detail: impl Into<String>) -> Self {
        Self::new(403, None, detail)
    }

    pub(crate) fn internal() -> Self {
        Self::new(500, None, "Internal server error")
    }

    /// The error's JSON body.
    pub fn body(&self) -> Value {
        let mut body = json!({
            "schemas": [resources::ERROR_SCHEMA],
            "status": self.status.to_string(),
            "detail": self.detail,
        });
        if let Some(scim_type) = self.scim_type {
            body["scimType"] = json!(scim_type);
        }
        body
    }
}

impl std::fmt::Display for ScimError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.status, self.detail)
    }
}

impl std::error::Error for ScimError {}

impl From<FilterError> for ScimError {
    fn from(error: FilterError) -> Self {
        ScimError::invalid_filter(error.0)
    }
}

impl From<patch::PatchError> for ScimError {
    fn from(error: patch::PatchError) -> Self {
        ScimError::new(400, Some(error.scim_type), error.detail)
    }
}

impl IntoResponse for ScimError {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let mut response = (status, scim_body(&self.body())).into_response();
        if status == StatusCode::UNAUTHORIZED {
            response.headers_mut().insert(
                header::WWW_AUTHENTICATE,
                HeaderValue::from_static("Bearer realm=\"SCIM\""),
            );
        }
        response
    }
}

/// A JSON body sent as `application/scim+json`.
pub(crate) fn scim_body(value: &Value) -> impl IntoResponse + use<> {
    (
        [(header::CONTENT_TYPE, HeaderValue::from_static(SCIM_CONTENT_TYPE))],
        serde_json::to_vec(value).unwrap_or_default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_have_the_rfc_shape() {
        let error = ScimError::invalid_filter("`is` is not a filter operator");
        assert_eq!(
            error.body(),
            json!({
                "schemas": ["urn:ietf:params:scim:api:messages:2.0:Error"],
                "status": "400",
                "scimType": "invalidFilter",
                "detail": "`is` is not a filter operator"
            })
        );
        assert!(ScimError::not_found("User not found").body().get("scimType").is_none());
        let response = ScimError::unauthorized().into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            HeaderValue::from_static(SCIM_CONTENT_TYPE)
        );
        assert!(response.headers().contains_key(header::WWW_AUTHENTICATE));
        let from_filter: ScimError = FilterError("bad".into()).into();
        assert_eq!(from_filter.scim_type, Some("invalidFilter"));
    }
}
