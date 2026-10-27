//! [`Exports`]: downloads with the app's CSV options and translated
//! headers, and registered exports run at once or queued.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use navree_contracts::BoxError;
use navree_db::Uow;
use navree_kernel::{AppContext, WeakAppContext};
use navree_queue::{Operation, OperationRecord, Operations};
use serde_json::Value;

use super::job::QueuedExport;
use super::registry::{ExportRegistration, ExportRegistry, Source};
use super::run::{Limits, Paging, Progress, csv_stream, xlsx_file};
use super::{
    Export, ExportColumnInfo, ExportContext, ExportInfo, ExportRequest, ExportResult,
};
use crate::download::{csv_download, csv_stream_download, with_extension, xlsx_download};
use crate::headers::resolve_headers;
use crate::support::{self, current_locale};
use crate::write::csv::CsvEncoder;
use crate::{
    CSV_CONTENT_TYPE, Download, Exportable, HeaderTranslator, SheetFormat, SheetsError,
    SheetsOptions, XLSX_CONTENT_TYPE,
};

/// Exports: provided and exported by [`SheetsModule`](crate::SheetsModule).
///
/// Downloads of rows you have, written with the app's `sheets.csv`
/// options and headers translated in the request's locale:
///
/// ```ignore
/// async fn export(exports: Inject<Exports>, uow: Uow) -> Result<Download, AppError> {
///     let rows = contacts(&uow).await?;            // Vec<ContactRow>
///     Ok(exports.csv("contacts", rows))
/// }
/// ```
///
/// Registered [`Export`]s, run at once ([`download`](Exports::download),
/// `GET /api/exports/{name}`) or queued ([`queue`](Exports::queue),
/// `POST /api/exports/{name}`): a queued export is an operation whose job
/// writes the file to `sheets.exports.disk` and finishes with an
/// [`ExportResult`] holding a signed link.
pub struct Exports {
    options: Arc<SheetsOptions>,
    translator: Option<Arc<dyn HeaderTranslator>>,
    registry: ExportRegistry,
    app: WeakAppContext,
}

impl fmt::Debug for Exports {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Exports")
            .field("options", &self.options)
            .field("translator", &self.translator.is_some())
            .finish_non_exhaustive()
    }
}

impl Exports {
    pub(crate) async fn new(
        app: &AppContext,
        options: Arc<SheetsOptions>,
    ) -> Result<Exports, String> {
        Ok(Exports {
            translator: support::header_translator(app).await,
            registry: ExportRegistry::build(app)?,
            options,
            app: app.downgrade(),
        })
    }

    /// The options exports run with (`sheets`).
    pub fn options(&self) -> &SheetsOptions {
        &self.options
    }

    fn app(&self) -> Result<AppContext, SheetsError> {
        self.app
            .upgrade()
            .ok_or(SheetsError::Missing("the app has shut down"))
    }

    /// The headers of `T` in the current locale (the request's, with
    /// navree-i18n).
    pub fn headers<T: Exportable>(&self) -> Vec<String> {
        self.headers_in(&T::columns(), current_locale().as_deref())
    }

    fn headers_in(&self, columns: &[crate::Column], locale: Option<&str>) -> Vec<String> {
        resolve_headers(columns, self.translator.as_ref(), locale)
    }

    /// `rows` as a CSV download named `filename` (`.csv` added), with the
    /// `sheets.csv` options and translated headers.
    pub fn csv<T: Exportable>(
        &self,
        filename: impl Into<String>,
        rows: impl IntoIterator<Item = T>,
    ) -> Download {
        csv_download(
            filename.into(),
            &self.options.csv,
            &self.headers::<T>(),
            rows,
        )
    }

    /// The rows `rows` yields as a CSV download, streamed (see
    /// [`Download::csv_stream`]), with the `sheets.csv` options and
    /// translated headers.
    pub fn csv_stream<T, S, E>(&self, filename: impl Into<String>, rows: S) -> Download
    where
        T: Exportable + Send + 'static,
        S: Stream<Item = Result<T, E>> + Send + 'static,
        E: Into<BoxError> + Send + 'static,
    {
        csv_stream_download(
            filename.into(),
            self.options.csv.clone(),
            self.headers::<T>(),
            rows,
        )
    }

    /// `rows` as an XLSX download named `filename` (`.xlsx` added), with
    /// translated headers.
    pub fn xlsx<T: Exportable>(
        &self,
        filename: impl Into<String>,
        rows: impl IntoIterator<Item = T>,
    ) -> Result<Download, SheetsError> {
        xlsx_download(filename.into(), &self.headers::<T>(), rows)
    }

    /// `rows` as a download in `format`.
    pub fn file<T: Exportable>(
        &self,
        format: SheetFormat,
        filename: impl Into<String>,
        rows: impl IntoIterator<Item = T>,
    ) -> Result<Download, SheetsError> {
        match format {
            SheetFormat::Csv => Ok(self.csv(filename, rows)),
            SheetFormat::Xlsx => self.xlsx(filename, rows),
        }
    }

    /// The registered exports, by name.
    pub fn names(&self) -> Vec<&'static str> {
        self.registry.all().map(|r| r.name).collect()
    }

    pub(crate) fn registration(&self, name: &str) -> Result<&ExportRegistration, SheetsError> {
        self.registry.get(name)
    }

    pub(crate) fn registrations(&self) -> impl Iterator<Item = &ExportRegistration> {
        self.registry.all()
    }

    /// What `GET /api/exports` shows of `registration`.
    pub(crate) fn info(&self, registration: &ExportRegistration) -> ExportInfo {
        let columns = (registration.columns)();
        let headers = self.headers_in(&columns, current_locale().as_deref());
        ExportInfo {
            name: registration.name.to_owned(),
            columns: columns
                .into_iter()
                .zip(headers)
                .map(|(column, header)| ExportColumnInfo {
                    key: column.key.into_owned(),
                    header,
                })
                .collect(),
        }
    }

    /// The most rows an export in `format` writes.
    fn max_rows(&self, format: SheetFormat) -> u64 {
        match format {
            SheetFormat::Csv => self.options.exports.max_rows,
            SheetFormat::Xlsx => self.options.exports.max_rows.min(crate::XLSX_MAX_ROWS),
        }
    }

    fn limits(&self) -> Limits {
        Limits {
            page_size: self.options.exports.page_size,
            max_rows: self.options.exports.max_rows,
        }
    }

    fn context(&self, app: AppContext, request: &ExportRequest, locale: Option<String>) -> ExportContext {
        ExportContext {
            app,
            user_id: request.user_id,
            workspace_id: request.workspace_id,
            locale,
        }
    }

    /// Runs the registered export `E` now, as a download: CSV streamed page
    /// by page, XLSX built (in constant memory) before the response starts.
    /// When [`Export::count`] is above `sheets.exports.max_rows` it is
    /// [`SheetsError::TooManyRows`] before anything is written.
    pub async fn download<E: Export>(
        &self,
        params: E::Params,
        request: ExportRequest,
    ) -> Result<Download, SheetsError> {
        let params = serde_json::to_value(params)?;
        self.download_named(E::NAME, params, &request).await
    }

    pub(crate) async fn download_named(
        &self,
        name: &str,
        params: Value,
        request: &ExportRequest,
    ) -> Result<Download, SheetsError> {
        let app = self.app()?;
        let registration = self.registry.get(name)?;
        let locale = current_locale();
        let ctx = self.context(app.clone(), request, locale.clone());
        let source = registration.open(app, params).await?;
        let total = source.count(&ctx).await.map_err(SheetsError::App)?;
        let max_rows = self.max_rows(request.format);
        if let Some(total) = total
            && total > max_rows
        {
            return Err(SheetsError::TooManyRows { limit: max_rows });
        }
        let columns = (registration.columns)();
        let headers = self.headers_in(&columns, locale.as_deref());
        let filename = with_extension(&source.filename(), request.format);
        let paging = Paging {
            source,
            ctx,
            limits: self.limits(),
            total,
            progress: None,
            written: Arc::new(AtomicU64::new(0)),
            exceeded: Arc::new(AtomicU64::new(0)),
        };
        match request.format {
            SheetFormat::Csv => {
                let encoder = Arc::new(CsvEncoder::new(self.options.csv.clone())?);
                let chunks = csv_stream(paging, encoder, headers).map(|chunk| {
                    if let Err(error) = &chunk {
                        tracing::warn!(target: "navree::sheets", %error, "an export download stopped");
                    }
                    chunk
                });
                Ok(Download::stream(filename, CSV_CONTENT_TYPE, chunks))
            }
            SheetFormat::Xlsx => {
                let (file, _) = xlsx_file(&paging, (registration.sheet)(), &columns, &headers)
                    .await
                    .map_err(into_sheets_error)?;
                Ok(Download::bytes(filename, XLSX_CONTENT_TYPE, file))
            }
        }
    }

    /// Queues the registered export `E`, as part of `uow`: an operation
    /// (kind `export.{NAME}`, owned by the request's user) whose job writes
    /// the file to `sheets.exports.disk` and finishes the operation with an
    /// [`ExportResult`]. The job runs in the current locale. Needs
    /// `DatabaseModule`, `QueueModule` and `StorageModule`.
    pub async fn queue<E: Export>(
        &self,
        uow: &Uow,
        params: E::Params,
        request: ExportRequest,
    ) -> Result<OperationRecord, SheetsError> {
        let params = serde_json::to_value(params)?;
        self.queue_named(uow, E::NAME, params, &request).await
    }

    pub(crate) async fn queue_named(
        &self,
        uow: &Uow,
        name: &str,
        params: Value,
        request: &ExportRequest,
    ) -> Result<OperationRecord, SheetsError> {
        let app = self.app()?;
        self.registry.get(name)?;
        let operations = app.try_get::<Operations>().await.ok_or(SheetsError::Missing(
            "queued exports need QueueModule and DatabaseModule in the app",
        ))?;
        let mut operation =
            Operation::new(format!("export.{name}")).note("Waiting to start the export");
        if let Some(user) = request.user_id {
            operation = operation.owner(user);
        }
        if let Some(workspace) = request.workspace_id {
            operation = operation.workspace(workspace);
        }
        let record = operations.start(uow, operation).await?;
        let job = QueuedExport {
            export: name.to_owned(),
            params,
            format: request.format,
            operation_id: record.id,
            user_id: request.user_id,
            workspace_id: request.workspace_id,
            locale: current_locale(),
        };
        operations.dispatch(uow, &record, job).await?;
        Ok(record)
    }

    /// The work of a queued export (its job): writes the file, then
    /// finishes the operation with its link.
    pub(crate) async fn write_queued(
        &self,
        job: &QueuedExport,
        progress: Progress,
    ) -> Result<ExportResult, BoxError> {
        let app = self.app()?;
        let registration = self.registry.get(&job.export)?;
        let ctx = ExportContext {
            app: app.clone(),
            user_id: job.user_id,
            workspace_id: job.workspace_id,
            locale: job.locale.clone(),
        };
        let source: Arc<dyn Source> = registration.open(app.clone(), job.params.clone()).await?;
        let total = source.count(&ctx).await?;
        let max_rows = self.max_rows(job.format);
        if let Some(total) = total
            && total > max_rows
        {
            return Err(SheetsError::TooManyRows { limit: max_rows }.into());
        }
        let columns = (registration.columns)();
        let headers = self.headers_in(&columns, job.locale.as_deref());
        let filename = with_extension(&source.filename(), job.format);
        let disk = support::disk(&app, &self.options.exports.disk).await?;
        let now = support::now(&app).await;
        let path = format!(
            "{}/{}/{}/{}",
            SheetsOptions::clean(&self.options.exports.directory),
            now.format("%Y-%m-%d"),
            job.operation_id,
            safe_filename(&filename)
        );
        let written = Arc::new(AtomicU64::new(0));
        let exceeded = Arc::new(AtomicU64::new(0));
        let paging = Paging {
            source,
            ctx,
            limits: self.limits(),
            total,
            progress: Some(progress),
            written: written.clone(),
            exceeded: exceeded.clone(),
        };
        let bytes = match job.format {
            SheetFormat::Csv => {
                let encoder = Arc::new(CsvEncoder::new(self.options.csv.clone())?);
                match disk.put_stream(&path, csv_stream(paging, encoder, headers)).await {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        let _ = disk.delete(&path).await;
                        return match exceeded.load(Ordering::Relaxed) {
                            0 => Err(error.into()),
                            limit => Err(SheetsError::TooManyRows { limit }.into()),
                        };
                    }
                }
            }
            SheetFormat::Xlsx => {
                let (file, _) =
                    xlsx_file(&paging, (registration.sheet)(), &columns, &headers).await?;
                let size = file.len() as u64;
                disk.put(&path, Bytes::from(file)).await?;
                size
            }
        };
        let expires_at = now + support::seconds(self.options.exports.link_ttl_secs);
        let url = disk.temporary_url(&path, expires_at).await?;
        Ok(ExportResult {
            url,
            filename,
            format: job.format,
            rows: written.load(Ordering::Relaxed),
            bytes,
            expires_at,
            disk: disk.name().to_owned(),
            path,
        })
    }
}

impl Exports {
    /// Deletes the files of queued exports whose link has expired (written
    /// more than `sheets.exports.link_ttl_secs` ago); returns how many.
    /// What `sheets:prune` runs.
    pub async fn prune_files(&self) -> Result<u64, SheetsError> {
        let app = self.app()?;
        let disk = support::disk(&app, &self.options.exports.disk).await?;
        let before =
            support::now(&app).await - support::seconds(self.options.exports.link_ttl_secs);
        let mut pruned = 0;
        let directory = SheetsOptions::clean(&self.options.exports.directory);
        for file in disk.files(directory).await? {
            if file.last_modified < before {
                disk.delete(&file.path).await?;
                pruned += 1;
            }
        }
        Ok(pruned)
    }
}

/// A file name safe as the last segment of a path on any disk.
fn safe_filename(filename: &str) -> String {
    let cleaned: String = filename
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.').to_owned();
    if cleaned.is_empty() {
        "export".to_owned()
    } else {
        cleaned
    }
}

/// A boxed error back as a [`SheetsError`] (refusals keep their kind).
pub(crate) fn into_sheets_error(error: BoxError) -> SheetsError {
    match error.downcast::<SheetsError>() {
        Ok(error) => *error,
        Err(error) => SheetsError::App(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_are_safe_path_segments() {
        assert_eq!(safe_filename("contacts.csv"), "contacts.csv");
        assert_eq!(safe_filename("../a/b.csv"), "_a_b.csv");
        assert_eq!(safe_filename("جهات.xlsx"), "جهات.xlsx");
        assert_eq!(safe_filename("  "), "export");
    }

    #[test]
    fn refusals_survive_boxing() {
        let boxed: BoxError = SheetsError::TooManyRows { limit: 3 }.into();
        assert!(matches!(
            into_sheets_error(boxed),
            SheetsError::TooManyRows { limit: 3 }
        ));
        let boxed: BoxError = "other".into();
        assert!(matches!(into_sheets_error(boxed), SheetsError::App(_)));
    }
}
