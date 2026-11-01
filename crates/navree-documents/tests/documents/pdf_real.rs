//! Documents an office suite wrote (a book of three thousand pages, a tagged
//! spreadsheet, a document of images, a deck), plain and rewritten with
//! object streams, are read and edited within the default limits, and the
//! room they leave is printed (`--nocapture`).
//!
//! The documents are made by LibreOffice and qpdf (`real_documents/make.sh`).
//! Point `NAVREE_REAL_PDFS` at a directory to read them from, or at an empty
//! one to have them made there; without the variable, or without the tools,
//! the test does nothing.

use std::path::{Path, PathBuf};
use std::process::Command;

use navree_documents::{Pages, Pdf, Stamp};
use navree_pdf::{Document, Objects, ReadLimits, TextLimits};

const MIB: usize = 1024 * 1024;

fn have(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

/// The directory the documents are in, made when it has none.
fn corpus() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("NAVREE_REAL_PDFS")?);
    if !dir.join("sheet.os.pdf").exists() {
        if !have("soffice") || !have("qpdf") {
            println!("no soffice or qpdf: no real documents");
            return None;
        }
        std::fs::create_dir_all(&dir).unwrap();
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/real_documents/make.sh");
        let made = Command::new("bash")
            .arg(script)
            .arg(&dir)
            .status()
            .expect("bash runs");
        assert!(made.success(), "make.sh failed");
    }
    Some(dir)
}

/// `qpdf --check` of `pdf`, when qpdf is there: nothing wrong with it.
fn checked_by_qpdf(pdf: &Pdf, name: &str) {
    if !have("qpdf") {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.pdf");
    std::fs::write(&path, &pdf[..]).unwrap();
    let output = Command::new("qpdf")
        .arg("--check")
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{name}: qpdf --check: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

fn percent(used: usize, limit: usize) -> f64 {
    used as f64 * 100.0 / limit as f64
}

#[test]
fn real_documents_are_read_and_edited_with_room_to_spare() {
    let Some(dir) = corpus() else { return };
    let limits = ReadLimits::default();
    let text_limits = TextLimits::default();
    println!(
        "{:<12} {:>6} {:>6} {:>8} | text work | edit: objects counted | read work | merge keeps",
        "document", "MiB", "pages", "objects"
    );
    for name in [
        "sheet",
        "sheet.os",
        "book",
        "book.os",
        "images",
        "images.os",
        "deck",
        "deck.os",
    ] {
        let bytes = std::fs::read(dir.join(format!("{name}.pdf"))).unwrap();
        let size = bytes.len();
        let pdf = Pdf::from_bytes(bytes.clone()).unwrap_or_else(|error| panic!("{name}: {error}"));
        let pages = pdf.page_count().unwrap();

        // What the text of every page takes of the budget for text.
        let document = Document::load(bytes.clone(), limits).unwrap();
        let (texts, text_work) = navree_pdf::text::page_texts_counted(&document, text_limits)
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(texts.len(), pages, "{name}");
        drop(document);

        // What an edit takes of the budget for objects: all of them are read.
        let objects = Objects::read(Document::load(bytes, limits).unwrap())
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        let (memory, work, count) = (
            objects.memory_counted(),
            objects.work_counted(),
            objects.objects.len(),
        );
        drop(objects);

        // What a merge keeps of it: the objects of its output, read again.
        let kept = {
            let merged = Pdf::merge([&pdf]).unwrap_or_else(|error| panic!("{name}: {error}"));
            let document = Document::load(merged.bytes().clone(), limits).unwrap();
            Objects::read(document).unwrap().memory_counted()
        };
        println!(
            "{name:<12} {:>6} {pages:>6} {count:>8} | {:>8.1}% | {:>8.1}% ({} MiB) | {:>5.1}% | {} MiB",
            size / MIB,
            percent(text_work, text_limits.document_work),
            percent(memory, limits.memory),
            memory / MIB,
            percent(work, limits.work),
            kept / MIB,
        );
        // Four copies of it merge within the limits of one document.
        let four = Pdf::merge(std::iter::repeat_n(&pdf, 4))
            .unwrap_or_else(|error| panic!("{name}: four copies: {error}"));
        assert_eq!(four.page_count().unwrap(), 4 * pages, "{name}");
        assert!(
            memory < limits.memory * 6 / 10,
            "{name}: {} MiB of {} MiB counted",
            memory / MIB,
            limits.memory / MIB
        );
        assert!(work < limits.work / 4, "{name}: {} MiB of work", work / MIB);
        assert!(
            text_work < text_limits.document_work / 2,
            "{name}: {} MiB of text work",
            text_work / MIB
        );

        // The edits, each one read back by a second reader.
        let seal = Pdf::from_lines(["SEAL"]);
        let edited = [
            ("with_metadata", pdf.with_metadata("Title", Some("Author"))),
            ("stamp", pdf.stamp(&Stamp::text("PAID"))),
            ("overlay", pdf.overlay(&seal, Pages::All)),
            ("merge", Pdf::merge([&pdf, &seal])),
        ];
        for (edit, result) in edited {
            let out = result.unwrap_or_else(|error| panic!("{name} {edit}: {error}"));
            let expected = if edit == "merge" { pages + 1 } else { pages };
            assert_eq!(out.page_count().unwrap(), expected, "{name} {edit}");
            checked_by_qpdf(&out, &format!("{name} {edit}"));
        }
    }
}
