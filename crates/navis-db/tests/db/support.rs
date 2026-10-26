//! Helpers shared by the navis-db integration tests.

#![allow(dead_code)]

use navis_db::DbOptions;
use tempfile::TempDir;

/// A fresh temporary SQLite database file. Keep the guard alive for the
/// duration of the test; dropping it deletes the directory.
pub struct TempDb {
    pub dir: TempDir,
    pub url: String,
}

impl TempDb {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("test.db");
        let url = format!("sqlite://{}?mode=rwc", path.display());
        Self { dir, url }
    }

    /// Options pointing at this database.
    pub fn options(&self) -> DbOptions {
        DbOptions {
            url: self.url.clone(),
            ..DbOptions::default()
        }
    }
}

/// Captures formatted tracing output written while the guard is alive (on
/// this thread; use with current-thread tokio tests).
#[derive(Clone, Default)]
pub struct LogCapture(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl LogCapture {
    pub fn install(&self) -> tracing::subscriber::DefaultGuard {
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .with_ansi(false)
            .with_writer(self.clone())
            .finish();
        tracing::subscriber::set_default(subscriber)
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
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
