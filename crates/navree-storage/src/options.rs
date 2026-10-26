//! [`StorageOptions`] and [`DiskConfig`], with the cloud disk settings
//! [`S3Config`], [`GcsConfig`] and [`AzureConfig`].

use std::collections::BTreeMap;
use std::path::PathBuf;

use navree_config::ConfigSection;
use navree_contracts::Secret;
use serde::Deserialize;

use crate::path::normalize;

/// `StorageModule` settings: configuration section `storage`.
///
/// ```toml
/// [storage]
/// default = "local"
/// base_url = "https://api.example.com"   # optional, for temporary URLs
///
/// [storage.disks.local]
/// driver = "local"
/// root = "storage/app"
///
/// [storage.disks.public]
/// driver = "local"
/// root = "storage/app/public"
/// public_url = "https://cdn.example.com"
///
/// [storage.disks.uploads]
/// driver = "s3"
/// bucket = "acme-uploads"
/// region = "eu-west-1"
///
/// [storage.disks.scratch]
/// driver = "memory"
/// ```
///
/// Without a `storage` section there is one disk, `local`, rooted at
/// `storage/app` (relative to the working directory). Defining `disks`
/// replaces that default disk.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct StorageOptions {
    /// The name of the default disk (default `local`).
    pub default: String,
    /// The disks, by name. Names use `[A-Za-z0-9_-]`.
    pub disks: BTreeMap<String, DiskConfig>,
    /// An absolute base URL (`https://api.example.com`) prepended to the
    /// links the app serves itself ([temporary URLs](crate::Disk::temporary_url)
    /// and [presigned uploads](crate::Disk::presigned_put) of local and
    /// memory disks); without one they are path-only
    /// (`/storage/<disk>/<path>?expires=...&signature=...`).
    pub base_url: Option<String>,
}

impl Default for StorageOptions {
    fn default() -> Self {
        Self {
            default: "local".to_owned(),
            disks: BTreeMap::from([("local".to_owned(), DiskConfig::local("storage/app"))]),
            base_url: None,
        }
    }
}

/// One disk: where its files live (`driver = "..."`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(tag = "driver", rename_all = "snake_case")]
#[non_exhaustive]
pub enum DiskConfig {
    /// Files under a directory of the local filesystem (`driver = "local"`),
    /// created at boot if missing.
    Local {
        /// The root directory.
        root: PathBuf,
        /// The URL the root is publicly served from (a CDN or a static file
        /// server), for [`Disk::url`](crate::Disk::url).
        #[serde(default)]
        public_url: Option<String>,
    },
    /// Files in process memory, lost on restart (`driver = "memory"`); for
    /// tests and scratch data.
    Memory,
    /// An S3 bucket (`driver = "s3"`): AWS, or an S3-compatible store such
    /// as MinIO, Cloudflare R2 or DigitalOcean Spaces through `endpoint`.
    /// Needs the crate's `s3` feature (navree's `storage-s3`).
    S3(S3Config),
    /// A Google Cloud Storage bucket (`driver = "gcs"`). Needs the crate's
    /// `gcs` feature (navree's `storage-gcs`).
    Gcs(GcsConfig),
    /// An Azure Blob Storage container (`driver = "azure"`). Needs the
    /// crate's `azure` feature (navree's `storage-azure`).
    Azure(AzureConfig),
}

impl DiskConfig {
    /// A local disk rooted at `root`, with no public URL.
    pub fn local(root: impl Into<PathBuf>) -> Self {
        Self::Local {
            root: root.into(),
            public_url: None,
        }
    }

    /// The driver name: `local`, `memory`, `s3`, `gcs` or `azure`.
    pub fn driver(&self) -> &'static str {
        match self {
            Self::Local { .. } => "local",
            Self::Memory => "memory",
            Self::S3(_) => "s3",
            Self::Gcs(_) => "gcs",
            Self::Azure(_) => "azure",
        }
    }

    /// The public URL of the disk root, if any.
    pub(crate) fn public_url(&self) -> Option<&str> {
        match self {
            Self::Local { public_url, .. } => public_url.as_deref(),
            Self::Memory => None,
            Self::S3(config) => config.public_url.as_deref(),
            Self::Gcs(config) => config.public_url.as_deref(),
            Self::Azure(config) => config.public_url.as_deref(),
        }
    }

    /// The key prefix of a cloud disk, if any.
    pub(crate) fn prefix(&self) -> Option<&str> {
        match self {
            Self::Local { .. } | Self::Memory => None,
            Self::S3(config) => config.prefix.as_deref(),
            Self::Gcs(config) => config.prefix.as_deref(),
            Self::Azure(config) => config.prefix.as_deref(),
        }
    }
}

impl From<S3Config> for DiskConfig {
    fn from(config: S3Config) -> Self {
        Self::S3(config)
    }
}

impl From<GcsConfig> for DiskConfig {
    fn from(config: GcsConfig) -> Self {
        Self::Gcs(config)
    }
}

impl From<AzureConfig> for DiskConfig {
    fn from(config: AzureConfig) -> Self {
        Self::Azure(config)
    }
}

/// An S3 disk (`driver = "s3"`).
///
/// ```toml
/// [storage.disks.uploads]
/// driver = "s3"
/// bucket = "acme-uploads"
/// region = "eu-west-1"
/// access_key = "AKIA..."            # or AWS_ACCESS_KEY_ID, or an IAM role
/// secret_key = "..."                # or AWS_SECRET_ACCESS_KEY
/// prefix = "production"             # optional: keys live under production/
///
/// [storage.disks.r2]
/// driver = "s3"
/// bucket = "acme-media"
/// endpoint = "https://<account-id>.r2.cloudflarestorage.com"
/// region = "auto"
/// ```
///
/// Settings missing here come from the standard AWS environment variables:
/// `AWS_REGION` (or `AWS_DEFAULT_REGION`), `AWS_ENDPOINT_URL`, and
/// credentials from `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY` and
/// `AWS_SESSION_TOKEN`, a web identity (`AWS_WEB_IDENTITY_TOKEN_FILE` with
/// `AWS_ROLE_ARN`), the container credentials of ECS and EKS, or the
/// instance metadata service. Keys set here win; with `access_key` and
/// `secret_key` set, no credential is read from the environment.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct S3Config {
    /// The bucket. Required.
    pub bucket: String,
    /// The region (default `AWS_REGION`, then `us-east-1`; `auto` for R2).
    pub region: Option<String>,
    /// The endpoint of an S3-compatible store (`http://127.0.0.1:9000`,
    /// `https://<account-id>.r2.cloudflarestorage.com`,
    /// `https://nyc3.digitaloceanspaces.com`); default `AWS_ENDPOINT_URL`,
    /// then AWS. An `http://` endpoint allows plain HTTP.
    pub endpoint: Option<String>,
    /// The access key id.
    pub access_key: Option<String>,
    /// The secret access key.
    pub secret_key: Option<Secret<String>>,
    /// A session token, for temporary credentials.
    pub session_token: Option<Secret<String>>,
    /// Address the bucket in the path (`https://endpoint/bucket/key`)
    /// rather than as a subdomain (`https://bucket.endpoint/key`). Default:
    /// path-style with an `endpoint` (MinIO and other stores reached by IP
    /// or a single host), virtual-hosted-style on AWS.
    pub path_style: Option<bool>,
    /// A key prefix: every path of the disk lives under it (`tenants/acme`).
    pub prefix: Option<String>,
    /// The URL the bucket (or the prefix) is publicly served from, such as
    /// a CDN, for [`Disk::url`](crate::Disk::url).
    pub public_url: Option<String>,
}

impl S3Config {
    /// An S3 disk on `bucket`, with every other setting from the
    /// environment.
    pub fn new(bucket: impl Into<String>) -> Self {
        Self {
            bucket: bucket.into(),
            ..Self::default()
        }
    }

    /// Sets the region.
    pub fn region(mut self, region: impl Into<String>) -> Self {
        self.region = Some(region.into());
        self
    }

    /// Sets the endpoint of an S3-compatible store.
    pub fn endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }

    /// Sets the access key id and the secret access key.
    pub fn credentials(
        mut self,
        access_key: impl Into<String>,
        secret_key: impl Into<String>,
    ) -> Self {
        self.access_key = Some(access_key.into());
        self.secret_key = Some(Secret::new(secret_key.into()));
        self
    }

    /// Chooses path-style (`true`) or virtual-hosted-style (`false`) URLs.
    pub fn path_style(mut self, path_style: bool) -> Self {
        self.path_style = Some(path_style);
        self
    }

    /// Puts every path of the disk under `prefix`.
    pub fn prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }

    /// Sets the public URL, for [`Disk::url`](crate::Disk::url).
    pub fn public_url(mut self, public_url: impl Into<String>) -> Self {
        self.public_url = Some(public_url.into());
        self
    }
}

/// A Google Cloud Storage disk (`driver = "gcs"`).
///
/// ```toml
/// [storage.disks.media]
/// driver = "gcs"
/// bucket = "acme-media"
/// service_account_path = "/run/secrets/gcs.json"   # optional
/// ```
///
/// Without `service_account_path` or `service_account_key`, credentials
/// come from `GOOGLE_SERVICE_ACCOUNT` / `GOOGLE_SERVICE_ACCOUNT_KEY`, then
/// `GOOGLE_APPLICATION_CREDENTIALS`, then the metadata server (workload
/// identity on GKE and Cloud Run). Signed URLs need a service account key,
/// or the metadata server's account with the `iam.serviceAccounts.signBlob`
/// permission.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GcsConfig {
    /// The bucket. Required.
    pub bucket: String,
    /// The path of a service account key file (JSON).
    pub service_account_path: Option<PathBuf>,
    /// The contents of a service account key file (JSON).
    pub service_account_key: Option<Secret<String>>,
    /// Another endpoint than `https://storage.googleapis.com` (an emulator).
    pub endpoint: Option<String>,
    /// A key prefix: every path of the disk lives under it.
    pub prefix: Option<String>,
    /// The URL the bucket (or the prefix) is publicly served from.
    pub public_url: Option<String>,
}

impl GcsConfig {
    /// A Google Cloud Storage disk on `bucket`.
    pub fn new(bucket: impl Into<String>) -> Self {
        Self {
            bucket: bucket.into(),
            ..Self::default()
        }
    }
}

/// An Azure Blob Storage disk (`driver = "azure"`).
///
/// ```toml
/// [storage.disks.archive]
/// driver = "azure"
/// account = "acmestorage"
/// container = "archive"
/// access_key = "..."   # or AZURE_STORAGE_ACCOUNT_KEY, or a managed identity
/// ```
///
/// Settings missing here come from the `AZURE_*` environment variables
/// (`AZURE_STORAGE_ACCOUNT_NAME`, `AZURE_STORAGE_ACCOUNT_KEY`, a service
/// principal with `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET` and
/// `AZURE_TENANT_ID`, workload identity), then the managed identity.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AzureConfig {
    /// The storage account (default `AZURE_STORAGE_ACCOUNT_NAME`).
    pub account: Option<String>,
    /// The container. Required (`bucket` is accepted as another name).
    #[serde(alias = "bucket")]
    pub container: String,
    /// The account's access key (base64).
    pub access_key: Option<Secret<String>>,
    /// Another endpoint than `https://<account>.blob.core.windows.net`, such
    /// as `http://127.0.0.1:10000/devstoreaccount1` for Azurite.
    pub endpoint: Option<String>,
    /// A key prefix: every path of the disk lives under it.
    pub prefix: Option<String>,
    /// The URL the container (or the prefix) is publicly served from.
    pub public_url: Option<String>,
}

impl AzureConfig {
    /// An Azure disk on `container` of `account`.
    pub fn new(account: impl Into<String>, container: impl Into<String>) -> Self {
        Self {
            account: Some(account.into()),
            container: container.into(),
            ..Self::default()
        }
    }
}

impl ConfigSection for StorageOptions {
    const KEY: &'static str = "storage";

    /// The default disk must exist; disk names use `[A-Za-z0-9_-]`; local
    /// roots and public URLs are not empty; buckets and containers are set;
    /// endpoints and `base_url` are absolute `http(s)` URLs; prefixes are
    /// valid paths; a cloud driver is compiled in.
    fn validate(&self) -> Result<(), String> {
        if !self.disks.contains_key(&self.default) {
            return Err(format!(
                "the default disk `{}` is not configured under `storage.disks`",
                self.default
            ));
        }
        for (name, disk) in &self.disks {
            if name.is_empty()
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            {
                return Err(format!(
                    "disk name `{name}` must be made of letters, digits, `-` and `_`"
                ));
            }
            validate_disk(disk).map_err(|error| format!("disk `{name}`: {error}"))?;
        }
        if let Some(base_url) = &self.base_url
            && !is_http_url(base_url)
        {
            return Err("base_url must be an absolute http(s) URL".to_owned());
        }
        Ok(())
    }
}

/// Checks one disk; the error is prefixed with the disk name by the caller.
fn validate_disk(disk: &DiskConfig) -> Result<(), String> {
    if disk
        .public_url()
        .is_some_and(|url| url.trim().is_empty())
    {
        return Err("public_url must not be empty".to_owned());
    }
    if let Some(prefix) = disk.prefix()
        && normalize(prefix, false).is_err()
    {
        return Err(format!(
            "prefix `{prefix}` must be a relative path without `..` segments"
        ));
    }
    match disk {
        DiskConfig::Local { root, .. } => {
            if root.as_os_str().is_empty() {
                return Err("the root directory must not be empty".to_owned());
            }
        }
        DiskConfig::Memory => {}
        DiskConfig::S3(config) => {
            compiled_in("s3", cfg!(feature = "s3"), "storage-s3")?;
            required("bucket", &config.bucket, "s3")?;
            endpoint(config.endpoint.as_deref())?;
            if config.access_key.is_some() != config.secret_key.is_some() {
                return Err("set both access_key and secret_key, or neither".to_owned());
            }
        }
        DiskConfig::Gcs(config) => {
            compiled_in("gcs", cfg!(feature = "gcs"), "storage-gcs")?;
            required("bucket", &config.bucket, "gcs")?;
            endpoint(config.endpoint.as_deref())?;
            if config.service_account_path.is_some() && config.service_account_key.is_some() {
                return Err(
                    "set service_account_path or service_account_key, not both".to_owned()
                );
            }
        }
        DiskConfig::Azure(config) => {
            compiled_in("azure", cfg!(feature = "azure"), "storage-azure")?;
            required("container", &config.container, "azure")?;
            endpoint(config.endpoint.as_deref())?;
            if config
                .account
                .as_deref()
                .is_some_and(|account| account.trim().is_empty())
            {
                return Err("account must not be empty".to_owned());
            }
        }
    }
    Ok(())
}

/// A driver whose cargo feature is off fails the boot, naming the feature.
fn compiled_in(driver: &str, enabled: bool, facade_feature: &str) -> Result<(), String> {
    if enabled {
        Ok(())
    } else {
        Err(format!(
            "the `{driver}` driver is not compiled in: turn on navree's `{facade_feature}` \
             feature (navree-storage's `{driver}`)"
        ))
    }
}

/// `value` is set.
fn required(key: &str, value: &str, driver: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("`{key}` is required for the {driver} driver"))
    } else {
        Ok(())
    }
}

/// An endpoint, when set, is an absolute http(s) URL.
fn endpoint(endpoint: Option<&str>) -> Result<(), String> {
    match endpoint {
        Some(url) if !is_http_url(url) => {
            Err(format!("endpoint `{url}` must be an absolute http(s) URL"))
        }
        _ => Ok(()),
    }
}

/// `http://` or `https://` followed by a host.
fn is_http_url(url: &str) -> bool {
    url.strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .is_some_and(|rest| !rest.is_empty() && !rest.starts_with('/'))
}
