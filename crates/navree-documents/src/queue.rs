//! Stored and queued documents: [`StoredDocument`], [`QueuedDocument`],
//! [`DocumentStatus`] and the [`RenderDocument`] job.

use chrono::{DateTime, Utc};
use navree_contracts::{Job, JobCancelled, JobContext, JobOptions};
use navree_kernel::BoxError;
use navree_storage::{Disk, StorageError};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::render::Render;
use crate::{Documents, DocumentsError, Pdf};

/// The suffix of the file that records why a queued render failed for
/// good, next to where the PDF would be (`exports/q1.pdf.failed`).
pub const FAILED_SUFFIX: &str = ".failed";

/// A document written to a disk by [`Documents::save`].
#[derive(Clone, Debug)]
pub struct StoredDocument {
    disk: Disk,
    /// The path on the disk.
    pub path: String,
    /// The size in bytes.
    pub size: u64,
}

impl StoredDocument {
    pub(crate) fn new(disk: Disk, path: &str, size: u64) -> Self {
        Self {
            disk,
            path: path.to_owned(),
            size,
        }
    }

    /// The disk it is on.
    pub fn disk(&self) -> &Disk {
        &self.disk
    }

    /// Its public URL (a disk with a `public_url` only).
    pub fn url(&self) -> Result<String, DocumentsError> {
        Ok(self.disk.url(&self.path)?)
    }

    /// A link that downloads it until `expires_at`, without a session: the
    /// cloud store's presigned URL on an S3, GCS or Azure disk, else a
    /// signed link served by navree-storage's `GET /storage/{disk}/{*path}`.
    pub async fn temporary_url(&self, expires_at: DateTime<Utc>) -> Result<String, DocumentsError> {
        Ok(self.disk.temporary_url(&self.path, expires_at).await?)
    }
}

/// A render queued by [`Documents::queue`]: where its PDF will be.
/// It serializes, so a handler can return it and a client can poll.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct QueuedDocument {
    /// The render's id.
    pub id: Uuid,
    /// The disk the PDF goes to.
    pub disk: String,
    /// The path on that disk.
    pub path: String,
    /// The queued job's id; `None` when the document was rendered at once
    /// (no dispatcher in the app).
    pub job_id: Option<String>,
    /// The operation the render is the work of (set by
    /// `Documents::queue_operation`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<Uuid>,
}

impl QueuedDocument {
    pub(crate) fn new(disk: String, path: String, job_id: Option<String>) -> Self {
        Self::with_id(Uuid::now_v7(), disk, path, job_id)
    }

    pub(crate) fn with_id(
        id: Uuid,
        disk: impl Into<String>,
        path: String,
        job_id: Option<String>,
    ) -> Self {
        Self {
            id,
            disk: disk.into(),
            path,
            job_id,
            operation_id: None,
        }
    }

    /// The same document, as the work of operation `id`.
    #[cfg(feature = "operations")]
    pub(crate) fn in_operation(mut self, id: Uuid) -> Self {
        self.operation_id = Some(id);
        self
    }
}

/// Where a queued document stands.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum DocumentStatus {
    /// Not rendered yet (queued, rendering, or waiting for a retry).
    Pending,
    /// On the disk.
    Ready {
        /// Its size in bytes.
        size: u64,
    },
    /// Rendering failed for good: a template or data error, or every
    /// attempt failed.
    Failed {
        /// Why, as the error's message. It can name template files: log it,
        /// and show people a message of your own.
        error: String,
    },
}

/// What [`Documents::status`] reads: the PDF, else the failure record.
pub(crate) async fn status(disk: &Disk, path: &str) -> Result<DocumentStatus, DocumentsError> {
    match disk.size(path).await {
        Ok(size) => return Ok(DocumentStatus::Ready { size }),
        Err(StorageError::NotFound { .. }) => {}
        Err(error) => return Err(error.into()),
    }
    match disk.get(&format!("{path}{FAILED_SUFFIX}")).await {
        Ok(bytes) => Ok(DocumentStatus::Failed {
            error: String::from_utf8_lossy(&bytes).into_owned(),
        }),
        Err(StorageError::NotFound { .. }) => Ok(DocumentStatus::Pending),
        Err(error) => Err(error.into()),
    }
}

/// The job behind [`Documents::queue`] (`documents.render`): renders the
/// document and writes it to the disk.
///
/// An error a retry can fix (the engine failed, timed out or was
/// unavailable, the disk failed) fails the attempt, so the queue retries
/// it; after the last attempt the failure is recorded. A template or data
/// error is recorded at once and the job ends without retrying: rendering
/// again cannot fix it. The failure record is `<path>.failed` on the same
/// disk; a later successful render removes it.
///
/// The job reports its progress (rendering, then saving) through
/// `ctx.progress`, and stops when its operation is cancelled, recording
/// the render as cancelled. As the work of an operation
/// (`Documents::queue_operation`, crate feature `operations`), it finishes
/// the operation itself: `succeeded` with `{"disk", "path", "size"}` as the
/// result, or `failed` at once on a template or data error.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RenderDocument {
    /// The render's id ([`QueuedDocument::id`]).
    pub id: Uuid,
    /// What to render.
    pub render: Render,
    /// The disk to write to.
    pub disk: String,
    /// The path on the disk.
    pub path: String,
    /// The operation this render is the work of.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<Uuid>,
}

/// The failure record of a render whose operation was cancelled.
const CANCELLED_MESSAGE: &str = "the render was cancelled";

/// The progress note while a queued render renders.
const RENDERING_NOTE: &str = "Rendering the document";

/// The progress note while a queued render is written to its disk.
const SAVING_NOTE: &str = "Saving the document";

/// Why a queued render stopped.
enum Stop {
    /// Its operation or batch was cancelled.
    Cancelled(JobCancelled),
    /// Rendering or storing failed.
    Failed(DocumentsError),
}

impl RenderDocument {
    pub(crate) fn new(render: Render, disk: &str, path: &str) -> Self {
        Self {
            id: Uuid::now_v7(),
            render,
            disk: disk.to_owned(),
            path: path.to_owned(),
            operation: None,
        }
    }

    /// The same job, as the work of operation `id`.
    #[cfg(feature = "operations")]
    pub(crate) fn in_operation(mut self, id: Uuid) -> Self {
        self.operation = Some(id);
        self
    }

    /// Renders and stores the document, returning its size: what
    /// [`Documents::queue`] does at once without a dispatcher.
    pub(crate) async fn run(&self, documents: &Documents) -> Result<u64, DocumentsError> {
        let disk = documents.storage_disk(&self.disk)?;
        let pdf = documents.render(&self.render).await?;
        self.store(&disk, pdf).await
    }

    /// What the job does: [`run`](RenderDocument::run), reporting progress
    /// before each step and stopping once the job is cancelled.
    async fn attempt(&self, documents: &Documents, ctx: &JobContext) -> Result<u64, Stop> {
        ctx.progress(0, 2, RENDERING_NOTE)
            .await
            .map_err(Stop::Cancelled)?;
        let disk = documents.storage_disk(&self.disk).map_err(Stop::Failed)?;
        let pdf = documents.render(&self.render).await.map_err(Stop::Failed)?;
        ctx.progress(1, 2, SAVING_NOTE)
            .await
            .map_err(Stop::Cancelled)?;
        self.store(&disk, pdf).await.map_err(Stop::Failed)
    }

    /// Writes `pdf` and removes an earlier failure record; returns its size.
    async fn store(&self, disk: &Disk, pdf: Pdf) -> Result<u64, DocumentsError> {
        let size = pdf.len() as u64;
        disk.put(&self.path, pdf.into_bytes()).await?;
        let marker = format!("{}{FAILED_SUFFIX}", self.path);
        if disk.exists(&marker).await? {
            disk.delete(&marker).await?;
        }
        Ok(size)
    }

    async fn record_failure(&self, documents: &Documents, message: &str) {
        let marker = format!("{}{FAILED_SUFFIX}", self.path);
        let written = match documents.storage_disk(&self.disk) {
            Ok(disk) => disk
                .put(&marker, message.to_owned())
                .await
                .map_err(DocumentsError::from),
            Err(error) => Err(error),
        };
        if let Err(write_error) = written {
            tracing::error!(
                target: "navree_documents",
                path = self.path,
                error = %write_error,
                "cannot record a failed render"
            );
        }
    }

    /// Finishes the render's operation as `succeeded`, with where the
    /// document is as its result.
    async fn completed(&self, ctx: &JobContext, size: u64) {
        #[cfg(feature = "operations")]
        if let Some(id) = self.operation
            && let Some(operations) = ctx.app.try_get::<navree_queue::Operations>().await
        {
            let result = serde_json::json!({ "disk": self.disk, "path": self.path, "size": size });
            if let Err(error) = operations.complete(id, result).await {
                tracing::warn!(
                    target: "navree_documents",
                    operation_id = %id,
                    %error,
                    "cannot record a rendered document on its operation"
                );
            }
        }
        #[cfg(not(feature = "operations"))]
        let _ = (ctx, size);
    }

    /// Finishes the render's operation as `failed`, with a message for
    /// people (the details are in the logs and the failure record).
    async fn failed(&self, ctx: &JobContext) {
        #[cfg(feature = "operations")]
        if let Some(id) = self.operation
            && let Some(operations) = ctx.app.try_get::<navree_queue::Operations>().await
            && let Err(error) = operations.fail(id, RENDER_FAILED_MESSAGE).await
        {
            tracing::warn!(
                target: "navree_documents",
                operation_id = %id,
                %error,
                "cannot record a failed render on its operation"
            );
        }
        #[cfg(not(feature = "operations"))]
        let _ = ctx;
    }
}

/// The error an operation shows when its render failed for good (a
/// template or data error); the details are in the logs and in the
/// `<path>.failed` record.
pub const RENDER_FAILED_MESSAGE: &str = "The document could not be rendered.";

/// Whether rendering again can succeed after `error`.
fn retryable(error: &DocumentsError) -> bool {
    matches!(
        error,
        DocumentsError::EngineUnavailable { .. }
            | DocumentsError::Engine { .. }
            | DocumentsError::Timeout { .. }
            | DocumentsError::Storage(_)
            | DocumentsError::Queue(_)
    )
}

impl Job for RenderDocument {
    const NAME: &'static str = "documents.render";

    /// Three attempts, five minutes each.
    fn options() -> JobOptions {
        JobOptions {
            timeout_secs: 300,
            ..JobOptions::default()
        }
    }

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        let documents = ctx.app.get::<Documents>().await?;
        let error = match self.attempt(&documents, &ctx).await {
            Ok(size) => {
                self.completed(&ctx, size).await;
                return Ok(());
            }
            Err(Stop::Cancelled(cancelled)) => {
                self.record_failure(&documents, CANCELLED_MESSAGE).await;
                return Err(Box::new(cancelled));
            }
            Err(Stop::Failed(error)) => error,
        };
        if !retryable(&error) {
            tracing::error!(
                target: "navree_documents",
                id = %self.id,
                template = self.render.name(),
                %error,
                "a queued render failed; not retrying"
            );
            self.record_failure(&documents, &error.to_string()).await;
            self.failed(&ctx).await;
            return Ok(());
        }
        if ctx.attempt >= Self::options().max_attempts {
            self.record_failure(&documents, &error.to_string()).await;
        }
        Err(Box::new(error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_a_retry_can_fix() {
        assert!(retryable(&DocumentsError::Timeout {
            engine: "chrome",
            secs: 30
        }));
        assert!(!retryable(&DocumentsError::Data("x".into())));
        assert!(!retryable(&DocumentsError::TemplateNotFound {
            name: "x".into(),
            tried: vec![]
        }));
    }

    #[test]
    fn statuses_serialize_with_a_tag() {
        assert_eq!(
            serde_json::to_value(DocumentStatus::Ready { size: 3 }).unwrap(),
            serde_json::json!({"status": "ready", "size": 3})
        );
        assert_eq!(
            serde_json::to_value(DocumentStatus::Pending).unwrap(),
            serde_json::json!({"status": "pending"})
        );
    }
}
