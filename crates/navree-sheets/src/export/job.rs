//! The job of a queued export.

use std::sync::Arc;

use navree_contracts::{BoxError, Job, JobCancelled, JobContext, JobOptions};
use navree_queue::Operations;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::run::Progress;
use super::service::{Exports, into_sheets_error};
use crate::support::in_locale;
use crate::{SheetFormat, SheetsError};

/// `sheets.export`: writes a queued export's file and finishes its
/// operation with an [`ExportResult`](crate::ExportResult). Refusals
/// (more rows than `sheets.exports.max_rows`, parameters that no longer
/// parse) fail the operation with their message at once; other errors are
/// retried.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QueuedExport {
    /// The export's name.
    pub export: String,
    /// Its parameters, as JSON.
    pub params: Value,
    /// CSV or XLSX.
    pub format: SheetFormat,
    /// The operation it is the work of.
    pub operation_id: Uuid,
    /// The user who queued it.
    pub user_id: Option<Uuid>,
    /// The workspace it runs in.
    pub workspace_id: Option<Uuid>,
    /// The locale it runs in.
    pub locale: Option<String>,
}

impl Job for QueuedExport {
    const NAME: &'static str = "sheets.export";

    fn options() -> JobOptions {
        JobOptions {
            timeout_secs: 3_600,
            ..JobOptions::default()
        }
    }

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        let app = ctx.app.clone();
        let exports = app.get::<Exports>().await?;
        let operations = app.get::<Operations>().await?;
        let reporter = Arc::new(ctx);
        let progress: Progress = Arc::new(move |done, total| {
            let reporter = reporter.clone();
            Box::pin(async move {
                reporter
                    .progress(done, total.unwrap_or(0), "Exporting rows")
                    .await
                    .map_err(BoxError::from)
            })
        });
        let locale = self.locale.clone();
        let outcome = in_locale(&app, locale.as_deref(), exports.write_queued(&self, progress)).await;
        match outcome {
            Ok(result) => {
                operations.complete(self.operation_id, &result).await?;
                Ok(())
            }
            Err(error) if error.is::<JobCancelled>() => Err(error),
            Err(error) => {
                let message = match into_sheets_error(error) {
                    SheetsError::TooManyRows { limit } => {
                        format!("The export has more than {limit} rows.")
                    }
                    error @ (SheetsError::Invalid(_) | SheetsError::UnknownExport(_)) => {
                        error.to_string()
                    }
                    other => return Err(other.into()),
                };
                operations.fail(self.operation_id, message).await?;
                Ok(())
            }
        }
    }
}
