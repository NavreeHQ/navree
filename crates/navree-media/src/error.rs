//! [`MediaError`] and [`ArchiveError`], and how they become responses.

use navree_db::sea_orm::DbErr;
use navree_http::AppError;
use navree_orm::OrmError;
use navree_storage::StorageError;
use uuid::Uuid;

/// Everything that can go wrong attaching, converting, downloading,
/// unpacking or reading a file.
///
/// Returned from a handler with `?`, it becomes an error response (see
/// [`MediaError::status`] and [`MediaError::code`]): refusals of the
/// file itself are 4xx with a code of their own, the rest 500.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum MediaError {
    /// The model declares collections, and not this one.
    #[error("{model_type} has no media collection `{collection}`")]
    UnknownCollection {
        /// The model type (`HasMedia::MEDIA_TYPE`).
        model_type: String,
        /// The collection asked for.
        collection: String,
    },
    /// The collection does not accept files of this type.
    #[error("files of type {mime_type} are not accepted in `{collection}`")]
    TypeNotAllowed {
        /// The collection.
        collection: String,
        /// The type sniffed from the content.
        mime_type: String,
    },
    /// The file is bigger than the collection (or `media.max_file_size`)
    /// allows.
    #[error("the file must not exceed {max} bytes")]
    TooLarge {
        /// The limit in bytes.
        max: u64,
    },
    /// The file has no content.
    #[error("the file is empty")]
    Empty,
    /// A remote file could not be downloaded (connection, status or
    /// timeout).
    #[error("cannot download {url}: {reason}")]
    Download {
        /// The URL, without its query string.
        url: String,
        /// What went wrong.
        reason: String,
    },
    /// A remote file's address is refused by the SSRF guard.
    #[error("{url} is not allowed: {reason}")]
    Blocked {
        /// The URL, without its query string.
        url: String,
        /// Why the address is refused.
        reason: String,
    },
    /// The image cannot be decoded, is too big to decode, or cannot be
    /// encoded.
    #[error("invalid image: {message}")]
    Image {
        /// What went wrong.
        message: String,
    },
    /// A zip archive is invalid or unsafe.
    #[error(transparent)]
    Archive(#[from] ArchiveError),
    /// Text cannot be extracted from a document that claims a supported
    /// type.
    #[error("cannot read the document: {message}")]
    Text {
        /// What went wrong.
        message: String,
    },
    /// The operation does not support this type of file.
    #[error("{operation} is not supported for {mime_type}")]
    Unsupported {
        /// What was asked (`text extraction`, `conversion`).
        operation: &'static str,
        /// The file's type.
        mime_type: String,
    },
    /// No media item has this id.
    #[error("media {id} not found")]
    NotFound {
        /// The id.
        id: Uuid,
    },
    /// The media item has no generated conversion of this name.
    #[error("media {id} has no conversion `{conversion}`")]
    MissingConversion {
        /// The media item.
        id: Uuid,
        /// The conversion asked for.
        conversion: String,
    },
    /// The disk has no public URL; use a temporary URL.
    #[error("the disk `{disk}` has no public URL; use `temporary_url`")]
    NoPublicUrl {
        /// The disk.
        disk: String,
    },
    /// FFmpeg or ffprobe failed, timed out or is not installed.
    #[error("ffmpeg: {message}")]
    Ffmpeg {
        /// What went wrong (the tool's last error lines).
        message: String,
    },
    /// A local file could not be read ([`MediaSource::file`](crate::MediaSource::file))
    /// or a temporary file written.
    #[error("{path}: {message}")]
    File {
        /// The file.
        path: String,
        /// What went wrong.
        message: String,
    },
    /// The conversion job could not be queued.
    #[error("cannot queue the conversions: {message}")]
    Queue {
        /// The dispatcher's error.
        message: String,
    },
    /// A disk operation failed.
    #[error(transparent)]
    Storage(#[from] StorageError),
    /// A database operation failed.
    #[error(transparent)]
    Db(#[from] DbErr),
}

impl From<OrmError> for MediaError {
    fn from(error: OrmError) -> Self {
        MediaError::Db(error.into())
    }
}

impl MediaError {
    /// The HTTP status the error answers with.
    pub fn status(&self) -> u16 {
        match self {
            MediaError::UnknownCollection { .. }
            | MediaError::TypeNotAllowed { .. }
            | MediaError::Empty
            | MediaError::Download { .. }
            | MediaError::Blocked { .. }
            | MediaError::Image { .. }
            | MediaError::Text { .. } => 400,
            MediaError::Archive(error) => error.status(),
            MediaError::TooLarge { .. } => 413,
            MediaError::Unsupported { .. } => 415,
            MediaError::NotFound { .. } | MediaError::MissingConversion { .. } => 404,
            MediaError::NoPublicUrl { .. }
            | MediaError::Ffmpeg { .. }
            | MediaError::File { .. }
            | MediaError::Queue { .. }
            | MediaError::Storage(_)
            | MediaError::Db(_) => 500,
        }
    }

    /// The machine-readable `code` of the error response.
    pub fn code(&self) -> &'static str {
        match self {
            MediaError::UnknownCollection { .. } => "unknown_media_collection",
            MediaError::TypeNotAllowed { .. } => "file_type_not_allowed",
            MediaError::TooLarge { .. } => "payload_too_large",
            MediaError::Empty => "empty_file",
            MediaError::Download { .. } => "remote_file_unavailable",
            MediaError::Blocked { .. } => "remote_file_blocked",
            MediaError::Image { .. } => "invalid_image",
            MediaError::Archive(error) => error.code(),
            MediaError::Text { .. } => "invalid_document",
            MediaError::Unsupported { .. } => "unsupported_media_type",
            MediaError::NotFound { .. } => "media_not_found",
            MediaError::MissingConversion { .. } => "conversion_not_found",
            MediaError::NoPublicUrl { .. }
            | MediaError::Ffmpeg { .. }
            | MediaError::File { .. }
            | MediaError::Queue { .. }
            | MediaError::Storage(_)
            | MediaError::Db(_) => "internal_error",
        }
    }

    pub(crate) fn image(message: impl std::fmt::Display) -> Self {
        MediaError::Image {
            message: message.to_string(),
        }
    }

    pub(crate) fn text(message: impl std::fmt::Display) -> Self {
        MediaError::Text {
            message: message.to_string(),
        }
    }
}

/// Why a zip archive is refused or cannot be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ArchiveError {
    /// Not a zip archive, or a damaged one.
    #[error("invalid archive: {message}")]
    Invalid {
        /// What the reader reported.
        message: String,
    },
    /// An entry would be written outside the destination: an absolute
    /// path, a drive letter, a `..` segment or a control character.
    #[error("the archive entry `{path}` has an unsafe path")]
    UnsafePath {
        /// The entry's name as stored.
        path: String,
    },
    /// The archive holds a symbolic link.
    #[error("the archive entry `{path}` is a symbolic link")]
    Symlink {
        /// The entry's name.
        path: String,
    },
    /// The archive holds an encrypted entry.
    #[error("the archive entry `{path}` is encrypted")]
    Encrypted {
        /// The entry's name.
        path: String,
    },
    /// Two entries have the same path.
    #[error("the archive holds `{path}` twice")]
    Duplicate {
        /// The path.
        path: String,
    },
    /// More entries than `max_entries`.
    #[error("the archive holds more than {max} entries")]
    TooManyEntries {
        /// The limit.
        max: usize,
    },
    /// One entry unpacks to more than `max_entry_bytes`.
    #[error("the archive entry `{path}` unpacks to more than {max} bytes")]
    EntryTooLarge {
        /// The entry.
        path: String,
        /// The limit.
        max: u64,
    },
    /// The entries unpack to more than `max_total_bytes` together.
    #[error("the archive unpacks to more than {max} bytes")]
    TooLarge {
        /// The limit.
        max: u64,
    },
    /// An entry is compressed more than `max_ratio` to one (a zip bomb).
    #[error("the archive entry `{path}` is compressed more than {max} to 1")]
    Ratio {
        /// The entry.
        path: String,
        /// The limit.
        max: u64,
    },
    /// No entry has this path.
    #[error("the archive has no entry `{path}`")]
    NotFound {
        /// The path asked for.
        path: String,
    },
}

impl ArchiveError {
    /// The HTTP status the error answers with: 413 for the size limits,
    /// 404 for a missing entry, 400 otherwise.
    pub fn status(&self) -> u16 {
        match self {
            ArchiveError::TooManyEntries { .. }
            | ArchiveError::EntryTooLarge { .. }
            | ArchiveError::TooLarge { .. }
            | ArchiveError::Ratio { .. } => 413,
            ArchiveError::NotFound { .. } => 404,
            _ => 400,
        }
    }

    /// The machine-readable `code`: `invalid_archive`, `unsafe_archive`,
    /// `archive_too_large` or `not_found`.
    pub fn code(&self) -> &'static str {
        match self {
            ArchiveError::Invalid { .. } => "invalid_archive",
            ArchiveError::UnsafePath { .. }
            | ArchiveError::Symlink { .. }
            | ArchiveError::Encrypted { .. }
            | ArchiveError::Duplicate { .. } => "unsafe_archive",
            ArchiveError::TooManyEntries { .. }
            | ArchiveError::EntryTooLarge { .. }
            | ArchiveError::TooLarge { .. }
            | ArchiveError::Ratio { .. } => "archive_too_large",
            ArchiveError::NotFound { .. } => "not_found",
        }
    }
}

/// The response for a [`MediaError`] returned from a handler.
pub(crate) fn map_media_error(error: &MediaError) -> Option<AppError> {
    match error {
        MediaError::Storage(_) | MediaError::Db(_) => {
            let mapped = match error {
                MediaError::Db(db) => navree_db::map_db_error(db),
                _ => None,
            };
            Some(mapped.unwrap_or_else(|| AppError::internal(error.to_string())))
        }
        _ if error.status() >= 500 => Some(AppError::internal(error.to_string())),
        _ => Some(app_error(error.status(), error.code(), error.to_string())),
    }
}

/// The response for an [`ArchiveError`] returned from a handler.
pub(crate) fn map_archive_error(error: &ArchiveError) -> Option<AppError> {
    Some(app_error(error.status(), error.code(), error.to_string()))
}

fn app_error(status: u16, code: &str, message: String) -> AppError {
    let status = http::StatusCode::from_u16(status).unwrap_or(http::StatusCode::BAD_REQUEST);
    let title = status.canonical_reason().unwrap_or("Bad Request");
    AppError::new(status, code, title).with_detail(message)
}
