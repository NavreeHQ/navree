//! Google Cloud Storage and Azure disks against recorded API responses
//! (wiremock): requests are authenticated and addressed as the providers
//! expect, and links are signed by the provider with the right bindings.

#[cfg(feature = "gcs")]
mod gcs {
    use chrono::{Duration, Utc};
    use navree_storage::{Disk, GcsConfig, Storage, UploadRules};
    use serde_json::json;
    use wiremock::matchers::{header_exists, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::support::{SIGNING_KEY, single_disk};

    const PRIVATE_KEY: &str = include_str!("../fixtures/rsa-2048.pem");
    const EMAIL: &str = "navree-test@navree-test.iam.gserviceaccount.com";

    /// A gcs disk on `server`, signing with the test key.
    fn disk(server: &MockServer, prefix: Option<&str>) -> Disk {
        let key = json!({
            "private_key": PRIVATE_KEY,
            "private_key_id": "test-key",
            "client_email": EMAIL,
        });
        let config = GcsConfig {
            service_account_key: Some(key.to_string().into()),
            endpoint: Some(server.uri()),
            prefix: prefix.map(str::to_owned),
            ..GcsConfig::new("acme-media")
        };
        Storage::new(&single_disk("media", config.into()), SIGNING_KEY)
            .unwrap()
            .default_disk()
    }

    fn object() -> ResponseTemplate {
        ResponseTemplate::new(200)
            .insert_header("etag", "\"abc\"")
            .insert_header("last-modified", "Mon, 26 Oct 2026 10:00:00 GMT")
    }

    #[tokio::test]
    async fn files_go_to_the_bucket_with_a_bearer_token() {
        let server = MockServer::start().await;
        // The object name (prefix and path) is one percent-encoded segment.
        let object_path = "/acme%2Dmedia/site%2Freports%2Fq1%2Ecsv";
        Mock::given(method("PUT"))
            .and(path(object_path))
            .and(header_exists("authorization"))
            .respond_with(object())
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(object_path))
            .respond_with(object().set_body_string("a,b\n"))
            .mount(&server)
            .await;
        let disk = disk(&server, Some("site"));
        assert_eq!(disk.driver(), "gcs");
        disk.put("reports/q1.csv", "a,b\n").await.unwrap();
        assert_eq!(disk.get("reports/q1.csv").await.unwrap(), "a,b\n");

        let requests = server.received_requests().await.unwrap();
        let authorization = requests[0].headers["authorization"].to_str().unwrap();
        assert!(
            authorization.starts_with("Bearer ey"),
            "a self-signed JWT: {authorization}"
        );
    }

    #[tokio::test]
    async fn uploads_are_bound_to_the_type_and_the_size_limit() {
        let server = MockServer::start().await;
        let disk = disk(&server, Some("site"));
        let rules = UploadRules::new(5_000_000, ["image/png"]);
        let upload = disk
            .presigned_put("avatars/a.png", Duration::minutes(10), &rules)
            .content_type("image/png")
            .await
            .unwrap();
        assert_eq!(
            upload.headers,
            [
                ("content-type".to_owned(), "image/png".to_owned()),
                (
                    "x-goog-content-length-range".to_owned(),
                    "0,5000000".to_owned()
                ),
            ]
            .into()
        );
        let url = &upload.url;
        assert!(
            url.starts_with(&format!("{}/acme-media/site/avatars/a.png?", server.uri())),
            "{url}"
        );
        for param in [
            "X-Goog-Algorithm=GOOG4-RSA-SHA256",
            "X-Goog-Credential=navree-test%40navree-test.iam.gserviceaccount.com",
            "X-Goog-Expires=600",
            "X-Goog-SignedHeaders=content-type%3Bhost%3Bx-goog-content-length-range",
            "X-Goog-Signature=",
        ] {
            assert!(url.contains(param), "{param}: {url}");
        }

        let download = disk
            .temporary_url("avatars/a.png", Utc::now() + Duration::minutes(5))
            .await
            .unwrap();
        assert!(
            download.contains("X-Goog-SignedHeaders=host&"),
            "{download}"
        );
        // Nothing was sent to the provider: signing is local.
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_service_account_key_must_be_valid() {
        let config = GcsConfig {
            service_account_key: Some("not json".to_owned().into()),
            ..GcsConfig::new("acme-media")
        };
        let error = Storage::new(&single_disk("media", config.into()), SIGNING_KEY).unwrap_err();
        assert!(error.to_string().contains("disk `media`"), "{error}");
        assert!(!error.to_string().contains("not json"), "{error}");
    }
}

#[cfg(feature = "azure")]
mod azure {
    use chrono::{Duration, Utc};
    use navree_storage::{AzureConfig, Disk, Storage, UploadRules};
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::support::{SIGNING_KEY, single_disk};

    /// The account key Azurite publishes for local development.
    const ACCOUNT_KEY: &str =
        "Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2UVErCz4I6tq/K1SZFPTOtr/KBHBeksoGMGw==";

    fn disk(server: &MockServer) -> Disk {
        let config = AzureConfig {
            access_key: Some(ACCOUNT_KEY.to_owned().into()),
            endpoint: Some(format!("{}/devstoreaccount1", server.uri())),
            ..AzureConfig::new("devstoreaccount1", "archive")
        };
        Storage::new(&single_disk("archive", config.into()), SIGNING_KEY)
            .unwrap()
            .default_disk()
    }

    fn blob() -> ResponseTemplate {
        ResponseTemplate::new(201)
            .insert_header("etag", "\"0x8D\"")
            .insert_header("last-modified", "Mon, 26 Oct 2026 10:00:00 GMT")
    }

    #[tokio::test]
    async fn blobs_are_written_with_a_shared_key() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/devstoreaccount1/archive/2026/q1.csv"))
            .and(header("x-ms-blob-type", "BlockBlob"))
            .respond_with(blob())
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/devstoreaccount1/archive/2026/q1.csv"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("etag", "\"0x8D\"")
                    .insert_header("last-modified", "Mon, 26 Oct 2026 10:00:00 GMT")
                    .set_body_string("a,b\n"),
            )
            .mount(&server)
            .await;
        let disk = disk(&server);
        assert_eq!(disk.driver(), "azure");
        disk.put("2026/q1.csv", "a,b\n").await.unwrap();
        assert_eq!(disk.get("2026/q1.csv").await.unwrap(), "a,b\n");
        let requests = server.received_requests().await.unwrap();
        let authorization = requests[0].headers["authorization"].to_str().unwrap();
        assert!(
            authorization.starts_with("SharedKey devstoreaccount1:"),
            "{authorization}"
        );
    }

    #[tokio::test]
    async fn links_are_service_sas_and_uploads_name_the_blob_type() {
        let server = MockServer::start().await;
        let disk = disk(&server);
        let rules = UploadRules::new(1000, ["application/pdf"]);
        let upload = disk
            .presigned_put("contracts/a.pdf", Duration::minutes(10), &rules)
            .content_type("application/pdf")
            .size(900)
            .await
            .unwrap();
        assert_eq!(
            upload.headers,
            [
                ("content-type".to_owned(), "application/pdf".to_owned()),
                ("x-ms-blob-type".to_owned(), "BlockBlob".to_owned()),
            ]
            .into()
        );
        assert!(
            upload.url.starts_with(&format!(
                "{}/devstoreaccount1/archive/contracts/a.pdf?",
                server.uri()
            )),
            "{}",
            upload.url
        );
        for param in ["sv=", "se=", "sr=b", "sp=", "sig="] {
            assert!(upload.url.contains(param), "{param}: {}", upload.url);
        }
        let download = disk
            .temporary_url("contracts/a.pdf", Utc::now() + Duration::days(30))
            .await
            .unwrap();
        assert!(download.contains("sp=r"), "{download}");
        assert!(server.received_requests().await.unwrap().is_empty());
    }
}
