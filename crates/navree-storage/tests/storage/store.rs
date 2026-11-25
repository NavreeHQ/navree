//! The public store (`navree_storage::store`): a store written outside the
//! crate implements `Store` with what the crate exports, sits under a
//! prefix like any other, and gives signed links when it can sign.

use std::fmt;
use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use bytes::Bytes;
use futures_util::TryStreamExt;
use navree_kernel::BoxFuture;
use navree_storage::store::{
    MemoryStore, Meta, Method, Object, PrefixedStore, Store, StoreError, StoreResult, Upload, Url,
};

/// A store in front of another, as a plugin's would be: it counts what it
/// is asked, refuses every write once it is told to, and signs links to
/// `https://files.amritsar.example`.
#[derive(Default)]
struct Godown {
    files: MemoryStore,
    asked: AtomicUsize,
    full: AtomicBool,
}

impl Godown {
    fn asked(&self) -> usize {
        self.asked.swap(0, Ordering::SeqCst)
    }

    fn count(&self) {
        self.asked.fetch_add(1, Ordering::SeqCst);
    }

    fn room(&self) -> StoreResult<()> {
        if self.full.load(Ordering::SeqCst) {
            Err(StoreError::failed("the Amritsar godown is full"))
        } else {
            Ok(())
        }
    }
}

impl fmt::Display for Godown {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Godown({})", self.files)
    }
}

impl Store for Godown {
    fn put<'a>(&'a self, path: &'a str, contents: Bytes) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move {
            self.count();
            self.room()?;
            self.files.put(path, contents).await
        })
    }

    fn begin_upload<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<Box<dyn Upload>>> {
        Box::pin(async move {
            self.count();
            self.room()?;
            self.files.begin_upload(path).await
        })
    }

    fn read<'a>(
        &'a self,
        path: &'a str,
        range: Option<Range<u64>>,
    ) -> BoxFuture<'a, StoreResult<Bytes>> {
        Box::pin(async move {
            self.count();
            self.files.read(path, range).await
        })
    }

    fn open<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<Object>> {
        Box::pin(async move {
            self.count();
            // An object of its own making, as a driver hands one over.
            let contents = self.files.read(path, None).await?;
            let size = contents.len() as u64;
            let halves = [
                contents.slice(..contents.len() / 2),
                contents.slice(contents.len() / 2..),
            ];
            let stream = futures_util::stream::iter(halves.map(Ok));
            Ok(Object::new(size, Box::pin(stream)))
        })
    }

    fn head<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<Meta>> {
        Box::pin(async move {
            self.count();
            self.files.head(path).await
        })
    }

    fn delete<'a>(&'a self, path: &'a str) -> BoxFuture<'a, StoreResult<()>> {
        Box::pin(async move {
            self.count();
            self.files.delete(path).await
        })
    }

    fn list<'a>(&'a self, prefix: &'a str) -> BoxFuture<'a, StoreResult<Vec<Meta>>> {
        Box::pin(async move {
            self.count();
            self.files.list(prefix).await
        })
    }

    fn signed_url<'a>(
        &'a self,
        path: &'a str,
        method: Method,
        expires_in: Duration,
    ) -> BoxFuture<'a, StoreResult<Url>> {
        Box::pin(async move {
            self.count();
            if method != Method::GET && method != Method::PUT {
                return Err(StoreError::failed(format!(
                    "the godown signs no {method} links"
                )));
            }
            let link = format!(
                "https://files.amritsar.example/{path}?method={method}&expires={}&signature=abc",
                expires_in.as_secs()
            );
            Url::parse(&link).map_err(StoreError::failed)
        })
    }
}

fn godown_under(prefix: &str) -> (Arc<Godown>, Arc<dyn Store>) {
    let godown = Arc::new(Godown::default());
    let store: Arc<dyn Store> = Arc::new(PrefixedStore::new(godown.clone(), prefix));
    (godown, store)
}

#[tokio::test]
async fn a_store_written_outside_the_crate_is_served_under_a_prefix() {
    let (godown, store) = godown_under("tenants/agra");
    assert_eq!(store.to_string(), "Prefixed(tenants/agra, Godown(Memory))");

    store
        .put(
            "challans/CH-0042.csv",
            Bytes::from("item,total\nsugar,100000\n"),
        )
        .await
        .unwrap();
    let mut upload = store.begin_upload("challans/CH-0043.csv").await.unwrap();
    upload.write(Bytes::from("item,total\n")).await.unwrap();
    upload.write(Bytes::from("rice,42\n")).await.unwrap();
    assert!(matches!(
        store.head("challans/CH-0043.csv").await,
        Err(StoreError::NotFound)
    ));
    upload.finish().await.unwrap();

    // The files are in the store behind, under the prefix.
    assert_eq!(
        godown
            .files
            .read("tenants/agra/challans/CH-0043.csv", None)
            .await
            .unwrap(),
        "item,total\nrice,42\n"
    );
    assert_eq!(
        store
            .read("challans/CH-0042.csv", Some(11..16))
            .await
            .unwrap(),
        "sugar"
    );
    let object = store.open("challans/CH-0042.csv").await.unwrap();
    assert_eq!(object.size, 24);
    assert_eq!(format!("{object:?}"), "Object { size: 24, .. }");
    let chunks: Vec<Bytes> = object.stream.try_collect().await.unwrap();
    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks.concat(), b"item,total\nsugar,100000\n");

    let meta = store.head("challans/CH-0042.csv").await.unwrap();
    assert_eq!(
        (meta.path.as_str(), meta.size),
        ("challans/CH-0042.csv", 24)
    );
    let mut listed: Vec<String> = store
        .list("challans")
        .await
        .unwrap()
        .into_iter()
        .map(|meta| meta.path)
        .collect();
    listed.sort();
    assert_eq!(listed, ["challans/CH-0042.csv", "challans/CH-0043.csv"]);

    store.delete("challans/CH-0042.csv").await.unwrap();
    assert!(matches!(
        store.delete("challans/CH-0042.csv").await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        store.read("challans/CH-0042.csv", None).await,
        Err(StoreError::NotFound)
    ));
    // Every operation reached the store behind, once.
    assert_eq!(godown.asked(), 10);
}

#[tokio::test]
async fn a_failure_of_the_store_behind_is_the_failure_the_caller_gets() {
    let (godown, store) = godown_under("tenants/agra");
    godown.full.store(true, Ordering::SeqCst);
    for result in [
        store.put("a.txt", Bytes::from("a")).await,
        store.begin_upload("b.txt").await.map(drop),
    ] {
        match result {
            Err(StoreError::Failed(error)) => {
                assert_eq!(error.to_string(), "the Amritsar godown is full");
            }
            other => panic!("{other:?}"),
        }
    }
    assert!(godown.files.list("").await.unwrap().is_empty());
}

#[tokio::test]
async fn a_link_is_signed_by_a_store_that_can_sign_and_by_no_other() {
    let (godown, store) = godown_under("tenants/agra");
    let link = store
        .signed_url(
            "challans/CH 0042.csv",
            Method::GET,
            Duration::from_secs(900),
        )
        .await
        .unwrap();
    assert_eq!(link.host_str(), Some("files.amritsar.example"));
    assert_eq!(link.path(), "/tenants/agra/challans/CH%200042.csv");
    assert_eq!(link.query(), Some("method=GET&expires=900&signature=abc"));
    let link = store
        .signed_url("challans/new.csv", Method::PUT, Duration::from_secs(60))
        .await
        .unwrap();
    assert_eq!(link.path(), "/tenants/agra/challans/new.csv");
    assert_eq!(link.query(), Some("method=PUT&expires=60&signature=abc"));
    // A method it does not sign is a failure of the store.
    match store
        .signed_url("challans/new.csv", Method::DELETE, Duration::from_secs(60))
        .await
    {
        Err(StoreError::Failed(error)) => {
            assert_eq!(error.to_string(), "the godown signs no DELETE links");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(godown.asked(), 3);

    // A store in memory signs none, with or without a prefix.
    let memory: Arc<dyn Store> = Arc::new(MemoryStore::new());
    let prefixed: Arc<dyn Store> = Arc::new(PrefixedStore::new(memory.clone(), "tenants/agra"));
    for store in [memory, prefixed] {
        let refused = store
            .signed_url("a.txt", Method::GET, Duration::from_secs(60))
            .await
            .unwrap_err();
        assert!(matches!(refused, StoreError::Unsupported(_)), "{refused:?}");
        assert_eq!(refused.to_string(), "this store has no signed links");
    }
}
