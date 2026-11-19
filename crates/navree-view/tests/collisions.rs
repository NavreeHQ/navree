//! Two modules that embed one template name: a warning between modules of an
//! app, an error when a plugin is one of them, and a module that replaces a
//! template of a module it imports (said at info for an app's module, a warning
//! for a plugin's module replacing another plugin's template, an error for a
//! plugin's module replacing any other while its version is the one rendered:
//! not when the app has a file of that name in a template folder or a module of
//! the app registers the name after it).
//!
//! A test binary of its own: the tests read the log, and `tracing` caches
//! per-callsite interest process-wide, so events first hit by tests that
//! install no subscriber could be skipped here if they ran in this process.

// This binary uses only `options` and `views_dir` of the helpers the view tests share.
#[allow(dead_code)]
#[path = "view/support.rs"]
mod support;

use std::sync::{Arc, Mutex};

use navree_kernel::{App, BootErrors, Module, ModuleBuilder, ModuleId, Navree};
use navree_view::{ModuleBuilderViewExt, ViewModule, ViewOptions, Views};
use serde_json::json;

use crate::navree_plugin_announcements::{
    AnnouncementsModule, Ledger, LooseAnnouncementsModule, PluginHelperModule, PluginShellModule,
};
use crate::navree_plugin_notice_board::NoticeBoardPlugin;
use crate::support::{options, views_dir};

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

/// An app module that imports the plugin and replaces its list.
struct ShellModule;
impl Module for ShellModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.import(AnnouncementsModule)
            .templates(&[("announcements/list.html", "the app's list")]);
    }
}

/// A module of the app with the welcome mail `text`, for registering after a plugin.
struct AppWelcomeModule(&'static str);
impl Module for AppWelcomeModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.templates(&[("mail/welcome.html", self.0)]);
    }
}

/// The announcements plugin: its crate is this module, so the app's modules at the top of the
/// file are in no crate it names.
mod navree_plugin_announcements {
    use navree_kernel::{Module, ModuleBuilder, PluginInfo};
    use navree_view::{ModuleBuilderPluginTemplatesExt, ModuleBuilderViewExt, TemplateDir};

    use crate::CrmModule;

    /// The plugin's info.
    fn announcements() -> PluginInfo {
        PluginInfo::new(
            "navree-plugin-announcements",
            module_path!(),
            "announcements",
            "1.0.0",
        )
    }

    const ANNOUNCEMENT_VIEWS: &[(&str, &str)] = &[("list.html", "the plugin's list")];

    /// A plugin with a list under its namespace.
    pub struct AnnouncementsModule;
    impl Module for AnnouncementsModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(announcements())
                .plugin_templates(TemplateDir::from(ANNOUNCEMENT_VIEWS));
        }
    }

    /// A plugin that also registers a welcome mail of its own, outside its namespace.
    pub struct LooseAnnouncementsModule;
    impl Module for LooseAnnouncementsModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(announcements())
                .templates(&[("mail/welcome.html", "Welcome from Harpreet Sandhu")])
                .plugin_templates(TemplateDir::from(ANNOUNCEMENT_VIEWS));
        }
    }

    /// A module of a plugin that imports the CRM and replaces its welcome mail.
    pub struct PluginShellModule;
    impl Module for PluginShellModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(announcements())
                .import(CrmModule)
                .templates(&[("mail/welcome.html", "Welcome from the plugin's shell")]);
        }
    }

    /// A module of the plugin's own crate that does not call `m.plugin`, and registers the
    /// plugin's welcome mail again.
    pub struct PluginHelperModule;
    impl Module for PluginHelperModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.templates(&[("mail/welcome.html", "Welcome from the plugin's shell")]);
        }
    }

    /// A module of the plugin's own crate that does not call `m.plugin`, with a welcome mail
    /// of the plugin's ledger.
    pub struct Ledger;
    impl Module for Ledger {
        fn register(&self, m: &mut ModuleBuilder) {
            m.templates(&[("mail/welcome.html", "Welcome to the ledger of Ludhiana")]);
        }
    }
}

/// The notice board plugin, another plugin: its crate is this module.
mod navree_plugin_notice_board {
    use navree_kernel::{Module, ModuleBuilder, PluginInfo};
    use navree_view::ModuleBuilderViewExt;

    use crate::navree_plugin_announcements::AnnouncementsModule;

    /// The plugin's info.
    fn notice_board() -> PluginInfo {
        PluginInfo::new(
            "navree-plugin-notice-board",
            module_path!(),
            "notice_board",
            "1.0.0",
        )
    }

    /// A module of another plugin that imports the announcements plugin and
    /// replaces its list.
    pub struct NoticeBoardPlugin;
    impl Module for NoticeBoardPlugin {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(notice_board())
                .import(AnnouncementsModule)
                .templates(&[("announcements/list.html", "the notice board's list")]);
        }
    }
}

/// `ViewModule::for_root(options(&[]))` and `modules`, booted.
async fn boot(modules: Vec<Box<dyn Module>>) -> Result<App, BootErrors> {
    boot_with(options(&[]), modules).await
}

/// `ViewModule::for_root(view_options)` and `modules`, booted.
async fn boot_with(
    view_options: ViewOptions,
    modules: Vec<Box<dyn Module>>,
) -> Result<App, BootErrors> {
    let mut app = Navree::app().module(ViewModule::for_root(view_options));
    for module in modules {
        app = app.module(Boxed(module));
    }
    app.boot().await
}

/// How many captured `INFO` lines contain `text`.
fn info_lines(capture: &LogCapture, text: &str) -> usize {
    capture
        .lines()
        .iter()
        .filter(|line| line.contains("INFO") && line.contains(text))
        .count()
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
    let error = boot(vec![
        Box::new(CrmModule),
        Box::new(LooseAnnouncementsModule),
    ])
    .await
    .expect_err("fails")
    .to_string();
    assert!(
        error.contains(
            "embedded template `mail/welcome.html` is registered by both `CrmModule` and `LooseAnnouncementsModule`; a plugin's templates are under its namespace"
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

    let error = boot(vec![Box::new(CrmModule), Box::new(PluginShellModule)])
        .await
        .expect_err("fails")
        .to_string();
    // No template folder is configured: the text does not name one.
    assert!(
        error.contains(
            "embedded template `mail/welcome.html` of `CrmModule` cannot be replaced by `PluginShellModule`, a module of the plugin `navree-plugin-announcements`; to settle it, put the template you want, the plugin's or your own, in a template folder of the app, or register it from a module of the app that comes after `PluginShellModule`"
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

    // The text names where the file goes in the first configured template folder, whatever
    // `view.dirs` says, and not a folder this app does not have.
    let dir = views_dir(&[("mail/other.html", "Another mail")]);
    let second = views_dir(&[]);
    let error = boot_with(
        options(&[dir.path(), second.path()]),
        vec![Box::new(CrmModule), Box::new(PluginShellModule)],
    )
    .await
    .expect_err("fails")
    .to_string();
    let file = dir
        .path()
        .join("mail/welcome.html")
        .to_string_lossy()
        .replace('\\', "/");
    assert!(
        error.contains(&format!(
            "embedded template `mail/welcome.html` of `CrmModule` cannot be replaced by `PluginShellModule`, a module of the plugin `navree-plugin-announcements`; to settle it, put the template you want, the plugin's or your own, in a template folder of the app ({file}), or register it from a module of the app that comes after `PluginShellModule`"
        )),
        "{error}"
    );
    assert!(!error.contains("resources/views"), "{error}");
}

#[tokio::test(flavor = "current_thread")]
async fn a_plugin_replacing_an_app_template_boots_when_a_template_folder_has_the_file() {
    let capture = LogCapture::default();
    let _guard = capture.install();
    let used = "embedded template `mail/welcome.html` of `CrmModule` is replaced by `PluginShellModule`'s, a module of the plugin `navree-plugin-announcements`; the file `mail/welcome.html` in a template folder is used";

    // The app's own file of that name wins over every embedded template, with or without
    // reloading.
    let dir = views_dir(&[("mail/welcome.html", "Welcome from the app's own file")]);
    for (reload, lines) in [(false, 1), (true, 2)] {
        let app = boot_with(
            ViewOptions {
                dirs: vec![dir.path().to_path_buf()],
                reload,
            },
            vec![Box::new(CrmModule), Box::new(PluginShellModule)],
        )
        .await
        .expect("boots");
        assert_eq!(
            render(&app, "mail/welcome").await,
            "Welcome from the app's own file"
        );
        assert_eq!(info_lines(&capture, used), lines, "reload: {reload}");
        app.shutdown().await.unwrap();
    }

    // A file of another name does not settle it, and neither does one whose name differs only
    // in case, whatever the disk does with case: with or without reloading, the boot answers
    // the same.
    for files in [
        [("mail/welcome.txt", "Welcome from a text file")],
        [("Mail/Welcome.html", "Welcome from a file of another case")],
    ] {
        let other = views_dir(&files);
        for reload in [false, true] {
            let error = boot_with(
                ViewOptions {
                    dirs: vec![other.path().to_path_buf()],
                    reload,
                },
                vec![Box::new(CrmModule), Box::new(PluginShellModule)],
            )
            .await
            .expect_err("fails")
            .to_string();
            assert!(
                error.contains(
                    "embedded template `mail/welcome.html` of `CrmModule` cannot be replaced by `PluginShellModule`"
                ),
                "{files:?}, reload {reload}: {error}"
            );
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_plugin_replacing_an_app_template_boots_when_a_module_of_the_app_registers_it_after() {
    let capture = LogCapture::default();
    let _guard = capture.install();
    let used = "embedded template `mail/welcome.html` of `CrmModule` is replaced by `PluginShellModule`'s, a module of the plugin `navree-plugin-announcements`; `AppWelcomeModule`'s is used";

    // A module of the app registers the name after the plugin's module, with text of its own.
    let app = boot(vec![
        Box::new(CrmModule),
        Box::new(PluginShellModule),
        Box::new(AppWelcomeModule("Welcome from the app")),
    ])
    .await
    .expect("boots");
    assert_eq!(render(&app, "mail/welcome").await, "Welcome from the app");
    assert_eq!(info_lines(&capture, used), 1);
    app.shutdown().await.unwrap();

    // Or with the plugin's own text: the app takes the plugin's version, and it is the app's
    // registration that is rendered.
    let app = boot(vec![
        Box::new(CrmModule),
        Box::new(PluginShellModule),
        Box::new(AppWelcomeModule("Welcome from the plugin's shell")),
    ])
    .await
    .expect("boots");
    assert_eq!(
        render(&app, "mail/welcome").await,
        "Welcome from the plugin's shell"
    );
    assert_eq!(info_lines(&capture, used), 2);
    app.shutdown().await.unwrap();

    // Registered before the plugin's module, it does not: the plugin's version is the one
    // rendered.
    let error = boot(vec![
        Box::new(CrmModule),
        Box::new(AppWelcomeModule("Welcome from the app")),
        Box::new(PluginShellModule),
    ])
    .await
    .expect_err("fails")
    .to_string();
    assert!(
        error.contains("embedded template `mail/welcome.html`"),
        "{error}"
    );
    assert_eq!(info_lines(&capture, used), 2, "no new line");
}

#[tokio::test(flavor = "current_thread")]
async fn a_module_of_the_plugins_own_crate_that_names_no_plugin_does_not_settle_the_replacement() {
    let capture = LogCapture::default();
    let _guard = capture.install();

    // The helper registers the plugin's own text after the plugin's module, and names no
    // plugin: it is in the plugin's crate, so it is not a module of the app.
    let error = boot(vec![
        Box::new(CrmModule),
        Box::new(PluginShellModule),
        Box::new(PluginHelperModule),
    ])
    .await
    .expect_err("fails")
    .to_string();
    assert!(
        error.contains(
            "embedded template `mail/welcome.html` of `CrmModule` cannot be replaced by `PluginShellModule`, a module of the plugin `navree-plugin-announcements`"
        ),
        "{error}"
    );
    assert!(
        capture
            .lines()
            .iter()
            .all(|line| !line.contains("is replaced by")),
        "nothing was said at info"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_module_of_a_plugins_crate_that_names_no_plugin_is_held_to_the_template_rules() {
    let capture = LogCapture::default();
    let _guard = capture.install();

    // The ledger names no plugin, and is in the announcements plugin's crate: it acts for the
    // plugin, so a name it shares with the CRM fails the boot.
    let error = boot(vec![
        Box::new(CrmModule),
        Box::new(AnnouncementsModule),
        Box::new(Ledger),
    ])
    .await
    .expect_err("fails")
    .to_string();
    assert!(
        error.contains(
            "embedded template `mail/welcome.html` is registered by both `CrmModule` and `Ledger`; a plugin's templates are under its namespace"
        ),
        "{error}"
    );

    // The same registration from a module of the app, beside the same plugin, warns.
    let app = boot(vec![
        Box::new(CrmModule),
        Box::new(AnnouncementsModule),
        Box::new(NoticeBoardModule),
    ])
    .await
    .expect("boots");
    assert!(capture.lines().iter().any(|line| line.contains("WARN")
        && line.contains(
            "embedded template `mail/welcome.html` is registered by both `CrmModule` and `NoticeBoardModule`; `NoticeBoardModule`'s is used, and this becomes an error in 2.0"
        )));
    app.shutdown().await.unwrap();
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

/// A module of the app with the template `name`.
struct AppTemplate(&'static str);
impl Module for AppTemplate {
    fn register(&self, m: &mut ModuleBuilder) {
        m.templates(&[(self.0, "the app's template")]);
    }
}

/// Two modules of the app with a summary each, one text and one HTML.
struct AppSummaryText;
impl Module for AppSummaryText {
    fn register(&self, m: &mut ModuleBuilder) {
        m.templates(&[("reports/summary.txt", "the app's summary")]);
    }
}
struct AppSummaryHtml;
impl Module for AppSummaryHtml {
    fn register(&self, m: &mut ModuleBuilder) {
        m.templates(&[("reports/summary.html", "the app's summary")]);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_plugins_template_beside_an_app_template_of_the_same_stem_fails_the_boot() {
    let capture = LogCapture::default();
    let _guard = capture.install();
    let error = "plugin `navree-plugin-announcements`: its template `announcements/list.html` stands beside the app's `announcements/list.txt`, and a lookup of `announcements/list` or a mail of that name would take the plugin's; put `announcements/list.html` in a template folder too (plugin:publish announcements --tag views copies it) or rename the app's `announcements/list.txt`";

    // The app has `announcements/list.txt` as a file in a template folder, or embedded.
    let dir = views_dir(&[("announcements/list.txt", "the app's text list")]);
    let text = boot_with(options(&[dir.path()]), vec![Box::new(AnnouncementsModule)])
        .await
        .expect_err("fails")
        .to_string();
    assert!(text.contains(error), "{text}");
    let text = boot(vec![
        Box::new(AnnouncementsModule),
        Box::new(AppTemplate("announcements/list.txt")),
    ])
    .await
    .expect_err("fails")
    .to_string();
    assert!(text.contains(error), "{text}");
    // Whatever the order of the modules.
    let text = boot(vec![
        Box::new(AppTemplate("announcements/list.txt")),
        Box::new(AnnouncementsModule),
    ])
    .await
    .expect_err("fails")
    .to_string();
    assert!(text.contains(error), "{text}");

    // The app keeps its boot by having the plugin's name too, in a folder: that file wins.
    let both = views_dir(&[
        ("announcements/list.txt", "the app's text list"),
        ("announcements/list.html", "Meera Sharma's list"),
    ]);
    let app = boot_with(options(&[both.path()]), vec![Box::new(AnnouncementsModule)])
        .await
        .expect("boots");
    assert_eq!(
        render(&app, "announcements/list").await,
        "Meera Sharma's list"
    );
    app.shutdown().await.unwrap();

    // A template of the app that is no other extension of the same name is no problem.
    let unrelated = views_dir(&[("announcements/other.txt", "the app's other text")]);
    let app = boot_with(
        options(&[unrelated.path()]),
        vec![Box::new(AnnouncementsModule)],
    )
    .await
    .expect("boots");
    assert_eq!(
        render(&app, "announcements/list").await,
        "the plugin's list"
    );
    app.shutdown().await.unwrap();

    // The same pair from two modules of the app boots as in 1.2: the rule looks only at a
    // plugin that is not first-party.
    let app = boot(vec![Box::new(AppSummaryText), Box::new(AppSummaryHtml)])
        .await
        .expect("boots");
    assert_eq!(render(&app, "reports/summary").await, "the app's summary");
    app.shutdown().await.unwrap();
}
