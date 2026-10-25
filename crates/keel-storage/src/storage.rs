//! [`Storage`]: the configured disks.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use object_store::ObjectStore;
use object_store::local::LocalFileSystem;
use object_store::memory::InMemory;

use keel_config::ConfigSection;

use crate::signing::Signer;
use crate::{Disk, DiskConfig, StorageError, StorageOptions};

/// The app's disks, by name (what `StorageModule` provides).
#[derive(Clone)]
pub struct Storage {
    default: String,
    disks: Arc<BTreeMap<String, Disk>>,
}

impl Storage {
    /// Builds the disks of `options`. Temporary URLs are signed with an
    /// HMAC-SHA-256 keyed by `signing_key`.
    ///
    /// This blocks: it creates the root directories of local disks. Inside
    /// an async runtime call it from `tokio::task::spawn_blocking` (the
    /// module does).
    pub fn new(options: &StorageOptions, signing_key: &[u8]) -> Result<Self, StorageError> {
        options.validate().map_err(StorageError::InvalidOptions)?;
        let signer = Arc::new(Signer::new(signing_key));
        let mut disks = BTreeMap::new();
        for (name, config) in &options.disks {
            let store = build_store(name, config)?;
            let public_url = match config {
                DiskConfig::Local { public_url, .. } => public_url.as_deref(),
                DiskConfig::Memory => None,
            };
            let disk = Disk::new(
                name.clone(),
                store,
                public_url,
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

/// The object store behind one disk.
fn build_store(name: &str, config: &DiskConfig) -> Result<Arc<dyn ObjectStore>, StorageError> {
    match config {
        DiskConfig::Local { root, .. } => {
            let local_root =
                |source: Box<dyn std::error::Error + Send + Sync>| StorageError::LocalRoot {
                    disk: name.to_owned(),
                    root: root.clone(),
                    source,
                };
            std::fs::create_dir_all(root).map_err(|e| local_root(e.into()))?;
            let store = LocalFileSystem::new_with_prefix(root)
                .map_err(|e| local_root(e.into()))?
                .with_automatic_cleanup(true);
            Ok(Arc::new(store))
        }
        DiskConfig::Memory => Ok(Arc::new(InMemory::new())),
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
