//! [`Imports`]: uploads, staging, the preview, mappings, applying and
//! error files.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::Arc;

use bytes::Bytes;
use navree_contracts::{BoxError, JobContext};
use navree_db::{Db, Uow};
use navree_kernel::{AppContext, WeakAppContext};
use navree_queue::{Batch, Operation, OperationStatus, Operations};
use serde_json::Value;
use uuid::Uuid;

use super::jobs::{ApplyImportChunk, FinishImport, StageImport};
use super::mapping::{self, normalize};
use super::read::{ReadOptions, read_table, refuse};
use super::registry::{ImporterRegistration, ImporterRegistry, RowChecker, SaveError};
use super::store::{self, NewRow};
use super::{
    DuplicateMode, ImportColumn, ImportContext, ImportCounts, ImportRecord, ImportStatus, Mapping,
    PageMeta, RowQuery, RowStatus, RowsPage,
};
use crate::download::{CSV_CONTENT_TYPE, Download};
use crate::support;
use crate::write::csv::CsvEncoder;
use crate::write::xlsx::XlsxBuilder;
use crate::{Cell, Column, SheetFormat, SheetsError, SheetsOptions, XLSX_CONTENT_TYPE};

/// The message of a row that repeats an earlier row's key.
fn repeats(row: u64) -> String {
    format!("Repeats row {row}")
}

/// The error-file message of a duplicate left alone.
const SKIPPED_MESSAGE: &str = "A record with this key already exists";

/// The message of a row that failed to save for a reason the import did
/// not give.
const FAILED_ROW_MESSAGE: &str = "This row could not be saved";

/// Rows read per page while writing an error file.
const ERROR_FILE_PAGE: u64 = 1_000;

/// An uploaded file: its name and contents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Upload {
    /// The file name the client sent (`recipients.csv`); its extension
    /// tells CSV from TSV.
    pub filename: String,
    /// The contents.
    pub bytes: Bytes,
}

impl Upload {
    /// A file named `filename` holding `bytes`.
    pub fn new(filename: impl Into<String>, bytes: impl Into<Bytes>) -> Self {
        Upload {
            filename: filename.into(),
            bytes: bytes.into(),
        }
    }
}

/// How an upload is read, and who it belongs to.
///
/// ```
/// use navree_sheets::{Mapping, UploadOptions};
///
/// let options = UploadOptions::default()
///     .encoding("windows-1256")
///     .mapping(Mapping::new().map("email", "البريد الإلكتروني"))
///     .save_mapping(true);
/// assert_eq!(options.encoding.as_deref(), Some("windows-1256"));
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UploadOptions {
    /// The text encoding of a CSV file (`windows-1256`, `shift_jis`);
    /// detected when `None`.
    pub encoding: Option<String>,
    /// The workbook sheet to read; the first when `None`.
    pub sheet: Option<String>,
    /// Which header each field reads; found automatically (or from a
    /// saved mapping) when `None`.
    pub mapping: Option<Mapping>,
    /// Save `mapping` for later files with the same headers.
    pub save_mapping: bool,
    /// The user uploading (the owner of the import and its operations).
    pub owner_id: Option<Uuid>,
    /// The workspace it belongs to.
    pub workspace_id: Option<Uuid>,
}

impl UploadOptions {
    /// The text encoding of a CSV file.
    pub fn encoding(mut self, label: impl Into<String>) -> Self {
        self.encoding = Some(label.into());
        self
    }

    /// The workbook sheet to read.
    pub fn sheet(mut self, name: impl Into<String>) -> Self {
        self.sheet = Some(name.into());
        self
    }

    /// The mapping to use.
    pub fn mapping(mut self, mapping: Mapping) -> Self {
        self.mapping = Some(mapping);
        self
    }

    /// Save the mapping for later files with the same headers.
    pub fn save_mapping(mut self, save: bool) -> Self {
        self.save_mapping = save;
        self
    }

    /// The uploading user.
    pub fn owner(mut self, user_id: Uuid) -> Self {
        self.owner_id = Some(user_id);
        self
    }

    /// The workspace.
    pub fn workspace(mut self, workspace_id: Uuid) -> Self {
        self.workspace_id = Some(workspace_id);
        self
    }
}

/// Imports: provided and exported by [`SheetsModule`](crate::SheetsModule).
///
/// An import goes through these states ([`ImportStatus`]):
///
/// 1. [`upload`](Imports::upload) stores the file on
///    `sheets.imports.disk` and queues its staging as an operation
///    (`uploaded`).
/// 2. Staging (`staging`) detects the encoding, reads the CSV, TSV, XLSX,
///    XLS or ODS file, maps its headers to the importer's columns (a
///    chosen mapping, else one saved for files with these headers, else
///    automatic), reads and validates every row, finds rows that repeat
///    an earlier row's key and rows whose key the app already has, and
///    stores each row in `import_rows`. A required column no header
///    matched stops at `needs_mapping`: choose one with
///    [`map`](Imports::map).
/// 3. `staged`: [`rows`](Imports::rows) previews the rows by status.
/// 4. [`apply`](Imports::apply) saves the valid rows (and updates or
///    skips the duplicates) in queue batches, each row in its own unit of
///    work (`applying`), then writes the error file (`completed`).
pub struct Imports {
    options: Arc<SheetsOptions>,
    registry: ImporterRegistry,
    app: WeakAppContext,
}

impl fmt::Debug for Imports {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Imports")
            .field("options", &self.options)
            .field("importers", &self.importers())
            .finish_non_exhaustive()
    }
}

impl Imports {
    pub(crate) fn new(app: &AppContext, options: Arc<SheetsOptions>) -> Result<Imports, String> {
        Ok(Imports {
            registry: ImporterRegistry::build(app)?,
            options,
            app: app.downgrade(),
        })
    }

    /// The options imports run with (`sheets`).
    pub fn options(&self) -> &SheetsOptions {
        &self.options
    }

    /// The registered importers, by name.
    pub fn importers(&self) -> Vec<&'static str> {
        self.registry.all().map(|r| r.name).collect()
    }

    pub(crate) fn registration(
        &self,
        importer: &str,
    ) -> Result<&ImporterRegistration, SheetsError> {
        self.registry.get(importer)
    }

    pub(crate) fn registrations(&self) -> impl Iterator<Item = &ImporterRegistration> {
        self.registry.all()
    }

    /// The columns of `importer`.
    pub fn columns(&self, importer: &str) -> Result<Vec<ImportColumn>, SheetsError> {
        Ok((self.registry.get(importer)?.columns)())
    }

    /// An empty file with `importer`'s headers, to fill in: CSV (with the
    /// `sheets.csv` options) or XLSX.
    pub fn template(&self, importer: &str, format: SheetFormat) -> Result<Download, SheetsError> {
        let columns = self.columns(importer)?;
        let headers: Vec<String> = columns.iter().map(|c| c.header.to_string()).collect();
        let filename = format!("{importer}-template.{}", format.extension());
        match format {
            SheetFormat::Csv => {
                let encoder = CsvEncoder::new(self.options.csv.clone())?;
                Ok(Download::bytes(
                    filename,
                    CSV_CONTENT_TYPE,
                    encoder.header(&headers)?,
                ))
            }
            SheetFormat::Xlsx => {
                let sheet_columns: Vec<Column> = columns
                    .iter()
                    .map(|c| Column::new(c.field.clone(), c.header.clone()))
                    .collect();
                let builder = XlsxBuilder::new(importer, &sheet_columns, &headers, false)?;
                Ok(Download::bytes(
                    filename,
                    XLSX_CONTENT_TYPE,
                    builder.finish()?,
                ))
            }
        }
    }

    fn app(&self) -> Result<AppContext, SheetsError> {
        self.app
            .upgrade()
            .ok_or(SheetsError::Missing("the app has shut down"))
    }

    async fn db(app: &AppContext) -> Result<Arc<Db>, SheetsError> {
        app.try_get::<Db>().await.ok_or(SheetsError::Missing(
            "imports need DatabaseModule in the app",
        ))
    }

    async fn operations(app: &AppContext) -> Result<Arc<Operations>, SheetsError> {
        app.try_get::<Operations>()
            .await
            .ok_or(SheetsError::Missing(
                "imports need QueueModule (and DatabaseModule) in the app",
            ))
    }

    /// Stores `upload` for `importer` and queues its staging, as part of
    /// `uow`: an operation (kind `import.{importer}`) the client follows
    /// at `GET /api/operations/{id}/events`. The file is refused at once
    /// when it is larger than `sheets.imports.max_upload_bytes`, empty, or
    /// plainly not a spreadsheet (a PDF, an image).
    pub async fn upload(
        &self,
        uow: &Uow,
        importer: &str,
        upload: Upload,
        options: UploadOptions,
    ) -> Result<ImportRecord, SheetsError> {
        let app = self.app()?;
        let registration = self.registry.get(importer)?;
        let limit = self.options.imports.max_upload_bytes;
        if upload.bytes.len() as u64 > limit {
            return Err(SheetsError::FileTooLarge { limit });
        }
        refuse(&upload.bytes)?;
        let operations = Self::operations(&app).await?;
        let disk = support::disk(&app, &self.options.imports.disk).await?;
        let id = Uuid::now_v7();
        let filename = display_name(&upload.filename);
        let path = format!(
            "{}/{id}/upload.{}",
            SheetsOptions::clean(&self.options.imports.directory),
            extension(&filename)
        );
        disk.put(&path, upload.bytes).await?;
        let now = support::now(&app).await;
        let mut record = ImportRecord {
            id,
            importer: registration.name.to_owned(),
            status: ImportStatus::Uploaded,
            filename,
            format: None,
            encoding: options.encoding.filter(|e| !e.trim().is_empty()),
            sheet: options.sheet.filter(|s| !s.trim().is_empty()),
            headers: Vec::new(),
            mapping: options.mapping.clone().unwrap_or_default(),
            missing: Vec::new(),
            duplicates: None,
            counts: ImportCounts::default(),
            operation_id: None,
            has_error_file: false,
            error: None,
            owner_id: options.owner_id,
            workspace_id: options.workspace_id,
            created_at: now,
            updated_at: now,
            finished_at: None,
            disk: disk.name().to_owned(),
            path,
            error_path: None,
            locale: support::current_locale(),
            mapping_chosen: options.mapping.is_some(),
            save_mapping: options.save_mapping && options.mapping.is_some(),
        };
        record.operation_id = Some(self.queue_staging(uow, &operations, &record).await?);
        store::insert_import(uow, &record).await?;
        Ok(record)
    }

    async fn queue_staging(
        &self,
        uow: &Uow,
        operations: &Operations,
        record: &ImportRecord,
    ) -> Result<Uuid, SheetsError> {
        let operation = self.operation(record).note("Waiting to read the file");
        let operation = operations.start(uow, operation).await?;
        operations
            .dispatch(
                uow,
                &operation,
                StageImport {
                    import_id: record.id,
                },
            )
            .await?;
        Ok(operation.id)
    }

    fn operation(&self, record: &ImportRecord) -> Operation {
        let mut operation = Operation::new(format!("import.{}", record.importer));
        if let Some(owner) = record.owner_id {
            operation = operation.owner(owner);
        }
        if let Some(workspace) = record.workspace_id {
            operation = operation.workspace(workspace);
        }
        operation
    }

    /// The import `id`.
    pub async fn find(&self, id: Uuid) -> Result<ImportRecord, SheetsError> {
        let db = Self::db(&self.app()?).await?;
        store::find_import(&*db, id)
            .await?
            .ok_or(SheetsError::NotFound)
    }

    /// The imports `owner` uploaded (in `workspace`, when given), newest
    /// first, at most `limit`.
    pub async fn list(
        &self,
        owner: Uuid,
        workspace: Option<Uuid>,
        limit: u64,
    ) -> Result<Vec<ImportRecord>, SheetsError> {
        let db = Self::db(&self.app()?).await?;
        Ok(store::imports_of(&*db, owner, workspace, limit).await?)
    }

    /// A page of the import's staged rows, by row number: every row, or
    /// those in `query.status`.
    pub async fn rows(&self, id: Uuid, query: &RowQuery) -> Result<RowsPage, SheetsError> {
        self.find(id).await?;
        let per_page = query.per_page.unwrap_or(50).clamp(1, 500);
        let page = query.page.unwrap_or(1).max(1);
        let statuses: Vec<RowStatus> = query.status.into_iter().collect();
        let db = Self::db(&self.app()?).await?;
        let (data, total) = store::rows_page(
            &*db,
            id,
            &statuses,
            (page - 1).saturating_mul(per_page),
            per_page,
        )
        .await?;
        Ok(RowsPage {
            data,
            meta: PageMeta {
                current_page: page,
                per_page,
                total,
                last_page: total.div_ceil(per_page).max(1),
            },
        })
    }

    /// Chooses the mapping of an import that is `needs_mapping` or
    /// `staged` and stages it again with it, as part of `uow`. With
    /// `save`, the mapping is kept for later files with the same headers.
    /// A mapping that names a field the importer lacks or a header the
    /// file lacks, reads one header twice, or leaves a required field
    /// unmapped is [`SheetsError::Mapping`] (errors by field).
    pub async fn map(
        &self,
        uow: &Uow,
        id: Uuid,
        mapping: Mapping,
        save: bool,
    ) -> Result<ImportRecord, SheetsError> {
        let app = self.app()?;
        let mut record = self.find(id).await?;
        if !matches!(
            record.status,
            ImportStatus::NeedsMapping | ImportStatus::Staged
        ) {
            return Err(SheetsError::NotReady(format!(
                "The import is {}; a mapping can be chosen once the file has been read.",
                record.status
            )));
        }
        let columns = self.columns(&record.importer)?;
        mapping::check(&mapping, &columns, &record.headers).map_err(SheetsError::Mapping)?;
        let operations = Self::operations(&app).await?;
        record.mapping = mapping;
        record.mapping_chosen = true;
        record.save_mapping = save;
        record.missing = Vec::new();
        record.status = ImportStatus::Uploaded;
        record.error = None;
        record.updated_at = support::now(&app).await;
        record.operation_id = Some(self.queue_staging(uow, &operations, &record).await?);
        store::update_import(uow, &record).await?;
        Ok(record)
    }

    /// Applies a `staged` import, as part of `uow`: an operation (kind
    /// `import.{importer}`) runs a batch of jobs of `sheets.imports.chunk_size`
    /// rows. Valid rows are saved with [`Import::insert`](crate::Import::insert);
    /// duplicates are left alone (`skip`), saved with
    /// [`Import::update`](crate::Import::update) (`update`), or, with
    /// `fail`, applying is refused while the file has duplicates or
    /// repeats ([`SheetsError::Duplicates`]).
    pub async fn apply(
        &self,
        uow: &Uow,
        id: Uuid,
        duplicates: DuplicateMode,
    ) -> Result<ImportRecord, SheetsError> {
        let app = self.app()?;
        let mut record = self.find(id).await?;
        match record.status {
            ImportStatus::Staged => {}
            ImportStatus::NeedsMapping => {
                return Err(SheetsError::NotReady(
                    "Choose a mapping for the file's columns first.".into(),
                ));
            }
            ImportStatus::Applying => {
                return Err(SheetsError::NotReady(
                    "The import is already being applied.".into(),
                ));
            }
            ImportStatus::Completed => {
                return Err(SheetsError::NotReady(
                    "The import has already been applied.".into(),
                ));
            }
            other => {
                return Err(SheetsError::NotReady(format!(
                    "The import is {other}; only a staged import can be applied."
                )));
            }
        }
        let conflicts = record.counts.duplicate + record.counts.repeated;
        if duplicates == DuplicateMode::Fail && conflicts > 0 {
            return Err(SheetsError::Duplicates { count: conflicts });
        }
        let registration = self.registry.get(&record.importer)?;
        let operations = Self::operations(&app).await?;
        let ranges = store::row_ranges(
            uow,
            id,
            &[RowStatus::Valid, RowStatus::Duplicate],
            self.options.imports.chunk_size,
        )
        .await?;
        let to_apply = record.counts.valid + record.counts.duplicate;
        let operation = operations
            .start(
                uow,
                self.operation(&record)
                    .total(to_apply)
                    .note("Waiting to save the rows"),
            )
            .await?;
        let mut chunks: Vec<ApplyImportChunk> = ranges
            .into_iter()
            .map(|(from_row, to_row)| ApplyImportChunk {
                import_id: id,
                from_row,
                to_row,
            })
            .collect();
        if chunks.is_empty() {
            // The batch still runs its follow-up, which writes the error file.
            chunks.push(ApplyImportChunk {
                import_id: id,
                from_row: 0,
                to_row: 0,
            });
        }
        let batch = Batch::new(format!("import.{}", registration.name))
            .add(chunks)
            .allow_failures()
            .finally(FinishImport { import_id: id });
        operations.dispatch_batch(uow, &operation, batch).await?;
        record.status = ImportStatus::Applying;
        record.duplicates = Some(duplicates);
        record.operation_id = Some(operation.id);
        record.error = None;
        record.updated_at = support::now(&app).await;
        store::update_import(uow, &record).await?;
        Ok(record)
    }

    /// Deletes an import that is not being applied: its rows, its record,
    /// its upload and its error file.
    pub async fn discard(&self, uow: &Uow, id: Uuid) -> Result<(), SheetsError> {
        let app = self.app()?;
        let record = self.find(id).await?;
        if record.status == ImportStatus::Applying {
            return Err(SheetsError::NotReady(
                "The import is being applied; it can be deleted once it finishes.".into(),
            ));
        }
        store::delete_import(uow, id).await?;
        Self::delete_files(&app, &record).await;
        Ok(())
    }

    async fn delete_files(app: &AppContext, record: &ImportRecord) {
        let Ok(disk) = support::disk(app, &record.disk).await else {
            return;
        };
        for path in std::iter::once(&record.path).chain(record.error_path.as_ref()) {
            if let Err(error) = disk.delete(path).await
                && !matches!(error, navree_storage::StorageError::NotFound { .. })
            {
                tracing::warn!(target: "navree::sheets", import_id = %record.id, %error, "cannot delete an import file");
            }
        }
    }

    /// The error file of the import: a CSV of the rows that were not saved
    /// (invalid, repeated, skipped and failed rows), with the file's
    /// headers, the row number and the errors. `NotFound` when it has none.
    pub async fn error_file(&self, id: Uuid) -> Result<Download, SheetsError> {
        let app = self.app()?;
        let record = self.find(id).await?;
        let path = record.error_path.as_ref().ok_or(SheetsError::NotFound)?;
        let disk = support::disk(&app, &record.disk).await?;
        let stream = disk.get_stream(path).await?;
        let stem = record
            .filename
            .rsplit_once('.')
            .map_or(record.filename.as_str(), |(stem, _)| stem);
        Ok(Download::stream(
            format!("{stem}-errors.csv"),
            CSV_CONTENT_TYPE,
            stream,
        ))
    }

    /// Deletes imports (their rows, upload and error file) last changed
    /// more than `sheets.imports.keep_days` days ago, unless they are being
    /// applied; returns how many. What `sheets:prune` runs.
    pub async fn prune(&self) -> Result<u64, SheetsError> {
        let app = self.app()?;
        let db = Self::db(&app).await?;
        let keep = support::seconds(self.options.imports.keep_days.saturating_mul(86_400));
        let before = support::now(&app).await - keep;
        let stale = store::stale_imports(&*db, before).await?;
        for record in &stale {
            store::delete_import(&*db, record.id).await?;
            Self::delete_files(&app, record).await;
        }
        Ok(stale.len() as u64)
    }

    // -----------------------------------------------------------------
    // The work of the jobs.

    /// Marks an import failed with `message` and fails its operation.
    async fn fail(
        &self,
        app: &AppContext,
        conn: &Db,
        mut record: ImportRecord,
        message: String,
    ) -> Result<(), BoxError> {
        record.status = ImportStatus::Failed;
        record.error = Some(message.clone());
        record.updated_at = support::now(app).await;
        store::update_import(conn, &record).await?;
        if let Some(operation) = record.operation_id
            && let Some(operations) = app.try_get::<Operations>().await
        {
            operations.fail(operation, message).await?;
        }
        Ok(())
    }

    /// Marks the import failed when it is still being read (its staging
    /// job gave up or was cancelled).
    pub(crate) async fn staging_stopped(&self, id: Uuid, message: &str) -> Result<(), BoxError> {
        let app = self.app()?;
        let db = Self::db(&app).await?;
        if let Some(record) = store::find_import(&*db, id).await?
            && matches!(
                record.status,
                ImportStatus::Uploaded | ImportStatus::Staging
            )
        {
            self.fail(&app, &db, record, message.to_owned()).await?;
        }
        Ok(())
    }

    /// Staging (the `sheets.import.stage` job): reads the file, maps it,
    /// checks and stores every row.
    pub(crate) async fn stage(&self, id: Uuid, job: &JobContext) -> Result<(), BoxError> {
        let app = self.app()?;
        let db = Self::db(&app).await?;
        let conn = &*db;
        let Some(mut record) = store::find_import(conn, id).await? else {
            return Ok(());
        };
        if !matches!(
            record.status,
            ImportStatus::Uploaded | ImportStatus::Staging
        ) {
            return Ok(());
        }
        let operations = Self::operations(&app).await?;
        record.status = ImportStatus::Staging;
        record.error = None;
        record.updated_at = support::now(&app).await;
        store::update_import(conn, &record).await?;
        let registration = match self.registry.get(&record.importer) {
            Ok(registration) => registration,
            Err(error) => return self.fail(&app, conn, record, error.to_string()).await,
        };
        let disk = support::disk(&app, &record.disk).await?;
        let bytes = disk.get(&record.path).await?;
        let read = read_table(
            &bytes,
            &record.filename,
            &ReadOptions {
                encoding: record.encoding.as_deref(),
                sheet: record.sheet.as_deref(),
                max_rows: self.options.imports.max_rows,
            },
        );
        let table = match read {
            Ok(table) => table,
            Err(error) => {
                return match error {
                    SheetsError::UnsupportedFile(message) => {
                        self.fail(&app, conn, record, message).await
                    }
                    SheetsError::TooManyRows { limit } => {
                        let message = format!("The file has more than {limit} rows.");
                        self.fail(&app, conn, record, message).await
                    }
                    SheetsError::Invalid(message) => self.fail(&app, conn, record, message).await,
                    other => Err(other.into()),
                };
            }
        };
        drop(bytes);
        record.format = Some(table.kind);
        record.encoding = table.encoding.map(str::to_owned).or(record.encoding);
        record.headers = table.headers.clone();
        let columns = (registration.columns)();
        record.mapping = self.choose_mapping(conn, &record, &columns).await?;
        record.missing = mapping::missing(&record.mapping, &columns);
        let total = table.rows.len() as u64;
        store::delete_rows(conn, id).await?;
        if !record.missing.is_empty() {
            record.status = ImportStatus::NeedsMapping;
            record.counts = ImportCounts {
                total,
                ..ImportCounts::default()
            };
            record.updated_at = support::now(&app).await;
            store::update_import(conn, &record).await?;
            if let Some(operation) = record.operation_id {
                operations.complete(operation, &record).await?;
            }
            return Ok(());
        }

        let import = registration.open(app.clone()).await?;
        let checker = RowChecker::new(&app).await;
        let ctx = ImportContext {
            app: app.clone(),
            import_id: id,
            owner: record.owner_id,
            workspace: record.workspace_id,
        };
        // Where each mapped field's cells are.
        let positions: Vec<(String, usize)> = columns
            .iter()
            .filter_map(|column| {
                let header = record.mapping.header(&column.field)?;
                let index = record.headers.iter().position(|h| h == header)?;
                Some((column.field.to_string(), index))
            })
            .collect();
        let mut first_row_of: HashMap<String, u64> = HashMap::new();
        let mut counts = ImportCounts {
            total,
            ..ImportCounts::default()
        };
        let chunk = usize::try_from(self.options.imports.chunk_size).unwrap_or(usize::MAX);
        let mut done = 0u64;
        for rows in table.rows.chunks(chunk.max(1)) {
            let mut staged = Vec::with_capacity(rows.len());
            let mut new_keys = Vec::new();
            for row in rows {
                let fields: Vec<(String, Cell)> = positions
                    .iter()
                    .map(|(field, index)| {
                        (
                            field.clone(),
                            row.cells.get(*index).cloned().unwrap_or_default(),
                        )
                    })
                    .collect();
                let cells: BTreeMap<String, Value> = fields
                    .iter()
                    .map(|(field, cell)| (field.clone(), cell.to_json()))
                    .collect();
                let (status, key, errors) = match import.check(&checker, fields).await {
                    Err(errors) => (RowStatus::Invalid, None, errors.into_map()),
                    Ok(None) => (RowStatus::Valid, None, BTreeMap::new()),
                    Ok(Some(key)) => match first_row_of.get(&key) {
                        Some(first) => (RowStatus::Repeated, Some(key), row_error(repeats(*first))),
                        None => {
                            first_row_of.insert(key.clone(), row.number);
                            new_keys.push(key.clone());
                            (RowStatus::Valid, Some(key), BTreeMap::new())
                        }
                    },
                };
                staged.push(NewRow {
                    row: row.number,
                    status,
                    key,
                    cells,
                    errors,
                });
            }
            if !new_keys.is_empty() {
                let existing = import.existing(&ctx, &new_keys).await?;
                for row in &mut staged {
                    if row.status == RowStatus::Valid
                        && row.key.as_ref().is_some_and(|key| existing.contains(key))
                    {
                        row.status = RowStatus::Duplicate;
                    }
                }
            }
            for row in &staged {
                counts.add(row.status, 1);
            }
            store::insert_rows(conn, id, &staged).await?;
            done += rows.len() as u64;
            job.progress(done, total, "Checking rows").await?;
        }

        if record.save_mapping && record.mapping_chosen {
            let fingerprint = mapping::fingerprint(&record.headers);
            let now = support::now(&app).await;
            store::save_mapping(
                conn,
                &record.importer,
                record.workspace_id,
                &fingerprint,
                &record.mapping,
                now,
            )
            .await?;
        }
        record.status = ImportStatus::Staged;
        record.counts = counts;
        record.error_path = self.write_error_file(&app, &record, &columns).await?;
        record.has_error_file = record.error_path.is_some();
        record.updated_at = support::now(&app).await;
        store::update_import(conn, &record).await?;
        if let Some(operation) = record.operation_id {
            operations.complete(operation, &record).await?;
        }
        Ok(())
    }

    /// The mapping staging uses: the chosen one (without entries that do
    /// not fit the file), else the one saved for files with these headers,
    /// else the automatic one.
    async fn choose_mapping(
        &self,
        conn: &Db,
        record: &ImportRecord,
        columns: &[ImportColumn],
    ) -> Result<Mapping, BoxError> {
        let headers = &record.headers;
        if record.mapping_chosen {
            let mut used = Vec::new();
            return Ok(record
                .mapping
                .iter()
                .filter(|(field, header)| {
                    let fits = columns.iter().any(|c| c.field == *field)
                        && headers.iter().any(|h| h == header)
                        && !used.contains(header);
                    if fits {
                        used.push(*header);
                    }
                    fits
                })
                .collect());
        }
        let fingerprint = mapping::fingerprint(headers);
        if let Some(saved) =
            store::find_mapping(conn, &record.importer, record.workspace_id, &fingerprint).await?
        {
            // The saved headers, matched to this file's spelling of them.
            let adapted: Mapping = saved
                .iter()
                .filter_map(|(field, header)| {
                    let wanted = normalize(header);
                    let found = headers.iter().find(|h| normalize(h) == wanted)?;
                    Some((field.to_owned(), found.clone()))
                })
                .collect();
            if mapping::check(&adapted, columns, headers).is_ok() {
                return Ok(adapted);
            }
        }
        Ok(mapping::automatic(columns, headers))
    }

    /// Applying one chunk (the `sheets.import.apply` job): saves the rows
    /// numbered `from..=to`, each in its own unit of work.
    pub(crate) async fn apply_chunk(
        &self,
        id: Uuid,
        from: u64,
        to: u64,
        job: &JobContext,
    ) -> Result<(), BoxError> {
        let app = self.app()?;
        let db = Self::db(&app).await?;
        let conn = &*db;
        let Some(record) = store::find_import(conn, id).await? else {
            return Ok(());
        };
        if record.status != ImportStatus::Applying {
            return Ok(());
        }
        let mode = record.duplicates.unwrap_or_default();
        let registration = self.registry.get(&record.importer)?;
        let import = registration.open(app.clone()).await?;
        let checker = RowChecker::new(&app).await;
        let ctx = ImportContext {
            app: app.clone(),
            import_id: id,
            owner: record.owner_id,
            workspace: record.workspace_id,
        };
        let rows = store::rows_between(
            conn,
            id,
            from,
            to,
            &[RowStatus::Valid, RowStatus::Duplicate],
        )
        .await?;
        let total = rows.len() as u64;
        let mut skipped = Vec::new();
        for (index, row) in rows.into_iter().enumerate() {
            if index % 50 == 0 {
                job.check_cancelled().await?;
            }
            let update = match (row.status, mode) {
                (RowStatus::Valid, _) => false,
                (RowStatus::Duplicate, DuplicateMode::Update) => true,
                _ => {
                    skipped.push(row.row);
                    continue;
                }
            };
            let fields: Vec<(String, Cell)> = row
                .cells
                .iter()
                .map(|(field, value)| (field.clone(), Cell::from_json(value)))
                .collect();
            let uow = db.uow(record.owner_id, None).await;
            let saved = uow
                .scope(import.save(&ctx, &uow, &checker, fields, update))
                .await;
            let outcome = match saved {
                Ok(()) => {
                    let status = if update {
                        RowStatus::Updated
                    } else {
                        RowStatus::Created
                    };
                    let marked = async {
                        store::set_row(&uow, id, row.row, status, &BTreeMap::new()).await?;
                        uow.commit().await
                    }
                    .await;
                    match marked {
                        Ok(()) => None,
                        Err(error) => {
                            tracing::warn!(target: "navree::sheets", import_id = %id, row = row.row, %error, "an imported row did not commit");
                            Some(row_error(FAILED_ROW_MESSAGE.to_owned()))
                        }
                    }
                }
                Err(error) => {
                    if let Err(rollback) = uow.rollback().await {
                        tracing::warn!(target: "navree::sheets", import_id = %id, %rollback, "cannot roll back a failed row");
                    }
                    Some(match error {
                        SaveError::Invalid(errors) => errors.into_map(),
                        SaveError::Rejected(message) => row_error(message),
                        SaveError::Failed(error) => {
                            tracing::warn!(target: "navree::sheets", import_id = %id, row = row.row, %error, "an imported row failed to save");
                            row_error(FAILED_ROW_MESSAGE.to_owned())
                        }
                    })
                }
            };
            if let Some(errors) = outcome {
                store::set_row(conn, id, row.row, RowStatus::Failed, &errors).await?;
            }
        }
        store::set_rows(conn, id, &skipped, RowStatus::Skipped).await?;
        job.progress(total, total, "Saving rows").await?;
        Ok(())
    }

    /// After the apply batch (the `sheets.import.finish` job): counts the
    /// rows, writes the error file and completes the import, or puts it
    /// back to `staged` when applying was cancelled.
    pub(crate) async fn finish(&self, id: Uuid) -> Result<(), BoxError> {
        let app = self.app()?;
        let db = Self::db(&app).await?;
        let conn = &*db;
        let Some(mut record) = store::find_import(conn, id).await? else {
            return Ok(());
        };
        if record.status != ImportStatus::Applying {
            return Ok(());
        }
        let operations = Self::operations(&app).await?;
        let cancelled = match record.operation_id {
            Some(operation) => operations
                .find(operation)
                .await?
                .is_some_and(|op| op.status == OperationStatus::Cancelled),
            None => false,
        };
        record.counts = store::counts(conn, id).await?;
        let now = support::now(&app).await;
        if cancelled {
            record.status = ImportStatus::Staged;
            record.error = Some(format!(
                "Applying was cancelled after {} rows were saved; apply again to save the rest.",
                record.counts.created + record.counts.updated
            ));
        } else {
            record.status = ImportStatus::Completed;
            record.finished_at = Some(now);
        }
        let columns = self
            .registry
            .get(&record.importer)
            .map(|registration| (registration.columns)())
            .unwrap_or_default();
        record.error_path = self.write_error_file(&app, &record, &columns).await?;
        record.has_error_file = record.error_path.is_some();
        record.updated_at = now;
        store::update_import(conn, &record).await?;
        if !cancelled && let Ok(registration) = self.registry.get(&record.importer) {
            let import = registration.open(app.clone()).await?;
            let ctx = ImportContext {
                app: app.clone(),
                import_id: id,
                owner: record.owner_id,
                workspace: record.workspace_id,
            };
            if let Err(error) = import.completed(&ctx, &record).await {
                tracing::warn!(target: "navree::sheets", import_id = %id, %error, "the completed hook of an import failed");
            }
        }
        if let Some(operation) = record.operation_id {
            operations.complete(operation, &record).await?;
        }
        Ok(())
    }

    /// Writes the error file (`errors.csv` next to the upload) when rows
    /// were not saved, else deletes an old one; returns its path.
    async fn write_error_file(
        &self,
        app: &AppContext,
        record: &ImportRecord,
        columns: &[ImportColumn],
    ) -> Result<Option<String>, BoxError> {
        let disk = support::disk(app, &record.disk).await?;
        let path = format!(
            "{}/{}/errors.csv",
            SheetsOptions::clean(&self.options.imports.directory),
            record.id
        );
        if record.counts.reported() == 0 {
            if record.error_path.is_some() {
                let _ = disk.delete(&path).await;
            }
            return Ok(None);
        }
        let db = Self::db(app).await?;
        // The mapped fields, under the file's own headers, so the fixed
        // file maps the same way when it is uploaded again.
        let fields: Vec<(String, String)> = columns
            .iter()
            .filter_map(|column| {
                let header = record.mapping.header(&column.field)?;
                Some((column.field.to_string(), header.to_owned()))
            })
            .collect();
        let mut headers = vec!["Row".to_owned()];
        headers.extend(fields.iter().map(|(_, header)| header.clone()));
        headers.push("Errors".to_owned());
        let encoder = CsvEncoder::new(self.options.csv.clone())?;
        let mut file = encoder.header(&headers)?.to_vec();
        let mut after = 0u64;
        loop {
            let rows = store::rows_after(
                &*db,
                record.id,
                after,
                ERROR_FILE_PAGE,
                &RowStatus::REPORTED,
            )
            .await?;
            let Some(last) = rows.last() else {
                break;
            };
            after = last.row;
            let lines: Vec<Vec<Cell>> = rows
                .iter()
                .map(|row| {
                    let mut line = vec![Cell::Int(support::to_i64(row.row))];
                    line.extend(fields.iter().map(|(field, _)| {
                        row.cells
                            .get(field)
                            .map(Cell::from_json)
                            .unwrap_or_default()
                    }));
                    let mut messages: Vec<String> =
                        row.errors.values().flatten().cloned().collect();
                    if messages.is_empty() && row.status == RowStatus::Skipped {
                        messages.push(SKIPPED_MESSAGE.to_owned());
                    }
                    line.push(Cell::Text(messages.join("; ")));
                    line
                })
                .collect();
            file.extend_from_slice(&encoder.rows(&lines)?);
            if (rows.len() as u64) < ERROR_FILE_PAGE {
                break;
            }
        }
        disk.put(&path, Bytes::from(file)).await?;
        Ok(Some(path))
    }
}

/// `{"row": [message]}`.
fn row_error(message: String) -> BTreeMap<String, Vec<String>> {
    BTreeMap::from([("row".to_owned(), vec![message])])
}

/// The client's file name without directories or control characters.
fn display_name(filename: &str) -> String {
    let name = filename
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>();
    let name = name.trim();
    if name.is_empty() {
        "upload".to_owned()
    } else {
        name.chars().take(255).collect()
    }
}

/// The lower-case extension of `filename` when it is short and
/// alphanumeric, else `bin`.
fn extension(filename: &str) -> String {
    filename
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .filter(|ext| {
            !ext.is_empty() && ext.len() <= 8 && ext.chars().all(|c| c.is_ascii_alphanumeric())
        })
        .unwrap_or_else(|| "bin".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_file_names_lose_their_directories() {
        assert_eq!(
            display_name("C:\\Users\\ada\\recipients.csv"),
            "recipients.csv"
        );
        assert_eq!(display_name("../../etc/passwd"), "passwd");
        assert_eq!(display_name("  "), "upload");
        assert_eq!(display_name("a\u{0}b.csv"), "ab.csv");
    }

    #[test]
    fn extensions_are_kept_when_they_are_plain() {
        assert_eq!(extension("people.CSV"), "csv");
        assert_eq!(extension("book.xlsx"), "xlsx");
        assert_eq!(extension("noext"), "bin");
        assert_eq!(extension("x.c$v"), "bin");
    }
}
