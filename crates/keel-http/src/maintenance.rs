//! Maintenance mode: while the maintenance file exists, every request gets 503.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError, RwLock};
use std::time::Duration;

use http::StatusCode;
use http::header::{HeaderValue, RETRY_AFTER};
use serde::{Deserialize, Serialize};
use tokio::time::Instant;

use crate::AppError;

/// How long a file check is reused before the file is looked at again.
const CHECK_INTERVAL: Duration = Duration::from_secs(1);

/// The contents of the maintenance file (written by the `down` command).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaintenanceInfo {
    /// When maintenance started (Unix seconds).
    #[serde(default)]
    pub time: Option<i64>,
    /// Seconds clients should wait before retrying (`Retry-After`).
    #[serde(default)]
    pub retry: Option<u64>,
    /// The message shown as the problem `detail`.
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
            "maintenance",
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
#[derive(Debug)]
pub struct MaintenanceMode {
    path: PathBuf,
    state: RwLock<Option<MaintenanceInfo>>,
    checked_at: Mutex<Option<Instant>>,
}

impl MaintenanceMode {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self {
            path,
            state: RwLock::new(None),
            checked_at: Mutex::new(None),
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
        self.state
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
        *self.state.write().unwrap_or_else(PoisonError::into_inner) = info;
        *self
            .checked_at
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(Instant::now());
    }

    /// True for the one caller that should re-read the file now.
    fn claim_refresh(&self) -> bool {
        let mut checked_at = self
            .checked_at
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let now = Instant::now();
        match *checked_at {
            Some(at) if now.duration_since(at) < CHECK_INTERVAL => false,
            _ => {
                *checked_at = Some(now);
                true
            }
        }
    }
}

/// Reads the maintenance file: `None` when absent; an unreadable or
/// malformed file still means "down" (with default info).
async fn read_info(path: &Path) -> Option<MaintenanceInfo> {
    match tokio::fs::read(path).await {
        Ok(bytes) => Some(serde_json::from_slice(&bytes).unwrap_or_default()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => {
            tracing::warn!(target: "keel::http", path = %path.display(), %error, "cannot read the maintenance file; treating the app as down");
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
