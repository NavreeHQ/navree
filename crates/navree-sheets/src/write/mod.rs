//! Writing CSV and XLSX files.

pub(crate) mod csv;
pub(crate) mod xlsx;

use navree_openapi::ToSchema;
use serde::{Deserialize, Serialize};

use crate::download::{CSV_CONTENT_TYPE, XLSX_CONTENT_TYPE};

/// A spreadsheet file format.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum SheetFormat {
    /// Comma-separated values (UTF-8).
    #[default]
    Csv,
    /// An Excel workbook (Office Open XML).
    Xlsx,
}

impl SheetFormat {
    /// The file extension, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            SheetFormat::Csv => "csv",
            SheetFormat::Xlsx => "xlsx",
        }
    }

    /// The content type of the format's files.
    pub fn content_type(self) -> &'static str {
        match self {
            SheetFormat::Csv => CSV_CONTENT_TYPE,
            SheetFormat::Xlsx => XLSX_CONTENT_TYPE,
        }
    }

    /// `csv` or `xlsx` (case-insensitive), else `None`.
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "csv" => Some(SheetFormat::Csv),
            "xlsx" => Some(SheetFormat::Xlsx),
            _ => None,
        }
    }
}
