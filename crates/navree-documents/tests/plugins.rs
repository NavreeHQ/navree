//! Document files a plugin embeds: under its namespace, two modules with one path, and a plugin
//! replacing a file of a module it imports until the app takes the path itself.
//!
//! A test binary of its own: the tests read the log, and `tracing` caches per-callsite interest
//! process-wide, so events first hit by tests that install no subscriber could be skipped here
//! if they ran in this process.

use std::path::Path;
use std::sync::{Arc, Mutex};

use navree_documents::{Documents, DocumentsModule, ModuleBuilderDocumentsExt};
use navree_kernel::{Module, ModuleBuilder, ModuleId, PluginInfo};
use navree_testing::{TestApp, TestAppBuilder};

/// Captures formatted tracing output written while the guard is alive (on this thread: use a
/// current-thread runtime).
#[derive(Clone, Default)]
struct LogCapture(Arc<Mutex<Vec<u8>>>);

impl LogCapture {
    fn install(&self) -> tracing::subscriber::DefaultGuard {
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .with_ansi(false)
            .with_writer(self.clone())
            .finish();
        tracing::subscriber::set_default(subscriber)
    }

    fn lines(&self) -> Vec<String> {
        String::from_utf8_lossy(&self.0.lock().unwrap())
            .lines()
            .map(str::to_owned)
            .collect()
    }

    /// How many captured lines of `level` contain `text`.
    fn count(&self, level: &str, text: &str) -> usize {
        self.lines()
            .iter()
            .filter(|line| line.contains(level) && line.contains(text))
            .count()
    }
}

impl std::io::Write for LogCapture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogCapture {
    type Writer = LogCapture;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

fn announcements() -> PluginInfo {
    PluginInfo::new(
        "navree-plugin-announcements",
        env!("CARGO_CRATE_NAME"),
        "announcements",
        "1.0.0",
    )
}

fn notice_board() -> PluginInfo {
    PluginInfo::new(
        "navree-plugin-notice-board",
        env!("CARGO_CRATE_NAME"),
        "notice_board",
        "1.0.0",
    )
}

/// A test app with the documents module, reading templates from the folder `dir`.
fn builder(dir: &Path) -> TestAppBuilder {
    TestApp::builder()
        .config("documents.templates", dir.to_string_lossy().into_owned())
        .module(DocumentsModule::from_config())
}

/// An empty template folder.
fn folder() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap()
}

/// What the app reads at `path` of its document templates.
async fn read(app: &TestApp, path: &str) -> Option<String> {
    app.get::<Documents>()
        .await
        .templates()
        .read(path)
        .unwrap()
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

/// `dir` as the boot's messages show it.
fn shown(dir: &Path) -> String {
    dir.to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_owned()
}

/// An app module with an invoice.
struct CrmDocuments;
impl Module for CrmDocuments {
    fn register(&self, m: &mut ModuleBuilder) {
        m.document_files([("invoices/standard.html", "the CRM's invoice")]);
    }
}

/// Another app module with the same path and other contents.
struct BoardDocuments;
impl Module for BoardDocuments {
    fn register(&self, m: &mut ModuleBuilder) {
        m.document_files([("invoices/standard.html", "the board's invoice")]);
    }
}

/// An app module with the CRM's path and the CRM's contents.
struct TwinDocuments;
impl Module for TwinDocuments {
    fn register(&self, m: &mut ModuleBuilder) {
        m.document_files([("invoices/standard.html", "the CRM's invoice")]);
    }
}

/// An app module that imports the CRM's and embeds the path with other contents.
struct AppReplacesCrm;
impl Module for AppReplacesCrm {
    fn register(&self, m: &mut ModuleBuilder) {
        m.import(CrmDocuments)
            .document_files([("invoices/standard.html", "the app's replacement")]);
    }
}

/// The plugin, embedding `path`.
struct PluginDocuments(&'static str);
impl Module for PluginDocuments {
    fn register(&self, m: &mut ModuleBuilder) {
        m.plugin(announcements())
            .document_files([(self.0, "the plugin's invoice")]);
    }
}

/// The plugin, importing the CRM's module and embedding `path`.
struct PluginAfterImport(&'static str);
impl Module for PluginAfterImport {
    fn register(&self, m: &mut ModuleBuilder) {
        m.plugin(announcements())
            .import(CrmDocuments)
            .document_files([(self.0, "the plugin's invoice")]);
    }
}

/// Another plugin, importing the first plugin's module and embedding its path again.
struct NoticeBoardDocuments;
impl Module for NoticeBoardDocuments {
    fn register(&self, m: &mut ModuleBuilder) {
        m.plugin(notice_board())
            .import(PluginDocuments("announcements/invoice.html"))
            .document_files([("announcements/invoice.html", "the notice board's invoice")]);
    }
}

/// A module of the app's own crate embedding the invoice with `contents`, for registering
/// after the plugin's module. This file is one crate, and the plugin names it, so the module
/// takes the id of a type of another crate, as a module of the app's own crate would have.
struct AppDocuments(&'static str);
impl Module for AppDocuments {
    fn id(&self) -> ModuleId {
        ModuleId::named::<String>("AppDocuments")
    }

    fn register(&self, m: &mut ModuleBuilder) {
        m.document_files([("invoices/standard.html", self.0)]);
    }
}

/// A module of the plugin's own crate that does not call `m.plugin`, and embeds the plugin's
/// contents again.
struct PluginHelperDocuments;
impl Module for PluginHelperDocuments {
    fn register(&self, m: &mut ModuleBuilder) {
        m.document_files([("invoices/standard.html", "the plugin's invoice")]);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_plugins_document_file_outside_its_namespace_fails_the_boot() {
    let dir = folder();
    let paths = [
        "invoices/standard.html",
        "images/seal.png",
        "announcements.html",
    ];
    for path in paths {
        let Err(error) = builder(dir.path())
            .module(PluginDocuments(path))
            .try_build()
            .await
        else {
            panic!("fails")
        };
        let error = error.to_string();
        assert!(
            error.contains(&format!(
                "plugin `navree-plugin-announcements` embeds the document file `{path}` in `PluginDocuments`, which is not under its namespace: put it under `announcements/`"
            )),
            "{path}: {error}"
        );
    }

    // Under its namespace it boots, and the file is there.
    let app = builder(dir.path())
        .module(PluginDocuments("announcements/invoice.html"))
        .build()
        .await;
    assert_eq!(
        read(&app, "announcements/invoice.html").await.as_deref(),
        Some("the plugin's invoice")
    );
    app.shutdown().await.unwrap();

    // The exact path of a module it imports is the replacement rule's: its error, not this one.
    let Err(error) = builder(dir.path())
        .module(PluginAfterImport("invoices/standard.html"))
        .try_build()
        .await
    else {
        panic!("fails")
    };
    let error = error.to_string();
    assert!(
        error.contains(
            "document file `invoices/standard.html` of `CrmDocuments` cannot be replaced by `PluginAfterImport`, a module of the plugin `navree-plugin-announcements`"
        ),
        "{error}"
    );
    assert!(
        !error.contains("which is not under its namespace"),
        "{error}"
    );

    // The same paths from a module of the app boot, as in 1.2.
    let app = builder(dir.path()).module(CrmDocuments).build().await;
    assert_eq!(
        read(&app, "invoices/standard.html").await.as_deref(),
        Some("the CRM's invoice")
    );
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn a_duplicate_document_file_warns_between_modules_and_fails_with_a_plugin() {
    let capture = LogCapture::default();
    let _guard = capture.install();
    let dir = folder();

    // One module per path says nothing, and two modules with the same contents are one file.
    let app = builder(dir.path())
        .module(CrmDocuments)
        .module(TwinDocuments)
        .build()
        .await;
    assert!(
        capture
            .lines()
            .iter()
            .all(|line| !line.contains("is embedded by both")),
    );
    app.shutdown().await.unwrap();

    // Two of today's modules: the later module's file is used, with a warning.
    let app = builder(dir.path())
        .module(CrmDocuments)
        .module(BoardDocuments)
        .build()
        .await;
    assert_eq!(
        capture.count(
            "WARN",
            "document file `invoices/standard.html` is embedded by both `CrmDocuments` and `BoardDocuments`; `BoardDocuments`'s is used, and this becomes an error in 2.0"
        ),
        1
    );
    assert_eq!(
        read(&app, "invoices/standard.html").await.as_deref(),
        Some("the board's invoice")
    );
    app.shutdown().await.unwrap();

    // A plugin on one side: the boot fails.
    let Err(error) = builder(dir.path())
        .module(CrmDocuments)
        .module(PluginDocuments("invoices/standard.html"))
        .try_build()
        .await
    else {
        panic!("fails")
    };
    assert!(
        error.to_string().contains(
            "document file `invoices/standard.html` is embedded by both `CrmDocuments` and `PluginDocuments`; a plugin's document files are under its namespace"
        ),
        "{error}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_plugin_replacing_an_app_document_file_fails_until_the_app_takes_it() {
    let capture = LogCapture::default();
    let _guard = capture.install();
    let dir = folder();
    let replaced = "document file `invoices/standard.html` of `CrmDocuments` is replaced by `PluginAfterImport`'s, a module of the plugin `navree-plugin-announcements`";

    // Nothing takes the path: the plugin's version would be the one used, and the boot fails,
    // telling the folder to use.
    let Err(error) = builder(dir.path())
        .module(PluginAfterImport("invoices/standard.html"))
        .try_build()
        .await
    else {
        panic!("fails")
    };
    assert!(
        error.to_string().contains(&format!(
            "document file `invoices/standard.html` of `CrmDocuments` cannot be replaced by `PluginAfterImport`, a module of the plugin `navree-plugin-announcements`; to use the plugin's version, put it in a documents.templates folder ({}/invoices/standard.html) or embed it from a module of the app",
            shown(dir.path())
        )),
        "{error}"
    );

    // A file at that path in a documents.templates folder wins, and clears it.
    let with_file = folder();
    std::fs::create_dir_all(with_file.path().join("invoices")).unwrap();
    std::fs::write(
        with_file.path().join("invoices/standard.html"),
        "the app's own file",
    )
    .unwrap();
    let app = builder(with_file.path())
        .module(PluginAfterImport("invoices/standard.html"))
        .build()
        .await;
    assert_eq!(
        read(&app, "invoices/standard.html").await.as_deref(),
        Some("the app's own file")
    );
    assert_eq!(
        capture.count(
            "INFO",
            &format!("{replaced}; the file `invoices/standard.html` in a documents.templates folder is used")
        ),
        1
    );
    app.shutdown().await.unwrap();

    // A module of the app that embeds the path after the plugin's module, with contents of its
    // own or with the plugin's, clears it too.
    for (contents, used) in [("the app's invoice", 1), ("the plugin's invoice", 2)] {
        let app = builder(dir.path())
            .module(PluginAfterImport("invoices/standard.html"))
            .module(AppDocuments(contents))
            .build()
            .await;
        assert_eq!(
            read(&app, "invoices/standard.html").await.as_deref(),
            Some(contents)
        );
        assert_eq!(
            capture.count("INFO", &format!("{replaced}; `AppDocuments`'s is used")),
            used
        );
        app.shutdown().await.unwrap();
    }

    // A module of the plugin's own crate that names no plugin is not the app.
    let Err(error) = builder(dir.path())
        .module(PluginAfterImport("invoices/standard.html"))
        .module(PluginHelperDocuments)
        .try_build()
        .await
    else {
        panic!("fails")
    };
    assert!(
        error
            .to_string()
            .contains("document file `invoices/standard.html` of `CrmDocuments` cannot be replaced by `PluginAfterImport`"),
        "{error}"
    );

    // An app module that imports the CRM's and replaces its file is said at info, as in 1.2.
    let app = builder(dir.path()).module(AppReplacesCrm).build().await;
    assert_eq!(
        read(&app, "invoices/standard.html").await.as_deref(),
        Some("the app's replacement")
    );
    assert_eq!(
        capture.count(
            "INFO",
            "document file `invoices/standard.html` of `CrmDocuments` is replaced by `AppReplacesCrm`'s"
        ),
        1
    );
    app.shutdown().await.unwrap();

    // A plugin replacing another plugin's file it imports is a warning.
    let app = builder(dir.path())
        .module(NoticeBoardDocuments)
        .build()
        .await;
    assert_eq!(
        read(&app, "announcements/invoice.html").await.as_deref(),
        Some("the notice board's invoice")
    );
    assert_eq!(
        capture.count(
            "WARN",
            "document file `announcements/invoice.html` of `PluginDocuments` is replaced by `NoticeBoardDocuments`'s, a module of the plugin `navree-plugin-notice-board`"
        ),
        1
    );
    app.shutdown().await.unwrap();
}
