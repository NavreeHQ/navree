//! Named exports: [`Export`] (a row type, its parameters and how its rows
//! are fetched page by page), run at once as a [`Download`](crate::Download)
//! or queued as an operation that writes a file to a disk and hands back a
//! signed link ([`ExportResult`]).

pub(crate) mod controller;
pub(crate) mod job;
pub(crate) mod registry;
pub(crate) mod run;
pub(crate) mod service;

use std::fmt;
use std::future::Future;

use chrono::{DateTime, Utc};
use navree_contracts::BoxError;
use navree_kernel::AppContext;
use navree_openapi::ToSchema;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Exportable, SheetFormat};

/// Who an export runs for, given to every [`Export`] method.
#[derive(Clone)]
pub struct ExportContext {
    /// The app, to resolve services.
    pub app: AppContext,
    /// The user who asked for it.
    pub user_id: Option<Uuid>,
    /// The workspace it runs in.
    pub workspace_id: Option<Uuid>,
    /// The locale it runs in (`ar`), with navree-i18n.
    pub locale: Option<String>,
}

impl fmt::Debug for ExportContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExportContext")
            .field("user_id", &self.user_id)
            .field("workspace_id", &self.workspace_id)
            .field("locale", &self.locale)
            .finish_non_exhaustive()
    }
}

/// The rows a page asks for: `limit` rows after the first `offset`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ExportPage {
    /// Rows to skip.
    pub offset: u64,
    /// Rows to return (`sheets.exports.page_size`); fewer ends the export.
    pub limit: u64,
}

/// A named export: its row type, its parameters and how its rows are
/// fetched. Register it with `m.export::<E>()`
/// ([`ModuleBuilderSheetsExt`](crate::ModuleBuilderSheetsExt)); it is
/// resolved from the app like any provider, so it can hold services.
///
/// ```
/// use navree_contracts::BoxError;
/// use navree_macros::injectable;
/// use navree_sheets::{Export, ExportContext, ExportPage, Exportable};
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Exportable)]
/// #[export(sheet = "Contacts")]
/// struct ContactRow {
///     name: String,
///     email: String,
/// }
///
/// #[derive(Default, Serialize, Deserialize)]
/// struct ContactFilter {
///     company: Option<String>,
/// }
///
/// #[injectable]
/// struct ContactsExport;
///
/// impl Export for ContactsExport {
///     const NAME: &'static str = "contacts";
///     type Row = ContactRow;
///     type Params = ContactFilter;
///
///     async fn rows(
///         &self,
///         _ctx: &ExportContext,
///         _filter: &ContactFilter,
///         page: ExportPage,
///     ) -> Result<Vec<ContactRow>, BoxError> {
///         // SELECT ... ORDER BY id LIMIT page.limit OFFSET page.offset
///         let _ = page;
///         Ok(Vec::new())
///     }
/// }
/// ```
pub trait Export: Send + Sync + 'static {
    /// The export's name: the `{name}` of its routes (`contacts`). Letters,
    /// digits, `-`, `_` and `.`.
    const NAME: &'static str;

    /// What one row is.
    type Row: Exportable + Send + 'static;

    /// The parameters (filters, a date range): read from the query string
    /// of `GET /api/exports/{name}` and from `params` in the body of
    /// `POST /api/exports/{name}`. Use `()` for none.
    type Params: Serialize + DeserializeOwned + Send + Sync + 'static;

    /// The permission a user needs to run it (default `{NAME}.export`,
    /// checked app-wide or in the request's workspace); `None` lets every
    /// signed-in user run it.
    fn permission() -> Option<String> {
        Some(format!("{}.export", Self::NAME))
    }

    /// The file name, without the extension (default the export's name).
    fn filename(&self, params: &Self::Params) -> String {
        let _ = params;
        Self::NAME.to_owned()
    }

    /// How many rows the export has, when it is cheap to know: the
    /// progress of a queued export, and a `400 too_many_rows` before a
    /// download starts when it is above `sheets.exports.max_rows`.
    fn count(
        &self,
        ctx: &ExportContext,
        params: &Self::Params,
    ) -> impl Future<Output = Result<Option<u64>, BoxError>> + Send {
        let _ = (ctx, params);
        async { Ok(None) }
    }

    /// One page of rows, in a stable order. A page shorter than
    /// `page.limit` is the last.
    fn rows(
        &self,
        ctx: &ExportContext,
        params: &Self::Params,
        page: ExportPage,
    ) -> impl Future<Output = Result<Vec<Self::Row>, BoxError>> + Send;
}

/// Who runs an export, and in which format.
///
/// ```
/// use navree_sheets::{ExportRequest, SheetFormat};
///
/// let request = ExportRequest::new(SheetFormat::Xlsx).user(uuid::Uuid::nil());
/// assert_eq!(request.format, SheetFormat::Xlsx);
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExportRequest {
    /// CSV or XLSX.
    pub format: SheetFormat,
    /// The user it runs for (the owner of a queued export's operation).
    pub user_id: Option<Uuid>,
    /// The workspace it runs in.
    pub workspace_id: Option<Uuid>,
}

impl ExportRequest {
    /// An export in `format`, for nobody in particular.
    pub fn new(format: SheetFormat) -> Self {
        ExportRequest {
            format,
            user_id: None,
            workspace_id: None,
        }
    }

    /// For user `id`.
    pub fn user(mut self, id: Uuid) -> Self {
        self.user_id = Some(id);
        self
    }

    /// In workspace `id`.
    pub fn workspace(mut self, id: Uuid) -> Self {
        self.workspace_id = Some(id);
        self
    }
}

/// What a queued export's operation finishes with: its `result`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ExportResult {
    /// The signed download link (relative for local disks).
    pub url: String,
    /// The file name.
    pub filename: String,
    /// CSV or XLSX.
    pub format: SheetFormat,
    /// Data rows written.
    pub rows: u64,
    /// The file's size in bytes.
    pub bytes: u64,
    /// When the link stops working.
    pub expires_at: DateTime<Utc>,
    /// The disk the file is on.
    pub disk: String,
    /// Its path on the disk (to attach it to a mail, or copy it).
    pub path: String,
}

/// An export the user may run: what `GET /api/exports` lists.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct ExportInfo {
    /// The export's name.
    pub name: String,
    /// Its columns, with their headers in the request's locale.
    pub columns: Vec<ExportColumnInfo>,
}

/// One column of an [`ExportInfo`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct ExportColumnInfo {
    /// The field (`first_name`).
    pub key: String,
    /// The header.
    pub header: String,
}
