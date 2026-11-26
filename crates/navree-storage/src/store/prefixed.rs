//! [`PrefixedStore`]: a store whose files all sit under one directory of
//! another.

use std::fmt;
use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use navree_kernel::BoxFuture;

use super::{HeaderName, HeaderValue, Meta, Method, Object, Store, StoreResult, Upload, Url};

/// `inner`, seen from the directory `prefix`: path `a.txt` is `prefix/a.txt`
/// in `inner`, listings leave the prefix out again, and a link is signed
/// by `inner` for the path under the prefix, bound to the headers given.
///
/// ```
/// use std::sync::Arc;
///
/// use bytes::Bytes;
/// use navree_storage::store::{MemoryStore, PrefixedStore, Store};
///
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() -> Result<(), navree_storage::store::StoreError> {
/// let files = Arc::new(MemoryStore::new());
/// let jaipur = PrefixedStore::new(files.clone(), "tenants/jaipur");
/// jaipur.put("a.txt", Bytes::from("a")).await?;
/// assert_eq!(files.read("tenants/jaipur/a.txt", None).await?, "a");
/// assert_eq!(jaipur.to_string(), "Prefixed(tenants/jaipur, Memory)");
/// # Ok(())
/// # }
/// ```
pub struct PrefixedStore {
    inner: Arc<dyn Store>,
    /// A normalised path without `/` at its ends; empty for `inner` itself.
    prefix: String,
}

impl PrefixedStore {
    /// `inner` seen from its directory `prefix`, a normalised path
    /// (`tenants/agra`). A `/` at either end of it is dropped; with an
    /// empty prefix the store is `inner` itself.
    pub fn new(inner: Arc<dyn Store>, prefix: impl Into<String>) -> Self {
        let prefix: String = prefix.into();
        let trimmed = prefix.trim_matches('/');
        let prefix = if trimmed.len() == prefix.len() {
            prefix
        } else {
            trimmed.to_owned()
        };
        Self { inner, prefix }
    }

    /// The path in `inner` of `path` (which may be `""`, the directory itself).
    fn full(&self, path: &str) -> String {
        if self.prefix.is_empty() {
            path.to_owned()
        } else if path.is_empty() {
            self.prefix.clone()
        } else {
            format!("{}/{path}", self.prefix)
        }
    }
}

impl fmt::Display for PrefixedStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Prefixed({}, {})", self.prefix, self.inner)
    }
}

/// The prefix and what the store under it calls itself.
impl fmt::Debug for PrefixedStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PrefixedStore")
            .field("prefix", &self.prefix)
            .field("inner", &self.inner.to_string())
            .finish()
    }
}

impl Store for PrefixedStore {
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
            let mut files = self.inner.list(&self.full(prefix)).await?;
            if self.prefix.is_empty() {
                return Ok(files);
            }
            let directory = format!("{}/", self.prefix);
            files.retain_mut(|meta| match meta.path.strip_prefix(&directory) {
                Some(relative) if !relative.is_empty() => {
                    meta.path = relative.to_owned();
                    true
                }
                _ => false,
            });
            Ok(files)
        })
    }

    fn signed_url<'a>(
        &'a self,
        path: &'a str,
        method: Method,
        expires_in: Duration,
    ) -> BoxFuture<'a, StoreResult<Url>> {
        Box::pin(async move {
            self.inner
                .signed_url(&self.full(path), method, expires_in)
                .await
        })
    }

    fn signed_url_with<'a>(
        &'a self,
        path: &'a str,
        method: Method,
        expires_in: Duration,
        headers: &'a [(HeaderName, HeaderValue)],
    ) -> BoxFuture<'a, StoreResult<Url>> {
        Box::pin(async move {
            self.inner
                .signed_url_with(&self.full(path), method, expires_in, headers)
                .await
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::store::{MemoryStore, StoreError};

    fn setup() -> (Arc<MemoryStore>, PrefixedStore) {
        let memory = Arc::new(MemoryStore::new());
        let prefixed = PrefixedStore::new(memory.clone(), "tenants/agra");
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
        assert_eq!(
            format!("{prefixed:?}"),
            r#"PrefixedStore { prefix: "tenants/agra", inner: "Memory" }"#
        );
    }

    #[tokio::test]
    async fn a_slash_at_either_end_of_the_prefix_is_dropped() {
        let memory = Arc::new(MemoryStore::new());
        let prefixed = PrefixedStore::new(memory.clone(), "/tenants/agra/");
        prefixed.put("a.txt", Bytes::from("a")).await.unwrap();
        assert_eq!(memory.read("tenants/agra/a.txt", None).await.unwrap(), "a");
        assert_eq!(prefixed.list("").await.unwrap()[0].path, "a.txt");
        assert_eq!(prefixed.to_string(), "Prefixed(tenants/agra, Memory)");
    }

    #[tokio::test]
    async fn an_empty_prefix_is_the_store_itself() {
        let memory = Arc::new(MemoryStore::new());
        for prefix in ["", "/"] {
            let prefixed = PrefixedStore::new(memory.clone(), prefix);
            prefixed.put("docs/a.txt", Bytes::from("a")).await.unwrap();
            assert_eq!(memory.read("docs/a.txt", None).await.unwrap(), "a");
            assert_eq!(
                prefixed.head("docs/a.txt").await.unwrap().path,
                "docs/a.txt"
            );
            let listed = prefixed.list("docs").await.unwrap();
            assert_eq!(listed.len(), 1);
            assert_eq!(listed[0].path, "docs/a.txt");
            assert_eq!(prefixed.list("").await.unwrap().len(), 1);
        }
    }

    /// What a store was asked to sign: by which method of the trait, the
    /// path, the method, the lifetime and the headers.
    type Asked = (
        &'static str,
        String,
        Method,
        Duration,
        Vec<(String, String)>,
    );

    /// A store that signs: it records what it is asked to sign and answers
    /// with a link to the path.
    #[derive(Default)]
    struct Signing {
        asked: Mutex<Vec<Asked>>,
    }

    impl Signing {
        fn sign(
            &self,
            by: &'static str,
            path: &str,
            method: Method,
            expires_in: Duration,
            headers: &[(HeaderName, HeaderValue)],
        ) -> StoreResult<Url> {
            let headers = headers
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_str().unwrap().to_owned()))
                .collect();
            self.asked
                .lock()
                .unwrap()
                .push((by, path.to_owned(), method, expires_in, headers));
            Url::parse(&format!("https://files.amritsar.example/{path}?sig=abc"))
                .map_err(StoreError::failed)
        }
    }

    impl fmt::Display for Signing {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("Signing")
        }
    }

    impl Store for Signing {
        fn put<'a>(&'a self, _: &'a str, _: Bytes) -> BoxFuture<'a, StoreResult<()>> {
            unimplemented!()
        }

        fn begin_upload<'a>(&'a self, _: &'a str) -> BoxFuture<'a, StoreResult<Box<dyn Upload>>> {
            unimplemented!()
        }

        fn read<'a>(
            &'a self,
            _: &'a str,
            _: Option<Range<u64>>,
        ) -> BoxFuture<'a, StoreResult<Bytes>> {
            unimplemented!()
        }

        fn open<'a>(&'a self, _: &'a str) -> BoxFuture<'a, StoreResult<Object>> {
            unimplemented!()
        }

        fn head<'a>(&'a self, _: &'a str) -> BoxFuture<'a, StoreResult<Meta>> {
            unimplemented!()
        }

        fn delete<'a>(&'a self, _: &'a str) -> BoxFuture<'a, StoreResult<()>> {
            unimplemented!()
        }

        fn list<'a>(&'a self, _: &'a str) -> BoxFuture<'a, StoreResult<Vec<Meta>>> {
            unimplemented!()
        }

        // A store may sign a link without headers in a way of its own.
        fn signed_url<'a>(
            &'a self,
            path: &'a str,
            method: Method,
            expires_in: Duration,
        ) -> BoxFuture<'a, StoreResult<Url>> {
            Box::pin(async move { self.sign("signed_url", path, method, expires_in, &[]) })
        }

        fn signed_url_with<'a>(
            &'a self,
            path: &'a str,
            method: Method,
            expires_in: Duration,
            headers: &'a [(HeaderName, HeaderValue)],
        ) -> BoxFuture<'a, StoreResult<Url>> {
            Box::pin(async move { self.sign("signed_url_with", path, method, expires_in, headers) })
        }
    }

    #[tokio::test]
    async fn a_link_is_signed_for_the_path_under_the_prefix() {
        let signing = Arc::new(Signing::default());
        let prefixed = PrefixedStore::new(signing.clone(), "tenants/agra");
        let link = prefixed
            .signed_url("docs/a.txt", Method::GET, Duration::from_secs(300))
            .await
            .unwrap();
        assert_eq!(
            link.as_str(),
            "https://files.amritsar.example/tenants/agra/docs/a.txt?sig=abc"
        );

        // Bound to headers, they reach the store under the prefix as given.
        let headers = [
            (
                HeaderName::from_static("content-type"),
                HeaderValue::from_static("image/png"),
            ),
            (
                HeaderName::from_static("x-goog-content-length-range"),
                HeaderValue::from_static("0,5000000"),
            ),
        ];
        let link = prefixed
            .signed_url_with(
                "avatars/a.png",
                Method::PUT,
                Duration::from_secs(600),
                &headers,
            )
            .await
            .unwrap();
        assert_eq!(link.path(), "/tenants/agra/avatars/a.png");

        let bound = |name: &str, value: &str| (name.to_owned(), value.to_owned());
        assert_eq!(
            *signing.asked.lock().unwrap(),
            [
                (
                    "signed_url",
                    "tenants/agra/docs/a.txt".to_owned(),
                    Method::GET,
                    Duration::from_secs(300),
                    Vec::new(),
                ),
                (
                    "signed_url_with",
                    "tenants/agra/avatars/a.png".to_owned(),
                    Method::PUT,
                    Duration::from_secs(600),
                    vec![
                        bound("content-type", "image/png"),
                        bound("x-goog-content-length-range", "0,5000000"),
                    ],
                ),
            ]
        );

        // Under a store that signs none, there is none either.
        let (_, prefixed) = setup();
        assert!(matches!(
            prefixed
                .signed_url("docs/a.txt", Method::GET, Duration::from_secs(300))
                .await,
            Err(StoreError::Unsupported(_))
        ));
        assert!(matches!(
            prefixed
                .signed_url_with(
                    "docs/a.txt",
                    Method::PUT,
                    Duration::from_secs(300),
                    &headers
                )
                .await,
            Err(StoreError::Unsupported(_))
        ));
    }
}
