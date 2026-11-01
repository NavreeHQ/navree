//! [`Memory`]: files in process memory.

use std::collections::BTreeMap;
use std::fmt;
use std::ops::Range;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use bytes::{Bytes, BytesMut};
use chrono::{DateTime, Utc};
use futures_util::stream;
use navree_kernel::BoxFuture;

use super::{Meta, Object, Store, StoreError, StoreResult, Upload};

/// Files kept in memory, lost with the process: for tests and scratch data.
#[derive(Default)]
pub(crate) struct Memory {
    files: Arc<Mutex<BTreeMap<String, Entry>>>,
}

#[derive(Clone)]
struct Entry {
    contents: Bytes,
    last_modified: DateTime<Utc>,
}

type Files = Mutex<BTreeMap<String, Entry>>;

/// The files, even if a thread panicked while holding them: every change
/// is one insert or remove, so they are never half written.
fn locked(files: &Files) -> MutexGuard<'_, BTreeMap<String, Entry>> {
    files.lock().unwrap_or_else(PoisonError::into_inner)
}

fn insert(files: &Files, path: &str, contents: Bytes) {
    let entry = Entry {
        contents,
        last_modified: Utc::now(),
    };
    locked(files).insert(path.to_owned(), entry);
}

impl Memory {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    fn entry(&self, path: &str) -> StoreResult<Entry> {
        locked(&self.files)
            .get(path)
            .cloned()
            .ok_or(StoreError::NotFound)
    }
}

impl fmt::Display for Memory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Memory")
    }
}

impl Store for Memory {
    fn put<'a>(&'a self, path: &'a str, contents: Bytes) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move {
            insert(&self.files, path, contents);
            Ok(())
        })
    }

    fn begin_upload<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<Box<dyn Upload>>> {
        Box::pin(async move {
            Ok(Box::new(MemoryUpload {
                files: self.files.clone(),
                path: path.to_owned(),
                buffer: BytesMut::new(),
            }) as Box<dyn Upload>)
        })
    }

    fn read<'a>(
        &'a self,
        path: &'a str,
        range: Option<Range<u64>>,
    ) -> BoxFuture<'a, StoreResult<Bytes>> {
        Box::pin(async move {
            let contents = self.entry(path)?.contents;
            let Some(range) = range else {
                return Ok(contents);
            };
            let end =
                usize::try_from(range.end).map_or(contents.len(), |end| end.min(contents.len()));
            let start = usize::try_from(range.start).map_or(end, |start| start.min(end));
            Ok(contents.slice(start..end))
        })
    }

    fn open<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<Object>> {
        Box::pin(async move {
            let contents = self.entry(path)?.contents;
            Ok(Object {
                size: contents.len() as u64,
                stream: Box::pin(stream::once(async move { Ok(contents) })),
            })
        })
    }

    fn head<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<Meta>> {
        Box::pin(async move {
            let entry = self.entry(path)?;
            Ok(Meta {
                path: path.to_owned(),
                size: entry.contents.len() as u64,
                last_modified: entry.last_modified,
            })
        })
    }

    fn delete<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move {
            locked(&self.files)
                .remove(path)
                .map(drop)
                .ok_or(StoreError::NotFound)
        })
    }

    fn list<'a>(&'a self, prefix: &'a str) -> BoxFuture<'a, StoreResult<Vec<Meta>>> {
        Box::pin(async move {
            // A prefix is a directory: `docs` holds `docs/a.txt`, not `docs2/b.txt`.
            let directory = (!prefix.is_empty()).then(|| format!("{prefix}/"));
            let files = locked(&self.files);
            Ok(files
                .iter()
                .filter(|(path, _)| directory.as_ref().is_none_or(|dir| path.starts_with(dir)))
                .map(|(path, entry)| Meta {
                    path: path.clone(),
                    size: entry.contents.len() as u64,
                    last_modified: entry.last_modified,
                })
                .collect())
        })
    }
}

/// Chunks collected until the upload is finished.
struct MemoryUpload {
    files: Arc<Files>,
    path: String,
    buffer: BytesMut,
}

impl Upload for MemoryUpload {
    fn write<'a>(&'a mut self, chunk: Bytes) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move {
            self.buffer.extend_from_slice(&chunk);
            Ok(())
        })
    }

    fn finish(self: Box<Self>) -> BoxFuture<'static, StoreResult<()>> {
        Box::pin(async move {
            insert(&self.files, &self.path, self.buffer.freeze());
            Ok(())
        })
    }

    fn abort(self: Box<Self>) -> BoxFuture<'static, StoreResult<()>> {
        Box::pin(async move { Ok(()) })
    }
}

#[cfg(test)]
mod tests {
    use futures_util::TryStreamExt;

    use super::*;

    #[tokio::test]
    async fn files_are_put_read_and_deleted() {
        let store = Memory::new();
        store.put("a/b.txt", Bytes::from("hello")).await.unwrap();
        assert_eq!(store.read("a/b.txt", None).await.unwrap(), "hello");
        assert_eq!(store.read("a/b.txt", Some(1..3)).await.unwrap(), "el");
        assert_eq!(store.read("a/b.txt", Some(3..99)).await.unwrap(), "lo");
        assert_eq!(store.read("a/b.txt", Some(9..12)).await.unwrap(), "");
        let meta = store.head("a/b.txt").await.unwrap();
        assert_eq!((meta.path.as_str(), meta.size), ("a/b.txt", 5));
        let object = store.open("a/b.txt").await.unwrap();
        assert_eq!(object.size, 5);
        let chunks: Vec<Bytes> = object.stream.try_collect().await.unwrap();
        assert_eq!(chunks.concat(), b"hello");

        store.put("a/b.txt", Bytes::from("again")).await.unwrap();
        assert_eq!(store.read("a/b.txt", None).await.unwrap(), "again");
        store.delete("a/b.txt").await.unwrap();
        for result in [
            store.read("a/b.txt", None).await.map(drop),
            store.head("a/b.txt").await.map(drop),
            store.open("a/b.txt").await.map(drop),
            store.delete("a/b.txt").await,
        ] {
            assert!(matches!(result, Err(StoreError::NotFound)));
        }
    }

    #[tokio::test]
    async fn a_prefix_is_a_directory() {
        let store = Memory::new();
        for path in [
            "docs/b.txt",
            "docs/a.txt",
            "docs/2026/r.pdf",
            "docs2/o.txt",
            "docs.txt",
        ] {
            store.put(path, Bytes::from("x")).await.unwrap();
        }
        let paths =
            |files: Vec<Meta>| -> Vec<String> { files.into_iter().map(|file| file.path).collect() };
        assert_eq!(
            paths(store.list("docs").await.unwrap()),
            ["docs/2026/r.pdf", "docs/a.txt", "docs/b.txt"]
        );
        assert_eq!(store.list("").await.unwrap().len(), 5);
        assert!(store.list("docs/a.txt").await.unwrap().is_empty());
        assert!(store.list("nothing").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_upload_appears_when_it_is_finished_and_not_when_it_is_aborted() {
        let store = Memory::new();
        let mut upload = store.begin_upload("up.bin").await.unwrap();
        upload.write(Bytes::from("ab")).await.unwrap();
        upload.write(Bytes::from("cd")).await.unwrap();
        assert!(matches!(
            store.head("up.bin").await,
            Err(StoreError::NotFound)
        ));
        upload.finish().await.unwrap();
        assert_eq!(store.read("up.bin", None).await.unwrap(), "abcd");

        let mut upload = store.begin_upload("gone.bin").await.unwrap();
        upload.write(Bytes::from("x")).await.unwrap();
        upload.abort().await.unwrap();
        assert!(matches!(
            store.head("gone.bin").await,
            Err(StoreError::NotFound)
        ));
    }

    #[tokio::test]
    async fn the_last_modified_time_is_the_time_of_the_write() {
        let store = Memory::new();
        let before = Utc::now();
        store.put("t", Bytes::new()).await.unwrap();
        let meta = store.head("t").await.unwrap();
        assert!(meta.last_modified >= before && meta.last_modified <= Utc::now());
        assert_eq!(meta.size, 0);
    }
}
