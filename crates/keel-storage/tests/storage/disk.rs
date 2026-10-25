//! `Disk` operations on the local and in-memory drivers.

use bytes::Bytes;
use futures_util::{TryStreamExt, stream};
use keel_storage::StorageError;

use crate::support::{DISKS, storage};

#[tokio::test]
async fn put_get_delete_local_and_memory() {
    let root = tempfile::tempdir().unwrap();
    let storage = storage(root.path());

    for name in DISKS {
        let disk = storage.disk(name).unwrap();
        assert_eq!(disk.name(), name);

        disk.put("docs/readme.txt", Bytes::from_static(b"hello"))
            .await
            .unwrap();
        assert!(disk.exists("docs/readme.txt").await.unwrap(), "{name}");
        assert_eq!(
            disk.get("docs/readme.txt").await.unwrap(),
            "hello",
            "{name}"
        );
        assert_eq!(disk.size("docs/readme.txt").await.unwrap(), 5, "{name}");

        // Overwrite.
        disk.put("docs/readme.txt", "hello again").await.unwrap();
        assert_eq!(disk.get("docs/readme.txt").await.unwrap(), "hello again");

        // Streams in and out.
        let chunks: Vec<Result<Bytes, std::io::Error>> =
            (0..64u8).map(|i| Ok(Bytes::from(vec![i; 1024]))).collect();
        let written = disk
            .put_stream("big/data.bin", stream::iter(chunks))
            .await
            .unwrap();
        assert_eq!(written, 64 * 1024, "{name}");
        let read: Vec<u8> = disk
            .get_stream("big/data.bin")
            .await
            .unwrap()
            .map_ok(|chunk| chunk.to_vec())
            .try_concat()
            .await
            .unwrap();
        assert_eq!(read.len(), 64 * 1024, "{name}");
        assert!(read[..1024].iter().all(|&b| b == 0));
        assert!(read[63 * 1024..].iter().all(|&b| b == 63));

        disk.delete("docs/readme.txt").await.unwrap();
        assert!(!disk.exists("docs/readme.txt").await.unwrap(), "{name}");
        let missing = disk.get("docs/readme.txt").await.unwrap_err();
        assert!(
            matches!(missing, StorageError::NotFound { .. }),
            "{name}: {missing:?}"
        );
        let missing = disk.size("docs/readme.txt").await.unwrap_err();
        assert!(matches!(missing, StorageError::NotFound { .. }), "{name}");
        assert!(matches!(
            disk.get_stream("docs/readme.txt").await.err(),
            Some(StorageError::NotFound { .. })
        ));
        // Deleting a missing file is not an error.
        disk.delete("docs/readme.txt").await.unwrap();
    }

    // The local disk writes under its root.
    let on_disk = std::fs::read(root.path().join("big").join("data.bin")).unwrap();
    assert_eq!(on_disk.len(), 64 * 1024);
    assert!(!root.path().join("docs").join("readme.txt").exists());

    // The default disk is the configured one.
    assert_eq!(storage.default_disk().name(), "local");
    assert!(matches!(
        storage.disk("s3"),
        Err(StorageError::UnknownDisk { .. })
    ));
}

#[tokio::test]
async fn failed_stream_leaves_nothing_behind() {
    let root = tempfile::tempdir().unwrap();
    let storage = storage(root.path());

    for name in DISKS {
        let disk = storage.disk(name).unwrap();
        let chunks: Vec<Result<Bytes, std::io::Error>> = vec![
            Ok(Bytes::from_static(b"partial")),
            Err(std::io::Error::other("client went away")),
        ];
        let error = disk
            .put_stream("broken.bin", stream::iter(chunks))
            .await
            .unwrap_err();
        assert!(matches!(error, StorageError::Stream { .. }), "{error:?}");
        assert!(!disk.exists("broken.bin").await.unwrap(), "{name}");
        assert!(disk.list("").await.unwrap().is_empty(), "{name}");
    }
    let leftovers: Vec<_> = std::fs::read_dir(root.path()).unwrap().collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

/// Paths that must never reach the backend.
const TRAVERSALS: [&str; 12] = [
    "../secret.txt",
    "a/../../secret.txt",
    r"..\secret.txt",
    r"a\..\..\secret.txt",
    "..",
    "./..",
    "a/./../b",
    "...",
    "a/. ./b",
    "a/ /b",
    "x\0y",
    "a/\nb",
];

#[tokio::test]
async fn rejects_path_traversal() {
    let outer = tempfile::tempdir().unwrap();
    let root = outer.path().join("root");
    std::fs::write(outer.path().join("secret.txt"), "top secret").unwrap();
    let storage = storage(&root);

    for name in DISKS {
        let disk = storage.disk(name).unwrap();
        for path in TRAVERSALS {
            let invalid = |result: Result<(), StorageError>| {
                assert!(
                    matches!(result, Err(StorageError::InvalidPath { .. })),
                    "{name} {path:?}: {result:?}"
                );
            };
            invalid(disk.put(path, "pwned").await);
            invalid(disk.get(path).await.map(drop));
            invalid(disk.get_stream(path).await.map(drop));
            invalid(disk.exists(path).await.map(drop));
            invalid(disk.size(path).await.map(drop));
            invalid(disk.delete(path).await);
            invalid(disk.list(path).await.map(drop));
            invalid(
                disk.put_stream(path, stream::iter([Ok::<_, std::io::Error>(Bytes::new())]))
                    .await
                    .map(drop),
            );
        }
        for empty in ["", "/", "./", "//"] {
            assert!(
                matches!(
                    disk.put(empty, "x").await,
                    Err(StorageError::InvalidPath { .. })
                ),
                "{name} {empty:?}"
            );
        }
    }
    assert_eq!(
        std::fs::read_to_string(outer.path().join("secret.txt")).unwrap(),
        "top secret"
    );
    assert_eq!(std::fs::read_dir(outer.path()).unwrap().count(), 2);
}

#[tokio::test]
async fn normalises_paths() {
    let root = tempfile::tempdir().unwrap();
    let storage = storage(root.path());

    for name in DISKS {
        let disk = storage.disk(name).unwrap();
        disk.put("/reports//2026/./q1.csv", "a,b").await.unwrap();
        assert_eq!(disk.get("reports/2026/q1.csv").await.unwrap(), "a,b");
        assert_eq!(disk.get(r"reports\2026\q1.csv").await.unwrap(), "a,b");
        assert_eq!(
            disk.list("/reports/").await.unwrap(),
            vec!["reports/2026/q1.csv"],
            "{name}"
        );
        // Names with spaces and other punctuation survive a round trip.
        disk.put("my files/report (final) #2.txt", "ok")
            .await
            .unwrap();
        assert_eq!(
            disk.list("my files").await.unwrap(),
            vec!["my files/report (final) #2.txt"],
            "{name}"
        );
        assert_eq!(
            disk.get("my files/report (final) #2.txt").await.unwrap(),
            "ok"
        );
    }
    assert!(
        root.path()
            .join("reports")
            .join("2026")
            .join("q1.csv")
            .exists()
    );
}

#[tokio::test]
async fn list_prefix() {
    let root = tempfile::tempdir().unwrap();
    let storage = storage(root.path());

    for name in DISKS {
        let disk = storage.disk(name).unwrap();
        for path in [
            "docs/b.txt",
            "docs/a.txt",
            "docs/2026/report.pdf",
            "docs2/other.txt",
            "docs.txt",
            "images/logo.png",
        ] {
            disk.put(path, "x").await.unwrap();
        }

        // A prefix is a directory: `docs` does not match `docs2` or `docs.txt`.
        assert_eq!(
            disk.list("docs").await.unwrap(),
            vec!["docs/2026/report.pdf", "docs/a.txt", "docs/b.txt"],
            "{name}"
        );
        assert_eq!(disk.list("/docs/").await.unwrap().len(), 3, "{name}");
        assert_eq!(
            disk.list("docs/2026").await.unwrap(),
            vec!["docs/2026/report.pdf"],
            "{name}"
        );
        // The root lists everything.
        assert_eq!(disk.list("").await.unwrap().len(), 6, "{name}");
        assert_eq!(disk.list("/").await.unwrap().len(), 6, "{name}");
        // A missing directory is empty, not an error.
        assert!(
            disk.list("nothing/here").await.unwrap().is_empty(),
            "{name}"
        );
    }
}
