//! [`Download`]: a document as an HTTP response that names its file.

use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use http::header::{
    CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE, HeaderValue,
    X_CONTENT_TYPE_OPTIONS,
};

/// The content type of PDF documents.
pub const PDF_CONTENT_TYPE: &str = "application/pdf";
/// The content type of DOCX documents.
pub const DOCX_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

/// A document as a response: its bytes, its content type and a
/// `Content-Disposition` that names the file, as an attachment (the browser
/// saves it) or inline (the browser shows it).
///
/// Return it from a handler. [`Pdf::download`](crate::Pdf::download),
/// [`Pdf::inline`](crate::Pdf::inline) and
/// [`DocxFile::download`](crate::DocxFile::download) build one.
///
/// The file name is sent twice: as `filename`, with characters outside
/// printable ASCII replaced by `_`, and as `filename*`, UTF-8 and
/// percent-encoded, which browsers prefer, so `شهادة.pdf` keeps its name.
/// Path separators, quotes and control characters are removed. The response
/// carries `Cache-Control: private, no-store` (documents usually hold
/// personal data) and `X-Content-Type-Options: nosniff`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Download {
    bytes: Bytes,
    content_type: String,
    filename: String,
    inline: bool,
}

impl Download {
    /// `bytes` of `content_type`, saved as `filename`.
    pub fn new(
        bytes: impl Into<Bytes>,
        content_type: impl Into<String>,
        filename: impl Into<String>,
    ) -> Self {
        Self {
            bytes: bytes.into(),
            content_type: content_type.into(),
            filename: clean_filename(&filename.into()),
            inline: false,
        }
    }

    /// Shown in the browser instead of saved (`Content-Disposition: inline`),
    /// keeping the file name for when it is saved.
    pub fn inline(mut self) -> Self {
        self.inline = true;
        self
    }

    /// The file name, cleaned.
    pub fn filename(&self) -> &str {
        &self.filename
    }

    /// The content type.
    pub fn content_type(&self) -> &str {
        &self.content_type
    }

    /// The bytes.
    pub fn bytes(&self) -> &Bytes {
        &self.bytes
    }

    /// The `Content-Disposition` header value.
    pub fn disposition(&self) -> String {
        let kind = if self.inline { "inline" } else { "attachment" };
        let ascii: String = self
            .filename
            .chars()
            .map(|c| if c.is_ascii() && !c.is_ascii_control() { c } else { '_' })
            .collect();
        if ascii == self.filename {
            format!("{kind}; filename=\"{ascii}\"")
        } else {
            format!(
                "{kind}; filename=\"{ascii}\"; filename*=UTF-8''{}",
                encode_rfc5987(&self.filename)
            )
        }
    }
}

impl IntoResponse for Download {
    fn into_response(self) -> Response {
        let disposition = self.disposition();
        let content_type = HeaderValue::from_str(&self.content_type)
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
        let length = self.bytes.len();
        let mut response = self.bytes.into_response();
        let headers = response.headers_mut();
        headers.insert(CONTENT_TYPE, content_type);
        headers.insert(CONTENT_LENGTH, HeaderValue::from(length));
        if let Ok(value) = HeaderValue::from_str(&disposition) {
            headers.insert(CONTENT_DISPOSITION, value);
        }
        headers.insert(CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
        headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
        response
    }
}

/// `name` without path separators, quotes, control characters or leading
/// dots; `document` when nothing is left.
fn clean_filename(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let cleaned: String = base
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '"' | '\\' | '/' | ';'))
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.').trim().to_owned();
    if cleaned.is_empty() {
        "document".to_owned()
    } else {
        cleaned
    }
}

/// `value` percent-encoded for `filename*` (RFC 5987 `attr-char`).
fn encode_rfc5987(value: &str) -> String {
    let mut out = String::with_capacity(value.len() * 3);
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'!'
            | b'#'
            | b'$'
            | b'&'
            | b'+'
            | b'-'
            | b'.'
            | b'^'
            | b'_'
            | b'`'
            | b'|'
            | b'~' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_names_are_sent_once() {
        let download = Download::new(&b"%PDF"[..], PDF_CONTENT_TYPE, "invoice-42.pdf");
        assert_eq!(download.disposition(), "attachment; filename=\"invoice-42.pdf\"");
        assert_eq!(
            download.clone().inline().disposition(),
            "inline; filename=\"invoice-42.pdf\""
        );
    }

    #[test]
    fn other_names_get_an_ascii_fallback_and_a_utf8_form() {
        let download = Download::new(&b""[..], PDF_CONTENT_TYPE, "شهادة 1.pdf");
        assert_eq!(
            download.disposition(),
            "attachment; filename=\"_____ 1.pdf\"; filename*=UTF-8''%D8%B4%D9%87%D8%A7%D8%AF%D8%A9%201.pdf"
        );
    }

    #[test]
    fn names_cannot_carry_paths_quotes_or_headers() {
        assert_eq!(clean_filename("../../etc/passwd"), "passwd");
        assert_eq!(clean_filename("a\"b;c\r\n.pdf"), "abc.pdf");
        assert_eq!(clean_filename("C:\\reports\\q1.pdf"), "q1.pdf");
        assert_eq!(clean_filename(".hidden"), "hidden");
        assert_eq!(clean_filename("  "), "document");
    }

    #[test]
    fn the_response_has_the_headers() {
        let response = Download::new(&b"%PDF-1.7"[..], PDF_CONTENT_TYPE, "a.pdf").into_response();
        let headers = response.headers();
        assert_eq!(headers[CONTENT_TYPE], "application/pdf");
        assert_eq!(headers[CONTENT_LENGTH], "8");
        assert_eq!(headers[CONTENT_DISPOSITION], "attachment; filename=\"a.pdf\"");
        assert_eq!(headers[CACHE_CONTROL], "private, no-store");
        assert_eq!(headers[X_CONTENT_TYPE_OPTIONS], "nosniff");
    }
}
