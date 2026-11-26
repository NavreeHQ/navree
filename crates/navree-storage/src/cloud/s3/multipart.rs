//! S3 multipart uploads (`CreateMultipartUpload`, `UploadPart`,
//! `CompleteMultipartUpload`, `AbortMultipartUpload`).
//!
//! Every part attempt asks the store's credential [`Provider`] afresh inside
//! [`with_backoff`], so an upload that outlives a temporary STS, container
//! or IMDS session refreshes its token on the next part and finishes.

use ::http::{Method, StatusCode};
use bytes::{Bytes, BytesMut};
use navree_kernel::BoxFuture;
use url::Url;

use super::{S3Store, enrich_error, sigv4};
use crate::cloud::http::Request;
use crate::cloud::keys::{encode_component, object_url};
use crate::cloud::retry::{Replay, with_backoff};
use crate::cloud::{Error, ErrorKind, Result, xml};
use crate::store::{StoreError, StoreResult, Upload};

/// Default multipart part size (5 MiB, S3's minimum for every part before
/// the last).
pub const DEFAULT_PART_SIZE: usize = 5 * 1024 * 1024;

/// Maximum number of parts S3 allows in one multipart upload.
const MAX_PARTS: u32 = 10_000;

/// Maximum XML body size expected on multipart control responses.
const MULTIPART_XML_LIMIT: usize = 256 * 1024;

/// Starts a multipart upload for `path` on `store`.
pub(super) async fn begin(store: S3Store, path: &str) -> Result<Box<dyn Upload>> {
    let url = object_url(&store.bucket_url, path)?;
    let mut create_url = url.clone();
    create_url.set_query(Some("uploads"));

    let answer = with_backoff(&store.retry, Replay::Safe, || {
        let create_url = create_url.clone();
        let store = &store;
        async move {
            let credential = store.credentials.credential().await?;
            let mut request = Request::new(Method::POST, create_url);
            sigv4::sign_request(&mut request, &credential.secret, &store.region, store.now())?;
            store
                .client
                .send(request)
                .await?
                .success()
                .await
                .map_err(enrich_error)
        }
    })
    .await?;

    let body = answer.bytes_up_to(MULTIPART_XML_LIMIT).await?;
    let root = xml::parse(&body)?;
    let upload_id = root
        .value("UploadId")
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| {
            Error::new(
                ErrorKind::Answer,
                "the multipart upload answer has no UploadId",
            )
        })?
        .to_owned();

    let part_size = store.part_size.max(1);
    Ok(Box::new(S3Upload {
        store,
        object_url: url,
        upload_id,
        part_size,
        buffer: BytesMut::new(),
        parts: Vec::new(),
        failed: false,
    }))
}

struct S3Upload {
    store: S3Store,
    object_url: Url,
    upload_id: String,
    part_size: usize,
    buffer: BytesMut,
    parts: Vec<(u32, String)>,
    failed: bool,
}

impl S3Upload {
    async fn send_part(&mut self, data: Bytes) -> Result<()> {
        let part_number = u32::try_from(self.parts.len() + 1)
            .ok()
            .filter(|&n| n <= MAX_PARTS)
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::Refused,
                    "an S3 multipart upload has at most 10000 parts",
                )
            })?;

        let query = format!(
            "partNumber={part_number}&uploadId={}",
            encode_component(&self.upload_id)
        );
        let mut part_url = self.object_url.clone();
        part_url.set_query(Some(&query));

        let store = &self.store;
        let etag = with_backoff(&store.retry, Replay::Safe, || {
            let part_url = part_url.clone();
            let data = data.clone();
            async move {
                let credential = store.credentials.credential().await?;
                let mut request = Request::new(Method::PUT, part_url).with_body(data);
                sigv4::sign_request(&mut request, &credential.secret, &store.region, store.now())?;
                let answer = store
                    .client
                    .send(request)
                    .await?
                    .success()
                    .await
                    .map_err(enrich_error)?;
                let etag = answer
                    .header("etag")
                    .map(str::trim)
                    .filter(|tag| !tag.is_empty())
                    .ok_or_else(|| {
                        Error::new(
                            ErrorKind::Answer,
                            "an uploaded S3 part was answered without an ETag",
                        )
                    })?;
                Ok(etag.to_owned())
            }
        })
        .await?;

        self.parts.push((part_number, etag));
        Ok(())
    }

    async fn complete_remote(&self) -> Result<()> {
        let mut xml_body = String::from("<CompleteMultipartUpload>");
        for (number, etag) in &self.parts {
            xml_body.push_str("<Part><PartNumber>");
            xml_body.push_str(&number.to_string());
            xml_body.push_str("</PartNumber><ETag>");
            xml_body.push_str(&xml::escape(etag));
            xml_body.push_str("</ETag></Part>");
        }
        xml_body.push_str("</CompleteMultipartUpload>");
        let xml_bytes = Bytes::from(xml_body);

        let mut complete_url = self.object_url.clone();
        complete_url.set_query(Some(&format!(
            "uploadId={}",
            encode_component(&self.upload_id)
        )));

        let store = &self.store;
        with_backoff(&store.retry, Replay::Safe, || {
            let complete_url = complete_url.clone();
            let xml_bytes = xml_bytes.clone();
            async move {
                let credential = store.credentials.credential().await?;
                let mut request = Request::new(Method::POST, complete_url)
                    .header("content-type", "application/xml")?
                    .with_body(xml_bytes);
                sigv4::sign_request(&mut request, &credential.secret, &store.region, store.now())?;
                let answer = store
                    .client
                    .send(request)
                    .await?
                    .success()
                    .await
                    .map_err(enrich_error)?;
                // S3 can answer 200 OK while streaming whitespace and then
                // write an <Error> element at the end of the body.
                let body = answer.bytes_up_to(MULTIPART_XML_LIMIT).await?;
                let root = xml::parse(&body)?;
                if root.name() == "Error" {
                    let code = root.value("Code").map(str::trim).unwrap_or("");
                    if matches!(
                        code,
                        "InternalError" | "SlowDown" | "RequestTimeout" | "ServiceUnavailable"
                    ) {
                        return Err(Error::busy("the store asked to complete the upload again")
                            .with_code(code));
                    }
                    return Err(Error::new(
                        ErrorKind::Answer,
                        "the store refused to complete the upload",
                    )
                    .with_code(code));
                }
                Ok(())
            }
        })
        .await
    }

    async fn abort_remote(&self) -> Result<()> {
        let mut abort_url = self.object_url.clone();
        abort_url.set_query(Some(&format!(
            "uploadId={}",
            encode_component(&self.upload_id)
        )));

        let store = &self.store;
        with_backoff(&store.retry, Replay::Safe, || {
            let abort_url = abort_url.clone();
            async move {
                let credential = store.credentials.credential().await?;
                let mut request = Request::new(Method::DELETE, abort_url);
                sigv4::sign_request(&mut request, &credential.secret, &store.region, store.now())?;
                let answer = store.client.send(request).await?;
                if answer.status() == StatusCode::NOT_FOUND {
                    return Ok(());
                }
                answer.success().await.map(drop).map_err(enrich_error)
            }
        })
        .await
    }
}

impl Upload for S3Upload {
    fn write<'a>(&'a mut self, chunk: Bytes) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move {
            if self.failed {
                return Err(StoreError::failed(
                    "the multipart upload has already failed",
                ));
            }
            self.buffer.extend_from_slice(&chunk);
            while self.buffer.len() >= self.part_size {
                let part = self.buffer.split_to(self.part_size).freeze();
                if let Err(error) = self.send_part(part).await {
                    self.failed = true;
                    return Err(StoreError::from(error));
                }
            }
            Ok(())
        })
    }

    fn finish(mut self: Box<Self>) -> BoxFuture<'static, StoreResult<()>> {
        Box::pin(async move {
            if self.failed {
                let _ = self.abort_remote().await;
                return Err(StoreError::failed(
                    "the multipart upload has already failed",
                ));
            }
            // S3 requires at least one part in CompleteMultipartUpload, so an
            // empty stream uploads a single zero-byte part.
            if !self.buffer.is_empty() || self.parts.is_empty() {
                let tail = self.buffer.split().freeze();
                if let Err(error) = self.send_part(tail).await {
                    let _ = self.abort_remote().await;
                    return Err(StoreError::from(error));
                }
            }
            if let Err(error) = self.complete_remote().await {
                let _ = self.abort_remote().await;
                return Err(StoreError::from(error));
            }
            Ok(())
        })
    }

    fn abort(self: Box<Self>) -> BoxFuture<'static, StoreResult<()>> {
        Box::pin(async move { self.abort_remote().await.map_err(StoreError::from) })
    }
}
