//! The jobs of an import: staging, applying a chunk, and finishing.

use navree_contracts::{BoxError, Job, JobCancelled, JobContext, JobOptions};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::service::Imports;
use crate::support::in_locale;

/// `sheets.import.stage`: reads an uploaded file, maps its headers, and
/// checks and stores every row (the import's staging operation). A file
/// that cannot be read fails the import with the reason; when the job gives
/// up or is cancelled the import fails too.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageImport {
    /// The import.
    pub import_id: Uuid,
}

impl Job for StageImport {
    const NAME: &'static str = "sheets.import.stage";

    fn options() -> JobOptions {
        JobOptions {
            timeout_secs: 3_600,
            ..JobOptions::default()
        }
    }

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        let imports = ctx.app.get::<Imports>().await?;
        let locale = imports.find(self.import_id).await.ok().and_then(|r| r.locale);
        let app = ctx.app.clone();
        let outcome = in_locale(&app, locale.as_deref(), imports.stage(self.import_id, &ctx)).await;
        if let Err(error) = &outcome {
            let message = if error.is::<JobCancelled>() {
                Some("Reading the file was cancelled.")
            } else if ctx.attempt >= Self::options().max_attempts {
                Some("Reading the file failed.")
            } else {
                None
            };
            if let Some(message) = message {
                imports.staging_stopped(self.import_id, message).await?;
            }
        }
        outcome
    }
}

/// `sheets.import.apply`: saves the rows numbered `from_row..=to_row` of an
/// import being applied (one job of its batch).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyImportChunk {
    /// The import.
    pub import_id: Uuid,
    /// The first row number.
    pub from_row: u64,
    /// The last row number.
    pub to_row: u64,
}

impl Job for ApplyImportChunk {
    const NAME: &'static str = "sheets.import.apply";

    fn options() -> JobOptions {
        JobOptions {
            timeout_secs: 1_800,
            ..JobOptions::default()
        }
    }

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        let imports = ctx.app.get::<Imports>().await?;
        let locale = imports.find(self.import_id).await.ok().and_then(|r| r.locale);
        let app = ctx.app.clone();
        in_locale(
            &app,
            locale.as_deref(),
            imports.apply_chunk(self.import_id, self.from_row, self.to_row, &ctx),
        )
        .await
    }
}

/// `sheets.import.finish`: the `finally` of an apply batch: counts the
/// rows, writes the error file and completes the import (or, when applying
/// was cancelled, puts it back to `staged`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FinishImport {
    /// The import.
    pub import_id: Uuid,
}

impl Job for FinishImport {
    const NAME: &'static str = "sheets.import.finish";

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        let imports = ctx.app.get::<Imports>().await?;
        imports.finish(self.import_id).await
    }
}
