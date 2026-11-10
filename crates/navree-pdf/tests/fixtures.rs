//! Real files: three made by other writers (the native engine's invoice and
//! certificate in Arabic, Hebrew and Hindi; a LibreOffice table), and one of
//! Navree's own plain documents rewritten by qpdf in every way a file can be
//! laid out: object streams, cross-reference streams, linearized, QDF,
//! incremental updates, a broken or shifted cross-reference, cut short, and
//! encrypted with RC4 and AES of 40 to 256 bits.
//!
//! The pages and titles are what poppler's `pdfinfo` reads; the texts are
//! recorded next to each file.

use std::path::PathBuf;

use navree_pdf::text::{decode_text_string, page_texts};
use navree_pdf::{Document, Objects, PdfError, ReadLimits};

fn path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn load(name: &str) -> Result<Document, PdfError> {
    Document::load(std::fs::read(path(name)).unwrap(), ReadLimits::default())
}

fn read(name: &str) -> Document {
    load(name).unwrap_or_else(|error| panic!("{name}: {error}"))
}

fn text(document: &Document) -> String {
    page_texts(document).unwrap().join("\u{c}")
}

fn recorded(name: &str) -> String {
    std::fs::read_to_string(path(name)).unwrap()
}

fn title(document: &Document) -> Option<String> {
    document
        .info()
        .and_then(|info| info.get(b"Title"))
        .and_then(|title| title.as_str())
        .map(decode_text_string)
}

/// Every way `lines.pdf` was rewritten that still opens without a password.
const LINES: [&str; 13] = [
    "lines.pdf",
    "lines.objstm.pdf",
    "lines.linear.pdf",
    "lines.qdf.pdf",
    "lines.update.pdf",
    "lines.update2.pdf",
    "lines.nostartxref.pdf",
    "lines.shifted.pdf",
    "lines.rc4-40.pdf",
    "lines.rc4-128.pdf",
    "lines.aes-128.pdf",
    "lines.aes-256.pdf",
    "lines.aes-256-r5.pdf",
];

#[test]
fn pages_and_titles_are_those_pdfinfo_reads() {
    let expected: [(&str, usize, Option<&str>); 3] = [
        ("invoice.pdf", 1, Some("Invoice INV-2026-0042")),
        ("multilingual.pdf", 1, Some("شهادات إتمام")),
        ("table.pdf", 1, Some("Quarterly figures")),
    ];
    for (name, pages, expected_title) in expected {
        let document = read(name);
        assert_eq!(document.pages().len(), pages, "{name}");
        assert_eq!(title(&document).as_deref(), expected_title, "{name}");
    }
    for name in LINES {
        let document = read(name);
        assert_eq!(document.pages().len(), 2, "{name}");
        let expected = match name {
            "lines.update.pdf" => Some("Updated title"),
            "lines.update2.pdf" => Some("Second update"),
            _ => None,
        };
        assert_eq!(title(&document).as_deref(), expected, "{name}");
        document.check().unwrap();
    }
}

#[test]
fn texts_are_the_recorded_ones() {
    for name in ["invoice", "multilingual", "table", "lines"] {
        let document = read(&format!("{name}.pdf"));
        assert_eq!(text(&document), recorded(&format!("{name}.txt")), "{name}");
    }
}

#[test]
fn every_layout_of_one_file_reads_the_same_text() {
    let expected = recorded("lines.txt");
    for name in LINES {
        assert_eq!(text(&read(name)), expected, "{name}");
    }
}

#[test]
fn a_broken_or_shifted_cross_reference_is_rebuilt() {
    for name in ["lines.nostartxref.pdf", "lines.shifted.pdf"] {
        assert!(read(name).rebuilt(), "{name}");
    }
    for name in [
        "lines.pdf",
        "lines.objstm.pdf",
        "lines.linear.pdf",
        "lines.update2.pdf",
    ] {
        assert!(!read(name).rebuilt(), "{name}");
    }
}

#[test]
fn files_that_need_a_password_are_refused() {
    for name in ["lines.password.pdf", "lines.password-rc4.pdf"] {
        assert_eq!(load(name).unwrap_err(), PdfError::Encrypted, "{name}");
    }
}

#[test]
fn a_file_cut_short_is_damaged() {
    assert!(matches!(
        load("lines.truncated.pdf"),
        Err(PdfError::Damaged(_))
    ));
}

#[test]
fn saved_documents_read_back_with_the_same_text() {
    for name in [
        "invoice.pdf",
        "multilingual.pdf",
        "table.pdf",
        "lines.objstm.pdf",
        "lines.aes-256.pdf",
    ] {
        let document = read(name);
        let expected = text(&document);
        let expected_title = title(&document);
        let objects = Objects::read(document).unwrap();
        for object_streams in [false, true] {
            let saved = objects.save_with(object_streams).unwrap();
            let again = Document::load(saved, ReadLimits::default()).unwrap();
            assert_eq!(
                text(&again),
                expected,
                "{name}, object streams: {object_streams}"
            );
            assert_eq!(title(&again), expected_title, "{name}");
        }
    }
}
