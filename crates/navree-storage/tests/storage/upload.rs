//! `store_upload`: streaming multipart fields to a disk with size and MIME checks.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Multipart, State};
use axum::http::Request;
use axum::routing::post;
use bytes::Bytes;
use futures_util::StreamExt;
use http_body_util::BodyExt;
use navree_http::{AppError, Json};
use navree_storage::{Disk, StoredFile, UploadRules, store_upload};
use serde_json::Value;
use tower::ServiceExt;

use crate::support::{DISKS, storage};

/// The first bytes of a PNG file.
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";

/// The first bytes of a Windows executable.
const EXE: &[u8] = b"MZ\x90\0\x03\0\0\0\x04\0\0\0\xff\xff\0\0";

fn png(len: usize) -> Vec<u8> {
    let mut bytes = PNG.to_vec();
    bytes.resize(len, 7);
    bytes
}

struct Upload {
    disk: Disk,
    rules: UploadRules,
}

async fn upload(
    State(state): State<Arc<Upload>>,
    mut multipart: Multipart,
) -> Result<Json<StoredFile>, AppError> {
    let field = multipart
        .next_field()
        .await?
        .ok_or_else(|| AppError::bad_request("no file"))?;
    let stored = store_upload(field, &state.disk, "avatars", state.rules.clone()).await?;
    Ok(Json(stored))
}

fn router(disk: Disk, rules: UploadRules, body_limit: usize) -> Router {
    Router::new()
        .route("/upload", post(upload))
        .layer(DefaultBodyLimit::max(body_limit))
        .with_state(Arc::new(Upload { disk, rules }))
}

fn router_with(disk: Disk, rules: UploadRules) -> Router {
    router(disk, rules, 1024 * 1024)
}

const BOUNDARY: &str = "navree-test-boundary";

fn multipart(field: &str, file_name: &str, content_type: &str, contents: &[u8]) -> Request<Body> {
    let mut body = format!(
        "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{field}\"; \
         filename=\"{file_name}\"\r\nContent-Type: {content_type}\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(contents);
    body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
    // Sent in 1 KiB chunks that arrive one at a time, like a network upload
    // (multer reads everything that is ready at once). The first chunk is
    // ready immediately: multer 3.1 reports an incomplete stream when its
    // first poll is pending and the whole body then arrives with the end.
    let chunks: Vec<Bytes> = body.chunks(1024).map(Bytes::copy_from_slice).collect();
    let slow = futures_util::stream::iter(chunks)
        .enumerate()
        .then(|(i, chunk)| async move {
            if i > 0 {
                tokio::task::yield_now().await;
            }
            Ok::<_, std::io::Error>(chunk)
        });
    Request::post("/upload")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(Body::from_stream(slow))
        .unwrap()
}

async fn send(router: &Router, request: Request<Body>) -> (u16, Value) {
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status().as_u16();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json = serde_json::from_slice(&body)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&body)));
    (status, json)
}

fn images(max_bytes: u64) -> UploadRules {
    UploadRules::new(max_bytes, ["image/png", "image/jpeg"])
}

#[tokio::test]
async fn upload_streams_the_file_under_a_random_name() {
    let root = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let storage = storage(root.path());

    for name in DISKS {
        let disk = storage.disk(name).unwrap();
        let router = router(disk.clone(), images(1024 * 1024), 2 * 1024 * 1024);
        // The client claims a JPEG; the content is a PNG.
        let contents = png(300 * 1024);
        let (status, body) = send(
            &router,
            multipart("avatar", "C:\\fakepath\\me.jpg", "image/jpeg", &contents),
        )
        .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["disk"], name);
        assert_eq!(body["mime"], "image/png");
        assert_eq!(body["size"], 300 * 1024);
        assert_eq!(body["original_name"], "me.jpg");
        let path = body["path"].as_str().unwrap();
        let file = path.strip_prefix("avatars/").expect("stored under the dir");
        let (stem, extension) = file.split_once('.').unwrap();
        assert_eq!(extension, "png");
        assert_eq!(stem.len(), 40);
        assert!(stem.bytes().all(|b| b.is_ascii_alphanumeric()), "{stem}");
        assert_eq!(disk.get(path).await.unwrap().as_ref(), contents.as_slice());
    }
}

#[tokio::test]
async fn upload_rejects_wrong_mime_and_too_large() {
    let root = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let storage = storage(root.path());

    for name in DISKS {
        let disk = storage.disk(name).unwrap();
        let router = router_with(disk.clone(), images(16 * 1024));

        // Text renamed to `.png` with a PNG content type: sniffed as text.
        let (status, body) = send(
            &router,
            multipart(
                "avatar",
                "evil.png",
                "image/png",
                b"definitely not an image",
            ),
        )
        .await;
        assert_eq!(status, 400, "{body}");
        assert_eq!(
            body,
            serde_json::json!({
                "statusCode": 400,
                "message": ["avatar must be a file of type: image/png, image/jpeg"],
                "error": "Bad Request",
                "code": "validation_failed",
                "errors": {"avatar": ["avatar must be a file of type: image/png, image/jpeg"]},
            })
        );

        // An executable claiming to be a JPEG.
        let (status, body) = send(&router, multipart("avatar", "cat.jpg", "image/jpeg", EXE)).await;
        assert_eq!(status, 400, "{body}");

        // A real PNG over the limit, detected while streaming.
        let (status, body) = send(
            &router,
            multipart("avatar", "big.png", "image/png", &png(64 * 1024)),
        )
        .await;
        assert_eq!(status, 413, "{body}");
        assert_eq!(
            body,
            serde_json::json!({
                "statusCode": 413,
                "message": "avatar must not be larger than 16384 bytes",
                "error": "Payload Too Large",
                "code": "payload_too_large",
            })
        );
        let (status, _) = send(
            &router,
            multipart("avatar", "big.png", "image/png", &png(16 * 1024 + 1)),
        )
        .await;
        assert_eq!(status, 413);

        // Exactly at the limit is fine.
        let (status, accepted) = send(
            &router,
            multipart("avatar", "ok.png", "image/png", &png(16 * 1024)),
        )
        .await;
        assert_eq!(status, 200, "{accepted}");

        // Over a limit smaller than the sniffed head.
        let tiny = router_with(disk.clone(), images(100));
        let (status, _) = send(&tiny, multipart("avatar", "a.png", "image/png", &png(101))).await;
        assert_eq!(status, 413);

        // Nothing but the accepted file was stored.
        let stored = disk.list("").await.unwrap();
        assert_eq!(stored, vec![accepted["path"].as_str().unwrap()], "{name}");
    }
    // No staging files are left behind.
    let leftovers: Vec<_> = std::fs::read_dir(root.path().join("avatars"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(leftovers.len(), 1, "{leftovers:?}");
}

#[tokio::test]
async fn upload_over_the_request_body_limit_is_payload_too_large() {
    let root = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let disk = storage(root.path()).disk("memory").unwrap();
    let router = router(disk.clone(), images(1024 * 1024), 16 * 1024);

    let (status, body) = send(
        &router,
        multipart("avatar", "big.png", "image/png", &png(64 * 1024)),
    )
    .await;
    assert_eq!(status, 413, "{body}");
    assert!(disk.list("").await.unwrap().is_empty());
}

#[tokio::test]
async fn text_uploads_take_their_type_from_the_file_name() {
    let root = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let disk = storage(root.path()).disk("memory").unwrap();
    let csv = UploadRules::new(1024, ["text/csv"]);
    let router = router(disk.clone(), csv, 1024 * 1024);

    let (status, body) = send(
        &router,
        multipart(
            "import",
            "contacts.csv",
            "text/csv",
            b"name,email\nJane,j@x.io\n",
        ),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["mime"], "text/csv");
    assert!(body["path"].as_str().unwrap().ends_with(".csv"));

    // A text file named like an image is plain text, not an image.
    let any = UploadRules::new(1024, Vec::<String>::new());
    let router = self::router(disk.clone(), any, 1024 * 1024);
    let (status, body) = send(
        &router,
        multipart(
            "file",
            "logo.svg",
            "image/svg+xml",
            b"<svg onload=\"alert(1)\"></svg>",
        ),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["mime"], "text/plain");
    assert!(body["path"].as_str().unwrap().ends_with(".txt"));

    // Unrecognised binary data.
    let (status, body) = send(
        &router,
        multipart(
            "file",
            "blob.dat",
            "application/x-thing",
            b"\0\x01\x02\x03binary",
        ),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["mime"], "application/octet-stream");
    assert!(body["path"].as_str().unwrap().ends_with(".bin"));
}

#[tokio::test]
async fn wildcard_types_and_empty_files() {
    let root = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let disk = storage(root.path()).disk("memory").unwrap();
    let router = router(
        disk.clone(),
        UploadRules::new(1024, ["image/*"]),
        1024 * 1024,
    );

    let (status, body) = send(&router, multipart("photo", "a.png", "image/png", &png(100))).await;
    assert_eq!(status, 200, "{body}");

    let (status, body) = send(&router, multipart("photo", "a.png", "image/png", b"")).await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["errors"]["photo"][0], "photo should not be empty");
}

#[test]
fn upload_rules_match_types_case_insensitively() {
    let rules = UploadRules::new(10, ["image/*", "application/PDF"]);
    assert!(rules.allows("image/png"));
    assert!(rules.allows("IMAGE/JPEG"));
    assert!(rules.allows("application/pdf"));
    assert!(!rules.allows("application/zip"));
    assert!(!rules.allows("imagex/png"));
    assert!(UploadRules::new(10, Vec::<String>::new()).allows("anything/else"));
}
