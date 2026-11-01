//! Where template sources come from: template directories and embedded
//! templates, and the loader used when reloading.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use navree_template::{Error, ErrorKind, TemplateSource};

use crate::ViewError;

/// File extensions that make a file in a template directory a template.
pub(crate) const TEMPLATE_EXTENSIONS: [&str; 7] =
    ["html", "htm", "xml", "txt", "md", "j2", "jinja"];

/// Template directories (searched in order) over embedded templates.
#[derive(Debug)]
pub(crate) struct Sources {
    dirs: Vec<PathBuf>,
    embedded: BTreeMap<String, String>,
}

impl Sources {
    pub(crate) fn new(dirs: Vec<PathBuf>, embedded: BTreeMap<String, String>) -> Self {
        Self { dirs, embedded }
    }

    pub(crate) fn dirs(&self) -> &[PathBuf] {
        &self.dirs
    }

    /// Every template name and its source, read now: files (the first
    /// directory holding a name wins) over embedded templates.
    pub(crate) fn read_all(&self) -> Result<BTreeMap<String, String>, ViewError> {
        let mut all = BTreeMap::new();
        for (name, path) in self.files()? {
            let source = fs::read_to_string(&path).map_err(|source| ViewError::Io {
                path: path.clone(),
                source,
            })?;
            all.insert(name, source);
        }
        for (name, source) in &self.embedded {
            all.entry(name.clone()).or_insert_with(|| source.clone());
        }
        Ok(all)
    }

    /// Every template name currently available (files and embedded).
    pub(crate) fn names(&self) -> Result<Vec<String>, ViewError> {
        let mut names: Vec<String> = self.files()?.into_keys().collect();
        names.extend(self.embedded.keys().cloned());
        names.sort();
        names.dedup();
        Ok(names)
    }

    /// Every template file, by name; the first directory holding a name wins.
    fn files(&self) -> Result<BTreeMap<String, PathBuf>, ViewError> {
        let mut files = BTreeMap::new();
        for dir in &self.dirs {
            if !dir.exists() {
                tracing::debug!(dir = %dir.display(), "template directory does not exist; skipped");
                continue;
            }
            let mut found = Vec::new();
            walk(dir, "", &mut found, &mut HashSet::new())?;
            for (name, path) in found {
                files.entry(name).or_insert(path);
            }
        }
        Ok(files)
    }

    /// The file that provides template `name`, if any.
    fn file(&self, name: &str) -> Option<PathBuf> {
        if !is_template_name(name) {
            return None;
        }
        self.dirs
            .iter()
            .filter_map(|dir| safe_join(dir, name))
            .find(|path| path.is_file())
    }

    /// The loader used when reloading: reads `name` from disk (or the
    /// embedded templates) with an up-to-date check, so a changed, added or
    /// removed file is picked up on the next lookup.
    pub(crate) fn load(self: &Arc<Self>, name: &str) -> Result<Option<TemplateSource>, Error> {
        if let Some(path) = self.file(name) {
            let stamp = stamp(&path);
            let source = fs::read_to_string(&path).map_err(|error| {
                Error::new(ErrorKind::InvalidOperation, "cannot read template file")
                    .with_source(error)
            })?;
            let sources = self.clone();
            let name = name.to_owned();
            return Ok(Some(TemplateSource::new(source).with_uptodate_check(
                move || sources.file(&name).as_ref() == Some(&path) && self::stamp(&path) == stamp,
            )));
        }
        if let Some(source) = self.embedded.get(name) {
            let sources = self.clone();
            let name = name.to_owned();
            return Ok(Some(
                TemplateSource::new(source.clone())
                    .with_uptodate_check(move || sources.file(&name).is_none()),
            ));
        }
        Ok(None)
    }
}

/// Whether the last segment of `name` ends in one of [`TEMPLATE_EXTENSIONS`].
pub(crate) fn is_template_name(name: &str) -> bool {
    let file = name.rsplit('/').next().unwrap_or(name);
    file.rsplit_once('.')
        .is_some_and(|(_, extension)| TEMPLATE_EXTENSIONS.contains(&extension))
}

/// `dir` joined with the `/`-separated template `name`, or `None` when the
/// name could leave `dir` or reach a file the walk would not list: empty,
/// hidden (`.`, `..`) or drive segments, backslashes, an absolute path, or on
/// Windows another name for a file.
fn safe_join(dir: &Path, name: &str) -> Option<PathBuf> {
    let mut path = dir.to_path_buf();
    for segment in name.split('/') {
        if segment.is_empty()
            || segment.starts_with('.')
            || segment.contains(['\\', ':'])
            || (cfg!(windows) && is_windows_alias(segment))
        {
            return None;
        }
        path.push(segment);
    }
    Some(path)
}

/// Whether Windows could take this path segment for another name: an 8.3 short
/// name, a trailing dot or space (ignored when opening), a device name (with
/// any extension), or an alternate stream.
fn is_windows_alias(segment: &str) -> bool {
    segment.ends_with(['.', ' '])
        || segment.contains(':')
        || is_short_name(segment)
        || is_device_name(segment)
}

/// Whether Windows could have made `segment` as the short name of another
/// file: `~` and digits at the end of a base of at most 8 characters, with at
/// most one extension of at most 3.
fn is_short_name(segment: &str) -> bool {
    let (base, extension) = segment.split_once('.').unwrap_or((segment, ""));
    let Some((before, digits)) = base.rsplit_once('~') else {
        return false;
    };
    !before.is_empty()
        && !digits.is_empty()
        && digits.bytes().all(|byte| byte.is_ascii_digit())
        && base.chars().count() <= 8
        && !extension.contains('.')
        && extension.chars().count() <= 3
}

/// Whether `segment` is the name of a Windows device: `CON`, `PRN`, `AUX`,
/// `NUL`, `CONIN$`, `CONOUT$`, `CLOCK$`, `COM` and `LPT` with a digit, in any
/// case, followed by an extension or spaces or nothing.
fn is_device_name(segment: &str) -> bool {
    let base = segment
        .split('.')
        .next()
        .unwrap_or(segment)
        .trim_end_matches(' ');
    let upper = base.to_uppercase();
    if matches!(
        upper.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$" | "CLOCK$"
    ) {
        return true;
    }
    let mut letters = upper.chars();
    let prefix: String = letters.by_ref().take(3).collect();
    matches!(prefix.as_str(), "COM" | "LPT")
        && matches!(
            (letters.next(), letters.next()),
            (Some('0'..='9' | '\u{b9}' | '\u{b2}' | '\u{b3}'), None)
        )
}

/// Modification time and size: what the up-to-date check compares.
fn stamp(path: &Path) -> Option<(SystemTime, u64)> {
    let metadata = fs::metadata(path).ok()?;
    Some((metadata.modified().ok()?, metadata.len()))
}

/// Collects the template files under `dir` as (`/`-separated name, path),
/// skipping hidden entries, names a template name cannot hold and files
/// without a template extension. Links are followed, but a directory already
/// walked (`visited`, by canonical path) is not walked again, so a link loop
/// ends.
fn walk(
    dir: &Path,
    prefix: &str,
    found: &mut Vec<(String, PathBuf)>,
    visited: &mut HashSet<PathBuf>,
) -> Result<(), ViewError> {
    let io_error = |source| ViewError::Io {
        path: dir.to_path_buf(),
        source,
    };
    if !visited.insert(fs::canonicalize(dir).map_err(io_error)?) {
        return Ok(());
    }
    for entry in fs::read_dir(dir).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let file_name = entry.file_name();
        // Hidden entries, names that are not valid UTF-8 and names a template
        // name cannot hold (see `safe_join`) are not templates.
        let Some(file_name) = file_name
            .to_str()
            .filter(|n| !n.starts_with('.') && !n.contains(['\\', ':']))
            .filter(|n| !(cfg!(windows) && is_windows_alias(n)))
        else {
            continue;
        };
        let path = entry.path();
        let name = format!("{prefix}{file_name}");
        if path.is_dir() {
            walk(&path, &format!("{name}/"), found, visited)?;
        } else if path.is_file() && is_template_name(&name) {
            found.push((name, path));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_join_rejects_names_that_leave_the_directory() {
        let dir = Path::new("views");
        assert_eq!(
            safe_join(dir, "mail/welcome.html"),
            Some(dir.join("mail").join("welcome.html"))
        );
        for name in [
            "",
            "/etc/passwd",
            "../secret.html",
            "mail/../../secret.html",
            "mail//welcome.html",
            ".hidden/a.html",
            r"..\secret.html",
            "C:/Windows/win.ini",
            "mail/C:secret.html",
        ] {
            assert_eq!(safe_join(dir, name), None, "{name}");
        }
    }

    #[test]
    fn a_name_with_a_character_across_its_third_byte_is_a_name() {
        for segment in [
            "ab\u{e9}.html",
            "\u{e9}\u{65e5}.html",
            "se\u{f1}or.html",
            "na\u{ef}ve.html",
        ] {
            assert!(!is_windows_alias(segment), "{segment}");
        }
        for segment in [
            "COM\u{b9}",
            "com\u{b2}.html",
            "LPT\u{b3}.txt",
            "COM0",
            "NUL .html",
            "CLOCK$",
            "clock$.html",
        ] {
            assert!(is_windows_alias(segment), "{segment}");
        }
    }

    #[test]
    fn only_what_a_short_name_can_be_is_taken_for_one() {
        for segment in ["AVERYL~1.HTM", "a~12.htm", "HIDDEN~1"] {
            assert!(is_windows_alias(segment), "{segment}");
        }
        for segment in ["notes~2024.html", "a~1.b.html", "page~1x.html", "a~.html"] {
            assert!(!is_windows_alias(segment), "{segment}");
        }
    }

    #[test]
    fn names_windows_also_answers_to_are_recognized_everywhere() {
        for segment in [
            "HIDDEN~1",
            "AVERYL~1.HTM",
            "a~12.htm",
            "page.html.",
            "page.html ",
            "dir.",
            "dir ",
            "NUL",
            "nul.html",
            "CON.txt",
            "com1.html",
            "LPT9",
            "CONIN$",
            "page.html:$DATA",
        ] {
            assert!(is_windows_alias(segment), "{segment}");
        }
        for segment in [
            "page.html",
            "a~b.html",
            "a~.html",
            "~1a.html",
            "console.html",
            "nullable.html",
            "com10.html",
            "mail",
        ] {
            assert!(!is_windows_alias(segment), "{segment}");
        }
    }
}
