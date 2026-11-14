//! Two modules that embed one template name: a warning between modules of an
//! app, an error when a plugin is one of them, and a module that replaces a
//! template of a module it imports (said at info for an app's module, a warning
//! for a plugin's module replacing another plugin's template, an error for a
//! plugin's module replacing any other).
//!
//! A test binary of its own: the tests read the log, and `tracing` caches
//! per-callsite interest process-wide, so events first hit by tests that
//! install no subscriber could be skipped here if they ran in this process.

// This binary uses only `options` of the helpers the view tests share.
#[allow(dead_code)]
#[path = "view/support.rs"]
mod support;

use std::sync::{Arc, Mutex};

use navree_kernel::{App, BootErrors, Module, ModuleBuilder, ModuleId, Navree, PluginInfo};
use navree_view::{
    ModuleBuilderPluginTemplatesExt, ModuleBuilderViewExt, TemplateDir, ViewModule, Views,
};
use serde_json::json;

use crate::support::options;

/// Captures formatted tracing output written while the guard is alive (on
/// this thread: use a current-thread runtime).
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

/// A boxed module, so modules chosen by a test reach `AppBuilder::module` in order.
struct Boxed(Box<dyn Module>);

impl Module for Boxed {
    fn id(&self) -> ModuleId {
        self.0.id()
    }

    fn name(&self) -> &'static str {
        self.0.name()
    }

    fn is_configured(&self) -> bool {
        self.0.is_configured()
    }

    fn register(&self, m: &mut ModuleBuilder) {
        self.0.register(m);
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

/// An app module with a welcome mail.
struct CrmModule;
impl Module for CrmModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.templates(&[("mail/welcome.html", "Welcome from the CRM")]);
    }
}

/// Another app module with a welcome mail of its own.
struct NoticeBoardModule;
impl Module for NoticeBoardModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.templates(&[("mail/welcome.html", "Welcome from the notice board")]);
    }
}

const ANNOUNCEMENT_VIEWS: &[(&str, &str)] = &[("list.html", "the plugin's list")];

/// A plugin with a welcome mail of its own, outside its namespace, and a list
/// under it.
struct AnnouncementsModule;
impl Module for AnnouncementsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.plugin(announcements())
            .templates(&[("mail/welcome.html", "Welcome from Harpreet Sandhu")])
            .plugin_templates(TemplateDir::from(ANNOUNCEMENT_VIEWS));
    }
}

/// An app module that imports the plugin and replaces its list.
struct ShellModule;
impl Module for ShellModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.import(AnnouncementsModule)
            .templates(&[("announcements/list.html", "the app's list")]);
    }
}

/// A module of a plugin that imports the CRM and replaces its welcome mail.
struct PluginShellModule;
impl Module for PluginShellModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.plugin(announcements())
            .import(CrmModule)
            .templates(&[("mail/welcome.html", "Welcome from the plugin's shell")]);
    }
}

fn notice_board() -> PluginInfo {
    PluginInfo::new(
        "navree-plugin-notice-board",
        env!("CARGO_CRATE_NAME"),
        "notice_board",
        "1.0.0",
    )
}

/// A module of another plugin that imports the announcements plugin and
/// replaces its list.
struct NoticeBoardPlugin;
impl Module for NoticeBoardPlugin {
    fn register(&self, m: &mut ModuleBuilder) {
        m.plugin(notice_board())
            .import(AnnouncementsModule)
            .templates(&[("announcements/list.html", "the notice board's list")]);
    }
}

/// `ViewModule::for_root(options(&[]))` and `modules`, booted.
async fn boot(modules: Vec<Box<dyn Module>>) -> Result<App, BootErrors> {
    let mut app = Navree::app().module(ViewModule::for_root(options(&[])));
    for module in modules {
        app = app.module(Boxed(module));
    }
    app.boot().await
}

/// What `name` renders as in `app`.
async fn render(app: &App, name: &str) -> String {
    app.ctx()
        .get::<Views>()
        .await
        .unwrap()
        .render(name, json!({}))
        .unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn a_duplicate_template_warns_between_modules_and_fails_with_a_plugin_but_an_importer_may_replace()
 {
    let capture = LogCapture::default();
    let _guard = capture.install();

    // One module per name: nothing is said.
    let app = boot(vec![Box::new(CrmModule)]).await.expect("boots");
    assert!(
        capture
            .lines()
            .iter()
            .all(|line| !line.contains("embedded template"))
    );
    app.shutdown().await.unwrap();

    // Two of today's modules: the later module's template is used, with a warning.
    let app = boot(vec![Box::new(CrmModule), Box::new(NoticeBoardModule)])
        .await
        .expect("boots");
    assert_eq!(
        render(&app, "mail/welcome").await,
        "Welcome from the notice board"
    );
    assert!(capture.lines().iter().any(|line| line.contains("WARN")
        && line.contains(
            "embedded template `mail/welcome.html` is registered by both `CrmModule` and `NoticeBoardModule`; `NoticeBoardModule`'s is used, and this becomes an error in 2.0"
        )));
    app.shutdown().await.unwrap();

    // A plugin on one side: the boot fails.
    let error = boot(vec![Box::new(CrmModule), Box::new(AnnouncementsModule)])
        .await
        .expect_err("fails")
        .to_string();
    assert!(
        error.contains(
            "embedded template `mail/welcome.html` is registered by both `CrmModule` and `AnnouncementsModule`; a plugin's templates are under its namespace"
        ),
        "{error}"
    );

    // A module may replace a template of a module it imports: said at info, never an error.
    let app = boot(vec![Box::new(ShellModule)]).await.expect("boots");
    assert_eq!(render(&app, "announcements/list").await, "the app's list");
    assert!(capture.lines().iter().any(|line| line.contains("INFO")
        && line.contains(
            "embedded template `announcements/list.html` of `AnnouncementsModule` is replaced by `ShellModule`'s"
        )));
    assert!(
        capture
            .lines()
            .iter()
            .all(|line| !line.contains("WARN") || !line.contains("announcements/list.html")),
        "an app module replacing a plugin's template is not a warning"
    );
    app.shutdown().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn a_plugin_replacing_a_template_of_an_app_module_it_imports_fails_the_boot() {
    let capture = LogCapture::default();
    let _guard = capture.install();

    let error = boot(vec![Box::new(PluginShellModule)])
        .await
        .expect_err("fails")
        .to_string();
    assert!(
        error.contains(
            "embedded template `mail/welcome.html` of `CrmModule` cannot be replaced by `PluginShellModule`, a module of the plugin `navree-plugin-announcements`; to use the plugin's version, copy it to resources/views/mail/welcome.html or register it from a module of the app"
        ),
        "{error}"
    );
    assert!(
        capture
            .lines()
            .iter()
            .all(|line| !line.contains("is replaced by")),
        "an error, not a log line"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_plugin_replacing_another_plugins_template_it_imports_is_warned_about() {
    let capture = LogCapture::default();
    let _guard = capture.install();

    let app = boot(vec![Box::new(NoticeBoardPlugin)])
        .await
        .expect("boots");
    assert_eq!(
        render(&app, "announcements/list").await,
        "the notice board's list"
    );
    assert!(capture.lines().iter().any(|line| line.contains("WARN")
        && line.contains(
            "embedded template `announcements/list.html` of `AnnouncementsModule` is replaced by `NoticeBoardPlugin`'s, a module of the plugin `navree-plugin-notice-board`"
        )));
    assert!(
        capture
            .lines()
            .iter()
            .all(|line| !(line.contains("INFO") && line.contains("is replaced by"))),
        "the warning replaces the info line"
    );
    app.shutdown().await.unwrap();
}
