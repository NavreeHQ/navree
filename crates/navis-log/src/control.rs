//! [`LogControl`]: runtime level changes.

use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use tracing_subscriber::{EnvFilter, Registry, reload};

use crate::LogError;
use crate::file::FileSink;
use crate::filter::{Levels, check_target, parse_level};

/// How long [`LogControl::flush`] waits for the file writer.
const FLUSH_TIMEOUT: Duration = Duration::from_secs(5);

/// The reload handle of the level filter.
pub(crate) type FilterHandle = reload::Handle<EnvFilter, Registry>;

/// Changes log levels of a running process without a restart, through the
/// subscriber's reload handle.
///
/// `LoggerModule` exports it globally, so any provider, command or admin
/// endpoint can resolve `Arc<LogControl>`. Cloning is cheap; clones control
/// the same subscriber.
///
/// ```
/// use navis_log::{CaptureWriter, LogOptions, build_subscriber};
///
/// let (_dispatch, control) = build_subscriber(&LogOptions::default(), CaptureWriter::new())?;
/// control.set_level("navis_queue", "debug")?;
/// assert_eq!(control.directives(), "info,navis_queue=debug");
/// # Ok::<(), navis_log::LogError>(())
/// ```
#[derive(Clone)]
pub struct LogControl {
    inner: Arc<Inner>,
}

struct Inner {
    /// `None` when another global subscriber was installed first.
    filter: Option<FilterState>,
    /// The log file, when one is configured.
    file: Option<Arc<FileSink>>,
}

struct FilterState {
    handle: FilterHandle,
    levels: Mutex<Levels>,
}

impl LogControl {
    /// Controls the filter behind `handle`, currently set to `levels`, and
    /// the log file `file`.
    pub(crate) fn new(handle: FilterHandle, levels: Levels, file: Option<Arc<FileSink>>) -> Self {
        Self {
            inner: Arc::new(Inner {
                filter: Some(FilterState {
                    handle,
                    levels: Mutex::new(levels),
                }),
                file,
            }),
        }
    }

    /// A control for a subscriber Navis did not install: levels cannot change.
    pub(crate) fn detached() -> Self {
        Self {
            inner: Arc::new(Inner {
                filter: None,
                file: None,
            }),
        }
    }

    /// Sets the level of `target` and everything below it (`navis_db` also
    /// covers `navis_db::pool`). Levels: `trace`, `debug`, `info`, `warn`,
    /// `error`, `off` (any case). Targets are module paths: ASCII letters,
    /// digits, `_`, `-`, `.` and `::`.
    pub fn set_level(&self, target: &str, level: &str) -> Result<(), LogError> {
        check_target(target)?;
        let level = parse_level(level)?;
        self.update(|levels| levels.set_target(target, level))
    }

    /// Sets the level of every target without an explicit level.
    pub fn set_default_level(&self, level: &str) -> Result<(), LogError> {
        let level = parse_level(level)?;
        self.update(|levels| levels.set_default(level))
    }

    /// The current filter in `EnvFilter` syntax: the default level, then
    /// `target=level` pairs in target order (`info,navis_queue=debug`). Empty
    /// when this process's subscriber is not Navis's.
    pub fn directives(&self) -> String {
        self.inner
            .filter
            .as_ref()
            .map(|state| lock(&state.levels).directives())
            .unwrap_or_default()
    }

    /// Waits (up to 5 s) until every line logged so far is written to the
    /// log file; does nothing without a file. This blocks: call it from a
    /// blocking thread (`tokio::task::spawn_blocking`). `LoggerModule` calls
    /// it at shutdown.
    pub fn flush(&self) -> Result<(), LogError> {
        match &self.inner.file {
            Some(file) => file.flush(FLUSH_TIMEOUT),
            None => Ok(()),
        }
    }

    /// Lines not written to the log file because its queue was full (the
    /// disk could not keep up). Always 0 without a file.
    pub fn dropped_file_lines(&self) -> u64 {
        self.inner
            .file
            .as_ref()
            .map_or(0, |file| file.dropped_lines())
    }

    /// Replaces the whole filter (a later boot's options).
    pub(crate) fn replace_levels(&self, levels: Levels) -> Result<(), LogError> {
        self.update(|current| *current = levels)
    }

    /// Applies a change to a copy of the levels, reloads the filter, then
    /// keeps the copy; on error nothing changes.
    fn update(&self, change: impl FnOnce(&mut Levels)) -> Result<(), LogError> {
        let state = self.inner.filter.as_ref().ok_or(LogError::NotInstalled)?;
        let mut current = lock(&state.levels);
        let mut next = current.clone();
        change(&mut next);
        state
            .handle
            .reload(next.filter()?)
            .map_err(|_| LogError::SubscriberDropped)?;
        *current = next;
        Ok(())
    }
}

fn lock(levels: &Mutex<Levels>) -> MutexGuard<'_, Levels> {
    levels.lock().unwrap_or_else(PoisonError::into_inner)
}

impl fmt::Debug for LogControl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LogControl")
            .field("installed", &self.inner.filter.is_some())
            .field("directives", &self.directives())
            .finish()
    }
}
