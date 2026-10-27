//! [`MediaSource`]: where a new file comes from, and how it is read.

use std::path::PathBuf;
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use navree_http_client::{HttpClient, HttpClientError};
use navree_storage::Storage;

use crate::MediaError;

/// Where the file of [`Media::add`](crate::Media::add) comes from.
///
/// ```
/// use navree_media::MediaSource;
///
/// let uploaded = MediaSource::bytes("cover.png", vec![0u8; 4]);
/// let remote = MediaSource::url("https://cdn.example.com/logo.svg");
/// let presigned = MediaSource::disk("uploads", "tmp/3f2a/cover.png");
/// let local = MediaSource::file("/var/imports/handbook.pdf");
/// # let _ = (uploaded, remote, presigned, local);
/// ```
#[derive(Clone, Debug)]
pub struct MediaSource {
    kind: Kind,
}

#[derive(Clone, Debug)]
enum Kind {
    Bytes { file_name: String, bytes: Bytes },
    Url(String),
    Disk { disk: String, path: String },
    File(PathBuf),
}

impl MediaSource {
    /// File contents in memory (a multipart upload's bytes), with the name
    /// it was uploaded under.
    pub fn bytes(file_name: impl Into<String>, bytes: impl Into<Bytes>) -> Self {
        Self {
            kind: Kind::Bytes {
                file_name: file_name.into(),
                bytes: bytes.into(),
            },
        }
    }

    /// A remote `http` or `https` file, downloaded through the app's
    /// `HttpClient` and its SSRF guard: private, loopback, link-local and
    /// cloud metadata addresses are refused, on every redirect too. The
    /// download is capped at the collection's size limit and
    /// `media.download_timeout_secs`.
    pub fn url(url: impl Into<String>) -> Self {
        Self {
            kind: Kind::Url(url.into()),
        }
    }

    /// A file already on a storage disk (a presigned upload): copied, not
    /// moved.
    pub fn disk(disk: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            kind: Kind::Disk {
                disk: disk.into(),
                path: path.into(),
            },
        }
    }

    /// A file on the local filesystem (imports, seeders).
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self {
            kind: Kind::File(path.into()),
        }
    }

    /// The URL, for a remote source.
    pub(crate) fn remote_url(&self) -> Option<&str> {
        match &self.kind {
            Kind::Url(url) => Some(url),
            _ => None,
        }
    }
}

/// A source's contents and name.
pub(crate) struct Loaded {
    pub(crate) file_name: String,
    pub(crate) bytes: Bytes,
}

/// What reading a source needs.
pub(crate) struct Loader<'a> {
    pub(crate) storage: &'a Storage,
    pub(crate) http: &'a HttpClient,
    pub(crate) max: u64,
    pub(crate) timeout: Duration,
}

impl Loader<'_> {
    pub(crate) async fn load(&self, source: MediaSource) -> Result<Loaded, MediaError> {
        let loaded = match source.kind {
            Kind::Bytes { file_name, bytes } => Loaded { file_name, bytes },
            Kind::Url(url) => {
                match tokio::time::timeout(self.timeout, self.download(&url)).await {
                    Ok(result) => result?,
                    Err(_) => {
                        return Err(MediaError::Download {
                            url: display_url(&url),
                            reason: format!(
                                "the download took longer than {} s",
                                self.timeout.as_secs()
                            ),
                        });
                    }
                }
            }
            Kind::Disk { disk, path } => {
                let disk = self.storage.disk(&disk)?;
                if disk.size(&path).await? > self.max {
                    return Err(MediaError::TooLarge { max: self.max });
                }
                Loaded {
                    file_name: base_name(&path),
                    bytes: disk.get(&path).await?,
                }
            }
            Kind::File(path) => {
                let read = |error: std::io::Error| MediaError::File {
                    path: path.display().to_string(),
                    message: error.to_string(),
                };
                let size = tokio::fs::metadata(&path).await.map_err(read)?.len();
                if size > self.max {
                    return Err(MediaError::TooLarge { max: self.max });
                }
                Loaded {
                    file_name: path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    bytes: tokio::fs::read(&path).await.map_err(read)?.into(),
                }
            }
        };
        if loaded.bytes.is_empty() {
            return Err(MediaError::Empty);
        }
        if loaded.bytes.len() as u64 > self.max {
            return Err(MediaError::TooLarge { max: self.max });
        }
        Ok(loaded)
    }

    async fn download(&self, url: &str) -> Result<Loaded, MediaError> {
        let shown = display_url(url);
        let parsed = url::Url::parse(url).map_err(|_| MediaError::Download {
            url: shown.clone(),
            reason: "not a valid URL".to_owned(),
        })?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(MediaError::Blocked {
                url: shown,
                reason: "only http and https URLs can be downloaded".to_owned(),
            });
        }
        let mut response = self
            .http
            .get(url)
            .timeout(self.timeout)
            .send_streaming()
            .await
            .map_err(|error| http_error(error, &shown))?;
        if !response.is_success() {
            return Err(MediaError::Download {
                url: shown,
                reason: format!("the server answered {}", response.status()),
            });
        }
        if let Some(length) = response
            .header("content-length")
            .and_then(|length| length.parse::<u64>().ok())
            && length > self.max
        {
            return Err(MediaError::TooLarge { max: self.max });
        }
        let file_name = response
            .header("content-disposition")
            .and_then(disposition_name)
            .unwrap_or_else(|| url_name(response.url()));
        let mut body = BytesMut::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| http_error(error, &shown))?
        {
            if (body.len() + chunk.len()) as u64 > self.max {
                return Err(MediaError::TooLarge { max: self.max });
            }
            body.extend_from_slice(&chunk);
        }
        Ok(Loaded {
            file_name,
            bytes: body.freeze(),
        })
    }
}

fn http_error(error: HttpClientError, url: &str) -> MediaError {
    match error {
        HttpClientError::Blocked { reason, .. } => MediaError::Blocked {
            url: url.to_owned(),
            reason,
        },
        other => MediaError::Download {
            url: url.to_owned(),
            reason: scrub_urls(&other.to_string()),
        },
    }
}

/// `text` with every `http(s)://` URL in it shown without its query
/// string, fragment or credentials (client errors name the URL they
/// failed on, signed query strings included).
pub(crate) fn scrub_urls(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("http://").into_iter().chain(rest.find("https://")).min() {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let end = tail
            .find(|c: char| c.is_whitespace() || matches!(c, ')' | '(' | '`' | '"' | '\'' | '<' | '>'))
            .unwrap_or(tail.len());
        out.push_str(&display_url(&tail[..end]));
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

/// The URL without its query string, fragment or credentials.
pub(crate) fn display_url(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(mut parsed) => {
            parsed.set_query(None);
            parsed.set_fragment(None);
            let _ = parsed.set_username("");
            let _ = parsed.set_password(None);
            parsed.to_string()
        }
        Err(_) => url.split(['?', '#']).next().unwrap_or_default().to_owned(),
    }
}

/// The last segment of a path.
fn base_name(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or_default().to_owned()
}

/// The file name a URL's path ends with, percent-decoded.
fn url_name(url: &url::Url) -> String {
    let segment = url
        .path_segments()
        .and_then(|mut segments| segments.next_back().map(str::to_owned))
        .unwrap_or_default();
    percent_encoding::percent_decode_str(&segment)
        .decode_utf8_lossy()
        .into_owned()
}

/// `filename="..."` (or `filename*=UTF-8''...`) of a Content-Disposition.
fn disposition_name(header: &str) -> Option<String> {
    let mut plain = None;
    for part in header.split(';').map(str::trim) {
        if let Some(value) = part.strip_prefix("filename*=") {
            let value = value.split_once("''").map_or(value, |(_, v)| v);
            let decoded = percent_encoding::percent_decode_str(value.trim_matches('"'))
                .decode_utf8_lossy()
                .into_owned();
            if !decoded.is_empty() {
                return Some(decoded);
            }
        } else if let Some(value) = part.strip_prefix("filename=") {
            let value = value.trim_matches('"').to_owned();
            if !value.is_empty() {
                plain = Some(value);
            }
        }
    }
    plain
}

/// A file name safe on every disk and in a `Content-Disposition`: the
/// last path segment, control characters and `/\:*?"<>|` replaced by
/// `-`, leading dots removed, at most 200 bytes (keeping the extension).
/// `file.<extension>` when nothing is left.
pub(crate) fn sanitize_file_name(name: &str, extension: &str) -> String {
    let name = base_name(name);
    let mut clean: String = name
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '-'
            } else {
                c
            }
        })
        .collect();
    clean = clean.trim().trim_start_matches('.').trim().to_owned();
    if clean.is_empty() || clean.chars().all(|c| c == '-' || c == '.') {
        return format!("file.{extension}");
    }
    if clean.len() > 200 {
        let (stem, ext) = match clean.rsplit_once('.') {
            Some((stem, ext)) if ext.len() <= 16 => (stem.to_owned(), format!(".{ext}")),
            _ => (clean.clone(), String::new()),
        };
        let mut end = 200 - ext.len();
        while !stem.is_char_boundary(end) {
            end -= 1;
        }
        clean = format!("{}{ext}", &stem[..end]);
    }
    clean
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_are_sanitized() {
        assert_eq!(sanitize_file_name("cover.png", "png"), "cover.png");
        assert_eq!(sanitize_file_name("../../etc/passwd", "bin"), "passwd");
        assert_eq!(sanitize_file_name("C:\\Users\\me\\a:b?.txt", "txt"), "a-b-.txt");
        assert_eq!(sanitize_file_name(".htaccess", "txt"), "htaccess");
        assert_eq!(sanitize_file_name("", "pdf"), "file.pdf");
        assert_eq!(sanitize_file_name("...", "pdf"), "file.pdf");
        let long = format!("{}.pdf", "a".repeat(300));
        let clean = sanitize_file_name(&long, "pdf");
        assert_eq!(clean.len(), 200);
        assert!(clean.ends_with(".pdf"));
    }

    #[test]
    fn names_from_headers_and_urls() {
        assert_eq!(
            disposition_name("attachment; filename=\"report.pdf\"").as_deref(),
            Some("report.pdf")
        );
        assert_eq!(
            disposition_name("attachment; filename=x.pdf; filename*=UTF-8''r%C3%A9sum%C3%A9.pdf")
                .as_deref(),
            Some("résumé.pdf")
        );
        let url = url::Url::parse("https://cdn.example.com/a/brand%20logo.svg?v=2").unwrap();
        assert_eq!(url_name(&url), "brand logo.svg");
        assert_eq!(
            display_url("https://user:pw@cdn.example.com/a.png?token=secret#x"),
            "https://cdn.example.com/a.png"
        );
        assert_eq!(
            scrub_urls("request to https://cdn.example.com/a.png?sig=abc timed out (http://x.test/?k=1)"),
            "request to https://cdn.example.com/a.png timed out (http://x.test/)"
        );
        assert_eq!(scrub_urls("no url here"), "no url here");
    }
}
