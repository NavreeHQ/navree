//! Two types reading one configuration section: a warning between modules of
//! an app, an error when a plugin is one of them, at boot and in
//! `config:check`.
//!
//! A test binary of its own: the test reads the log, and `tracing` caches
//! per-callsite interest process-wide, so events first hit by tests that
//! install no subscriber could be skipped here if they ran in this process.

use std::sync::{Arc, Mutex};

use navree_config::{ConfigModule, ConfigSection, ConfigSources, ModuleBuilderConfigExt};
use navree_kernel::{AppBuilder, Module, ModuleBuilder, Navree, PluginInfo};
use serde::Deserialize;

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

/// The notices section as the CRM reads it.
#[derive(Default, Deserialize)]
#[serde(default)]
#[allow(dead_code)]
struct NoticesA {
    title: String,
}

impl ConfigSection for NoticesA {
    const KEY: &'static str = "notices";
}

/// The same section as another module reads it.
#[derive(Default, Deserialize)]
#[serde(default)]
#[allow(dead_code)]
struct NoticesB {
    per_page: u32,
}

impl ConfigSection for NoticesB {
    const KEY: &'static str = "notices";
}

/// An app module that reads the section as `NoticesA`.
struct CrmModule;
impl Module for CrmModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.config::<NoticesA>();
    }
}

/// An app module that reads the section as `NoticesA` too: one type, so one
/// section.
struct TwinModule;
impl Module for TwinModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.config::<NoticesA>();
    }
}

/// Another app module that reads it as `NoticesB`.
struct NoticeBoardModule;
impl Module for NoticeBoardModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.config::<NoticesB>();
    }
}

/// A plugin's module that reads it as `NoticesB`.
struct AnnouncementsModule;
impl Module for AnnouncementsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.plugin(PluginInfo::new(
            "navree-plugin-notices",
            env!("CARGO_CRATE_NAME"),
            "notices",
            "1.0.0",
        ))
        .config::<NoticesB>();
    }
}

/// The configuration module, the CRM and `second`.
fn app(second: impl Module) -> AppBuilder {
    Navree::app()
        .module(ConfigModule::for_root(ConfigSources::new()))
        .module(CrmModule)
        .module(second)
}

#[tokio::test(flavor = "current_thread")]
async fn two_config_types_on_one_key_warn_between_modules_and_fail_with_a_plugin() {
    let capture = LogCapture::default();
    let _guard = capture.install();

    // One type, in one module or in two: nothing is said.
    let booted = Navree::app()
        .module(ConfigModule::for_root(ConfigSources::new()))
        .module(CrmModule)
        .boot()
        .await
        .expect("boots");
    booted.shutdown().await.unwrap();
    let booted = app(TwinModule).boot().await.expect("boots");
    booted.shutdown().await.unwrap();
    assert!(
        capture
            .lines()
            .iter()
            .all(|line| !line.contains("is read by two types"))
    );

    // Two of today's modules: a warning, and the check still passes.
    let booted = app(NoticeBoardModule).boot().await.expect("a warning only");
    assert!(capture.lines().iter().any(|line| line.contains("WARN")
        && line.contains(
            "configuration section `notices` is read by two types: `collisions::NoticesA` (CrmModule) and `collisions::NoticesB` (NoticeBoardModule); this becomes an error in 2.0"
        )));
    booted.shutdown().await.unwrap();
    app(NoticeBoardModule)
        .run_with_args(vec!["config:check".to_owned()])
        .await
        .expect("a warning does not fail the check");

    // A plugin on one side: the boot and the check fail.
    let error = app(AnnouncementsModule)
        .boot()
        .await
        .expect_err("fails")
        .to_string();
    assert!(
        error.contains(
            "configuration section `notices` is read by two types: `collisions::NoticesA` (CrmModule) and `collisions::NoticesB` (AnnouncementsModule); a section belongs to one type"
        ),
        "{error}"
    );
    assert_eq!(
        app(AnnouncementsModule)
            .run_with_args(vec!["config:check".to_owned()])
            .await
            .unwrap_err()
            .to_string(),
        "config:check found 1 problem(s)"
    );
}
