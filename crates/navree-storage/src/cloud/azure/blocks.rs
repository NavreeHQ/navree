//! Block blob multipart uploads (`PUT ?comp=block&blockid=...` followed by
//! `PUT ?comp=blocklist`).
//!
//! Each block asks [`Provider::credential`](crate::cloud::credentials::Provider::credential)
//! inside [`with_backoff`](crate::cloud::retry::with_backoff), so an OAuth
//! token that expires mid-upload is refreshed before the next block or retry
//! is sent. Uncommitted blocks are never visible to callers and are dropped
//! automatically by Azure if an upload is aborted.

use std::sync::Arc;

use ::http::Method;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use bytes::{Bytes, BytesMut};
use navree_crypto::random_token;
use navree_kernel::BoxFuture;
use url::Url;

use super::AzureInner;
use crate::cloud::http::Request;
use crate::cloud::keys::encode_component;
use crate::cloud::retry::{Replay, with_backoff};
use crate::cloud::{Error, ErrorKind, Result, xml};
use crate::store::{StoreError, StoreResult, Upload};

/// Default size of each staged block in [`AzureStore::begin_upload`](super::AzureStore):
/// 4 MiB.
pub const DEFAULT_BLOCK_SIZE: usize = 4 * 1024 * 1024;

/// Maximum block size accepted by Azure `Put Block` (`4000 MiB`).
pub(crate) const MAX_BLOCK_SIZE: usize = {
    let limit = 4000u64 * 1024 * 1024;
    if limit > usize::MAX as u64 {
        usize::MAX
    } else {
        limit as usize
    }
};

/// Azure allows at most 50,000 blocks in a single block blob.
const MAX_BLOCKS: usize = 50_000;

/// Stages fixed-width block IDs and commits them on [`Upload::finish`].
pub(crate) struct BlockUpload {
    inner: Arc<AzureInner>,
    url: Url,
    nonce: String,
    block_size: usize,
    buffer: BytesMut,
    blocks: Vec<String>,
}

impl BlockUpload {
    pub(crate) fn new(inner: Arc<AzureInner>, url: Url, block_size: usize) -> Self {
        Self {
            inner,
            url,
            nonce: random_token(16),
            block_size: block_size.clamp(1, MAX_BLOCK_SIZE),
            buffer: BytesMut::new(),
            blocks: Vec::new(),
        }
    }

    async fn put_block(&mut self, data: Bytes) -> Result<()> {
        if self.blocks.len() >= MAX_BLOCKS {
            return Err(Error::new(
                ErrorKind::Refused,
                "an Azure block blob holds at most 50,000 blocks",
            ));
        }
        // Azure requires every block ID of a blob to have the same length
        // before base64 encoding (25 ASCII bytes here: `<16-char-nonce>-<8-digits>`).
        let raw_id = format!("{}-{:08}", self.nonce, self.blocks.len());
        let block_id = STANDARD.encode(raw_id.as_bytes());

        let mut url = self.url.clone();
        url.set_query(Some(&format!(
            "comp=block&blockid={}",
            encode_component(&block_id)
        )));

        let inner = &self.inner;
        with_backoff(&inner.policy, Replay::Safe, || async {
            let cred = inner.credentials.credential().await?;
            let request = Request::new(Method::PUT, url.clone()).with_body(data.clone());
            let request = inner.authorize(request, &cred.secret)?;
            inner.send_checked(request).await.map(drop)
        })
        .await?;

        self.blocks.push(block_id);
        Ok(())
    }

    async fn commit(&mut self) -> Result<()> {
        if !self.buffer.is_empty() {
            let tail = self.buffer.split().freeze();
            self.put_block(tail).await?;
        }

        let mut xml_body = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<BlockList>");
        for block_id in &self.blocks {
            xml_body.push_str("\n<Latest>");
            xml_body.push_str(&xml::escape(block_id));
            xml_body.push_str("</Latest>");
        }
        if !self.blocks.is_empty() {
            xml_body.push('\n');
        }
        xml_body.push_str("</BlockList>");
        let payload = Bytes::from(xml_body);

        let mut url = self.url.clone();
        url.set_query(Some("comp=blocklist"));

        let inner = &self.inner;
        with_backoff(&inner.policy, Replay::Safe, || async {
            let cred = inner.credentials.credential().await?;
            let request = Request::new(Method::PUT, url.clone())
                .header("content-type", "application/xml")?
                .with_body(payload.clone());
            let request = inner.authorize(request, &cred.secret)?;
            inner.send_checked(request).await.map(drop)
        })
        .await
    }
}

impl Upload for BlockUpload {
    fn write<'a>(&'a mut self, chunk: Bytes) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move {
            self.buffer.extend_from_slice(&chunk);
            while self.buffer.len() >= self.block_size {
                let part = self.buffer.split_to(self.block_size).freeze();
                self.put_block(part).await.map_err(StoreError::from)?;
            }
            Ok(())
        })
    }

    fn finish(mut self: Box<Self>) -> BoxFuture<'static, StoreResult<()>> {
        Box::pin(async move { self.commit().await.map_err(StoreError::from) })
    }

    fn abort(self: Box<Self>) -> BoxFuture<'static, StoreResult<()>> {
        Box::pin(async move { Ok(()) })
    }
}
