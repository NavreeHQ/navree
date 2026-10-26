//! `Disk::url` and `Disk::temporary_url`.

use std::collections::BTreeMap;

use chrono::{TimeZone, Utc};
use navree_storage::{DiskConfig, Storage, StorageError, StorageOptions};

use crate::support::{SIGNING_KEY, options};

fn with_public_disk(root: &std::path::Path, base_url: Option<&str>) -> Storage {
    let mut options = options(root);
    options.disks.insert(
        "public".to_owned(),
        DiskConfig::Local {
            root: root.join("public"),
            public_url: Some("https://cdn.example.com/files/".to_owned()),
        },
    );
    options.base_url = base_url.map(str::to_owned);
    Storage::new(&options, SIGNING_KEY).unwrap()
}

#[test]
fn url_joins_the_public_url_and_the_encoded_path() {
    let root = tempfile::tempdir().unwrap();
    let storage = with_public_disk(root.path(), None);
    let public = storage.disk("public").unwrap();

    assert_eq!(
        public.url("/avatars//jane doe.png").unwrap(),
        "https://cdn.example.com/files/avatars/jane%20doe.png"
    );
    assert_eq!(
        public.url("a/100% #1?.txt").unwrap(),
        "https://cdn.example.com/files/a/100%25%20%231%3F.txt"
    );
    assert!(matches!(
        public.url("../etc/passwd"),
        Err(StorageError::InvalidPath { .. })
    ));
    for name in ["local", "memory"] {
        assert!(matches!(
            storage.disk(name).unwrap().url("a.txt"),
            Err(StorageError::NoPublicUrl { .. })
        ));
    }
}

#[tokio::test]
async fn temporary_url_is_signed_and_points_at_the_storage_route() {
    let root = tempfile::tempdir().unwrap();
    let storage = with_public_disk(root.path(), None);
    let expires = Utc.with_ymd_and_hms(2030, 1, 2, 3, 4, 5).unwrap();
    let disk = storage.disk("memory").unwrap();

    let url = disk
        .temporary_url("/reports/q1 final.pdf", expires)
        .await
        .unwrap();
    let (path, query) = url.split_once('?').unwrap();
    assert_eq!(path, "/storage/memory/reports/q1%20final.pdf");
    let (expires_part, signature_part) = query.split_once('&').unwrap();
    assert_eq!(expires_part, format!("expires={}", expires.timestamp()));
    let signature = signature_part.strip_prefix("signature=").unwrap();
    assert_eq!(signature.len(), 64);
    assert!(signature.bytes().all(|b| b.is_ascii_hexdigit()));

    // Deterministic for the same input; different for another path, expiry or disk.
    assert_eq!(
        disk.temporary_url("reports/q1 final.pdf", expires).await.unwrap(),
        url
    );
    assert_ne!(disk.temporary_url("reports/q2.pdf", expires).await.unwrap(), url);
    let later = expires + chrono::Duration::seconds(1);
    assert!(
        !disk
            .temporary_url("reports/q1 final.pdf", later)
            .await
            .unwrap()
            .ends_with(signature)
    );
    let local = storage
        .disk("local")
        .unwrap()
        .temporary_url("reports/q1 final.pdf", expires).await;
    assert!(!local.unwrap().ends_with(signature));
    // Another key signs differently.
    let other = Storage::new(&options(root.path()), b"another key").unwrap();
    let other = other
        .disk("memory")
        .unwrap()
        .temporary_url("reports/q1 final.pdf", expires).await;
    assert!(!other.unwrap().ends_with(signature));

    assert!(matches!(
        disk.temporary_url("../x", expires).await,
        Err(StorageError::InvalidPath { .. })
    ));
}

#[tokio::test]
async fn temporary_url_uses_the_base_url() {
    let root = tempfile::tempdir().unwrap();
    let storage = with_public_disk(root.path(), Some("https://api.example.com/"));
    let url = storage
        .default_disk()
        .temporary_url("a.txt", Utc::now())
        .await
        .unwrap();
    assert!(
        url.starts_with("https://api.example.com/storage/local/a.txt?expires="),
        "{url}"
    );
}

#[test]
fn invalid_options_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    let invalid = |options: StorageOptions| match Storage::new(&options, SIGNING_KEY) {
        Err(StorageError::InvalidOptions(message)) => message,
        other => panic!("must be rejected: {other:?}"),
    };

    let mut missing_default = options(root.path());
    missing_default.default = "s3".to_owned();
    assert!(invalid(missing_default).contains("`s3`"));

    let mut bad_name = options(root.path());
    bad_name
        .disks
        .insert("my disk".to_owned(), DiskConfig::Memory);
    assert!(invalid(bad_name).contains("`my disk`"));

    let no_disks = StorageOptions {
        disks: BTreeMap::new(),
        ..StorageOptions::default()
    };
    assert!(invalid(no_disks).contains("`local`"));

    let mut empty_root = options(root.path());
    empty_root
        .disks
        .insert("empty".to_owned(), DiskConfig::local(""));
    assert!(invalid(empty_root).contains("root"));
}
