//! [`Prefixed`]: a store whose files all sit under one directory of another.

use std::fmt;
use std::ops::Range;
use std::sync::Arc;

use bytes::Bytes;
use navree_kernel::BoxFuture;

use super::{Meta, Object, Store, StoreResult, Upload};

/// `inner`, seen from the directory `prefix`: path `a.txt` is `prefix/a.txt`
/// in `inner`, and listings leave the prefix out again.
pub(crate) struct Prefixed {
    inner: Arc<dyn Store>,
    /// A normalised, non-empty path.
    prefix: String,
}

impl Prefixed {
    pub(crate) fn new(inner: Arc<dyn Store>, prefix: String) -> Self {
        Self { inner, prefix }
    }

    /// The path in `inner` of `path` (which may be `""`, the directory itself).
    fn full(&self, path: &str) -> String {
        if path.is_empty() {
            self.prefix.clone()
        } else {
            format!("{}/{path}", self.prefix)
        }
    }
}

impl fmt::Display for Prefixed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Prefixed({}, {})", self.prefix, self.inner)
    }
}

impl Store for Prefixed {
    fn put<'a>(&'a self, path: &'a str, contents: Bytes) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move { self.inner.put(&self.full(path), contents).await })
    }

    fn begin_upload<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<Box<dyn Upload>>> {
        Box::pin(async move { self.inner.begin_upload(&self.full(path)).await })
    }

    fn read<'a>(
        &'a self,
        path: &'a str,
        range: Option<Range<u64>>,
    ) -> BoxFuture<'a, StoreResult<Bytes>> {
        Box::pin(async move { self.inner.read(&self.full(path), range).await })
    }

    fn open<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<Object>> {
        Box::pin(async move { self.inner.open(&self.full(path)).await })
    }

    fn head<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<Meta>> {
        Box::pin(async move {
            let mut meta = self.inner.head(&self.full(path)).await?;
            meta.path = path.to_owned();
            Ok(meta)
        })
    }

    fn delete<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move { self.inner.delete(&self.full(path)).await })
    }

    fn list<'a>(&'a self, prefix: &'a str) -> BoxFuture<'a, StoreResult<Vec<Meta>>> {
        Box::pin(async move {
            let directory = format!("{}/", self.prefix);
            let mut files = self.inner.list(&self.full(prefix)).await?;
            files.retain_mut(|meta| match meta.path.strip_prefix(&directory) {
                Some(relative) => {
                    meta.path = relative.to_owned();
                    true
                }
                None => false,
            });
            Ok(files)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Memory, StoreError};

    fn setup() -> (Arc<Memory>, Prefixed) {
        let memory = Arc::new(Memory::new());
        let prefixed = Prefixed::new(memory.clone(), "tenants/agra".to_owned());
        (memory, prefixed)
    }

    #[tokio::test]
    async fn files_live_under_the_prefix() {
        let (memory, prefixed) = setup();
        prefixed.put("docs/a.txt", Bytes::from("a")).await.unwrap();
        let mut upload = prefixed.begin_upload("docs/b.txt").await.unwrap();
        upload.write(Bytes::from("b")).await.unwrap();
        upload.finish().await.unwrap();

        assert_eq!(
            memory.read("tenants/agra/docs/a.txt", None).await.unwrap(),
            "a"
        );
        assert_eq!(prefixed.read("docs/b.txt", None).await.unwrap(), "b");
        assert_eq!(
            prefixed.head("docs/a.txt").await.unwrap().path,
            "docs/a.txt"
        );
        assert!(matches!(
            memory.head("docs/a.txt").await,
            Err(StoreError::NotFound)
        ));
        prefixed.delete("docs/a.txt").await.unwrap();
        assert!(matches!(
            memory.head("tenants/agra/docs/a.txt").await,
            Err(StoreError::NotFound)
        ));
    }

    #[tokio::test]
    async fn listings_leave_the_prefix_out_and_other_prefixes_alone() {
        let (memory, prefixed) = setup();
        memory
            .put("tenants/agra/docs/a.txt", Bytes::from("a"))
            .await
            .unwrap();
        memory
            .put("tenants/agra/b.txt", Bytes::from("b"))
            .await
            .unwrap();
        memory
            .put("tenants/agra2/c.txt", Bytes::from("c"))
            .await
            .unwrap();
        memory
            .put("tenants/other/d.txt", Bytes::from("d"))
            .await
            .unwrap();
        memory.put("e.txt", Bytes::from("e")).await.unwrap();

        let paths = |files: Vec<Meta>| {
            let mut paths: Vec<String> = files.into_iter().map(|meta| meta.path).collect();
            paths.sort();
            paths
        };
        assert_eq!(
            paths(prefixed.list("").await.unwrap()),
            ["b.txt", "docs/a.txt"]
        );
        assert_eq!(paths(prefixed.list("docs").await.unwrap()), ["docs/a.txt"]);
        assert!(prefixed.list("nothing").await.unwrap().is_empty());
    }

    #[test]
    fn it_names_its_prefix_and_its_store() {
        let (_, prefixed) = setup();
        assert_eq!(prefixed.to_string(), "Prefixed(tenants/agra, Memory)");
    }
}
