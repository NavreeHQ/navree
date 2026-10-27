//! File types: sniffed from the content, the file name only as a fallback
//! for text formats that have no signature.

use crate::images;

/// The type of a file, read from its content: the signatures `infer`
/// knows (images, PDF, Office documents, archives, audio and video), SVG
/// documents, and, for text without a signature, the type the file
/// name's extension gives (`text/html`, `text/csv`, `text/markdown`,
/// `text/plain`). `application/octet-stream` when nothing matches.
pub fn sniff(bytes: &[u8], file_name: Option<&str>) -> String {
    if let Some(kind) = infer::get(bytes) {
        let mime = kind.mime_type();
        // `infer` reads a zip with Office parts as the Office type; plain
        // zips stay `application/zip`.
        return mime.to_owned();
    }
    if images::looks_like_svg(bytes) {
        return "image/svg+xml".to_owned();
    }
    let by_name = file_name
        .and_then(|name| mime_guess::from_path(name).first())
        .map(|mime| mime.essence_str().to_owned());
    if is_text(bytes) {
        return match by_name.as_deref() {
            Some(mime) if mime.starts_with("text/") || mime == "application/json" => {
                mime.to_owned()
            }
            _ if looks_like_html(bytes) => "text/html".to_owned(),
            _ => "text/plain".to_owned(),
        };
    }
    "application/octet-stream".to_owned()
}

/// Whether `mime_type` matches `pattern`: an exact type
/// (`application/pdf`), a family (`image/*`) or `*`.
pub fn matches(pattern: &str, mime_type: &str) -> bool {
    let pattern = pattern.trim().to_ascii_lowercase();
    let mime_type = mime_type.to_ascii_lowercase();
    match pattern.strip_suffix("/*") {
        _ if pattern == "*" || pattern == "*/*" => true,
        Some(family) => mime_type
            .split_once('/')
            .is_some_and(|(kind, _)| kind == family),
        None => pattern == mime_type,
    }
}

/// The usual extension of `mime_type`, without the dot (`bin` when there
/// is none).
pub(crate) fn extension(mime_type: &str) -> &'static str {
    match mime_type {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "image/svg+xml" => "svg",
        "application/pdf" => "pdf",
        "text/plain" => "txt",
        "text/html" => "html",
        "text/csv" => "csv",
        "application/zip" => "zip",
        "video/mp4" => "mp4",
        "audio/mpeg" => "mp3",
        other => mime_guess::get_mime_extensions_str(other)
            .and_then(|extensions| extensions.first().copied())
            .unwrap_or("bin"),
    }
}

/// Valid UTF-8 (allowing a cut at the end) without NUL bytes.
fn is_text(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(8192)];
    if head.contains(&0) {
        return false;
    }
    match std::str::from_utf8(head) {
        Ok(_) => true,
        Err(error) => error.error_len().is_none(),
    }
}

fn looks_like_html(bytes: &[u8]) -> bool {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(1024)]).to_ascii_lowercase();
    let head = head.trim_start_matches('\u{feff}').trim_start();
    head.starts_with("<!doctype html") || head.starts_with("<html") || head.contains("<body")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_wins_over_the_name() {
        let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";
        assert_eq!(sniff(png, Some("cat.jpg")), "image/png");
        assert_eq!(sniff(b"%PDF-1.7\n", Some("x.txt")), "application/pdf");
        assert_eq!(sniff(b"<svg xmlns='http://www.w3.org/2000/svg'/>", None), "image/svg+xml");
    }

    #[test]
    fn text_uses_the_extension() {
        assert_eq!(sniff(b"a,b\n1,2\n", Some("data.csv")), "text/csv");
        assert_eq!(sniff(b"# Notes", Some("notes.md")), "text/markdown");
        assert_eq!(sniff(b"<!DOCTYPE html><p>x</p>", None), "text/html");
        assert_eq!(sniff(b"hello", None), "text/plain");
        assert_eq!(sniff(b"hello", Some("evil.exe")), "text/plain");
        assert_eq!(sniff(&[0, 1, 2, 3], Some("a.txt")), "application/octet-stream");
    }

    #[test]
    fn patterns() {
        assert!(matches("image/*", "image/png"));
        assert!(matches("IMAGE/PNG", "image/png"));
        assert!(matches("*", "video/mp4"));
        assert!(!matches("image/*", "application/pdf"));
        assert!(!matches("image/png", "image/jpeg"));
    }
}
