//! The zip file of an XLSX, XLSB or ODS workbook: its parts by name, each
//! read as a stream that stops at the size the file declares for it.

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Cursor, Read};

use zip::ZipArchive;
use zip::result::ZipError;

use super::{Budget, Damaged, damaged};

/// A workbook's zip file.
#[derive(Clone)]
pub(super) struct Archive<'a> {
    zip: ZipArchive<Cursor<&'a [u8]>>,
    /// The parts' names by the same names in lower case, with `/` between
    /// folders: OOXML part names do not depend on case.
    folded: HashMap<String, String>,
}

impl<'a> Archive<'a> {
    pub(super) fn open(bytes: &'a [u8]) -> Result<Self, Damaged> {
        let zip = ZipArchive::new(Cursor::new(bytes)).map_err(zip_error)?;
        let mut folded = HashMap::new();
        for name in zip.file_names() {
            let name = name.map_err(zip_error)?;
            folded.insert(
                name.replace('\\', "/").to_ascii_lowercase(),
                name.into_owned(),
            );
        }
        Ok(Archive { zip, folded })
    }

    /// The name the part `name` has in the file, whatever the case of its
    /// letters; `name` itself when there is none.
    pub(super) fn name_of<'n>(&'n self, name: &'n str) -> &'n str {
        self.folded
            .get(&name.to_ascii_lowercase())
            .map_or(name, String::as_str)
    }

    /// Whether the file has a part named `name`, whatever the case of its
    /// letters.
    pub(super) fn has(&self, name: &str) -> bool {
        self.folded.contains_key(&name.to_ascii_lowercase())
    }

    /// Reads the part named exactly `name` with `read`; none when the file
    /// has no such part.
    pub(super) fn read<T, E: From<Damaged>>(
        &self,
        name: &str,
        read: impl FnOnce(&mut dyn BufRead) -> Result<T, E>,
    ) -> Result<Option<T>, E> {
        // A copy shares the file and its directory: it only has a cursor of
        // its own.
        let mut zip = self.zip.clone();
        let part = match zip.by_name(name) {
            Ok(part) => part,
            Err(ZipError::FileNotFound) => return Ok(None),
            Err(error) => return Err(zip_error(error).into()),
        };
        read(&mut BufReader::new(part)).map(Some)
    }

    /// [`Archive::read`] while the workbook is opened: what is unpacked counts
    /// against what opening may read, and the part stops when that runs out.
    pub(super) fn read_opening<T, E: From<Damaged>>(
        &self,
        name: &str,
        budget: &Budget,
        read: impl FnOnce(&mut dyn BufRead) -> Result<T, E>,
    ) -> Result<Option<T>, E> {
        let mut zip = self.zip.clone();
        let part = match zip.by_name(name) {
            Ok(part) => part,
            Err(ZipError::FileNotFound) => return Ok(None),
            Err(error) => return Err(zip_error(error).into()),
        };
        read(&mut BufReader::new(Charged { part, budget })).map(Some)
    }
}

/// A part whose bytes are charged to what opening may read.
struct Charged<'b, R> {
    part: R,
    budget: &'b Budget,
}

impl<R: Read> Read for Charged<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let read = self.part.read(buf)?;
        if !self.budget.charge_read(read) {
            return Err(io::Error::other("opening has read all it may"));
        }
        Ok(read)
    }
}

fn zip_error(error: ZipError) -> Damaged {
    damaged(format_args!("the zip file: {error}"))
}
