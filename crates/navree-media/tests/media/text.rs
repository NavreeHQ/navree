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

#[test]
fn pdf_text_reads_right_to_left_scripts_in_reading_order() {
    // The native engine's certificate in Arabic, Hebrew and Hindi.
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../navree-pdf/tests/fixtures/multilingual.pdf"
    ))
    .unwrap();
    let out = text::extract(&bytes, "application/pdf").unwrap();
    assert!(out.text.contains("شهادة إتمام"), "{}", out.text);
    assert!(out.text.contains("תעודת סיום"), "{}", out.text);
    assert!(out.text.contains("प्रमाण पत्र"), "{}", out.text);
    assert_eq!(out.title.as_deref(), Some("شهادات إتمام"));
}

/// A PDF whose one font's `ToUnicode` map has `ranges` codespace ranges.
fn pdf_with_codespace_ranges(ranges: usize) -> Vec<u8> {
    use navree_pdf::{Objects, Stream, dictionary};

    let mut cmap = format!(
        "/CIDInit /ProcSet findresource begin 12 dict begin begincmap
{ranges} begincodespacerange
"
    );
    cmap.push_str(
        &"<00> <FF>
"
        .repeat(ranges),
    );
    cmap.push_str(
        "endcodespacerange
1 beginbfchar
<41> <0041>
endbfchar
endcmap
",
    );
    let mut doc = Objects::new();
    let pages_id = doc.reserve();
    let map = doc.add(Stream::new(dictionary! {}, cmap.into_bytes()));
    let font = doc.add(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica", "ToUnicode" => map,
    });
    let content = doc.add(Stream::new(
        dictionary! {},
        b"BT /F1 12 Tf 72 720 Td (A) Tj ET".to_vec(),
    ));
    let page = doc.add(dictionary! {
        "Type" => "Page", "Parent" => pages_id, "Contents" => content,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
    });
    doc.insert(
        pages_id,
        dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 },
    );
    let catalog = doc.add(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    doc.save().unwrap()
}

#[test]
fn pdf_text_is_held_to_the_caps_on_to_unicode_maps() {
    let out = text::extract(&pdf_with_codespace_ranges(64), "application/pdf").unwrap();
    assert_eq!(out.text, "A");
    // Every code is checked against each range: more than 64 are refused.
    let err = text::extract(&pdf_with_codespace_ranges(65), "application/pdf").unwrap_err();
    assert!(matches!(err, MediaError::Text { .. }), "{err:?}");
    assert!(err.to_string().contains("codespace"), "{err}");
}

/// A PDF whose one font's `Differences` give code 65 the name `name`, by
/// reference.
fn pdf_with_glyph_name(name: &str) -> Vec<u8> {
    use navree_pdf::{Object, Objects, Stream, dictionary};

    let mut doc = Objects::new();
    let pages_id = doc.reserve();
    let glyph = doc.add(Object::name(name));
    let font = doc.add(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        "Encoding" => dictionary! { "Differences" => vec![65.into(), glyph.into()] },
    });
    let content = doc.add(Stream::new(
        dictionary! {},
        b"BT /F1 12 Tf 72 720 Td (A) Tj ET".to_vec(),
    ));
    let page = doc.add(dictionary! {
        "Type" => "Page", "Parent" => pages_id, "Contents" => content,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
    });
    doc.insert(
        pages_id,
        dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 },
    );
    let catalog = doc.add(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    doc.save().unwrap()
}

#[test]
fn pdf_text_reads_a_glyph_name_no_longer_than_a_name_may_be() {
    let name = format!("uni{}", "0042".repeat(31));
    let out = text::extract(&pdf_with_glyph_name(&name), "application/pdf").unwrap();
    assert_eq!(out.text, "B".repeat(31));
    // 16 MiB of text from each code of each font, were it read.
    let name = format!("uni{}", "0800".repeat(1024 * 1024));
    let out = text::extract(&pdf_with_glyph_name(&name), "application/pdf").unwrap();
    assert_eq!(out.text, "");
}
