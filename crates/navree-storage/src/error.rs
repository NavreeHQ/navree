//! [`StorageError`] and [`UploadRejection`].

use std::fmt;
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

    /// An upload breaks its [`UploadRules`](crate::UploadRules): refused by
    /// [`Disk::presigned_put`](crate::Disk::presigned_put) before any byte
    /// is sent, or by [`Disk::accept_upload`](crate::Disk::accept_upload),
    /// which deletes the file.
    #[error("upload `{path}` on disk `{disk}` rejected: {rejection}")]
    Rejected {
        /// The disk name.
        disk: String,
        /// The normalised path.
        path: String,
        /// Which rule it breaks.
        rejection: UploadRejection,
    },

    /// A link cannot be signed as asked: a presigned upload whose lifetime
    /// is not positive, or a link lasting longer than the provider allows
    /// (7 days on S3 and Google Cloud Storage).
    #[error("disk `{disk}` cannot sign this link: {reason}")]
    Signing {
        /// The disk name.
        disk: String,
        /// Why.
        reason: String,
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

/// Which [`UploadRules`](crate::UploadRules) rule an upload breaks.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum UploadRejection {
    /// The file is empty.
    Empty,
    /// The file is larger than the rules allow.
    TooLarge {
        /// The largest accepted file, in bytes.
        max_bytes: u64,
    },
    /// The file's type is not allowed.
    Type {
        /// The declared or sniffed type.
        mime: String,
        /// The allowed types.
        allowed: Vec<String>,
    },
    /// The rules allow only some types, and no content type was declared.
    MissingType {
        /// The allowed types.
        allowed: Vec<String>,
    },
    /// The declared content type is not a valid `type/subtype` header
    /// value (it is not repeated in the message).
    InvalidType,
}

impl fmt::Display for UploadRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("the file should not be empty"),
            Self::TooLarge { max_bytes } => {
                write!(f, "the file must not be larger than {max_bytes} bytes")
            }
            Self::Type { mime, allowed } => write!(
                f,
                "the file must be of type: {} (it is {mime})",
                allowed.join(", ")
            ),
            Self::MissingType { allowed } => write!(
                f,
                "declare the file's content type (allowed: {})",
                allowed.join(", ")
            ),
            Self::InvalidType => {
                f.write_str("the declared content type is not a valid `type/subtype`")
            }
        }
    }
}
