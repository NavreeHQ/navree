//! Zip archives: read with zip-slip and size-bomb guards, extract to a
//! disk, and build new ones.
//!
//! ```
//! use navree_media::archive::{Archive, ArchiveLimits, ZipBuilder};
//!
//! # fn main() -> Result<(), navree_media::MediaError> {
//! let mut zip = ZipBuilder::new();
//! zip.file("imsmanifest.xml", b"<manifest/>".to_vec())?;
//! zip.file("content/index.html", b"<h1>Lesson</h1>".to_vec())?;
//! let bytes = zip.finish()?;
//!
//! let mut archive = Archive::open(bytes, &ArchiveLimits::default())?;
//! assert!(archive.contains("imsmanifest.xml"));
//! assert_eq!(&archive.read("content/index.html")?[..], b"<h1>Lesson</h1>");
//! # Ok(())
//! # }
//! ```

use std::collections::HashSet;
use std::io::{Cursor, Read, Write};

use bytes::Bytes;
use navree_storage::Disk;
use serde::{Deserialize, Serialize};
use zip::ZipArchive;
use zip::write::SimpleFileOptions;

use crate::{ArchiveError, MediaError};

/// Entries smaller than this are not held to [`ArchiveLimits::max_ratio`]:
/// small files of repeated bytes compress very well and are harmless.
const RATIO_FLOOR: u64 = 1024 * 1024;

/// The limits an archive must stay within (configuration section
/// `media.archives`). A `0` turns a limit off.
///
/// They are checked against the sizes the archive declares when it is
/// opened, and again against the bytes actually inflated when an entry
/// is read, so a forged header cannot get past them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ArchiveLimits {
    /// The most entries, directories included (default 10 000).
    pub max_entries: usize,
    /// The largest unpacked entry in bytes (default 512 MiB).
    pub max_entry_bytes: u64,
    /// The most bytes all entries unpack to together (default 2 GiB).
    pub max_total_bytes: u64,
    /// The highest compression ratio of an entry over 1 MiB (default 200).
    pub max_ratio: u64,
}

impl Default for ArchiveLimits {
    fn default() -> Self {
        Self {
            max_entries: 10_000,
            max_entry_bytes: 512 * 1024 * 1024,
            max_total_bytes: 2 * 1024 * 1024 * 1024,
            max_ratio: 200,
        }
    }
}

/// One entry of an [`Archive`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ArchiveEntry {
    /// The normalized path: `/`-separated, relative, without `.` or empty
    /// segments, without a trailing `/` for directories.
    pub path: String,
    /// The declared unpacked size in bytes.
    pub size: u64,
    /// The compressed size in bytes.
    pub compressed_size: u64,
    /// Whether the entry is a directory.
    pub is_dir: bool,
}

/// What [`Archive::extract_to`] wrote.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ExtractReport {
    /// The disk paths written, in archive order.
    pub files: Vec<String>,
    /// The bytes written.
    pub bytes: u64,
}

/// A zip archive checked when it is opened: every entry has a safe path
/// (no absolute paths, drive letters, `..` segments or control
/// characters), none is a symbolic link, encrypted or a duplicate, and
/// the declared sizes stay within the [`ArchiveLimits`].
pub struct Archive {
    zip: ZipArchive<Cursor<Bytes>>,
    entries: Vec<ArchiveEntry>,
    /// The zip index of each entry.
    indexes: Vec<usize>,
    limits: ArchiveLimits,
}

impl std::fmt::Debug for Archive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Archive")
            .field("entries", &self.entries)
            .finish_non_exhaustive()
    }
}

impl Archive {
    /// Opens and checks a zip archive.
    pub fn open(bytes: impl Into<Bytes>, limits: &ArchiveLimits) -> Result<Archive, ArchiveError> {
        let mut zip = ZipArchive::new(Cursor::new(bytes.into())).map_err(invalid)?;
        if limits.max_entries > 0 && zip.len() > limits.max_entries {
            return Err(ArchiveError::TooManyEntries {
                max: limits.max_entries,
            });
        }
        let mut entries = Vec::with_capacity(zip.len());
        let mut indexes = Vec::with_capacity(zip.len());
        let mut seen = HashSet::new();
        let mut total = 0u64;
        for index in 0..zip.len() {
            let file = zip.by_index_raw(index).map_err(invalid)?;
            let raw = file.name().map_err(invalid)?.into_owned();
            let path = safe_path(&raw).ok_or(ArchiveError::UnsafePath { path: raw.clone() })?;
            if file.is_symlink() {
                return Err(ArchiveError::Symlink { path });
            }
            if file.encrypted() {
                return Err(ArchiveError::Encrypted { path });
            }
            if !seen.insert(path.clone()) {
                return Err(ArchiveError::Duplicate { path });
            }
            let size = file.size();
            let compressed_size = file.compressed_size();
            if limits.max_entry_bytes > 0 && size > limits.max_entry_bytes {
                return Err(ArchiveError::EntryTooLarge {
                    path,
                    max: limits.max_entry_bytes,
                });
            }
            if limits.max_ratio > 0
                && size > RATIO_FLOOR
                && size / compressed_size.max(1) > limits.max_ratio
            {
                return Err(ArchiveError::Ratio {
                    path,
                    max: limits.max_ratio,
                });
            }
            total = total.saturating_add(size);
            if limits.max_total_bytes > 0 && total > limits.max_total_bytes {
                return Err(ArchiveError::TooLarge {
                    max: limits.max_total_bytes,
                });
            }
            entries.push(ArchiveEntry {
                path,
                size,
                compressed_size,
                is_dir: file.is_dir(),
            });
            indexes.push(index);
        }
        Ok(Archive {
            zip,
            entries,
            indexes,
            limits: limits.clone(),
        })
    }

    /// Every entry, in archive order.
    pub fn entries(&self) -> &[ArchiveEntry] {
        &self.entries
    }

    /// The entries that are files.
    pub fn files(&self) -> impl Iterator<Item = &ArchiveEntry> {
        self.entries.iter().filter(|entry| !entry.is_dir)
    }

    /// Whether a file or directory has this (normalized) path.
    pub fn contains(&self, path: &str) -> bool {
        self.position(path).is_some()
    }

    fn position(&self, path: &str) -> Option<usize> {
        let path = safe_path(path)?;
        self.entries.iter().position(|entry| entry.path == path)
    }

    /// The unpacked contents of the file at `path`, read no further than
    /// its declared size and the entry limit.
    pub fn read(&mut self, path: &str) -> Result<Bytes, ArchiveError> {
        let position = self
            .position(path)
            .filter(|&position| !self.entries[position].is_dir)
            .ok_or_else(|| ArchiveError::NotFound {
                path: path.to_owned(),
            })?;
        self.read_at(position)
    }

    fn read_at(&mut self, position: usize) -> Result<Bytes, ArchiveError> {
        let entry = &self.entries[position];
        let path = entry.path.clone();
        let declared = entry.size;
        let cap = if self.limits.max_entry_bytes > 0 {
            declared.min(self.limits.max_entry_bytes)
        } else {
            declared
        };
        let file = self.zip.by_index(self.indexes[position]).map_err(invalid)?;
        let mut out = Vec::with_capacity(usize::try_from(cap.min(64 * 1024 * 1024)).unwrap_or(0));
        // One byte over the cap tells a lying header from an honest one.
        file.take(cap + 1)
            .read_to_end(&mut out)
            .map_err(|error| ArchiveError::Invalid {
                message: format!("{path}: {error}"),
            })?;
        if out.len() as u64 > cap {
            return Err(ArchiveError::EntryTooLarge { path, max: cap });
        }
        Ok(Bytes::from(out))
    }

    /// Writes every file under `prefix` on `disk` (directories are
    /// implied by the paths) and returns what was written. The total is
    /// checked again against `max_total_bytes` while unpacking.
    pub async fn extract_to(
        mut self,
        disk: &Disk,
        prefix: &str,
    ) -> Result<ExtractReport, MediaError> {
        let prefix = prefix.trim_matches('/');
        let mut report = ExtractReport::default();
        for position in 0..self.entries.len() {
            if self.entries[position].is_dir {
                continue;
            }
            let contents = self.read_at(position)?;
            report.bytes += contents.len() as u64;
            if self.limits.max_total_bytes > 0 && report.bytes > self.limits.max_total_bytes {
                return Err(ArchiveError::TooLarge {
                    max: self.limits.max_total_bytes,
                }
                .into());
            }
            let path = if prefix.is_empty() {
                self.entries[position].path.clone()
            } else {
                format!("{prefix}/{}", self.entries[position].path)
            };
            disk.put(&path, contents).await?;
            report.files.push(path);
        }
        Ok(report)
    }
}

/// Builds a zip archive in memory (deflated entries).
pub struct ZipBuilder {
    writer: zip::ZipWriter<Cursor<Vec<u8>>>,
    seen: HashSet<String>,
}

impl Default for ZipBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for ZipBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ZipBuilder")
            .field("entries", &self.seen.len())
            .finish_non_exhaustive()
    }
}

impl ZipBuilder {
    /// An empty archive.
    pub fn new() -> Self {
        Self {
            writer: zip::ZipWriter::new(Cursor::new(Vec::new())),
            seen: HashSet::new(),
        }
    }

    fn options() -> SimpleFileOptions {
        SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .large_file(false)
    }

    /// Adds a file at `path` (checked like the entries [`Archive::open`]
    /// reads).
    pub fn file(
        &mut self,
        path: &str,
        contents: impl AsRef<[u8]>,
    ) -> Result<&mut Self, MediaError> {
        let path = self.claim(path)?;
        let contents = contents.as_ref();
        let options = Self::options().large_file(contents.len() as u64 >= u64::from(u32::MAX));
        self.writer.start_file(path, options).map_err(invalid)?;
        self.writer
            .write_all(contents)
            .map_err(|error| ArchiveError::Invalid {
                message: error.to_string(),
            })?;
        Ok(self)
    }

    /// Adds an empty directory at `path`.
    pub fn directory(&mut self, path: &str) -> Result<&mut Self, MediaError> {
        let path = self.claim(path)?;
        self.writer
            .add_directory(path, Self::options())
            .map_err(invalid)?;
        Ok(self)
    }

    fn claim(&mut self, path: &str) -> Result<String, ArchiveError> {
        let safe = safe_path(path).ok_or_else(|| ArchiveError::UnsafePath {
            path: path.to_owned(),
        })?;
        if !self.seen.insert(safe.clone()) {
            return Err(ArchiveError::Duplicate { path: safe });
        }
        Ok(safe)
    }

    /// The finished archive.
    pub fn finish(self) -> Result<Bytes, MediaError> {
        let cursor = self.writer.finish().map_err(invalid)?;
        Ok(Bytes::from(cursor.into_inner()))
    }
}

fn invalid(error: zip::result::ZipError) -> ArchiveError {
    ArchiveError::Invalid {
        message: error.to_string(),
    }
}

/// `name` as a safe relative path, or `None`: `\` is a separator, `.` and
/// empty segments are dropped; absolute paths, drive letters, `..`,
/// segments made only of dots and control characters are refused.
pub(crate) fn safe_path(name: &str) -> Option<String> {
    if name.chars().any(char::is_control) {
        return None;
    }
    let name = name.replace('\\', "/");
    if name.starts_with('/') {
        return None;
    }
    let bytes = name.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return None;
    }
    let mut segments = Vec::new();
    for segment in name.split('/') {
        match segment {
            "" | "." => continue,
            _ if segment.chars().all(|c| c == '.') => return None,
            _ => segments.push(segment),
        }
    }
    if segments.is_empty() {
        return None;
    }
    Some(segments.join("/"))
}

#[cfg(test)]
mod tests {
    use super::safe_path;

    #[test]
    fn safe_paths() {
        assert_eq!(safe_path("a/b.txt").as_deref(), Some("a/b.txt"));
        assert_eq!(safe_path("./a//b/").as_deref(), Some("a/b"));
        assert_eq!(safe_path("a\\b.txt").as_deref(), Some("a/b.txt"));
        assert_eq!(safe_path("..hidden").as_deref(), Some("..hidden"));
        for bad in [
            "", "/", "../a", "a/..", "/abs", "C:/x", "c:x", "a/\0b", "...",
        ] {
            assert_eq!(safe_path(bad), None, "{bad:?}");
        }
    }
}
