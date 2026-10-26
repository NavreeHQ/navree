//! The Typst `World` of one render: the template tree, the render's data
//! at `/data.json`, the fonts, local packages and the clock.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use chrono::{Datelike, Duration};
use navree_contracts::Clock;
use typst::diag::{FileError, FileResult, PackageError};
use typst::foundations::{Bytes, Datetime};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, World};

use super::fonts::FontSet;
use crate::templates::Templates;

/// The virtual path of the render's data.
pub(crate) const DATA_PATH: &str = "/data.json";
/// The virtual path of inline markup; no template file can have it (hidden
/// names are never read from disk).
pub(crate) const INLINE_PATH: &str = "/.inline.typ";

/// A file id in the project root.
pub(crate) fn project_file(path: &str) -> Option<FileId> {
    let vpath = VirtualPath::new(path).ok()?;
    Some(RootedPath::new(VirtualRoot::Project, vpath).intern())
}

pub(crate) struct RenderWorld {
    pub(crate) library: Arc<LazyHash<Library>>,
    pub(crate) fonts: Arc<FontSet>,
    pub(crate) templates: Templates,
    pub(crate) packages_dir: Option<PathBuf>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) main: FileId,
    pub(crate) main_source: Source,
    pub(crate) data_id: Option<FileId>,
    pub(crate) data: Bytes,
    pub(crate) sources: Mutex<HashMap<FileId, Source>>,
}

impl RenderWorld {
    /// The file's path as the template tree names it (`a/b.typ`), or the
    /// package path for package files.
    pub(crate) fn display_path(&self, id: FileId) -> String {
        if id.vpath().get_with_slash() == INLINE_PATH {
            return "inline.typ".to_owned();
        }
        match id.root() {
            VirtualRoot::Project => id.vpath().get_without_slash().to_owned(),
            VirtualRoot::Package(spec) => format!("{spec}{}", id.vpath().get_with_slash()),
        }
    }

    fn read(&self, id: FileId) -> FileResult<Bytes> {
        if Some(id) == self.data_id {
            return Ok(self.data.clone());
        }
        let path = id.vpath().get_without_slash();
        match id.root() {
            VirtualRoot::Project => match self.templates.read(path) {
                Ok(Some(bytes)) => Ok(Bytes::new(bytes)),
                Ok(None) => Err(FileError::NotFound(PathBuf::from(path))),
                Err(error) => Err(FileError::from_io(error, &PathBuf::from(path))),
            },
            VirtualRoot::Package(spec) => {
                let Some(dir) = &self.packages_dir else {
                    return Err(FileError::Package(PackageError::Other(Some(
                        format!(
                            "package {spec} is not available: set documents.packages_dir to a \
                             directory holding packages as <namespace>/<name>/<version>"
                        )
                        .into(),
                    ))));
                };
                let root = dir
                    .join(spec.namespace.as_str())
                    .join(spec.name.as_str())
                    .join(spec.version.to_string());
                if !root.is_dir() {
                    return Err(FileError::Package(PackageError::NotFound(spec.clone())));
                }
                let file = id
                    .vpath()
                    .realize(&root)
                    .map_err(FileError::Realize)?;
                std::fs::read(&file)
                    .map(Bytes::new)
                    .map_err(|error| FileError::from_io(error, &file))
            }
        }
    }
}

impl World for RenderWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &self.fonts.book
    }

    fn main(&self) -> FileId {
        self.main
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main {
            return Ok(self.main_source.clone());
        }
        let mut sources = self.sources.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(source) = sources.get(&id) {
            return Ok(source.clone());
        }
        let bytes = self.read(id)?;
        let text = std::str::from_utf8(bytes.as_slice()).map_err(|_| FileError::InvalidUtf8)?;
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let source = Source::new(id, text.to_owned());
        sources.insert(id, source.clone());
        Ok(source)
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        if id == self.main {
            return Ok(Bytes::from_string(self.main_source.text().to_owned()));
        }
        self.read(id)
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.fonts.get(index).cloned()
    }

    /// The clock's date in UTC (servers have no meaningful local zone), or
    /// shifted by `offset`.
    fn today(&self, offset: Option<typst::foundations::Duration>) -> Option<Datetime> {
        let mut now = self.clock.now().naive_utc();
        if let Some(offset) = offset {
            now += Duration::seconds(offset.seconds() as i64);
        }
        Datetime::from_ymd(
            now.year(),
            u8::try_from(now.month()).ok()?,
            u8::try_from(now.day()).ok()?,
        )
    }
}
