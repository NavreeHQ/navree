//! The app's own signed routes for local and memory disks: downloads
//! (`GET /storage/{disk}/{*path}`) and presigned uploads
//! (`PUT /storage/{disk}/{*path}`).

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Path, RawQuery};
use axum::http::header::{
    CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE, HeaderValue,
    X_CONTENT_TYPE_OPTIONS,
};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use chrono::Utc;
use futures_util::{StreamExt, stream};
use navree_contracts::Clock;
use navree_http::{ApiMeta, AppError, Controller, RouteDef};
use navree_kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use percent_encoding::percent_decode_str;

use crate::path::normalize;
use crate::signing::{Refusal, UploadGrant};
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
        let downloads = self.clone();
        vec![
            RouteDef::on(
                Method::GET,
                "/{disk}/{*path}",
                move |Path((disk, path)): Path<(String, String)>, RawQuery(query): RawQuery| {
                    let controller = downloads.clone();
                    async move { controller.serve(&disk, &path, query.as_deref()).await }
                },
            )
            // The signature is the authorization.
            .public()
            .name("storage.temporary_url")
            .api(ApiMeta {
                summary: Some("Download a file from a temporary URL".to_owned()),
                description: Some(
                    "Serves the file a signed temporary URL points at; the `expires` and `signature` query parameters are the authorization."
                        .to_owned(),
                ),
                tags: vec!["Storage".to_owned()],
                responses: vec![403, 404],
                ..ApiMeta::default()
            }),
            RouteDef::on(
                Method::PUT,
                "/{disk}/{*path}",
                move |Path((disk, path)): Path<(String, String)>,
                      RawQuery(query): RawQuery,
                      headers: HeaderMap,
                      body: Body| {
                    let controller = self.clone();
                    async move {
                        controller
                            .receive(&disk, &path, query.as_deref(), &headers, body)
                            .await
                    }
                },
            )
            // The signature is the authorization, and it carries the size
            // limit, which the handler applies while streaming.
            .public()
            .body_limit(usize::MAX)
            .timeout(UPLOAD_TIMEOUT)
            .name("storage.presigned_upload")
            .api(ApiMeta {
                summary: Some("Upload a file to a presigned URL".to_owned()),
                description: Some(
                    "Stores the request body at the path a signed presigned upload URL points at; the query parameters are the authorization and carry the size limit and content type."
                        .to_owned(),
                ),
                tags: vec!["Storage".to_owned()],
                responses: vec![400, 403, 413],
                ..ApiMeta::default()
            }),
        ]
    }
}

/// How long a presigned upload to a local or memory disk may take.
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(60 * 60);

/// The query of a presigned upload link.
#[derive(Debug, Default, PartialEq, Eq)]
struct UploadParams {
    expires: i64,
    content_type: Option<String>,
    max_bytes: u64,
    size: Option<u64>,
    signature: String,
}

/// `expires`, `max` and `signature` (each exactly once), and `type` and
/// `size` (at most once each); `None` otherwise.
fn upload_params(query: Option<&str>) -> Option<UploadParams> {
    let mut expires = None;
    let mut content_type = None;
    let mut max_bytes = None;
    let mut size = None;
    let mut signature = None;
    for pair in query?.split('&') {
        let Some((name, value)) = pair.split_once('=') else {
            continue;
        };
        let value = percent_decode_str(value).decode_utf8().ok()?.into_owned();
        let duplicate = match name {
            "expires" => expires.replace(value.parse::<i64>().ok()?).is_some(),
            "type" => content_type.replace(value).is_some(),
            "max" => max_bytes.replace(value.parse::<u64>().ok()?).is_some(),
            "size" => size.replace(value.parse::<u64>().ok()?).is_some(),
            "signature" => signature.replace(value).is_some(),
            _ => false,
        };
        if duplicate {
            return None;
        }
    }
    Some(UploadParams {
        expires: expires?,
        content_type,
        max_bytes: max_bytes?,
        size,
        signature: signature?,
    })
}

/// Why the body of a presigned upload was refused.
#[derive(Debug)]
enum BodyError {
    /// Over the signed size limit.
    TooLarge(u64),
    /// Not the declared size.
    Size(u64),
    /// The client's body failed.
    Body(axum::Error),
}

impl std::fmt::Display for BodyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge(max) => write!(f, "the upload is larger than {max} bytes"),
            Self::Size(size) => write!(f, "the upload is not {size} bytes long"),
            Self::Body(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for BodyError {}

/// The chunks of `body`, failing once more than `max_bytes`, or other than
/// `size` bytes, arrive.
fn checked_body(
    body: Body,
    max_bytes: u64,
    size: Option<u64>,
) -> impl futures_util::Stream<Item = Result<bytes::Bytes, BodyError>> {
    let limit = size.map_or(max_bytes, |size| size.min(max_bytes));
    stream::try_unfold(
        (body.into_data_stream(), 0u64),
        move |(mut chunks, total)| async move {
            match chunks.next().await {
                Some(Ok(chunk)) => {
                    let total = total + chunk.len() as u64;
                    if total > limit {
                        return Err(match size {
                            Some(size) if size <= max_bytes => BodyError::Size(size),
                            _ => BodyError::TooLarge(max_bytes),
                        });
                    }
                    Ok(Some((chunk, (chunks, total))))
                }
                Some(Err(error)) => Err(BodyError::Body(error)),
                None => match size {
                    Some(size) if size != total => Err(BodyError::Size(size)),
                    _ => Ok(None),
                },
            }
        },
    )
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
        disk.signer()
            .verify(disk.name(), &path, expires, signature, self.now())
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

impl StorageController {
    /// Stores the body of a presigned upload (see
    /// [`Disk::presigned_put`](crate::Disk::presigned_put)).
    async fn receive(
        &self,
        disk: &str,
        path: &str,
        query: Option<&str>,
        headers: &HeaderMap,
        body: Body,
    ) -> Result<Response, AppError> {
        let disk = self.storage.disk(disk).map_err(|_| invalid_signature())?;
        let path = normalize(path, false).map_err(|_| invalid_signature())?;
        let params = upload_params(query).ok_or_else(invalid_signature)?;
        let grant = UploadGrant {
            disk: disk.name(),
            path: &path,
            expires: params.expires,
            content_type: params.content_type.as_deref(),
            max_bytes: params.max_bytes,
            size: params.size,
        };
        disk.signer()
            .verify_upload(&grant, &params.signature, self.now())
            .map_err(|refusal| match refusal {
                Refusal::Invalid => invalid_signature(),
                Refusal::Expired => invalid_signature().with_detail("The link has expired."),
            })?;
        if let Some(expected) = grant.content_type {
            let sent = headers
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .map(str::trim);
            if sent != Some(expected) {
                return Err(invalid_signature().with_detail(format!(
                    "The upload must be sent with `Content-Type: {expected}`."
                )));
            }
        }
        let declared = headers
            .get(CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok());
        if let Some(length) = declared {
            if length > grant.max_bytes {
                return Err(too_large(grant.max_bytes));
            }
            if grant.size.is_some_and(|size| size != length) {
                return Err(wrong_size(grant.size.unwrap_or_default()));
            }
        }

        let chunks = checked_body(body, grant.max_bytes, grant.size);
        match disk.put_stream(&path, chunks).await {
            Ok(_) => Ok(StatusCode::OK.into_response()),
            Err(StorageError::Stream { source }) => match source.downcast::<BodyError>() {
                Ok(error) => Err(match *error {
                    BodyError::TooLarge(max) => too_large(max),
                    BodyError::Size(size) => wrong_size(size),
                    BodyError::Body(error) => {
                        AppError::bad_request("The upload body could not be read.")
                            .with_source(error)
                    }
                }),
                Err(source) => Err(AppError::internal(source)),
            },
            Err(error) => Err(AppError::internal(error)),
        }
    }

    /// The app's clock, or the system's.
    fn now(&self) -> chrono::DateTime<Utc> {
        match &self.clock {
            Some(clock) => clock.now(),
            None => Utc::now(),
        }
    }
}

/// 413: over the signed size limit.
fn too_large(max_bytes: u64) -> AppError {
    AppError::from_status(StatusCode::PAYLOAD_TOO_LARGE).with_detail(format!(
        "The file must not be larger than {max_bytes} bytes."
    ))
}

/// 400: not the declared size.
fn wrong_size(size: u64) -> AppError {
    AppError::bad_request(format!("The upload must be exactly {size} bytes long."))
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

    #[test]
    fn upload_params_decode_and_refuse_duplicates() {
        assert_eq!(
            upload_params(Some(
                "expires=10&type=image%2Fpng&max=100&size=5&signature=ab"
            )),
            Some(UploadParams {
                expires: 10,
                content_type: Some("image/png".into()),
                max_bytes: 100,
                size: Some(5),
                signature: "ab".into(),
            })
        );
        assert_eq!(
            upload_params(Some("expires=10&max=100&signature=ab&x=1")),
            Some(UploadParams {
                expires: 10,
                max_bytes: 100,
                signature: "ab".into(),
                ..UploadParams::default()
            })
        );
        for bad in [
            "expires=10&signature=ab",
            "max=1&signature=ab",
            "expires=10&max=1",
            "expires=10&max=-1&signature=ab",
            "expires=10&max=1&size=big&signature=ab",
            "expires=10&max=1&type=a&type=b&signature=ab",
            "expires=10&max=1&max=2&signature=ab",
            "expires=10&max=1&type=%FF&signature=ab",
        ] {
            assert_eq!(upload_params(Some(bad)), None, "{bad}");
        }
        assert_eq!(upload_params(None), None);
    }
}
