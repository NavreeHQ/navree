//! Writes XLS files for the tests: BIFF records, the workbook stream they
//! make, and the compound file that holds it.
//!
//! Only what the tests need, written the way [MS-XLS] and [MS-CFB] lay
//! files out: version 3 compound files with 512-byte sectors, streams under
//! 4,096 bytes in the mini stream, a directory whose entries hang off the
//! root as a chain of right siblings.
#![cfg_attr(not(test), allow(dead_code))]
#![allow(
    clippy::indexing_slicing,
    clippy::unwrap_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

/// A compound file's sector numbers that mean something other than a
/// sector.
pub(crate) const END_OF_CHAIN: u32 = 0xFFFF_FFFE;
pub(crate) const FREE: u32 = 0xFFFF_FFFF;
const FAT_SECTOR: u32 = 0xFFFF_FFFD;
const NO_STREAM: u32 = 0xFFFF_FFFF;

/// A compound file and where its parts are.
pub(crate) struct Compound {
    pub(crate) bytes: Vec<u8>,
    /// The sectors of each stream, in order (mini sectors for a stream in
    /// the mini stream).
    pub(crate) chains: Vec<Vec<u32>>,
    /// Whether each stream is in the mini stream.
    pub(crate) mini: Vec<bool>,
    /// The directory's sectors.
    pub(crate) directory: Vec<u32>,
    /// The sectors of the mini stream, and of the mini FAT.
    pub(crate) mini_stream: Vec<u32>,
    pub(crate) mini_fat: Vec<u32>,
}

impl Compound {
    /// The file offset of sector `id`.
    pub(crate) fn offset(id: u32) -> usize {
        (id as usize + 1) * 512
    }

    /// Points the FAT entry of sector `id` at `next`.
    pub(crate) fn set_fat(&mut self, id: u32, next: u32) {
        // The FAT sectors come first, from sector 0.
        let at = Self::offset(id / 128) + (id as usize % 128) * 4;
        self.bytes[at..at + 4].copy_from_slice(&next.to_le_bytes());
    }

    /// Points the mini FAT entry of mini sector `id` at `next`.
    pub(crate) fn set_mini_fat(&mut self, id: u32, next: u32) {
        let sector = self.mini_fat[id as usize / 128];
        let at = Self::offset(sector) + (id as usize % 128) * 4;
        self.bytes[at..at + 4].copy_from_slice(&next.to_le_bytes());
    }

    /// Sets the size the directory gives stream `index` (the root is entry
    /// 0, the streams follow).
    pub(crate) fn set_size(&mut self, index: usize, size: u32) {
        let entry = index + 1;
        let at = Self::offset(self.directory[entry / 4]) + (entry % 4) * 128 + 120;
        self.bytes[at..at + 4].copy_from_slice(&size.to_le_bytes());
    }

    /// Writes `value` at byte `at` of the header.
    pub(crate) fn set_header(&mut self, at: usize, value: u32) {
        self.bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
}

fn sectors_for(len: usize, size: usize) -> usize {
    len.div_ceil(size)
}

/// A version 3 compound file holding `streams`, each a name and its bytes.
pub(crate) fn compound(streams: &[(&str, &[u8])]) -> Compound {
    let mini: Vec<bool> = streams.iter().map(|(_, data)| data.len() < 4096).collect();
    let mut mini_count = 0usize;
    let mut mini_chains = Vec::new();
    for ((_, data), small) in streams.iter().zip(&mini) {
        let n = if *small {
            sectors_for(data.len(), 64)
        } else {
            0
        };
        mini_chains.push(
            (mini_count..mini_count + n)
                .map(|i| i as u32)
                .collect::<Vec<_>>(),
        );
        mini_count += n;
    }
    let mini_stream_sectors = sectors_for(mini_count * 64, 512);
    let mini_fat_sectors = sectors_for(mini_count * 4, 512);
    let dir_sectors = sectors_for((streams.len() + 1) * 128, 512);
    let big_sectors: usize = streams
        .iter()
        .zip(&mini)
        .map(|((_, data), small)| {
            if *small {
                0
            } else {
                sectors_for(data.len(), 512)
            }
        })
        .sum();
    let others = dir_sectors + mini_fat_sectors + mini_stream_sectors + big_sectors;
    let mut fat_sectors = 1;
    while fat_sectors * 128 < fat_sectors + others {
        fat_sectors += 1;
    }
    assert!(fat_sectors <= 109, "too large for a header's DIFAT");
    let total = fat_sectors + others;
    let mut fat = vec![FREE; fat_sectors * 128];
    let mut next = fat_sectors as u32;
    let mut take = |count: usize, fat: &mut Vec<u32>| -> Vec<u32> {
        let ids: Vec<u32> = (next..next + count as u32).collect();
        for pair in ids.windows(2) {
            fat[pair[0] as usize] = pair[1];
        }
        if let Some(last) = ids.last() {
            fat[*last as usize] = END_OF_CHAIN;
        }
        next += count as u32;
        ids
    };
    let fats: Vec<u32> = (0..fat_sectors as u32).collect();
    for id in &fats {
        fat[*id as usize] = FAT_SECTOR;
    }
    let directory = take(dir_sectors, &mut fat);
    let mini_fat = take(mini_fat_sectors, &mut fat);
    let mini_stream = take(mini_stream_sectors, &mut fat);
    let mut chains = Vec::new();
    for ((_, data), small) in streams.iter().zip(&mini) {
        if *small {
            chains.push(Vec::new());
        } else {
            chains.push(take(sectors_for(data.len(), 512), &mut fat));
        }
    }
    for (chain, mini_chain) in chains.iter_mut().zip(&mini_chains) {
        if chain.is_empty() {
            chain.clone_from(mini_chain);
        }
    }

    let mut bytes = vec![0u8; (total + 1) * 512];
    // The header.
    bytes[..8].copy_from_slice(b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1");
    bytes[24..26].copy_from_slice(&0x003Eu16.to_le_bytes());
    bytes[26..28].copy_from_slice(&3u16.to_le_bytes());
    bytes[28..30].copy_from_slice(&0xFFFEu16.to_le_bytes());
    bytes[30..32].copy_from_slice(&9u16.to_le_bytes());
    bytes[32..34].copy_from_slice(&6u16.to_le_bytes());
    bytes[44..48].copy_from_slice(&(fat_sectors as u32).to_le_bytes());
    bytes[48..52].copy_from_slice(&directory[0].to_le_bytes());
    bytes[56..60].copy_from_slice(&4096u32.to_le_bytes());
    let mini_fat_start = mini_fat.first().copied().unwrap_or(END_OF_CHAIN);
    bytes[60..64].copy_from_slice(&mini_fat_start.to_le_bytes());
    bytes[64..68].copy_from_slice(&(mini_fat_sectors as u32).to_le_bytes());
    bytes[68..72].copy_from_slice(&END_OF_CHAIN.to_le_bytes());
    for i in 0..109 {
        let id = fats.get(i).copied().unwrap_or(FREE);
        bytes[76 + i * 4..80 + i * 4].copy_from_slice(&id.to_le_bytes());
    }
    // The FAT.
    for (i, entry) in fat.iter().enumerate() {
        let at = Compound::offset((i / 128) as u32) + (i % 128) * 4;
        bytes[at..at + 4].copy_from_slice(&entry.to_le_bytes());
    }
    // The mini FAT, and the mini stream.
    let mut mini_fat_entries = vec![FREE; mini_fat_sectors * 128];
    for chain in &mini_chains {
        for pair in chain.windows(2) {
            mini_fat_entries[pair[0] as usize] = pair[1];
        }
        if let Some(last) = chain.last() {
            mini_fat_entries[*last as usize] = END_OF_CHAIN;
        }
    }
    for (i, entry) in mini_fat_entries.iter().enumerate() {
        let at = Compound::offset(mini_fat[i / 128]) + (i % 128) * 4;
        bytes[at..at + 4].copy_from_slice(&entry.to_le_bytes());
    }
    for (((_, data), small), chain) in streams.iter().zip(&mini).zip(&chains) {
        for (i, id) in chain.iter().enumerate() {
            let (at, size) = if *small {
                let byte = *id as usize * 64;
                (Compound::offset(mini_stream[byte / 512]) + byte % 512, 64)
            } else {
                (Compound::offset(*id), 512)
            };
            let piece = &data[i * size..data.len().min((i + 1) * size)];
            bytes[at..at + piece.len()].copy_from_slice(piece);
        }
    }
    // The directory: the root, then each stream as the right sibling of
    // the one before.
    let entry = |bytes: &mut Vec<u8>,
                 index: usize,
                 name: &str,
                 kind: u8,
                 start: u32,
                 size: u32,
                 right: u32,
                 child: u32| {
        let at = Compound::offset(directory[index / 4]) + (index % 4) * 128;
        let units: Vec<u16> = name.encode_utf16().collect();
        for (i, unit) in units.iter().enumerate() {
            bytes[at + i * 2..at + i * 2 + 2].copy_from_slice(&unit.to_le_bytes());
        }
        bytes[at + 64..at + 66].copy_from_slice(&(((units.len() + 1) * 2) as u16).to_le_bytes());
        bytes[at + 66] = kind;
        bytes[at + 67] = 1;
        bytes[at + 68..at + 72].copy_from_slice(&NO_STREAM.to_le_bytes());
        bytes[at + 72..at + 76].copy_from_slice(&right.to_le_bytes());
        bytes[at + 76..at + 80].copy_from_slice(&child.to_le_bytes());
        bytes[at + 116..at + 120].copy_from_slice(&start.to_le_bytes());
        bytes[at + 120..at + 124].copy_from_slice(&size.to_le_bytes());
    };
    let root_start = mini_stream.first().copied().unwrap_or(END_OF_CHAIN);
    let child = if streams.is_empty() { NO_STREAM } else { 1 };
    entry(
        &mut bytes,
        0,
        "Root Entry",
        5,
        root_start,
        (mini_count * 64) as u32,
        NO_STREAM,
        child,
    );
    for (i, ((name, data), chain)) in streams.iter().zip(&chains).enumerate() {
        let right = if i + 1 < streams.len() {
            i as u32 + 2
        } else {
            NO_STREAM
        };
        let start = chain.first().copied().unwrap_or(END_OF_CHAIN);
        entry(
            &mut bytes,
            i + 1,
            name,
            2,
            start,
            data.len() as u32,
            right,
            NO_STREAM,
        );
    }
    Compound {
        bytes,
        chains,
        mini,
        directory,
        mini_stream,
        mini_fat,
    }
}

/// One BIFF record.
pub(crate) fn record(kind: u16, data: &[u8]) -> Vec<u8> {
    let mut out = kind.to_le_bytes().to_vec();
    out.extend_from_slice(&(data.len() as u16).to_le_bytes());
    out.extend_from_slice(data);
    out
}

/// A record and the CONTINUE records that carry its data past `max` bytes
/// a record.
pub(crate) fn continued(kind: u16, data: &[u8], max: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, piece) in data.chunks(max).enumerate() {
        out.extend(record(if i == 0 { kind } else { CONTINUE }, piece));
    }
    out
}

pub(crate) const CONTINUE: u16 = 0x003C;
pub(crate) const BOF: u16 = 0x0809;
pub(crate) const EOF: u16 = 0x000A;

fn cell_head(row: u16, col: u16, xf: u16) -> Vec<u8> {
    let mut out = row.to_le_bytes().to_vec();
    out.extend_from_slice(&col.to_le_bytes());
    out.extend_from_slice(&xf.to_le_bytes());
    out
}

/// NUMBER.
pub(crate) fn number(row: u16, col: u16, xf: u16, value: f64) -> Vec<u8> {
    let mut data = cell_head(row, col, xf);
    data.extend_from_slice(&value.to_le_bytes());
    record(0x0203, &data)
}

/// An RK value: an integer (times 100 when `cents`).
pub(crate) fn rk_int(value: i32, cents: bool) -> u32 {
    ((value as u32) << 2) | 2 | u32::from(cents)
}

/// An RK value: the high 30 bits of a float (divided by 100 when `cents`).
pub(crate) fn rk_float(value: f64, cents: bool) -> u32 {
    ((value.to_bits() >> 32) as u32 & !3) | u32::from(cents)
}

/// RK.
pub(crate) fn rk(row: u16, col: u16, xf: u16, rk: u32) -> Vec<u8> {
    let mut data = cell_head(row, col, xf);
    data.extend_from_slice(&rk.to_le_bytes());
    record(0x027E, &data)
}

/// MULRK: `values` from column `first`, each with its XF.
pub(crate) fn mulrk(row: u16, first: u16, values: &[(u16, u32)]) -> Vec<u8> {
    let mut data = row.to_le_bytes().to_vec();
    data.extend_from_slice(&first.to_le_bytes());
    for (xf, rk) in values {
        data.extend_from_slice(&xf.to_le_bytes());
        data.extend_from_slice(&rk.to_le_bytes());
    }
    data.extend_from_slice(&(first + values.len() as u16 - 1).to_le_bytes());
    record(0x00BD, &data)
}

/// BOOLERR: a boolean, or an error by its code.
pub(crate) fn boolerr(row: u16, col: u16, xf: u16, value: u8, error: bool) -> Vec<u8> {
    let mut data = cell_head(row, col, xf);
    data.push(value);
    data.push(u8::from(error));
    record(0x0205, &data)
}

/// BLANK.
pub(crate) fn blank(row: u16, col: u16, xf: u16) -> Vec<u8> {
    record(0x0201, &cell_head(row, col, xf))
}

/// LABELSST.
pub(crate) fn label_sst(row: u16, col: u16, xf: u16, index: u32) -> Vec<u8> {
    let mut data = cell_head(row, col, xf);
    data.extend_from_slice(&index.to_le_bytes());
    record(0x00FD, &data)
}

/// The characters of a BIFF8 string: the flags byte and the characters,
/// one byte each when they all fit, two otherwise.
fn chars8(text: &str) -> (u8, Vec<u8>) {
    let units: Vec<u16> = text.encode_utf16().collect();
    if units.iter().all(|u| *u < 0x100) {
        (0, units.iter().map(|u| *u as u8).collect())
    } else {
        (1, units.iter().flat_map(|u| u.to_le_bytes()).collect())
    }
}

/// `text` in Windows-1252, the code page of the BIFF5 books the tests write.
fn windows_1252(text: &str) -> Vec<u8> {
    encoding_rs::WINDOWS_1252.encode(text).0.into_owned()
}

/// An XLUnicodeString (BIFF8) or a byte string with a 16-bit length
/// (BIFF5).
pub(crate) fn string(text: &str, biff5: bool) -> Vec<u8> {
    if biff5 {
        let bytes = windows_1252(text);
        let mut out = (bytes.len() as u16).to_le_bytes().to_vec();
        out.extend(bytes);
        return out;
    }
    let (flags, chars) = chars8(text);
    let mut out = (text.encode_utf16().count() as u16).to_le_bytes().to_vec();
    out.push(flags);
    out.extend(chars);
    out
}

/// LABEL.
pub(crate) fn label(row: u16, col: u16, xf: u16, text: &str, biff5: bool) -> Vec<u8> {
    let mut data = cell_head(row, col, xf);
    data.extend(string(text, biff5));
    record(0x0204, &data)
}

/// RSTRING (BIFF8): the text and one formatting run.
pub(crate) fn rstring(row: u16, col: u16, xf: u16, text: &str) -> Vec<u8> {
    let mut data = cell_head(row, col, xf);
    data.extend(string(text, false));
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&[0, 0, 0, 0]);
    record(0x00D6, &data)
}

/// FORMULA with the eight bytes of its cached value and a formula of one
/// token (`=1`).
pub(crate) fn formula(row: u16, col: u16, xf: u16, value: [u8; 8]) -> Vec<u8> {
    let mut data = cell_head(row, col, xf);
    data.extend_from_slice(&value);
    data.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
    // cce, then PtgInt 1.
    data.extend_from_slice(&3u16.to_le_bytes());
    data.extend_from_slice(&[0x1E, 1, 0]);
    record(0x0006, &data)
}

/// The cached value of a formula whose result is text (it follows in a
/// STRING record), a boolean, an error, or empty text.
pub(crate) fn formula_text() -> [u8; 8] {
    [0, 0, 0, 0, 0, 0, 0xFF, 0xFF]
}
pub(crate) fn formula_bool(value: bool) -> [u8; 8] {
    [1, 0, u8::from(value), 0, 0, 0, 0xFF, 0xFF]
}
pub(crate) fn formula_error(code: u8) -> [u8; 8] {
    [2, 0, code, 0, 0, 0, 0xFF, 0xFF]
}
pub(crate) fn formula_empty() -> [u8; 8] {
    [3, 0, 0, 0, 0, 0, 0xFF, 0xFF]
}

/// STRING, the text result of the formula before it.
pub(crate) fn string_record(text: &str, biff5: bool) -> Vec<u8> {
    record(0x0207, &string(text, biff5))
}

/// DIMENSIONS (BIFF8): the used rows and columns, last ones exclusive.
pub(crate) fn dimensions(first_row: u32, last_row: u32, first_col: u16, last_col: u16) -> Vec<u8> {
    let mut data = first_row.to_le_bytes().to_vec();
    data.extend_from_slice(&last_row.to_le_bytes());
    data.extend_from_slice(&first_col.to_le_bytes());
    data.extend_from_slice(&last_col.to_le_bytes());
    data.extend_from_slice(&[0, 0]);
    record(0x0200, &data)
}

/// CODEPAGE.
pub(crate) fn codepage(page: u16) -> Vec<u8> {
    record(0x0042, &page.to_le_bytes())
}

/// DATEMODE: the 1904 date system when `date1904`.
pub(crate) fn datemode(date1904: bool) -> Vec<u8> {
    record(0x0022, &u16::from(date1904).to_le_bytes())
}

/// XF with the number format `format`.
pub(crate) fn xf(format: u16) -> Vec<u8> {
    let mut data = vec![0, 0];
    data.extend_from_slice(&format.to_le_bytes());
    data.extend_from_slice(&[0; 16]);
    record(0x00E0, &data)
}

/// FORMAT: the number format `id` with the code `code`.
pub(crate) fn format(id: u16, code: &str, biff5: bool) -> Vec<u8> {
    let mut data = id.to_le_bytes().to_vec();
    if biff5 {
        let bytes = windows_1252(code);
        data.push(bytes.len() as u8);
        data.extend(bytes);
    } else {
        data.extend(string(code, false));
    }
    record(0x041E, &data)
}

/// FILEPASS (RC4 encryption).
pub(crate) fn filepass() -> Vec<u8> {
    let mut data = 1u16.to_le_bytes().to_vec();
    data.extend_from_slice(&[0; 52]);
    record(0x002F, &data)
}

/// One string of a shared string table: its text, how many formatting runs
/// follow it and how many bytes of phonetic data.
pub(crate) struct Shared<'a> {
    pub(crate) text: &'a str,
    pub(crate) runs: u16,
    pub(crate) phonetic: u32,
}

impl<'a> From<&'a str> for Shared<'a> {
    fn from(text: &'a str) -> Self {
        Shared {
            text,
            runs: 0,
            phonetic: 0,
        }
    }
}

/// SST and its CONTINUE records, `max` bytes of data a record at most,
/// with `unique` as the count of unique strings it declares. A string
/// that does not fit goes on in the next record, whose characters start
/// with a flags byte of their own: one byte a character when that part of
/// the string fits in them, two otherwise.
pub(crate) fn sst(strings: &[Shared<'_>], unique: u32, max: usize) -> Vec<u8> {
    let mut records: Vec<Vec<u8>> = vec![Vec::new()];
    let total = strings.len() as u32;
    records[0].extend_from_slice(&total.to_le_bytes());
    records[0].extend_from_slice(&unique.to_le_bytes());
    fn room(records: &[Vec<u8>], max: usize) -> usize {
        max - records.last().unwrap().len()
    }
    for shared in strings {
        let units: Vec<u16> = shared.text.encode_utf16().collect();
        let mut head = (units.len() as u16).to_le_bytes().to_vec();
        let mut flags = 0u8;
        if shared.runs > 0 {
            flags |= 8;
        }
        if shared.phonetic > 0 {
            flags |= 4;
        }
        let wide_first = units.iter().any(|u| *u >= 0x100);
        head.push(flags | u8::from(wide_first));
        if shared.runs > 0 {
            head.extend_from_slice(&shared.runs.to_le_bytes());
        }
        if shared.phonetic > 0 {
            head.extend_from_slice(&shared.phonetic.to_le_bytes());
        }
        if room(&records, max) < head.len() + 2 {
            records.push(Vec::new());
        }
        records.last_mut().unwrap().extend(head);
        let mut rest = &units[..];
        let mut first = true;
        while !rest.is_empty() {
            let mut space = room(&records, max);
            if !first {
                // A part in a record of its own starts with its flags.
                if space < 3 {
                    records.push(Vec::new());
                    space = max;
                }
                space -= 1;
            }
            let wide = if first {
                wide_first
            } else {
                // The part that fits, wide or not.
                let narrow = rest.iter().take(space).all(|u| *u < 0x100);
                !narrow
            };
            let width = if wide { 2 } else { 1 };
            let count = (space / width).min(rest.len());
            if count == 0 {
                records.push(Vec::new());
                first = false;
                continue;
            }
            let record = records.last_mut().unwrap();
            if !first {
                record.push(u8::from(wide));
            }
            for unit in &rest[..count] {
                if wide {
                    record.extend_from_slice(&unit.to_le_bytes());
                } else {
                    record.push(*unit as u8);
                }
            }
            rest = &rest[count..];
            if !rest.is_empty() {
                records.push(Vec::new());
            }
            first = false;
        }
        let mut tail = vec![0u8; shared.runs as usize * 4];
        tail.extend(std::iter::repeat_n(0x55u8, shared.phonetic as usize));
        let mut tail = &tail[..];
        while !tail.is_empty() {
            let space = room(&records, max);
            if space == 0 {
                records.push(Vec::new());
                continue;
            }
            let n = space.min(tail.len());
            records.last_mut().unwrap().extend_from_slice(&tail[..n]);
            tail = &tail[n..];
        }
    }
    let mut out = Vec::new();
    for (i, data) in records.iter().enumerate() {
        out.extend(record(if i == 0 { 0x00FC } else { CONTINUE }, data));
    }
    out
}

/// A sheet of a workbook the tests write.
pub(crate) struct Sheet {
    pub(crate) name: String,
    /// BOUNDSHEET's sheet type: 0 a worksheet, 1 a macro sheet, 2 a chart.
    pub(crate) kind: u8,
    /// BOUNDSHEET's visibility: 0 visible, 1 hidden, 2 very hidden.
    pub(crate) state: u8,
    /// The records between the sheet's BOF and its EOF.
    pub(crate) records: Vec<u8>,
}

impl Sheet {
    pub(crate) fn new(name: &str, records: Vec<u8>) -> Self {
        Sheet {
            name: name.to_owned(),
            kind: 0,
            state: 0,
            records,
        }
    }
}

/// A workbook the tests write: the records of its globals substream after
/// its BOF, and its sheets.
pub(crate) struct Book {
    pub(crate) biff5: bool,
    pub(crate) globals: Vec<u8>,
    pub(crate) sheets: Vec<Sheet>,
}

impl Book {
    pub(crate) fn new(globals: Vec<u8>, sheets: Vec<Sheet>) -> Self {
        Book {
            biff5: false,
            globals,
            sheets,
        }
    }

    fn bof(&self, kind: u16) -> Vec<u8> {
        let mut data = if self.biff5 { 0x0500u16 } else { 0x0600 }
            .to_le_bytes()
            .to_vec();
        data.extend_from_slice(&kind.to_le_bytes());
        data.extend_from_slice(&[0; 4]);
        if !self.biff5 {
            data.extend_from_slice(&[0; 8]);
        }
        record(BOF, &data)
    }

    fn boundsheet(&self, offset: u32, sheet: &Sheet) -> Vec<u8> {
        let mut data = offset.to_le_bytes().to_vec();
        data.push(sheet.state);
        data.push(sheet.kind);
        if self.biff5 {
            let bytes = windows_1252(&sheet.name);
            data.push(bytes.len() as u8);
            data.extend(bytes);
        } else {
            let (flags, chars) = chars8(&sheet.name);
            data.push(sheet.name.encode_utf16().count() as u8);
            data.push(flags);
            data.extend(chars);
        }
        record(0x0085, &data)
    }

    /// The workbook stream.
    pub(crate) fn stream(&self) -> Vec<u8> {
        let mut globals = self.bof(0x0005);
        globals.extend_from_slice(&self.globals);
        let bound_len: usize = self
            .sheets
            .iter()
            .map(|sheet| self.boundsheet(0, sheet).len())
            .sum();
        let mut offset = globals.len() + bound_len + record(EOF, &[]).len();
        let mut substreams = Vec::new();
        for sheet in &self.sheets {
            globals.extend(self.boundsheet(offset as u32, sheet));
            let mut sub = self.bof(if sheet.kind == 2 { 0x0020 } else { 0x0010 });
            sub.extend_from_slice(&sheet.records);
            sub.extend(record(EOF, &[]));
            offset += sub.len();
            substreams.extend(sub);
        }
        globals.extend(record(EOF, &[]));
        globals.extend(substreams);
        globals
    }

    /// The XLS file: the workbook stream in a compound file.
    pub(crate) fn file(&self) -> Vec<u8> {
        self.compound().bytes
    }

    pub(crate) fn compound(&self) -> Compound {
        let stream = self.stream();
        let name = if self.biff5 { "Book" } else { "Workbook" };
        compound(&[(name, &stream)])
    }
}
