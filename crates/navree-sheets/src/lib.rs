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
mod write;

pub use cell::{Cell, IntoCell};
pub use download::{CSV_CONTENT_TYPE, Download, XLSX_CONTENT_TYPE};
pub use error::SheetsError;
pub use exportable::{Column, Exportable, sentence_case};
pub use headers::{HeaderCatalog, HeaderTranslator};
pub use write::SheetFormat;
pub use write::csv::{CsvOptions, UTF8_BOM, escape_formula, write_csv};
pub use write::xlsx::{XLSX_MAX_ROWS, write_xlsx};
