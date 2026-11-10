//! Zip archives: listing, bounded reads, zip-slip and size-bomb guards,
//! extraction to a disk, and building archives.

use navree_media::archive::{Archive, ArchiveLimits, ZipBuilder};
use navree_media::{ArchiveError, MediaError};
use navree_storage::{DiskConfig, Storage, StorageOptions};

use crate::support::zip_of;

fn limits() -> ArchiveLimits {
    ArchiveLimits::default()
}

#[test]
fn lists_entries_and_reads_files() {
    let bytes = zip_of(&[
        ("imsmanifest.xml", b"<manifest/>"),
        ("content/", b""),
        ("content/index.html", b"<h1>Lesson</h1>"),
    ]);
    let mut archive = Archive::open(bytes, &limits()).unwrap();
    let paths: Vec<(&str, bool)> = archive
        .entries()
        .iter()
        .map(|e| (e.path.as_str(), e.is_dir))
        .collect();
    assert_eq!(
        paths,
        vec![
            ("imsmanifest.xml", false),
            ("content", true),
            ("content/index.html", false)
        ]
    );
    assert_eq!(archive.entries()[2].size, 15);
    assert!(archive.contains("content/index.html"));
    assert_eq!(
        &archive.read("content/index.html").unwrap()[..],
        b"<h1>Lesson</h1>"
    );
    assert_eq!(
        archive.read("missing.txt").unwrap_err(),
        ArchiveError::NotFound {
            path: "missing.txt".into()
        }
    );
}

#[test]
fn entry_names_are_normalized() {
    let bytes = zip_of(&[("./docs//a.txt", b"a"), ("docs\\b.txt", b"b")]);
    let archive = Archive::open(bytes, &limits()).unwrap();
    let paths: Vec<&str> = archive.entries().iter().map(|e| e.path.as_str()).collect();
    assert_eq!(paths, vec!["docs/a.txt", "docs/b.txt"]);
}

#[test]
fn zip_slip_paths_are_refused() {
    for name in [
        "../evil.txt",
        "/etc/passwd",
        "C:/Windows/evil.dll",
        "c:evil.txt",
        "a/../../evil.txt",
        "a\\..\\..\\evil.txt",
        "a/b/../c.txt",
        "bad\u{1}name.txt",
        "...",
    ] {
        let bytes = zip_of(&[("ok.txt", b"fine"), (name, b"x")]);
        let err = Archive::open(bytes, &limits()).unwrap_err();
        assert!(
            matches!(err, ArchiveError::UnsafePath { .. }),
            "{name:?} gave {err:?}"
        );
        assert_eq!(err.code(), "unsafe_archive");
        assert_eq!(err.status(), 400);
    }
}

#[test]
fn duplicate_paths_are_refused() {
    let bytes = zip_of(&[("docs/a.txt", b"one"), ("docs//a.txt", b"two")]);
    let err = Archive::open(bytes, &limits()).unwrap_err();
    assert_eq!(
        err,
        ArchiveError::Duplicate {
            path: "docs/a.txt".into()
        }
    );
}

#[test]
fn symbolic_links_are_refused() {
    use std::io::Cursor;
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .add_symlink(
            "link",
            "/etc/passwd",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
    let bytes = writer.finish().unwrap().into_inner();
    let err = Archive::open(bytes, &limits()).unwrap_err();
    assert!(matches!(err, ArchiveError::Symlink { .. }), "{err:?}");
}

#[test]
fn size_bombs_are_refused() {
    let zeros = vec![0u8; 8 * 1024 * 1024];
    let bomb = zip_of(&[("zeros.bin", &zeros)]);
    let err = Archive::open(bomb.clone(), &limits()).unwrap_err();
    assert!(matches!(err, ArchiveError::Ratio { .. }), "{err:?}");
    assert_eq!(err.code(), "archive_too_large");
    assert_eq!(err.status(), 413);

    let generous = ArchiveLimits {
        max_ratio: 0,
        max_entry_bytes: 1024 * 1024,
        ..limits()
    };
    let err = Archive::open(bomb.clone(), &generous).unwrap_err();
    assert!(matches!(err, ArchiveError::EntryTooLarge { .. }), "{err:?}");

    let total = ArchiveLimits {
        max_ratio: 0,
        max_total_bytes: 10 * 1024 * 1024,
        ..limits()
    };
    let two = zip_of(&[("a.bin", &zeros), ("b.bin", &zeros)]);
    let err = Archive::open(two, &total).unwrap_err();
    assert!(matches!(err, ArchiveError::TooLarge { .. }), "{err:?}");

    // Small files may compress well: the ratio applies above 1 MiB.
    let small = zip_of(&[("blank.csv", &vec![b','; 64 * 1024])]);
    assert!(Archive::open(small, &limits()).is_ok());
}

#[test]
fn too_many_entries_are_refused() {
    let names: Vec<String> = (0..11).map(|i| format!("f{i}.txt")).collect();
    let entries: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), &b"x"[..])).collect();
    let few = ArchiveLimits {
        max_entries: 10,
        ..limits()
    };
    let err = Archive::open(zip_of(&entries), &few).unwrap_err();
    assert_eq!(err, ArchiveError::TooManyEntries { max: 10 });
}

/// `bytes` with the first central directory header damaged: an archive that
/// can only be told from a good one by reading the directory.
fn damage_directory(mut bytes: Vec<u8>) -> Vec<u8> {
    let at = bytes.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
    bytes[at] = b'X';
    bytes
}

/// Where the end-of-central-directory record of `bytes` is.
fn end_record(bytes: &[u8]) -> usize {
    bytes.windows(4).rposition(|w| w == b"PK\x05\x06").unwrap()
}

fn two_entries() -> Vec<u8> {
    zip_of(&[("a.txt", b"a"), ("b.txt", b"b")])
}

fn at_most(max_entries: usize) -> ArchiveLimits {
    ArchiveLimits {
        max_entries,
        ..limits()
    }
}

#[test]
fn the_entry_count_is_checked_before_the_directory_is_read() {
    // Refused for the count the end record declares; a reader that held the
    // entries first would have found the damaged directory instead.
    let bytes = damage_directory(two_entries());
    let err = Archive::open(bytes.clone(), &at_most(1)).unwrap_err();
    assert_eq!(err, ArchiveError::TooManyEntries { max: 1 });
    // Under the limit the damage is what's reported.
    let err = Archive::open(bytes, &limits()).unwrap_err();
    assert!(matches!(err, ArchiveError::Invalid { .. }), "{err:?}");
}

#[test]
fn a_stray_end_record_does_not_hide_the_entry_count() {
    // A record in the archive comment that declares one entry and points
    // nowhere: the archive reader falls back to the real one before it.
    let mut bytes = damage_directory(two_entries());
    let mut stray = b"PK\x05\x06".to_vec();
    stray.extend_from_slice(&[0, 0, 0, 0, 1, 0, 1, 0]);
    stray.extend_from_slice(&[0; 8]);
    stray.extend_from_slice(&[0, 0]);
    let end = end_record(&bytes);
    bytes[end + 20..end + 22].copy_from_slice(&(stray.len() as u16).to_le_bytes());
    bytes.extend_from_slice(&stray);
    let err = Archive::open(bytes, &at_most(1)).unwrap_err();
    assert_eq!(err, ArchiveError::TooManyEntries { max: 1 });
}

#[test]
fn a_zip64_count_is_read_from_the_headers_that_exist() {
    // The record defers to a zip64 one, which can declare anything: what the
    // file can hold is its central headers, counted without being kept.
    let mut bytes = damage_directory(two_entries());
    for _ in 0..4 {
        let at = end_record(&bytes);
        bytes.splice(at..at, *b"PK\x01\x02");
    }
    let end = end_record(&bytes);
    bytes[end + 8..end + 12].copy_from_slice(&[0xff; 4]);
    let err = Archive::open(bytes, &at_most(3)).unwrap_err();
    assert_eq!(err, ArchiveError::TooManyEntries { max: 3 });
}

#[test]
fn a_limit_of_zero_does_not_count_entries() {
    let bytes = damage_directory(two_entries());
    let err = Archive::open(bytes, &at_most(0)).unwrap_err();
    assert!(matches!(err, ArchiveError::Invalid { .. }), "{err:?}");
}

#[test]
fn many_small_entries_are_refused_by_their_count() {
    // The shape of the attack, at a size a test can build: thousands of
    // empty entries in a few hundred kilobytes.
    let names: Vec<String> = (0..3_000).map(|i| format!("{i}")).collect();
    let entries: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), &b""[..])).collect();
    let bytes = zip_of(&entries);
    let err = Archive::open(bytes.clone(), &at_most(2_999)).unwrap_err();
    assert_eq!(err, ArchiveError::TooManyEntries { max: 2_999 });
    assert_eq!(
        Archive::open(bytes, &at_most(3_000))
            .unwrap()
            .entries()
            .len(),
        3_000
    );
}

#[test]
fn other_files_are_invalid_archives() {
    let err = Archive::open(b"plain text".to_vec(), &limits()).unwrap_err();
    assert!(matches!(err, ArchiveError::Invalid { .. }), "{err:?}");
    assert_eq!(err.code(), "invalid_archive");
}

#[tokio::test]
async fn extract_to_writes_every_file_under_the_prefix() {
    let mut options = StorageOptions {
        default: "memory".into(),
        ..StorageOptions::default()
    };
    options.disks.clear();
    options.disks.insert("memory".into(), DiskConfig::Memory);
    let storage = Storage::new(&options, b"key").unwrap();
    let disk = storage.default_disk();

    let bytes = zip_of(&[
        ("imsmanifest.xml", b"<manifest/>"),
        ("content/", b""),
        ("content/index.html", b"<h1>Lesson</h1>"),
    ]);
    let archive = Archive::open(bytes, &limits()).unwrap();
    let report = archive.extract_to(&disk, "scorm/42").await.unwrap();
    assert_eq!(
        report.files,
        vec!["scorm/42/imsmanifest.xml", "scorm/42/content/index.html"]
    );
    assert_eq!(report.bytes, 11 + 15);
    assert_eq!(
        &disk.get("scorm/42/content/index.html").await.unwrap()[..],
        b"<h1>Lesson</h1>"
    );
}

/// Inflating an archive is CPU work: it must not hold the task that awaits
/// `extract_to`. On a runtime with one thread, a task spawned beside it
/// gets to run before the extraction finishes only if the extraction
/// yields while it inflates.
#[tokio::test]
async fn extract_to_lets_other_tasks_run_while_it_inflates() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let mut options = StorageOptions {
        default: "memory".into(),
        ..StorageOptions::default()
    };
    options.disks.clear();
    options.disks.insert("memory".into(), DiskConfig::Memory);
    let storage = Storage::new(&options, b"key").unwrap();
    let disk = storage.default_disk();

    let page = vec![b'a'; 256 * 1024];
    let names: Vec<String> = (0..8).map(|i| format!("pages/{i}.html")).collect();
    let entries: Vec<(&str, &[u8])> = names
        .iter()
        .map(|name| (name.as_str(), page.as_slice()))
        .collect();
    let archive = Archive::open(zip_of(&entries), &limits()).unwrap();

    let ran = Arc::new(AtomicBool::new(false));
    let flag = ran.clone();
    let other = tokio::spawn(async move { flag.store(true, Ordering::SeqCst) });
    let report = archive.extract_to(&disk, "scorm").await.unwrap();
    assert_eq!(report.files.len(), 8);
    assert!(
        ran.load(Ordering::SeqCst),
        "a task spawned beside the extraction did not run until it finished"
    );
    other.await.unwrap();
}

#[test]
fn zip_builder_round_trips_and_refuses_unsafe_names() {
    let mut zip = ZipBuilder::new();
    zip.file("report.csv", b"region,total\n").unwrap();
    zip.file("charts/q1.svg", b"<svg/>").unwrap();
    assert!(matches!(
        zip.file("../escape.txt", b"no"),
        Err(MediaError::Archive(ArchiveError::UnsafePath { .. }))
    ));
    let bytes = zip.finish().unwrap();

    let mut archive = Archive::open(bytes, &limits()).unwrap();
    assert_eq!(&archive.read("charts/q1.svg").unwrap()[..], b"<svg/>");
    assert_eq!(archive.entries().len(), 2);
}
