//! [`SheetsOptions`]: the `sheets` configuration section.

use navree_config::ConfigSection;
use serde::{Deserialize, Serialize};

use crate::CsvOptions;

/// Options of the sheets module (configuration section `sheets`).
///
/// ```toml
/// [sheets.csv]
/// bom = true                     # a UTF-8 byte-order mark, so Excel reads UTF-8
/// delimiter = ","                # ";" for spreadsheets set to a European locale
/// escape_formulas = true         # a leading ' on text starting with = + - @
/// crlf = true                    # RFC 4180 line endings
///
/// [sheets.exports]
/// disk = ""                      # the disk of queued exports ("": the default disk)
/// directory = "exports"          # where on the disk
/// link_ttl_secs = 86400          # how long the download link of a queued export works
/// page_size = 1000               # rows asked of an export per page
/// max_rows = 1000000             # the most rows one export writes
/// routes = true                  # the /api/exports routes
///
/// [sheets.imports]
/// disk = ""                      # the disk of uploads and error files ("": the default disk)
/// directory = "imports"          # where on the disk
/// max_upload_bytes = 20971520    # the largest upload (20 MiB)
/// max_rows = 100000              # the most data rows one file may have
/// max_columns = 16384            # the most columns one file may have
/// max_cells = 10000000           # the most cells (rows times columns) one file may hold
/// chunk_size = 500               # rows per staging step and per apply job
/// keep_days = 7                  # sheets:prune deletes imports older than this
/// routes = true                  # the /api/imports routes
/// ```
///
/// Every key also comes from the environment:
/// `APP_SHEETS__IMPORTS__MAX_ROWS=250000`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SheetsOptions {
    /// How CSV files are written.
    pub csv: CsvOptions,
    /// Exports.
    pub exports: ExportOptions,
    /// Imports.
    pub imports: ImportOptions,
}

/// Exports (`sheets.exports`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExportOptions {
    /// The disk queued exports are written to (default `""`: the storage
    /// module's default disk).
    pub disk: String,
    /// The directory on the disk (default `exports`).
    pub directory: String,
    /// How long the signed download link of a queued export works, in
    /// seconds (default 86,400: a day).
    pub link_ttl_secs: u64,
    /// How many rows an [`Export`](crate::Export) is asked for at a time
    /// (default 1,000).
    pub page_size: u64,
    /// The most rows one export writes (default 1,000,000; an XLSX sheet
    /// holds at most 1,048,575).
    pub max_rows: u64,
    /// Mount the `/api/exports` routes (default true).
    pub routes: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        ExportOptions {
            disk: String::new(),
            directory: "exports".to_owned(),
            link_ttl_secs: 86_400,
            page_size: 1_000,
            max_rows: 1_000_000,
            routes: true,
        }
    }
}

/// Imports (`sheets.imports`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ImportOptions {
    /// The disk uploads and error files are kept on (default `""`: the
    /// storage module's default disk).
    pub disk: String,
    /// The directory on the disk (default `imports`).
    pub directory: String,
    /// The largest file accepted, in bytes (default 20 MiB).
    pub max_upload_bytes: u64,
    /// The most data rows a file may have (default 100,000).
    pub max_rows: u64,
    /// The most columns a file may have (default 16,384, the width of an
    /// Excel sheet).
    pub max_columns: u64,
    /// The most cells a file may hold, counting every row at the width of
    /// the header (default 10,000,000: 100,000 rows of 100 columns).
    pub max_cells: u64,
    /// Rows checked per staging step and saved per apply job (default 500).
    pub chunk_size: u64,
    /// `sheets:prune` deletes imports (their rows, upload and error file)
    /// last changed more than this many days ago (default 7).
    pub keep_days: u64,
    /// Mount the `/api/imports` routes (default true).
    pub routes: bool,
}

impl Default for ImportOptions {
    fn default() -> Self {
        ImportOptions {
            disk: String::new(),
            directory: "imports".to_owned(),
            max_upload_bytes: 20 * 1024 * 1024,
            max_rows: 100_000,
            max_columns: 16_384,
            max_cells: 10_000_000,
            chunk_size: 500,
            keep_days: 7,
            routes: true,
        }
    }
}

impl ConfigSection for SheetsOptions {
    const KEY: &'static str = "sheets";

    fn validate(&self) -> Result<(), String> {
        self.csv.validate()?;
        if self.exports.page_size == 0 {
            return Err("`exports.page_size` must be at least 1".to_owned());
        }
        if self.exports.max_rows == 0 {
            return Err("`exports.max_rows` must be at least 1".to_owned());
        }
        if self.exports.link_ttl_secs == 0 {
            return Err("`exports.link_ttl_secs` must be at least 1".to_owned());
        }
        if self.imports.max_upload_bytes == 0 {
            return Err("`imports.max_upload_bytes` must be at least 1".to_owned());
        }
        if self.imports.max_rows == 0 {
            return Err("`imports.max_rows` must be at least 1".to_owned());
        }
        if self.imports.max_columns == 0 {
            return Err("`imports.max_columns` must be at least 1".to_owned());
        }
        if self.imports.max_cells == 0 {
            return Err("`imports.max_cells` must be at least 1".to_owned());
        }
        if self.imports.chunk_size == 0 {
            return Err("`imports.chunk_size` must be at least 1".to_owned());
        }
        for (key, directory) in [
            ("exports.directory", &self.exports.directory),
            ("imports.directory", &self.imports.directory),
        ] {
            if directory.trim_matches(['/', '\\', ' ']).is_empty() {
                return Err(format!("`{key}` must not be empty"));
            }
        }
        Ok(())
    }
}

impl SheetsOptions {
    /// `directory` without slashes at either end.
    pub(crate) fn clean(directory: &str) -> &str {
        directory.trim_matches(['/', '\\', ' '])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_valid() {
        assert_eq!(SheetsOptions::default().validate(), Ok(()));
    }

    #[test]
    fn bad_values_name_their_key() {
        let mut options = SheetsOptions::default();
        options.imports.chunk_size = 0;
        assert!(
            options
                .validate()
                .unwrap_err()
                .contains("imports.chunk_size")
        );
        let mut options = SheetsOptions::default();
        options.exports.directory = "/".into();
        assert!(
            options
                .validate()
                .unwrap_err()
                .contains("exports.directory")
        );
        let mut options = SheetsOptions::default();
        options.csv.delimiter = '"';
        assert!(
            options
                .validate()
                .unwrap_err()
                .contains("sheets.csv.delimiter")
        );
    }

    #[test]
    fn the_import_size_limits_are_at_least_one() {
        for key in ["max_columns", "max_cells"] {
            let mut options = SheetsOptions::default();
            match key {
                "max_columns" => options.imports.max_columns = 0,
                _ => options.imports.max_cells = 0,
            }
            let message = options.validate().unwrap_err();
            assert!(message.contains(&format!("imports.{key}")), "{message}");
        }
    }

    #[test]
    fn a_partial_section_keeps_the_other_defaults() {
        let options: SheetsOptions = serde_json::from_value(serde_json::json!({
            "csv": { "delimiter": ";" },
            "imports": { "max_rows": 10 }
        }))
        .unwrap();
        assert_eq!(options.csv.delimiter, ';');
        assert!(options.csv.bom);
        assert_eq!(options.imports.max_rows, 10);
        assert_eq!(options.imports.chunk_size, 500);
        assert_eq!(options.exports, ExportOptions::default());
    }
}
