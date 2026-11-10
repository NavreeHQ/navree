//! Temporary directories and SQLite databases for tests.
//!
//! Every test folder is named `navree-test-<random>` under the system
//! temporary directory, so one rule finds them all. On Windows a database
//! file still open (sqlx closes SQLite connections on worker threads, and
//! background tasks keep some until the runtime shuts down after the test)
//! blocks the delete when the guard drops. A folder that could not be
//! deleted then goes to the process's reaper thread, which retries for
//! about [`REAP_WINDOW`] without blocking the test, and the first guard of a
//! process starts a sweep of `navree-test-*` folders left by earlier runs.
//!
//! This file depends on `std` and `tempfile` only, so test code in crates
//! below navree-testing (which they may not depend on) includes it with
//! `#[path = "../../../navree-testing/src/temp.rs"] mod temp;` and gets the
//! same guard, reaper and sweep.

use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Mutex, Once, OnceLock};
use std::thread;
#[cfg(test)]
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use tempfile::TempDir;

/// What every test folder's name starts with.
pub(crate) const PREFIX: &str = "navree-test-";

/// Folders under [`PREFIX`] that are shared between runs and never swept:
/// the SQLite templates of navree-orm's `testing` module.
const KEEP: &[&str] = &["navree-test-templates"];

/// How old a `navree-test-*` folder must be before the sweep removes it. A
/// younger one may belong to a test process running in parallel.
pub(crate) const SWEEP_AGE: Duration = Duration::from_secs(60 * 60);

/// The environment variable that replaces [`SWEEP_AGE`] with a number of
/// seconds (for proof runs and tests; `0` removes every `navree-test-*`
/// folder, also those of tests running in parallel).
pub(crate) const SWEEP_AGE_VAR: &str = "NAVREE_TEST_SWEEP_AGE_SECS";

/// The file whose modification time records the latest sweep, so parallel
/// test processes do not sweep the same folders at once.
const SWEEP_STAMP: &str = "navree-test-sweep.stamp";

/// How soon after a sweep another process may sweep again.
pub(crate) const SWEEP_EVERY: Duration = Duration::from_secs(10 * 60);

/// How long the reaper keeps retrying one folder.
pub(crate) const REAP_WINDOW: Duration = Duration::from_secs(30);

/// A temporary directory, deleted (with its contents) when dropped.
///
/// Deletion is best effort: on Windows a file still open (a database pool
/// that outlives the guard) blocks it. The folder then goes to a background
/// thread that retries for about 30 seconds without holding up the test, and
/// a folder that stays is removed by a later test process once it is an hour
/// old.
pub struct TempDirGuard {
    dir: Option<TempDir>,
}

impl TempDirGuard {
    /// A new, empty temporary directory named `navree-test-<random>`.
    ///
    /// The first call in a process also starts the sweep of old
    /// `navree-test-*` folders, on a background thread.
    ///
    /// # Panics
    ///
    /// When the directory cannot be created.
    pub fn new() -> Self {
        sweep_once();
        let dir = tempfile::Builder::new()
            .prefix(PREFIX)
            .tempdir()
            .expect("a temporary directory can be created");
        Self { dir: Some(dir) }
    }

    /// The directory.
    pub fn path(&self) -> &Path {
        self.dir
            .as_ref()
            .expect("the directory lives until the guard drops")
            .path()
    }
}

impl Default for TempDirGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        if let Some(dir) = self.dir.take() {
            discard(dir.keep());
        }
    }
}

impl fmt::Debug for TempDirGuard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("TempDirGuard").field(&self.path()).finish()
    }
}

/// A navree-db URL for a new SQLite file in a fresh temporary directory
/// (`sqlite://<dir>/database.sqlite?mode=rwc`; SQLite creates the file on
/// first connect), and the guard that deletes the directory.
///
/// Each test gets its own database **file**: an in-memory SQLite database
/// would give every pool connection a different database.
///
/// Connecting needs navree-db's `sqlite` feature (a crate whose tests use
/// this URL turns it on in `[dev-dependencies]`); this function only
/// writes the URL.
///
/// ```
/// let (url, _guard) = navree_testing::temp_sqlite_url();
/// assert!(url.starts_with("sqlite://") && url.ends_with("?mode=rwc"));
/// ```
pub fn temp_sqlite_url() -> (String, TempDirGuard) {
    let guard = TempDirGuard::new();
    let file = guard.path().join("database.sqlite");
    (format!("sqlite://{}?mode=rwc", file.display()), guard)
}

/// Deletes `path` with its contents; a folder that stays (a file in it is
/// open) goes to the reaper.
fn discard(path: PathBuf) {
    if remove(&path).is_err() && path.exists() {
        global_reaper().hand_over(path);
    }
}

/// Deletes a directory tree; one that is gone already counts as deleted.
fn remove(path: &Path) -> io::Result<()> {
    match fs::remove_dir_all(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// How the reaper deletes a folder (replaced in tests).
type Remover = Box<dyn Fn(&Path) -> io::Result<()> + Send>;

/// The process's reaper.
pub(crate) fn global_reaper() -> &'static Reaper {
    static REAPER: OnceLock<Reaper> = OnceLock::new();
    REAPER.get_or_init(|| {
        Reaper::start(
            REAP_WINDOW,
            Duration::from_millis(2),
            Duration::from_millis(500),
            Box::new(remove),
        )
    })
}

/// A thread that keeps deleting folders that could not be deleted when
/// their guard dropped, waiting longer between attempts (`first_delay`,
/// doubling up to `max_delay`) and giving up on a folder `window` after it
/// was handed over.
pub(crate) struct Reaper {
    sender: Mutex<Option<Sender<PathBuf>>>,
    #[cfg(test)]
    thread: Mutex<Option<JoinHandle<()>>>,
}

/// A folder waiting for its next delete attempt.
struct Pending {
    path: PathBuf,
    next: Instant,
    delay: Duration,
    give_up: Instant,
}

impl Reaper {
    /// Starts the thread. When it cannot be started, folders handed over
    /// are left alone (the sweep gets them later).
    pub(crate) fn start(
        window: Duration,
        first_delay: Duration,
        max_delay: Duration,
        remover: Remover,
    ) -> Self {
        let (sender, receiver) = mpsc::channel::<PathBuf>();
        let handle = thread::Builder::new()
            .name("navree-test-reaper".to_owned())
            .spawn(move || {
                let mut pending: Vec<Pending> = Vec::new();
                let mut open = true;
                loop {
                    if pending.is_empty() {
                        if !open {
                            return;
                        }
                        match receiver.recv() {
                            Ok(path) => pending.push(Pending::new(path, window, first_delay)),
                            Err(_) => return,
                        }
                        continue;
                    }
                    let due = pending
                        .iter()
                        .map(|p| p.next)
                        .min()
                        .unwrap_or_else(Instant::now);
                    let wait = due.saturating_duration_since(Instant::now());
                    if open {
                        match receiver.recv_timeout(wait) {
                            Ok(path) => pending.push(Pending::new(path, window, first_delay)),
                            Err(RecvTimeoutError::Timeout) => {}
                            Err(RecvTimeoutError::Disconnected) => open = false,
                        }
                    } else {
                        thread::sleep(wait);
                    }
                    let now = Instant::now();
                    pending.retain_mut(|p| {
                        if p.next > now {
                            return true;
                        }
                        if remover(&p.path).is_ok() || !p.path.exists() || now >= p.give_up {
                            return false;
                        }
                        p.delay = (p.delay * 2).min(max_delay);
                        p.next = now + p.delay;
                        true
                    });
                }
            })
            .ok();
        #[cfg(not(test))]
        drop(handle);
        Self {
            sender: Mutex::new(Some(sender)),
            #[cfg(test)]
            thread: Mutex::new(handle),
        }
    }

    /// Hands a folder over; returns at once.
    pub(crate) fn hand_over(&self, path: PathBuf) {
        if let Some(sender) = self.sender.lock().ok().and_then(|s| s.clone()) {
            let _ = sender.send(path);
        }
    }

    /// Waits until every folder handed over so far is deleted or given up
    /// on, and stops the thread.
    #[cfg(test)]
    pub(crate) fn finish(&self) {
        if let Ok(mut sender) = self.sender.lock() {
            sender.take();
        }
        let thread = self.thread.lock().ok().and_then(|mut t| t.take());
        if let Some(thread) = thread {
            let _ = thread.join();
        }
    }
}

impl Pending {
    fn new(path: PathBuf, window: Duration, first_delay: Duration) -> Self {
        let now = Instant::now();
        Self {
            path,
            next: now + first_delay,
            delay: first_delay,
            give_up: now + window,
        }
    }
}

/// Whether the sweep may remove a folder of this name: it starts with
/// [`PREFIX`] and is not one of the folders shared between runs.
pub(crate) fn is_sweepable(name: &OsStr) -> bool {
    name.to_str()
        .is_some_and(|name| name.starts_with(PREFIX) && !KEEP.contains(&name))
}

/// Removes the `navree-test-*` folders directly under `root` that were
/// created at least `max_age` before `now`; returns how many it removed.
///
/// A folder's age is the time since its creation (its modification time
/// where the file system keeps no creation time). Everything else is left
/// alone: files, symbolic links, other names and folders whose age cannot be
/// read. Errors are ignored; what stays is for the next sweep.
pub(crate) fn sweep_dir(root: &Path, now: SystemTime, max_age: Duration) -> usize {
    let Ok(entries) = fs::read_dir(root) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        if !is_sweepable(&entry.file_name()) {
            continue;
        }
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let Ok(born) = meta.created().or_else(|_| meta.modified()) else {
            continue;
        };
        let old_enough = now.duration_since(born).is_ok_and(|age| age >= max_age);
        if old_enough && fs::remove_dir_all(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// Whether this process should sweep `root` now: no process did within
/// `every`. Records the sweep in a stamp file when it says yes.
pub(crate) fn claim_sweep(root: &Path, now: SystemTime, every: Duration) -> bool {
    let stamp = root.join(SWEEP_STAMP);
    // A stamp newer than `now` is recent too: the clock ticked since `now`
    // was read.
    let recent = !every.is_zero()
        && fs::metadata(&stamp)
            .and_then(|meta| meta.modified())
            .is_ok_and(|swept| {
                now.duration_since(swept)
                    .map_or(true, |since| since < every)
            });
    if recent {
        return false;
    }
    let _ = fs::write(&stamp, b"");
    true
}

/// The age the sweep uses: [`SWEEP_AGE_VAR`] when it holds a number of
/// seconds (the second value then says the override is on), else
/// [`SWEEP_AGE`].
pub(crate) fn sweep_age(var: Option<&str>) -> (Duration, bool) {
    match var.and_then(|seconds| seconds.trim().parse::<u64>().ok()) {
        Some(seconds) => (Duration::from_secs(seconds), true),
        None => (SWEEP_AGE, false),
    }
}

/// Starts the sweep of the system temporary directory, once per process, on
/// a background thread so the first test does not wait for it.
///
/// With [`SWEEP_AGE_VAR`] set the sweep ignores the stamp file and runs at
/// once.
fn sweep_once() {
    static SWEEP: Once = Once::new();
    SWEEP.call_once(|| {
        let _ = thread::Builder::new()
            .name("navree-test-sweep".to_owned())
            .spawn(|| {
                let root = std::env::temp_dir();
                let (age, forced) = sweep_age(std::env::var(SWEEP_AGE_VAR).ok().as_deref());
                let now = SystemTime::now();
                if forced || claim_sweep(&root, now, SWEEP_EVERY) {
                    sweep_dir(&root, now, age);
                }
            });
    });
}
