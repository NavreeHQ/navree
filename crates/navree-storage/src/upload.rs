//! [`store_upload`]: streams a multipart file field to a disk, checking its
//! size and its sniffed MIME type.

use std::collections::BTreeMap;
use std::fmt;

use axum::extract::multipart::{Field, MultipartError};
use axum::http::StatusCode;
use bytes::{Bytes, BytesMut};
use futures_util::stream;
use navree_crypto::random_token;
use navree_http::AppError;
use serde::{Deserialize, Serialize};

use crate::path::normalize;
use crate::{Disk, StorageError};

/// Bytes read before sniffing the content type (enough for every format
/// `infer` recognises by its leading bytes).
pub(crate) const SNIFF_BYTES: usize = 8 * 1024;

/// What [`store_upload`] accepts.
///
/// ```
/// use navree_storage::UploadRules;
///
/// let avatars = UploadRules::new(2 * 1024 * 1024, ["image/png", "image/jpeg"]);
/// assert!(avatars.allows("image/png"));
/// let documents = UploadRules::new(10 * 1024 * 1024, ["application/pdf", "image/*"]);
/// assert!(documents.allows("image/webp"));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UploadRules {
    /// The largest accepted file, in bytes.
    pub max_bytes: u64,
    /// Accepted MIME types (`image/png`), or wildcards (`image/*`);
    /// case-insensitive. Empty accepts any type.
    pub allowed_mime: Vec<String>,
}

impl UploadRules {
    /// Rules accepting files up to `max_bytes` of the `allowed_mime` types.
    pub fn new(max_bytes: u64, allowed_mime: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            max_bytes,
            allowed_mime: allowed_mime.into_iter().map(Into::into).collect(),
        }
    }

    /// Whether a file of type `mime` is accepted.
    pub fn allows(&self, mime: &str) -> bool {
        self.allowed_mime.is_empty()
            || self.allowed_mime.iter().any(|allowed| {
                allowed.eq_ignore_ascii_case(mime)
                    || allowed == "*/*"
                    || allowed.strip_suffix('*').is_some_and(|prefix| {
                        prefix.ends_with('/')
                            && mime.len() > prefix.len()
                            && mime
                                .get(..prefix.len())
                                .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
                    })
            })
    }
}

/// A file stored by [`store_upload`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredFile {
    /// The disk it was stored on.
    pub disk: String,
    /// Its path on the disk: `<dir>/<40 random characters>.<extension>`.
    pub path: String,
    /// Its size in bytes.
    pub size: u64,
    /// The MIME type sniffed from its contents (not the one the client sent).
    pub mime: String,
    /// The file name the client sent, without directories. Untrusted:
    /// display it, never use it as a path.
    pub original_name: Option<String>,
}

/// Streams the multipart file `field` to `disk` under `dir`, with a random
/// 40-character name and an extension matching its content.
///
/// The type is sniffed from the first bytes with `infer`, never taken from
/// the client: a renamed executable is not an image. Content `infer` does
/// not recognise is `text/*` (by the file name's extension, `text/plain`
/// otherwise) when it is UTF-8 text, else `application/octet-stream`; so an
/// SVG or other markup is never accepted as an image.
///
/// Errors (in Navree's JSON error shape):
/// - 400 validation errors, keyed by the field name, for an empty file or
///   a type `rules` does not allow (nothing is written);
/// - 413 for a file over `rules.max_bytes`, detected
///   while streaming (the partial file is discarded) or a request body over
///   the route's body limit;
/// - 400 for a malformed multipart body, 500 for a storage failure.
///
/// ```no_run
/// use axum::extract::Multipart;
/// use navree_http::{AppError, Json};
/// use navree_storage::{Disk, StoredFile, UploadRules, store_upload};
///
/// async fn upload_avatar(disk: &Disk, mut form: Multipart) -> Result<Json<StoredFile>, AppError> {
///     let field = form
///         .next_field()
///         .await?
///         .ok_or_else(|| AppError::bad_request("avatar should not be empty"))?;
///     let rules = UploadRules::new(2 * 1024 * 1024, ["image/png", "image/jpeg"]);
///     Ok(Json(store_upload(field, disk, "avatars", rules).await?))
/// }
/// ```
pub async fn store_upload(
    mut field: Field<'_>,
    disk: &Disk,
    dir: &str,
    rules: UploadRules,
) -> Result<StoredFile, AppError> {
    let field_name = field.name().unwrap_or("file").to_owned();
    let original_name = field.file_name().and_then(base_name);
    let dir = normalize(dir, true).map_err(AppError::internal)?;
    let too_large = || {
        AppError::from_status(StatusCode::PAYLOAD_TOO_LARGE).with_detail(format!(
            "{field_name} must not be larger than {} bytes",
            rules.max_bytes
        ))
    };

    // Read enough to sniff the type before anything is written.
    let mut head = BytesMut::new();
    let mut finished = false;
    while head.len() < SNIFF_BYTES {
        match field.chunk().await.map_err(multipart_error)? {
            Some(chunk) => head.extend_from_slice(&chunk),
            None => {
                finished = true;
                break;
            }
        }
        if head.len() as u64 > rules.max_bytes {
            return Err(too_large());
        }
    }
    if head.is_empty() {
        return Err(invalid(
            &field_name,
            format!("{field_name} should not be empty"),
        ));
    }
    let sniffed = sniff(&head, original_name.as_deref());
    if !rules.allows(&sniffed.mime) {
        return Err(invalid(
            &field_name,
            format!(
                "{field_name} must be a file of type: {}",
                rules.allowed_mime.join(", ")
            ),
        ));
    }

    let file_name = format!("{}.{}", random_token(40), sniffed.extension);
    let path = if dir.is_empty() {
        file_name
    } else {
        format!("{dir}/{file_name}")
    };
    let rest = (!finished).then_some(field);
    let chunks = limited(head.freeze(), rest, rules.max_bytes);
    match disk.put_stream(&path, chunks).await {
        Ok(size) => Ok(StoredFile {
            disk: disk.name().to_owned(),
            path,
            size,
            mime: sniffed.mime,
            original_name,
        }),
        Err(StorageError::Stream { source }) => match source.downcast::<UploadError>() {
            Ok(error) => match *error {
                UploadError::TooLarge => Err(too_large()),
                UploadError::Multipart(error) => Err(multipart_error(error)),
            },
            Err(source) => Err(AppError::internal(source)),
        },
        Err(error) => Err(AppError::internal(error)),
    }
}

/// The sniffed type of an upload and the extension it is stored with.
pub(crate) struct Sniffed {
    pub(crate) mime: String,
    pub(crate) extension: String,
}

/// Sniffs the type of `head` (see [`store_upload`]).
pub(crate) fn sniff(head: &[u8], original_name: Option<&str>) -> Sniffed {
    if let Some(kind) = infer::get(head) {
        return Sniffed {
            mime: kind.mime_type().to_owned(),
            extension: kind.extension().to_owned(),
        };
    }
    if !is_text(head) {
        return Sniffed {
            mime: "application/octet-stream".to_owned(),
            extension: "bin".to_owned(),
        };
    }
    // Text: trust the file name only for a `text/*` type.
    let named = original_name.and_then(|name| {
        let extension = name.rsplit_once('.')?.1.to_ascii_lowercase();
        let valid = !extension.is_empty()
            && extension.len() <= 10
            && extension.bytes().all(|b| b.is_ascii_alphanumeric());
        let mime = mime_guess::from_ext(&extension).first()?;
        (valid && mime.type_() == mime_guess::mime::TEXT).then(|| Sniffed {
            mime: mime.essence_str().to_owned(),
            extension,
        })
    });
    named.unwrap_or_else(|| Sniffed {
        mime: "text/plain".to_owned(),
        extension: "txt".to_owned(),
    })
}

/// UTF-8 without NUL bytes (the head may end inside a character).
fn is_text(head: &[u8]) -> bool {
    !head.contains(&0)
        && match std::str::from_utf8(head) {
            Ok(_) => true,
            Err(error) => error.error_len().is_none(),
        }
}

/// The last component of a client file name (`C:\fakepath\a.png` → `a.png`).
fn base_name(name: &str) -> Option<String> {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name).trim();
    (!base.is_empty()).then(|| base.chars().filter(|c| !c.is_control()).collect())
}

/// Why the chunk stream of an upload stopped.
#[derive(Debug)]
enum UploadError {
    TooLarge,
    Multipart(MultipartError),
}

impl fmt::Display for UploadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => f.write_str("the upload is larger than allowed"),
            Self::Multipart(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for UploadError {}

/// `head` then the rest of `field`, failing with [`UploadError::TooLarge`]
/// once more than `max_bytes` have been produced.
fn limited<'a>(
    head: Bytes,
    field: Option<Field<'a>>,
    max_bytes: u64,
) -> impl futures_util::Stream<Item = Result<Bytes, UploadError>> + 'a {
    stream::try_unfold(
        (Some(head), field, 0u64),
        move |(head, mut field, total)| async move {
            let chunk = match (head, field.as_mut()) {
                (Some(head), _) => Some(head),
                (None, Some(field)) => field.chunk().await.map_err(UploadError::Multipart)?,
                (None, None) => None,
            };
            let Some(chunk) = chunk else {
                return Ok(None);
            };
            let total = total + chunk.len() as u64;
            if total > max_bytes {
                return Err(UploadError::TooLarge);
            }
            Ok(Some((chunk, (None, field, total))))
        },
    )
}

/// A 400 with one message for `field`.
fn invalid(field: &str, message: String) -> AppError {
    AppError::validation(BTreeMap::from([(field.to_owned(), vec![message])]))
}

/// A multipart parsing failure: its own status (400, 413, ...), and its
/// message as the error message of client errors.
fn multipart_error(error: MultipartError) -> AppError {
    match client_error(&error) {
        Some(app_error) => app_error,
        None => AppError::internal(error),
    }
}

/// The client error (4xx) a multipart failure stands for; `None` for a
/// server-side failure. Registered by `StorageModule` as an error mapper, so
/// `multipart.next_field().await?` in a handler keeps the status.
pub(crate) fn client_error(error: &MultipartError) -> Option<AppError> {
    let status = error.status();
    status
        .is_client_error()
        .then(|| AppError::from_status(status).with_detail(error.body_text()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_detection_tolerates_a_cut_character() {
        assert!(is_text("héllo".as_bytes()));
        assert!(is_text(&"é".as_bytes()[..1]));
        assert!(!is_text(b"a\0b"));
        assert!(!is_text(b"\xff\xfe\xfd"));
    }

    #[test]
    fn base_name_strips_directories() {
        assert_eq!(base_name(r"C:\fakepath\a.png").as_deref(), Some("a.png"));
        assert_eq!(base_name("../../etc/passwd").as_deref(), Some("passwd"));
        assert_eq!(base_name("a\nb.txt").as_deref(), Some("ab.txt"));
        assert_eq!(base_name("dir/").as_deref(), None);
    }

    #[test]
    fn sniffing_ignores_markup_disguised_as_images() {
        let svg = sniff(b"<svg onload=alert(1)>", Some("x.svg"));
        assert_eq!(svg.mime, "text/plain");
        let html = sniff(b"<html><body>", Some("x.png"));
        assert_eq!(html.mime, "text/html");
        let csv = sniff(b"a,b\n", Some("data.CSV"));
        assert_eq!(
            (csv.mime.as_str(), csv.extension.as_str()),
            ("text/csv", "csv")
        );
        let odd = sniff(b"a,b\n", Some("data.c$v"));
        assert_eq!(odd.extension, "txt");
    }
}
