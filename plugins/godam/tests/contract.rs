//! Direct contract checks for [`GodamDisk`], [`GodamSettings`] and
//! [`GodamModule`]: byte ranges, path normalisation, unfinished uploads,
//! missing files, folder boundaries and configuration errors.

use std::fmt;
use std::ops::Range;
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use futures_util::StreamExt;
use futures_util::stream::{self, BoxStream};
use navree::kernel::BoxFuture;
use navree::plugin::prelude::*;
use navree::prelude::*;
use navree::storage::{
    Disk, DiskDriver, DiskDriverError, DiskUpload, FileMeta, OpenedFile, StorageError,
};
use navree::testing::TestApp;
use navree_plugin_godam::{GodamDisk, GodamModule, GodamSettings};
use object_store::memory::InMemory;
use object_store::path::Path as ObjectPath;
use object_store::{
    CopyOptions, GetOptions, GetResult, ListResult, MultipartUpload, ObjectMeta, ObjectStore,
    PutMultipartOptions, PutOptions, PutPayload, PutResult,
};
use serde_json::json;

#[tokio::test]
async fn a_range_past_the_end_is_cut_and_one_past_the_file_is_empty() {
    let driver = GodamDisk::new(GodamSettings {
        prefix: "ludhiana/".to_owned(),
    })
    .unwrap();
    driver
        .put("notes/ten.txt", Bytes::from_static(b"0123456789"))
        .await
        .unwrap();

    assert_eq!(
        driver.read("notes/ten.txt", None).await.unwrap(),
        Bytes::from_static(b"0123456789")
    );
    assert_eq!(
        driver.read("notes/ten.txt", Some(2..6)).await.unwrap(),
        Bytes::from_static(b"2345")
    );
    assert_eq!(
        driver.read("notes/ten.txt", Some(6..20)).await.unwrap(),
        Bytes::from_static(b"6789")
    );
    assert!(
        driver
            .read("notes/ten.txt", Some(10..20))
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        driver
            .read("notes/ten.txt", Some(12..20))
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        driver
            .read("notes/ten.txt", Some(0..0))
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        driver
            .read("notes/ten.txt", Some(8..3))
            .await
            .unwrap()
            .is_empty()
    );

    driver.put("notes/empty.txt", Bytes::new()).await.unwrap();
    assert!(
        driver
            .read("notes/empty.txt", Some(0..10))
            .await
            .unwrap()
            .is_empty()
    );

    assert!(matches!(
        driver.read("notes/missing.txt", Some(0..0)).await,
        Err(DiskDriverError::NotFound)
    ));
}

#[derive(Clone, Default)]
struct RecordingStore {
    inner: InMemory,
    paths: Arc<Mutex<Vec<String>>>,
}

impl RecordingStore {
    fn record(&self, path: &ObjectPath) {
        self.paths.lock().unwrap().push(path.as_ref().to_owned());
    }

    fn seen(&self) -> Vec<String> {
        self.paths.lock().unwrap().clone()
    }
}

impl fmt::Debug for RecordingStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RecordingStore").finish_non_exhaustive()
    }
}

impl fmt::Display for RecordingStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RecordingStore")
    }
}

#[async_trait::async_trait]
impl ObjectStore for RecordingStore {
    async fn put_opts(
        &self,
        location: &ObjectPath,
        payload: PutPayload,
        opts: PutOptions,
    ) -> object_store::Result<PutResult> {
        self.record(location);
        self.inner.put_opts(location, payload, opts).await
    }

    async fn put_multipart_opts(
        &self,
        location: &ObjectPath,
        opts: PutMultipartOptions,
    ) -> object_store::Result<Box<dyn MultipartUpload>> {
        self.record(location);
        self.inner.put_multipart_opts(location, opts).await
    }

    async fn get_opts(
        &self,
        location: &ObjectPath,
        options: GetOptions,
    ) -> object_store::Result<GetResult> {
        self.record(location);
        self.inner.get_opts(location, options).await
    }

    fn delete_stream(
        &self,
        locations: BoxStream<'static, object_store::Result<ObjectPath>>,
    ) -> BoxStream<'static, object_store::Result<ObjectPath>> {
        let paths = self.paths.clone();
        let recorded = locations
            .map(move |item| {
                if let Ok(path) = &item {
                    paths.lock().unwrap().push(path.as_ref().to_owned());
                }
                item
            })
            .boxed();
        self.inner.delete_stream(recorded)
    }

    fn list(
        &self,
        prefix: Option<&ObjectPath>,
    ) -> BoxStream<'static, object_store::Result<ObjectMeta>> {
        if let Some(prefix) = prefix {
            self.record(prefix);
        }
        self.inner.list(prefix)
    }

    async fn list_with_delimiter(
        &self,
        prefix: Option<&ObjectPath>,
    ) -> object_store::Result<ListResult> {
        if let Some(prefix) = prefix {
            self.record(prefix);
        }
        self.inner.list_with_delimiter(prefix).await
    }

    async fn copy_opts(
        &self,
        from: &ObjectPath,
        to: &ObjectPath,
        options: CopyOptions,
    ) -> object_store::Result<()> {
        self.record(from);
        self.record(to);
        self.inner.copy_opts(from, to, options).await
    }
}

#[derive(Clone)]
struct RecordingDriver {
    inner: GodamDisk,
    paths: Arc<Mutex<Vec<String>>>,
}

impl RecordingDriver {
    fn record(&self, path: &str) {
        self.paths.lock().unwrap().push(path.to_owned());
    }

    fn seen(&self) -> Vec<String> {
        self.paths.lock().unwrap().clone()
    }
}

impl DiskDriver for RecordingDriver {
    fn describe(&self) -> String {
        self.inner.describe()
    }

    fn put<'a>(
        &'a self,
        path: &'a str,
        contents: Bytes,
    ) -> BoxFuture<'a, std::result::Result<(), DiskDriverError>> {
        self.record(path);
        self.inner.put(path, contents)
    }

    fn begin_upload<'a>(
        &'a self,
        path: &'a str,
    ) -> BoxFuture<'a, std::result::Result<Box<dyn DiskUpload>, DiskDriverError>> {
        self.record(path);
        self.inner.begin_upload(path)
    }

    fn read<'a>(
        &'a self,
        path: &'a str,
        range: Option<Range<u64>>,
    ) -> BoxFuture<'a, std::result::Result<Bytes, DiskDriverError>> {
        self.record(path);
        self.inner.read(path, range)
    }

    fn open<'a>(
        &'a self,
        path: &'a str,
    ) -> BoxFuture<'a, std::result::Result<OpenedFile, DiskDriverError>> {
        self.record(path);
        self.inner.open(path)
    }

    fn head<'a>(
        &'a self,
        path: &'a str,
    ) -> BoxFuture<'a, std::result::Result<FileMeta, DiskDriverError>> {
        self.record(path);
        self.inner.head(path)
    }

    fn delete<'a>(
        &'a self,
        path: &'a str,
    ) -> BoxFuture<'a, std::result::Result<(), DiskDriverError>> {
        self.record(path);
        self.inner.delete(path)
    }

    fn list<'a>(
        &'a self,
        prefix: &'a str,
    ) -> BoxFuture<'a, std::result::Result<Vec<FileMeta>, DiskDriverError>> {
        self.record(prefix);
        self.inner.list(prefix)
    }
}

struct RecordingGodamModule {
    driver: RecordingDriver,
}

impl Module for RecordingGodamModule {
    fn register(&self, m: &mut ModuleBuilder) {
        let driver = self.driver.clone();
        m.disk_driver("godam_recorded", move |_| {
            let driver = driver.clone();
            async move { Ok(driver) }
        });
    }
}

#[tokio::test]
async fn godam_receives_only_normalised_paths() {
    let store = RecordingStore::default();
    let driver = RecordingDriver {
        inner: GodamDisk::from_store(Arc::new(store.clone())),
        paths: Arc::new(Mutex::new(Vec::new())),
    };

    let app = TestApp::builder()
        .config("storage.default", "uploads")
        .config(
            "storage.disks.uploads",
            json!({ "driver": "godam_recorded" }),
        )
        .module(StorageModule::from_config())
        .module(RecordingGodamModule {
            driver: driver.clone(),
        })
        .build()
        .await;

    let disk = app.get::<Disk>().await;

    assert!(matches!(
        disk.put("a/../b", "x").await,
        Err(StorageError::InvalidPath { .. })
    ));
    assert!(matches!(
        disk.put("x\0y", "x").await,
        Err(StorageError::InvalidPath { .. })
    ));
    disk.put(r"a\b", "x").await.unwrap();
    disk.put("./c", "x").await.unwrap();
    disk.put("//d", "x").await.unwrap();

    for seen in [driver.seen(), store.seen()] {
        assert_eq!(seen, vec!["a/b", "c", "d"]);
        for path in &seen {
            assert!(!path.split('/').any(|segment| segment == ".."), "{path:?}");
            assert!(!path.contains('\\'), "{path:?}");
            assert!(!path.chars().any(char::is_control), "{path:?}");
            assert!(!path.starts_with('/'), "{path:?}");
        }
    }

    app.shutdown().await.expect("clean shutdown");
}

#[tokio::test]
async fn a_failed_upload_leaves_no_file() {
    let app = TestApp::builder()
        .config("storage.default", "invoices")
        .config(
            "storage.disks.invoices",
            json!({ "driver": "godam", "prefix": "ludhiana/" }),
        )
        .module(StorageModule::from_config())
        .module(GodamModule)
        .build()
        .await;

    let disk = app.get::<Disk>().await;
    let chunks: Vec<std::result::Result<Bytes, std::io::Error>> = vec![
        Ok(Bytes::from_static(b"INV-1101,50000\n")),
        Ok(Bytes::from_static(b"INV-1102,50000\n")),
        Err(std::io::Error::other("sender disconnected")),
    ];
    let error = disk
        .put_stream("exports/april.csv", stream::iter(chunks))
        .await
        .unwrap_err();
    assert!(matches!(error, StorageError::Stream { .. }), "{error:?}");
    assert!(!disk.exists("exports/april.csv").await.unwrap());
    assert!(disk.list("").await.unwrap().is_empty());

    // Direct driver uploads aborted or dropped without finishing also leave nothing behind.
    let driver = GodamDisk::new(GodamSettings {
        prefix: "ludhiana/".to_owned(),
    })
    .unwrap();
    let mut aborted = driver.begin_upload("exports/aborted.csv").await.unwrap();
    aborted.write(Bytes::from_static(b"part-1")).await.unwrap();
    aborted.write(Bytes::from_static(b"part-2")).await.unwrap();
    aborted.abort().await.unwrap();
    assert!(matches!(
        driver.head("exports/aborted.csv").await,
        Err(DiskDriverError::NotFound)
    ));

    let mut dropped = driver.begin_upload("exports/dropped.csv").await.unwrap();
    dropped.write(Bytes::from_static(b"part-1")).await.unwrap();
    dropped.write(Bytes::from_static(b"part-2")).await.unwrap();
    drop(dropped);
    assert!(matches!(
        driver.head("exports/dropped.csv").await,
        Err(DiskDriverError::NotFound)
    ));
    assert!(driver.list("").await.unwrap().is_empty());

    app.shutdown().await.expect("clean shutdown");
}

#[tokio::test]
async fn a_missing_file_is_not_found_on_read_head_open_and_delete() {
    for prefix in ["", "ludhiana/"] {
        let driver = GodamDisk::new(GodamSettings {
            prefix: prefix.to_owned(),
        })
        .unwrap();

        assert!(matches!(
            driver.read("missing.txt", None).await,
            Err(DiskDriverError::NotFound)
        ));
        assert!(matches!(
            driver.read("missing.txt", Some(0..16)).await,
            Err(DiskDriverError::NotFound)
        ));
        assert!(matches!(
            driver.head("missing.txt").await,
            Err(DiskDriverError::NotFound)
        ));
        assert!(matches!(
            driver.open("missing.txt").await,
            Err(DiskDriverError::NotFound)
        ));
        assert!(matches!(
            driver.delete("missing.txt").await,
            Err(DiskDriverError::NotFound)
        ));
    }
}

async fn boot_error(disk_table: serde_json::Value) -> String {
    let sources = ConfigSources::new().set(
        "storage",
        json!({ "default": "invoices", "disks": { "invoices": disk_table } }),
    );
    let error = Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(StorageModule::from_config())
        .module(GodamModule)
        .boot()
        .await
        .expect_err("boot must fail");
    format!("{error}\n{error:?}")
}

#[tokio::test]
async fn a_bad_setting_fails_the_boot_naming_the_key_only() {
    let expected =
        r#"storage.disks.invoices.prefix must be a folder inside the disk, such as "ludhiana/""#;

    for (bad_prefix, secret_fragment) in [
        ("../elsewhere", "elsewhere"),
        ("/etc/ludhiana", "/etc/ludhiana"),
        (r"ludhiana\secret_dir", "secret_dir"),
        ("ludhiana\0secret_nul", "secret_nul"),
        ("C:/secret_drive", "secret_drive"),
        ("ludhiana//secret_empty", "secret_empty"),
        ("ludhiana/./secret_dot", "secret_dot"),
    ] {
        let text = boot_error(json!({ "driver": "godam", "prefix": bad_prefix })).await;
        assert!(text.contains(expected), "{bad_prefix:?} -> {text}");
        assert!(
            !text.contains(bad_prefix) && !text.contains(secret_fragment),
            "boot error echoed {bad_prefix:?}: {text}"
        );
    }

    let unknown = boot_error(json!({ "driver": "godam", "cache_size": 987654 })).await;
    assert!(unknown.contains("cache_size"), "{unknown}");
    assert!(!unknown.contains("987654"), "{unknown}");
}

#[tokio::test]
async fn listing_a_folder_stops_at_its_boundary() {
    for prefix in ["", "ludhiana/"] {
        let driver = GodamDisk::new(GodamSettings {
            prefix: prefix.to_owned(),
        })
        .unwrap();
        for path in [
            "docs/a.txt",
            "docs/2026/summary.pdf",
            "docs2/b.txt",
            "docs.txt",
            "root.txt",
        ] {
            driver.put(path, Bytes::from_static(b"x")).await.unwrap();
        }

        let mut docs: Vec<String> = driver
            .list("docs")
            .await
            .unwrap()
            .into_iter()
            .map(|meta| meta.path)
            .collect();
        docs.sort();
        assert_eq!(docs, vec!["docs/2026/summary.pdf", "docs/a.txt"]);

        let mut root: Vec<String> = driver
            .list("")
            .await
            .unwrap()
            .into_iter()
            .map(|meta| meta.path)
            .collect();
        root.sort();
        assert_eq!(
            root,
            vec![
                "docs.txt",
                "docs/2026/summary.pdf",
                "docs/a.txt",
                "docs2/b.txt",
                "root.txt",
            ]
        );

        assert!(driver.list("missing").await.unwrap().is_empty());
    }
}
