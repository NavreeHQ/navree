//! Direct uploads: [`Disk::presigned_put`] signs a link a client uploads to
//! without going through the app, and [`Disk::accept_upload`] checks what
//! arrived.

use std::collections::BTreeMap;
use std::future::{Future, IntoFuture};
use std::pin::Pin;

use chrono::{DateTime, Duration, Utc};
use navree_crypto::random_token;
use serde::{Deserialize, Serialize};

use crate::path::normalize;
use crate::signing::{UploadGrant, encode_component, encode_path};
use crate::upload::{SNIFF_BYTES, sniff};
use crate::{Disk, StorageError, StoredFile, UploadRejection, UploadRules};

/// A signed upload link, returned by [`Disk::presigned_put`]. Send it to the
/// client as JSON: the client sends the file as the body of a `method`
/// request to `url`, with every header of `headers`, before `expires_at`.
///
/// ```json
/// {
///   "disk": "uploads",
///   "path": "avatars/3fQx...k9Ta.png",
///   "method": "PUT",
///   "url": "https://agra-uploads.s3.eu-west-1.amazonaws.com/avatars/3fQx...k9Ta.png?X-Amz-Algorithm=...",
///   "headers": { "content-type": "image/png" },
///   "expires_at": "2026-10-26T12:10:00Z"
/// }
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresignedUpload {
    /// The disk the file goes to.
    pub disk: String,
    /// The file's path on the disk.
    pub path: String,
    /// The HTTP method: `PUT`.
    pub method: String,
    /// Where to send the file.
    pub url: String,
    /// Headers the request must carry, with exactly these values.
    pub headers: BTreeMap<String, String>,
    /// When the link stops working.
    pub expires_at: DateTime<Utc>,
}

/// The request [`Disk::presigned_put`] builds; `.await` it for the
/// [`PresignedUpload`].
#[must_use = "a presigned upload does nothing until it is awaited"]
pub struct PresignPut<'a> {
    disk: &'a Disk,
    path: String,
    ttl: Duration,
    rules: &'a UploadRules,
    content_type: Option<String>,
    size: Option<u64>,
}

impl PresignPut<'_> {
    /// The file's content type, as the client declares it (a browser's
    /// `File.type`). It must be allowed by the rules, and the upload must
    /// send it as its `Content-Type`. Required when the rules allow only
    /// some types.
    pub fn content_type(mut self, content_type: impl Into<String>) -> Self {
        self.content_type = Some(content_type.into());
        self
    }

    /// The file's size in bytes, as the client declares it (a browser's
    /// `File.size`). It must be within the rules; local and memory disks
    /// also refuse a body of another size.
    pub fn size(mut self, bytes: u64) -> Self {
        self.size = Some(bytes);
        self
    }

    async fn sign(self) -> Result<PresignedUpload, StorageError> {
        let disk = self.disk;
        let path = normalize(&self.path, false)?;
        let rejected = |rejection| StorageError::Rejected {
            disk: disk.name().to_owned(),
            path: path.clone(),
            rejection,
        };
        let signing = |reason: &str| StorageError::Signing {
            disk: disk.name().to_owned(),
            reason: reason.to_owned(),
        };
        if let Some(size) = self.size {
            if size == 0 {
                return Err(rejected(UploadRejection::Empty));
            }
            if size > self.rules.max_bytes {
                return Err(rejected(UploadRejection::TooLarge {
                    max_bytes: self.rules.max_bytes,
                }));
            }
        }
        let content_type = self.content_type.map(|value| value.trim().to_owned());
        match &content_type {
            Some(value) => {
                if !is_content_type(value) {
                    return Err(rejected(UploadRejection::InvalidType));
                }
                let essence = essence(value);
                if !self.rules.allows(&essence) {
                    return Err(rejected(UploadRejection::Type {
                        mime: essence,
                        allowed: self.rules.allowed_mime.clone(),
                    }));
                }
            }
            None if !self.rules.allowed_mime.is_empty() => {
                return Err(rejected(UploadRejection::MissingType {
                    allowed: self.rules.allowed_mime.clone(),
                }));
            }
            None => {}
        }
        if self.ttl <= Duration::zero() {
            return Err(signing("the link must stay valid for a positive duration"));
        }
        let expires_at = Utc::now() + self.ttl;

        if let Some(provider) = disk.provider() {
            let ttl = disk.provider_ttl(provider, expires_at)?;
            let (url, headers) = provider
                .put_url(
                    &path,
                    ttl,
                    content_type.as_deref(),
                    self.rules.max_bytes,
                    self.size,
                )
                .await
                .map_err(|error| disk.backend_error(error))?;
            return Ok(PresignedUpload {
                disk: disk.name().to_owned(),
                path,
                method: "PUT".to_owned(),
                url,
                headers,
                expires_at,
            });
        }

        let grant = UploadGrant {
            disk: disk.name(),
            path: &path,
            expires: expires_at.timestamp(),
            content_type: content_type.as_deref(),
            max_bytes: self.rules.max_bytes,
            size: self.size,
        };
        let signature = disk.signer().sign_upload(&grant);
        let mut url = format!(
            "{}/storage/{}/{}?expires={}",
            disk.base_url(),
            disk.name(),
            encode_path(&path),
            grant.expires,
        );
        if let Some(content_type) = grant.content_type {
            url.push_str(&format!("&type={}", encode_component(content_type)));
        }
        url.push_str(&format!("&max={}", grant.max_bytes));
        if let Some(size) = grant.size {
            url.push_str(&format!("&size={size}"));
        }
        url.push_str(&format!("&signature={signature}"));
        let headers = content_type
            .iter()
            .map(|value| ("content-type".to_owned(), value.clone()))
            .collect();
        Ok(PresignedUpload {
            disk: disk.name().to_owned(),
            path,
            method: "PUT".to_owned(),
            url,
            headers,
            expires_at,
        })
    }
}

impl<'a> IntoFuture for PresignPut<'a> {
    type Output = Result<PresignedUpload, StorageError>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send + 'a>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.sign())
    }
}

impl Disk {
    /// A link that lets a client upload one file to `path` for `ttl`,
    /// straight to the bucket, without the file going through the app.
    /// Declare what the client will send with
    /// [`content_type`](PresignPut::content_type) and
    /// [`size`](PresignPut::size), then `.await` it:
    ///
    /// ```no_run
    /// use chrono::Duration;
    /// use navree_storage::{Disk, PresignedUpload, StorageError, UploadRules, upload_path};
    ///
    /// async fn sign(disk: &Disk) -> Result<PresignedUpload, StorageError> {
    ///     let rules = UploadRules::new(5 * 1024 * 1024, ["image/png", "image/jpeg"]);
    ///     disk.presigned_put(&upload_path("avatars", "image/png"), Duration::minutes(10), &rules)
    ///         .content_type("image/png")
    ///         .size(48_213)
    ///         .await
    /// }
    /// ```
    ///
    /// The declared type and size are checked against `rules` first
    /// ([`StorageError::Rejected`]). Then:
    ///
    /// - S3 signs a `PUT` bound to the declared `Content-Type` (S3 cannot
    ///   bind a size to a presigned `PUT`);
    /// - Google Cloud Storage also binds `x-goog-content-length-range`, so
    ///   it refuses a body over `rules.max_bytes`, or of another size than
    ///   the declared one;
    /// - Azure signs a service SAS; the client also sends
    ///   `x-ms-blob-type: BlockBlob` (listed in the headers);
    /// - local and memory disks sign the app's own route
    ///   `PUT /storage/{disk}/{*path}`, which checks the type, the declared
    ///   size and `rules.max_bytes` while it streams the body.
    ///
    /// The provider cannot see what a file contains, so when the client
    /// reports the upload as done, call [`Disk::accept_upload`] before using
    /// the file.
    pub fn presigned_put<'a>(
        &'a self,
        path: &str,
        ttl: Duration,
        rules: &'a UploadRules,
    ) -> PresignPut<'a> {
        PresignPut {
            disk: self,
            path: path.to_owned(),
            ttl,
            rules,
            content_type: None,
            size: None,
        }
    }

    /// Checks a file a client uploaded with a [presigned
    /// link](Disk::presigned_put): its size against `rules.max_bytes`, and
    /// its type, sniffed from the first bytes like [`store_upload`]
    /// does, against `rules.allowed_mime`. A file that breaks a rule is
    /// deleted and the error is [`StorageError::Rejected`]; a missing file is
    /// [`StorageError::NotFound`].
    ///
    /// [`store_upload`]: crate::store_upload
    pub async fn accept_upload(
        &self,
        path: &str,
        rules: &UploadRules,
    ) -> Result<StoredFile, StorageError> {
        let path = normalize(path, false)?;
        let meta = self.metadata(&path).await?;
        let reject = |rejection| async {
            if let Err(error) = self.delete(&path).await {
                tracing::warn!(
                    disk = self.name(),
                    path = %path,
                    error = %error,
                    "could not delete a rejected upload"
                );
            }
            Err(StorageError::Rejected {
                disk: self.name().to_owned(),
                path: path.clone(),
                rejection,
            })
        };
        if meta.size == 0 {
            return reject(UploadRejection::Empty).await;
        }
        if meta.size > rules.max_bytes {
            return reject(UploadRejection::TooLarge {
                max_bytes: rules.max_bytes,
            })
            .await;
        }
        let location = self.location(&path)?;
        let head_len = meta.size.min(SNIFF_BYTES as u64);
        let head = self
            .store()
            .read(&location, Some(0..head_len))
            .await
            .map_err(|error| self.error(&location, error))?;
        let name = path.rsplit('/').next();
        let sniffed = sniff(&head, name);
        if !rules.allows(&sniffed.mime) {
            return reject(UploadRejection::Type {
                mime: sniffed.mime,
                allowed: rules.allowed_mime.clone(),
            })
            .await;
        }
        Ok(StoredFile {
            disk: self.name().to_owned(),
            path,
            size: meta.size,
            mime: sniffed.mime,
            original_name: None,
        })
    }
}

/// A fresh path for an upload of type `content_type` under `dir`:
/// `<dir>/<40 random characters>.<extension>`, like the names
/// [`store_upload`](crate::store_upload) picks. The extension comes from the
/// type (`bin` when it has none).
///
/// ```
/// let path = navree_storage::upload_path("avatars", "image/png");
/// assert!(path.starts_with("avatars/") && path.ends_with(".png"));
/// assert_eq!(path.len(), "avatars/".len() + 40 + ".png".len());
/// ```
pub fn upload_path(dir: &str, content_type: &str) -> String {
    let name = format!("{}.{}", random_token(40), extension_for(content_type));
    match normalize(dir, true) {
        Ok(dir) if !dir.is_empty() => format!("{dir}/{name}"),
        _ => name,
    }
}

/// The usual extension of a content type.
fn extension_for(content_type: &str) -> String {
    let essence = essence(content_type);
    let preferred = match essence.as_str() {
        "image/jpeg" => Some("jpg"),
        "image/svg+xml" => Some("svg"),
        "text/plain" => Some("txt"),
        "text/markdown" => Some("md"),
        "application/octet-stream" => Some("bin"),
        "application/zip" => Some("zip"),
        "audio/mpeg" => Some("mp3"),
        "video/mp4" => Some("mp4"),
        "video/quicktime" => Some("mov"),
        _ => None,
    };
    if let Some(extension) = preferred {
        return extension.to_owned();
    }
    let subtype = essence.split_once('/').map(|(_, subtype)| subtype);
    mime_guess::get_mime_extensions_str(&essence)
        .and_then(|extensions| {
            // Prefer the extension that is the subtype itself (`png`, `pdf`).
            extensions
                .iter()
                .find(|extension| Some(**extension) == subtype)
                .or_else(|| extensions.first())
        })
        .map_or_else(|| "bin".to_owned(), |extension| (*extension).to_owned())
}

/// `type/subtype` in lower case, without parameters.
pub(crate) fn essence(content_type: &str) -> String {
    content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

/// A `type/subtype[; params]` made of visible ASCII.
fn is_content_type(value: &str) -> bool {
    let essence = essence(value);
    let token = |part: &str| {
        !part.is_empty()
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"!#$&-^_.+".contains(&b))
    };
    value.bytes().all(|b| (b' '..=b'~').contains(&b))
        && essence
            .split_once('/')
            .is_some_and(|(kind, subtype)| token(kind) && token(subtype))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_follow_the_content_type() {
        assert_eq!(extension_for("image/png"), "png");
        assert_eq!(extension_for("image/jpeg"), "jpg");
        assert_eq!(extension_for("IMAGE/PNG; charset=binary"), "png");
        assert_eq!(extension_for("application/pdf"), "pdf");
        assert_eq!(extension_for("text/csv"), "csv");
        assert_eq!(extension_for("text/plain"), "txt");
        assert_eq!(extension_for("application/x-unknown-thing"), "bin");
    }

    #[test]
    fn upload_paths_are_random_and_normalised() {
        let a = upload_path("/avatars//2026/", "image/png");
        let b = upload_path("avatars/2026", "image/png");
        assert!(a.starts_with("avatars/2026/"), "{a}");
        assert_ne!(a, b);
        assert!(upload_path("", "text/csv").ends_with(".csv"));
        assert!(!upload_path("../x", "text/csv").contains(".."));
    }

    #[test]
    fn content_types_are_checked() {
        assert!(is_content_type("image/png"));
        assert!(is_content_type("text/plain; charset=utf-8"));
        assert!(is_content_type("application/vnd.ms-excel"));
        assert!(!is_content_type("png"));
        assert!(!is_content_type("image/"));
        assert!(!is_content_type("image/png\r\nx-evil: 1"));
        assert!(!is_content_type("ïmage/png"));
        assert_eq!(essence(" Image/PNG ; q=1"), "image/png");
    }
}
