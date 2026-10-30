//! Tests of the sweep and the reaper behind `TempDirGuard`.

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime};

use crate::temp::{
    PREFIX, Reaper, SWEEP_AGE, claim_sweep, global_reaper, is_sweepable, sweep_age, sweep_dir,
};

const HOUR: Duration = Duration::from_secs(60 * 60);

/// A scratch directory for one test, with the test prefix.
fn scratch() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(PREFIX)
        .tempdir()
        .expect("a temporary directory")
}

fn folder(root: &Path, name: &str) -> PathBuf {
    let path = root.join(name);
    fs::create_dir_all(path.join("inner")).unwrap();
    fs::write(path.join("inner").join("database.sqlite"), b"x").unwrap();
    path
}

#[test]
fn only_prefixed_folders_other_than_the_shared_templates_are_sweepable() {
    assert!(is_sweepable(OsStr::new("navree-test-a1B2c3")));
    assert!(is_sweepable(OsStr::new("navree-test-")));
    assert!(!is_sweepable(OsStr::new("navree-test-templates")));
    assert!(!is_sweepable(OsStr::new(".tmpA1B2c3")));
    assert!(!is_sweepable(OsStr::new("navree-testing")));
    assert!(!is_sweepable(OsStr::new("my-navree-test-a1B2c3")));
    assert!(!is_sweepable(OsStr::new("Navree-Test-a1B2c3")));
    assert!(!is_sweepable(OsStr::new("")));
}

#[test]
fn the_sweep_removes_old_prefixed_folders_and_nothing_else() {
    let root = scratch();
    let old = folder(root.path(), "navree-test-old");
    let templates = folder(root.path(), "navree-test-templates");
    let other = folder(root.path(), "other-tool");
    let hidden = folder(root.path(), ".tmpAbC123");
    let file = root.path().join("navree-test-a-file");
    fs::write(&file, b"x").unwrap();

    // Two hours later every folder is two hours old.
    let later = SystemTime::now() + 2 * HOUR;
    let removed = sweep_dir(root.path(), later, HOUR);

    assert_eq!(removed, 1);
    assert!(!old.exists(), "an old test folder goes, contents and all");
    assert!(templates.is_dir(), "the shared templates stay");
    assert!(other.is_dir(), "other names stay");
    assert!(hidden.is_dir(), "other tools' temporary folders stay");
    assert!(file.is_file(), "files stay");
}

#[test]
fn the_sweep_keeps_folders_younger_than_the_age() {
    let root = scratch();
    let fresh = folder(root.path(), "navree-test-fresh");

    // Just created: a test process running in parallel may be using it.
    assert_eq!(sweep_dir(root.path(), SystemTime::now(), SWEEP_AGE), 0);
    assert!(fresh.is_dir());

    // 59 minutes later it is still younger than the hour.
    let almost = SystemTime::now() + Duration::from_secs(59 * 60);
    assert_eq!(sweep_dir(root.path(), almost, SWEEP_AGE), 0);
    assert!(fresh.is_dir());

    // A clock that reads earlier than the creation time never makes a
    // folder old.
    let before = SystemTime::now() - 2 * HOUR;
    assert_eq!(sweep_dir(root.path(), before, SWEEP_AGE), 0);
    assert!(fresh.is_dir());

    // An hour and a minute later it goes.
    let later = SystemTime::now() + HOUR + Duration::from_secs(60);
    assert_eq!(sweep_dir(root.path(), later, SWEEP_AGE), 1);
    assert!(!fresh.exists());
}

#[test]
fn a_zero_age_removes_every_prefixed_folder_but_the_templates() {
    let root = scratch();
    let a = folder(root.path(), "navree-test-a");
    let b = folder(root.path(), "navree-test-b");
    let templates = folder(root.path(), "navree-test-templates");

    let later = SystemTime::now() + Duration::from_secs(1);
    assert_eq!(sweep_dir(root.path(), later, Duration::ZERO), 2);
    assert!(!a.exists() && !b.exists());
    assert!(templates.is_dir());
}

#[test]
fn sweeping_a_missing_directory_is_not_an_error() {
    let root = scratch();
    let missing = root.path().join("nowhere");
    assert_eq!(sweep_dir(&missing, SystemTime::now(), HOUR), 0);
}

#[test]
fn one_process_claims_the_sweep_for_a_while() {
    let root = scratch();
    let now = SystemTime::now();
    let every = Duration::from_secs(600);

    assert!(claim_sweep(root.path(), now, every), "nobody swept yet");
    assert!(!claim_sweep(root.path(), now, every), "just swept");
    assert!(
        !claim_sweep(root.path(), now - Duration::from_secs(60), every),
        "a stamp newer than the clock is recent"
    );
    assert!(
        !claim_sweep(root.path(), now + Duration::from_secs(300), every),
        "swept five minutes ago"
    );
    assert!(
        claim_sweep(root.path(), now + Duration::from_secs(700), every),
        "swept long ago"
    );
    assert!(
        claim_sweep(root.path(), now, Duration::ZERO),
        "an interval of zero always sweeps"
    );
}

#[test]
fn the_age_comes_from_the_variable_when_it_is_a_number_of_seconds() {
    assert_eq!(sweep_age(None), (SWEEP_AGE, false));
    assert_eq!(sweep_age(Some("")), (SWEEP_AGE, false));
    assert_eq!(sweep_age(Some("soon")), (SWEEP_AGE, false));
    assert_eq!(sweep_age(Some("-5")), (SWEEP_AGE, false));
    assert_eq!(sweep_age(Some("0")), (Duration::ZERO, true));
    assert_eq!(sweep_age(Some(" 90 ")), (Duration::from_secs(90), true));
}

/// A reaper whose remover fails `failures` times, then deletes.
fn failing_then_deleting(failures: usize, window: Duration) -> (Reaper, Arc<AtomicUsize>) {
    let attempts = Arc::new(AtomicUsize::new(0));
    let counted = attempts.clone();
    let reaper = Reaper::start(
        window,
        Duration::from_millis(5),
        Duration::from_millis(20),
        Box::new(move |path: &Path| {
            let attempt = counted.fetch_add(1, Ordering::SeqCst) + 1;
            if attempt <= failures {
                Err(io::Error::new(io::ErrorKind::PermissionDenied, "in use"))
            } else {
                fs::remove_dir_all(path)
            }
        }),
    );
    (reaper, attempts)
}

#[test]
fn the_reaper_retries_until_the_folder_can_be_deleted() {
    let root = scratch();
    let stuck = folder(root.path(), "navree-test-stuck");
    let (reaper, attempts) = failing_then_deleting(3, Duration::from_secs(30));

    reaper.hand_over(stuck.clone());
    reaper.finish();

    assert!(!stuck.exists(), "deleted once the files were free");
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        4,
        "three failures, one success"
    );
}

#[test]
fn the_reaper_gives_up_after_its_window() {
    let root = scratch();
    let stuck = folder(root.path(), "navree-test-forever");
    let (reaper, attempts) = failing_then_deleting(usize::MAX, Duration::from_millis(150));

    let started = Instant::now();
    reaper.hand_over(stuck.clone());
    reaper.finish();

    assert!(stuck.exists(), "nothing could delete it");
    let attempts = attempts.load(Ordering::SeqCst);
    assert!(attempts >= 3, "it kept trying ({attempts} attempts)");
    assert!(attempts < 60, "it backed off ({attempts} attempts)");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "it stopped after the window, not after the files were free"
    );
}

#[test]
fn the_reaper_stops_when_the_folder_is_gone_by_other_means() {
    let root = scratch();
    let gone = root.path().join("navree-test-gone");
    let (reaper, attempts) = failing_then_deleting(usize::MAX, Duration::from_secs(30));

    reaper.hand_over(gone);
    reaper.finish();

    assert_eq!(attempts.load(Ordering::SeqCst), 1, "one look was enough");
}

#[test]
fn the_reaper_works_on_several_folders_at_once() {
    let root = scratch();
    let first = folder(root.path(), "navree-test-first");
    let second = folder(root.path(), "navree-test-second");
    let (reaper, _) = failing_then_deleting(2, Duration::from_secs(30));

    reaper.hand_over(first.clone());
    reaper.hand_over(second.clone());
    reaper.finish();

    assert!(!first.exists() && !second.exists());
}

#[test]
fn handing_a_folder_over_does_not_wait_for_the_delete() {
    let root = scratch();
    let slow = folder(root.path(), "navree-test-slow");
    let reaper = Reaper::start(
        Duration::from_secs(30),
        Duration::from_millis(1),
        Duration::from_millis(5),
        Box::new(|path: &Path| {
            std::thread::sleep(Duration::from_millis(300));
            fs::remove_dir_all(path)
        }),
    );

    let started = Instant::now();
    reaper.hand_over(slow.clone());
    assert!(
        started.elapsed() < Duration::from_millis(150),
        "hand_over returned while the thread was still deleting"
    );
    reaper.finish();
    assert!(!slow.exists());
}

#[test]
fn the_process_shares_one_reaper() {
    assert!(std::ptr::eq(global_reaper(), global_reaper()));
}

/// A file opened without delete sharing, as SQLite opens its files, blocks
/// the delete of its folder on Windows; the folder goes once it is closed.
#[cfg(windows)]
#[test]
fn a_guard_whose_database_file_is_still_open_loses_its_folder_later() {
    use std::os::windows::fs::OpenOptionsExt;

    const SHARE_READ_WRITE: u32 = 0x1 | 0x2;

    let guard = crate::TempDirGuard::new();
    let dir = guard.path().to_owned();
    let file = dir.join("database.sqlite");
    let open = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .share_mode(SHARE_READ_WRITE)
        .open(&file)
        .unwrap();

    drop(guard);
    assert!(dir.exists(), "Windows keeps a folder with an open file");

    drop(open);
    let deadline = Instant::now() + Duration::from_secs(15);
    while dir.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!dir.exists(), "the reaper deleted it once the file closed");
}
