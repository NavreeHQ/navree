//! A document whose objects are mostly structure elements (a tagged
//! spreadsheet) is read to be edited within the default limits, with room to
//! spare, however its objects are laid out in the file.

#[path = "support/tagged_sheet.rs"]
mod tagged_sheet;

use navree_pdf::{Document, Objects, ReadLimits};

const MIB: usize = 1024 * 1024;

/// The file `rows` rows make, plain or with object streams.
fn file(rows: usize, object_streams: bool) -> Vec<u8> {
    let mut objects = tagged_sheet::tagged_sheet(rows);
    objects.compress();
    objects.save_with(object_streams).unwrap()
}

/// What reading the file's objects to edit them is counted as, in bytes, and
/// how many objects there are.
fn counted(file: Vec<u8>, limits: ReadLimits) -> (usize, usize) {
    let document = Document::load(file, limits).unwrap();
    let objects = Objects::read(document).unwrap_or_else(|error| panic!("{error}"));
    (objects.memory_counted(), objects.objects.len())
}

/// A quarter of the sheet of the next test, within a quarter of the memory.
#[test]
fn a_tagged_sheet_is_read_for_an_edit_within_a_quarter_of_the_limits() {
    let limits = ReadLimits {
        memory: 64 * MIB,
        ..ReadLimits::default()
    };
    for object_streams in [false, true] {
        let (memory, objects) = counted(file(2_500, object_streams), limits);
        println!(
            "2,500 rows, object streams {object_streams}: {objects} objects, counted {} MiB of 64",
            memory / MIB
        );
        assert!(objects > 60_000, "{objects}");
        assert!(
            memory < limits.memory * 7 / 10,
            "object streams {object_streams}: {} MiB counted of {} MiB",
            memory / MIB,
            limits.memory / MIB
        );
    }
}

/// The object streams a document is laid out in are let go of as their objects
/// are read, and not counted after: a file with them is counted as the same
/// objects without.
#[test]
fn object_streams_are_not_counted_once_they_are_read() {
    let limits = ReadLimits::default();
    let (plain, _) = counted(file(2_500, false), limits);
    let (streams, _) = counted(file(2_500, true), limits);
    assert!(
        streams < plain + plain / 20,
        "{} MiB in object streams, {} MiB plain",
        streams / MIB,
        plain / MIB
    );
}

/// A sheet of 10,000 rows has 250,000 objects: 31 MB written plain, 5 MB in
/// object streams, either way well inside what an upload may be.
#[test]
fn a_tagged_sheet_of_ten_thousand_rows_is_read_for_an_edit() {
    let limits = ReadLimits::default();
    for object_streams in [false, true] {
        let (memory, objects) = counted(file(10_000, object_streams), limits);
        println!(
            "10,000 rows, object streams {object_streams}: {objects} objects, counted {} MiB of {}",
            memory / MIB,
            limits.memory / MIB
        );
        assert!(objects > 250_000, "{objects}");
        assert!(
            memory < limits.memory * 7 / 10,
            "object streams {object_streams}: {} MiB counted of {} MiB",
            memory / MIB,
            limits.memory / MIB
        );
    }
}
