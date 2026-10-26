//! The signed download route: `GET /storage/{disk}/{*path}`.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, RawQuery};
use axum::http::header::{
    CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE, HeaderValue,
    X_CONTENT_TYPE_OPTIONS,
};
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono::Utc;
use navis_contracts::Clock;
use navis_http::{AppError, Controller, RouteDef};
use navis_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};

use crate::path::normalize;
use crate::signing::Refusal;
use crate::{Storage, StorageError};

/// Content types a browser would execute or render as a document; they are
/// served as attachments so a stored file can never run in the app's origin.
const ACTIVE_CONTENT: [&str; 7] = [
    "text/html",
    "application/xhtml+xml",
    "image/svg+xml",
    "text/xml",
    "application/xml",
    "text/javascript",
    "application/javascript",
];

/// Serves temporary URLs (see [`Disk::temporary_url`](crate::Disk::temporary_url)).
pub(crate) struct StorageController {
    storage: Arc<Storage>,
    clock: Option<Arc<dyn Clock>>,
}

impl Injectable for StorageController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Storage>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(Self {
                storage: r.get::<Storage>().await?,
                clock: r.app().try_get::<dyn Clock>().await,
            })
        })
    }
}

impl Controller for StorageController {
    fn prefix(&self) -> &'static str {
        "!/storage"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(
                Method::GET,
                "/{disk}/{*path}",
                move |Path((disk, path)): Path<(String, String)>, RawQuery(query): RawQuery| {
                    let controller = self.clone();
                    async move { controller.serve(&disk, &path, query.as_deref()).await }
                },
            )
            // The signature is the authorization.
            .public()
            .name("storage.temporary_url"),
        ]
    }
}

impl StorageController {
    async fn serve(
        &self,
        disk: &str,
        path: &str,
        query: Option<&str>,
    ) -> Result<Response, AppError> {
        // Nothing without a valid signature can be told apart: an unknown
        // disk or an invalid path is reported like a forged signature.
        let disk = self.storage.disk(disk).map_err(|_| invalid_signature())?;
        let path = normalize(path, false).map_err(|_| invalid_signature())?;
        let (expires, signature) = signature_params(query).ok_or_else(invalid_signature)?;
        let now = match &self.clock {
            Some(clock) => clock.now(),
            None => Utc::now(),
        };
        disk.signer()
            .verify(disk.name(), &path, expires, signature, now)
            .map_err(|refusal| match refusal {
                Refusal::Invalid => invalid_signature(),
                Refusal::Expired => invalid_signature().with_detail("The link has expired."),
            })?;

        let (size, stream) = disk.open(&path).await.map_err(|error| match error {
            StorageError::NotFound { .. } => AppError::not_found(),
            error => AppError::internal(error),
        })?;
        let content_type = mime_guess::from_path(&path).first_or_octet_stream();
        let mut response = Body::from_stream(stream).into_response();
        let headers = response.headers_mut();
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_str(content_type.essence_str())
                .unwrap_or(HeaderValue::from_static("application/octet-stream")),
        );
        headers.insert(CONTENT_LENGTH, HeaderValue::from(size));
        headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
        headers.insert(CACHE_CONTROL, HeaderValue::from_static("private"));
        if ACTIVE_CONTENT.contains(&content_type.essence_str()) {
            headers.insert(CONTENT_DISPOSITION, HeaderValue::from_static("attachment"));
        }
        Ok(response)
    }
}

/// `expires` and `signature` from the query; `None` unless each appears
/// exactly once (a second `signature` could smuggle a value past a proxy).
fn signature_params(query: Option<&str>) -> Option<(i64, &str)> {
    let mut expires = None;
    let mut signature = None;
    for pair in query?.split('&') {
        let duplicate = match pair.split_once('=') {
            Some(("expires", value)) => expires.replace(value.parse::<i64>().ok()?).is_some(),
            Some(("signature", value)) => signature.replace(value).is_some(),
            _ => false,
        };
        if duplicate {
            return None;
        }
    }
    Some((expires?, signature?))
}

/// 403: the link is forged, tampered with or expired.
fn invalid_signature() -> AppError {
    AppError::new(
        StatusCode::FORBIDDEN,
        "invalid_signature",
        "Invalid signature.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_params_need_exactly_one_of_each() {
        assert_eq!(
            signature_params(Some("expires=10&signature=ab")),
            Some((10, "ab"))
        );
        assert_eq!(
            signature_params(Some("x=1&signature=ab&expires=10")),
            Some((10, "ab"))
        );
        assert_eq!(signature_params(None), None);
        assert_eq!(signature_params(Some("expires=10")), None);
        assert_eq!(signature_params(Some("signature=ab")), None);
        assert_eq!(signature_params(Some("expires=soon&signature=ab")), None);
        assert_eq!(
            signature_params(Some("expires=10&signature=ab&signature=cd")),
            None
        );
        assert_eq!(
            signature_params(Some("expires=10&expires=11&signature=ab")),
            None
        );
    }
}
