//! The local disk on the filesystem: where hostile paths end up, symlinks,
//! hidden upload files, directories, cleanup, atomic writes and listing.

use std::path::{Path, PathBuf};
use std::time::Duration;

use bytes::Bytes;
use futures_util::{StreamExt, TryStreamExt, stream};
use navree_storage::{Disk, DiskConfig, Storage, StorageError, StorageOptions};

use crate::support::{SIGNING_KEY, storage};

/// A temporary directory with a `root` for the disk inside it, so that
/// anything written outside the root shows up next to it.
struct Sandbox {
    outer: tempfile::TempDir,
}

impl Sandbox {
    fn new() -> Self {
        let outer = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        std::fs::write(outer.path().join("secret.txt"), "top secret").unwrap();
        Self { outer }
    }

    fn root(&self) -> PathBuf {
        self.outer.path().join("root")
    }

    fn disk(&self) -> Disk {
        storage(&self.root()).disk("local").unwrap()
    }

    /// Everything outside the root is as it was.
    fn assert_untouched(&self) {
        let mut names: Vec<String> = std::fs::read_dir(self.outer.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["root", "secret.txt"]);
        assert_eq!(
            std::fs::read_to_string(self.outer.path().join("secret.txt")).unwrap(),
            "top secret"
        );
    }
}

/// Every file under `dir`, as paths relative to it with `/` separators.
fn files_under(dir: &Path) -> Vec<String> {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                walk(&path, base, out);
            } else {
                let relative = path.strip_prefix(base).unwrap();
                let parts: Vec<_> = relative
                    .components()
                    .map(|part| part.as_os_str().to_string_lossy().into_owned())
                    .collect();
                out.push(parts.join("/"));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

#[tokio::test]
async fn paths_that_look_absolute_or_encoded_stay_under_the_root() {
    let sandbox = Sandbox::new();
    let disk = sandbox.disk();
    // Drive letters, UNC shares, rooted paths, percent-encoded and shell
    // forms: none of them is more than a name under the root.
    let paths = [
        "C:/Windows/system.ini",
        r"C:\Windows\system.ini",
        r"\\server\share\x.txt",
        "//server/share/x.txt",
        "/etc/passwd",
        r"\etc\passwd",
        "%2e%2e/%2e%2e/secret.txt",
        "..%2fsecret.txt",
        "%2e%2e%2fsecret.txt",
        "..%5csecret.txt",
        "a/..%2f..%2fsecret.txt",
        "%252e%252e/x",
        "~/x",
        "$HOME/x",
        "a/b/..c/d",
    ];
    for path in paths {
        disk.put(path, path).await.unwrap();
        assert_eq!(disk.get(path).await.unwrap(), path, "{path:?}");
        assert!(disk.exists(path).await.unwrap(), "{path:?}");
        assert_eq!(disk.size(path).await.unwrap(), path.len() as u64);
    }
    sandbox.assert_untouched();
    // One file for each distinct path (a backslash is a separator, a
    // leading one is dropped), all under the root.
    let distinct: std::collections::BTreeSet<String> = paths
        .iter()
        .map(|path| path.replace('\\', "/").trim_start_matches('/').to_owned())
        .collect();
    assert_eq!(files_under(&sandbox.root()).len(), distinct.len());
    assert_eq!(disk.list("").await.unwrap().len(), distinct.len());
    for path in paths {
        disk.delete(path).await.unwrap();
        assert!(!disk.exists(path).await.unwrap(), "{path:?}");
    }
    assert!(files_under(&sandbox.root()).is_empty());
    sandbox.assert_untouched();
}

#[tokio::test]
async fn names_a_windows_file_cannot_have_stay_under_the_root_or_are_refused() {
    let sandbox = Sandbox::new();
    let disk = sandbox.disk();
    // `?`, `*`, `<`, `>`, `|` and `"` are not legal in a Windows file name,
    // so there the backend refuses the write; elsewhere they are ordinary
    // characters. Either way nothing lands outside the root.
    for name in [
        r"\\?\C:\Windows\x.txt",
        "a?b",
        "a*b",
        "a<b",
        "a>b",
        "a|b",
        "a\"b",
    ] {
        let result = disk.put(name, "x").await;
        if cfg!(windows) {
            assert!(
                matches!(result, Err(StorageError::Backend { .. })),
                "{name}: {result:?}"
            );
        } else {
            result.unwrap();
            assert_eq!(disk.get(name).await.unwrap(), "x");
        }
        sandbox.assert_untouched();
    }
}

#[tokio::test]
async fn a_drive_letter_is_a_directory_name_under_the_root() {
    let sandbox = Sandbox::new();
    let disk = sandbox.disk();
    disk.put("C:/Windows/system.ini", "x").await.unwrap();
    // Where Windows would take `:` for a drive or a stream, the name has it
    // written as `%3A`; elsewhere a colon is an ordinary character.
    let directory = if cfg!(windows) { "C%3A" } else { "C:" };
    assert_eq!(
        files_under(&sandbox.root()),
        [format!("{directory}/Windows/system.ini")]
    );
    assert_eq!(
        disk.list("").await.unwrap(),
        [format!("{directory}/Windows/system.ini")]
    );
    assert_eq!(disk.get("C:/Windows/system.ini").await.unwrap(), "x");
    assert_eq!(disk.get("C:\\Windows\\system.ini").await.unwrap(), "x");
}

#[tokio::test]
async fn encoded_dots_are_ordinary_names() {
    let sandbox = Sandbox::new();
    let disk = sandbox.disk();
    disk.put("%2e%2e/secret.txt", "mine").await.unwrap();
    assert_eq!(
        std::fs::read_to_string(sandbox.root().join("%2e%2e").join("secret.txt")).unwrap(),
        "mine"
    );
    assert_eq!(
        std::fs::read_to_string(sandbox.outer.path().join("secret.txt")).unwrap(),
        "top secret"
    );
    assert_eq!(disk.list("").await.unwrap(), ["%2e%2e/secret.txt"]);
}

#[tokio::test]
async fn names_with_percent_signs_and_unicode_round_trip() {
    let sandbox = Sandbox::new();
    let disk = sandbox.disk();
    let paths = [
        "L%3ABC.parquet",
        "100% done.txt",
        "caf\u{e9}/\u{4e2d}\u{6587}.txt",
        "emoji \u{1f600}.txt",
        ".hidden/.also",
        "a b/c d.txt",
    ];
    for path in paths {
        disk.put(path, path).await.unwrap();
    }
    let mut listed = disk.list("").await.unwrap();
    listed.sort();
    let mut expected: Vec<String> = paths.iter().map(|p| (*p).to_owned()).collect();
    expected.sort();
    assert_eq!(listed, expected);
    for path in paths {
        assert_eq!(disk.get(path).await.unwrap(), path);
    }
}

#[tokio::test]
async fn files_named_like_uploads_in_progress_are_refused_and_never_listed() {
    let sandbox = Sandbox::new();
    let disk = sandbox.disk();
    disk.put("reports/q1.csv", "a").await.unwrap();
    std::fs::write(sandbox.root().join("reports").join("q2.csv#1"), "half").unwrap();
    std::fs::write(sandbox.root().join("loose#42"), "half").unwrap();

    // Listing skips them.
    assert_eq!(disk.list("").await.unwrap(), ["reports/q1.csv"]);
    // Naming one is an error from the backend, for every operation.
    for path in ["reports/q2.csv#1", "loose#42", "new#7"] {
        let expected = |error: StorageError| {
            assert!(matches!(error, StorageError::Backend { .. }), "{error:?}");
            assert!(error.to_string().contains("not supported"), "{error}");
            assert!(error.to_string().contains(path), "{error}");
        };
        expected(disk.put(path, "x").await.unwrap_err());
        expected(disk.get(path).await.unwrap_err());
        expected(disk.size(path).await.unwrap_err());
        expected(disk.delete(path).await.unwrap_err());
    }
    // A `#` that is not followed only by digits is a name like any other.
    for path in ["notes#draft", "notes#1a", "notes#", "notes#1/inside.txt"] {
        disk.put(path, "ok").await.unwrap();
        assert_eq!(disk.get(path).await.unwrap(), "ok", "{path}");
    }
    assert_eq!(
        disk.list("").await.unwrap(),
        [
            "notes#",
            "notes#1/inside.txt",
            "notes#1a",
            "notes#draft",
            "reports/q1.csv"
        ]
    );
}

#[tokio::test]
async fn a_write_leaves_no_temporary_file_behind() {
    let sandbox = Sandbox::new();
    let disk = sandbox.disk();
    disk.put("a/b.txt", "one").await.unwrap();
    disk.put("a/b.txt", "two").await.unwrap();
    disk.put_stream(
        "a/c.txt",
        stream::iter([Ok::<_, std::io::Error>(Bytes::from_static(b"stream"))]),
    )
    .await
    .unwrap();
    assert_eq!(files_under(&sandbox.root()), ["a/b.txt", "a/c.txt"]);

    // A failed stream, and a stream dropped half way, leave nothing either.
    let failing = stream::iter([
        Ok(Bytes::from_static(b"partial")),
        Err(std::io::Error::other("client went away")),
    ]);
    assert!(disk.put_stream("a/d.txt", failing).await.is_err());
    let never_ends = stream::iter([Ok::<_, std::io::Error>(Bytes::from_static(b"partial"))])
        .chain(stream::pending());
    let timed_out = tokio::time::timeout(
        Duration::from_millis(200),
        disk.put_stream("a/e.txt", never_ends),
    )
    .await;
    assert!(timed_out.is_err());
    // The file the dropped upload staged is removed in the background.
    for _ in 0..100 {
        if files_under(&sandbox.root()) == ["a/b.txt", "a/c.txt"] {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(files_under(&sandbox.root()), ["a/b.txt", "a/c.txt"]);
    assert!(!disk.exists("a/d.txt").await.unwrap());
    assert!(!disk.exists("a/e.txt").await.unwrap());
}

#[tokio::test]
async fn deleting_a_file_removes_the_directories_it_leaves_empty() {
    let sandbox = Sandbox::new();
    let disk = sandbox.disk();
    disk.put("a/b/c/one.txt", "1").await.unwrap();
    disk.put("a/two.txt", "2").await.unwrap();
    disk.delete("a/b/c/one.txt").await.unwrap();
    assert!(!sandbox.root().join("a").join("b").exists());
    assert!(sandbox.root().join("a").join("two.txt").exists());
    disk.delete("a/two.txt").await.unwrap();
    assert!(!sandbox.root().join("a").exists());
    assert!(sandbox.root().exists(), "the root itself stays");
    // A missing file is not an error, and does not disturb anything.
    disk.delete("a/two.txt").await.unwrap();
    disk.delete("never/was.txt").await.unwrap();
}

#[tokio::test]
async fn directories_are_not_files() {
    let sandbox = Sandbox::new();
    let disk = sandbox.disk();
    disk.put("dir/file.txt", "x").await.unwrap();

    // Looking at a directory as a file is a missing file where the
    // filesystem lets a directory be opened, and a refusal of the backend
    // where it does not.
    let errors = [
        disk.get("dir").await.unwrap_err(),
        disk.get_stream("dir").await.err().unwrap(),
        disk.size("dir").await.unwrap_err(),
        disk.metadata("dir").await.unwrap_err(),
    ];
    for error in errors {
        if cfg!(windows) {
            assert!(matches!(error, StorageError::Backend { .. }), "{error:?}");
        } else {
            assert!(matches!(error, StorageError::NotFound { .. }), "{error:?}");
        }
    }
    if cfg!(windows) {
        assert!(disk.exists("dir").await.is_err());
    } else {
        assert!(!disk.exists("dir").await.unwrap());
    }
    // Deleting a directory, writing over one, and writing under a file fail
    // and leave everything as it was.
    assert!(matches!(
        disk.delete("dir").await.unwrap_err(),
        StorageError::Backend { .. }
    ));
    assert!(matches!(
        disk.put("dir", "x").await.unwrap_err(),
        StorageError::Backend { .. }
    ));
    assert!(matches!(
        disk.put("dir/file.txt/inside", "x").await.unwrap_err(),
        StorageError::Backend { .. }
    ));
    assert!(matches!(
        disk.put_stream(
            "dir",
            stream::iter([Ok::<_, std::io::Error>(Bytes::from_static(b"x"))])
        )
        .await
        .unwrap_err(),
        StorageError::Backend { .. }
    ));
    let published: Vec<String> = files_under(&sandbox.root())
        .into_iter()
        .filter(|name| !name.contains('#'))
        .collect();
    assert_eq!(published, ["dir/file.txt"]);
    assert_eq!(disk.get("dir/file.txt").await.unwrap(), "x");
}

#[tokio::test]
async fn listing_covers_hidden_files_and_skips_empty_directories() {
    let sandbox = Sandbox::new();
    let disk = sandbox.disk();
    std::fs::create_dir_all(sandbox.root().join("empty").join("deeper")).unwrap();
    disk.put(".hidden", "h").await.unwrap();
    disk.put("dir/.dot/file", "f").await.unwrap();
    disk.put("dir/zeta.txt", "z").await.unwrap();
    disk.put("dir/Alpha.txt", "a").await.unwrap();

    assert_eq!(
        disk.list("").await.unwrap(),
        [".hidden", "dir/.dot/file", "dir/Alpha.txt", "dir/zeta.txt"]
    );
    assert!(disk.list("empty").await.unwrap().is_empty());
    // A file is not a directory to list.
    assert!(disk.list("dir/zeta.txt").await.unwrap().is_empty());

    let files = disk.files("dir").await.unwrap();
    let paths: Vec<_> = files.iter().map(|file| file.path.as_str()).collect();
    assert_eq!(paths, ["dir/.dot/file", "dir/Alpha.txt", "dir/zeta.txt"]);
    assert_eq!(files[0].size, 1);
}

#[tokio::test]
async fn metadata_has_the_size_and_the_modification_time() {
    let sandbox = Sandbox::new();
    let disk = sandbox.disk();
    let before = chrono::Utc::now() - chrono::Duration::seconds(5);
    disk.put("m/file.bin", vec![7u8; 1234]).await.unwrap();
    let meta = disk.metadata("m/file.bin").await.unwrap();
    assert_eq!(meta.path, "m/file.bin");
    assert_eq!(meta.size, 1234);
    assert!(meta.last_modified >= before, "{meta:?}");
    assert!(
        meta.last_modified <= chrono::Utc::now() + chrono::Duration::seconds(5),
        "{meta:?}"
    );
    let listed = disk.files("m").await.unwrap();
    assert_eq!(listed, [meta]);
    // An empty file has a size of zero.
    disk.put("m/empty", Bytes::new()).await.unwrap();
    assert_eq!(disk.size("m/empty").await.unwrap(), 0);
    assert_eq!(disk.get("m/empty").await.unwrap(), Bytes::new());
    assert!(
        disk.get_stream("m/empty")
            .await
            .unwrap()
            .try_collect::<Vec<_>>()
            .await
            .unwrap()
            .iter()
            .all(|chunk| chunk.is_empty())
    );
}

#[tokio::test]
async fn large_files_stream_in_and_out_unchanged() {
    let sandbox = Sandbox::new();
    let disk = sandbox.disk();
    // 12 MiB in odd-sized chunks: more than one part of an upload.
    let total: usize = 12 * 1024 * 1024;
    let bytes: Vec<u8> = (0..total).map(|i| (i % 251) as u8).collect();
    let chunks: Vec<Result<Bytes, std::io::Error>> = bytes
        .chunks(100_003)
        .map(|chunk| Ok(Bytes::copy_from_slice(chunk)))
        .collect();
    let written = disk
        .put_stream("big/data.bin", stream::iter(chunks))
        .await
        .unwrap();
    assert_eq!(written, total as u64);
    assert_eq!(disk.size("big/data.bin").await.unwrap(), total as u64);
    let read: Vec<u8> = disk
        .get_stream("big/data.bin")
        .await
        .unwrap()
        .try_fold(Vec::new(), |mut all, chunk| async move {
            all.extend_from_slice(&chunk);
            Ok(all)
        })
        .await
        .unwrap();
    assert_eq!(read.len(), total);
    assert!(read == bytes, "the streamed file differs");
    assert!(disk.get("big/data.bin").await.unwrap() == Bytes::from(bytes));
}

#[tokio::test]
async fn a_reader_sees_the_old_file_or_the_new_one_never_a_mix() {
    let sandbox = Sandbox::new();
    let disk = sandbox.disk();
    let size = 512 * 1024;
    disk.put("swap.bin", vec![0u8; size]).await.unwrap();

    let writer = {
        let disk = disk.clone();
        tokio::spawn(async move {
            for round in 1..=40u8 {
                disk.put("swap.bin", vec![round; size]).await.unwrap();
            }
        })
    };
    let reader = {
        let disk = disk.clone();
        tokio::spawn(async move {
            let mut reads = 0;
            while reads < 200 {
                let bytes = disk.get("swap.bin").await.unwrap();
                assert_eq!(bytes.len(), size);
                let first = bytes[0];
                assert!(bytes.iter().all(|&b| b == first), "a torn read");
                reads += 1;
            }
        })
    };
    writer.await.unwrap();
    reader.await.unwrap();
    assert_eq!(disk.get("swap.bin").await.unwrap(), vec![40u8; size]);
    assert_eq!(files_under(&sandbox.root()), ["swap.bin"]);
}

#[tokio::test]
async fn the_root_is_created_when_missing_and_must_be_a_directory() {
    let sandbox = Sandbox::new();
    let nested = sandbox.outer.path().join("not").join("yet").join("there");
    let options = |root: &Path| StorageOptions {
        default: "local".to_owned(),
        disks: [("local".to_owned(), DiskConfig::local(root))].into(),
        ..StorageOptions::default()
    };
    let storage = Storage::new(&options(&nested), SIGNING_KEY).unwrap();
    storage
        .disk("local")
        .unwrap()
        .put("a.txt", "x")
        .await
        .unwrap();
    assert!(nested.join("a.txt").exists());

    let file = sandbox.outer.path().join("secret.txt");
    let error = Storage::new(&options(&file), SIGNING_KEY).err().unwrap();
    assert!(matches!(error, StorageError::LocalRoot { .. }), "{error:?}");
}

#[cfg(unix)]
mod symlinks {
    use std::os::unix::fs::symlink;

    use super::*;

    #[tokio::test]
    async fn symlinks_inside_the_root_are_followed() {
        let sandbox = Sandbox::new();
        let disk = sandbox.disk();
        disk.put("real/a.txt", "a").await.unwrap();
        let root = sandbox.root();
        symlink(root.join("real"), root.join("linked")).unwrap();
        symlink(root.join("real").join("a.txt"), root.join("alias.txt")).unwrap();

        assert_eq!(disk.get("linked/a.txt").await.unwrap(), "a");
        assert_eq!(disk.get("alias.txt").await.unwrap(), "a");
        assert!(disk.exists("linked/a.txt").await.unwrap());
        assert_eq!(disk.size("alias.txt").await.unwrap(), 1);
        assert_eq!(
            disk.list("").await.unwrap(),
            ["alias.txt", "linked/a.txt", "real/a.txt"]
        );
        assert_eq!(disk.list("linked").await.unwrap(), ["linked/a.txt"]);
        // Writing through a link writes to its target.
        disk.put("linked/b.txt", "b").await.unwrap();
        assert!(root.join("real").join("b.txt").exists());
    }

    #[tokio::test]
    async fn broken_symlinks_are_missing_files() {
        let sandbox = Sandbox::new();
        let disk = sandbox.disk();
        disk.put("kept.txt", "k").await.unwrap();
        let root = sandbox.root();
        symlink(root.join("nowhere"), root.join("dangling")).unwrap();

        assert_eq!(disk.list("").await.unwrap(), ["kept.txt"]);
        assert!(!disk.exists("dangling").await.unwrap());
        assert!(matches!(
            disk.get("dangling").await.unwrap_err(),
            StorageError::NotFound { .. }
        ));
    }

    #[tokio::test]
    async fn a_symlink_out_of_the_root_is_followed_like_any_other() {
        let sandbox = Sandbox::new();
        let disk = sandbox.disk();
        disk.put("kept.txt", "k").await.unwrap();
        // The operator put this link there; the disk does not second-guess it.
        let secret = sandbox.outer.path().join("secret.txt");
        symlink(&secret, sandbox.root().join("out.txt")).unwrap();
        assert_eq!(disk.get("out.txt").await.unwrap(), "top secret");
        assert_eq!(disk.list("").await.unwrap(), ["kept.txt", "out.txt"]);
    }

    #[tokio::test]
    async fn a_symlink_loop_fails_the_listing_but_not_the_other_files() {
        let sandbox = Sandbox::new();
        let disk = sandbox.disk();
        disk.put("kept.txt", "k").await.unwrap();
        let root = sandbox.root();
        symlink(&root, root.join("loop")).unwrap();

        let error = disk.list("").await.unwrap_err();
        assert!(matches!(error, StorageError::Backend { .. }), "{error:?}");
        assert_eq!(disk.get("kept.txt").await.unwrap(), "k");
        assert!(disk.list("nothing").await.unwrap().is_empty());
    }
}
