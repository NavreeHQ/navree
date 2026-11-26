//! Disk drivers a module registers under a name: a disk's table names one,
//! its factory reads the table as its settings, and the disk passes the
//! same conformance suite as the built-in drivers, its driver receiving
//! only normalised paths.

#[path = "storage/support.rs"]
mod support;

#[path = "storage/conformance.rs"]
#[allow(dead_code)]
mod conformance;

use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use futures_util::stream;
use navree_config::check::CheckOptions;
use navree_config::{ConfigModule, ConfigSources};
use navree_datetime::Timestamp;
use navree_kernel::{AppBuilder, BoxFuture, CommandSpec, Module, ModuleBuilder, Navree};
use navree_storage::{
    Disk, DiskConfig, DiskDriver, DiskDriverError, DiskUpload, FileMeta, ModuleBuilderStorageExt,
    OpenedFile, Storage, StorageModule, StorageOptions,
};
use serde::Deserialize;
use serde_json::json;

use crate::conformance::{Http, Subject};
use crate::support::{SIGNING_KEY, TestApp, boot_error, single_disk};

type Files = Arc<Mutex<BTreeMap<String, (Bytes, Timestamp)>>>;

/// Files in memory, as a module's driver keeps them; it records every path
/// it is given.
#[derive(Clone, Default)]
struct Sanduk {
    files: Files,
    paths: Arc<Mutex<Vec<String>>>,
}

impl Sanduk {
    /// Records `path` and returns the file there.
    fn file(&self, path: &str) -> Result<(Bytes, Timestamp), DiskDriverError> {
        self.saw(path);
        let files = self.files.lock().unwrap();
        files.get(path).cloned().ok_or(DiskDriverError::NotFound)
    }

    fn saw(&self, path: &str) {
        self.paths.lock().unwrap().push(path.to_owned());
    }

    fn paths(&self) -> Vec<String> {
        self.paths.lock().unwrap().clone()
    }
}

impl DiskDriver for Sanduk {
    fn describe(&self) -> String {
        "sanduk disk in memory".to_owned()
    }

    fn put<'a>(
        &'a self,
        path: &'a str,
        contents: Bytes,
    ) -> BoxFuture<'a, Result<(), DiskDriverError>> {
        self.saw(path);
        self.files
            .lock()
            .unwrap()
            .insert(path.to_owned(), (contents, Timestamp::now()));
        Box::pin(async { Ok(()) })
    }

    fn begin_upload<'a>(
        &'a self,
        path: &'a str,
    ) -> BoxFuture<'a, Result<Box<dyn DiskUpload>, DiskDriverError>> {
        self.saw(path);
        let upload = SandukUpload {
            files: self.files.clone(),
            path: path.to_owned(),
            buffer: BytesMut::new(),
        };
        Box::pin(async move { Ok(Box::new(upload) as Box<dyn DiskUpload>) })
    }

    fn read<'a>(
        &'a self,
        path: &'a str,
        range: Option<Range<u64>>,
    ) -> BoxFuture<'a, Result<Bytes, DiskDriverError>> {
        let read = self.file(path).map(|(contents, _)| match range {
            Some(range) => {
                let end = usize::try_from(range.end)
                    .map_or(contents.len(), |end| end.min(contents.len()));
                let start = usize::try_from(range.start).map_or(end, |start| start.min(end));
                contents.slice(start..end)
            }
            None => contents,
        });
        Box::pin(async move { read })
    }

    fn open<'a>(&'a self, path: &'a str) -> BoxFuture<'a, Result<OpenedFile, DiskDriverError>> {
        let opened = self.file(path).map(|(contents, _)| {
            let size = contents.len() as u64;
            OpenedFile::new(size, Box::pin(stream::once(async move { Ok(contents) })))
        });
        Box::pin(async move { opened })
    }

    fn head<'a>(&'a self, path: &'a str) -> BoxFuture<'a, Result<FileMeta, DiskDriverError>> {
        let meta = self.file(path).map(|(contents, last_modified)| FileMeta {
            path: path.to_owned(),
            size: contents.len() as u64,
            last_modified,
        });
        Box::pin(async move { meta })
    }

    fn delete<'a>(&'a self, path: &'a str) -> BoxFuture<'a, Result<(), DiskDriverError>> {
        self.saw(path);
        let removed = self.files.lock().unwrap().remove(path);
        Box::pin(async move { removed.map(drop).ok_or(DiskDriverError::NotFound) })
    }

    fn list<'a>(
        &'a self,
        prefix: &'a str,
    ) -> BoxFuture<'a, Result<Vec<FileMeta>, DiskDriverError>> {
        self.saw(prefix);
        // A prefix is a folder: `docs` holds `docs/a.txt`, not `docs2/b.txt`.
        let folder = format!("{prefix}/");
        let files: Vec<FileMeta> = self
            .files
            .lock()
            .unwrap()
            .iter()
            .filter(|(path, _)| prefix.is_empty() || path.starts_with(&folder))
            .map(|(path, (contents, last_modified))| FileMeta {
                path: path.clone(),
                size: contents.len() as u64,
                last_modified: *last_modified,
            })
            .collect();
        Box::pin(async move { Ok(files) })
    }
}

/// A file being written: it appears only when it is finished.
struct SandukUpload {
    files: Files,
    path: String,
    buffer: BytesMut,
}

impl DiskUpload for SandukUpload {
    fn write<'a>(&'a mut self, chunk: Bytes) -> BoxFuture<'a, Result<(), DiskDriverError>> {
        self.buffer.extend_from_slice(&chunk);
        Box::pin(async { Ok(()) })
    }

    fn finish(self: Box<Self>) -> BoxFuture<'static, Result<(), DiskDriverError>> {
        let SandukUpload {
            files,
            path,
            buffer,
        } = *self;
        files
            .lock()
            .unwrap()
            .insert(path, (buffer.freeze(), Timestamp::now()));
        Box::pin(async { Ok(()) })
    }

    fn abort(self: Box<Self>) -> BoxFuture<'static, Result<(), DiskDriverError>> {
        Box::pin(async { Ok(()) })
    }
}

/// What a `sanduk` disk reads from its table.
#[derive(Default, Deserialize)]
#[serde(default)]
struct SandukSettings {
    label: String,
}

/// Registers the disk driver `sanduk` over files the test keeps, and
/// records the label and the disk its factory was given.
#[derive(Default)]
struct SandukModule {
    driver: Sanduk,
    seen: Seen,
}

/// The label and the disk a factory was given, once it has run.
type Seen = Arc<Mutex<Option<(String, Option<String>)>>>;

impl Module for SandukModule {
    fn register(&self, m: &mut ModuleBuilder) {
        let (driver, seen) = (self.driver.clone(), self.seen.clone());
        m.disk_driver("sanduk", move |ctx| {
            let (driver, seen) = (driver.clone(), seen.clone());
            async move {
                let settings: SandukSettings = ctx.settings()?;
                *seen.lock().unwrap() = Some((settings.label, ctx.instance().map(str::to_owned)));
                Ok(driver)
            }
        });
    }
}

/// A module that registers one command.
struct CommandModule(CommandSpec);

impl Module for CommandModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.command(self.0.clone());
    }
}

/// What `config:check` prints for an app built by `add`, without booting it.
async fn check_text(sources: ConfigSources, add: impl FnOnce(AppBuilder) -> AppBuilder) -> String {
    let out = Arc::new(Mutex::new(String::new()));
    let sink = out.clone();
    let probe = CommandModule(
        CommandSpec::new("probe:check", "config:check's report", move |ctx, _| {
            let sink = sink.clone();
            async move {
                let report =
                    navree_config::__private::check_report(&ctx, &CheckOptions::default()).await?;
                *sink.lock().unwrap() = report.render();
                Ok(())
            }
        })
        .offline(),
    );
    add(Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(probe))
    .run_with_args(vec!["probe:check".to_owned()])
    .await
    .unwrap();
    out.lock().unwrap().clone()
}

/// Section `storage` with the one disk `uploads`, of `disk`.
fn uploads(disk: serde_json::Value) -> ConfigSources {
    ConfigSources::new().set(
        "storage",
        json!({"default": "uploads", "disks": {"uploads": disk}}),
    )
}

#[tokio::test]
async fn a_registered_disk_driver_serves_a_disk_from_its_table() {
    let module = SandukModule::default();
    let (driver, seen) = (module.driver.clone(), module.seen.clone());
    let sources = || uploads(json!({"driver": "sanduk", "label": "Ludhiana invoices"}));
    let app = TestApp::boot(
        Navree::app()
            .module(ConfigModule::for_root(sources()))
            .module(StorageModule::from_config())
            .module(module),
    )
    .await;
    let disk = app.app.ctx().get::<Disk>().await.unwrap();
    assert_eq!((disk.name(), disk.driver()), ("uploads", "sanduk"));
    assert_eq!(
        *seen.lock().unwrap(),
        Some(("Ludhiana invoices".to_owned(), Some("uploads".to_owned())))
    );

    // Normalisation, traversal refusal, ranges, streams, uploads, listing,
    // temporary URLs and direct uploads, as for every built-in driver: such a
    // disk has no URL provider, so its links go through the app's routes.
    let subject = Subject {
        disk: (*disk).clone(),
        http: Http::app(app.router.clone()),
    };
    conformance::run(&subject).await;

    let paths = driver.paths();
    assert!(!paths.is_empty());
    for path in &paths {
        assert!(!path.split('/').any(|segment| segment == ".."), "{path:?}");
        assert!(!path.contains('\\'), "{path:?}");
        assert!(!path.chars().any(char::is_control), "{path:?}");
    }

    // The table is claimed by its driver: `config:check` reports no key of it.
    let text = check_text(sources(), |app| {
        app.module(StorageModule::from_config())
            .module(SandukModule::default())
    })
    .await;
    assert!(text.contains("config:check: ok ("), "{text}");
}

#[tokio::test]
async fn a_disk_given_in_code_hands_its_driver_its_table() {
    let module = SandukModule::default();
    let seen = module.seen.clone();
    // The options come from code (read from JSON here, as an app may), and
    // the configuration has no `storage` section.
    let options: StorageOptions = serde_json::from_value(json!({
        "default": "invoices",
        "disks": {"invoices": {"driver": "sanduk", "label": "Amritsar invoices"}},
    }))
    .unwrap();
    let _app = TestApp::boot(
        Navree::app()
            .module(ConfigModule::for_root(ConfigSources::new()))
            .module(StorageModule::for_root(options))
            .module(module),
    )
    .await;
    assert_eq!(
        *seen.lock().unwrap(),
        Some(("Amritsar invoices".to_owned(), Some("invoices".to_owned())))
    );
}

/// Registers the disk driver `sanduk` a second time.
struct SecondSandukModule;

impl Module for SecondSandukModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.disk_driver("sanduk", |_| async { Ok(Sanduk::default()) });
    }
}

#[tokio::test]
async fn a_disk_driver_registered_twice_fails_the_boot_also_when_storage_is_replaced() {
    let storage = Storage::new(&single_disk("memory", DiskConfig::Memory), SIGNING_KEY).unwrap();
    let error = boot_error(
        Navree::app()
            .module(ConfigModule::for_root(ConfigSources::new()))
            .module(StorageModule::from_config())
            .module(SandukModule::default())
            .module(SecondSandukModule)
            .override_value(Arc::new(storage)),
    )
    .await;
    assert!(
        error.contains("disk driver `sanduk` is registered by both `SandukModule` and `SecondSandukModule`; register it once"),
        "{error}"
    );
}

/// Registers the disk driver `darpan`, which would mirror another disk: its
/// factory asks the app for `Storage`, which is being built.
struct DarpanModule;

impl Module for DarpanModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.disk_driver("darpan", |ctx| async move {
            ctx.app().get::<Storage>().await?;
            Ok(Sanduk::default())
        });
    }
}

#[tokio::test]
async fn a_disk_driver_that_asks_for_storage_fails_the_boot_naming_the_cycle() {
    let booting = boot_error(
        Navree::app()
            .module(ConfigModule::for_root(uploads(json!({"driver": "darpan"}))))
            .module(StorageModule::from_config())
            .module(DarpanModule),
    );
    let error = tokio::time::timeout(Duration::from_secs(10), booting)
        .await
        .expect("the boot fails instead of waiting");
    assert!(
        error.contains("disk driver `darpan` (registered with m.disk_driver in `DarpanModule`) failed to start: provider dependency cycle: "),
        "{error}"
    );
    assert!(error.contains("Storage -> "), "{error}");
}

#[tokio::test]
async fn an_unregistered_disk_driver_fails_the_boot_naming_m_disk_driver() {
    let unregistered = "storage.disks.uploads.driver does not name a built-in disk driver or one a module registers with m.disk_driver (known: local, memory, s3, gcs, azure, sanduk)";
    let sources = || uploads(json!({"driver": "godown"}));
    let error = boot_error(
        Navree::app()
            .module(ConfigModule::for_root(sources()))
            .module(StorageModule::from_config())
            .module(SandukModule::default()),
    )
    .await;
    assert!(error.contains(unregistered), "{error}");
    let text = check_text(sources(), |app| {
        app.module(StorageModule::from_config())
            .module(SandukModule::default())
    })
    .await;
    assert!(text.contains(&format!("error    {unregistered}")), "{text}");

    // Without an app there is no driver to build.
    let options: StorageOptions = serde_json::from_value(json!({
        "default": "uploads",
        "disks": {"uploads": {"driver": "godown"}},
    }))
    .unwrap();
    let error = Storage::new(&options, SIGNING_KEY).unwrap_err().to_string();
    assert_eq!(
        error,
        "invalid storage options: disk `uploads`: the `godown` driver is not built in, and only StorageModule builds a driver a module registers, at boot"
    );
}
