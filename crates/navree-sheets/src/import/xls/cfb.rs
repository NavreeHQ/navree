//! The compound file (OLE2, [MS-CFB]) an XLS workbook is stored in: a
//! header, a FAT (sector chains) listed by the DIFAT, a directory of
//! streams, and a mini stream (with a mini FAT) for streams under 4,096
//! bytes. [`workbook_stream`] finds the workbook stream and copies it out.
//!
//! Nothing is copied but the workbook stream: the FAT is read as far as the
//! file has sectors, the directory and the mini stream are read where they
//! are. Every chain is followed at most as many steps as there are sectors
//! (or mini sectors) to follow, so a chain that loops is refused, and a
//! stream's size is measured on the sectors that hold it before it is
//! copied.

use super::{XlsError, damaged};

const SIGNATURE: &[u8] = b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1";
/// Sector numbers from here up are not sectors.
const LAST_SECTOR: u32 = 0xFFFF_FFFA;
const DIFAT_SECTOR: u32 = 0xFFFF_FFFC;
const END_OF_CHAIN: u32 = 0xFFFF_FFFE;
/// Streams under this size are in the mini stream.
const MINI_CUTOFF: u64 = 4096;
const MINI_SECTOR: usize = 64;
const ENTRY: usize = 128;
const STREAM: u8 = 2;
/// The names a workbook stream has, in the order they are looked for:
/// BIFF8, BIFF5, and as some writers spell them.
const WORKBOOK_NAMES: [&str; 4] = ["Workbook", "Book", "WORKBOOK", "BOOK"];

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    let bytes: [u8; 2] = data.get(at..at.checked_add(2)?)?.try_into().ok()?;
    Some(u16::from_le_bytes(bytes))
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    let bytes: [u8; 4] = data.get(at..at.checked_add(4)?)?.try_into().ok()?;
    Some(u32::from_le_bytes(bytes))
}

fn u64_at(data: &[u8], at: usize) -> Option<u64> {
    let bytes: [u8; 8] = data.get(at..at.checked_add(8)?)?.try_into().ok()?;
    Some(u64::from_le_bytes(bytes))
}

/// The sector numbers in a sector of the FAT or the DIFAT.
fn entries(sector: &[u8]) -> impl Iterator<Item = u32> + '_ {
    sector
        .as_chunks::<4>()
        .0
        .iter()
        .map(|entry| u32::from_le_bytes(*entry))
}

fn loops() -> XlsError {
    damaged("a chain of sectors loops")
}

fn past_the_end() -> XlsError {
    damaged("a chain of sectors points past the end of the file")
}

/// A compound file, its header read and its FAT loaded.
struct Compound<'a> {
    bytes: &'a [u8],
    /// 512 or 4,096 bytes.
    sector: usize,
    /// How many sectors the file has (the last one may be cut short).
    sectors: u32,
    fat: Vec<u32>,
}

impl<'a> Compound<'a> {
    fn open(bytes: &'a [u8]) -> Result<Self, XlsError> {
        if !bytes.starts_with(SIGNATURE) {
            return Err(XlsError::NotXls);
        }
        let header = bytes
            .get(..512)
            .ok_or_else(|| damaged("the compound file's header is cut short"))?;
        let sector = match u16_at(header, 30) {
            Some(9) => 512,
            Some(12) => 4096,
            _ => {
                return Err(damaged(
                    "the compound file's sector size is not 512 or 4,096 bytes",
                ));
            }
        };
        if u16_at(header, 32) != Some(6) {
            return Err(damaged(
                "the compound file's mini sector size is not 64 bytes",
            ));
        }
        // The header takes the first sector; sector 0 is the next one.
        let body = bytes
            .len()
            .checked_sub(sector)
            .ok_or_else(|| damaged("the compound file's header is cut short"))?;
        let sectors = u32::try_from(body.div_ceil(sector))
            .map_err(|_| damaged("the compound file has too many sectors"))?;
        let mut compound = Compound {
            bytes,
            sector,
            sectors,
            fat: Vec::new(),
        };
        compound.load_fat(header)?;
        Ok(compound)
    }

    /// The bytes of sector `id` (fewer than a sector for the last one when
    /// the file is cut short).
    fn sector(&self, id: u32) -> Result<&'a [u8], XlsError> {
        if id >= self.sectors {
            return Err(past_the_end());
        }
        let start = usize::try_from(id)
            .ok()
            .and_then(|id| id.checked_add(1))
            .and_then(|n| n.checked_mul(self.sector))
            .ok_or_else(past_the_end)?;
        let end = start.saturating_add(self.sector).min(self.bytes.len());
        self.bytes.get(start..end).ok_or_else(past_the_end)
    }

    /// Reads the FAT: the sectors the DIFAT lists (the header's 109
    /// entries, then the DIFAT's own chain of sectors, whose last entry
    /// each leads to the next), as far as the file has sectors to describe.
    fn load_fat(&mut self, header: &[u8]) -> Result<(), XlsError> {
        let mut difat: Vec<u32> = entries(header.get(76..512).unwrap_or_default()).collect();
        let mut next = u32_at(header, 68).unwrap_or(END_OF_CHAIN);
        let mut seen = vec![false; usize::try_from(self.sectors).unwrap_or(0)];
        while next < LAST_SECTOR {
            let mark = usize::try_from(next)
                .ok()
                .and_then(|at| seen.get_mut(at))
                .ok_or_else(past_the_end)?;
            if *mark {
                return Err(loops());
            }
            *mark = true;
            let sector = self.sector(next)?;
            difat.extend(entries(sector));
            next = difat
                .pop()
                .ok_or_else(|| damaged("the compound file's DIFAT is empty"))?;
        }
        // The FAT needs one entry a sector; FAT sectors past those describe
        // sectors the file does not have.
        let needed = usize::try_from(self.sectors).unwrap_or(usize::MAX);
        for id in difat.into_iter().filter(|id| *id < DIFAT_SECTOR) {
            let sector = self.sector(id)?;
            if self.fat.len() < needed {
                self.fat.extend(entries(sector));
            }
        }
        Ok(())
    }

    /// Follows the chain from sector `start`, handing each sector (its id
    /// and its bytes) to `visit`, until it ends or `limit` bytes have been
    /// handed over.
    fn walk(
        &self,
        start: u32,
        limit: u64,
        mut visit: impl FnMut(u32, &'a [u8]),
    ) -> Result<(), XlsError> {
        let mut id = start;
        let mut given = 0u64;
        let mut steps = 0u32;
        while id != END_OF_CHAIN && given < limit {
            if steps >= self.sectors {
                return Err(loops());
            }
            steps = steps.saturating_add(1);
            let sector = self.sector(id)?;
            visit(id, sector);
            given = given.saturating_add(sector.len() as u64);
            id = *usize::try_from(id)
                .ok()
                .and_then(|at| self.fat.get(at))
                .ok_or_else(past_the_end)?;
        }
        Ok(())
    }

    /// The sectors of the chain from `start`, `limit` bytes of them at most.
    fn chain(&self, start: u32, limit: u64) -> Result<Vec<u32>, XlsError> {
        let mut ids = Vec::new();
        self.walk(start, limit, |id, _| ids.push(id))?;
        Ok(ids)
    }
}

/// A directory entry that may be the workbook stream.
#[derive(Clone, Copy, Debug)]
struct Entry {
    start: u32,
    size: u64,
}

/// The root entry (whose stream is the mini stream) and, of the stream
/// entries, the first with each of the workbook stream's names.
fn find_entries(
    compound: &Compound<'_>,
    directory_start: u32,
) -> Result<(Entry, [Option<Entry>; 4]), XlsError> {
    let mut root = None;
    let mut found = [None; 4];
    let mut broken = false;
    compound.walk(directory_start, u64::MAX, |_, sector| {
        if sector.len() % ENTRY != 0 {
            broken = true;
        }
        for entry in sector.as_chunks::<ENTRY>().0 {
            let parsed = parse_entry(entry, compound.sector);
            if root.is_none() {
                root = Some(parsed.1);
                continue;
            }
            if entry.get(66) != Some(&STREAM) {
                continue;
            }
            if let Some(slot) = WORKBOOK_NAMES
                .iter()
                .position(|name| *name == parsed.0)
                .and_then(|at| found.get_mut(at))
            {
                slot.get_or_insert(parsed.1);
            }
        }
    })?;
    if broken {
        return Err(damaged("the compound file's directory is cut short"));
    }
    let root = root.ok_or_else(|| damaged("the compound file's directory is empty"))?;
    Ok((root, found))
}

/// An entry's name (up to its first NUL) and where its stream is.
fn parse_entry(entry: &[u8], sector: usize) -> (String, Entry) {
    let units: Vec<u16> = entry
        .get(..64)
        .unwrap_or_default()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|unit| u16::from_le_bytes(*unit))
        .take_while(|unit| *unit != 0)
        .collect();
    let name = String::from_utf16_lossy(&units);
    let start = u32_at(entry, 116).unwrap_or(END_OF_CHAIN);
    // Version 3 files (512-byte sectors) keep the size in 32 bits; the high
    // ones may hold anything.
    let size = if sector == 512 {
        u64::from(u32_at(entry, 120).unwrap_or(0))
    } else {
        u64_at(entry, 120).unwrap_or(0)
    };
    (name, Entry { start, size })
}

/// The mini stream: the root entry's chain, and the mini FAT.
struct Mini<'a> {
    compound: &'a Compound<'a>,
    /// The sectors of the mini stream, and its size.
    sectors: Vec<u32>,
    size: u64,
    /// The sectors of the mini FAT.
    fat: Vec<u32>,
}

impl Mini<'_> {
    /// How many mini sectors the mini stream has.
    fn count(&self) -> u64 {
        self.size.div_ceil(MINI_SECTOR as u64)
    }

    /// The bytes at `at` in the mini stream, `len` of them at most (fewer
    /// at the end of a sector or of the stream).
    fn bytes_at(&self, at: u64, len: usize) -> Result<&[u8], XlsError> {
        let sector = self.compound.sector as u64;
        let (Some(id), Some(within)) = (at.checked_div(sector), at.checked_rem(sector)) else {
            return Err(past_the_end());
        };
        let id = usize::try_from(id).map_err(|_| past_the_end())?;
        let within = usize::try_from(within).map_err(|_| past_the_end())?;
        let id = *self.sectors.get(id).ok_or_else(past_the_end)?;
        let data = self.compound.sector(id)?;
        let end = within.saturating_add(len).min(data.len());
        data.get(within..end).ok_or_else(past_the_end)
    }

    /// The mini FAT entry of mini sector `id`.
    fn next(&self, id: u32) -> Result<u32, XlsError> {
        let per_sector = (self.compound.sector / 4) as u64;
        let id = u64::from(id);
        let (Some(sector), Some(within)) = (id.checked_div(per_sector), id.checked_rem(per_sector))
        else {
            return Err(past_the_end());
        };
        let sector = usize::try_from(sector).map_err(|_| past_the_end())?;
        let within = usize::try_from(within)
            .ok()
            .and_then(|n| n.checked_mul(4))
            .ok_or_else(past_the_end)?;
        let sector = *self.fat.get(sector).ok_or_else(past_the_end)?;
        u32_at(self.compound.sector(sector)?, within).ok_or_else(past_the_end)
    }

    /// Follows the chain of mini sectors from `start`, handing each mini
    /// sector's bytes to `visit`, until it ends or `limit` bytes are given.
    fn walk(&self, start: u32, limit: u64, mut visit: impl FnMut(&[u8])) -> Result<(), XlsError> {
        let count = self.count();
        let mut id = start;
        let mut given = 0u64;
        let mut steps = 0u64;
        while id != END_OF_CHAIN && given < limit {
            if u64::from(id) >= count {
                return Err(past_the_end());
            }
            if steps >= count {
                return Err(loops());
            }
            steps = steps.saturating_add(1);
            let at = u64::from(id).saturating_mul(MINI_SECTOR as u64);
            let len = usize::try_from(self.size.saturating_sub(at))
                .unwrap_or(MINI_SECTOR)
                .min(MINI_SECTOR);
            let data = self.bytes_at(at, len)?;
            visit(data);
            given = given.saturating_add(data.len() as u64);
            id = self.next(id)?;
        }
        Ok(())
    }
}

/// Copies the stream from `start`, `size` bytes of it at most, out of the
/// chain `walk` follows: measured first, so a stream past `max` bytes is
/// refused before anything is copied.
fn copy_stream(
    size: u64,
    max: u64,
    walk: impl Fn(&mut dyn FnMut(&[u8])) -> Result<(), XlsError>,
) -> Result<Vec<u8>, XlsError> {
    let mut held = 0u64;
    walk(&mut |piece: &[u8]| held = held.saturating_add(piece.len() as u64))?;
    let held = held.min(size);
    if held > max {
        return Err(XlsError::TooLarge(format!(
            "The workbook stream is larger than {max} bytes."
        )));
    }
    let mut stream = Vec::with_capacity(usize::try_from(held).unwrap_or(0));
    walk(&mut |piece: &[u8]| {
        let room = usize::try_from(held)
            .unwrap_or(usize::MAX)
            .saturating_sub(stream.len());
        stream.extend_from_slice(piece.get(..room.min(piece.len())).unwrap_or_default());
    })?;
    Ok(stream)
}

/// The workbook stream of the compound file in `bytes`: `Workbook` (BIFF8)
/// or `Book` (BIFF5). Refused when the stream holds more than
/// `max_stream_bytes`.
pub(super) fn workbook_stream(bytes: &[u8], max_stream_bytes: u64) -> Result<Vec<u8>, XlsError> {
    let compound = Compound::open(bytes)?;
    let header = bytes.get(..512).unwrap_or_default();
    let directory = u32_at(header, 48).unwrap_or(END_OF_CHAIN);
    let mini_fat_start = u32_at(header, 60).unwrap_or(END_OF_CHAIN);
    let mini_fat_sectors = u64::from(u32_at(header, 64).unwrap_or(0));
    let (root, found) = find_entries(&compound, directory)?;
    // The mini stream and its FAT, when the file has one: checked whether
    // the workbook is in it or not.
    let mini = if mini_fat_sectors > 0 {
        let fat_bytes = mini_fat_sectors.saturating_mul(compound.sector as u64);
        Some(Mini {
            compound: &compound,
            sectors: compound.chain(root.start, root.size)?,
            size: root.size,
            fat: compound.chain(mini_fat_start, fat_bytes)?,
        })
    } else {
        None
    };
    let entry = found.into_iter().flatten().next().ok_or(XlsError::NotXls)?;
    if entry.size < MINI_CUTOFF {
        let Some(mini) = mini else {
            if entry.start == END_OF_CHAIN {
                return Ok(Vec::new());
            }
            return Err(damaged(
                "the workbook stream is in a mini stream the file does not have",
            ));
        };
        // The mini stream as far as its chain of sectors goes.
        let mini = Mini {
            size: mini
                .size
                .min((mini.sectors.len() as u64).saturating_mul(compound.sector as u64)),
            ..mini
        };
        copy_stream(entry.size, max_stream_bytes, |visit| {
            mini.walk(entry.start, entry.size, visit)
        })
    } else {
        copy_stream(entry.size, max_stream_bytes, |visit| {
            compound.walk(entry.start, entry.size, |_, piece| visit(piece))
        })
    }
}
