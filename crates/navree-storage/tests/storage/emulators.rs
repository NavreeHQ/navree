//! The Google Cloud Storage and Azure Blob drivers against the providers'
//! emulators, which `scripts/ci-services.sh start` runs:
//!
//! - `NAVREE_TEST_GCS_URL=http://127.0.0.1:4443/<bucket>`: fake-gcs-server;
//! - `NAVREE_TEST_AZURE_URL=http://127.0.0.1:10000/<account>/<container>`:
//!   Azurite, with its published development account.
//!
//! A test whose variable is not set prints `SKIPPED` and passes, unless
//! `NAVREE_TEST_REQUIRE_SERVICES=1`, which makes it fail.

#[cfg(feature = "gcs")]
mod gcs {
    //! fake-gcs-server answers the XML API's `GET`, `HEAD`, list and `DELETE`,
    //! which are what the driver reads and deletes with. It does not take the
    //! XML API's `PUT`, which is what the driver writes with, so these tests
    //! put their files in the bucket through the JSON API and read, list,
    //! count and delete them through the driver. The driver's writes and its
    //! signed upload links stay covered by the recorded-response tests in
    //! `cloud.rs` until a real bucket is tested.

    use chrono::{Duration, Utc};
    use futures_util::TryStreamExt;
    use navree_storage::{Disk, GcsConfig, Storage, StorageError};
    use serde_json::json;

    use crate::support::{SIGNING_KEY, service_url, single_disk};

    const PRIVATE_KEY: &str = include_str!("../fixtures/rsa-2048.pem");
    const EMAIL: &str = "navree-test@navree-test.iam.gserviceaccount.com";

    /// A bucket of the emulator and a key prefix of this test's own.
    struct Bucket {
        endpoint: String,
        name: String,
        prefix: String,
    }

    impl Bucket {
        /// `None` (after a notice) when `NAVREE_TEST_GCS_URL` is not set.
        fn open(test: &str) -> Option<Bucket> {
            let url = service_url("NAVREE_TEST_GCS_URL", test)?;
            let url = reqwest::Url::parse(&url).expect("NAVREE_TEST_GCS_URL is a URL");
            let name = url.path().trim_matches('/').to_owned();
            assert!(
                !name.is_empty(),
                "NAVREE_TEST_GCS_URL names the bucket in its path"
            );
            Some(Bucket {
                endpoint: format!(
                    "{}://{}:{}",
                    url.scheme(),
                    url.host_str().unwrap(),
                    url.port_or_known_default().unwrap()
                ),
                name,
                prefix: format!("navree-test-{}", navree_crypto::random_token(12)),
            })
        }

        fn disk(&self) -> Disk {
            let key = json!({
                "private_key": PRIVATE_KEY,
                "private_key_id": "test-key",
                "client_email": EMAIL,
            });
            let config = GcsConfig {
                service_account_key: Some(key.to_string().into()),
                endpoint: Some(self.endpoint.clone()),
                prefix: Some(self.prefix.clone()),
                ..GcsConfig::new(self.name.clone())
            };
            Storage::new(&single_disk("media", config.into()), SIGNING_KEY)
                .unwrap()
                .default_disk()
        }

        /// Puts `body` at `path` of the disk through the JSON API.
        async fn seed(&self, path: &str, body: &'static str) {
            let name = format!("{}/{path}", self.prefix);
            let url = reqwest::Url::parse_with_params(
                &format!("{}/upload/storage/v1/b/{}/o", self.endpoint, self.name),
                [("uploadType", "media"), ("name", name.as_str())],
            )
            .unwrap();
            let reply = reqwest::Client::new()
                .post(url)
                .header("content-type", "text/plain")
                .body(body)
                .send()
                .await
                .expect("the emulator answers");
            assert!(
                reply.status().is_success(),
                "seeding `{name}`: {}",
                reply.status()
            );
        }
    }

    // CONN-052
    #[tokio::test]
    async fn the_driver_reads_and_counts_what_the_bucket_holds() {
        let Some(bucket) = Bucket::open("the_driver_reads_and_counts_what_the_bucket_holds") else {
            return;
        };
        let disk = bucket.disk();
        assert_eq!(disk.driver(), "gcs");
        bucket.seed("docs/readme.txt", "hello").await;

        assert!(disk.exists("docs/readme.txt").await.unwrap());
        assert_eq!(disk.get("docs/readme.txt").await.unwrap(), "hello");
        assert_eq!(disk.size("docs/readme.txt").await.unwrap(), 5);
        let meta = disk.metadata("docs/readme.txt").await.unwrap();
        assert_eq!((meta.path.as_str(), meta.size), ("docs/readme.txt", 5));
        assert!(meta.last_modified > Utc::now() - Duration::minutes(5), "{meta:?}");
        let streamed: Vec<u8> = disk
            .get_stream("docs/readme.txt")
            .await
            .unwrap()
            .map_ok(|chunk| chunk.to_vec())
            .try_concat()
            .await
            .unwrap();
        assert_eq!(streamed, b"hello");

        // A file that is not there is `NotFound` however it is asked for.
        assert!(!disk.exists("docs/missing.txt").await.unwrap());
        for missing in [
            disk.get("docs/missing.txt").await.err(),
            disk.size("docs/missing.txt").await.err(),
            disk.metadata("docs/missing.txt").await.err(),
        ] {
            assert!(
                matches!(missing, Some(StorageError::NotFound { .. })),
                "{missing:?}"
            );
        }
    }

    // CONN-052
    #[tokio::test]
    async fn the_driver_lists_by_directory_and_under_its_prefix() {
        let Some(bucket) = Bucket::open("the_driver_lists_by_directory_and_under_its_prefix")
        else {
            return;
        };
        let disk = bucket.disk();
        for path in [
            "docs/b.txt",
            "docs/a.txt",
            "docs/2026/report.pdf",
            "docs2/other.txt",
            "docs.txt",
            "images/logo.png",
            "my files/report (final) #2+\u{fc}.txt",
        ] {
            bucket.seed(path, "x").await;
        }
        // Another prefix of the same bucket is not part of this disk.
        let stranger = Bucket {
            endpoint: bucket.endpoint.clone(),
            name: bucket.name.clone(),
            prefix: format!("{}-other", bucket.prefix),
        };
        stranger.seed("docs/c.txt", "x").await;

        assert_eq!(
            disk.list("docs").await.unwrap(),
            vec!["docs/2026/report.pdf", "docs/a.txt", "docs/b.txt"]
        );
        assert_eq!(disk.list("/docs/").await.unwrap().len(), 3);
        assert_eq!(disk.list("").await.unwrap().len(), 7);
        assert!(disk.list("nothing/here").await.unwrap().is_empty());
        assert_eq!(
            disk.list("my files").await.unwrap(),
            vec!["my files/report (final) #2+\u{fc}.txt"]
        );
        assert_eq!(
            disk.get("my files/report (final) #2+\u{fc}.txt").await.unwrap(),
            "x"
        );
        let files = disk.files("docs").await.unwrap();
        let listed: Vec<(&str, u64)> = files
            .iter()
            .map(|file| (file.path.as_str(), file.size))
            .collect();
        assert_eq!(
            listed,
            [("docs/2026/report.pdf", 1), ("docs/a.txt", 1), ("docs/b.txt", 1)]
        );
    }

    // CONN-052
    #[tokio::test]
    async fn deleting_removes_the_file_and_a_missing_file_is_not_an_error() {
        let Some(bucket) =
            Bucket::open("deleting_removes_the_file_and_a_missing_file_is_not_an_error")
        else {
            return;
        };
        let disk = bucket.disk();
        bucket.seed("docs/readme.txt", "hello").await;
        assert!(disk.exists("docs/readme.txt").await.unwrap());

        disk.delete("docs/readme.txt").await.unwrap();

        assert!(!disk.exists("docs/readme.txt").await.unwrap());
        disk.delete("docs/readme.txt").await.unwrap();
        assert!(disk.list("").await.unwrap().is_empty());
    }

    // CONN-052
    #[tokio::test]
    async fn a_signed_download_link_reaches_the_object() {
        let Some(bucket) = Bucket::open("a_signed_download_link_reaches_the_object") else {
            return;
        };
        let disk = bucket.disk();
        bucket.seed("reports/q1-final.csv", "region,total\n").await;

        let url = disk
            .temporary_url("reports/q1-final.csv", Utc::now() + Duration::minutes(5))
            .await
            .unwrap();

        // The link names the bucket and the prefixed key, signed for the
        // provider's V4 scheme. (The emulator does not check signatures, so a
        // tampered link is not refused here.)
        assert!(
            url.starts_with(&format!(
                "{}/{}/{}/reports/q1-final.csv?",
                bucket.endpoint, bucket.name, bucket.prefix
            )),
            "{url}"
        );
        assert!(url.contains("X-Goog-Signature="), "{url}");
        let reply = reqwest::get(&url).await.unwrap();
        assert_eq!(reply.status(), 200);
        assert_eq!(reply.text().await.unwrap(), "region,total\n");
    }
}

#[cfg(feature = "azure")]
mod azure {
    //! Azurite runs the whole disk conformance suite except the two cases
    //! about binding a content type or a size to a signed upload: a service
    //! SAS cannot carry either, so the disk checks both afterwards, in
    //! `accept_upload`, which the suite runs.

    use navree_storage::{AzureConfig, Storage};

    use crate::conformance::{Http, Subject, run_except};
    use crate::support::{SIGNING_KEY, service_url, single_disk};

    /// The account key Azurite publishes for local development.
    const ACCOUNT_KEY: &str =
        "Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2UVErCz4I6tq/K1SZFPTOtr/KBHBeksoGMGw==";

    // CONN-053
    #[tokio::test]
    async fn azure_disk_conforms_on_azurite() {
        let Some(url) = service_url("NAVREE_TEST_AZURE_URL", "azure_disk_conforms_on_azurite")
        else {
            return;
        };
        // http://127.0.0.1:10000/<account>/<container>
        let url = reqwest::Url::parse(&url).expect("NAVREE_TEST_AZURE_URL is a URL");
        let mut segments = url.path().trim_matches('/').split('/');
        let account = segments
            .next()
            .filter(|segment| !segment.is_empty())
            .expect("NAVREE_TEST_AZURE_URL names the account in its path");
        let container = segments
            .next()
            .filter(|segment| !segment.is_empty())
            .expect("NAVREE_TEST_AZURE_URL names the container after the account");
        let endpoint = format!(
            "{}://{}:{}/{account}",
            url.scheme(),
            url.host_str().unwrap(),
            url.port_or_known_default().unwrap()
        );
        let prefix = format!("navree-test-{}", navree_crypto::random_token(12));
        let config = AzureConfig {
            access_key: Some(ACCOUNT_KEY.to_owned().into()),
            endpoint: Some(endpoint),
            prefix: Some(prefix),
            ..AzureConfig::new(account, container)
        };
        // Build the disk first: it installs the TLS crypto provider reqwest
        // needs.
        let disk = Storage::new(&single_disk("archive", config.into()), SIGNING_KEY)
            .unwrap()
            .default_disk();
        assert_eq!(disk.driver(), "azure");
        let subject = Subject {
            disk,
            http: Http::network(),
        };

        run_except(
            &subject,
            &[
                "presigned_put_binds_the_content_type",
                "presigned_put_binds_the_declared_size",
            ],
        )
        .await;
    }
}
