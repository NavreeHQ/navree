//! Maintenance mode: while the maintenance file exists, every request gets 503.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{PoisonError, RwLock};
#[cfg(test)]
use std::time::Duration;

use http::StatusCode;
use http::header::{HeaderValue, RETRY_AFTER};
use serde::{Deserialize, Serialize};
use tokio::time::Instant;

use crate::AppError;

/// How long a file check is reused before the file is looked at again.
#[cfg(test)]
const CHECK_INTERVAL: Duration = Duration::from_millis(CHECK_INTERVAL_MS);
const CHECK_INTERVAL_MS: u64 = 1000;

/// The contents of the maintenance file (written by the `down` command).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaintenanceInfo {
    /// When maintenance started (Unix seconds).
    #[serde(default)]
    pub time: Option<i64>,
    /// Seconds clients should wait before retrying (`Retry-After`).
    #[serde(default)]
    pub retry: Option<u64>,
    /// The message shown as the error `message`.
    #[serde(default)]
    pub message: Option<String>,
}

impl MaintenanceInfo {
    /// The 503 response for requests made during maintenance.
    pub fn error(&self) -> AppError {
        let detail = self
            .message
            .clone()
            .unwrap_or_else(|| "The application is down for maintenance.".to_owned());
        let mut error = AppError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            crate::codes::MAINTENANCE,
            "Service Unavailable",
        )
        .with_detail(detail);
        if let Some(retry) = self.retry {
            error = error.with_header(RETRY_AFTER, HeaderValue::from(retry));
        }
        error
    }
}

/// Maintenance-mode state backed by a file, so `down` in one process takes
/// effect in a running server within a second.
///
/// Every request reads it, so while the app is up it takes no lock: the
/// state is an atomic flag, and the time of the last check an atomic
/// timestamp that one request at a time claims to re-read the file.
#[derive(Debug)]
pub struct MaintenanceMode {
    path: PathBuf,
    /// Whether the app is down (then `info` says how).
    down: AtomicBool,
    /// The contents of the maintenance file, while the app is down.
    info: RwLock<Option<MaintenanceInfo>>,
    /// When the state was last set, in milliseconds after `epoch`, plus one
    /// (0: never).
    checked_at: AtomicU64,
    epoch: Instant,
}

impl MaintenanceMode {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self {
            path,
            down: AtomicBool::new(false),
            info: RwLock::new(None),
            checked_at: AtomicU64::new(0),
            epoch: Instant::now(),
        }
    }

    /// The maintenance file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The current maintenance state; `None` when the app is up. Reads the
    /// file at most once per second (one request refreshes, others reuse).
    pub async fn current(&self) -> Option<MaintenanceInfo> {
        if self.claim_refresh() {
            let info = read_info(&self.path).await;
            self.set(info);
        }
        if !self.down.load(Ordering::Acquire) {
            return None;
        }
        self.info
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Puts the app into maintenance: writes the file and updates this process.
    pub async fn down(&self, info: MaintenanceInfo) -> io::Result<()> {
        if let Some(dir) = self.path.parent()
            && !dir.as_os_str().is_empty()
        {
            tokio::fs::create_dir_all(dir).await?;
        }
        let json = serde_json::to_vec_pretty(&info).map_err(io::Error::other)?;
        tokio::fs::write(&self.path, json).await?;
        self.set(Some(info));
        Ok(())
    }

    /// Brings the app back up: removes the file (if any) and updates this process.
    pub async fn up(&self) -> io::Result<()> {
        match tokio::fs::remove_file(&self.path).await {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        self.set(None);
        Ok(())
    }

    fn set(&self, info: Option<MaintenanceInfo>) {
        let down = info.is_some();
        *self.info.write().unwrap_or_else(PoisonError::into_inner) = info;
        self.down.store(down, Ordering::Release);
        self.checked_at.store(self.now(), Ordering::Release);
    }

    /// True for the one caller that should re-read the file now.
    fn claim_refresh(&self) -> bool {
        let now = self.now();
        let at = self.checked_at.load(Ordering::Acquire);
        if at != 0 && now.saturating_sub(at) < CHECK_INTERVAL_MS {
            return false;
        }
        self.checked_at
            .compare_exchange(at, now, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
    }

    /// Milliseconds since `epoch`, plus one (never 0).
    fn now(&self) -> u64 {
        let elapsed = Instant::now().duration_since(self.epoch).as_millis();
        u64::try_from(elapsed).unwrap_or(u64::MAX - 1) + 1
    }
}

/// Reads the maintenance file: `None` when absent; an unreadable or
/// malformed file still means "down" (with default info).
async fn read_info(path: &Path) -> Option<MaintenanceInfo> {
    match tokio::fs::read(path).await {
        Ok(bytes) => Some(serde_json::from_slice(&bytes).unwrap_or_default()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => {
            tracing::warn!(target: "navree::http", path = %path.display(), %error, "cannot read the maintenance file; treating the app as down");
            Some(MaintenanceInfo::default())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn down_and_up_toggle_the_file_and_the_state() {
        let dir = tempfile::tempdir().unwrap();
        let mode = MaintenanceMode::new(dir.path().join("framework").join("down"));
        assert_eq!(mode.current().await, None);

        let info = MaintenanceInfo {
            time: Some(1),
            retry: Some(60),
            message: Some("Upgrading".into()),
        };
        mode.down(info.clone()).await.unwrap();
        assert!(mode.path().exists());
        assert_eq!(mode.current().await, Some(info));

        mode.up().await.unwrap();
        assert!(!mode.path().exists());
        assert_eq!(mode.current().await, None);
        mode.up().await.unwrap();
    }

    #[tokio::test]
    async fn a_file_removed_by_another_process_is_noticed_within_a_second() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("down");
        std::fs::write(&path, r#"{"retry": 5}"#).unwrap();
        let mode = MaintenanceMode::new(path.clone());
        assert_eq!(mode.current().await.and_then(|info| info.retry), Some(5));

        std::fs::remove_file(&path).unwrap();
        assert!(
            mode.current().await.is_some(),
            "the last check is reused for up to a second"
        );
        tokio::time::sleep(CHECK_INTERVAL + Duration::from_millis(100)).await;
        assert_eq!(mode.current().await, None);
    }

    /// Many requests at once: one of them reads the file, all see the
    /// same state.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_requests_agree_on_the_state() {
        let dir = tempfile::tempdir().unwrap();
        let mode = std::sync::Arc::new(MaintenanceMode::new(dir.path().join("down")));
        let readers = |mode: &std::sync::Arc<MaintenanceMode>| {
            (0..64)
                .map(|_| {
                    let mode = mode.clone();
                    tokio::spawn(async move { mode.current().await })
                })
                .collect::<Vec<_>>()
        };
        for reader in readers(&mode) {
            assert_eq!(reader.await.unwrap(), None);
        }
        let info = MaintenanceInfo {
            message: Some("Upgrading".into()),
            ..MaintenanceInfo::default()
        };
        mode.down(info.clone()).await.unwrap();
        for reader in readers(&mode) {
            assert_eq!(reader.await.unwrap(), Some(info.clone()));
        }
        mode.up().await.unwrap();
        for reader in readers(&mode) {
            assert_eq!(reader.await.unwrap(), None);
        }
    }

    #[tokio::test]
    async fn a_file_written_by_another_process_is_noticed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("down");
        std::fs::write(&path, "not json").unwrap();
        let mode = MaintenanceMode::new(path);
        let info = mode.current().await.expect("down");
        assert_eq!(info, MaintenanceInfo::default());
        assert_eq!(info.error().status, StatusCode::SERVICE_UNAVAILABLE);
    }
}
