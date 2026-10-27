//! Text extraction from PDF, DOCX, HTML and plain text.

use navree_media::MediaError;
use navree_media::text::{self, DOCX};

use crate::support::{docx, pdf, zip_of};

#[test]
fn pdf_text_comes_page_by_page() {
    let bytes = pdf(&["Phishing awareness", "Report suspicious mail"]);
    let out = text::extract(&bytes, "application/pdf").unwrap();
    assert_eq!(out.pages.len(), 2);
    assert!(
        out.pages[0].contains("Phishing awareness"),
        "{:?}",
        out.pages
    );
    assert!(
        out.pages[1].contains("Report suspicious mail"),
        "{:?}",
        out.pages
    );
    assert!(out.text.contains("Phishing awareness"));
    assert!(out.text.contains("Report suspicious mail"));
    assert_eq!(out.mime_type, "application/pdf");
}

#[test]
fn docx_keeps_paragraphs_tabs_and_breaks() {
    let out = text::extract(&docx(), DOCX).unwrap();
    assert_eq!(
        out.text,
        "Phishing awareness\nReport suspicious\tmail\nat once & always."
    );
    assert_eq!(out.title.as_deref(), Some("Security handbook"));
    assert_eq!(out.pages, vec![out.text.clone()]);
}

#[test]
fn html_skips_scripts_and_styles_and_keeps_blocks() {
    let html = br#"<!doctype html><html><head><title>Portal &amp; news</title>
        <style>p { color: red }</style><script>alert("<p>no</p>")</script></head>
        <body><h1>Quarterly   update</h1><p>Training is <b>due</b> on Friday.</p>
        <ul><li>Report mail</li><li>Lock screens</li></ul>
        <noscript>Enable JavaScript</noscript><p>Questions?<br>Ask&nbsp;us.</p></body></html>"#;
    let out = text::extract(html, "text/html").unwrap();
    assert_eq!(out.title.as_deref(), Some("Portal & news"));
    assert_eq!(
        out.text,
        "Quarterly update\n\nTraining is due on Friday.\n\nReport mail\nLock screens\n\nQuestions?\nAsk\u{a0}us."
    );
}

#[test]
fn plain_text_passes_through() {
    let out = text::extract("\u{feff}line one\r\nline two\n".as_bytes(), "text/plain").unwrap();
    assert_eq!(out.text, "line one\nline two");
    let out = text::extract(b"# Title\n\nBody", "text/markdown").unwrap();
    assert_eq!(out.text, "# Title\n\nBody");
}

#[test]
fn the_type_is_sniffed_when_unknown() {
    let bytes = pdf(&["Sniffed"]);
    let out = text::extract(&bytes, "application/octet-stream").unwrap();
    assert!(out.text.contains("Sniffed"));
    assert!(text::supports("application/pdf"));
    assert!(text::supports(DOCX));
    assert!(text::supports("text/html"));
    assert!(!text::supports("image/png"));
}

#[test]
fn unsupported_and_broken_documents_are_errors() {
    let err = text::extract(b"\x89PNG\r\n\x1a\n", "image/png").unwrap_err();
    assert!(matches!(err, MediaError::Unsupported { .. }), "{err:?}");
    assert_eq!(err.status(), 415);

    let err = text::extract(b"%PDF-1.7 broken", "application/pdf").unwrap_err();
    assert!(matches!(err, MediaError::Text { .. }), "{err:?}");
    assert_eq!(err.code(), "invalid_document");

    let not_word = zip_of(&[("readme.txt", b"no document.xml")]);
    let err = text::extract(&not_word, DOCX).unwrap_err();
    assert!(matches!(err, MediaError::Text { .. }), "{err:?}");
}
