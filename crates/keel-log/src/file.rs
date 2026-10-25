//! The log file: a rotating file (`tracing-appender`) written by a
//! background thread, so logging never waits for the disk, plus a flush that
//! waits until every queued line is written.
//!
//! `tracing_appender::non_blocking` can only flush by shutting its worker
//! down (dropping the `WorkerGuard`), which would end file logging for the
//! rest of the process; the subscriber is installed once per process and
//! outlives any one app, so this sink keeps its worker and flushes on demand.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::thread;
use std::time::{Duration, Instant};

use tracing_appender::rolling::{self, RollingFileAppender};
use tracing_subscriber::fmt::MakeWriter;

use crate::{FileLog, LogError, Rotation};

/// Lines queued for the writer thread before new lines are dropped (the
/// `tracing-appender` default).
const QUEUE_LINES: usize = 128_000;

enum Msg {
    Line(Vec<u8>),
    /// Acknowledged once every earlier line is written.
    Flush(SyncSender<()>),
}

/// The queue to the writer thread. The thread stops when the sink is dropped.
#[derive(Debug)]
pub(crate) struct FileSink {
    tx: SyncSender<Msg>,
    dropped: AtomicU64,
}

impl FileSink {
    /// Opens (creating directories as needed) the rotating file described by
    /// `file` and starts its writer thread.
    pub(crate) fn open(file: &FileLog) -> Result<Self, LogError> {
        let appender = appender(file).map_err(|message| LogError::File {
            path: file.path.clone(),
            message,
        })?;
        Self::spawn(appender, QUEUE_LINES).map_err(|error| LogError::File {
            path: file.path.clone(),
            message: format!("cannot start the log writer thread: {error}"),
        })
    }

    /// Starts a writer thread for `out` with room for `capacity` queued lines.
    fn spawn(out: impl Write + Send + 'static, capacity: usize) -> io::Result<Self> {
        let (tx, rx) = mpsc::sync_channel(capacity);
        thread::Builder::new()
            .name("keel-log-file".to_owned())
            .spawn(move || write_lines(out, rx))?;
        Ok(Self {
            tx,
            dropped: AtomicU64::new(0),
        })
    }

    /// Queues one formatted line; drops it (and counts it) when the queue is full.
    fn push(&self, line: &[u8]) {
        if let Err(TrySendError::Full(_)) = self.tx.try_send(Msg::Line(line.to_vec())) {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Waits up to `timeout` until every line queued so far is written.
    pub(crate) fn flush(&self, timeout: Duration) -> Result<(), LogError> {
        let deadline = Instant::now() + timeout;
        let (ack_tx, ack_rx) = mpsc::sync_channel(1);
        let mut msg = Msg::Flush(ack_tx);
        loop {
            match self.tx.try_send(msg) {
                Ok(()) => break,
                Err(TrySendError::Full(back)) if Instant::now() < deadline => {
                    msg = back;
                    thread::sleep(Duration::from_millis(1));
                }
                Err(TrySendError::Full(_)) => return Err(LogError::FlushTimedOut { timeout }),
                Err(TrySendError::Disconnected(_)) => return Err(LogError::FileWriterStopped),
            }
        }
        match ack_rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(()) => Ok(()),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(LogError::FlushTimedOut { timeout }),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(LogError::FileWriterStopped),
        }
    }

    /// Lines dropped so far because the queue was full.
    pub(crate) fn dropped_lines(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

/// The writer thread: writes lines in order and acknowledges flushes. A
/// write error is reported on stderr once per run of failures.
fn write_lines(mut out: impl Write, rx: Receiver<Msg>) {
    let mut failing = false;
    for msg in rx {
        match msg {
            Msg::Line(line) => match out.write_all(&line) {
                Ok(()) => failing = false,
                Err(error) => {
                    if !failing {
                        eprintln!("keel-log: cannot write to the log file: {error}");
                    }
                    failing = true;
                }
            },
            Msg::Flush(ack) => {
                if let Err(error) = out.flush() {
                    eprintln!("keel-log: cannot flush the log file: {error}");
                }
                // The waiter may have timed out and gone; nothing to do then.
                let _ = ack.send(());
            }
        }
    }
}

/// The `MakeWriter` of the file layer: every formatted event is one queued line.
#[derive(Clone, Debug)]
pub(crate) struct FileWriter(pub(crate) std::sync::Arc<FileSink>);

impl<'a> MakeWriter<'a> for FileWriter {
    type Writer = QueuedLine<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        QueuedLine(&self.0)
    }
}

/// Queues what the formatter writes (one `write_all` per event).
pub(crate) struct QueuedLine<'a>(&'a FileSink);

impl Write for QueuedLine<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.push(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The rolling appender for `file`.
fn appender(file: &FileLog) -> Result<RollingFileAppender, String> {
    let (dir, prefix, suffix) = split_path(&file.path);
    // tracing-appender prunes old files before creating the directory.
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("cannot create `{}`: {error}", dir.display()))?;
    let rotation = match file.rotation {
        Rotation::Daily => rolling::Rotation::DAILY,
        Rotation::Hourly => rolling::Rotation::HOURLY,
        Rotation::Never => rolling::Rotation::NEVER,
    };
    let mut builder = RollingFileAppender::builder()
        .rotation(rotation)
        .filename_prefix(prefix);
    if let Some(suffix) = suffix {
        builder = builder.filename_suffix(suffix);
    }
    if let Some(max_files) = max_files(file) {
        builder = builder.max_log_files(max_files);
    }
    builder.build(&dir).map_err(|error| error.to_string())
}

/// `dir/name.ext` → (`dir`, `name`, `Some("ext")`); a bare file name lives in `.`.
fn split_path(path: &Path) -> (PathBuf, String, Option<String>) {
    let dir = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let prefix = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let suffix = path
        .extension()
        .map(|ext| ext.to_string_lossy().into_owned());
    (dir, prefix, suffix)
}

/// How many files `keep_days` days of rotation produce (`None`: never rotated).
fn max_files(file: &FileLog) -> Option<usize> {
    let days = usize::try_from(file.keep_days).unwrap_or(usize::MAX);
    match file.rotation {
        Rotation::Daily => Some(days),
        Rotation::Hourly => Some(days.saturating_mul(24)),
        Rotation::Never => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Condvar, Mutex};

    #[test]
    fn paths_split_into_directory_prefix_and_suffix() {
        assert_eq!(
            split_path(Path::new("storage/logs/keel.log")),
            (
                PathBuf::from("storage/logs"),
                "keel".to_owned(),
                Some("log".to_owned())
            )
        );
        assert_eq!(
            split_path(Path::new("app")),
            (PathBuf::from("."), "app".to_owned(), None)
        );
    }

    #[test]
    fn keep_days_become_a_file_count() {
        let file = |rotation| FileLog {
            rotation,
            keep_days: 3,
            ..FileLog::default()
        };
        assert_eq!(max_files(&file(Rotation::Daily)), Some(3));
        assert_eq!(max_files(&file(Rotation::Hourly)), Some(72));
        assert_eq!(max_files(&file(Rotation::Never)), None);
    }

    /// A two-state flag threads can wait on.
    #[derive(Clone, Default)]
    struct Flag(Arc<(Mutex<bool>, Condvar)>);

    impl Flag {
        fn set(&self) {
            *self.0.0.lock().expect("flag") = true;
            self.0.1.notify_all();
        }

        fn wait(&self) {
            let mut set = self.0.0.lock().expect("flag");
            while !*set {
                set = self.0.1.wait(set).expect("flag");
            }
        }
    }

    /// A writer that signals `entered` on its first write, blocks until
    /// `open` is set, then records what it writes.
    #[derive(Clone, Default)]
    struct Gate {
        entered: Flag,
        open: Flag,
        written: Arc<Mutex<Vec<u8>>>,
    }

    impl Write for Gate {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.entered.set();
            self.open.wait();
            self.written.lock().expect("written").extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn full_queue_drops_and_counts_lines_then_flush_writes_the_rest() {
        let gate = Gate::default();
        let sink = FileSink::spawn(gate.clone(), 1).expect("spawn");
        // The thread takes `a` and blocks in the writer; `b` fills the
        // one-line queue; `c`, `d` and `e` are dropped.
        sink.push(
            b"a
",
        );
        gate.entered.wait();
        for line in [
            "b
", "c
", "d
", "e
",
        ] {
            sink.push(line.as_bytes());
        }
        assert_eq!(sink.dropped_lines(), 3);
        gate.open.set();
        sink.flush(Duration::from_secs(5)).expect("flush");
        let written = gate.written.lock().expect("written").clone();
        assert_eq!(
            String::from_utf8(written).expect("utf-8"),
            "a
b
"
        );
    }

    #[test]
    fn flush_times_out_when_the_writer_is_stuck() {
        let gate = Gate::default();
        let sink = FileSink::spawn(gate.clone(), 4).expect("spawn");
        sink.push(
            b"stuck
",
        );
        let error = sink
            .flush(Duration::from_millis(50))
            .expect_err("writer is blocked");
        assert!(matches!(error, LogError::FlushTimedOut { .. }), "{error:?}");
        gate.open.set();
    }
}
