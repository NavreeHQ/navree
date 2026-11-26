//! Resumable multipart uploads on Google Cloud Storage's JSON API.

use std::sync::Arc;

use ::http::{Method, StatusCode};
use bytes::{Bytes, BytesMut};
use navree_kernel::BoxFuture;
use url::Url;

use super::OBJECT_JSON_LIMIT;
use super::token::attach_error_code;
use crate::cloud::credentials::Provider;
use crate::cloud::http::{Client, Request};
use crate::cloud::keys::encode_component;
use crate::cloud::retry::{Replay, RetryPolicy, with_backoff};
use crate::cloud::{Error, ErrorKind, Result};
use crate::store::{StoreError, StoreResult, Upload};

/// Google Cloud Storage's required granularity for every non-final resumable
/// upload chunk (256 KiB).
pub const UPLOAD_QUANTUM: usize = 256 * 1024;

/// Default resumable upload chunk size (8 MiB = 32 * 256 KiB).
pub const DEFAULT_CHUNK_SIZE: usize = 8 * 1024 * 1024;

/// An active resumable upload session (`uploadType=resumable`).
///
/// Chunks are buffered until strictly more than `chunk_size` bytes are queued,
/// so every non-empty upload finishes with a non-empty final `PUT` carrying
/// `Content-Range: bytes <start>-<end>/<total>`, and every part fetches its
/// bearer token right before sending so an expiring credential refreshes
/// seamlessly mid-upload.
pub(super) struct GcsUpload {
    client: Client,
    tokens: Arc<Provider<String>>,
    retry: RetryPolicy,
    session_url: Url,
    chunk_size: usize,
    offset: u64,
    buffer: BytesMut,
}

impl GcsUpload {
    /// Starts a resumable upload session at `upload_endpoint` for `key` and
    /// captures its `Location` session URI (which must stay on `base_url`'s
    /// origin).
    pub(super) async fn start(
        client: Client,
        tokens: Arc<Provider<String>>,
        retry: RetryPolicy,
        base_url: &Url,
        upload_endpoint: &Url,
        key: &str,
        chunk_size: usize,
    ) -> Result<Self> {
        let initiate_url = Url::parse(&format!(
            "{}?uploadType=resumable&name={}",
            upload_endpoint.as_str(),
            encode_component(key),
        ))
        .map_err(|_| Error::new(ErrorKind::Other, "could not build the upload URL"))?;

        let answer = with_backoff(&retry, Replay::Safe, || async {
            let token = tokens.credential().await?;
            let request = Request::new(Method::POST, initiate_url.clone())
                .secret("authorization", &format!("Bearer {}", token.secret))?
                .header("x-upload-content-type", "application/octet-stream")?;
            client
                .send(request)
                .await?
                .success()
                .await
                .map_err(attach_error_code)
        })
        .await?;

        let location = answer.header("location").ok_or_else(|| {
            Error::new(
                ErrorKind::Answer,
                "the resumable upload answer had no Location header",
            )
        })?;
        let session_url = base_url.join(location).map_err(|_| {
            Error::new(
                ErrorKind::Answer,
                "the resumable upload Location is not a valid URL",
            )
        })?;
        if session_url.origin() != base_url.origin() {
            return Err(Error::new(
                ErrorKind::Answer,
                "the resumable upload session points to another origin",
            ));
        }

        Ok(Self {
            client,
            tokens,
            retry,
            session_url,
            chunk_size,
            offset: 0,
            buffer: BytesMut::new(),
        })
    }

    async fn flush_intermediate(&mut self, part: Bytes) -> Result<()> {
        let len = part.len() as u64;
        let start = self.offset;
        let end = start + len - 1;
        let content_range = format!("bytes {start}-{end}/*");
        let (client, tokens, session_url) = (&self.client, &self.tokens, &self.session_url);

        with_backoff(&self.retry, Replay::Safe, || async {
            let token = tokens.credential().await?;
            let request = Request::new(Method::PUT, session_url.clone())
                .secret("authorization", &format!("Bearer {}", token.secret))?
                .header("content-range", &content_range)?
                .with_body(part.clone());
            let answer = client.send(request).await?;
            if answer.status() == StatusCode::PERMANENT_REDIRECT {
                return Ok(());
            }
            let answer = answer.success().await.map_err(attach_error_code)?;
            Err(Error::new(
                ErrorKind::Answer,
                format!(
                    "the store answered {} before the resumable upload finished",
                    answer.status()
                ),
            ))
        })
        .await?;

        self.offset += len;
        Ok(())
    }
}

impl Upload for GcsUpload {
    fn write<'a>(&'a mut self, chunk: Bytes) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move {
            if chunk.is_empty() {
                return Ok(());
            }
            self.buffer.extend_from_slice(&chunk);
            while self.buffer.len() > self.chunk_size {
                let part = self.buffer.split_to(self.chunk_size).freeze();
                self.flush_intermediate(part)
                    .await
                    .map_err(StoreError::from)?;
            }
            Ok(())
        })
    }

    fn finish(self: Box<Self>) -> BoxFuture<'static, StoreResult<()>> {
        Box::pin(async move {
            let Self {
                client,
                tokens,
                retry,
                session_url,
                offset,
                buffer,
                ..
            } = *self;
            let rest = buffer.freeze();
            let total = offset + rest.len() as u64;
            let content_range = if rest.is_empty() {
                format!("bytes */{total}")
            } else {
                let end = total - 1;
                format!("bytes {offset}-{end}/{total}")
            };

            let answer = with_backoff(&retry, Replay::Safe, || async {
                let token = tokens.credential().await?;
                let request = Request::new(Method::PUT, session_url.clone())
                    .secret("authorization", &format!("Bearer {}", token.secret))?
                    .header("content-range", &content_range)?
                    .with_body(rest.clone());
                client
                    .send(request)
                    .await?
                    .success()
                    .await
                    .map_err(attach_error_code)
            })
            .await
            .map_err(StoreError::from)?;

            let _ = answer
                .bytes_up_to(OBJECT_JSON_LIMIT)
                .await
                .map_err(StoreError::from)?;
            Ok(())
        })
    }

    fn abort(self: Box<Self>) -> BoxFuture<'static, StoreResult<()>> {
        Box::pin(async move {
            let Self {
                client,
                tokens,
                retry,
                session_url,
                ..
            } = *self;

            with_backoff(&retry, Replay::Safe, || async {
                let token = tokens.credential().await?;
                let request = Request::new(Method::DELETE, session_url.clone())
                    .secret("authorization", &format!("Bearer {}", token.secret))?
                    .header("content-length", "0")?;
                let answer = client.send(request).await?;
                let status = answer.status();
                // Google Cloud Storage answers a cancelled resumable session
                // with `499 Client Closed Request` (emulators may answer 2xx
                // or 404).
                if status.is_success() || status.as_u16() == 499 || status == StatusCode::NOT_FOUND
                {
                    return Ok(());
                }
                answer.success().await.map(drop).map_err(attach_error_code)
            })
            .await
            .map_err(StoreError::from)
        })
    }
}
