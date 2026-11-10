//! [`Templates`]: the document template directories and the files modules
//! embed, seen as one tree.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bytes::Bytes;

/// The file extension of templates (both engines print HTML).
pub const HTML_EXTENSION: &str = "html";

/// The document templates: the files under the template directories
/// (`documents.templates`, searched in order) over the files modules embed
/// with [`ModuleBuilderDocumentsExt::document_files`](crate::ModuleBuilderDocumentsExt::document_files).
///
/// Paths are `/`-separated and relative to a template directory
/// (`certificates/classic.html`, `images/logo.png`). The first directory
/// holding a path wins, and any file wins over an embedded one, so an app
/// can override a template a package ships by putting a file at the same
/// path. A path that would leave its directory (`..`, an absolute path, a
/// backslash, a drive prefix) or names a hidden segment (`.env`) finds
/// nothing.
///
/// Templates are read on every render, so edits show up at once.
#[derive(Clone, Debug, Default)]
pub struct Templates {
    dirs: Vec<PathBuf>,
    embedded: Arc<BTreeMap<String, Bytes>>,
}

impl Templates {
    /// The files under `dirs` (searched in order) over `embedded`
    /// (path, contents) pairs; a later pair replaces an earlier one with
    /// the same path. Directories that do not exist are skipped.
    pub fn new<I, P, B>(dirs: Vec<PathBuf>, embedded: I) -> Self
    where
        I: IntoIterator<Item = (P, B)>,
        P: Into<String>,
        B: Into<Bytes>,
    {
        let embedded = embedded
            .into_iter()
            .filter_map(|(path, contents)| {
                let path = path.into();
                normalize(&path).map(|path| (path, contents.into()))
            })
            .collect();
        Self {
            dirs,
            embedded: Arc::new(embedded),
        }
    }

    /// Only the embedded files: handy in tests.
    pub fn from_files<I, P, B>(files: I) -> Self
    where
        I: IntoIterator<Item = (P, B)>,
        P: Into<String>,
        B: Into<Bytes>,
    {
        Self::new(Vec::new(), files)
    }

    /// The template directories.
    pub fn dirs(&self) -> &[PathBuf] {
        &self.dirs
    }

    /// The embedded files, by path.
    pub fn embedded(&self) -> impl Iterator<Item = (&str, &Bytes)> {
        self.embedded
            .iter()
            .map(|(path, bytes)| (path.as_str(), bytes))
    }

    /// Whether a file exists at `path`.
    pub fn exists(&self, path: &str) -> bool {
        let Some(path) = normalize(path) else {
            return false;
        };
        self.dirs.iter().any(|dir| dir.join(&path).is_file()) || self.embedded.contains_key(&path)
    }

    /// The contents of the file at `path`, `None` when no directory and no
    /// embedded file has it. Blocking file-system I/O.
    pub fn read(&self, path: &str) -> std::io::Result<Option<Bytes>> {
        let Some(path) = normalize(path) else {
            return Ok(None);
        };
        for dir in &self.dirs {
            let file = dir.join(&path);
            match std::fs::read(&file) {
                Ok(contents) => return Ok(Some(Bytes::from(contents))),
                // A directory reads as an error (access denied on Windows).
                Err(error) if error.kind() == std::io::ErrorKind::NotFound || file.is_dir() => {}
                Err(error) => return Err(error),
            }
        }
        Ok(self.embedded.get(&path).cloned())
    }

    /// The path of the template `name` with `extension`: `name` itself when
    /// it already ends in `.extension`, otherwise `name.extension`. `None`
    /// when the name is not a valid path.
    pub fn template_path(name: &str, extension: &str) -> Option<String> {
        let path = normalize(name)?;
        if Path::new(&path)
            .extension()
            .is_some_and(|ext| ext == extension)
        {
            Some(path)
        } else {
            Some(format!("{path}.{extension}"))
        }
    }

    /// The directory for HTML templates that navree-view reads.
    #[cfg(any(feature = "native", feature = "chrome"))]
    pub(crate) fn view_dirs(&self) -> Vec<PathBuf> {
        self.dirs.clone()
    }

    /// Every embedded HTML template, as navree-view (name, source) pairs.
    #[cfg(any(feature = "native", feature = "chrome"))]
    pub(crate) fn embedded_html(&self) -> Vec<(String, String)> {
        self.embedded
            .iter()
            .filter(|(path, _)| {
                Path::new(path.as_str())
                    .extension()
                    .is_some_and(|ext| ext == HTML_EXTENSION)
            })
            .filter_map(|(path, bytes)| {
                std::str::from_utf8(bytes)
                    .ok()
                    .map(|source| (path.clone(), source.to_owned()))
            })
            .collect()
    }
}

/// `path` as a clean relative path (`a/b.html`), or `None` when it is
/// empty, absolute, uses backslashes or a drive prefix, has `..`, or names
/// a hidden segment.
pub(crate) fn normalize(path: &str) -> Option<String> {
    if path.contains('\\') || path.contains(':') || path.contains('\0') {
        return None;
    }
    let trimmed = path.strip_prefix('/').unwrap_or(path);
    if trimmed.starts_with('/') {
        return None;
    }
    let mut segments = Vec::new();
    for segment in trimmed.split('/') {
        match segment {
            "" | "." => {}
            ".." => return None,
            s if s.starts_with('.') => return None,
            s => segments.push(s),
        }
    }
    if segments.is_empty() {
        return None;
    }
    Some(segments.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_normalized_and_cannot_leave_the_directory() {
        assert_eq!(normalize("a/b.html").as_deref(), Some("a/b.html"));
        assert_eq!(normalize("/a//./b.html").as_deref(), Some("a/b.html"));
        assert_eq!(normalize("../secret.txt"), None);
        assert_eq!(normalize("a/../../b"), None);
        assert_eq!(normalize("a\\b"), None);
        assert_eq!(normalize("C:/windows"), None);
        assert_eq!(normalize("//server/share"), None);
        assert_eq!(normalize(".env"), None);
        assert_eq!(normalize("a/.git/config"), None);
        assert_eq!(normalize(""), None);
        assert_eq!(normalize("/"), None);
    }

    #[test]
    fn template_paths_add_the_extension_once() {
        assert_eq!(
            Templates::template_path("certificates/classic", "html").as_deref(),
            Some("certificates/classic.html")
        );
        assert_eq!(
            Templates::template_path("certificates/classic.html", "html").as_deref(),
            Some("certificates/classic.html")
        );
        assert_eq!(
            Templates::template_path("invoices/show", "html").as_deref(),
            Some("invoices/show.html")
        );
        assert_eq!(Templates::template_path("../x", "html"), None);
    }

    #[test]
    fn files_win_over_embedded_ones_and_the_first_directory_wins() {
        let first = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        let second = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        std::fs::create_dir_all(first.path().join("a")).unwrap();
        std::fs::create_dir_all(second.path().join("a")).unwrap();
        std::fs::write(first.path().join("a/one.html"), "first").unwrap();
        std::fs::write(second.path().join("a/one.html"), "second").unwrap();
        std::fs::write(second.path().join("a/two.html"), "second two").unwrap();
        let templates = Templates::new(
            vec![first.path().into(), second.path().into()],
            [
                ("a/two.html", "embedded two"),
                ("a/three.html", "embedded three"),
            ],
        );
        let read = |path: &str| templates.read(path).unwrap();
        assert_eq!(read("a/one.html").unwrap(), "first");
        assert_eq!(read("a/two.html").unwrap(), "second two");
        assert_eq!(read("/a/three.html").unwrap(), "embedded three");
        assert_eq!(read("a/four.html"), None);
        assert_eq!(read("a"), None);
        assert_eq!(read("../a/one.html"), None);
        assert!(templates.exists("a/three.html"));
        assert!(!templates.exists("a/four.html"));
    }
}
