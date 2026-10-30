//! The signed download route `GET /storage/{disk}/{*path}`.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use navree_contracts::{Clock, FixedClock};
use navree_kernel::Navree;
use navree_storage::{Storage, StorageModule};

use crate::support::{TestApp, options};

async fn boot(root: &std::path::Path, clock: Arc<FixedClock>) -> TestApp {
    TestApp::boot(
        Navree::app()
            .module(StorageModule::for_root(options(root)))
            .override_value::<dyn Clock>(clock),
    )
    .await
}

#[tokio::test]
async fn temporary_url_valid_then_expired() {
    let root = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let clock = Arc::new(FixedClock::new(Utc::now()));
    let app = boot(root.path(), clock.clone()).await;
    let storage = app.app.ctx().get::<Storage>().await.unwrap();
    let disk = storage.disk("local").unwrap();
    disk.put("reports/q1 final.pdf", "%PDF-1.7 quarterly report")
        .await
        .unwrap();

    let url = disk
        .temporary_url(
            "reports/q1 final.pdf",
            clock.now() + chrono::Duration::seconds(60),
        )
        .await
        .unwrap();

    let ok = app.get(&url).await;
    assert_eq!(ok.status, 200, "{}", ok.text());
    assert_eq!(ok.text(), "%PDF-1.7 quarterly report");
    assert_eq!(ok.header("content-type"), Some("application/pdf"));
    assert_eq!(ok.header("content-length"), Some("25"));
    assert_eq!(ok.header("x-content-type-options"), Some("nosniff"));

    clock.advance(Duration::from_secs(59));
    assert_eq!(app.get(&url).await.status, 200, "still valid");

    clock.advance(Duration::from_secs(1));
    let expired = app.get(&url).await.error(403);
    assert_eq!(
        expired,
        serde_json::json!({
            "statusCode": 403,
            "message": "The link has expired.",
            "error": "Forbidden",
            "code": "invalid_signature",
        })
    );
}

#[tokio::test]
async fn tampered_or_unsigned_urls_are_forbidden() {
    let root = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let clock = Arc::new(FixedClock::new(Utc::now()));
    let app = boot(root.path(), clock.clone()).await;
    let storage = app.app.ctx().get::<Storage>().await.unwrap();
    let disk = storage.disk("memory").unwrap();
    disk.put("a.txt", "secret a").await.unwrap();
    disk.put("b.txt", "secret b").await.unwrap();
    let expires_at = clock.now() + chrono::Duration::seconds(60);
    let url = disk.temporary_url("a.txt", expires_at).await.unwrap();
    assert_eq!(app.get(&url).await.text(), "secret a");

    let later = (expires_at + chrono::Duration::days(1)).timestamp();
    let tampered = [
        url.replace("a.txt", "b.txt"),
        url.replace("/memory/", "/local/"),
        url.replace(
            &format!("expires={}", expires_at.timestamp()),
            &format!("expires={later}"),
        ),
        url.split('&').next().unwrap().to_owned(),
        "/storage/memory/a.txt".to_owned(),
        format!("{url}&signature=00"),
        url.replace("signature=", "signature=zz"),
    ];
    for uri in tampered {
        let body = app.get(&uri).await.error(403);
        assert_eq!(body["message"], "Invalid signature.", "{uri}");
    }
}

#[tokio::test]
async fn signed_url_for_missing_file_is_not_found() {
    let root = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let clock = Arc::new(FixedClock::new(Utc::now()));
    let app = boot(root.path(), clock.clone()).await;
    let storage = app.app.ctx().get::<Storage>().await.unwrap();
    let expires_at = clock.now() + chrono::Duration::seconds(60);

    let url = storage
        .disk("local")
        .unwrap()
        .temporary_url("gone.txt", expires_at)
        .await
        .unwrap();
    app.get(&url).await.error(404);

    // Without a valid signature nothing is revealed: an unknown disk or a
    // traversal attempt looks like a forged link.
    for uri in [
        "/storage/nope/a.txt?expires=99999999999&signature=00",
        "/storage/local/..%2F..%2Fsecret.txt?expires=99999999999&signature=00",
        "/storage/local/a%5C..%5C..%5Csecret.txt?expires=99999999999&signature=00",
    ] {
        let body = app.get(uri).await.error(403);
        assert_eq!(body["message"], "Invalid signature.", "{uri}");
    }
}

#[tokio::test]
async fn active_content_is_served_as_an_attachment() {
    let root = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let clock = Arc::new(FixedClock::new(Utc::now()));
    let app = boot(root.path(), clock.clone()).await;
    let disk = app.app.ctx().get::<Storage>().await.unwrap().default_disk();
    let expires_at = clock.now() + chrono::Duration::seconds(60);

    disk.put("page.html", "<html><script>alert(1)</script></html>")
        .await
        .unwrap();
    let html = app
        .get(&disk.temporary_url("page.html", expires_at).await.unwrap())
        .await;
    assert_eq!(html.status, 200);
    assert_eq!(html.header("content-type"), Some("text/html"));
    assert_eq!(html.header("content-disposition"), Some("attachment"));

    disk.put("logo.png", "png").await.unwrap();
    let png = app
        .get(&disk.temporary_url("logo.png", expires_at).await.unwrap())
        .await;
    assert_eq!(png.header("content-type"), Some("image/png"));
    assert_eq!(png.header("content-disposition"), None);

    disk.put("data.unknownext", "?").await.unwrap();
    let unknown = app
        .get(
            &disk
                .temporary_url("data.unknownext", expires_at)
                .await
                .unwrap(),
        )
        .await;
    assert_eq!(
        unknown.header("content-type"),
        Some("application/octet-stream")
    );
}
