//! A tagged spreadsheet of ten thousand rows (250,000 objects, 31 MB written
//! plain and 5 MB in object streams, either way well inside what an upload
//! may be) is edited within the default limits: every edit reads all of its
//! objects.

#[path = "../../../navree-pdf/tests/support/tagged_sheet.rs"]
mod tagged_sheet;

use navree_documents::{Pages, Pdf, Stamp};

/// The file of `rows` rows, plain or with object streams.
fn sheet(rows: usize, object_streams: bool) -> Pdf {
    let mut objects = tagged_sheet::tagged_sheet(rows);
    objects.compress();
    Pdf::from_bytes(objects.save_with(object_streams).unwrap()).unwrap()
}

/// The text of the first page of `pdf`, up to its first form feed.
fn first_page(pdf: &Pdf) -> String {
    pdf.page_texts().unwrap().swap_remove(0)
}

#[test]
fn a_tagged_sheet_written_plain_is_edited() {
    let pdf = sheet(10_000, false);
    assert_eq!(pdf.page_count().unwrap(), 189);
    let titled = pdf.with_metadata("Report", Some("Navree")).unwrap();
    assert_eq!(titled.title().unwrap().as_deref(), Some("Report"));
    assert_eq!(titled.page_count().unwrap(), 189);
    let sealed = pdf
        .overlay(&Pdf::from_lines(["SEAL"]), Pages::First)
        .unwrap();
    assert!(first_page(&sealed).contains("SEAL"));
}

#[test]
fn a_tagged_sheet_in_object_streams_is_edited() {
    let pdf = sheet(10_000, true);
    assert!(pdf.len() < 20 * 1024 * 1024, "{} bytes", pdf.len());
    assert_eq!(pdf.page_count().unwrap(), 189);
    let stamped = pdf.stamp(&Stamp::text("PAID")).unwrap();
    assert!(first_page(&stamped).contains("PAID"));
    let merged = Pdf::merge([&pdf, &Pdf::from_lines(["one more page"])]).unwrap();
    assert_eq!(merged.page_count().unwrap(), 190);
}
