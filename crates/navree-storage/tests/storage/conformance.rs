//! The disk conformance suite: every driver (memory, local, the in-process
//! S3 server, MinIO when `NAVREE_TEST_S3_URL` is set) must behave the same.
//! [`run`] runs every case on one disk, emptying it between cases.

use std::collections::BTreeMap;

use axum::Router;
use axum::body::Body;
use axum::http::Request;
use bytes::Bytes;
use chrono::{Duration, Utc};
use futures_util::{TryStreamExt, stream};
use http_body_util::BodyExt;
use navree_contracts::HealthStatus;
use navree_storage::{
    Disk, DiskConfig, Storage, StorageError, SyncOptions, UploadRejection, UploadRules, upload_path,
};
use tower::ServiceExt;

use crate::support::{SIGNING_KEY, single_disk};

/// The disk under test, and how to reach the links it signs.
pub struct Subject {
    pub disk: Disk,
    pub http: Http,
}

/// Sends requests to signed links: the app's router for path-only links of
/// local and memory disks, the network for absolute ones.
pub struct Http {
    pub router: Option<Router>,
    pub client: Option<reqwest::Client>,
}

/// A response to a signed link.
#[derive(Debug)]
pub struct Reply {
    pub status: u16,
    pub body: Bytes,
}

impl Http {
    /// `http` for a cloud disk, whose links are absolute. Build the disk
    /// first: it installs the TLS crypto provider reqwest needs.
    #[cfg_attr(not(feature = "s3"), allow(dead_code))]
    pub fn network() -> Self {
        Self {
            router: None,
            client: Some(reqwest::Client::new()),
        }
    }

    /// `http` for an app's disks.
    pub fn app(router: Router) -> Self {
        Self {
            router: Some(router),
            client: None,
        }
    }

    pub async fn send(
        &self,
        method: &str,
        url: &str,
        headers: &BTreeMap<String, String>,
        body: impl Into<Bytes>,
    ) -> Reply {
        let body = body.into();
        if url.starts_with('/') {
            let router = self.router.clone().expect("an app for path-only links");
            let mut request = Request::builder().method(method).uri(url);
            for (name, value) in headers {
                request = request.header(name, value);
            }
            let request = request
                .header("content-length", body.len())
                .body(Body::from(body))
                .unwrap();
            let response = router.oneshot(request).await.unwrap();
            let status = response.status().as_u16();
            let body = response.into_body().collect().await.unwrap().to_bytes();
            return Reply { status, body };
        }
        let client = self.client.as_ref().expect("a client for absolute links");
        let mut request = client.request(method.parse().unwrap(), url).body(body);
        for (name, value) in headers {
            request = request.header(name, value);
        }
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        let body = response.bytes().await.unwrap();
        Reply { status, body }
    }

    pub async fn get(&self, url: &str) -> Reply {
        self.send("GET", url, &BTreeMap::new(), Bytes::new()).await
    }
}

/// A PNG header padded to `len` bytes.
pub fn png(len: usize) -> Vec<u8> {
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    png.resize(len, 0);
    png
}

/// One case of the suite.
type Case = fn(&Subject) -> futures_util::future::BoxFuture<'_, ()>;

/// Runs every case on `subject`.
pub async fn run(subject: &Subject) {
    let cases: [(&str, Case); 12] = [
        ("put_get_delete", |s| Box::pin(put_get_delete(s))),
        ("streams", |s| Box::pin(streams(s))),
        ("failed_stream_leaves_nothing", |s| {
            Box::pin(failed_stream_leaves_nothing(s))
        }),
        ("rejects_traversal", |s| Box::pin(rejects_traversal(s))),
        ("normalises_paths", |s| Box::pin(normalises_paths(s))),
        ("lists_by_directory", |s| Box::pin(lists_by_directory(s))),
        ("metadata", |s| Box::pin(metadata(s))),
        ("temporary_url_downloads", |s| {
            Box::pin(temporary_url_downloads(s))
        }),
        ("presigned_put_uploads", |s| {
            Box::pin(presigned_put_uploads(s))
        }),
        ("presigned_put_binds_the_content_type", |s| {
            Box::pin(presigned_put_binds_the_content_type(s))
        }),
        ("accept_upload_rejects_and_deletes", |s| {
            Box::pin(accept_upload_rejects_and_deletes(s))
        }),
        ("sync_both_ways", |s| Box::pin(sync_both_ways(s))),
    ];
    for (name, case) in cases {
        clear(&subject.disk).await;
        println!("conformance on `{}`: {name}", subject.disk.driver());
        case(subject).await;
    }
    clear(&subject.disk).await;
    assert_eq!(
        subject.disk.check_health().await,
        HealthStatus::Up,
        "health on `{}`",
        subject.disk.driver()
    );
    assert!(
        subject.disk.list("").await.unwrap().is_empty(),
        "the probe cleans up"
    );
}

/// Deletes every file of the disk.
pub async fn clear(disk: &Disk) {
    for path in disk.list("").await.unwrap() {
        disk.delete(&path).await.unwrap();
    }
}

async fn put_get_delete(subject: &Subject) {
    let disk = &subject.disk;
    let driver = disk.driver();
    disk.put("docs/readme.txt", Bytes::from_static(b"hello"))
        .await
        .unwrap();
    assert!(disk.exists("docs/readme.txt").await.unwrap(), "{driver}");
    assert_eq!(
        disk.get("docs/readme.txt").await.unwrap(),
        "hello",
        "{driver}"
    );
    assert_eq!(disk.size("docs/readme.txt").await.unwrap(), 5, "{driver}");

    disk.put("docs/readme.txt", "hello again").await.unwrap();
    assert_eq!(disk.get("docs/readme.txt").await.unwrap(), "hello again");

    disk.delete("docs/readme.txt").await.unwrap();
    assert!(!disk.exists("docs/readme.txt").await.unwrap(), "{driver}");
    for missing in [
        disk.get("docs/readme.txt").await.err(),
        disk.size("docs/readme.txt").await.err(),
        disk.get_stream("docs/readme.txt").await.err(),
        disk.metadata("docs/readme.txt").await.err(),
    ] {
        assert!(
            matches!(missing, Some(StorageError::NotFound { .. })),
            "{driver}: {missing:?}"
        );
    }
    disk.delete("docs/readme.txt").await.unwrap();
    // An empty file is a file.
    disk.put("empty.txt", Bytes::new()).await.unwrap();
    assert_eq!(disk.size("empty.txt").await.unwrap(), 0, "{driver}");
}

async fn streams(subject: &Subject) {
    let disk = &subject.disk;
    let chunks: Vec<Result<Bytes, std::io::Error>> =
        (0..64u8).map(|i| Ok(Bytes::from(vec![i; 1024]))).collect();
    let written = disk
        .put_stream("big/data.bin", stream::iter(chunks))
        .await
        .unwrap();
    assert_eq!(written, 64 * 1024, "{}", disk.driver());
    let read: Vec<u8> = disk
        .get_stream("big/data.bin")
        .await
        .unwrap()
        .map_ok(|chunk| chunk.to_vec())
        .try_concat()
        .await
        .unwrap();
    assert_eq!(read.len(), 64 * 1024);
    assert!(read[..1024].iter().all(|&b| b == 0));
    assert!(read[63 * 1024..].iter().all(|&b| b == 63));

    let empty = disk
        .put_stream(
            "big/empty.bin",
            stream::iter(Vec::<Result<Bytes, std::io::Error>>::new()),
        )
        .await
        .unwrap();
    assert_eq!(empty, 0);
    assert_eq!(disk.size("big/empty.bin").await.unwrap(), 0);
}

async fn failed_stream_leaves_nothing(subject: &Subject) {
    let disk = &subject.disk;
    let chunks: Vec<Result<Bytes, std::io::Error>> = vec![
        Ok(Bytes::from_static(b"partial")),
        Err(std::io::Error::other("client went away")),
    ];
    let error = disk
        .put_stream("broken.bin", stream::iter(chunks))
        .await
        .unwrap_err();
    assert!(matches!(error, StorageError::Stream { .. }), "{error:?}");
    assert!(
        !disk.exists("broken.bin").await.unwrap(),
        "{}",
        disk.driver()
    );
    assert!(disk.list("").await.unwrap().is_empty(), "{}", disk.driver());
}

async fn rejects_traversal(subject: &Subject) {
    let disk = &subject.disk;
    for path in ["../secret.txt", r"a\..\..\b", "..", "a/. ./b", "x\0y", ""] {
        let invalid = |result: Result<(), StorageError>| {
            assert!(
                matches!(result, Err(StorageError::InvalidPath { .. })),
                "{} {path:?}: {result:?}",
                disk.driver()
            );
        };
        invalid(disk.put(path, "pwned").await);
        invalid(disk.get(path).await.map(drop));
        invalid(disk.delete(path).await);
        invalid(disk.metadata(path).await.map(drop));
        invalid(disk.temporary_url(path, Utc::now()).await.map(drop));
        let rules = UploadRules::new(10, ["text/plain"]);
        invalid(
            disk.presigned_put(path, Duration::minutes(1), &rules)
                .content_type("text/plain")
                .await
                .map(drop),
        );
        invalid(disk.accept_upload(path, &rules).await.map(drop));
    }
    assert!(disk.list("").await.unwrap().is_empty());
}

async fn normalises_paths(subject: &Subject) {
    let disk = &subject.disk;
    disk.put("/reports//2026/./q1.csv", "a,b").await.unwrap();
    assert_eq!(disk.get("reports/2026/q1.csv").await.unwrap(), "a,b");
    assert_eq!(disk.get(r"reports\2026\q1.csv").await.unwrap(), "a,b");
    disk.put("my files/report (final) #2+ü.txt", "ok")
        .await
        .unwrap();
    assert_eq!(
        disk.list("my files").await.unwrap(),
        vec!["my files/report (final) #2+ü.txt"],
        "{}",
        disk.driver()
    );
    assert_eq!(
        disk.get("my files/report (final) #2+ü.txt").await.unwrap(),
        "ok"
    );
}

async fn lists_by_directory(subject: &Subject) {
    let disk = &subject.disk;
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
    assert_eq!(
        disk.list("docs").await.unwrap(),
        vec!["docs/2026/report.pdf", "docs/a.txt", "docs/b.txt"],
        "{}",
        disk.driver()
    );
    assert_eq!(disk.list("/docs/").await.unwrap().len(), 3);
    assert_eq!(disk.list("").await.unwrap().len(), 6);
    assert!(disk.list("nothing/here").await.unwrap().is_empty());
}

async fn metadata(subject: &Subject) {
    let disk = &subject.disk;
    let before = Utc::now() - Duration::minutes(5);
    disk.put("a/one.txt", "1").await.unwrap();
    disk.put("a/three.txt", "333").await.unwrap();
    let meta = disk.metadata("a/three.txt").await.unwrap();
    assert_eq!((meta.path.as_str(), meta.size), ("a/three.txt", 3));
    assert!(meta.last_modified > before, "{meta:?}");
    let files = disk.files("a").await.unwrap();
    let listed: Vec<(&str, u64)> = files
        .iter()
        .map(|file| (file.path.as_str(), file.size))
        .collect();
    assert_eq!(listed, [("a/one.txt", 1), ("a/three.txt", 3)]);
}

async fn temporary_url_downloads(subject: &Subject) {
    let disk = &subject.disk;
    disk.put("reports/q1 final.csv", "region,total\n")
        .await
        .unwrap();
    let url = disk
        .temporary_url("reports/q1 final.csv", Utc::now() + Duration::minutes(5))
        .await
        .unwrap();
    let reply = subject.http.get(&url).await;
    assert_eq!(reply.status, 200, "{}: {url}: {reply:?}", disk.driver());
    assert_eq!(reply.body, "region,total\n");

    // A tampered link is refused.
    let tampered = url.replacen("q1%20final", "q2%20final", 1);
    assert_ne!(tampered, url);
    let reply = subject.http.get(&tampered).await;
    assert_eq!(reply.status, 403, "{}: {tampered}", disk.driver());
}

async fn presigned_put_uploads(subject: &Subject) {
    let disk = &subject.disk;
    let rules = UploadRules::new(64 * 1024, ["image/png", "image/jpeg"]);
    let path = upload_path("avatars", "image/png");
    let body = png(2000);
    let upload = disk
        .presigned_put(&path, Duration::minutes(10), &rules)
        .content_type("image/png")
        .size(body.len() as u64)
        .await
        .unwrap();
    assert_eq!(upload.disk, disk.name());
    assert_eq!(upload.path, path);
    assert_eq!(upload.method, "PUT");
    assert_eq!(
        upload.headers.get("content-type").map(String::as_str),
        Some("image/png")
    );
    assert!(upload.expires_at > Utc::now() + Duration::minutes(9));

    let reply = subject
        .http
        .send(&upload.method, &upload.url, &upload.headers, body.clone())
        .await;
    assert!(
        (200..300).contains(&reply.status),
        "{}: {reply:?}",
        disk.driver()
    );
    assert_eq!(disk.get(&path).await.unwrap(), body);

    let stored = disk.accept_upload(&path, &rules).await.unwrap();
    assert_eq!(stored.disk, disk.name());
    assert_eq!(stored.path, path);
    assert_eq!(stored.size, 2000);
    assert_eq!(stored.mime, "image/png");

    // Declared values that break the rules are refused before signing.
    let rejection = |result: Result<_, StorageError>| match result {
        Err(StorageError::Rejected { rejection, .. }) => rejection,
        other => panic!("{}: must be rejected: {other:?}", disk.driver()),
    };
    let signed = |content_type: Option<&'static str>, size: Option<u64>| {
        let rules = &rules;
        async move {
            let mut request = disk.presigned_put("x.png", Duration::minutes(1), rules);
            if let Some(content_type) = content_type {
                request = request.content_type(content_type);
            }
            if let Some(size) = size {
                request = request.size(size);
            }
            request.await
        }
    };
    assert_eq!(
        rejection(signed(Some("image/png"), Some(64 * 1024 + 1)).await),
        UploadRejection::TooLarge {
            max_bytes: 64 * 1024
        }
    );
    assert_eq!(
        rejection(signed(Some("image/png"), Some(0)).await),
        UploadRejection::Empty
    );
    assert!(matches!(
        rejection(signed(Some("text/html"), None).await),
        UploadRejection::Type { mime, .. } if mime == "text/html"
    ));
    assert!(matches!(
        rejection(signed(None, Some(10)).await),
        UploadRejection::MissingType { .. }
    ));
}

async fn presigned_put_binds_the_content_type(subject: &Subject) {
    let disk = &subject.disk;
    let rules = UploadRules::new(1024, ["image/png"]);
    let upload = disk
        .presigned_put("bound.png", Duration::minutes(10), &rules)
        .content_type("image/png")
        .await
        .unwrap();
    let mut headers = upload.headers.clone();
    headers.insert("content-type".into(), "text/html".into());
    let reply = subject
        .http
        .send("PUT", &upload.url, &headers, "<script>alert(1)</script>")
        .await;
    assert_eq!(reply.status, 403, "{}: {reply:?}", disk.driver());
    assert!(
        !disk.exists("bound.png").await.unwrap(),
        "{}",
        disk.driver()
    );
}

async fn accept_upload_rejects_and_deletes(subject: &Subject) {
    let disk = &subject.disk;
    let rules = UploadRules::new(1000, ["image/png"]);

    disk.put("up/fake.png", "<svg onload=alert(1)>")
        .await
        .unwrap();
    let error = disk.accept_upload("up/fake.png", &rules).await.unwrap_err();
    assert!(
        matches!(&error, StorageError::Rejected { rejection: UploadRejection::Type { mime, .. }, .. } if mime == "text/plain"),
        "{}: {error:?}",
        disk.driver()
    );
    assert!(
        !disk.exists("up/fake.png").await.unwrap(),
        "rejected files are deleted"
    );

    disk.put("up/big.png", png(1001)).await.unwrap();
    let error = disk.accept_upload("up/big.png", &rules).await.unwrap_err();
    assert!(matches!(
        error,
        StorageError::Rejected {
            rejection: UploadRejection::TooLarge { max_bytes: 1000 },
            ..
        }
    ));
    assert!(!disk.exists("up/big.png").await.unwrap());

    disk.put("up/empty.png", Bytes::new()).await.unwrap();
    assert!(matches!(
        disk.accept_upload("up/empty.png", &rules).await,
        Err(StorageError::Rejected {
            rejection: UploadRejection::Empty,
            ..
        })
    ));

    assert!(matches!(
        disk.accept_upload("up/missing.png", &rules).await,
        Err(StorageError::NotFound { .. })
    ));

    // Within the rules, and over 8 KiB: only the head is read to sniff it.
    let rules = UploadRules::new(100_000, ["image/*"]);
    disk.put("up/real.png", png(50_000)).await.unwrap();
    let stored = disk.accept_upload("up/real.png", &rules).await.unwrap();
    assert_eq!((stored.size, stored.mime.as_str()), (50_000, "image/png"));
    assert!(disk.exists("up/real.png").await.unwrap());
}

async fn sync_both_ways(subject: &Subject) {
    let disk = &subject.disk;
    let scratch = Storage::new(&single_disk("scratch", DiskConfig::Memory), SIGNING_KEY)
        .unwrap()
        .default_disk();
    scratch.put("a.txt", "a").await.unwrap();
    scratch.put("dir/b.txt", "bb").await.unwrap();
    scratch.put(".navree-health/probe", "ok").await.unwrap();

    // Memory to the subject.
    let report = scratch
        .sync_to(disk, &SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(report.copied, ["a.txt", "dir/b.txt"], "{}", disk.driver());
    assert_eq!(report.bytes, 3);
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert_eq!(disk.get("dir/b.txt").await.unwrap(), "bb");
    assert!(!disk.exists(".navree-health/probe").await.unwrap());

    // Nothing to do the second time.
    let again = scratch
        .sync_to(disk, &SyncOptions::default())
        .await
        .unwrap();
    assert!(again.copied.is_empty());
    assert_eq!(again.unchanged, 2);

    // The subject back to memory, deleting what memory has extra.
    disk.put("dir/c.txt", "ccc").await.unwrap();
    scratch.put("stale.txt", "old").await.unwrap();
    let report = disk
        .sync_to(
            &scratch,
            &SyncOptions {
                delete: true,
                ..SyncOptions::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(report.copied, ["dir/c.txt"], "{}", disk.driver());
    assert_eq!(report.deleted, ["stale.txt"]);
    assert_eq!(scratch.get("dir/c.txt").await.unwrap(), "ccc");
    assert!(!scratch.exists("stale.txt").await.unwrap());
}
