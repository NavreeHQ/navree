//! # navree-sheets
//!
//! `SheetsModule`: CSV and Excel exports and imports.
//!
//! - **Exports.** `#[derive(Exportable)]` turns a struct into columns and
//!   cells ([`Exportable`], [`Column`], [`Cell`]). [`Download::csv`],
//!   [`Download::csv_stream`] and [`Download::xlsx`] answer a handler with
//!   a file; [`Exports`] does the same with the app's `sheets.csv` options
//!   and headers translated in the request's locale. CSV starts with a
//!   UTF-8 byte-order mark (Excel then reads Arabic, accented and CJK text
//!   correctly) and escapes text a spreadsheet would run as a formula
//!   ([`escape_formula`]); XLSX has a bold, frozen and filtered header row,
//!   typed cells and date formats.
//! - **Named exports.** An [`Export`] (registered with `m.export::<E>()`)
//!   fetches its rows page by page: downloaded at once
//!   (`GET /api/exports/{name}`, [`Exports::download`]) or queued as an
//!   operation (`POST /api/exports/{name}`, [`Exports::queue`]) whose job
//!   writes the file to a disk and finishes with a signed link
//!   ([`ExportResult`]).
//! - **Imports.** An [`Import`] (registered with `m.importer::<I>()`)
//!   reads rows into a `#[derive(Deserialize, Validate, Importable)]`
//!   struct. An upload ([`Imports::upload`], `POST /api/imports`) is
//!   staged on the queue: the text encoding is detected (UTF-8, UTF-16,
//!   Windows-1256, Shift_JIS and the rest), the CSV, TSV, XLSX, XLS or ODS
//!   file is read, headers are mapped to columns (chosen, saved for files
//!   with the same headers, or automatic), every row is read and validated
//!   (garde, in the app's validation wording), and rows that repeat a key
//!   or whose key the app already has are found. The staged rows can be
//!   previewed ([`Imports::rows`]); applying ([`Imports::apply`]) saves
//!   them in queue batches, each row in its own unit of work, with
//!   duplicates skipped, updated or refused ([`DuplicateMode`]), and writes
//!   an error file of the rows that were not saved.
//!
//! ```
//! use navree_sheets::{Cell, Download, Exportable};
//!
//! #[derive(Exportable)]
//! struct ContactRow {
//!     #[export(header = "Name")]
//!     name: String,
//!     email: String,
//! }
//!
//! let row = ContactRow { name: "=Ada".into(), email: "ada@example.com".into() };
//! assert_eq!(row.cells()[0], Cell::Text("=Ada".into()));
//! let download = Download::csv("contacts", [row]);
//! assert_eq!(download.filename(), "contacts.csv");
//! ```
#![forbid(unsafe_code)]

extern crate self as navree_sheets;

mod cell;
mod commands;
mod download;
mod error;
mod export;
mod exportable;
mod headers;
mod http;
mod import;
mod migration;
mod module;
mod options;
mod support;
mod write;

pub use cell::{Cell, IntoCell};
pub use download::{CSV_CONTENT_TYPE, Download, XLSX_CONTENT_TYPE};
pub use error::SheetsError;
pub use export::controller::{ExportsController, QueueExportInput};
pub use export::job::QueuedExport;
pub use export::service::Exports;
pub use export::{
    Export, ExportColumnInfo, ExportContext, ExportInfo, ExportPage, ExportRequest, ExportResult,
};
pub use exportable::{Column, Exportable, sentence_case};
pub use headers::{HeaderCatalog, HeaderTranslator};
pub use import::controller::{ApplyInput, ImporterInfo, ImportsController, MappingInput};
pub use import::jobs::{ApplyImportChunk, FinishImport, StageImport};
pub use import::service::{Imports, Upload, UploadOptions};
pub use import::store::{IMPORT_MAPPINGS_TABLE, IMPORT_ROWS_TABLE, IMPORTS_TABLE};
pub use import::{
    DuplicateMode, FileKind, Import, ImportColumn, ImportContext, ImportCounts, ImportRecord,
    ImportRow, ImportRowRecord, ImportStatus, Mapping, PageMeta, RowQuery, RowRejected, RowStatus,
    RowsPage,
};
pub use migration::SHEETS_MIGRATIONS;
pub use module::{ModuleBuilderSheetsExt, SheetsModule};
/// `#[derive(Exportable)]`: see [`Exportable`].
pub use navree_macros::Exportable;
/// `#[derive(Importable)]`: see [`ImportRow`].
pub use navree_macros::Importable;
pub use options::{ExportOptions, ImportOptions, SheetsOptions};
pub use write::SheetFormat;
pub use write::csv::{CsvOptions, UTF8_BOM, escape_formula, write_csv};
pub use write::xlsx::{XLSX_MAX_ROWS, write_xlsx};
