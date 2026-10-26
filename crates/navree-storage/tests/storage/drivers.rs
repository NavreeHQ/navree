//! The conformance suite on every driver: memory, local, S3 on the
//! in-process server, and S3 on MinIO (`NAVREE_TEST_S3_URL`).

use navree_kernel::Navree;
use navree_storage::{Disk, DiskConfig, StorageModule};

use crate::conformance::{Http, Subject, run};
use crate::support::{TestApp, single_disk};

/// An app whose default disk is `config`, and the disk as a [`Subject`].
async fn app_subject(name: &str, config: DiskConfig) -> (TestApp, Subject) {
    let app =
        TestApp::boot(Navree::app().module(StorageModule::for_root(single_disk(name, config))))
            .await;
    let disk = app.app.ctx().get::<Disk>().await.unwrap();
    let subject = Subject {
        disk: (*disk).clone(),
        http: Http::app(app.router.clone()),
    };
    (app, subject)
}

#[tokio::test]
async fn memory_disk_conforms() {
    let (_app, subject) = app_subject("memory", DiskConfig::Memory).await;
    assert_eq!(subject.disk.driver(), "memory");
    run(&subject).await;
}

#[tokio::test]
async fn local_disk_conforms() {
    let root = tempfile::tempdir().unwrap();
    let (_app, subject) = app_subject("local", DiskConfig::local(root.path())).await;
    assert_eq!(subject.disk.driver(), "local");
    run(&subject).await;
    assert_eq!(files_under(root.path()), 0, "nothing is left on disk");
}

/// The number of files under `dir`, recursively.
fn files_under(dir: &std::path::Path) -> usize {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                files_under(&entry.path())
            } else {
                1
            }
        })
        .sum()
}

#[cfg(feature = "s3")]
mod s3 {
    use std::collections::BTreeMap;
    use std::time::Duration;

    use chrono::Utc;
    use navree_contracts::{HealthCheck, HealthStatus};
    use navree_kernel::Navree;
    use navree_storage::{
        Disk, DiskConfig, S3Config, Storage, StorageError, StorageModule, StorageOptions,
        UploadRules,
    };

    use super::*;
    use crate::s3_server::{ACCESS_KEY, BUCKET, REGION, S3Server, SECRET_KEY};
    use crate::support::{SIGNING_KEY, service_url};

    /// An s3 disk on the in-process server.
    fn config(server: &S3Server) -> S3Config {
        S3Config::new(BUCKET)
            .endpoint(&server.url)
            .region(REGION)
            .credentials(ACCESS_KEY, SECRET_KEY)
    }

    fn disk(config: S3Config) -> Disk {
        Storage::new(&single_disk("s3", config.into()), SIGNING_KEY)
            .unwrap()
            .default_disk()
    }

    #[tokio::test]
    async fn s3_disk_conforms_on_the_in_process_server() {
        let server = S3Server::start().await;
        let subject = Subject {
            disk: disk(config(&server)),
            http: Http::network(),
        };
        assert_eq!(subject.disk.driver(), "s3");
        run(&subject).await;
        assert!(server.keys().is_empty(), "{:?}", server.keys());
        // Uploads went through multipart requests, links were presigned.
        let log = server.log();
        assert!(log.iter().any(|line| line.contains("?uploads")), "{log:?}");
        assert!(log.iter().any(|line| line.contains("X-Amz-Signature=")), "{log:?}");
    }

    #[tokio::test]
    async fn s3_disk_conforms_on_minio() {
        let Some(url) = service_url("NAVREE_TEST_S3_URL", "s3_disk_conforms_on_minio") else {
            return;
        };
        // http://<access key>:<secret key>@127.0.0.1:9000/<bucket>
        let url = reqwest::Url::parse(&url).expect("NAVREE_TEST_S3_URL is a URL");
        let bucket = url.path().trim_matches('/').to_owned();
        assert!(!bucket.is_empty(), "NAVREE_TEST_S3_URL names the bucket in its path");
        let endpoint = format!(
            "{}://{}:{}",
            url.scheme(),
            url.host_str().unwrap(),
            url.port_or_known_default().unwrap()
        );
        let prefix = format!("navree-test-{}", navree_crypto::random_token(12));
        let config = S3Config::new(bucket)
            .endpoint(endpoint)
            .credentials(url.username(), url.password().unwrap_or_default())
            .prefix(prefix);
        let subject = Subject {
            disk: disk(config),
            http: Http::network(),
        };
        run(&subject).await;
    }

    #[tokio::test]
    async fn a_prefix_keeps_every_key_under_it() {
        let server = S3Server::start().await;
        let disk = disk(config(&server).prefix("/tenants//acme/"));
        disk.put("logo.png", crate::conformance::png(100)).await.unwrap();
        assert_eq!(server.keys(), ["tenants/acme/logo.png"]);
        assert_eq!(disk.list("").await.unwrap(), ["logo.png"]);

        let http = Http::network();
        let url = disk
            .temporary_url("logo.png", Utc::now() + chrono::Duration::minutes(1))
            .await
            .unwrap();
        assert!(url.contains("/navree-test/tenants/acme/logo.png?"), "{url}");
        assert_eq!(http.get(&url).await.status, 200);

        let rules = UploadRules::new(1000, ["image/png"]);
        let upload = disk
            .presigned_put("next.png", chrono::Duration::minutes(1), &rules)
            .content_type("image/png")
            .await
            .unwrap();
        let reply = http
            .send("PUT", &upload.url, &upload.headers, crate::conformance::png(50))
            .await;
        assert_eq!(reply.status, 200, "{reply:?}");
        assert_eq!(server.keys(), ["tenants/acme/logo.png", "tenants/acme/next.png"]);
        assert_eq!(server.content_type("tenants/acme/next.png").as_deref(), Some("image/png"));
    }

    #[tokio::test]
    async fn links_are_signed_by_the_provider() {
        let server = S3Server::start().await;
        let disk = disk(config(&server));
        let url = disk
            .temporary_url("a b.txt", Utc::now() + chrono::Duration::hours(1))
            .await
            .unwrap();
        assert!(url.starts_with(&format!("{}/navree-test/a%20b.txt?", server.url)), "{url}");
        for param in [
            "X-Amz-Algorithm=AWS4-HMAC-SHA256",
            "X-Amz-Credential=navree-test%2F",
            "X-Amz-Expires=3600",
            "X-Amz-Signature=",
        ] {
            assert!(url.contains(param), "{param}: {url}");
        }
        // S3 refuses links over 7 days.
        let error = disk
            .temporary_url("a.txt", Utc::now() + chrono::Duration::days(8))
            .await
            .unwrap_err();
        assert!(
            matches!(&error, StorageError::Signing { reason, .. } if reason.contains("7 days")),
            "{error:?}"
        );
        // An expiry in the past still gives a (short) link.
        assert!(disk.temporary_url("a.txt", Utc::now()).await.is_ok());
    }

    #[tokio::test]
    async fn wrong_credentials_are_a_backend_error() {
        let server = S3Server::start().await;
        let disk = disk(config(&server).credentials(ACCESS_KEY, "not-the-secret"));
        let error = disk.put("a.txt", "x").await.unwrap_err();
        assert!(matches!(error, StorageError::Backend { .. }), "{error:?}");
        assert!(matches!(
            disk.check_health().await,
            HealthStatus::Down(reason) if reason.contains("storage error on disk `s3`")
        ));
        assert!(server.keys().is_empty());
    }

    #[tokio::test]
    async fn the_health_check_names_a_slow_disk() {
        let server = S3Server::start().await;
        server.slow_down(Duration::from_secs(5));
        let options = StorageOptions {
            default: "scratch".into(),
            disks: BTreeMap::from([
                ("scratch".to_owned(), DiskConfig::Memory),
                ("uploads".to_owned(), config(&server).into()),
            ]),
            ..StorageOptions::default()
        };
        let app = Navree::app()
            .module(StorageModule::for_root(options))
            .boot()
            .await
            .unwrap();
        let checks = app.ctx().get_all::<dyn HealthCheck>().await.unwrap();
        let storage = checks.iter().find(|check| check.name() == "storage").unwrap();
        let started = std::time::Instant::now();
        let status = storage.check().await;
        assert!(started.elapsed() < Duration::from_secs(3), "{:?}", started.elapsed());
        assert_eq!(
            status,
            HealthStatus::Down("disk `uploads`: timed out after 1500 ms".to_owned())
        );
    }
}
