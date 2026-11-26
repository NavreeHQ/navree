//! Shared integration suite run against every Navree cloud store (`S3Store`,
//! `GcsStore` and `AzureStore`) talking to its real emulator (`MinIO` /
//! `RustFS`, `fake-gcs-server` and `Azurite`).
//!
//! Each test function is named for what it proves and runs under a fresh prefix
//! so tests execute concurrently without interfering with one another.

#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use futures_util::{StreamExt, TryStreamExt, stream};
use navree_datetime::{Span, Timestamp};
use navree_storage::store::{HeaderName, HeaderValue, Method, PrefixedStore, Store, StoreError};

/// One isolated store instance connected to a live emulator, plus the raw
/// unprefixed driver store for root-boundary checks.
pub struct Subject {
    /// Driver label (`"s3"`, `"gcs"` or `"azure"`).
    pub driver: &'static str,
    /// Store scoped to a unique per-test prefix inside the emulator's bucket.
    pub store: Arc<PrefixedStore>,
    /// Underlying unprefixed driver store, used to verify empty-key and
    /// bucket-root traversal refusals directly on the driver.
    pub raw: Arc<dyn Store>,
    /// Whether `signed_url_with` binds request headers into the signature
    /// (`true` on S3 SigV4 and GCS V4; `false` on Azure SAS).
    pub binds_put_headers: bool,
    /// Required headers sent alongside an unsigned-header `PUT` link (such as
    /// `x-ms-blob-type: BlockBlob` on Azure Blob Storage).
    pub unsigned_put_headers: &'static [(&'static str, &'static str)],
    /// Keeps any local helper server (such as GCS's mock OAuth token endpoint)
    /// alive for the lifetime of the test.
    pub guard: Option<Arc<dyn Send + Sync>>,
}

/// Reads the emulator URL from `variable`. When unset locally, prints a
/// `SKIPPED` notice and returns `None` unless `NAVREE_TEST_REQUIRE_SERVICES=1`
/// is set (as in CI), in which case a missing emulator fails the test.
pub fn emulator_url(variable: &str, test: &str) -> Option<String> {
    match std::env::var(variable) {
        Ok(url) if !url.trim().is_empty() => Some(url),
        _ if std::env::var("NAVREE_TEST_REQUIRE_SERVICES").as_deref() == Ok("1") => {
            panic!("{test}: set {variable} (NAVREE_TEST_REQUIRE_SERVICES=1)")
        }
        _ => {
            println!("SKIPPED {test}: set {variable}");
            None
        }
    }
}

/// Removes every object left under `subject.store` at the end of a test.
async fn cleanup(subject: &Subject) {
    if let Ok(entries) = subject.store.list("").await {
        stream::iter(entries)
            .for_each_concurrent(32, |meta| async move {
                let _ = subject.store.delete(&meta.path).await;
            })
            .await;
    }
}

/// Proves `put`, `head`, whole-object `read`, atomic overwrite and empty-file
/// storage round-trip bytes and metadata accurately.
pub async fn put_head_and_read_round_trip_bytes_and_metadata(subject: Subject) {
    let store = &subject.store;
    let driver = subject.driver;
    let before = Timestamp::now() - Span::minutes(5);

    let first = Bytes::from_static("চালान 1101 — Ludhiana depot, ₹1,25,000\n".as_bytes());
    store
        .put("invoices/2026/inv-1101.txt", first.clone())
        .await
        .unwrap();

    let meta = store.head("invoices/2026/inv-1101.txt").await.unwrap();
    assert_eq!(meta.path, "invoices/2026/inv-1101.txt", "{driver}");
    assert_eq!(meta.size, first.len() as u64, "{driver}");
    assert!(meta.last_modified >= before, "{driver}: {meta:?}");
    assert_eq!(
        store
            .read("invoices/2026/inv-1101.txt", None)
            .await
            .unwrap(),
        first,
        "{driver}"
    );

    // Overwriting replaces the whole object with the new bytes and size.
    let updated = Bytes::from_static("Invoice 1101 settled in Amritsar — ₹1,25,000\n".as_bytes());
    store
        .put("invoices/2026/inv-1101.txt", updated.clone())
        .await
        .unwrap();
    assert_eq!(
        store.head("invoices/2026/inv-1101.txt").await.unwrap().size,
        updated.len() as u64,
        "{driver}"
    );
    assert_eq!(
        store
            .read("invoices/2026/inv-1101.txt", None)
            .await
            .unwrap(),
        updated,
        "{driver}"
    );

    // Zero-byte files are stored and reported with size 0.
    store
        .put("invoices/2026/empty.marker", Bytes::new())
        .await
        .unwrap();
    let empty_meta = store.head("invoices/2026/empty.marker").await.unwrap();
    assert_eq!(empty_meta.size, 0, "{driver}");
    assert!(
        store
            .read("invoices/2026/empty.marker", None)
            .await
            .unwrap()
            .is_empty(),
        "{driver}"
    );

    assert!(
        matches!(
            store.head("invoices/2026/missing.txt").await,
            Err(StoreError::NotFound)
        ),
        "{driver}"
    );
    assert!(
        matches!(
            store.read("invoices/2026/missing.txt", None).await,
            Err(StoreError::NotFound)
        ),
        "{driver}"
    );

    cleanup(&subject).await;
}

/// Proves `read` with `Some(start..end)` returns exact slices, clamps ranges
/// extending past EOF, returns empty bytes for empty or past-EOF ranges on
/// existing objects, and still reports `StoreError::NotFound` on missing keys.
pub async fn read_returns_exact_byte_ranges_and_clamps_past_the_end(subject: Subject) {
    let store = &subject.store;
    let driver = subject.driver;

    store
        .put("ranges/digits.txt", Bytes::from_static(b"0123456789abcdef"))
        .await
        .unwrap();

    assert_eq!(
        store.read("ranges/digits.txt", Some(0..4)).await.unwrap(),
        Bytes::from_static(b"0123"),
        "{driver}"
    );
    assert_eq!(
        store.read("ranges/digits.txt", Some(4..12)).await.unwrap(),
        Bytes::from_static(b"456789ab"),
        "{driver}"
    );
    assert_eq!(
        store
            .read("ranges/digits.txt", Some(12..100))
            .await
            .unwrap(),
        Bytes::from_static(b"cdef"),
        "{driver}"
    );
    assert!(
        store
            .read("ranges/digits.txt", Some(16..32))
            .await
            .unwrap()
            .is_empty(),
        "{driver}"
    );
    assert!(
        store
            .read("ranges/digits.txt", Some(5..5))
            .await
            .unwrap()
            .is_empty(),
        "{driver}"
    );
    #[allow(clippy::reversed_empty_ranges)]
    let reversed = 9..3;
    assert!(
        store
            .read("ranges/digits.txt", Some(reversed))
            .await
            .unwrap()
            .is_empty(),
        "{driver}"
    );

    assert!(
        matches!(
            store.read("ranges/missing.txt", Some(0..4)).await,
            Err(StoreError::NotFound)
        ),
        "{driver}"
    );
    assert!(
        matches!(
            store.read("ranges/missing.txt", Some(4..4)).await,
            Err(StoreError::NotFound)
        ),
        "{driver}"
    );

    cleanup(&subject).await;
}

/// Proves `open` streams the full object contents and reports the exact byte
/// length up front, and fails with `StoreError::NotFound` on a missing object.
pub async fn open_streams_the_whole_file_and_reports_its_size(subject: Subject) {
    let store = &subject.store;
    let driver = subject.driver;

    let expected: Vec<u8> = (0..131_072u32).map(|i| (i % 251) as u8).collect();
    store
        .put("streams/ledger.bin", Bytes::from(expected.clone()))
        .await
        .unwrap();

    let opened = store.open("streams/ledger.bin").await.unwrap();
    assert_eq!(opened.size, expected.len() as u64, "{driver}");
    let collected: Vec<u8> = opened
        .stream
        .map_ok(|chunk| chunk.to_vec())
        .try_concat()
        .await
        .unwrap();
    assert_eq!(collected, expected, "{driver}");

    assert!(
        matches!(
            store.open("streams/missing.bin").await,
            Err(StoreError::NotFound)
        ),
        "{driver}"
    );

    cleanup(&subject).await;
}

/// Proves `delete` removes a stored object and returns `StoreError::NotFound`
/// both when deleting a key that never existed and when deleting a second time.
pub async fn delete_removes_an_object_and_reports_not_found_when_missing(subject: Subject) {
    let store = &subject.store;
    let driver = subject.driver;

    assert!(
        matches!(
            store.delete("gone/never-there.txt").await,
            Err(StoreError::NotFound)
        ),
        "{driver}"
    );

    store
        .put("gone/receipt.txt", Bytes::from_static(b"Jaipur receipt"))
        .await
        .unwrap();
    assert_eq!(store.head("gone/receipt.txt").await.unwrap().size, 14);

    store.delete("gone/receipt.txt").await.unwrap();
    assert!(
        matches!(
            store.head("gone/receipt.txt").await,
            Err(StoreError::NotFound)
        ),
        "{driver}"
    );
    assert!(
        matches!(
            store.delete("gone/receipt.txt").await,
            Err(StoreError::NotFound)
        ),
        "{driver}"
    );

    cleanup(&subject).await;
}

/// Proves `list` treats prefixes as directory boundaries (`docs` never leaks
/// `docs2` or `docs.txt`) and follows continuation tokens across >1,000 keys.
pub async fn list_isolates_directory_prefixes_and_follows_continuation_pages(subject: Subject) {
    let store = &subject.store;
    let driver = subject.driver;

    for (path, body) in [
        ("docs/a.txt", "a"),
        ("docs/2026/summary.pdf", "pdf"),
        ("docs2/other.txt", "other"),
        ("docs.txt", "sibling"),
        ("root.txt", "root"),
    ] {
        store.put(path, Bytes::from(body)).await.unwrap();
    }

    let mut docs: Vec<String> = store
        .list("docs")
        .await
        .unwrap()
        .into_iter()
        .map(|meta| meta.path)
        .collect();
    docs.sort();
    assert_eq!(
        docs,
        vec!["docs/2026/summary.pdf", "docs/a.txt"],
        "{driver}"
    );

    let mut docs_slash: Vec<String> = store
        .list("docs/")
        .await
        .unwrap()
        .into_iter()
        .map(|meta| meta.path)
        .collect();
    docs_slash.sort();
    assert_eq!(docs_slash, docs, "{driver}");

    assert!(store.list("absent").await.unwrap().is_empty(), "{driver}");

    // Seed 1,025 keys under `pages/` to cross the 1,000-item page boundary on
    // S3 ListObjectsV2 and GCS objects.list and verify continuation/pagination.
    let total_pages_keys = 1_025usize;
    stream::iter(0..total_pages_keys)
        .for_each_concurrent(32, |index| async move {
            let key = format!("pages/item-{index:04}.txt");
            store.put(&key, Bytes::from_static(b"page")).await.unwrap();
        })
        .await;

    let mut paged: Vec<String> = store
        .list("pages")
        .await
        .unwrap()
        .into_iter()
        .map(|meta| meta.path)
        .collect();
    paged.sort();
    let expected: Vec<String> = (0..total_pages_keys)
        .map(|index| format!("pages/item-{index:04}.txt"))
        .collect();
    assert_eq!(paged.len(), total_pages_keys, "{driver}");
    assert_eq!(paged, expected, "{driver}");

    cleanup(&subject).await;
}

/// Proves `begin_upload` stages multi-part data invisibly until `finish`
/// publishes it, and `abort` discards already-uploaded parts leaving nothing.
pub async fn multipart_upload_publishes_on_finish_and_leaves_nothing_on_abort(subject: Subject) {
    let store = &subject.store;
    let driver = subject.driver;
    let one_mib = 1024 * 1024;

    // 5.5 MiB crosses S3's 5 MiB minimum part threshold, Azure's 4 MiB block
    // threshold, and GCS's resumable chunk threshold.
    let mut upload = store.begin_upload("multipart/complete.bin").await.unwrap();
    for index in 1..=5u8 {
        upload
            .write(Bytes::from(vec![index; one_mib]))
            .await
            .unwrap();
    }
    upload
        .write(Bytes::from(vec![6u8; 512 * 1024]))
        .await
        .unwrap();

    // Unfinished multipart uploads are never visible before `finish`.
    assert!(
        matches!(
            store.head("multipart/complete.bin").await,
            Err(StoreError::NotFound)
        ),
        "{driver}"
    );

    upload.finish().await.unwrap();

    let total = (5 * one_mib + 512 * 1024) as u64;
    let meta = store.head("multipart/complete.bin").await.unwrap();
    assert_eq!(meta.size, total, "{driver}");
    assert_eq!(
        store
            .read("multipart/complete.bin", Some(0..16))
            .await
            .unwrap(),
        Bytes::from(vec![1u8; 16]),
        "{driver}"
    );
    let boundary = (5 * one_mib - 4) as u64..(5 * one_mib + 4) as u64;
    assert_eq!(
        store
            .read("multipart/complete.bin", Some(boundary))
            .await
            .unwrap(),
        Bytes::from_static(&[5, 5, 5, 5, 6, 6, 6, 6]),
        "{driver}"
    );

    // An aborted multipart upload after parts have already been flushed leaves
    // no object behind.
    let mut aborted = store.begin_upload("multipart/aborted.bin").await.unwrap();
    for _ in 0..5 {
        aborted
            .write(Bytes::from(vec![9u8; one_mib]))
            .await
            .unwrap();
    }
    aborted
        .write(Bytes::from(vec![10u8; 64 * 1024]))
        .await
        .unwrap();
    aborted.abort().await.unwrap();

    assert!(
        matches!(
            store.head("multipart/aborted.bin").await,
            Err(StoreError::NotFound)
        ),
        "{driver}"
    );
    let listed: Vec<String> = store
        .list("multipart")
        .await
        .unwrap()
        .into_iter()
        .map(|m| m.path)
        .collect();
    assert_eq!(listed, vec!["multipart/complete.bin"], "{driver}");

    cleanup(&subject).await;
}

/// Proves `signed_url` GET and `signed_url_with` / `signed_url` PUT transfer
/// bytes directly between a plain `reqwest::Client` and the emulator.
pub async fn signed_get_and_put_links_transfer_bytes_over_plain_http(subject: Subject) {
    let store = &subject.store;
    let driver = subject.driver;
    let http = reqwest::Client::new();

    // 1. Upload via `put`, then fetch with plain HTTP GET through `signed_url`.
    let download_payload = Bytes::from_static("Agra ledger ₹2,50,000\n".as_bytes());
    store
        .put("signed/agra report.csv", download_payload.clone())
        .await
        .unwrap();
    let get_url = store
        .signed_url(
            "signed/agra report.csv",
            Method::GET,
            Duration::from_secs(300),
        )
        .await
        .unwrap();
    let get_reply = http.get(get_url.as_str()).send().await.unwrap();
    assert_eq!(get_reply.status().as_u16(), 200, "{driver}: {get_url}");
    assert_eq!(
        get_reply.bytes().await.unwrap(),
        download_payload,
        "{driver}"
    );

    // 2. Upload via a signed PUT link using plain HTTP, then read back via `Store`.
    let upload_payload = Bytes::from_static("Amritsar dispatch +91-9876543210\n".as_bytes());
    let bound_headers = [(
        HeaderName::from_static("content-type"),
        HeaderValue::from_static("text/plain; charset=utf-8"),
    )];
    if subject.binds_put_headers {
        let put_url = store
            .signed_url_with(
                "signed/amritsar upload.txt",
                Method::PUT,
                Duration::from_secs(300),
                &bound_headers,
            )
            .await
            .unwrap();
        let put_reply = http
            .put(put_url.as_str())
            .header("content-type", "text/plain; charset=utf-8")
            .body(upload_payload.clone())
            .send()
            .await
            .unwrap();
        assert!(
            put_reply.status().is_success(),
            "{driver}: status {} on {put_url}",
            put_reply.status()
        );
    } else {
        // Stores whose signed links cannot bind headers refuse `signed_url_with`
        // when headers are requested, and sign unbound PUT links cleanly.
        assert!(
            matches!(
                store
                    .signed_url_with(
                        "signed/amritsar upload.txt",
                        Method::PUT,
                        Duration::from_secs(300),
                        &bound_headers,
                    )
                    .await,
                Err(StoreError::Failed(_))
            ),
            "{driver}"
        );
        let put_url = store
            .signed_url(
                "signed/amritsar upload.txt",
                Method::PUT,
                Duration::from_secs(300),
            )
            .await
            .unwrap();
        let mut req = http.put(put_url.as_str()).body(upload_payload.clone());
        for &(name, value) in subject.unsigned_put_headers {
            req = req.header(name, value);
        }
        let put_reply = req.send().await.unwrap();
        assert!(
            put_reply.status().is_success(),
            "{driver}: status {} on {put_url}",
            put_reply.status()
        );
    }

    assert_eq!(
        store
            .read("signed/amritsar upload.txt", None)
            .await
            .unwrap(),
        upload_payload,
        "{driver}"
    );

    cleanup(&subject).await;
}

/// Proves special object keys (spaces, `+`, `%`, pre-encoded
/// `%20`/`%2F`, Gurmukhi, Devanagari, accented characters and a trailing `/`)
/// preserve their exact key and payload bytes across `put`, `head`, `read`,
/// `list` and a plain-HTTP `signed_url` GET.
pub async fn keys_with_spaces_plus_percent_unicode_and_trailing_slash_preserve_every_byte(
    subject: Subject,
) {
    let store = &subject.store;
    let driver = subject.driver;
    let http = reqwest::Client::new();

    let cases: [(&str, Bytes); 3] = [
        (
            "awkward/चालान ₹1,00,000 + 18% GST/INV 1101 (final) é.txt",
            Bytes::from_static("Harpreet Kaur — Ludhiana ₹1,18,000\n".as_bytes()),
        ),
        (
            "awkward/ਲੁਧਿਆਣਾ/ਰਸੀਦ%20+%2F100%.csv",
            Bytes::from_static("ਰਸੀਦ,ਰਕਮ\n42,50000\n".as_bytes()),
        ),
        // Zero-byte trailing-slash object key so POSIX-backed S3 emulators
        // (MinIO) as well as GCS and Azurite store and return the exact key.
        ("awkward/trailing/", Bytes::new()),
    ];

    for (key, payload) in &cases {
        store.put(key, payload.clone()).await.unwrap_or_else(|err| {
            panic!("{driver} put {key:?}: {err}");
        });

        let meta = store.head(key).await.unwrap_or_else(|err| {
            panic!("{driver} head {key:?}: {err}");
        });
        assert_eq!(meta.path, *key, "{driver}");
        assert_eq!(meta.size, payload.len() as u64, "{driver}: {key:?}");

        let got = store.read(key, None).await.unwrap_or_else(|err| {
            panic!("{driver} read {key:?}: {err}");
        });
        assert_eq!(&got, payload, "{driver}: {key:?}");

        let signed = store
            .signed_url(key, Method::GET, Duration::from_secs(300))
            .await
            .unwrap_or_else(|err| panic!("{driver} signed_url {key:?}: {err}"));
        let reply = http.get(signed.as_str()).send().await.unwrap();
        assert_eq!(
            reply.status().as_u16(),
            200,
            "{driver} GET signed URL for {key:?}: {signed}"
        );
        assert_eq!(&reply.bytes().await.unwrap(), payload, "{driver}: {key:?}");
    }

    let mut listed: Vec<(String, u64)> = store
        .list("awkward")
        .await
        .unwrap()
        .into_iter()
        .map(|meta| (meta.path, meta.size))
        .collect();
    listed.sort();

    let mut expected: Vec<(String, u64)> = cases
        .iter()
        .map(|(key, payload)| ((*key).to_owned(), payload.len() as u64))
        .collect();
    expected.sort();
    assert_eq!(listed, expected, "{driver}");

    cleanup(&subject).await;
}

/// Proves key refusal rules: `..`, `.`, empty keys and control
/// characters never escape their prefix or bucket and fail with
/// `StoreError::Failed` on every operation.
pub async fn keys_with_dot_segments_control_bytes_or_empty_names_never_leave_their_bucket_or_prefix(
    subject: Subject,
) {
    let driver = subject.driver;
    let prefixed: &dyn Store = subject.store.as_ref();
    let raw: &dyn Store = subject.raw.as_ref();

    // Seed a sentinel outside the test's prefix on `raw` to prove a `..`
    // path on `prefixed` can never read, overwrite or delete it.
    let sentinel_key = format!("sentinel-{}/keep.txt", navree_crypto::random_token(10));
    raw.put(&sentinel_key, Bytes::from_static(b"untouched"))
        .await
        .unwrap();

    let bad_keys = [
        "..",
        "../keep.txt",
        "../../other-bucket/keep.txt",
        "docs/../../keep.txt",
        "docs/..",
        ".",
        "docs/./keep.txt",
        "docs/\0bad.txt",
        "docs/\r\nbad.txt",
    ];

    for target in [prefixed, raw] {
        for key in bad_keys {
            assert!(
                matches!(
                    target.put(key, Bytes::from_static(b"nope")).await,
                    Err(StoreError::Failed(_))
                ),
                "{driver} put {key:?}"
            );
            assert!(
                matches!(target.head(key).await, Err(StoreError::Failed(_))),
                "{driver} head {key:?}"
            );
            assert!(
                matches!(target.read(key, None).await, Err(StoreError::Failed(_))),
                "{driver} read {key:?}"
            );
            assert!(
                matches!(target.open(key).await, Err(StoreError::Failed(_))),
                "{driver} open {key:?}"
            );
            assert!(
                matches!(target.delete(key).await, Err(StoreError::Failed(_))),
                "{driver} delete {key:?}"
            );
            assert!(
                matches!(
                    target.begin_upload(key).await.map(drop),
                    Err(StoreError::Failed(_))
                ),
                "{driver} begin_upload {key:?}"
            );
            assert!(
                matches!(
                    target
                        .signed_url(key, Method::GET, Duration::from_secs(60))
                        .await,
                    Err(StoreError::Failed(_))
                ),
                "{driver} signed_url {key:?}"
            );
            assert!(
                matches!(target.list(key).await, Err(StoreError::Failed(_))),
                "{driver} list {key:?}"
            );
        }
    }

    // Empty object keys are also refused directly on the raw driver.
    assert!(
        matches!(
            raw.put("", Bytes::from_static(b"nope")).await,
            Err(StoreError::Failed(_))
        ),
        "{driver} raw put empty"
    );
    assert!(
        matches!(raw.head("").await, Err(StoreError::Failed(_))),
        "{driver} raw head empty"
    );
    assert!(
        matches!(raw.delete("").await, Err(StoreError::Failed(_))),
        "{driver} raw delete empty"
    );

    // Sentinel outside the prefix remains intact, then we clean it up.
    assert_eq!(
        raw.read(&sentinel_key, None).await.unwrap(),
        Bytes::from_static(b"untouched"),
        "{driver}"
    );
    raw.delete(&sentinel_key).await.unwrap();
    cleanup(&subject).await;
}

/// Generates all nine named `#[tokio::test]` functions in a driver's test
/// crate, invoking `$connect(test_name).await` to obtain `Option<Subject>`.
#[macro_export]
macro_rules! declare_cloud_suite {
    ($connect:ident) => {
        #[tokio::test]
        async fn put_head_and_read_round_trip_bytes_and_metadata() {
            let Some(subject) =
                $connect("put_head_and_read_round_trip_bytes_and_metadata").await
            else {
                return;
            };
            cloud_suite::put_head_and_read_round_trip_bytes_and_metadata(subject).await;
        }

        #[tokio::test]
        async fn read_returns_exact_byte_ranges_and_clamps_past_the_end() {
            let Some(subject) =
                $connect("read_returns_exact_byte_ranges_and_clamps_past_the_end").await
            else {
                return;
            };
            cloud_suite::read_returns_exact_byte_ranges_and_clamps_past_the_end(subject).await;
        }

        #[tokio::test]
        async fn open_streams_the_whole_file_and_reports_its_size() {
            let Some(subject) =
                $connect("open_streams_the_whole_file_and_reports_its_size").await
            else {
                return;
            };
            cloud_suite::open_streams_the_whole_file_and_reports_its_size(subject).await;
        }

        #[tokio::test]
        async fn delete_removes_an_object_and_reports_not_found_when_missing() {
            let Some(subject) =
                $connect("delete_removes_an_object_and_reports_not_found_when_missing").await
            else {
                return;
            };
            cloud_suite::delete_removes_an_object_and_reports_not_found_when_missing(subject).await;
        }

        #[tokio::test]
        async fn list_isolates_directory_prefixes_and_follows_continuation_pages() {
            let Some(subject) =
                $connect("list_isolates_directory_prefixes_and_follows_continuation_pages").await
            else {
                return;
            };
            cloud_suite::list_isolates_directory_prefixes_and_follows_continuation_pages(subject)
                .await;
        }

        #[tokio::test]
        async fn multipart_upload_publishes_on_finish_and_leaves_nothing_on_abort() {
            let Some(subject) =
                $connect("multipart_upload_publishes_on_finish_and_leaves_nothing_on_abort").await
            else {
                return;
            };
            cloud_suite::multipart_upload_publishes_on_finish_and_leaves_nothing_on_abort(subject)
                .await;
        }

        #[tokio::test]
        async fn signed_get_and_put_links_transfer_bytes_over_plain_http() {
            let Some(subject) =
                $connect("signed_get_and_put_links_transfer_bytes_over_plain_http").await
            else {
                return;
            };
            cloud_suite::signed_get_and_put_links_transfer_bytes_over_plain_http(subject).await;
        }

        #[tokio::test]
        async fn keys_with_spaces_plus_percent_unicode_and_trailing_slash_preserve_every_byte() {
            let Some(subject) = $connect(
                "keys_with_spaces_plus_percent_unicode_and_trailing_slash_preserve_every_byte",
            )
            .await
            else {
                return;
            };
            cloud_suite::keys_with_spaces_plus_percent_unicode_and_trailing_slash_preserve_every_byte(
                subject,
            )
            .await;
        }

        #[tokio::test]
        async fn keys_with_dot_segments_control_bytes_or_empty_names_never_leave_their_bucket_or_prefix()
        {
            let Some(subject) = $connect(
                "keys_with_dot_segments_control_bytes_or_empty_names_never_leave_their_bucket_or_prefix",
            )
            .await
            else {
                return;
            };
            cloud_suite::keys_with_dot_segments_control_bytes_or_empty_names_never_leave_their_bucket_or_prefix(
                subject,
            )
            .await;
        }
    };
}
