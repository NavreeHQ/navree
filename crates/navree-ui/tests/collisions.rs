//! Two modules that give one static file name to two different files: a warning between the
//! modules of an app, a boot error when a plugin is one of them, and a plugin's file that is
//! not under its namespace.
//!
//! A test binary of its own: the tests read the log, and `tracing` caches per-callsite
//! interest process-wide, so events first hit by tests that install no subscriber could be
//! skipped here if they ran in this process.

use std::sync::{Arc, Mutex};

use navree_http::HttpModule;
use navree_kernel::{Module, ModuleBuilder};
use navree_plugin_announcements::{
    AnnouncementsModule, LooseClockModule, LooseModule, NoticeClockModule, ScriptHelper,
    SheetHelper,
};
use navree_testing::{TestApp, TestAppBuilder};
use navree_ui::{AppSheet, IslandScript, UiModule};

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

fn builder() -> TestAppBuilder {
    TestApp::builder()
        .config("http.per_ip_rate_limit", serde_json::Value::Null)
        .module(HttpModule::for_root(Default::default()))
}

/// The announcements plugin. A test binary is one crate, so the plugin's crate is this module,
/// and the app's modules at the top of the file are in no crate it names.
mod navree_plugin_announcements {
    use navree_kernel::{Module, ModuleBuilder, PluginInfo};
    use navree_ui::{AppSheet, IslandScript};

    use super::{LOOSE_BANNER, LOOSE_CLOCK, NOTICE_BANNER, NOTICE_CLOCK};

    /// The plugin's info: its namespace is `announcements`.
    fn announcements() -> PluginInfo {
        PluginInfo::new(
            "navree-plugin-announcements",
            module_path!(),
            "announcements",
            "1.0.0",
        )
    }

    /// The plugin, with a sheet under its namespace.
    pub struct AnnouncementsModule;
    impl Module for AnnouncementsModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(announcements()).contribute(NOTICE_BANNER);
        }
    }

    /// A plugin with a sheet outside its namespace.
    pub struct LooseModule;
    impl Module for LooseModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(announcements()).contribute(LOOSE_BANNER);
        }
    }

    /// The plugin, with a script under its namespace.
    pub struct NoticeClockModule;
    impl Module for NoticeClockModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(announcements()).contribute(NOTICE_CLOCK);
        }
    }

    /// A plugin with a script outside its namespace.
    pub struct LooseClockModule;
    impl Module for LooseClockModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(announcements()).contribute(LOOSE_CLOCK);
        }
    }

    /// A module of the plugin's crate that names no plugin: it contributes the sheet it holds,
    /// and acts for the plugin all the same when the plugin is in the app.
    pub struct SheetHelper(pub AppSheet);
    impl Module for SheetHelper {
        fn register(&self, m: &mut ModuleBuilder) {
            m.contribute(self.0);
        }
    }

    /// A module of the plugin's crate that names no plugin: it contributes the script it holds.
    pub struct ScriptHelper(pub IslandScript);
    impl Module for ScriptHelper {
        fn register(&self, m: &mut ModuleBuilder) {
            m.contribute(self.0);
        }
    }
}

/// Two sheets of one name and different text.
static CRM_BRAND: AppSheet = AppSheet::new("brand", b".hero{color:var(--ink-1)}", 1024);
static BOARD_BRAND: AppSheet = AppSheet::new("brand", b".hero{color:var(--accent)}", 1024);

/// Three sheets named `banner` under the plugin's namespace, with different text, and one
/// with no namespace.
static NOTICE_BANNER: AppSheet =
    AppSheet::new("banner", b".notice{border-color:var(--accent)}", 1024)
        .namespace("announcements");
static SHELL_BANNER: AppSheet =
    AppSheet::new("banner", b".notice{border-color:var(--ink-1)}", 1024).namespace("announcements");
static OTHER_SHELL_BANNER: AppSheet =
    AppSheet::new("banner", b".notice{border-color:var(--ink-2)}", 1024).namespace("announcements");
static LOOSE_BANNER: AppSheet =
    AppSheet::new("banner", b".notice{border-color:var(--accent)}", 1024);

/// An app module with a sheet of its own.
struct CrmModule;
impl Module for CrmModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.contribute(CRM_BRAND);
    }
}

/// Another app module with a sheet called `brand`.
struct NoticeBoardModule;
impl Module for NoticeBoardModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.contribute(BOARD_BRAND);
    }
}

/// An app module that puts a sheet under the namespace `announcements`.
struct ShellModule;
impl Module for ShellModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.contribute(SHELL_BANNER);
    }
}

/// Another app module that puts a sheet under the namespace `announcements`.
struct OtherShellModule;
impl Module for OtherShellModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.contribute(OTHER_SHELL_BANNER);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_duplicate_static_file_warns_between_modules_and_fails_with_a_plugin() {
    let capture = LogCapture::default();
    let _guard = capture.install();
    let app = builder().module(UiModule).module(CrmModule).build().await;
    assert!(
        capture
            .lines()
            .iter()
            .all(|line| !line.contains("is contributed by both"))
    );
    app.shutdown().await.unwrap();

    // Two of today's modules: the first module's file is served, with a warning.
    let app = builder()
        .module(UiModule)
        .module(CrmModule)
        .module(NoticeBoardModule)
        .build()
        .await;
    let sheet = app
        .client()
        .get(&CRM_BRAND.url())
        .send()
        .await
        .assert_status(200);
    assert_eq!(sheet.text().as_bytes(), CRM_BRAND.bytes());
    assert!(capture.lines().iter().any(|line| line.contains("WARN")
        && line.contains(
            "static file `brand.css` is contributed by both `CrmModule` and `NoticeBoardModule`; `CrmModule`'s is served, and this becomes an error in 2.0"
        )));
    app.shutdown().await.unwrap();

    // A plugin on one side, or a namespaced file of a module that is no first-party plugin: the
    // boot fails.
    let Err(error) = builder()
        .module(UiModule)
        .module(AnnouncementsModule)
        .module(ShellModule)
        .try_build()
        .await
    else {
        panic!("fails")
    };
    assert!(
        error.to_string().contains(
            "static file `announcements.banner.css` is contributed by both `AnnouncementsModule` and `ShellModule`; name one under its plugin's namespace"
        ),
        "{error}"
    );
    let Err(error) = builder()
        .module(UiModule)
        .module(ShellModule)
        .module(OtherShellModule)
        .try_build()
        .await
    else {
        panic!("fails")
    };
    assert!(
        error.to_string().contains(
            "static file `announcements.banner.css` is contributed by both `ShellModule` and `OtherShellModule`; name one under its plugin's namespace"
        ),
        "{error}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_plugins_static_file_outside_its_namespace_fails_the_boot() {
    let capture = LogCapture::default();
    let _guard = capture.install();
    let Err(error) = builder()
        .module(UiModule)
        .module(LooseModule)
        .try_build()
        .await
    else {
        panic!("fails")
    };
    assert!(
        error.to_string().contains(
            "plugin `navree-plugin-announcements` contributes the static file `banner.css`, which is not under its namespace: build it with .namespace(\"announcements\")"
        ),
        "{error}"
    );
    builder()
        .module(UiModule)
        .module(AnnouncementsModule)
        .build()
        .await
        .shutdown()
        .await
        .unwrap();
}

/// Island scripts named `clock`, with different text: two of the app's, a plugin's under its
/// namespace, one of the app's under that namespace too, and a plugin's with no namespace.
static CRM_CLOCK: IslandScript = IslandScript::new(
    "clock",
    b"export function mount(body) { body.textContent = 'IST'; }",
    1024,
);
static BOARD_CLOCK: IslandScript = IslandScript::new(
    "clock",
    b"export function mount(body) { body.textContent = 'UTC'; }",
    1024,
);
static NOTICE_CLOCK: IslandScript = IslandScript::new(
    "clock",
    b"export function mount(body) { body.textContent = 'Ludhiana'; }",
    1024,
)
.namespace("announcements");
static SHELL_CLOCK: IslandScript = IslandScript::new(
    "clock",
    b"export function mount(body) { body.textContent = 'Agra'; }",
    1024,
)
.namespace("announcements");
static LOOSE_CLOCK: IslandScript = IslandScript::new(
    "clock",
    b"export function mount(body) { body.textContent = 'Amritsar'; }",
    1024,
);

/// An app module with an island script.
struct ClockModule;
impl Module for ClockModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.contribute(CRM_CLOCK);
    }
}

/// Another app module with an island script called `clock`.
struct OtherClockModule;
impl Module for OtherClockModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.contribute(BOARD_CLOCK);
    }
}

/// An app module that puts a script under the namespace `announcements`.
struct ShellClockModule;
impl Module for ShellClockModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.contribute(SHELL_CLOCK);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_duplicate_island_script_warns_between_modules_and_fails_with_a_plugin() {
    let capture = LogCapture::default();
    let _guard = capture.install();

    // One script per name says nothing, a plugin's under its namespace included.
    builder()
        .module(UiModule)
        .module(ClockModule)
        .module(NoticeClockModule)
        .build()
        .await
        .shutdown()
        .await
        .unwrap();
    assert!(
        capture
            .lines()
            .iter()
            .all(|line| !line.contains("is contributed by both"))
    );

    // Two of today's modules: the first module's script is served, with a warning.
    let app = builder()
        .module(UiModule)
        .module(ClockModule)
        .module(OtherClockModule)
        .build()
        .await;
    assert!(capture.lines().iter().any(|line| line.contains("WARN")
        && line.contains(
            "static file `clock.js` is contributed by both `ClockModule` and `OtherClockModule`; `ClockModule`'s is served, and this becomes an error in 2.0"
        )));
    app.shutdown().await.unwrap();

    // A plugin's script with no namespace fails the boot, and so does a module that is no
    // first-party plugin's putting a script under the plugin's namespace.
    let Err(error) = builder()
        .module(UiModule)
        .module(ClockModule)
        .module(LooseClockModule)
        .try_build()
        .await
    else {
        panic!("fails")
    };
    let error = error.to_string();
    assert!(
        error.contains(
            "plugin `navree-plugin-announcements` contributes the static file `clock.js`, which is not under its namespace: build it with .namespace(\"announcements\")"
        ),
        "{error}"
    );
    assert!(
        error.contains(
            "static file `clock.js` is contributed by both `ClockModule` and `LooseClockModule`; name one under its plugin's namespace"
        ),
        "{error}"
    );
    let Err(error) = builder()
        .module(UiModule)
        .module(NoticeClockModule)
        .module(ShellClockModule)
        .try_build()
        .await
    else {
        panic!("fails")
    };
    assert!(
        error.to_string().contains(
            "static file `announcements.clock.js` is contributed by both `NoticeClockModule` and `ShellClockModule`; name one under its plugin's namespace"
        ),
        "{error}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_module_of_a_plugins_crate_contributes_files_under_the_plugins_namespace_only() {
    let capture = LogCapture::default();
    let _guard = capture.install();

    // A module of the plugin's crate that names no plugin acts for the plugin: its sheet and its
    // script with no namespace fail the boot, as the plugin's own would.
    let Err(error) = builder()
        .module(UiModule)
        .module(AnnouncementsModule)
        .module(SheetHelper(LOOSE_BANNER))
        .try_build()
        .await
    else {
        panic!("fails")
    };
    assert!(
        error.to_string().contains(
            "plugin `navree-plugin-announcements` contributes the static file `banner.css`, which is not under its namespace: build it with .namespace(\"announcements\")"
        ),
        "{error}"
    );
    // Beside the app's `clock.js`, its script is an error, never the warning between two of the
    // app's modules after which the first module's file is served.
    let Err(error) = builder()
        .module(UiModule)
        .module(AnnouncementsModule)
        .module(ScriptHelper(LOOSE_CLOCK))
        .module(ClockModule)
        .try_build()
        .await
    else {
        panic!("fails")
    };
    let error = error.to_string();
    assert!(
        error.contains(
            "plugin `navree-plugin-announcements` contributes the static file `clock.js`, which is not under its namespace: build it with .namespace(\"announcements\")"
        ),
        "{error}"
    );
    assert!(
        error.contains(
            "static file `clock.js` is contributed by both `ScriptHelper` and `ClockModule`; name one under its plugin's namespace"
        ),
        "{error}"
    );

    // Under the plugin's namespace its files boot.
    builder()
        .module(UiModule)
        .module(AnnouncementsModule)
        .module(ScriptHelper(NOTICE_CLOCK))
        .build()
        .await
        .shutdown()
        .await
        .unwrap();
    // Without the plugin in the app, the module acts for no plugin, and its files are the app's.
    builder()
        .module(UiModule)
        .module(SheetHelper(LOOSE_BANNER))
        .build()
        .await
        .shutdown()
        .await
        .unwrap();
}
