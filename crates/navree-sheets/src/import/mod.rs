//! Imports: [`Import`] definitions, the row types they read
//! ([`ImportRow`]), and what an import looks like while it is staged and
//! applied ([`ImportRecord`], [`ImportRowRecord`]).

pub(crate) mod controller;
pub(crate) mod de;
pub(crate) mod decode;
pub(crate) mod jobs;
pub(crate) mod mapping;
pub(crate) mod read;
pub(crate) mod registry;
pub(crate) mod service;
pub(crate) mod store;
pub(crate) mod workbook;
#[allow(dead_code)]
pub(crate) mod xls;

use std::borrow::Cow;
use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::future::Future;

use chrono::{DateTime, Utc};
use navree_contracts::BoxError;
use navree_db::Uow;
use navree_kernel::AppContext;
use navree_validation::Validate;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

pub use mapping::Mapping;
pub use read::FileKind;

/// One column an import reads: the row type's field, the header a template
/// shows and automatic mapping looks for, other headers it accepts, and
/// whether a file must have it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct ImportColumn {
    /// The field of the row type (`email`).
    #[schema(value_type = String)]
    pub field: Cow<'static, str>,
    /// The header (`Email`).
    #[schema(value_type = String)]
    pub header: Cow<'static, str>,
    /// Other headers automatic mapping accepts (`E-mail`, `Mail`).
    #[schema(value_type = Vec<String>)]
    pub aliases: Vec<Cow<'static, str>>,
    /// Whether the file must have the column.
    pub required: bool,
}

impl ImportColumn {
    /// A column for `field` headed `header`, optional.
    pub fn new(field: impl Into<Cow<'static, str>>, header: impl Into<Cow<'static, str>>) -> Self {
        ImportColumn {
            field: field.into(),
            header: header.into(),
            aliases: Vec::new(),
            required: false,
        }
    }

    /// Another header automatic mapping accepts.
    pub fn alias(mut self, alias: impl Into<Cow<'static, str>>) -> Self {
        self.aliases.push(alias.into());
        self
    }

    /// Whether the file must have the column.
    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }
}

/// A row type an import reads: its columns. Derive it with
/// `#[derive(Importable)]` next to serde's `Deserialize` and navree-validation's
/// `Validate`:
///
/// ```
/// use navree_validation::Validate;
/// use navree_sheets::{ImportRow, Importable};
/// use serde::Deserialize;
///
/// #[derive(Deserialize, Validate, Importable)]
/// struct RecipientRow {
///     #[import(header = "Email", aliases = ["E-mail", "Email address"])]
///     #[validate(email)]
///     email: String,
///     #[validate(length(max = 100))]
///     first_name: Option<String>,
/// }
///
/// let columns = RecipientRow::columns();
/// assert_eq!(columns[0].field, "email");
/// assert!(columns[0].required);
/// assert_eq!(columns[1].header, "First name");
/// assert!(!columns[1].required);
/// ```
pub trait ImportRow {
    /// The columns, in order.
    fn columns() -> Vec<ImportColumn>;
}

/// Who an import runs for, given to every [`Import`] method.
#[derive(Clone)]
pub struct ImportContext {
    /// The app, to resolve services.
    pub app: AppContext,
    /// The import.
    pub import_id: Uuid,
    /// The user who uploaded the file.
    pub owner: Option<Uuid>,
    /// The workspace it belongs to.
    pub workspace: Option<Uuid>,
}

impl fmt::Debug for ImportContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImportContext")
            .field("import_id", &self.import_id)
            .field("owner", &self.owner)
            .field("workspace", &self.workspace)
            .finish_non_exhaustive()
    }
}

/// An import: what a file's rows are read into, how duplicates are found,
/// and how a row is saved. Register it with `m.importer::<I>()`
/// ([`ModuleBuilderSheetsExt`](crate::ModuleBuilderSheetsExt)); it is
/// resolved from the app like any provider, so it can hold services.
///
/// ```
/// use std::collections::HashSet;
/// use std::sync::Arc;
///
/// use navree_validation::Validate;
/// use navree_contracts::BoxError;
/// use navree_db::Uow;
/// use navree_sheets::{Import, ImportContext, Importable, RowRejected};
/// use serde::Deserialize;
///
/// #[derive(Deserialize, Validate, Importable)]
/// struct ContactRow {
///     #[validate(email)]
///     email: String,
///     #[validate(length(min = 1, max = 100))]
///     name: String,
/// }
///
/// struct ContactsImport;
///
/// impl Import for ContactsImport {
///     const NAME: &'static str = "contacts";
///     type Row = ContactRow;
///
///     fn key(row: &ContactRow) -> Option<String> {
///         Some(row.email.to_lowercase())
///     }
///
///     async fn existing(&self, _ctx: &ImportContext, keys: &[String]) -> Result<HashSet<String>, BoxError> {
///         // SELECT email FROM contacts WHERE email IN (...)
///         Ok(HashSet::new())
///     }
///
///     async fn insert(&self, _ctx: &ImportContext, uow: &Uow, row: ContactRow) -> Result<(), BoxError> {
///         if row.name == "Robot" {
///             return Err(RowRejected::new("Robots cannot be contacts").into());
///         }
///         // Contact::create(uow, ...).await?;
///         Ok(())
///     }
/// }
/// ```
pub trait Import: Send + Sync + 'static {
    /// The importer's name: the `importer` of uploads and of the routes
    /// (`contacts`). Letters, digits, `-`, `_` and `.`.
    const NAME: &'static str;

    /// What one row is read into.
    type Row: ImportRow + DeserializeOwned + Validate + Send + Sync + 'static;

    /// The permission a user needs to upload and apply files of this
    /// import (default `{NAME}.import`, checked app-wide or in the import's
    /// workspace); `None` lets every signed-in user import.
    fn permission() -> Option<String> {
        Some(format!("{}.import", Self::NAME))
    }

    /// The row's duplicate key (an email, an employee number), or `None`
    /// when rows are never duplicates. Rows of one file with the same key
    /// are repeats; a key [`existing`](Import::existing) knows is a
    /// duplicate.
    fn key(row: &Self::Row) -> Option<String> {
        let _ = row;
        None
    }

    /// Which of `keys` the app already has (asked in chunks while staging).
    fn existing(
        &self,
        ctx: &ImportContext,
        keys: &[String],
    ) -> impl Future<Output = Result<HashSet<String>, BoxError>> + Send {
        let _ = (ctx, keys);
        async { Ok(HashSet::new()) }
    }

    /// Saves a new row, as part of `uow`. Return [`RowRejected`] for a row
    /// that cannot be saved (the message goes into the error file); any
    /// other error is logged and the row reported as failed.
    fn insert(
        &self,
        ctx: &ImportContext,
        uow: &Uow,
        row: Self::Row,
    ) -> impl Future<Output = Result<(), BoxError>> + Send;

    /// Updates the record a duplicate row stands for (applying with
    /// `duplicates = "update"`). By default duplicates cannot be updated.
    fn update(
        &self,
        ctx: &ImportContext,
        uow: &Uow,
        row: Self::Row,
    ) -> impl Future<Output = Result<(), BoxError>> + Send {
        let _ = (ctx, uow, row);
        async { Err(RowRejected::new("Existing records cannot be updated by this import").into()) }
    }

    /// Runs once an import of this kind has been applied, with its final
    /// counts (send a notification, refresh a cache).
    fn completed(
        &self,
        ctx: &ImportContext,
        import: &ImportRecord,
    ) -> impl Future<Output = Result<(), BoxError>> + Send {
        let _ = (ctx, import);
        async { Ok(()) }
    }
}

/// A row an [`Import`] refuses: the message goes into the error file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowRejected(String);

impl RowRejected {
    /// A refusal with a message for people.
    pub fn new(message: impl Into<String>) -> Self {
        RowRejected(message.into())
    }

    /// The message.
    pub fn message(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RowRejected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RowRejected {}

/// What happens to duplicate rows when an import is applied.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum DuplicateMode {
    /// Duplicates are left alone and listed in the error file (the
    /// default).
    #[default]
    Skip,
    /// Duplicates update the existing record ([`Import::update`]).
    Update,
    /// Applying is refused while the file has duplicates or repeats.
    Fail,
}

impl DuplicateMode {
    /// The stored name (`skip`).
    pub fn as_str(self) -> &'static str {
        match self {
            DuplicateMode::Skip => "skip",
            DuplicateMode::Update => "update",
            DuplicateMode::Fail => "fail",
        }
    }

    pub(crate) fn parse(name: &str) -> Option<Self> {
        match name {
            "skip" => Some(DuplicateMode::Skip),
            "update" => Some(DuplicateMode::Update),
            "fail" => Some(DuplicateMode::Fail),
            _ => None,
        }
    }
}

/// Where an import is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ImportStatus {
    /// The file is stored; reading it is queued.
    Uploaded,
    /// The file is being read and its rows checked.
    Staging,
    /// Some required columns could not be matched: choose a mapping.
    NeedsMapping,
    /// The rows are checked and can be previewed and applied.
    Staged,
    /// The rows are being saved.
    Applying,
    /// Every row was saved, skipped or reported.
    Completed,
    /// Reading or applying the file failed; `error` says why.
    Failed,
}

impl ImportStatus {
    /// The stored name (`staged`).
    pub fn as_str(self) -> &'static str {
        match self {
            ImportStatus::Uploaded => "uploaded",
            ImportStatus::Staging => "staging",
            ImportStatus::NeedsMapping => "needs_mapping",
            ImportStatus::Staged => "staged",
            ImportStatus::Applying => "applying",
            ImportStatus::Completed => "completed",
            ImportStatus::Failed => "failed",
        }
    }

    pub(crate) fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "uploaded" => ImportStatus::Uploaded,
            "staging" => ImportStatus::Staging,
            "needs_mapping" => ImportStatus::NeedsMapping,
            "staged" => ImportStatus::Staged,
            "applying" => ImportStatus::Applying,
            "completed" => ImportStatus::Completed,
            "failed" => ImportStatus::Failed,
            _ => return None,
        })
    }
}

impl fmt::Display for ImportStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where one row is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RowStatus {
    /// Checked, ready to be saved.
    Valid,
    /// A cell does not fit or a rule failed; `errors` says which.
    Invalid,
    /// Its key is one the app already has.
    Duplicate,
    /// Its key repeats an earlier row of the file.
    Repeated,
    /// Saved as a new record.
    Created,
    /// Updated an existing record.
    Updated,
    /// Left alone (a duplicate, applied with `skip`).
    Skipped,
    /// Saving it failed; `errors` says why.
    Failed,
}

impl RowStatus {
    /// The stored name (`valid`).
    pub fn as_str(self) -> &'static str {
        match self {
            RowStatus::Valid => "valid",
            RowStatus::Invalid => "invalid",
            RowStatus::Duplicate => "duplicate",
            RowStatus::Repeated => "repeated",
            RowStatus::Created => "created",
            RowStatus::Updated => "updated",
            RowStatus::Skipped => "skipped",
            RowStatus::Failed => "failed",
        }
    }

    /// `valid`, `invalid`, ... else `None`.
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "valid" => RowStatus::Valid,
            "invalid" => RowStatus::Invalid,
            "duplicate" => RowStatus::Duplicate,
            "repeated" => RowStatus::Repeated,
            "created" => RowStatus::Created,
            "updated" => RowStatus::Updated,
            "skipped" => RowStatus::Skipped,
            "failed" => RowStatus::Failed,
            _ => return None,
        })
    }

    /// The statuses that go into the error file.
    pub(crate) const REPORTED: [RowStatus; 4] = [
        RowStatus::Invalid,
        RowStatus::Repeated,
        RowStatus::Skipped,
        RowStatus::Failed,
    ];
}

/// How many rows of an import are in each state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ImportCounts {
    /// Data rows in the file.
    pub total: u64,
    /// Rows ready to be saved.
    pub valid: u64,
    /// Rows with errors.
    pub invalid: u64,
    /// Rows whose key the app already has.
    pub duplicate: u64,
    /// Rows whose key repeats an earlier row.
    pub repeated: u64,
    /// Rows saved as new records.
    pub created: u64,
    /// Rows that updated a record.
    pub updated: u64,
    /// Duplicates left alone.
    pub skipped: u64,
    /// Rows that failed to save.
    pub failed: u64,
}

impl ImportCounts {
    pub(crate) fn add(&mut self, status: RowStatus, n: u64) {
        let slot = match status {
            RowStatus::Valid => &mut self.valid,
            RowStatus::Invalid => &mut self.invalid,
            RowStatus::Duplicate => &mut self.duplicate,
            RowStatus::Repeated => &mut self.repeated,
            RowStatus::Created => &mut self.created,
            RowStatus::Updated => &mut self.updated,
            RowStatus::Skipped => &mut self.skipped,
            RowStatus::Failed => &mut self.failed,
        };
        *slot += n;
    }

    /// Rows in the error file.
    pub fn reported(&self) -> u64 {
        self.invalid + self.repeated + self.skipped + self.failed
    }
}

/// An import: what `GET /api/imports/{id}` answers.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct ImportRecord {
    /// The import id.
    pub id: Uuid,
    /// The importer (`contacts`).
    pub importer: String,
    /// Where it is.
    pub status: ImportStatus,
    /// The uploaded file's name.
    pub filename: String,
    /// The file's kind, once read.
    pub format: Option<FileKind>,
    /// The text encoding of a CSV file, detected or given.
    pub encoding: Option<String>,
    /// The workbook sheet read (`None`: the first).
    pub sheet: Option<String>,
    /// The file's headers, once read.
    pub headers: Vec<String>,
    /// Which header each field reads.
    pub mapping: Mapping,
    /// Required fields no header matched (status `needs_mapping`).
    pub missing: Vec<String>,
    /// The duplicate mode it was applied with.
    pub duplicates: Option<DuplicateMode>,
    /// Row counts.
    pub counts: ImportCounts,
    /// The operation reading or applying it, to follow at
    /// `GET /api/operations/{id}/events`.
    pub operation_id: Option<Uuid>,
    /// Whether an error file is ready (`GET /api/imports/{id}/errors`).
    pub has_error_file: bool,
    /// Why it failed.
    pub error: Option<String>,
    /// The user who uploaded the file.
    pub owner_id: Option<Uuid>,
    /// The workspace it belongs to.
    pub workspace_id: Option<Uuid>,
    /// When it was uploaded.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
    /// When it was applied.
    pub finished_at: Option<DateTime<Utc>>,
    /// The disk the upload and the error file are on.
    #[serde(skip)]
    pub(crate) disk: String,
    /// The upload's path on the disk.
    #[serde(skip)]
    pub(crate) path: String,
    /// The error file's path on the disk.
    #[serde(skip)]
    pub(crate) error_path: Option<String>,
    /// The locale the upload was made in (staging messages use it).
    #[serde(skip)]
    pub(crate) locale: Option<String>,
    /// Whether `mapping` was chosen (with the upload or later) rather than
    /// found.
    #[serde(skip)]
    pub(crate) mapping_chosen: bool,
    /// Save the chosen mapping for files with these headers once staged.
    #[serde(skip)]
    pub(crate) save_mapping: bool,
}

/// One staged row: what `GET /api/imports/{id}/rows` lists.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct ImportRowRecord {
    /// The row number in the file (the header row is usually 1).
    pub row: u64,
    /// Where it is.
    pub status: RowStatus,
    /// The values read for each field (`{"email": "ada@example.com"}`).
    #[schema(value_type = Object)]
    pub values: BTreeMap<String, Value>,
    /// Messages by field (`{"email": ["email must be a valid email address"]}`);
    /// row-wide ones are under `row`.
    pub errors: BTreeMap<String, Vec<String>>,
}

/// Filters for [`Imports::rows`](crate::Imports::rows).
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct RowQuery {
    /// Only rows in this state.
    pub status: Option<RowStatus>,
    /// The page, from 1 (default 1).
    pub page: Option<u64>,
    /// Rows per page (default 50, at most 500).
    pub per_page: Option<u64>,
}

/// A page of staged rows.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct RowsPage {
    /// The rows.
    pub data: Vec<ImportRowRecord>,
    /// The page's position.
    pub meta: PageMeta,
}

/// Where a page is: `{current_page, per_page, total, last_page}`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct PageMeta {
    /// The page, from 1.
    pub current_page: u64,
    /// Rows per page.
    pub per_page: u64,
    /// Rows in all.
    pub total: u64,
    /// The last page (at least 1).
    pub last_page: u64,
}
