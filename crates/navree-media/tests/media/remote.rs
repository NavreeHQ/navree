//! Remote files: downloaded through the HTTP client and its SSRF guard,
//! against a local mock server.

use std::time::Duration;

use navree_http_client::{HttpClient, HttpClientOptions, SsrfPolicy};
use navree_media::{Media, MediaError, MediaOptions, MediaSource};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::models::{Course, storage};
use crate::support::svg_logo;

/// A client that may reach the mock server on 127.0.0.1.
fn local_client() -> HttpClient {
    HttpClient::new(HttpClientOptions {
        ssrf: SsrfPolicy {
            allow_hosts: vec!["127.0.0.1".into()],
            ..SsrfPolicy::default()
        },
        ..HttpClientOptions::default()
    })
    .unwrap()
}

fn media(client: Option<HttpClient>, options: MediaOptions) -> (Media, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut builder = Media::builder(storage(dir.path())).options(options);
    if let Some(client) = client {
        builder = builder.http_client(client);
    }
    (builder.build().unwrap(), dir)
}

#[tokio::test]
async fn remote_files_are_downloaded_and_named() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/brand/logo%20dark.svg"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(svg_logo(120, 40)))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/download"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-disposition", "attachment; filename=\"terms.txt\"")
                .set_body_string("Terms"),
        )
        .mount(&server)
        .await;
    let (media, _dir) = media(Some(local_client()), MediaOptions::default());
    let course = Course { id: 1 };
    let item = media
        .add_from_url(
            &course,
            "thumbnail",
            format!("{}/brand/logo%20dark.svg?v=3", server.uri()),
        )
        .save(None)
        .await
        .unwrap();
    assert_eq!(item.file_name, "logo dark.svg");
    assert_eq!(item.mime_type, "image/svg+xml");
    assert_eq!((item.width, item.height), (Some(120), Some(40)));

    let item = media
        .add(
            &course,
            "attachments",
            MediaSource::url(format!("{}/download", server.uri())),
        )
        .save(None)
        .await
        .unwrap();
    assert_eq!(item.file_name, "terms.txt");
    assert_eq!(&media.read(&item).await.unwrap()[..], b"Terms");
}

#[tokio::test]
async fn private_addresses_and_other_schemes_are_refused() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("internal"))
        .mount(&server)
        .await;
    // The default policy: loopback is private.
    let (media, _dir) = media(None, MediaOptions::default());
    let course = Course { id: 1 };
    let error = media
        .add_from_url(
            &course,
            "attachments",
            format!("{}/secret.txt?token=abc", server.uri()),
        )
        .save(None)
        .await
        .unwrap_err();
    assert!(matches!(error, MediaError::Blocked { .. }), "{error}");
    assert_eq!((error.status(), error.code()), (400, "remote_file_blocked"));
    // The query string stays out of the message.
    assert!(!error.to_string().contains("token"), "{error}");

    let error = media
        .add_from_url(&course, "attachments", "file:///etc/passwd")
        .save(None)
        .await
        .unwrap_err();
    assert!(matches!(error, MediaError::Blocked { .. }), "{error}");
    let error = media
        .add_from_url(&course, "attachments", "not a url")
        .save(None)
        .await
        .unwrap_err();
    assert!(matches!(error, MediaError::Download { .. }), "{error}");
}

#[tokio::test]
async fn redirects_to_private_addresses_are_refused() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/logo.png"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", "http://169.254.169.254/latest/meta-data/"),
        )
        .mount(&server)
        .await;
    let (media, _dir) = media(Some(local_client()), MediaOptions::default());
    let error = media
        .add_from_url(
            &Course { id: 1 },
            "attachments",
            format!("{}/logo.png", server.uri()),
        )
        .save(None)
        .await
        .unwrap_err();
    assert!(matches!(error, MediaError::Blocked { .. }), "{error}");
}

#[tokio::test]
async fn downloads_are_capped_in_size_status_and_time() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/big.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string("x".repeat(70 * 1024)))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/gone.txt"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/slow.txt"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("late")
                .set_delay(Duration::from_secs(3)),
        )
        .mount(&server)
        .await;
    let (media, _dir) = media(
        Some(local_client()),
        MediaOptions {
            download_timeout_secs: 1,
            ..MediaOptions::default()
        },
    );
    let course = Course { id: 1 };
    // `attachments` takes at most 64 KiB.
    let error = media
        .add_from_url(&course, "attachments", format!("{}/big.txt", server.uri()))
        .save(None)
        .await
        .unwrap_err();
    assert!(
        matches!(error, MediaError::TooLarge { max: 65536 }),
        "{error}"
    );

    let error = media
        .add_from_url(&course, "attachments", format!("{}/gone.txt", server.uri()))
        .save(None)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("404"), "{error}");
    assert_eq!(
        (error.status(), error.code()),
        (400, "remote_file_unavailable")
    );

    let error = media
        .add_from_url(
            &course,
            "attachments",
            format!("{}/slow.txt?token=abc", server.uri()),
        )
        .save(None)
        .await
        .unwrap_err();
    assert!(matches!(error, MediaError::Download { .. }), "{error}");
    assert!(!error.to_string().contains("token"), "{error}");

    // Nothing listens on port 9: the client's error names the URL, and
    // the message shows it without its query string.
    let error = media
        .add_from_url(
            &course,
            "attachments",
            "http://127.0.0.1:9/a.txt?signature=s3cr3t",
        )
        .save(None)
        .await
        .unwrap_err();
    assert!(matches!(error, MediaError::Download { .. }), "{error}");
    assert!(!error.to_string().contains("s3cr3t"), "{error}");
}
