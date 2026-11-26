//! The object store behind each disk, built from its [`DiskConfig`].

use std::sync::Arc;

use crate::disk_driver_store::DriverStore;
use crate::path::normalize;
use crate::provider::Provider;
use crate::store::{Local, MemoryStore, PrefixedStore, Store};
use crate::{DiskConfig, DiskDriver, RegisteredDisk, StorageError};

/// A disk's store, and the provider that signs its URLs (cloud disks).
pub(crate) struct Backend {
    pub(crate) store: Arc<dyn Store>,
    pub(crate) provider: Option<Provider>,
}

/// Builds the store of disk `name`. Blocking: creates local roots and may
/// read credential files. `registered` is the driver a module registered
/// for a disk of [`DiskConfig::Registered`], which `StorageModule` built at
/// boot.
pub(crate) fn build(
    name: &str,
    config: &DiskConfig,
    registered: Option<Arc<dyn DiskDriver>>,
) -> Result<Backend, StorageError> {
    let backend = match config {
        DiskConfig::Local { root, .. } => {
            let local_root =
                |source: Box<dyn std::error::Error + Send + Sync>| StorageError::LocalRoot {
                    disk: name.to_owned(),
                    root: root.clone(),
                    source,
                };
            std::fs::create_dir_all(root).map_err(|e| local_root(e.into()))?;
            let store = Local::new(root).map_err(|e| local_root(e.into()))?;
            Backend {
                store: Arc::new(store),
                provider: None,
            }
        }
        DiskConfig::Memory => Backend {
            store: Arc::new(MemoryStore::new()),
            provider: None,
        },
        // Its driver's factory is async and needs the app: only the module
        // builds it. Such a disk has no URL provider, so its links go
        // through the app's own signed routes, as a memory disk's do.
        DiskConfig::Registered(disk) => match registered {
            Some(driver) => Backend {
                store: Arc::new(DriverStore(driver)),
                provider: None,
            },
            None => return Err(not_built_in(name, disk)),
        },
        #[cfg(feature = "s3")]
        DiskConfig::S3(config) => cloud::s3(name, config, &cloud::process_env())?,
        #[cfg(feature = "gcs")]
        DiskConfig::Gcs(config) => cloud::gcs(name, config, &cloud::process_env())?,
        #[cfg(feature = "azure")]
        DiskConfig::Azure(config) => cloud::azure(name, config)?,
        #[allow(unreachable_patterns)]
        other => {
            // Validation rejects a driver that is not compiled in first.
            return Err(StorageError::InvalidOptions(format!(
                "disk `{name}`: the `{}` driver is not compiled in",
                other.driver()
            )));
        }
    };
    let Some(prefix) = config.prefix() else {
        return Ok(backend);
    };
    let prefix = normalize(prefix, false).map_err(|_| {
        StorageError::InvalidOptions(format!("disk `{name}`: invalid prefix `{prefix}`"))
    })?;
    if prefix.is_empty() {
        return Err(StorageError::InvalidOptions(format!(
            "disk `{name}`: invalid prefix `{prefix}`"
        )));
    }
    Ok(Backend {
        store: Arc::new(PrefixedStore::new(backend.store, prefix.clone())),
        provider: backend
            .provider
            .map(|provider| provider.with_prefix(prefix)),
    })
}

/// The error of disk `name`, whose driver is not built in, where no driver
/// was built for it (`Storage::new`, which has no app). It does not say a
/// module registered the driver: nothing has checked that here.
pub(crate) fn not_built_in(name: &str, disk: &RegisteredDisk) -> StorageError {
    StorageError::InvalidOptions(format!(
        "disk `{name}`: the `{}` driver is not built in, and only StorageModule builds a driver a module registers, at boot",
        disk.driver()
    ))
}

#[cfg(any(feature = "s3", feature = "gcs", feature = "azure"))]
mod cloud {
    //! S3, Google Cloud Storage and Azure stores.

    #[cfg(any(feature = "s3", feature = "gcs"))]
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use super::Backend;
    #[cfg(feature = "s3")]
    use crate::S3Config;
    use crate::StorageError;
    #[cfg(feature = "azure")]
    use crate::options::AzureConfig;
    #[cfg(feature = "gcs")]
    use crate::options::GcsConfig;
    use crate::provider::{Provider, ProviderKind};
    #[cfg(feature = "azure")]
    use crate::store::cloud::Cloud;

    /// The process environment, for the cloud drivers' fallbacks.
    #[cfg(any(feature = "s3", feature = "gcs"))]
    pub(super) fn process_env() -> BTreeMap<String, String> {
        std::env::vars().collect()
    }

    /// The cloud drivers use reqwest with rustls but no default crypto
    /// provider: install ring. An error only means a process-wide provider
    /// is already installed.
    #[cfg(feature = "azure")]
    fn install_crypto_provider() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    /// A backend error while building the store of disk `name`.
    #[cfg(feature = "azure")]
    fn build_error(name: &str, source: object_store::Error) -> StorageError {
        StorageError::Backend {
            disk: name.to_owned(),
            source: Box::new(source),
        }
    }

    /// Client options allowing plain HTTP for an `http://` endpoint.
    #[cfg(feature = "azure")]
    fn client_options(endpoint: Option<&str>) -> object_store::ClientOptions {
        object_store::ClientOptions::new()
            .with_allow_http(endpoint.is_some_and(|url| url.starts_with("http://")))
    }

    /// Where an s3 disk's requests go.
    #[cfg(all(test, feature = "s3"))]
    #[derive(Debug, PartialEq, Eq)]
    pub(crate) struct S3Address {
        /// The region requests are signed for.
        pub(crate) region: String,
        /// The endpoint (with the bucket as a subdomain for
        /// virtual-hosted-style requests to a custom endpoint), or `None`
        /// for AWS's own.
        pub(crate) endpoint: Option<String>,
        /// Virtual-hosted-style requests.
        pub(crate) virtual_hosted: bool,
    }

    /// Resolves the region and endpoint of an s3 disk: the disk's settings,
    /// then `AWS_REGION` / `AWS_DEFAULT_REGION` and `AWS_ENDPOINT_URL_S3` /
    /// `AWS_ENDPOINT_URL` / `AWS_ENDPOINT`.
    #[cfg(all(test, feature = "s3"))]
    pub(crate) fn s3_address(config: &S3Config, env: &BTreeMap<String, String>) -> S3Address {
        let from_env = |keys: &[&str]| {
            keys.iter()
                .find_map(|key| env.get(*key).filter(|value| !value.trim().is_empty()))
                .cloned()
        };
        let region = config
            .region
            .clone()
            .or_else(|| from_env(&["AWS_REGION", "AWS_DEFAULT_REGION"]))
            .unwrap_or_else(|| "us-east-1".to_owned());
        let endpoint = config
            .endpoint
            .clone()
            .or_else(|| from_env(&["AWS_ENDPOINT_URL_S3", "AWS_ENDPOINT_URL", "AWS_ENDPOINT"]));
        let path_style = config.path_style.unwrap_or(endpoint.is_some());
        let endpoint = match endpoint {
            Some(endpoint) if !path_style => Some(bucket_subdomain(&endpoint, &config.bucket)),
            endpoint => endpoint.map(|url| url.trim_end_matches('/').to_owned()),
        };
        S3Address {
            region,
            endpoint,
            virtual_hosted: !path_style,
        }
    }

    /// `https://host` → `https://bucket.host`.
    #[cfg(all(test, feature = "s3"))]
    fn bucket_subdomain(endpoint: &str, bucket: &str) -> String {
        let endpoint = endpoint.trim_end_matches('/');
        match endpoint.split_once("://") {
            Some((scheme, host)) => format!("{scheme}://{bucket}.{host}"),
            None => format!("{bucket}.{endpoint}"),
        }
    }

    /// An S3 (or S3-compatible) store.
    #[cfg(feature = "s3")]
    pub(super) fn s3(
        name: &str,
        config: &S3Config,
        env: &BTreeMap<String, String>,
    ) -> Result<Backend, StorageError> {
        const _: fn(Arc<dyn object_store::ObjectStore>) -> crate::store::cloud::Cloud =
            crate::store::cloud::Cloud::new;

        let store = Arc::new(
            crate::cloud::s3::S3Store::with_env(config, env).map_err(|error| {
                StorageError::Backend {
                    disk: name.to_owned(),
                    source: Box::new(error),
                }
            })?,
        );
        Ok(Backend {
            store: store.clone(),
            provider: Some(Provider::from_store(ProviderKind::S3, store)),
        })
    }

    /// A Google Cloud Storage store.
    #[cfg(feature = "gcs")]
    pub(super) fn gcs(
        name: &str,
        config: &GcsConfig,
        env: &BTreeMap<String, String>,
    ) -> Result<Backend, StorageError> {
        let store = Arc::new(crate::cloud::gcs::GcsStore::from_env(config, env).map_err(
            |error| StorageError::Backend {
                disk: name.to_owned(),
                source: Box::new(error),
            },
        )?);
        Ok(Backend {
            store: store.clone(),
            provider: Some(Provider::from_store(ProviderKind::Gcs, store)),
        })
    }

    /// An Azure Blob Storage store.
    #[cfg(feature = "azure")]
    pub(super) fn azure(name: &str, config: &AzureConfig) -> Result<Backend, StorageError> {
        use object_store::azure::{AzureConfigKey, MicrosoftAzureBuilder};

        use crate::azure_sas::SharedKeySas;

        install_crypto_provider();
        let mut builder = MicrosoftAzureBuilder::from_env()
            .with_container_name(&config.container)
            .with_client_options(client_options(config.endpoint.as_deref()));
        if let Some(account) = &config.account {
            builder = builder.with_account(account);
        }
        if let Some(access_key) = &config.access_key {
            builder = builder.with_access_key(access_key.expose());
        }
        if let Some(endpoint) = &config.endpoint {
            builder = builder.with_endpoint(endpoint.trim_end_matches('/').to_owned());
        }
        // Links are signed here with the account key when there is one
        // (object_store signs the encoded name, Azure the decoded one; see
        // `azure_sas`). Without a key (a managed identity, say) object_store
        // signs them with a user delegation key.
        let shared_key = match (
            builder.get_config_value(&AzureConfigKey::AccountName),
            builder.get_config_value(&AzureConfigKey::AccessKey),
        ) {
            (Some(account), Some(key)) => {
                Some(SharedKeySas::new(&account, &key).map_err(|error| {
                    StorageError::InvalidOptions(format!(
                        "storage.disks.{name}.access_key is not a base64 account key: {error}"
                    ))
                })?)
            }
            _ => None,
        };
        let store = Arc::new(builder.build().map_err(|error| build_error(name, error))?);
        let mut provider = Provider::new(ProviderKind::Azure, store.clone());
        if let Some(shared_key) = shared_key {
            provider = provider.with_shared_key(shared_key);
        }
        Ok(Backend {
            store: Arc::new(Cloud::new(store)),
            provider: Some(provider),
        })
    }

    #[cfg(all(test, feature = "s3"))]
    mod tests {
        use super::*;

        fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
            pairs
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect()
        }

        #[test]
        fn aws_is_virtual_hosted_in_the_configured_region() {
            let address = s3_address(&S3Config::new("media").region("eu-west-1"), &env(&[]));
            assert_eq!(
                address,
                S3Address {
                    region: "eu-west-1".into(),
                    endpoint: None,
                    virtual_hosted: true,
                }
            );
            let address = s3_address(&S3Config::new("media"), &env(&[]));
            assert_eq!(address.region, "us-east-1");
        }

        #[test]
        fn an_endpoint_defaults_to_path_style() {
            let address = s3_address(
                &S3Config::new("media").endpoint("http://127.0.0.1:9000/"),
                &env(&[]),
            );
            assert_eq!(address.endpoint.as_deref(), Some("http://127.0.0.1:9000"));
            assert!(!address.virtual_hosted);
        }

        #[test]
        fn virtual_hosted_endpoints_put_the_bucket_in_the_host() {
            let address = s3_address(
                &S3Config::new("media")
                    .endpoint("https://nyc3.digitaloceanspaces.com")
                    .path_style(false),
                &env(&[]),
            );
            assert_eq!(
                address.endpoint.as_deref(),
                Some("https://media.nyc3.digitaloceanspaces.com")
            );
            assert!(address.virtual_hosted);
        }

        #[test]
        fn the_environment_fills_what_the_disk_leaves_out() {
            let vars = env(&[
                ("AWS_DEFAULT_REGION", "ap-south-1"),
                ("AWS_ENDPOINT_URL", "http://127.0.0.1:9000"),
            ]);
            let address = s3_address(&S3Config::new("media"), &vars);
            assert_eq!(address.region, "ap-south-1");
            assert_eq!(address.endpoint.as_deref(), Some("http://127.0.0.1:9000"));
            assert!(!address.virtual_hosted);

            let vars = env(&[
                ("AWS_REGION", "eu-central-1"),
                ("AWS_DEFAULT_REGION", "ap-south-1"),
                ("AWS_ENDPOINT_URL", "http://a"),
                ("AWS_ENDPOINT_URL_S3", "http://b"),
            ]);
            let address = s3_address(&S3Config::new("media"), &vars);
            assert_eq!(address.region, "eu-central-1");
            assert_eq!(address.endpoint.as_deref(), Some("http://b"));

            // The disk's own settings win.
            let address = s3_address(
                &S3Config::new("media")
                    .region("auto")
                    .endpoint("https://acct.r2.cloudflarestorage.com"),
                &vars,
            );
            assert_eq!(address.region, "auto");
            assert_eq!(
                address.endpoint.as_deref(),
                Some("https://acct.r2.cloudflarestorage.com")
            );
        }

        #[test]
        fn builds_without_network_access() {
            let config = S3Config::new("media")
                .endpoint("http://127.0.0.1:9")
                .credentials("key", "secret");
            let backend = s3("uploads", &config, &env(&[])).unwrap();
            assert!(backend.provider.is_some());
            assert!(backend.store.to_string().contains("media"));
        }

        /// The access key a link of `config` is signed with, given `vars`.
        async fn signing_key_id(config: &S3Config, vars: &[(&str, &str)]) -> String {
            let backend = s3("uploads", config, &env(vars)).unwrap();
            let url = backend
                .provider
                .unwrap()
                .get_url("a.txt", std::time::Duration::from_secs(60))
                .await
                .unwrap();
            let credential = url
                .split('&')
                .find_map(|pair| pair.strip_prefix("X-Amz-Credential="))
                .unwrap();
            credential.split("%2F").next().unwrap().to_owned()
        }

        #[tokio::test]
        async fn credentials_come_from_the_disk_then_the_environment() {
            let vars = [
                ("AWS_ACCESS_KEY_ID", "ENVKEY"),
                ("AWS_SECRET_ACCESS_KEY", "env-secret"),
                ("AWS_SESSION_TOKEN", "env-token"),
            ];
            let without = S3Config::new("media").endpoint("http://127.0.0.1:9");
            assert_eq!(signing_key_id(&without, &vars).await, "ENVKEY");
            let with = without.clone().credentials("DISKKEY", "disk-secret");
            assert_eq!(signing_key_id(&with, &vars).await, "DISKKEY");

            // The environment's session token belongs to its own keys only.
            let backend = s3("uploads", &with, &env(&vars)).unwrap();
            let url = backend
                .provider
                .unwrap()
                .get_url("a.txt", std::time::Duration::from_secs(60))
                .await
                .unwrap();
            assert!(!url.contains("env-token"), "{url}");
        }
    }
}
