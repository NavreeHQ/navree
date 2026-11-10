//! [`Storage`]: the configured disks.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use navree_config::ConfigSection;

use crate::signing::Signer;
use crate::{Disk, StorageError, StorageOptions, drivers};

/// The app's disks, by name (what `StorageModule` provides).
#[derive(Clone)]
pub struct Storage {
    default: String,
    disks: Arc<BTreeMap<String, Disk>>,
}

impl Storage {
    /// Builds the disks of `options`. The app's own links (temporary URLs
    /// and presigned uploads of local and memory disks) are signed with an
    /// HMAC-SHA-256 keyed by `signing_key`.
    ///
    /// This blocks: it creates the root directories of local disks and may
    /// read cloud credential files. Inside an async runtime call it from
    /// `tokio::task::spawn_blocking` (the module does). Cloud disks make no
    /// request until they are used.
    pub fn new(options: &StorageOptions, signing_key: &[u8]) -> Result<Self, StorageError> {
        options.validate().map_err(StorageError::InvalidOptions)?;
        let signer = Arc::new(Signer::new(signing_key));
        let mut disks = BTreeMap::new();
        for (name, config) in &options.disks {
            let backend = drivers::build(name, config)?;
            let disk = Disk::new(
                name.clone(),
                config.driver(),
                backend,
                config.public_url(),
                options.base_url.as_deref(),
                signer.clone(),
            );
            disks.insert(name.clone(), disk);
        }
        Ok(Self {
            default: options.default.clone(),
            disks: Arc::new(disks),
        })
    }

    /// The disk called `name`.
    pub fn disk(&self, name: &str) -> Result<Disk, StorageError> {
        self.disks
            .get(name)
            .cloned()
            .ok_or_else(|| StorageError::UnknownDisk {
                disk: name.to_owned(),
            })
    }

    /// The names of the configured disks, sorted.
    pub fn disk_names(&self) -> impl Iterator<Item = &str> {
        self.disks.keys().map(String::as_str)
    }

    /// The default disk.
    pub fn default_disk(&self) -> Disk {
        self.disks[&self.default].clone()
    }
}

impl fmt::Debug for Storage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Storage")
            .field("default", &self.default)
            .field("disks", &self.disks.values().collect::<Vec<_>>())
            .finish()
    }
}
