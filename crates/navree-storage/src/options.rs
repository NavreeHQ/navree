//! [`StorageOptions`] and [`DiskConfig`].

use std::collections::BTreeMap;
use std::path::PathBuf;

use navree_config::ConfigSection;
use serde::{Deserialize, Serialize};

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
/// [storage.disks.scratch]
/// driver = "memory"
/// ```
///
/// Without a `storage` section there is one disk, `local`, rooted at
/// `storage/app` (relative to the working directory). Defining `disks`
/// replaces that default disk.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StorageOptions {
    /// The name of the default disk (default `local`).
    pub default: String,
    /// The disks, by name. Names use `[A-Za-z0-9_-]`.
    pub disks: BTreeMap<String, DiskConfig>,
    /// An absolute base URL (`https://api.example.com`) prepended to
    /// [temporary URLs](crate::Disk::temporary_url); without one they are
    /// path-only (`/storage/<disk>/<path>?expires=...&signature=...`).
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

/// One disk: where its files live.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        public_url: Option<String>,
    },
    /// Files in process memory, lost on restart (`driver = "memory"`); for
    /// tests and scratch data.
    Memory,
}

impl DiskConfig {
    /// A local disk rooted at `root`, with no public URL.
    pub fn local(root: impl Into<PathBuf>) -> Self {
        Self::Local {
            root: root.into(),
            public_url: None,
        }
    }
}

impl ConfigSection for StorageOptions {
    const KEY: &'static str = "storage";

    /// The default disk must exist; disk names use `[A-Za-z0-9_-]`; local
    /// roots and public URLs are not empty; `base_url` is an absolute
    /// `http(s)` URL.
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
            if let DiskConfig::Local { root, public_url } = disk {
                if root.as_os_str().is_empty() {
                    return Err(format!(
                        "disk `{name}`: the root directory must not be empty"
                    ));
                }
                if public_url
                    .as_deref()
                    .is_some_and(|url| url.trim().is_empty())
                {
                    return Err(format!("disk `{name}`: public_url must not be empty"));
                }
            }
        }
        if let Some(base_url) = &self.base_url
            && !(base_url.starts_with("https://") || base_url.starts_with("http://"))
        {
            return Err("base_url must be an absolute http(s) URL".to_owned());
        }
        Ok(())
    }
}
