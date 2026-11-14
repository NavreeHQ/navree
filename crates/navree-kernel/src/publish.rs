//! Publishing the files a plugin ships into the app: the rule every published
//! path follows, and the checked writer every publishing command writes
//! through.

use std::collections::HashSet;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

/// Whether `path` is a safe relative path inside a publish folder: non-empty,
/// at most 255 bytes, with `/` as the only separator; every segment non-empty,
/// at most 128 bytes and neither `.` nor `..`; no `\`, `:` or control
/// character; none of `<>"|?*`; no segment ending in `.` or a space; no
/// segment whose part before its first `.`, without trailing spaces, is
/// `CON`, `PRN`, `AUX`, `NUL`, `CONIN$`, `CONOUT$`, `COM0` to `COM9`,
/// `LPT0` to `LPT9`, or `COM` or `LPT` followed by `¹`, `²` or `³`, in any
/// case. Not public API; the boot check, `plugin:publish` and Navree's own
/// macros use it.
pub fn is_safe_relative_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 255
        && !path.chars().any(is_refused_char)
        && path.split('/').all(is_safe_segment)
}

/// A character no published path holds: `\`, `:`, `<>"|?*` or a control
/// character.
fn is_refused_char(c: char) -> bool {
    c.is_control() || matches!(c, '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*')
}

/// Whether one segment of a published path is safe (see
/// [`is_safe_relative_path`]).
fn is_safe_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment.len() <= 128
        && segment != "."
        && segment != ".."
        && !segment.ends_with('.')
        && !segment.ends_with(' ')
        && !is_device_name(segment)
}

/// Whether `segment` names a Windows device: its part before the first `.`,
/// without trailing spaces, is `CON`, `PRN`, `AUX`, `NUL`, `CONIN$`,
/// `CONOUT$`, `COM0` to `COM9`, `LPT0` to `LPT9`, or `COM` or `LPT` followed
/// by a superscript one, two or three (`¹`, `²`, `³`), ignoring ASCII case.
fn is_device_name(segment: &str) -> bool {
    let stem = segment
        .split_once('.')
        .map_or(segment, |(stem, _)| stem)
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    match stem.as_bytes() {
        b"CON" | b"PRN" | b"AUX" | b"NUL" | b"CONIN$" | b"CONOUT$" => true,
        [b'C', b'O', b'M', digit] | [b'L', b'P', b'T', digit] => digit.is_ascii_digit(),
        // `¹` (U+00B9), `²` (U+00B2) and `³` (U+00B3) are `C2 B9`, `C2 B2`
        // and `C2 B3` in UTF-8.
        [b'C', b'O', b'M', 0xC2, last] | [b'L', b'P', b'T', 0xC2, last] => {
            matches!(*last, 0xB9 | 0xB2 | 0xB3)
        }
        _ => false,
    }
}

/// Whether `path`, a file under the `locales` tag, starts with a locale: at
/// least two segments, the first a language of 2 to 8 letters followed by any
/// number of `-` and 1 to 8 letters or digits (`en`, `pa-IN`, `zh-Hant-TW`).
pub(crate) fn is_locale_path(path: &str) -> bool {
    let Some((locale, _)) = path.split_once('/') else {
        return false;
    };
    let mut parts = locale.split('-');
    parts.next().is_some_and(|language| {
        (2..=8).contains(&language.len()) && language.bytes().all(|b| b.is_ascii_alphabetic())
    }) && parts.all(|part| {
        (1..=8).contains(&part.len()) && part.bytes().all(|b| b.is_ascii_alphanumeric())
    })
}

/// The first of `paths` that equals an earlier one once both are lower-cased,
/// or once both are upper-cased (file systems fold case differently, and
/// `ß` upper-cases to `SS`): on a disk that ignores case, the two are one
/// file. Names that differ only in how an accent is composed are not caught.
pub(crate) fn first_case_collision<'a>(
    paths: impl IntoIterator<Item = &'a str>,
) -> Option<&'a str> {
    let mut lower = HashSet::new();
    let mut upper = HashSet::new();
    paths.into_iter().find(|path| {
        // Both sets take the path, even when the first already had it.
        let seen_lower = !lower.insert(path.to_lowercase());
        let seen_upper = !upper.insert(path.to_uppercase());
        seen_lower || seen_upper
    })
}

/// What [`publish_files`] did with one file, or with a dry run would do. Not
/// public API; the commands that publish files use it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Published {
    /// The file was written: it was new, or it was replaced with `force`.
    Written,
    /// The file exists and was kept (no `force`).
    Kept,
    /// A dry run: the file is new and would be written.
    WouldBeWritten,
    /// A dry run: the file exists and would be kept (no `force`).
    WouldBeKept,
    /// A dry run: the file exists and would be replaced (`force`).
    WouldBeReplaced,
}

impl Published {
    /// The line a command prints for the file `path`: `<path>: written`,
    /// `<path>: exists, kept (use --force to replace it)`,
    /// `<path>: would be written`,
    /// `<path>: exists, would be kept (use --force to replace it)` or
    /// `<path>: exists, would be replaced`.
    pub fn line(&self, path: &str) -> String {
        let outcome = match self {
            Published::Written => "written",
            Published::Kept => "exists, kept (use --force to replace it)",
            Published::WouldBeWritten => "would be written",
            Published::WouldBeKept => "exists, would be kept (use --force to replace it)",
            Published::WouldBeReplaced => "exists, would be replaced",
        };
        format!("{path}: {outcome}")
    }
}

/// Why [`publish_files`] refused to write, or stopped writing: the text a
/// command prints after its `<command>: ` prefix. Not public API; the
/// commands that publish files use it.
#[derive(Debug)]
pub struct PublishRefused(String);

impl fmt::Display for PublishRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PublishRefused {}

/// Writes `files`, each a path relative to `root` (the app's folder),
/// `/`-separated, and its bytes, and says what happened to each, in path
/// order. The paths are distinct (the caller makes sure of it). Not public
/// API; `plugin:publish` and the commands that publish a plugin's
/// configuration or translations write through it, some without the boot
/// check, so it checks every path itself.
///
/// Nothing is written before every file has passed every check:
///
/// 1. every path is a safe relative path (the rule the boot applies to the
///    files a plugin ships);
/// 2. every folder below `root` on the way to a file is a folder, not a file
///    and not a symbolic link or a junction, and the file itself is not a
///    link and not a folder. `root` and the folders above it are not
///    checked, so an app inside a linked folder publishes.
///
/// An existing file is kept unless `force` is set. With `dry_run` nothing is
/// written and the result says what would be. A file is written as a new
/// file beside its destination and renamed over it: an existing file is
/// never opened for writing, so a hard link to it elsewhere keeps its
/// content, and a link put in its place after the check is replaced, not
/// followed. An error while writing stops at that file; the files before it
/// are written.
pub fn publish_files(
    root: &Path,
    files: &[(&str, &[u8])],
    force: bool,
    dry_run: bool,
) -> Result<Vec<(String, Published)>, PublishRefused> {
    let mut files = files.to_vec();
    files.sort_by_key(|&(path, _)| path);
    if let Some(&(path, _)) = files
        .iter()
        .find(|&&(path, _)| !is_safe_relative_path(path))
    {
        return Err(PublishRefused(format!(
            "{path} is not a safe relative path; nothing was written"
        )));
    }
    let mut outcomes = Vec::with_capacity(files.len());
    for &(path, _) in &files {
        let published = match (file_exists(root, path)?, force, dry_run) {
            (false, _, false) | (true, true, false) => Published::Written,
            (false, _, true) => Published::WouldBeWritten,
            (true, false, false) => Published::Kept,
            (true, false, true) => Published::WouldBeKept,
            (true, true, true) => Published::WouldBeReplaced,
        };
        outcomes.push((path.to_owned(), published));
    }
    if dry_run {
        return Ok(outcomes);
    }
    for (&(path, bytes), (_, published)) in files.iter().zip(&outcomes) {
        if *published == Published::Written {
            write_file(root, path, bytes)?;
        }
    }
    Ok(outcomes)
}

/// Whether the file `path` exists below `root`, once each folder on the way
/// to it has passed [`check_folder`] and the file itself is neither a link
/// nor a folder. The first missing folder ends the walk: the file is new.
fn file_exists(root: &Path, path: &str) -> Result<bool, PublishRefused> {
    for (end, _) in path.match_indices('/') {
        let folder = &path[..end];
        match fs::symlink_metadata(root.join(folder)) {
            Ok(metadata) => check_folder(folder, &metadata)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(cannot_write(path, &error)),
        }
    }
    match fs::symlink_metadata(root.join(path)) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(inside_a_link(path)),
        Ok(metadata) if metadata.is_dir() => Err(PublishRefused(format!(
            "{path} is a folder where a file would be written; nothing was written"
        ))),
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(cannot_write(path, &error)),
    }
}

/// Refuses `folder` (relative to the app's folder) when it is a symbolic link
/// or a junction (`is_symlink` holds for both), or not a folder.
fn check_folder(folder: &str, metadata: &fs::Metadata) -> Result<(), PublishRefused> {
    if metadata.file_type().is_symlink() {
        Err(inside_a_link(folder))
    } else if metadata.is_dir() {
        Ok(())
    } else {
        Err(PublishRefused(format!(
            "{folder} is a file where a folder is needed; nothing was written"
        )))
    }
}

/// Writes `bytes` to `path` below `root`: creates each missing folder (one
/// that turns out to exist is checked again), writes the bytes to a new file
/// beside the destination, and renames that file over the destination.
fn write_file(root: &Path, path: &str, bytes: &[u8]) -> Result<(), PublishRefused> {
    for (end, _) in path.match_indices('/') {
        let folder = &path[..end];
        match fs::create_dir(root.join(folder)) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let metadata = fs::symlink_metadata(root.join(folder))
                    .map_err(|error| cannot_write(path, &error))?;
                check_folder(folder, &metadata)?;
            }
            Err(error) => return Err(cannot_write(path, &error)),
        }
    }
    let destination = root.join(path);
    let name = path.rsplit_once('/').map_or(path, |(_, name)| name);
    let temporary = destination.with_file_name(format!(
        ".{name}.navree-publish-{}.tmp",
        std::process::id()
    ));
    // `create_new` never opens a file or a link that is already there.
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| cannot_write(path, &error))?;
    let written = file.write_all(bytes);
    // Closed before the rename, which Windows needs.
    drop(file);
    if let Err(error) = written.and_then(|()| fs::rename(&temporary, &destination)) {
        let _ = fs::remove_file(&temporary);
        return Err(cannot_write(path, &error));
    }
    Ok(())
}

/// `<path> is a symbolic link or inside one; nothing was written`.
fn inside_a_link(path: &str) -> PublishRefused {
    PublishRefused(format!(
        "{path} is a symbolic link or inside one; nothing was written"
    ))
}

/// `cannot write <path>: <error>`.
fn cannot_write(path: &str, error: &io::Error) -> PublishRefused {
    PublishRefused(format!("cannot write {path}: {error}"))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    /// A new, empty folder `navree-kernel-<name>-<process id>` in the
    /// system's temporary folder, removed on drop.
    struct TempFolder(PathBuf);

    impl TempFolder {
        fn new(name: &str) -> TempFolder {
            let path = std::env::temp_dir().join(format!(
                "navree-kernel-{name}-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("a temporary folder");
            TempFolder(path)
        }
    }

    impl Drop for TempFolder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// The line each outcome prints.
    fn lines(out: &[(String, Published)]) -> Vec<String> {
        out.iter()
            .map(|(path, published)| published.line(path))
            .collect()
    }

    /// Paths the rule accepts.
    fn accepted_paths() -> Vec<String> {
        let mut paths: Vec<String> = [
            "list.html",
            "admin/list.html",
            ".gitkeep",
            "a.b.c",
            "console.html",
            "con-list.html",
            "com10.css",
            "lpt.txt",
            "ਸੂਚਨਾ.html",
            "conin.html",
            "com⁴.css",
        ]
        .map(String::from)
        .to_vec();
        paths.push("a".repeat(128));
        paths.push(format!("{}/{}", "a".repeat(127), "b".repeat(127)));
        paths
    }

    /// Paths the rule refuses, one or more per shape.
    fn refused_paths() -> Vec<String> {
        let mut paths: Vec<String> = [
            "",
            "/",
            "/etc/navree.css",
            "../x",
            "a/../x",
            "./x",
            "a/./x",
            "a//b",
            "a/",
            "a\\b",
            "c:/x",
            "C:x",
            "banner.css:hidden",
            "a\u{1}b",
            "a\tb",
            "a\u{7f}b",
            "a<b",
            "a>b",
            "a\"b",
            "a|b",
            "a?b",
            "a*b",
            "list.html.",
            "list.html ",
            "...",
            ". ",
            "a/.../b",
            "con",
            "CON",
            "con.html",
            "Con.tar.gz",
            "nul",
            "aux.txt",
            "prn",
            "com0",
            "com1.css",
            "COM9",
            "lpt0",
            "lpt9",
            "Lpt5.log",
            "con .txt",
            "conin$",
            "CONIN$.log",
            "conout$",
            "CONOUT$.txt",
            "com¹",
            "emails/COM¹",
            "Com².css",
            "com³",
            "lpt¹",
            "lpt².html",
            "LPT³.txt",
        ]
        .map(String::from)
        .to_vec();
        paths.push("a".repeat(129));
        paths.push(format!("{}/{}", "a".repeat(128), "b".repeat(127)));
        paths
    }

    #[test]
    fn the_publish_path_rule_accepts_plain_paths_and_refuses_every_unsafe_shape() {
        for good in accepted_paths() {
            assert!(is_safe_relative_path(&good), "{good:?}");
        }
        for bad in refused_paths() {
            assert!(!is_safe_relative_path(&bad), "{bad:?}");
        }
        for good in [
            "en/announcements.ftl",
            "pa-IN/admin/announcements.ftl",
            "zh-Hant-TW/x.ftl",
        ] {
            assert!(is_locale_path(good), "{good}");
        }
        for bad in [
            "announcements.ftl",
            "e/x.ftl",
            "en_GB/x.ftl",
            "123/x.ftl",
            "en-/x.ftl",
        ] {
            assert!(!is_locale_path(bad), "{bad}");
        }
        assert_eq!(
            first_case_collision(["list.html", "show.html", "List.html"]),
            Some("List.html")
        );
        assert_eq!(
            first_case_collision(["list.html", "list.html"]),
            Some("list.html")
        );
        assert_eq!(first_case_collision(["a/list.html", "b/list.html"]), None);
        // `ß` upper-cases to `SS` but lower-cases to itself: only the
        // upper-case comparison catches this pair.
        assert_eq!(
            first_case_collision(["straße.html", "STRASSE.html"]),
            Some("STRASSE.html")
        );
        // The Kelvin sign lower-cases to `k` but upper-cases to itself: only
        // the lower-case comparison catches this pair.
        assert_eq!(
            first_case_collision(["k.html", "\u{212A}.html"]),
            Some("\u{212A}.html")
        );
        // Names that differ only in how an accent is composed (`é` as one
        // character, or `e` and a combining accent) are not caught: the
        // kernel has no normalisation table.
    }

    #[test]
    fn the_writer_writes_new_files_and_keeps_existing_ones_without_force() {
        let root = TempFolder::new("writer-keep");
        let banner = root.0.join("resources/announcements/banner.css");
        fs::create_dir_all(banner.parent().unwrap()).unwrap();
        fs::write(&banner, "/* Simran Kaur */").unwrap();
        let files = [
            ("resources/announcements/banner.css", &b"body{}"[..]),
            ("resources/announcements/print.css", &b"@media print{}"[..]),
        ];
        let out = publish_files(&root.0, &files, false, false).expect("published");
        assert_eq!(
            lines(&out),
            [
                "resources/announcements/banner.css: exists, kept (use --force to replace it)",
                "resources/announcements/print.css: written",
            ]
        );
        assert_eq!(fs::read_to_string(&banner).unwrap(), "/* Simran Kaur */");
        let out = publish_files(&root.0, &files, true, true).expect("dry run");
        assert_eq!(
            lines(&out),
            [
                "resources/announcements/banner.css: exists, would be replaced",
                "resources/announcements/print.css: exists, would be replaced",
            ]
        );
        assert_eq!(
            fs::read_to_string(&banner).unwrap(),
            "/* Simran Kaur */",
            "a dry run writes nothing"
        );
        fs::remove_file(root.0.join("resources/announcements/print.css")).unwrap();
        let out = publish_files(&root.0, &files, false, true).expect("dry run");
        assert_eq!(
            lines(&out),
            [
                "resources/announcements/banner.css: exists, would be kept (use --force to replace it)",
                "resources/announcements/print.css: would be written",
            ]
        );
        assert!(!root.0.join("resources/announcements/print.css").exists());
        let out = publish_files(&root.0, &files, true, false).expect("forced");
        assert_eq!(lines(&out)[0], "resources/announcements/banner.css: written");
        assert_eq!(fs::read_to_string(&banner).unwrap(), "body{}");
    }

    #[test]
    fn the_writer_checks_every_file_before_the_first_write() {
        let root = TempFolder::new("writer-check");
        let unsafe_late = [
            ("resources/a/one.css", &b"x"[..]),
            ("resources/a/../two.css", &b"y"[..]),
        ];
        let refused = publish_files(&root.0, &unsafe_late, false, false).expect_err("refused");
        assert_eq!(
            refused.to_string(),
            "resources/a/../two.css is not a safe relative path; nothing was written"
        );
        assert!(!root.0.join("resources").exists(), "nothing was written");
        fs::create_dir_all(root.0.join("resources")).unwrap();
        fs::write(root.0.join("resources/a"), "a file").unwrap();
        let refused =
            publish_files(&root.0, &unsafe_late[..1], false, false).expect_err("refused");
        assert_eq!(
            refused.to_string(),
            "resources/a is a file where a folder is needed; nothing was written"
        );
        fs::remove_file(root.0.join("resources/a")).unwrap();
        fs::create_dir_all(root.0.join("resources/a/one.css")).unwrap();
        let refused = publish_files(&root.0, &unsafe_late[..1], true, false).expect_err("refused");
        assert_eq!(
            refused.to_string(),
            "resources/a/one.css is a folder where a file would be written; nothing was written"
        );
    }

    /// `config:publish --plugin` and `i18n:publish <plugin>` reach the writer without the
    /// boot check, and a section key is free text: the writer refuses every shape itself.
    #[test]
    fn the_writer_refuses_every_unsafe_shape_itself() {
        let root = TempFolder::new("writer-shapes");
        for path in refused_paths() {
            for force in [false, true] {
                let refused = publish_files(&root.0, &[(path.as_str(), b"x")], force, false)
                    .expect_err(&path);
                assert_eq!(
                    refused.to_string(),
                    format!("{path} is not a safe relative path; nothing was written")
                );
            }
            assert_eq!(
                fs::read_dir(&root.0).unwrap().count(),
                0,
                "nothing was written for {path:?}"
            );
        }
    }

    /// The link check covers the app's folder and below it, not the folders above it.
    #[cfg(unix)]
    #[test]
    fn plugin_publish_works_in_an_app_inside_a_linked_folder() {
        let root = TempFolder::new("writer-linked-app");
        fs::create_dir(root.0.join("real")).unwrap();
        std::os::unix::fs::symlink(root.0.join("real"), root.0.join("app")).unwrap();
        publish_files(
            &root.0.join("app"),
            &[("resources/announcements/banner.css", b"body{}")],
            false,
            false,
        )
        .expect("an app inside a linked folder publishes");
        assert!(root.0.join("real/resources/announcements/banner.css").is_file());
    }
}
