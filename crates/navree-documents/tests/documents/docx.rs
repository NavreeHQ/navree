//! DOCX: building, reading back, right-to-left documents, images and the
//! response.

use navree_documents::docx::{Align, DocxLimits, Image, Paragraph, Run, Table};
use navree_documents::{DOCX_CONTENT_TYPE, DocumentsError, Docx, DocxFile, Margins, Paper};
use navree_http::IntoResponse;

use crate::support::{assert_golden, fixtures_dir};

fn logo() -> Vec<u8> {
    std::fs::read(fixtures_dir().join("images/logo.png")).unwrap()
}

#[test]
fn a_report_reads_back_block_by_block() {
    let file = Docx::new()
        .heading(1, "Quarterly audit report")
        .heading(2, "Summary")
        .paragraph(
            Paragraph::new()
                .text("Prepared for ")
                .bold("Agra Ltd")
                .text(" by ")
                .italic("the audit team")
                .text("."),
        )
        .bullets(["Every control was tested", "Two findings are open"])
        .numbered(["Rotate the keys", "Restore a backup"])
        .table(
            Table::new()
                .header(["Control", "Status"])
                .row(["Multi-factor sign-in", "Passed"])
                .row(["Backups restored", "Open"]),
        )
        .image(Image::new(logo()).width_mm(30.0).align(Align::Center))
        .page_break()
        .paragraph(Paragraph::from("Appendix").align(Align::End))
        .paragraph(Run::new("Line one\nLine two").size(9.0).color("#555555"))
        .header("Agra Ltd — confidential")
        .footer_page_numbers()
        .build()
        .unwrap();
    assert!(file.bytes().starts_with(b"PK"));
    assert_golden("docx-report.txt", &file.text().unwrap());
    let reread = DocxFile::from_bytes(file.bytes().clone()).unwrap();
    assert_eq!(reread, file);
}

#[test]
fn right_to_left_documents_set_every_paragraph_s_direction() {
    let file = Docx::new()
        .locale("ar")
        .font("Noto Sans Arabic")
        .heading(1, "تقرير التدقيق")
        .paragraph("كل الضوابط ناجحة")
        .table(
            Table::new()
                .header(["الضابط", "الحالة"])
                .row(["المصادقة", "ناجح"]),
        )
        .footer("سري")
        .build()
        .unwrap();
    assert_golden("docx-arabic.txt", &file.text().unwrap());
    // Both body paragraphs and the four cells read right to left, and the
    // table puts its first column on the right.
    let xml = document_xml(file.bytes());
    assert_eq!(xml.matches("<w:bidi />").count(), 6, "{xml}");
    assert!(xml.contains("<w:tblPr><w:bidiVisual />"), "{xml}");
    assert!(
        xml.contains(r#"w:cs="Noto Sans Arabic""#)
            || styles_xml(file.bytes()).contains(r#"w:cs="Noto Sans Arabic""#)
    );
    let ltr = Docx::new().paragraph("Left to right").build().unwrap();
    assert!(!document_xml(ltr.bytes()).contains("<w:bidi />"));
    assert!(!document_xml(ltr.bytes()).contains("bidiVisual"));
}

#[test]
fn the_title_and_author_go_into_the_properties() {
    let file = Docx::new()
        .title("Audit <Q3> & more")
        .author("Navree")
        .paragraph("x")
        .build()
        .unwrap();
    let core = part(file.bytes(), "docProps/core.xml");
    assert!(
        core.contains("<dc:title>Audit &lt;Q3&gt; &amp; more</dc:title>"),
        "{core}"
    );
    assert!(core.contains("<dc:creator>Navree</dc:creator>"), "{core}");
    // Still a valid document.
    DocxFile::from_bytes(file.into_bytes()).unwrap();
}

#[test]
fn paper_orientation_and_margins_are_written() {
    let file = Docx::new()
        .paper(Paper::Letter)
        .landscape()
        .margins(Margins::all(10.0))
        .paragraph("Wide")
        .build()
        .unwrap();
    // The section properties live in word/document.xml.
    let xml = document_xml(file.bytes());
    // 279.4 mm wide (landscape letter) is 15840 twips; 10 mm is 567.
    assert!(xml.contains(r#"w:w="15840""#), "{xml}");
    assert!(xml.contains(r#"w:orient="landscape""#), "{xml}");
    assert!(xml.contains(r#"w:top="567""#), "{xml}");
}

#[test]
fn an_image_that_is_not_png_or_jpeg_is_an_error() {
    let error = Docx::new()
        .image(Image::new(&b"GIF89a"[..]))
        .build()
        .unwrap_err();
    assert!(
        matches!(&error, DocumentsError::Docx(message) if message.contains("PNG")),
        "{error:?}"
    );
}

/// A file saved by an office suite, in either of its Word formats.
fn office_file(flavour: &str, name: &str) -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/docx")
            .join(flavour)
            .join(format!("{name}.docx")),
    )
    .unwrap()
}

#[test]
fn files_saved_by_an_office_suite_read_as_the_text_they_show() {
    // Paragraphs, runs, tabs, breaks, scripts, lists and a page break; a
    // table with merged cells, a table in a cell and cells of several
    // paragraphs; hyperlinks, notes, fields and tracked changes; images and
    // a text box.
    for name in ["basic", "table", "links", "image"] {
        for flavour in ["word", "ooxml"] {
            let file = DocxFile::from_bytes(office_file(flavour, name)).unwrap();
            assert_golden(&format!("docx-office-{name}.txt"), &file.text().unwrap());
        }
    }
}

#[test]
fn what_the_limits_refuse_is_too_large_and_a_broken_file_is_an_invalid_docx() {
    let bytes = office_file("word", "basic");
    let text_limit = DocxLimits {
        max_text_bytes: 100,
        ..DocxLimits::default()
    };
    let file = DocxFile::from_bytes(bytes.clone()).unwrap();
    assert!(matches!(
        file.text_within(&text_limit),
        Err(DocumentsError::TooLarge(message)) if message.contains("100 bytes")
    ));
    assert!(matches!(
        DocxFile::from_bytes_within(bytes.clone(), &text_limit),
        Err(DocumentsError::TooLarge(_))
    ));
    let entries = DocxLimits {
        max_entries: 3,
        ..DocxLimits::default()
    };
    assert!(matches!(
        DocxFile::from_bytes_within(bytes.clone(), &entries),
        Err(DocumentsError::TooLarge(_))
    ));
    // A file cut short is an error; none of the cuts panics.
    for end in (0..bytes.len()).step_by(7) {
        let _ = DocxFile::from_bytes(bytes[..end].to_vec());
    }
    let cut = DocxFile::from_bytes(bytes[..bytes.len() - 100].to_vec()).unwrap_err();
    assert!(matches!(cut, DocumentsError::InvalidDocx(_)), "{cut:?}");
    assert_eq!(cut.status(), http::StatusCode::BAD_REQUEST);
}

#[test]
fn garbage_is_not_a_docx_file() {
    let error = DocxFile::from_bytes(&b"not a zip"[..]).unwrap_err();
    assert!(matches!(error, DocumentsError::InvalidDocx(_)), "{error:?}");
    // The caller's error, like a PDF that cannot be read.
    assert_eq!(error.status(), http::StatusCode::BAD_REQUEST);
    assert!(error.to_string().starts_with("invalid DOCX: "), "{error}");
}

#[test]
fn a_failed_write_is_the_servers_error_and_an_unreadable_file_the_callers() {
    let write = Docx::new()
        .image(Image::new(&b"GIF89a"[..]))
        .build()
        .unwrap_err();
    assert!(matches!(write, DocumentsError::Docx(_)), "{write:?}");
    assert_eq!(write.status(), http::StatusCode::INTERNAL_SERVER_ERROR);
    let read = DocxFile::from_bytes(office_file("word", "basic")[..40].to_vec()).unwrap_err();
    assert!(matches!(read, DocumentsError::InvalidDocx(_)), "{read:?}");
    assert_eq!(read.status(), http::StatusCode::BAD_REQUEST);
}

#[test]
fn a_docx_file_is_a_download() {
    let file = Docx::new().paragraph("x").build().unwrap();
    let response = file.clone().download("Audit Q3.docx").into_response();
    assert_eq!(response.headers()["content-type"], DOCX_CONTENT_TYPE);
    assert_eq!(
        response.headers()["content-disposition"],
        "attachment; filename=\"Audit Q3.docx\""
    );
    let response = file.into_response();
    assert_eq!(
        response.headers()["content-disposition"],
        "attachment; filename=\"document.docx\""
    );
}

/// `word/document.xml` of a DOCX file.
fn document_xml(bytes: &[u8]) -> String {
    part(bytes, "word/document.xml")
}

/// `word/styles.xml` of a DOCX file.
fn styles_xml(bytes: &[u8]) -> String {
    part(bytes, "word/styles.xml")
}

fn part(bytes: &[u8], name: &str) -> String {
    use std::io::Read;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    archive
        .by_name(name)
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    xml
}
