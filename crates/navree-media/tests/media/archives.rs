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
        vec![("imsmanifest.xml", false), ("content", true), ("content/index.html", false)]
    );
    assert_eq!(archive.entries()[2].size, 15);
    assert!(archive.contains("content/index.html"));
    assert_eq!(&archive.read("content/index.html").unwrap()[..], b"<h1>Lesson</h1>");
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
        .add_symlink("link", "/etc/passwd", zip::write::SimpleFileOptions::default())
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

#[test]
fn other_files_are_invalid_archives() {
    let err = Archive::open(b"plain text".to_vec(), &limits()).unwrap_err();
    assert!(matches!(err, ArchiveError::Invalid { .. }), "{err:?}");
    assert_eq!(err.code(), "invalid_archive");
}

#[tokio::test]
async fn extract_to_writes_every_file_under_the_prefix() {
    let mut options = StorageOptions::default();
    options.default = "memory".into();
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

#[test]
fn zip_builder_round_trips_and_refuses_unsafe_names() {
    let mut zip = ZipBuilder::new();
    zip.file("report.csv", b"region,total\n".to_vec()).unwrap();
    zip.file("charts/q1.svg", b"<svg/>".to_vec()).unwrap();
    assert!(matches!(
        zip.file("../escape.txt", b"no".to_vec()),
        Err(MediaError::Archive(ArchiveError::UnsafePath { .. }))
    ));
    let bytes = zip.finish().unwrap();

    let mut archive = Archive::open(bytes, &limits()).unwrap();
    assert_eq!(&archive.read("charts/q1.svg").unwrap()[..], b"<svg/>");
    assert_eq!(archive.entries().len(), 2);
}
