//! One job name registered by two modules: a warning between modules of an
//! app, an error when a plugin is one of them.
//!
//! A test binary of its own: the test reads the log, and `tracing` caches
//! per-callsite interest process-wide, so events first hit by tests that
//! install no subscriber could be skipped here if they ran in this process.

use std::sync::{Arc, Mutex};

use navree_contracts::{BoxError, Job, JobContext, ModuleBuilderJobsExt};
use navree_kernel::{Module, ModuleBuilder, Navree, PluginInfo};
use navree_queue::{QueueDriver, QueueModule, QueueOptions};
use serde::{Deserialize, Serialize};

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

/// A job that sends the notices of the day.
#[derive(Serialize, Deserialize)]
struct Digest {
    ward: String,
}

impl Job for Digest {
    const NAME: &'static str = "notices.digest";

    async fn handle(self, _ctx: JobContext) -> Result<(), BoxError> {
        Ok(())
    }
}

/// An app module that registers the job.
struct CrmModule;
impl Module for CrmModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.job::<Digest>();
    }
}

/// Another app module that registers it.
struct NoticeBoardModule;
impl Module for NoticeBoardModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.job::<Digest>();
    }
}

/// A module that registers the job twice: one module, so one registration.
struct TwiceModule;
impl Module for TwiceModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.job::<Digest>().job::<Digest>();
    }
}

/// A plugin's module that registers it.
struct AnnouncementsModule;
impl Module for AnnouncementsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.plugin(PluginInfo::new(
            "navree-plugin-notices",
            env!("CARGO_CRATE_NAME"),
            "notices",
            "1.0.0",
        ))
        .job::<Digest>();
    }
}

/// The queue on its memory driver.
fn memory() -> impl Module {
    QueueModule::for_root(QueueOptions {
        driver: QueueDriver::Memory,
        ..QueueOptions::default()
    })
}

#[tokio::test(flavor = "current_thread")]
async fn a_duplicate_job_warns_between_modules_and_fails_with_a_plugin() {
    let capture = LogCapture::default();
    let _guard = capture.install();

    // One module, even registering the name twice: nothing is said.
    let app = Navree::app()
        .module(memory())
        .module(TwiceModule)
        .boot()
        .await
        .expect("boots");
    assert!(
        capture
            .lines()
            .iter()
            .all(|line| !line.contains("is registered by both")),
        "one module, even twice"
    );
    app.shutdown().await.unwrap();

    // Two of today's modules: the first module's handler runs, with a warning.
    let app = Navree::app()
        .module(memory())
        .module(CrmModule)
        .module(NoticeBoardModule)
        .boot()
        .await
        .expect("a warning only");
    assert!(capture.lines().iter().any(|line| line.contains("WARN")
        && line.contains(
            "job `notices.digest` is registered by both `CrmModule` and `NoticeBoardModule`; `CrmModule`'s runs, and this becomes an error in 2.0"
        )));
    app.shutdown().await.unwrap();

    // A plugin on one side: the boot fails.
    let error = Navree::app()
        .module(memory())
        .module(CrmModule)
        .module(AnnouncementsModule)
        .boot()
        .await
        .expect_err("fails")
        .to_string();
    assert!(
        error.contains(
            "job `notices.digest` is registered by both `CrmModule` and `AnnouncementsModule`; give one another name"
        ),
        "{error}"
    );
}
