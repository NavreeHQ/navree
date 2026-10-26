//! [`StorageError`].

use std::path::PathBuf;

use navree_kernel::BoxError;

/// Errors of storage operations. Messages name the disk and the path, never
/// file contents.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StorageError {
    /// The path is not a valid relative file path (it contains `..`, control
    /// characters, ...).
    #[error("invalid storage path {path:?}: {reason}")]
    InvalidPath {
        /// The path as given.
        path: String,
        /// Why it was rejected.
        reason: &'static str,
    },

    /// No disk with this name is configured.
    #[error("no storage disk named `{disk}` is configured")]
    UnknownDisk {
        /// The requested disk name.
        disk: String,
    },

    /// The file does not exist.
    #[error("file `{path}` not found on disk `{disk}`")]
    NotFound {
        /// The disk name.
        disk: String,
        /// The normalised path.
        path: String,
    },

    /// [`Disk::url`](crate::Disk::url) on a disk without a `public_url`.
    #[error("disk `{disk}` has no public URL")]
    NoPublicUrl {
        /// The disk name.
        disk: String,
    },

    /// The source stream of [`Disk::put_stream`](crate::Disk::put_stream)
    /// failed; nothing was stored.
    #[error("the source stream failed: {source}")]
    Stream {
        /// The stream's error.
        #[source]
        source: BoxError,
    },

    /// The storage options are invalid.
    #[error("invalid storage options: {0}")]
    InvalidOptions(String),

    /// The root directory of a local disk could not be created or opened.
    #[error("disk `{disk}`: cannot use root directory {root:?}: {source}")]
    LocalRoot {
        /// The disk name.
        disk: String,
        /// The configured root.
        root: PathBuf,
        /// The underlying error.
        #[source]
        source: BoxError,
    },

    /// The storage backend failed.
    #[error("storage error on disk `{disk}`: {source}")]
    Backend {
        /// The disk name.
        disk: String,
        /// The backend error.
        #[source]
        source: object_store::Error,
    },
}
