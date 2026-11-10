//! Cloud disk configuration: parsing, validation and the compiled-in drivers.

use navree_config::{ConfigModule, ConfigSection, ConfigSources};
use navree_kernel::Navree;
use navree_storage::{AzureConfig, DiskConfig, GcsConfig, S3Config, StorageModule, StorageOptions};
use serde_json::json;

use crate::support::{boot_error, single_disk};

fn parse(section: serde_json::Value) -> StorageOptions {
    serde_json::from_value(section).expect("the section parses")
}

#[test]
fn s3_disks_parse_every_key() {
    let options = parse(json!({
        "default": "uploads",
        "disks": {
            "uploads": {
                "driver": "s3",
                "bucket": "agra-uploads",
                "region": "eu-west-1",
                "endpoint": "https://acct.r2.cloudflarestorage.com",
                "access_key": "AKIA123",
                "secret_key": "s3cr3t",
                "session_token": "token",
                "path_style": false,
                "prefix": "production",
                "public_url": "https://cdn.example.com",
            }
        }
    }));
    let DiskConfig::S3(s3) = &options.disks["uploads"] else {
        panic!("an s3 disk");
    };
    assert_eq!(s3.bucket, "agra-uploads");
    assert_eq!(s3.region.as_deref(), Some("eu-west-1"));
    assert_eq!(
        s3.endpoint.as_deref(),
        Some("https://acct.r2.cloudflarestorage.com")
    );
    assert_eq!(s3.access_key.as_deref(), Some("AKIA123"));
    assert_eq!(s3.secret_key.as_ref().unwrap().expose(), "s3cr3t");
    assert_eq!(s3.session_token.as_ref().unwrap().expose(), "token");
    assert_eq!(s3.path_style, Some(false));
    assert_eq!(s3.prefix.as_deref(), Some("production"));
    assert_eq!(s3.public_url.as_deref(), Some("https://cdn.example.com"));
    assert_eq!(options.disks["uploads"].driver(), "s3");

    // The builder gives the same disk.
    let built = S3Config::new("agra-uploads")
        .region("eu-west-1")
        .endpoint("https://acct.r2.cloudflarestorage.com")
        .credentials("AKIA123", "s3cr3t")
        .path_style(false)
        .prefix("production")
        .public_url("https://cdn.example.com");
    assert_eq!(
        DiskConfig::from(built),
        DiskConfig::S3(S3Config {
            session_token: None,
            ..s3.clone()
        })
    );
}

#[test]
fn secrets_never_show_in_debug_output() {
    let options = parse(json!({
        "default": "uploads",
        "disks": {
            "uploads": { "driver": "s3", "bucket": "b", "access_key": "AKIA", "secret_key": "hunter2" },
            "media": { "driver": "gcs", "bucket": "b", "service_account_key": "{\"private_key\": \"hunter3\"}" },
            "archive": { "driver": "azure", "account": "agra", "container": "c", "access_key": "hunter4" },
        }
    }));
    let debug = format!("{options:?}");
    for secret in ["hunter2", "hunter3", "hunter4"] {
        assert!(!debug.contains(secret), "{debug}");
    }
    assert!(debug.contains("[redacted]"));
}

#[test]
fn gcs_and_azure_disks_parse() {
    let options = parse(json!({
        "default": "media",
        "disks": {
            "media": {
                "driver": "gcs",
                "bucket": "agra-media",
                "service_account_path": "/run/secrets/gcs.json",
                "endpoint": "http://127.0.0.1:4443",
                "prefix": "site",
            },
            "archive": {
                "driver": "azure",
                "account": "agrastorage",
                "bucket": "archive",
                "access_key": "a2V5",
                "endpoint": "http://127.0.0.1:10000/devstoreaccount1",
            },
        }
    }));
    let DiskConfig::Gcs(gcs) = &options.disks["media"] else {
        panic!("a gcs disk");
    };
    assert_eq!(gcs.bucket, "agra-media");
    assert_eq!(
        gcs.service_account_path.as_deref(),
        Some(std::path::Path::new("/run/secrets/gcs.json"))
    );
    assert_eq!(gcs.prefix.as_deref(), Some("site"));
    let DiskConfig::Azure(azure) = &options.disks["archive"] else {
        panic!("an azure disk");
    };
    assert_eq!(azure.account.as_deref(), Some("agrastorage"));
    assert_eq!(
        azure.container, "archive",
        "`bucket` is another name for `container`"
    );
    assert_eq!(azure.access_key.as_ref().unwrap().expose(), "a2V5");
    assert_eq!(GcsConfig::new("agra-media").bucket, "agra-media");
    let azure = AzureConfig::new("agrastorage", "archive");
    assert_eq!(
        (azure.account.as_deref(), azure.container.as_str()),
        (Some("agrastorage"), "archive")
    );
}

#[test]
fn unknown_keys_and_drivers_are_errors() {
    let typo = serde_json::from_value::<StorageOptions>(json!({
        "disks": { "uploads": { "driver": "s3", "bucket": "b", "acces_key": "x" } }
    }))
    .unwrap_err()
    .to_string();
    assert!(typo.contains("acces_key"), "{typo}");
    let driver = serde_json::from_value::<StorageOptions>(json!({
        "disks": { "uploads": { "driver": "ftp" } }
    }))
    .unwrap_err()
    .to_string();
    assert!(driver.contains("ftp"), "{driver}");
}

/// The validation error of a single disk `uploads`.
fn invalid(config: DiskConfig) -> String {
    single_disk("uploads", config)
        .validate()
        .expect_err("must be invalid")
}

#[test]
fn cloud_disks_are_validated() {
    if cfg!(feature = "s3") {
        assert_eq!(
            invalid(S3Config::default().into()),
            "disk `uploads`: `bucket` is required for the s3 driver"
        );
        assert!(
            invalid(S3Config::new("b").endpoint("127.0.0.1:9000").into())
                .contains("endpoint `127.0.0.1:9000` must be an absolute http(s) URL")
        );
        let half = S3Config {
            access_key: Some("AKIA".into()),
            ..S3Config::new("b")
        };
        assert!(invalid(half.into()).contains("set both access_key and secret_key"));
        assert!(
            invalid(S3Config::new("b").prefix("../other-tenant").into())
                .contains("prefix `../other-tenant`")
        );
        assert!(invalid(S3Config::new("b").public_url(" ").into()).contains("public_url"));
        assert_eq!(
            single_disk("uploads", S3Config::new("b").prefix("a/b").into()).validate(),
            Ok(())
        );
    }
    if cfg!(feature = "gcs") {
        assert!(invalid(GcsConfig::default().into()).contains("`bucket` is required for the gcs"));
        let both = GcsConfig {
            service_account_path: Some("key.json".into()),
            service_account_key: Some("{}".to_owned().into()),
            ..GcsConfig::new("b")
        };
        assert!(invalid(both.into()).contains("not both"));
    }
    if cfg!(feature = "azure") {
        assert!(
            invalid(AzureConfig::default().into())
                .contains("`container` is required for the azure")
        );
    }
}

#[test]
fn a_driver_that_is_not_compiled_in_names_its_feature() {
    let checks = [
        (
            cfg!(feature = "s3"),
            DiskConfig::from(S3Config::new("b")),
            "storage-s3",
        ),
        (
            cfg!(feature = "gcs"),
            GcsConfig::new("b").into(),
            "storage-gcs",
        ),
        (
            cfg!(feature = "azure"),
            AzureConfig::new("a", "c").into(),
            "storage-azure",
        ),
    ];
    for (compiled_in, config, feature) in checks {
        let result = single_disk("uploads", config).validate();
        if compiled_in {
            assert_eq!(result, Ok(()), "{feature}");
        } else {
            let error = result.unwrap_err();
            assert!(error.contains(feature), "{error}");
            assert!(error.contains("not compiled in"), "{error}");
        }
    }
}

#[tokio::test]
async fn s3_disks_come_from_the_config_and_its_environment_variables() {
    // `APP_STORAGE__DISKS__UPLOADS__SECRET_KEY` and friends land in the
    // same keys as the TOML file.
    let sources = ConfigSources::new().set(
        "storage",
        json!({
            "default": "uploads",
            "disks": { "uploads": { "driver": "s3", "bucket": "agra", "endpoint": "http://127.0.0.1:9", "access_key": "k", "secret_key": "s" } },
        }),
    );
    let builder = Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(StorageModule::from_config());
    if cfg!(feature = "s3") {
        let app = builder.boot().await.unwrap();
        let disk = app.ctx().get::<navree_storage::Disk>().await.unwrap();
        assert_eq!((disk.name(), disk.driver()), ("uploads", "s3"));
        // Building a cloud disk makes no request.
    } else {
        let error = boot_error(builder).await;
        assert!(error.contains("storage-s3"), "{error}");
    }
}
