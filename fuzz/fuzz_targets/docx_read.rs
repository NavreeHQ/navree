//! `navree_documents::DocxFile`: a Word file read from any bytes, within
//! limits.
//!
//! The first byte picks how the rest is fed. Even: the rest is the file, so
//! the zip reader and the package checks see it. Odd: the rest is the main
//! document part of a well-formed package, so that a mutation reaches the XML
//! reader instead of stopping at a checksum.
//!
//! Invariants: a file that is accepted reads as text; the text is no longer
//! than the limit; a refusal is a `Docx` or a `TooLarge` error; and limits
//! that are looser give the same text for a file the tight ones accepted.
#![no_main]

use std::io::{Cursor, Write};

use libfuzzer_sys::fuzz_target;
use navree_documents::docx::DocxLimits;
use navree_documents::{DocumentsError, DocxFile};

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;

const RELATIONSHIPS: &str = r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;

/// A package whose main part is `document`.
fn package(document: &[u8]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored);
    for (name, data) in [
        ("[Content_Types].xml", CONTENT_TYPES.as_bytes()),
        ("_rels/.rels", RELATIONSHIPS.as_bytes()),
        ("word/document.xml", document),
    ] {
        zip.start_file(name, options).expect("a name");
        zip.write_all(data).expect("memory");
    }
    zip.finish().expect("an archive").into_inner()
}

fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    let bytes = if mode % 2 == 0 {
        rest.to_vec()
    } else {
        package(rest)
    };
    let tight = DocxLimits {
        max_entries: 32,
        max_part_bytes: 1 << 20,
        max_inflated_bytes: 2 << 20,
        max_text_bytes: 64 * 1024,
        max_depth: 32,
    };
    match DocxFile::from_bytes_within(bytes, &tight) {
        Ok(file) => {
            let text = file.text_within(&tight).expect("a checked file reads");
            assert!(text.len() <= tight.max_text_bytes);
            let roomy = file
                .text_within(&DocxLimits::default())
                .expect("looser limits read what tight ones read");
            assert_eq!(roomy, text);
        }
        Err(DocumentsError::Docx(_) | DocumentsError::TooLarge(_)) => {}
        Err(other) => panic!("unexpected error: {other:?}"),
    }
});
