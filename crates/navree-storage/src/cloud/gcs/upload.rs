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
        if session_url.origin() != upload_endpoint.origin()
            || session_url.path() != upload_endpoint.path()
            || !session_url.username().is_empty()
            || session_url.password().is_some()
            || session_url.fragment().is_some()
        {
            return Err(Error::new(
                ErrorKind::Answer,
                "the resumable upload session points outside the bucket upload endpoint",
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

        let committed_end = with_backoff(&self.retry, Replay::Safe, || async {
            let token = tokens.credential().await?;
            let request = Request::new(Method::PUT, session_url.clone())
                .secret("authorization", &format!("Bearer {}", token.secret))?
                .header("content-range", &content_range)?
                .with_body(part.clone());
            let answer = client.send(request).await?;
            if answer.status() == StatusCode::PERMANENT_REDIRECT {
                let next_offset = match answer.header("range") {
                    None => end + 1,
                    Some(range_header) => parse_308_committed_offset(range_header, start, end)?,
                };
                if next_offset <= start {
                    return Err(Error::busy(
                        "the resumable upload session made no forward progress",
                    ));
                }
                return Ok(next_offset);
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

        let committed_in_part = (committed_end - start) as usize;
        if committed_in_part < part.len() {
            let mut rest = BytesMut::from(&part[committed_in_part..]);
            rest.extend_from_slice(&self.buffer);
            self.buffer = rest;
        }
        self.offset = committed_end;
        Ok(())
    }
}

/// Parses GCS's `Range: bytes=0-<last>` header on a `308 Resume Incomplete`
/// response and returns the next uncommitted byte offset (`last + 1`).
fn parse_308_committed_offset(header: &str, start: u64, end: u64) -> Result<u64> {
    let invalid = || {
        Error::new(
            ErrorKind::Answer,
            "the resumable upload 308 Range header is invalid",
        )
    };
    let rest = header.trim().strip_prefix("bytes=").ok_or_else(invalid)?;
    let (first_str, last_str) = rest.split_once('-').ok_or_else(invalid)?;
    let first: u64 = first_str.trim().parse().map_err(|_| invalid())?;
    let last: u64 = last_str.trim().parse().map_err(|_| invalid())?;
    if first > start || last < first || last > end {
        return Err(invalid());
    }
    last.checked_add(1).ok_or_else(invalid)
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
