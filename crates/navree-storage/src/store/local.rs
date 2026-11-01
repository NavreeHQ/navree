//! [`Local`]: files in a directory of the local filesystem.
//!
//! - **Atomic writes.** A file is written to `<name>#<n>` next to its
//!   destination and renamed over it, so readers see the old file or the new
//!   one. The staging file is removed if the write fails, the upload is
//!   aborted or its future is dropped. Names that end in `#` and digits are
//!   those staging names: listings skip them and no operation accepts them.
//! - **Paths stay under the root.** A path is joined to the root one segment
//!   at a time, and a segment that could mean anything but a name (`.`, `..`,
//!   a separator, a control character) is refused again here, whatever the
//!   caller did. On Windows a `:` in a name is written as `%3A`, so a drive
//!   letter or an alternate data stream cannot be named.
//! - **Symlinks are followed**, by reads, writes and listings alike. A link
//!   to nowhere is a missing file. A link back to a directory above it fails
//!   the listing.
//! - **Deleting a file removes the directories it leaves empty**, up to but
//!   not including the root.
//!
//! Blocking filesystem calls run on Tokio's blocking pool.

use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use bytes::Bytes;
use chrono::{DateTime, Utc};
use futures_util::stream::{self, BoxStream};
use navree_kernel::BoxFuture;

use super::{Meta, Object, Store, StoreError, StoreResult, Upload};

/// The largest chunk a stream reads at a time.
const CHUNK_BYTES: usize = 64 * 1024;

/// The most memory reserved up front to read a whole file.
const READ_RESERVE_BYTES: u64 = 64 * 1024 * 1024;

/// Files under a directory.
pub(crate) struct Local {
    /// The root, absolute with symlinks resolved.
    root: PathBuf,
}

impl Local {
    /// A store at `root`, which must exist.
    pub(crate) fn new(root: &Path) -> io::Result<Self> {
        Ok(Self {
            root: simplified(fs::canonicalize(root)?),
        })
    }

    /// The filesystem path of the directory or file `path`, one segment at
    /// a time under the root.
    fn locate(&self, path: &str) -> StoreResult<PathBuf> {
        let mut located = self.root.clone();
        for segment in path.split('/').filter(|segment| !segment.is_empty()) {
            if segment == "."
                || segment == ".."
                || segment.contains('\\')
                || segment.chars().any(char::is_control)
            {
                return Err(StoreError::failed(format!("invalid path `{path}`")));
            }
            if cfg!(windows) {
                located.push(segment.replace(':', "%3A"));
            } else {
                located.push(segment);
            }
        }
        Ok(located)
    }

    /// The filesystem path of the file `path`.
    fn locate_file(&self, path: &str) -> StoreResult<PathBuf> {
        let name = path.rsplit('/').next().unwrap_or(path);
        if name.is_empty() || is_staging_name(name) {
            return Err(StoreError::failed(format!(
                "filenames ending in `#` and digits are not supported: {path}"
            )));
        }
        self.locate(path)
    }
}

/// Removes the empty directories above `file`, up to but not including `root`.
fn remove_empty_parents(root: &Path, file: &Path) {
    let mut parent = file.parent();
    while let Some(directory) = parent {
        if directory != root && fs::remove_dir(directory).is_ok() {
            parent = directory.parent();
        } else {
            break;
        }
    }
}

impl fmt::Display for Local {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Local({})", self.root.display())
    }
}

impl Store for Local {
    fn put<'a>(&'a self, path: &'a str, contents: Bytes) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move {
            let destination = self.locate_file(path)?;
            let path = path.to_owned();
            blocking(move || {
                let (mut file, staging) = create_staged(&destination, &path)?;
                file.write_all(&contents)
                    .map_err(|error| fail("write", &path, error))?;
                drop(file);
                publish(staging, &destination, &path)
            })
            .await
        })
    }

    fn begin_upload<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<Box<dyn Upload>>> {
        Box::pin(async move {
            let destination = self.locate_file(path)?;
            let path = path.to_owned();
            blocking(move || {
                let (file, staging) = create_staged(&destination, &path)?;
                Ok(Box::new(LocalUpload {
                    file: Some(file),
                    staging,
                    destination,
                    path,
                }) as Box<dyn Upload>)
            })
            .await
        })
    }

    fn read<'a>(
        &'a self,
        path: &'a str,
        range: Option<Range<u64>>,
    ) -> BoxFuture<'a, StoreResult<Bytes>> {
        Box::pin(async move {
            let located = self.locate_file(path)?;
            let path = path.to_owned();
            blocking(move || {
                let (mut file, size) = open_file(&located, &path)?;
                let (start, end) = match range {
                    Some(range) => {
                        let end = range.end.min(size);
                        (range.start.min(end), end)
                    }
                    None => (0, size),
                };
                if start > 0 {
                    file.seek(SeekFrom::Start(start))
                        .map_err(|error| fail("read", &path, error))?;
                }
                let wanted = end - start;
                let mut contents = Vec::with_capacity(
                    usize::try_from(wanted.min(READ_RESERVE_BYTES)).unwrap_or_default(),
                );
                file.take(wanted)
                    .read_to_end(&mut contents)
                    .map_err(|error| fail("read", &path, error))?;
                Ok(Bytes::from(contents))
            })
            .await
        })
    }

    fn open<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<Object>> {
        Box::pin(async move {
            let located = self.locate_file(path)?;
            let path = path.to_owned();
            blocking(move || {
                let (file, size) = open_file(&located, &path)?;
                Ok(Object {
                    size,
                    stream: file_stream(file, size, path),
                })
            })
            .await
        })
    }

    fn head<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<Meta>> {
        Box::pin(async move {
            let located = self.locate_file(path)?;
            let path = path.to_owned();
            blocking(move || {
                let metadata =
                    fs::metadata(&located).map_err(|error| lookup_fail("read", &path, error))?;
                if metadata.is_dir() {
                    return Err(StoreError::NotFound);
                }
                Ok(meta(path, &metadata))
            })
            .await
        })
    }

    fn delete<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move {
            let located = self.locate_file(path)?;
            let root = self.root.clone();
            let path = path.to_owned();
            blocking(move || {
                fs::remove_file(&located).map_err(|error| lookup_fail("delete", &path, error))?;
                remove_empty_parents(&root, &located);
                Ok(())
            })
            .await
        })
    }

    fn list<'a>(&'a self, prefix: &'a str) -> BoxFuture<'a, StoreResult<Vec<Meta>>> {
        Box::pin(async move {
            let start = self.locate(prefix)?;
            let prefix = prefix.to_owned();
            blocking(move || walk(&start, &prefix)).await
        })
    }
}

/// A file being written to its staging name.
struct LocalUpload {
    /// Taken while a write is in progress; lost if that write is dropped.
    file: Option<File>,
    staging: Staging,
    destination: PathBuf,
    path: String,
}

impl Upload for LocalUpload {
    fn write<'a>(&'a mut self, chunk: Bytes) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move {
            let Some(mut file) = self.file.take() else {
                return Err(StoreError::failed(format!(
                    "the upload of `{}` is no longer usable",
                    self.path
                )));
            };
            let path = self.path.clone();
            let (file, written) = blocking(move || {
                let written = file
                    .write_all(&chunk)
                    .map_err(|error| fail("write", &path, error));
                Ok((file, written))
            })
            .await?;
            self.file = Some(file);
            written
        })
    }

    fn finish(self: Box<Self>) -> BoxFuture<'static, StoreResult<()>> {
        Box::pin(async move {
            let LocalUpload {
                file,
                staging,
                destination,
                path,
            } = *self;
            blocking(move || {
                if file.is_none() {
                    return Err(StoreError::failed(format!(
                        "the upload of `{path}` is no longer usable"
                    )));
                }
                drop(file);
                publish(staging, &destination, &path)
            })
            .await
        })
    }

    fn abort(self: Box<Self>) -> BoxFuture<'static, StoreResult<()>> {
        Box::pin(async move {
            // Dropping the upload closes the file and removes the staging file.
            drop(self);
            Ok(())
        })
    }
}

/// A staging file, removed when this is dropped unless it was published.
struct Staging {
    path: PathBuf,
    published: bool,
}

impl Drop for Staging {
    fn drop(&mut self) {
        if !self.published {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Opens a new staging file next to `destination`, creating the directories
/// above it as needed.
fn create_staged(destination: &Path, path: &str) -> StoreResult<(File, Staging)> {
    let mut number = 1u32;
    loop {
        let mut name = destination.as_os_str().to_owned();
        name.push(format!("#{number}"));
        let staging = PathBuf::from(name);
        match File::options().write(true).create_new(true).open(&staging) {
            Ok(file) => {
                let staging = Staging {
                    path: staging,
                    published: false,
                };
                return Ok((file, staging));
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                number = number
                    .checked_add(1)
                    .ok_or_else(|| StoreError::failed("too many uploads of the same file"))?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let parent = destination
                    .parent()
                    .ok_or_else(|| StoreError::failed(format!("invalid path `{path}`")))?;
                fs::create_dir_all(parent)
                    .map_err(|error| fail("create the directory of", path, error))?;
            }
            Err(error) => return Err(fail("create", path, error)),
        }
    }
}

/// Renames the written staging file over `destination`.
fn publish(mut staging: Staging, destination: &Path, path: &str) -> StoreResult<()> {
    fs::rename(&staging.path, destination).map_err(|error| fail("replace", path, error))?;
    staging.published = true;
    Ok(())
}

/// Whether `name` is a staging name: what follows its first `#` is digits.
fn is_staging_name(name: &str) -> bool {
    match name.split_once('#') {
        Some((_, suffix)) => !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit()),
        None => false,
    }
}

/// Opens the regular file at `located` and gives its size. A directory, a
/// link to nowhere and a path through a file are missing files.
fn open_file(located: &Path, path: &str) -> StoreResult<(File, u64)> {
    let metadata = fs::metadata(located).map_err(|error| lookup_fail("open", path, error))?;
    if metadata.is_dir() {
        return Err(StoreError::NotFound);
    }
    let file = File::open(located).map_err(|error| lookup_fail("open", path, error))?;
    let size = file
        .metadata()
        .map_err(|error| lookup_fail("open", path, error))?
        .len();
    Ok((file, size))
}

/// The contents of `file` as chunks of at most [`CHUNK_BYTES`], read on the
/// blocking pool.
fn file_stream(file: File, size: u64, path: String) -> BoxStream<'static, StoreResult<Bytes>> {
    Box::pin(stream::try_unfold(
        (file, size),
        move |(file, remaining)| {
            let path = path.clone();
            async move {
                if remaining == 0 {
                    return Ok(None);
                }
                let wanted = usize::try_from(remaining).map_or(CHUNK_BYTES, |n| n.min(CHUNK_BYTES));
                blocking(move || {
                    let mut file = file;
                    let mut chunk = vec![0u8; wanted];
                    let mut filled = 0;
                    while filled < wanted {
                        match file.read(&mut chunk[filled..]) {
                            Ok(0) => break,
                            Ok(read) => filled += read,
                            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                            Err(error) => return Err(fail("read", &path, error)),
                        }
                    }
                    if filled == 0 {
                        return Err(StoreError::failed(format!(
                            "`{path}` is shorter than it was when it was opened"
                        )));
                    }
                    chunk.truncate(filled);
                    Ok(Some((
                        Bytes::from(chunk),
                        (file, remaining - filled as u64),
                    )))
                })
                .await
            }
        },
    ))
}

/// Every file under the directory `start` (`prefix` in the store), skipping
/// staging files and links to nowhere.
fn walk(start: &Path, prefix: &str) -> StoreResult<Vec<Meta>> {
    let mut files = Vec::new();
    match fs::metadata(start) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => return Ok(files),
        Err(error) if is_missing(&error) => return Ok(files),
        Err(error) => return Err(fail("list", prefix, error)),
    }
    let entries = fs::read_dir(start).map_err(|error| fail("list", prefix, error))?;
    let canonical = fs::canonicalize(start).map_err(|error| fail("list", prefix, error))?;
    let mut open = vec![Level {
        entries,
        canonical,
        path: prefix.to_owned(),
    }];
    while let Some(level) = open.last_mut() {
        let Some(entry) = level.entries.next() else {
            open.pop();
            continue;
        };
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) if is_missing(&error) => continue,
            Err(error) => return Err(fail("list", &level.path, error)),
        };
        let Ok(name) = entry.file_name().into_string() else {
            return Err(StoreError::failed(format!(
                "`{}` has a file whose name is not Unicode",
                level.path
            )));
        };
        if name.chars().any(|c| c.is_ascii_control()) {
            return Err(StoreError::failed(format!(
                "`{}` has a file with a control character in its name",
                level.path
            )));
        }
        let path = if level.path.is_empty() {
            name.clone()
        } else {
            format!("{}/{name}", level.path)
        };
        let located = entry.path();
        let is_link = entry
            .file_type()
            .map_err(|error| fail("list", &path, error))?
            .is_symlink();
        let metadata = if is_link {
            // A link to nowhere is skipped, whatever the reason.
            match fs::metadata(&located) {
                Ok(metadata) => metadata,
                Err(_) => continue,
            }
        } else {
            match entry.metadata() {
                Ok(metadata) => metadata,
                Err(error) if is_missing(&error) => continue,
                Err(error) => return Err(fail("list", &path, error)),
            }
        };
        if metadata.is_dir() {
            let canonical = if is_link {
                match fs::canonicalize(&located) {
                    Ok(canonical) => canonical,
                    Err(_) => continue,
                }
            } else {
                level.canonical.join(&name)
            };
            if open.iter().any(|level| level.canonical == canonical) {
                return Err(StoreError::failed(format!(
                    "`{path}` is a link to a directory that contains it"
                )));
            }
            match fs::read_dir(&located) {
                Ok(entries) => open.push(Level {
                    entries,
                    canonical,
                    path,
                }),
                Err(error) if is_missing(&error) => {}
                Err(error) => return Err(fail("list", &path, error)),
            }
        } else if !is_staging_name(&name) {
            files.push(meta(path, &metadata));
        }
    }
    Ok(files)
}

/// A directory being listed.
struct Level {
    entries: fs::ReadDir,
    /// Its real path, to recognise a link back to it.
    canonical: PathBuf,
    /// Its path in the store.
    path: String,
}

/// Whether `error` says there is nothing at the path (or nothing a path can
/// run through).
fn is_missing(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
    )
}

/// The [`Meta`] of the file `path` with `metadata`.
fn meta(path: String, metadata: &fs::Metadata) -> Meta {
    Meta {
        path,
        size: metadata.len(),
        last_modified: metadata
            .modified()
            .map(DateTime::<Utc>::from)
            .unwrap_or_else(|_| DateTime::<Utc>::from(SystemTime::UNIX_EPOCH)),
    }
}

/// A failure that says what was being done to `path`.
fn fail(action: &str, path: &str, error: io::Error) -> StoreError {
    StoreError::failed(format!("unable to {action} `{path}`: {error}"))
}

/// Like [`fail`], for looking up a file that may not be there: a missing
/// file is [`StoreError::NotFound`].
fn lookup_fail(action: &str, path: &str, error: io::Error) -> StoreError {
    if is_missing(&error) {
        StoreError::NotFound
    } else {
        fail(action, path, error)
    }
}

/// Runs `work` on Tokio's blocking pool, or right here without a runtime.
async fn blocking<T, F>(work: F) -> StoreResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> StoreResult<T> + Send + 'static,
{
    if tokio::runtime::Handle::try_current().is_err() {
        return work();
    }
    match tokio::task::spawn_blocking(work).await {
        Ok(result) => result,
        Err(error) => Err(StoreError::failed(format!(
            "a file operation did not finish: {error}"
        ))),
    }
}

/// `path` without the verbatim prefix `canonicalize` gives on Windows,
/// which names the same place but switches off the checks Windows makes of
/// file names.
#[cfg(windows)]
fn simplified(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    match text.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => path,
    }
}

#[cfg(not(windows))]
fn simplified(path: PathBuf) -> PathBuf {
    path
}

#[cfg(test)]
mod tests {
    use std::path::Component;

    use futures_util::TryStreamExt;

    use super::*;

    fn store() -> (tempfile::TempDir, Local) {
        let directory = tempfile::tempdir().unwrap();
        let store = Local::new(directory.path()).unwrap();
        (directory, store)
    }

    #[test]
    fn segments_that_are_not_names_are_refused() {
        let (_directory, store) = store();
        for path in [
            "..", "a/../b", "a/./b", r"a\b", r"..\x", "a\0b", "a/\nb", "a/\u{7f}",
        ] {
            assert!(store.locate(path).is_err(), "{path:?}");
        }
    }

    #[test]
    fn every_path_that_is_accepted_is_under_the_root() {
        let (_directory, store) = store();
        for path in [
            "a/b",
            "a//b",
            "/a/b/",
            "",
            "C:/x",
            "C:",
            "%2e%2e/x",
            "..%2fx",
            "%5c..%5cx",
            "~/x",
            "//server/share/x",
            "a/...",
        ] {
            let located = store.locate(path).unwrap();
            assert!(located.starts_with(&store.root), "{path:?}: {located:?}");
            assert!(
                !located
                    .components()
                    .any(|part| matches!(part, Component::ParentDir | Component::CurDir)),
                "{path:?}: {located:?}"
            );
        }
        assert_eq!(store.locate("").unwrap(), store.root);
    }

    #[cfg(windows)]
    #[test]
    fn a_colon_is_written_as_percent_3a() {
        let (_directory, store) = store();
        assert_eq!(
            store.locate("C:/Windows/x:stream").unwrap(),
            store.root.join("C%3A").join("Windows").join("x%3Astream")
        );
    }

    #[cfg(windows)]
    #[test]
    fn the_verbatim_prefix_is_dropped() {
        assert_eq!(
            simplified(PathBuf::from(r"\\?\C:\Users\x")),
            PathBuf::from(r"C:\Users\x")
        );
        assert_eq!(
            simplified(PathBuf::from(r"\\?\UNC\server\share\x")),
            PathBuf::from(r"\\server\share\x")
        );
        assert_eq!(
            simplified(PathBuf::from(r"\\?\Volume{1234}\x")),
            PathBuf::from(r"\\?\Volume{1234}\x")
        );
    }

    #[test]
    fn staging_names_are_a_hash_and_digits() {
        for name in ["file#1", "file#123", "#1", "a#7", "\u{1f600}#34"] {
            assert!(is_staging_name(name), "{name}");
        }
        for name in [
            "file",
            "file#",
            "file#1a",
            "file#a1",
            "file#1.txt",
            "a#b#12",
            "file#\u{1f600}34",
            "#",
        ] {
            assert!(!is_staging_name(name), "{name}");
        }
    }

    #[test]
    fn a_file_name_that_is_a_staging_name_is_refused_by_every_operation() {
        let (_directory, store) = store();
        for path in ["a#1", "dir/b#42"] {
            assert!(store.locate_file(path).is_err(), "{path}");
        }
        assert!(store.locate_file("a#x").is_ok());
        assert!(store.locate_file("dir#1/b").is_ok());
        assert!(store.locate_file("").is_err());
    }

    #[tokio::test]
    async fn ranges_are_cut_short_at_the_end_of_the_file() {
        let (_directory, store) = store();
        store
            .put("digits", Bytes::from("0123456789"))
            .await
            .unwrap();
        let read = |range| store.read("digits", range);
        assert_eq!(read(None).await.unwrap(), "0123456789");
        assert_eq!(read(Some(0..4)).await.unwrap(), "0123");
        assert_eq!(read(Some(3..6)).await.unwrap(), "345");
        assert_eq!(read(Some(8..100)).await.unwrap(), "89");
        assert_eq!(read(Some(20..30)).await.unwrap(), "");
        assert_eq!(read(Some(5..5)).await.unwrap(), "");
        assert!(matches!(
            store.read("missing", None).await,
            Err(StoreError::NotFound)
        ));
    }

    #[tokio::test]
    async fn a_stream_has_the_whole_file_in_chunks() {
        let (_directory, store) = store();
        let contents: Vec<u8> = (0..(3 * CHUNK_BYTES + 5))
            .map(|n| (n % 253) as u8)
            .collect();
        store
            .put("big", Bytes::from(contents.clone()))
            .await
            .unwrap();
        let object = store.open("big").await.unwrap();
        assert_eq!(object.size, contents.len() as u64);
        let chunks: Vec<Bytes> = object.stream.try_collect().await.unwrap();
        assert_eq!(chunks.len(), 4);
        assert!(chunks.iter().all(|chunk| chunk.len() <= CHUNK_BYTES));
        assert_eq!(chunks.concat(), contents);
        assert!(matches!(
            store.open("nothing").await,
            Err(StoreError::NotFound)
        ));
    }

    #[tokio::test]
    async fn a_file_that_shrinks_while_it_is_read_is_an_error_not_a_short_read() {
        let (directory, store) = store();
        store
            .put("shrinking", Bytes::from(vec![7u8; 3 * CHUNK_BYTES]))
            .await
            .unwrap();
        let mut object = store.open("shrinking").await.unwrap();
        assert_eq!(
            object.stream.try_next().await.unwrap().unwrap().len(),
            CHUNK_BYTES
        );
        fs::OpenOptions::new()
            .write(true)
            .open(directory.path().join("shrinking"))
            .unwrap()
            .set_len(10)
            .unwrap();
        let rest: Result<Vec<Bytes>, StoreError> = object.stream.try_collect().await;
        assert!(rest.is_err());
    }

    #[tokio::test]
    async fn an_upload_appears_only_when_it_is_finished() {
        let (directory, store) = store();
        let mut upload = store.begin_upload("up/load.bin").await.unwrap();
        upload.write(Bytes::from("one ")).await.unwrap();
        upload.write(Bytes::from("two")).await.unwrap();
        assert!(matches!(
            store.head("up/load.bin").await,
            Err(StoreError::NotFound)
        ));
        upload.finish().await.unwrap();
        assert_eq!(store.read("up/load.bin", None).await.unwrap(), "one two");

        let mut upload = store.begin_upload("up/aborted.bin").await.unwrap();
        upload.write(Bytes::from("x")).await.unwrap();
        upload.abort().await.unwrap();
        let dropped = store.begin_upload("up/dropped.bin").await.unwrap();
        drop(dropped);
        let names: Vec<_> = fs::read_dir(directory.path().join("up"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, ["load.bin"]);
    }

    #[tokio::test]
    async fn a_failed_publish_removes_the_staging_file() {
        let (directory, store) = store();
        fs::create_dir(directory.path().join("taken")).unwrap();
        let mut upload = store.begin_upload("taken").await.unwrap();
        upload.write(Bytes::from("x")).await.unwrap();
        assert!(matches!(upload.finish().await, Err(StoreError::Failed(_))));
        let mut names: Vec<_> = fs::read_dir(directory.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        names.sort();
        assert_eq!(names, ["taken"]);
        assert!(matches!(
            store.put("taken", Bytes::from("x")).await,
            Err(StoreError::Failed(_))
        ));
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[tokio::test]
    async fn staging_names_that_are_taken_are_skipped() {
        let (directory, store) = store();
        fs::write(directory.path().join("f#1"), "someone else's").unwrap();
        fs::write(directory.path().join("f#2"), "someone else's").unwrap();
        store.put("f", Bytes::from("mine")).await.unwrap();
        assert_eq!(
            fs::read_to_string(directory.path().join("f")).unwrap(),
            "mine"
        );
        assert_eq!(
            fs::read_to_string(directory.path().join("f#1")).unwrap(),
            "someone else's"
        );
    }

    #[tokio::test]
    async fn listing_skips_staging_files_and_names_what_it_found() {
        let (directory, store) = store();
        store.put("a/one.txt", Bytes::from("1")).await.unwrap();
        store.put("a/b/two.txt", Bytes::from("22")).await.unwrap();
        fs::write(directory.path().join("a").join("three.txt#9"), "x").unwrap();
        let mut files = store.list("a").await.unwrap();
        files.sort_by(|x, y| x.path.cmp(&y.path));
        let listed: Vec<(&str, u64)> = files.iter().map(|f| (f.path.as_str(), f.size)).collect();
        assert_eq!(listed, [("a/b/two.txt", 2), ("a/one.txt", 1)]);
        assert_eq!(store.list("").await.unwrap().len(), 2);
        assert!(store.list("a/one.txt").await.unwrap().is_empty());
        assert!(store.list("nope").await.unwrap().is_empty());
    }

    #[test]
    fn it_names_its_root() {
        let (directory, store) = store();
        let shown = store.to_string();
        assert!(shown.starts_with("Local("), "{shown}");
        assert!(
            shown.contains(directory.path().file_name().unwrap().to_str().unwrap()),
            "{shown}"
        );
    }
}
