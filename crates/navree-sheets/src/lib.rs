//! # navree-sheets
//!
//! `SheetsModule`: CSV and Excel exports and imports.
#![forbid(unsafe_code)]

extern crate self as navree_sheets;

mod cell;
mod download;
mod error;
mod exportable;
mod headers;
mod import;
mod write;

pub use cell::{Cell, IntoCell};
pub use download::{CSV_CONTENT_TYPE, Download, XLSX_CONTENT_TYPE};
pub use error::SheetsError;
pub use exportable::{Column, Exportable, sentence_case};
pub use headers::{HeaderCatalog, HeaderTranslator};
pub use import::{
    DuplicateMode, FileKind, Import, ImportColumn, ImportContext, ImportCounts, ImportRecord,
    ImportRow, ImportRowRecord, ImportStatus, Mapping, PageMeta, RowQuery, RowRejected, RowStatus,
    RowsPage,
};
/// `#[derive(Exportable)]`: see [`Exportable`].
pub use navree_macros::Exportable;
/// `#[derive(Importable)]`: see [`ImportRow`].
pub use navree_macros::Importable;
pub use write::SheetFormat;
pub use write::csv::{CsvOptions, UTF8_BOM, escape_formula, write_csv};
pub use write::xlsx::{XLSX_MAX_ROWS, write_xlsx};
