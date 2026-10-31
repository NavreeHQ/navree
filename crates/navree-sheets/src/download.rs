//! [`Download`]: a file response, CSV streamed row by row.

use std::fmt;

use bytes::Bytes;
use futures_util::stream::{self, BoxStream, Stream, StreamExt};
use http::header::{
    CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE, HeaderValue,
    X_CONTENT_TYPE_OPTIONS,
};
use navree_contracts::BoxError;
use navree_http::body::Body;
use navree_http::response::{IntoResponse, Response};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

use crate::headers::resolve_headers;
use crate::write::csv::CsvEncoder;
use crate::write::xlsx::XlsxBuilder;
use crate::{CsvOptions, Exportable, SheetFormat, SheetsError};

/// The content type of CSV downloads.
pub const CSV_CONTENT_TYPE: &str = "text/csv; charset=utf-8";

/// The content type of XLSX downloads.
pub const XLSX_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";

/// How many rows of a stream are encoded into one chunk of the response.
const ROWS_PER_CHUNK: usize = 256;

enum DownloadBody {
    Bytes(Bytes),
    Stream(BoxStream<'static, Result<Bytes, BoxError>>),
}

/// A file download: `200` with `Content-Disposition: attachment`, the
/// file's content type, `Cache-Control: no-store` and
/// `X-Content-Type-Options: nosniff`. Return it from a handler.
///
/// - [`Download::csv`] and [`Download::xlsx`] write rows you have.
/// - [`Download::csv_stream`] writes rows as a stream yields them: the
///   response starts with the header row and never holds the whole file.
/// - [`Exports`](crate::Exports) does the same with the app's
///   `sheets.csv` options and translated headers, and streams an
///   [`Export`](crate::Export) page by page.
///
/// ```
/// use navree_sheets::{Download, Exportable};
///
/// #[derive(Exportable)]
/// struct ContactRow {
///     name: String,
///     email: String,
/// }
///
/// async fn export() -> Download {
///     let rows = vec![ContactRow { name: "Ada".into(), email: "ada@example.com".into() }];
///     Download::csv("contacts.csv", rows)
/// }
/// # let _ = navree_http::RouteDef::on(http::Method::GET, "/export", export);
/// ```
pub struct Download {
    filename: String,
    content_type: &'static str,
    body: DownloadBody,
}

impl fmt::Debug for Download {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Download")
            .field("filename", &self.filename)
            .field("content_type", &self.content_type)
            .finish_non_exhaustive()
    }
}

impl Download {
    /// A download of `bytes` as `filename`, of type `content_type`.
    pub fn bytes(
        filename: impl Into<String>,
        content_type: &'static str,
        bytes: impl Into<Bytes>,
    ) -> Self {
        Download {
            filename: filename.into(),
            content_type,
            body: DownloadBody::Bytes(bytes.into()),
        }
    }

    /// A download streamed from `chunks`.
    pub fn stream<S, E>(filename: impl Into<String>, content_type: &'static str, chunks: S) -> Self
    where
        S: Stream<Item = Result<Bytes, E>> + Send + 'static,
        E: Into<BoxError>,
    {
        Download {
            filename: filename.into(),
            content_type,
            body: DownloadBody::Stream(chunks.map(|chunk| chunk.map_err(Into::into)).boxed()),
        }
    }

    /// `rows` as a CSV file named `filename` (`.csv` is added when it has
    /// no extension), with the default [`CsvOptions`]: a byte-order mark,
    /// commas, CRLF and formula escaping. Headers are written as declared.
    pub fn csv<T: Exportable>(
        filename: impl Into<String>,
        rows: impl IntoIterator<Item = T>,
    ) -> Self {
        csv_download(
            filename.into(),
            &CsvOptions::default(),
            &plain_headers::<T>(),
            rows,
        )
    }

    /// The rows `stream` yields as a CSV file, streamed: the header row
    /// goes out at once, then the rows in chunks as they arrive. An error
    /// from the stream ends the response early (the client sees a cut-off
    /// download) and is logged.
    pub fn csv_stream<T, S, E>(filename: impl Into<String>, rows: S) -> Self
    where
        T: Exportable + Send + 'static,
        S: Stream<Item = Result<T, E>> + Send + 'static,
        E: Into<BoxError> + Send + 'static,
    {
        csv_stream_download(
            filename.into(),
            CsvOptions::default(),
            plain_headers::<T>(),
            rows,
        )
    }

    /// `rows` as an XLSX file named `filename` (`.xlsx` is added when it
    /// has no extension): one sheet named after
    /// [`Exportable::sheet_name`], a bold, frozen and filtered header row,
    /// typed cells. The file is built before the response starts.
    pub fn xlsx<T: Exportable>(
        filename: impl Into<String>,
        rows: impl IntoIterator<Item = T>,
    ) -> Result<Self, SheetsError> {
        xlsx_download(filename.into(), &plain_headers::<T>(), rows)
    }

    /// The file name the browser saves.
    pub fn filename(&self) -> &str {
        &self.filename
    }

    /// The content type.
    pub fn content_type(&self) -> &'static str {
        self.content_type
    }
}

fn plain_headers<T: Exportable>() -> Vec<String> {
    resolve_headers(&T::columns(), None, None)
}

/// `filename` with `.{extension}` when it has none.
pub(crate) fn with_extension(filename: &str, format: SheetFormat) -> String {
    let name = filename.trim();
    let name = if name.is_empty() { "export" } else { name };
    if name.rsplit_once('.').is_some_and(|(stem, ext)| {
        !stem.is_empty() && !ext.is_empty() && ext.chars().all(|c| c.is_ascii_alphanumeric())
    }) {
        name.to_owned()
    } else {
        format!("{name}.{}", format.extension())
    }
}

pub(crate) fn csv_download<T: Exportable>(
    filename: String,
    options: &CsvOptions,
    headers: &[String],
    rows: impl IntoIterator<Item = T>,
) -> Download {
    let filename = with_extension(&filename, SheetFormat::Csv);
    let encoded = CsvEncoder::new(options.clone()).and_then(|encoder| {
        let mut out = encoder.header(headers)?.to_vec();
        let cells: Vec<_> = rows.into_iter().map(|row| row.cells()).collect();
        out.extend_from_slice(&encoder.rows(&cells)?);
        Ok(out)
    });
    match encoded {
        Ok(bytes) => Download::bytes(filename, CSV_CONTENT_TYPE, bytes),
        Err(error) => Download::stream(
            filename,
            CSV_CONTENT_TYPE,
            stream::once(async move { Err::<Bytes, SheetsError>(error) }),
        ),
    }
}

pub(crate) fn csv_stream_download<T, S, E>(
    filename: String,
    options: CsvOptions,
    headers: Vec<String>,
    rows: S,
) -> Download
where
    T: Exportable + Send + 'static,
    S: Stream<Item = Result<T, E>> + Send + 'static,
    E: Into<BoxError> + Send + 'static,
{
    let filename = with_extension(&filename, SheetFormat::Csv);
    let encoder = match CsvEncoder::new(options) {
        Ok(encoder) => std::sync::Arc::new(encoder),
        Err(error) => {
            return Download::stream(
                filename,
                CSV_CONTENT_TYPE,
                stream::once(async move { Err::<Bytes, SheetsError>(error) }),
            );
        }
    };
    let head = encoder.header(&headers).map_err(BoxError::from);
    let body = rows
        .ready_chunks(ROWS_PER_CHUNK)
        .map(move |chunk| {
            let mut cells = Vec::with_capacity(chunk.len());
            for row in chunk {
                match row {
                    Ok(row) => cells.push(row.cells()),
                    Err(error) => {
                        let error: BoxError = error.into();
                        tracing::warn!(target: "navree::sheets", %error, "a streamed CSV download stopped");
                        // The rows before the failing one still go out.
                        return encoder
                            .rows(&cells)
                            .map_err(BoxError::from)
                            .and_then(|_| Err(error));
                    }
                }
            }
            encoder.rows(&cells).map_err(BoxError::from)
        })
        .scan(false, |failed, chunk| {
            // Stop after the first error.
            if *failed {
                return std::future::ready(None);
            }
            *failed = chunk.is_err();
            std::future::ready(Some(chunk))
        });
    Download::stream(
        filename,
        CSV_CONTENT_TYPE,
        stream::once(async move { head }).chain(body),
    )
}

pub(crate) fn xlsx_download<T: Exportable>(
    filename: String,
    headers: &[String],
    rows: impl IntoIterator<Item = T>,
) -> Result<Download, SheetsError> {
    let filename = with_extension(&filename, SheetFormat::Xlsx);
    let columns = T::columns();
    let mut builder = XlsxBuilder::new(T::sheet_name(), &columns, headers, false)?;
    let mut page = Vec::with_capacity(ROWS_PER_CHUNK);
    for row in rows {
        page.push(row.cells());
        if page.len() == ROWS_PER_CHUNK {
            builder.append(&page)?;
            page.clear();
        }
    }
    builder.append(&page)?;
    Ok(Download::bytes(
        filename,
        XLSX_CONTENT_TYPE,
        builder.finish()?,
    ))
}

/// Characters percent-encoded in `filename*` (RFC 8187 `attr-char` kept).
const ATTR_CHAR: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'!')
    .remove(b'#')
    .remove(b'$')
    .remove(b'&')
    .remove(b'+')
    .remove(b'-')
    .remove(b'.')
    .remove(b'^')
    .remove(b'_')
    .remove(b'`')
    .remove(b'|')
    .remove(b'~');

/// `attachment; filename="..."; filename*=UTF-8''...`: an ASCII fallback
/// and the exact name. Path separators, quotes and control characters
/// are dropped.
pub(crate) fn content_disposition(filename: &str) -> HeaderValue {
    let clean: String = filename
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '/' | '\\' | '"'))
        .collect();
    let clean = if clean.trim().is_empty() {
        "download".to_owned()
    } else {
        clean.trim().to_owned()
    };
    let ascii: String = clean
        .chars()
        .map(|c| {
            if c.is_ascii() && !c.is_ascii_control() && c != ';' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let encoded = utf8_percent_encode(&clean, ATTR_CHAR);
    HeaderValue::from_str(&format!(
        "attachment; filename=\"{ascii}\"; filename*=UTF-8''{encoded}"
    ))
    .unwrap_or_else(|_| HeaderValue::from_static("attachment"))
}

impl IntoResponse for Download {
    fn into_response(self) -> Response {
        let disposition = content_disposition(&self.filename);
        let (body, length) = match self.body {
            DownloadBody::Bytes(bytes) => {
                let length = bytes.len();
                (Body::from(bytes), Some(length))
            }
            DownloadBody::Stream(stream) => (Body::from_stream(stream), None),
        };
        let mut response = Response::new(body);
        let headers = response.headers_mut();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static(self.content_type));
        headers.insert(CONTENT_DISPOSITION, disposition);
        headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
        headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
        if let Some(length) = length {
            headers.insert(CONTENT_LENGTH, HeaderValue::from(length));
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filenames_get_their_extension() {
        assert_eq!(with_extension("contacts", SheetFormat::Csv), "contacts.csv");
        assert_eq!(
            with_extension("contacts.csv", SheetFormat::Csv),
            "contacts.csv"
        );
        assert_eq!(with_extension("q1.report", SheetFormat::Xlsx), "q1.report");
        assert_eq!(with_extension("", SheetFormat::Xlsx), "export.xlsx");
        assert_eq!(with_extension(".hidden", SheetFormat::Csv), ".hidden.csv");
    }

    #[test]
    fn the_disposition_has_an_ascii_fallback_and_the_exact_name() {
        let value = content_disposition("جهات الاتصال.csv");
        let value = value.to_str().unwrap();
        assert!(value.starts_with("attachment; filename=\""), "{value}");
        assert!(value.contains("filename*=UTF-8''%D8%AC"), "{value}");
        let value = content_disposition("../a\"b\r\n.csv");
        assert_eq!(
            value.to_str().unwrap(),
            "attachment; filename=\"..ab.csv\"; filename*=UTF-8''..ab.csv"
        );
    }
}
