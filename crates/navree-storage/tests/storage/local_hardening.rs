//! The local disk against names and files made to confuse it: staging names,
//! names the platform reads as other names, files that are not regular files,
//! and the modes of what it writes.

use std::time::Duration;

use bytes::Bytes;
use futures_util::{StreamExt, stream};
use navree_storage::{Disk, StorageError};

use crate::local::{Sandbox, files_under};

/// Runs `work` on a thread of its own, with a runtime of its own, and waits
/// `limit` for it: an operation stuck in a blocking call fails the test
/// instead of hanging it. The thread of a stuck call is left behind.
fn finishes_within<T: Send + 'static>(
    limit: Duration,
    work: impl std::future::Future<Output = T> + Send + 'static,
) -> Option<T> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = runtime.block_on(work);
        // Dropping a runtime waits for its blocking calls.
        std::mem::forget(runtime);
        let _ = sender.send(result);
    });
    receiver.recv_timeout(limit).ok()
}

/// An upload that has written `body` and is waiting for more, run by a task.
/// Aborting the task drops the upload.
fn stalled_upload(disk: &Disk, path: &str, body: &'static [u8]) -> tokio::task::JoinHandle<()> {
    let disk = disk.clone();
    let path = path.to_owned();
    tokio::spawn(async move {
        let chunks = stream::iter([Ok::<_, std::io::Error>(Bytes::from_static(body))])
            .chain(stream::pending());
        let _ = disk.put_stream(&path, chunks).await;
    })
}

#[tokio::test]
async fn only_the_last_hash_of_a_name_can_make_it_a_staging_name() {
    let sandbox = Sandbox::new();
    let disk = sandbox.disk();
    for destination in ["report#final.pdf", "trail#", "a#b#c", "plain.pdf"] {
        let upload = stalled_upload(&disk, destination, b"PARTIAL-UPLOAD");
        tokio::time::sleep(Duration::from_millis(300)).await;
        // The upload is not finished: nothing is listed, and no name the disk
        // has on disk for it can be read.
        assert!(disk.list("").await.unwrap().is_empty(), "{destination}");
        let staged = files_under(&sandbox.root());
        assert_eq!(staged.len(), 1, "{destination}: {staged:?}");
        let error = disk.get(&staged[0]).await.unwrap_err();
        assert!(
            error.to_string().contains("not supported"),
            "{destination}: {error}"
        );
        assert!(!disk.exists("report#final.pdf").await.unwrap());
        upload.abort();
        let _ = upload.await;
        for _ in 0..100 {
            if files_under(&sandbox.root()).is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(files_under(&sandbox.root()).is_empty(), "{destination}");
    }
    // A hash that is not the last one makes an ordinary name.
    for name in ["a#1#b", "x#1/y.txt", "report#final.pdf"] {
        disk.put(name, "ok").await.unwrap();
        assert_eq!(disk.get(name).await.unwrap(), "ok", "{name}");
    }
}

#[tokio::test]
async fn a_directory_planted_at_a_staging_name_is_skipped() {
    let sandbox = Sandbox::new();
    let disk = sandbox.disk();
    std::fs::create_dir_all(sandbox.root().join("f#1")).unwrap();
    std::fs::write(sandbox.root().join("f#1").join("x"), "planted").unwrap();
    disk.put("f", "mine").await.unwrap();
    assert_eq!(disk.get("f").await.unwrap(), "mine");
    assert_eq!(
        std::fs::read_to_string(sandbox.root().join("f#1").join("x")).unwrap(),
        "planted"
    );
}

#[tokio::test]
async fn a_write_gives_up_when_every_staging_name_is_taken() {
    let sandbox = Sandbox::new();
    let disk = sandbox.disk();
    for number in 1..=1100 {
        std::fs::write(sandbox.root().join(format!("f#{number}")), "left over").unwrap();
    }
    let started = std::time::Instant::now();
    let error = disk.put("f", "mine").await.unwrap_err();
    assert!(matches!(error, StorageError::Backend { .. }), "{error:?}");
    assert!(error.to_string().contains("too many"), "{error}");
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(!sandbox.root().join("f").exists());
    // Another file is not affected.
    disk.put("g", "mine").await.unwrap();
}

#[tokio::test]
async fn a_percent_escape_in_a_name_is_not_a_colon() {
    let sandbox = Sandbox::new();
    let disk = sandbox.disk();
    disk.put("a:b", "COLON").await.unwrap();
    disk.put("a%3Ab", "PERCENT").await.unwrap();
    disk.put("c%3ad", "LOWER").await.unwrap();
    disk.put("c:d", "COLON-LOWER").await.unwrap();
    disk.put("e%253Af", "DOUBLE").await.unwrap();
    assert_eq!(disk.get("a:b").await.unwrap(), "COLON");
    assert_eq!(disk.get("a%3Ab").await.unwrap(), "PERCENT");
    assert_eq!(disk.get("c%3ad").await.unwrap(), "LOWER");
    assert_eq!(disk.get("c:d").await.unwrap(), "COLON-LOWER");
    assert_eq!(disk.get("e%253Af").await.unwrap(), "DOUBLE");
    // A listing names every file the way it was written, and each name opens
    // that file.
    let listed = disk.list("").await.unwrap();
    assert_eq!(listed, ["a%3Ab", "a:b", "c%3ad", "c:d", "e%253Af"]);
    for name in &listed {
        assert!(disk.exists(name).await.unwrap(), "{name}");
    }
    assert_eq!(files_under(&sandbox.root()).len(), 5);
}

#[cfg(unix)]
mod files_that_are_not_regular {
    use super::*;

    #[tokio::test]
    async fn a_pipe_is_a_missing_file_and_nothing_waits_for_it() {
        let sandbox = Sandbox::new();
        let disk = sandbox.disk();
        disk.put("kept.txt", "k").await.unwrap();
        let made = std::process::Command::new("mkfifo")
            .arg(sandbox.root().join("pipe"))
            .status()
            .unwrap();
        assert!(made.success());

        let probe = disk.clone();
        let outcome = finishes_within(Duration::from_secs(10), async move {
            let read = probe.get("pipe").await;
            let streamed = probe.get_stream("pipe").await.err();
            let size = probe.size("pipe").await;
            let exists = probe.exists("pipe").await;
            let listed = probe.list("").await;
            (read, streamed, size, exists, listed)
        })
        .expect("a pipe in the root must not hold up a call");
        let (read, streamed, size, exists, listed) = outcome;
        assert!(
            matches!(read, Err(StorageError::NotFound { .. })),
            "{read:?}"
        );
        assert!(
            matches!(streamed, Some(StorageError::NotFound { .. })),
            "{streamed:?}"
        );
        assert!(
            matches!(size, Err(StorageError::NotFound { .. })),
            "{size:?}"
        );
        assert!(!exists.unwrap());
        assert_eq!(listed.unwrap(), ["kept.txt"]);
    }

    #[tokio::test]
    async fn a_file_name_with_a_control_character_fails_the_listing_but_not_the_other_files() {
        let sandbox = Sandbox::new();
        let disk = sandbox.disk();
        disk.put("kept.txt", "k").await.unwrap();
        std::fs::write(sandbox.root().join("bad\nname"), "x").unwrap();
        let error = disk.list("").await.unwrap_err();
        assert!(matches!(error, StorageError::Backend { .. }), "{error:?}");
        assert!(error.to_string().contains("control character"), "{error}");
        assert_eq!(disk.get("kept.txt").await.unwrap(), "k");
        assert!(disk.list("nothing").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn writing_over_a_link_replaces_the_link_and_leaves_its_target() {
        let sandbox = Sandbox::new();
        let disk = sandbox.disk();
        let secret = sandbox.outer.path().join("secret.txt");
        std::os::unix::fs::symlink(&secret, sandbox.root().join("out.txt")).unwrap();
        disk.put("out.txt", "mine").await.unwrap();
        assert_eq!(disk.get("out.txt").await.unwrap(), "mine");
        assert_eq!(std::fs::read_to_string(&secret).unwrap(), "top secret");
        let kind = std::fs::symlink_metadata(sandbox.root().join("out.txt")).unwrap();
        assert!(kind.is_file(), "the link is replaced by a file");
    }
}

#[cfg(unix)]
mod permissions {
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    use super::*;

    fn mode(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o7777
    }

    fn set_mode(path: &Path, mode: u32) {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    #[tokio::test]
    async fn replacing_a_file_keeps_its_mode() {
        let sandbox = Sandbox::new();
        let disk = sandbox.disk();
        for (name, wanted) in [
            ("private", 0o600),
            ("shared", 0o664),
            ("program", 0o755),
            ("closed", 0o400),
        ] {
            let path = sandbox.root().join(name);
            std::fs::write(&path, "old").unwrap();
            set_mode(&path, wanted);
            disk.put(name, "new").await.unwrap();
            assert_eq!(mode(&path), wanted, "{name} after put");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
            let streamed = stream::iter([Ok::<_, std::io::Error>(Bytes::from_static(b"streamed"))]);
            disk.put_stream(name, streamed).await.unwrap();
            assert_eq!(mode(&path), wanted, "{name} after put_stream");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "streamed");
        }
    }

    #[tokio::test]
    async fn a_new_file_gets_the_mode_a_file_made_here_gets() {
        let sandbox = Sandbox::new();
        let disk = sandbox.disk();
        std::fs::create_dir_all(sandbox.root().join("sub")).unwrap();
        let ordinary = sandbox.root().join("sub").join("ordinary");
        std::fs::File::create(&ordinary).unwrap();
        disk.put("sub/new.txt", "x").await.unwrap();
        let streamed = stream::iter([Ok::<_, std::io::Error>(Bytes::from_static(b"x"))]);
        disk.put_stream("sub/streamed.txt", streamed).await.unwrap();
        assert_eq!(mode(&sandbox.root().join("sub/new.txt")), mode(&ordinary));
        assert_eq!(
            mode(&sandbox.root().join("sub/streamed.txt")),
            mode(&ordinary)
        );
    }

    #[tokio::test]
    async fn a_file_is_private_while_it_is_written() {
        let sandbox = Sandbox::new();
        let disk = sandbox.disk();
        let upload = stalled_upload(&disk, "report.pdf", b"PARTIAL-UPLOAD");
        tokio::time::sleep(Duration::from_millis(300)).await;
        let staged = files_under(&sandbox.root());
        assert_eq!(staged.len(), 1, "{staged:?}");
        assert_eq!(
            mode(&sandbox.root().join(&staged[0])) & 0o077,
            0,
            "{staged:?}"
        );
        upload.abort();
        let _ = upload.await;
    }
}

#[cfg(windows)]
mod windows_names {
    use super::*;

    /// Names Windows reads as another name than the one written.
    const OTHER_NAMES: &[&str] = &[
        "secret.txt.",
        "secret.txt..",
        "secret.txt ",
        "secret.txt. ",
        "secret.txt .",
        "a./secret.txt",
        "a../secret.txt",
        "a /secret.txt",
        "a. /secret.txt",
        "AVERYL~1.TXT",
        "AVERYL~1/inner.txt",
        "a/PROGRA~1",
        "secret~2.txt",
        "up.pdf#1.",
        "up.pdf#1 ",
    ];

    /// Names of devices, which open the device wherever they are.
    const DEVICES: &[&str] = &[
        "CON",
        "nul",
        "AUX",
        "PRN",
        "COM1",
        "com9",
        "LPT1",
        "lpt9",
        "COM\u{b9}",
        "LPT\u{b3}",
        "CONIN$",
        "CONOUT$",
        "NUL.txt",
        "con.tar.gz",
        "a/NUL",
        "NUL/x",
        "a/aux.log",
        "NUL .txt",
    ];

    fn plant(sandbox: &Sandbox) {
        std::fs::create_dir_all(sandbox.root().join("a")).unwrap();
        std::fs::write(sandbox.root().join("secret.txt"), "SECRET").unwrap();
        std::fs::write(sandbox.root().join("a").join("secret.txt"), "A-SECRET").unwrap();
    }

    #[tokio::test]
    async fn names_that_windows_reads_as_other_names_are_refused() {
        let sandbox = Sandbox::new();
        let disk = sandbox.disk();
        plant(&sandbox);
        let before = files_under(&sandbox.root());
        for name in OTHER_NAMES {
            let name = (*name).to_owned();
            let probe = disk.clone();
            let shown = name.clone();
            let (get, exists, put, delete) = finishes_within(Duration::from_secs(10), async move {
                (
                    probe.get(&name).await.map(|bytes| bytes.len()),
                    probe.exists(&name).await.map(|_| ()),
                    probe.put(&name, "OVERWRITTEN").await,
                    probe.delete(&name).await,
                )
            })
            .unwrap_or_else(|| panic!("{shown:?} held up a call"));
            assert!(
                matches!(get, Err(StorageError::Backend { .. })),
                "get {shown:?}: {get:?}"
            );
            assert!(
                matches!(exists, Err(StorageError::Backend { .. })),
                "exists {shown:?}: {exists:?}"
            );
            assert!(
                matches!(put, Err(StorageError::Backend { .. })),
                "put {shown:?}: {put:?}"
            );
            assert!(
                matches!(delete, Err(StorageError::Backend { .. })),
                "delete {shown:?}: {delete:?}"
            );
        }
        assert_eq!(files_under(&sandbox.root()), before);
        assert_eq!(
            std::fs::read_to_string(sandbox.root().join("secret.txt")).unwrap(),
            "SECRET"
        );
        assert_eq!(
            std::fs::read_to_string(sandbox.root().join("a/secret.txt")).unwrap(),
            "A-SECRET"
        );
    }

    #[tokio::test]
    async fn device_names_are_refused_with_or_without_an_extension() {
        let sandbox = Sandbox::new();
        let disk = sandbox.disk();
        plant(&sandbox);
        let before = files_under(&sandbox.root());
        for name in DEVICES {
            let name = (*name).to_owned();
            let probe = disk.clone();
            let shown = name.clone();
            let (get, put, list) = finishes_within(Duration::from_secs(10), async move {
                (
                    probe.get(&name).await.map(|bytes| bytes.len()),
                    probe.put(&name, "DATA").await,
                    probe.list(&name).await.map(|_| ()),
                )
            })
            .unwrap_or_else(|| panic!("{shown:?} held up a call"));
            assert!(
                matches!(get, Err(StorageError::Backend { .. })),
                "get {shown:?}: {get:?}"
            );
            assert!(
                matches!(put, Err(StorageError::Backend { .. })),
                "put {shown:?}: {put:?}"
            );
            assert!(
                matches!(list, Err(StorageError::Backend { .. })),
                "list {shown:?}: {list:?}"
            );
        }
        assert_eq!(files_under(&sandbox.root()), before);
    }

    #[tokio::test]
    async fn a_directory_name_with_a_trailing_space_or_dot_does_not_spin() {
        let sandbox = Sandbox::new();
        let disk = sandbox.disk();
        for name in ["wd /x", "wd../x", "wd. /x", "NUL/x"] {
            let probe = disk.clone();
            let result = finishes_within(Duration::from_secs(10), async move {
                probe.put(name, "x").await
            });
            assert!(
                matches!(result, Some(Err(StorageError::Backend { .. }))),
                "{name:?}: {result:?}"
            );
        }
    }

    #[tokio::test]
    async fn the_short_name_of_a_file_is_not_a_name_of_it() {
        let sandbox = Sandbox::new();
        let disk = sandbox.disk();
        disk.put("averylongfilename.txt", "LONG").await.unwrap();
        disk.put("averylongdirectory/inner.txt", "INNER")
            .await
            .unwrap();
        // The short names the volume gave them, if it makes any.
        let listing = std::process::Command::new("cmd")
            .args(["/C", "dir", "/x"])
            .arg(sandbox.root())
            .output()
            .unwrap();
        let listing = String::from_utf8_lossy(&listing.stdout).into_owned();
        for name in listing.split_whitespace().filter(|word| word.contains('~')) {
            assert!(
                disk.get(name).await.is_err(),
                "{name} reads a file on the volume's own say-so"
            );
            let inner = format!("{name}/inner.txt");
            assert!(disk.get(&inner).await.is_err(), "{inner}");
        }
        assert_eq!(disk.get("averylongfilename.txt").await.unwrap(), "LONG");
        assert_eq!(
            disk.get("averylongdirectory/inner.txt").await.unwrap(),
            "INNER"
        );
    }

    #[tokio::test]
    async fn names_made_outside_the_disk_that_no_call_accepts_are_not_listed() {
        let sandbox = Sandbox::new();
        let disk = sandbox.disk();
        disk.put("kept.txt", "k").await.unwrap();
        // A name with `~1`, and one with a percent escape of its own.
        std::fs::write(sandbox.root().join("report~1.txt"), "x").unwrap();
        std::fs::write(sandbox.root().join("100%25x"), "x").unwrap();
        assert_eq!(disk.list("").await.unwrap(), ["kept.txt"]);
    }
}
