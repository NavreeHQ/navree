//! Text extraction for [`Pdf::text`](crate::Pdf::text): what each page
//! shows, by line, in reading order.
//!
//! The content streams are walked with the text and graphics state, so
//! every shown string has a position on the page. Strings are decoded
//! glyph by glyph through the font's encoding (`ToUnicode` for composite
//! fonts); marked content with `/ActualText` replaces the glyphs it covers,
//! and `/ReversedChars` (or a run of right-to-left letters) puts glyphs
//! shown in visual order back in logical order. Strings on one baseline
//! form a line, ordered left to right, with a space where there is a gap;
//! lines that are mostly right to left are then put in reading order, run
//! by run. Form XObjects are followed, so overlays are included.

use std::collections::{BTreeMap, HashMap, HashSet};

use lopdf::Encoding;
use lopdf::content::Content;
use lopdf::{DecompressError, Dictionary, Document, Object, ObjectId};

use crate::DocumentsError;

/// Decompressed content allowed per stream (64 MiB).
const MAX_CONTENT: usize = 64 * 1024 * 1024;
/// How deep form XObjects may nest.
const MAX_FORM_DEPTH: usize = 8;
/// What one font's `ToUnicode` map may inflate to (8 MiB; real ones are
/// kilobytes).
const MAX_TO_UNICODE: usize = 8 * 1024 * 1024;
/// What each time a form is drawn costs besides its content, in bytes of
/// work: looking it up, inflating it, setting it up.
const FORM_DRAW_COST: usize = 1024;
/// How deep saved graphics states (`q`) and marked content (`BMC`, `BDC`)
/// may nest in one content stream (256). Each level is held until it is
/// closed. The PDF 1.7 reference gives 28 as an implementation limit for
/// `q`; 256 leaves room for files that never close what they open.
const MAX_NESTING: usize = 256;
/// What reading a font costs in work for each byte of memory it builds
/// (one byte for every 16): building the memory is the work.
const MEMORY_PER_WORK: usize = 16;
/// What each character width a font sets costs, in bytes of work (16).
const WIDTH_COST: usize = 16;
/// What each character width a font sets holds in memory, at most (24
/// bytes: an entry of a hash map, with room to grow).
const WIDTH_MEMORY: usize = 24;
/// What a font holds besides its widths and maps (256 bytes).
const FONT_INFO_MEMORY: usize = 256;

// What lopdf holds while it reads a ToUnicode map, measured on its
// structures and rounded up (see `to_unicode_memory`).

/// For each byte of the map: the lines it parses before it builds the map.
const MAP_TEXT_MEMORY: u64 = 8;
/// For each range it keeps, besides the text the range maps to.
const MAP_RANGE_MEMORY: u64 = 96;
/// For each code it covers: the entry of the reverse lookup it builds for
/// every code, besides the code's text. Measured at about 190 bytes once
/// read and 230 at the peak of reading, for a map of every two-byte code.
const MAP_CODE_MEMORY: u64 = 256;
/// For each text of more than one UTF-16 unit, besides its units.
const MAP_TEXT_VALUE_MEMORY: u64 = 32;
/// For each text of a list (`[<..> <..>]`), besides its units.
const MAP_LIST_ITEM_MEMORY: u64 = 56;
/// The highest code a composite font can show: codes have two bytes.
const MAX_COMPOSITE_CODE: u32 = 0xFFFF;
/// The highest code a simple font can show: codes have one byte.
const MAX_SIMPLE_CODE: u64 = 0xFF;

/// How much work reading the text of a PDF may take, so that a small file
/// cannot make a call run for hours or fill the memory. Every figure is
/// per call unless it says otherwise.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Limits {
    /// Bytes of work one page may take (128 MiB): its content decoded,
    /// counting a form again every time it is drawn, and
    /// [`FORM_DRAW_COST`] for each draw; the fonts it reads (a `ToUnicode`
    /// map's inflated bytes and one byte for every [`MEMORY_PER_WORK`] of
    /// the memory reading it builds, and [`WIDTH_COST`] for each width);
    /// and the actual text of marked content each time it is decoded.
    pub(crate) page_work: usize,
    /// The same, over all the pages (1 GiB).
    pub(crate) document_work: usize,
    /// Glyphs one page may show (1,000,000).
    pub(crate) page_glyphs: usize,
    /// Comparisons one page may spend attaching marks to the glyphs they
    /// sit on, counting the bytes of text compared (50,000,000).
    pub(crate) page_steps: usize,
    /// Bytes of text one page may produce, before lines are put together
    /// (8 MiB). A dense page of text is tens of kilobytes; a million glyphs
    /// of Latin text is about 1 MB, of Chinese about 3 MB.
    pub(crate) page_text: usize,
    /// The same, over all the pages (64 MiB): some fifteen thousand dense
    /// pages. The text is returned whole, so this is memory as well.
    pub(crate) document_text: usize,
    /// Memory the fonts and `ToUnicode` maps read for a document may hold
    /// at once (256 MiB), as lopdf holds it: [`MAP_CODE_MEMORY`] for each
    /// code a map covers, 17 MB for a map of every two-byte code, of which
    /// fifteen fit. Past the limit, what was used longest ago is dropped
    /// and read again when a page needs it. One map that would take more on
    /// its own is refused.
    pub(crate) font_memory: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            page_work: 128 * 1024 * 1024,
            document_work: 1024 * 1024 * 1024,
            page_glyphs: 1_000_000,
            page_steps: 50_000_000,
            page_text: 8 * 1024 * 1024,
            document_text: 64 * 1024 * 1024,
            font_memory: 256 * 1024 * 1024,
        }
    }
}

/// The text of every page, in page order.
pub(crate) fn page_texts(document: &Document) -> Result<Vec<String>, DocumentsError> {
    page_texts_within(document, Limits::default())
}

/// [`page_texts`] under `limits`.
pub(crate) fn page_texts_within(
    document: &Document,
    limits: Limits,
) -> Result<Vec<String>, DocumentsError> {
    let mut texts = Vec::new();
    let (mut work, mut text) = (0usize, 0usize);
    // Fonts and their maps are kept for the whole document, within
    // `limits.font_memory`.
    let mut fonts = FontCache::default();
    for page_id in document.get_pages().into_values() {
        let mut walker = Walker {
            document,
            chunks: Vec::new(),
            fonts,
            font_names: HashMap::new(),
            budget: Budget::new(limits, work, text),
        };
        let content = inflate(&walker.budget, |limit| {
            document.get_page_content_with_limit(page_id, limit)
        })?
        .map_err(|e| DocumentsError::Pdf(e.to_string()))?;
        let resources = page_resources(document, page_id);
        walker.walk(&content, resources, IDENTITY, 0)?;
        work += walker.budget.work;
        text += walker.budget.text;
        fonts = walker.fonts;
        texts.push(assemble(walker.chunks, limits.page_steps)?);
    }
    Ok(texts)
}

/// What a page may still spend: work and text, each held to the page's
/// limit and to what is left of the document's.
struct Budget {
    limits: Limits,
    /// The work done on this page so far, and the most it may take.
    work: usize,
    work_allowed: usize,
    /// Whether the document runs out of work before the page does.
    work_by_document: bool,
    /// The text this page produced so far, and the most it may produce.
    text: usize,
    text_allowed: usize,
    /// Whether the document runs out of text before the page does.
    text_by_document: bool,
}

impl Budget {
    /// A page's budget, after the pages before it did `work` and produced
    /// `text`.
    fn new(limits: Limits, work: usize, text: usize) -> Self {
        let work_left = limits.document_work.saturating_sub(work);
        let text_left = limits.document_text.saturating_sub(text);
        Self {
            limits,
            work: 0,
            work_allowed: limits.page_work.min(work_left),
            work_by_document: work_left < limits.page_work,
            text: 0,
            text_allowed: limits.page_text.min(text_left),
            text_by_document: text_left < limits.page_text,
        }
    }

    /// The work the page may still do.
    fn work_left(&self) -> usize {
        self.work_allowed.saturating_sub(self.work)
    }

    /// Counts `amount` bytes of work, or fails when the page (or the
    /// document) has used all it may.
    fn charge(&mut self, amount: usize) -> Result<(), DocumentsError> {
        self.work = self.work.saturating_add(amount);
        if self.work > self.work_allowed {
            return Err(self.out_of_work());
        }
        Ok(())
    }

    fn out_of_work(&self) -> DocumentsError {
        let counted = "counting the content each time a form is drawn, the fonts' ToUnicode \
                       maps and widths, and the actual text of marked content";
        DocumentsError::TooLarge(if self.work_by_document {
            format!(
                "a PDF takes more work to read than the limit of {} for the whole document, \
                 {counted}",
                size(self.limits.document_work)
            )
        } else {
            format!(
                "a PDF page takes more work to read than the limit of {} for a page, {counted}",
                size(self.limits.page_work)
            )
        })
    }

    /// Counts `amount` bytes of text produced, or fails when the page (or
    /// the document) has produced all it may.
    fn produce(&mut self, amount: usize) -> Result<(), DocumentsError> {
        self.text = self.text.saturating_add(amount);
        if self.text > self.text_allowed {
            return Err(DocumentsError::TooLarge(if self.text_by_document {
                format!(
                    "a PDF has more text than the limit of {} for the whole document",
                    size(self.limits.document_text)
                )
            } else {
                format!(
                    "a PDF page has more text than the limit of {} for a page",
                    size(self.limits.page_text)
                )
            }));
        }
        Ok(())
    }
}

/// `bytes` in the largest unit that divides it: `128 MiB`, `8 KiB`,
/// `61445 bytes`.
fn size(bytes: usize) -> String {
    for (unit, name) in [(1 << 30, "GiB"), (1 << 20, "MiB"), (1 << 10, "KiB")] {
        if bytes >= unit && bytes.is_multiple_of(unit) {
            return format!("{} {name}", bytes / unit);
        }
    }
    format!("{bytes} bytes")
}

/// Inflates a content stream with `read`, within the work the page has
/// left (and [`MAX_CONTENT`]), so that nothing is inflated that the page
/// could not pay for. Going over is an error; `read`'s other errors are
/// returned inside, for the caller to decide.
fn inflate(
    budget: &Budget,
    read: impl FnOnce(usize) -> lopdf::Result<Vec<u8>>,
) -> Result<lopdf::Result<Vec<u8>>, DocumentsError> {
    let limit = budget.work_left().min(MAX_CONTENT);
    match read(limit) {
        Err(lopdf::Error::Decompress(DecompressError::MemoryLimitExceeded { .. })) => {
            Err(if limit < MAX_CONTENT {
                budget.out_of_work()
            } else {
                DocumentsError::TooLarge(format!(
                    "a PDF content stream inflates to more than {}",
                    size(MAX_CONTENT)
                ))
            })
        }
        other => Ok(other),
    }
}

/// A 2D affine matrix `[a b c d e f]` as PDF writes them.
type Matrix = [f32; 6];

const IDENTITY: Matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// `m1 × m2`.
fn multiply(m1: Matrix, m2: Matrix) -> Matrix {
    [
        m1[0] * m2[0] + m1[1] * m2[2],
        m1[0] * m2[1] + m1[1] * m2[3],
        m1[2] * m2[0] + m1[3] * m2[2],
        m1[2] * m2[1] + m1[3] * m2[3],
        m1[4] * m2[0] + m1[5] * m2[2] + m2[4],
        m1[4] * m2[1] + m1[5] * m2[3] + m2[5],
    ]
}

fn apply(m: Matrix, x: f32, y: f32) -> (f32, f32) {
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

fn translate(x: f32, y: f32) -> Matrix {
    [1.0, 0.0, 0.0, 1.0, x, y]
}

/// One shown string, placed on the page.
#[derive(Clone, Debug)]
struct Chunk {
    /// Start along the baseline direction.
    x0: f32,
    /// End along the baseline direction.
    x1: f32,
    /// Baseline position across the baseline direction.
    y: f32,
    /// Baseline direction in whole degrees (0 for horizontal text).
    angle: i32,
    size: f32,
    text: String,
}

/// What a font needs for decoding and widths.
struct FontInfo<'a> {
    decoding: Decoding<'a>,
    /// Bytes per character code: 2 for composite fonts, 1 otherwise.
    code_len: usize,
    widths: HashMap<u32, f32>,
    default_width: f32,
    /// A standard font without a `Widths` array.
    standard: bool,
}

#[derive(Clone)]
struct TextState {
    font: Option<Vec<u8>>,
    size: f32,
    char_spacing: f32,
    word_spacing: f32,
    scale: f32,
    leading: f32,
    rise: f32,
}

impl Default for TextState {
    fn default() -> Self {
        Self {
            font: None,
            size: 0.0,
            char_spacing: 0.0,
            word_spacing: 0.0,
            scale: 1.0,
            leading: 0.0,
            rise: 0.0,
        }
    }
}

/// An open marked-content sequence.
struct Mark {
    actual_text: Option<String>,
    /// The chunk the actual text went into.
    chunk: Option<usize>,
}

/// How lopdf decodes a font's strings.
enum Decoding<'a> {
    /// With the encoding read with the font.
    Own(Option<Encoding<'a>>),
    /// With its `ToUnicode` map alone: the map stream, read once for every
    /// font that has it.
    Map(&'a lopdf::Stream),
}

/// What the cache holds, with the memory it takes and when it was last used.
struct Held<T> {
    value: T,
    memory: usize,
    used: u64,
}

/// The fonts and `ToUnicode` maps read so far in a document, by the address
/// of their dictionary or stream (which stays put while the document is
/// read), within [`Limits::font_memory`].
#[derive(Default)]
struct FontCache<'a> {
    fonts: HashMap<usize, Held<FontInfo<'a>>>,
    maps: HashMap<usize, Held<Option<Encoding<'a>>>>,
    /// The memory all of it holds.
    memory: usize,
    /// Counts the runs of text shown: what the current one uses is never
    /// dropped.
    clock: u64,
}

fn address<T>(value: &T) -> usize {
    value as *const T as usize
}

impl<'a> FontCache<'a> {
    /// Makes sure `font`, and the map it decodes with, are read, reading
    /// what is not (again, if it was dropped), and marks them used now.
    fn read(
        &mut self,
        document: &'a Document,
        font: &'a Dictionary,
        budget: &mut Budget,
    ) -> Result<(), DocumentsError> {
        let limit = budget.limits.font_memory;
        let key = address(font);
        match self.fonts.get_mut(&key) {
            Some(held) => held.used = self.clock,
            None => {
                let (value, memory) = font_info(document, font, budget)?;
                self.make_room(memory, limit)?;
                self.memory += memory;
                let used = self.clock;
                self.fonts.insert(
                    key,
                    Held {
                        value,
                        memory,
                        used,
                    },
                );
            }
        }
        let Decoding::Map(stream) = self.fonts[&key].value.decoding else {
            return Ok(());
        };
        match self.maps.get_mut(&address(stream)) {
            Some(held) => held.used = self.clock,
            None => {
                let memory = map_memory(stream, budget)?;
                self.make_room(memory, limit)?;
                // The map alone decides the encoding of any font that
                // decodes with it alone: this font's is every such font's.
                let value = match font.get_font_encoding_with_limit(document, MAX_TO_UNICODE) {
                    Ok(encoding) => Some(encoding),
                    Err(lopdf::Error::Decompress(DecompressError::MemoryLimitExceeded {
                        ..
                    })) => return Err(to_unicode_too_large()),
                    Err(_) => None,
                };
                self.memory += memory;
                let used = self.clock;
                self.maps.insert(
                    address(stream),
                    Held {
                        value,
                        memory,
                        used,
                    },
                );
            }
        }
        Ok(())
    }

    /// Drops what was used longest ago, but not what is in use now, until
    /// `memory` more fits under `limit`; fails when it cannot fit at all.
    fn make_room(&mut self, memory: usize, limit: usize) -> Result<(), DocumentsError> {
        if memory > limit {
            return Err(font_too_large(limit));
        }
        while self.memory + memory > limit {
            let font = oldest_of(&self.fonts, self.clock);
            let map = oldest_of(&self.maps, self.clock);
            let dropped = match (font, map) {
                (Some(font), Some(map)) if map.0 < font.0 => {
                    self.maps.remove(&map.1).map(|held| held.memory)
                }
                (Some(font), _) => self.fonts.remove(&font.1).map(|held| held.memory),
                (None, Some(map)) => self.maps.remove(&map.1).map(|held| held.memory),
                // Only what is in use now is left.
                (None, None) => break,
            };
            self.memory -= dropped.unwrap_or(0);
        }
        Ok(())
    }
}

/// When the entry of `held` used longest ago, before `clock`, was used, and
/// its key.
fn oldest_of<T>(held: &HashMap<usize, Held<T>>, clock: u64) -> Option<(u64, usize)> {
    held.iter()
        .filter(|(_, held)| held.used < clock)
        .map(|(&key, held)| (held.used, key))
        .min()
}

struct Walker<'a> {
    document: &'a Document,
    chunks: Vec<Chunk>,
    fonts: FontCache<'a>,
    /// The font dictionary a font name stands for, by the address of the
    /// resources it is looked up in.
    font_names: HashMap<(Option<usize>, Vec<u8>), Option<&'a Dictionary>>,
    budget: Budget,
}

fn too_deep(what: &str) -> DocumentsError {
    DocumentsError::TooLarge(format!("a PDF page {what} more than {MAX_NESTING} deep"))
}

impl<'a> Walker<'a> {
    fn walk(
        &mut self,
        content: &[u8],
        resources: Option<&'a Dictionary>,
        base: Matrix,
        depth: usize,
    ) -> Result<(), DocumentsError> {
        self.budget.charge(content.len())?;
        let resources_key = resources.map(|r| r as *const Dictionary as usize);
        let mut ctm = base;
        let mut stack: Vec<(Matrix, TextState)> = Vec::new();
        let mut state = TextState::default();
        let mut tm = IDENTITY;
        let mut tlm = IDENTITY;
        let mut marks: Vec<Mark> = Vec::new();
        // One operation at a time: a stream of millions of operations is
        // never held decoded at once.
        for op in Content::decode_iter(content) {
            let op = op.map_err(|e| DocumentsError::Pdf(e.to_string()))?;
            let operands = &op.operands;
            let number = |i: usize| {
                operands
                    .get(i)
                    .and_then(|o| o.as_float().ok())
                    .unwrap_or(0.0)
            };
            match op.operator.as_str() {
                "q" => {
                    if stack.len() >= MAX_NESTING {
                        return Err(too_deep("saves the graphics state (q)"));
                    }
                    stack.push((ctm, state.clone()));
                }
                "Q" => {
                    if let Some((saved_ctm, saved_state)) = stack.pop() {
                        ctm = saved_ctm;
                        state = saved_state;
                    }
                }
                "cm" => {
                    let m = [
                        number(0),
                        number(1),
                        number(2),
                        number(3),
                        number(4),
                        number(5),
                    ];
                    ctm = multiply(m, ctm);
                }
                "BT" => {
                    tm = IDENTITY;
                    tlm = IDENTITY;
                }
                "Tf" => {
                    state.font = operands
                        .first()
                        .and_then(|o| o.as_name().ok())
                        .map(<[u8]>::to_vec);
                    state.size = number(1);
                }
                "Tc" => state.char_spacing = number(0),
                "Tw" => state.word_spacing = number(0),
                "Tz" => state.scale = number(0) / 100.0,
                "TL" => state.leading = number(0),
                "Ts" => state.rise = number(0),
                "Td" => {
                    tlm = multiply(translate(number(0), number(1)), tlm);
                    tm = tlm;
                }
                "TD" => {
                    state.leading = -number(1);
                    tlm = multiply(translate(number(0), number(1)), tlm);
                    tm = tlm;
                }
                "Tm" => {
                    tlm = [
                        number(0),
                        number(1),
                        number(2),
                        number(3),
                        number(4),
                        number(5),
                    ];
                    tm = tlm;
                }
                "T*" => {
                    tlm = multiply(translate(0.0, -state.leading), tlm);
                    tm = tlm;
                }
                "Tj" | "'" | "\"" | "TJ" => {
                    if op.operator == "'" || op.operator == "\"" {
                        tlm = multiply(translate(0.0, -state.leading), tlm);
                        tm = tlm;
                    }
                    if op.operator == "\"" {
                        state.word_spacing = number(0);
                        state.char_spacing = number(1);
                    }
                    let shown: Vec<&Object> = match op.operator.as_str() {
                        "TJ" => operands
                            .first()
                            .and_then(|o| o.as_array().ok())
                            .map(|items| items.iter().collect())
                            .unwrap_or_default(),
                        "\"" => operands.get(2).into_iter().collect(),
                        _ => operands.first().into_iter().collect(),
                    };
                    self.show(
                        &shown,
                        resources,
                        resources_key,
                        &state,
                        &mut tm,
                        ctm,
                        &mut marks,
                    )?;
                }
                // `/ReversedChars` needs no handling: glyphs are ordered by
                // where they are drawn, not by the order they are shown in.
                "BMC" | "BDC" => {
                    if marks.len() >= MAX_NESTING {
                        return Err(too_deep("nests marked content (BMC, BDC)"));
                    }
                    let actual_text = operands
                        .get(1)
                        .and_then(|o| self.properties(o, resources))
                        .and_then(|p| p.get(b"ActualText").ok())
                        .and_then(|o| self.document.dereference(o).ok())
                        .and_then(|(_, o)| o.as_str().ok())
                        .map(decode_text_string);
                    if let Some(text) = &actual_text {
                        self.budget.charge(text.len())?;
                    }
                    marks.push(Mark {
                        actual_text,
                        chunk: None,
                    });
                }
                "EMC" => {
                    marks.pop();
                }
                "Do" if depth < MAX_FORM_DEPTH => {
                    let Some(name) = operands.first().and_then(|o| o.as_name().ok()) else {
                        continue;
                    };
                    let Some(form) = self.form(resources, name) else {
                        continue;
                    };
                    self.budget.charge(FORM_DRAW_COST)?;
                    let matrix = form
                        .dict
                        .get(b"Matrix")
                        .and_then(Object::as_array)
                        .ok()
                        .filter(|m| m.len() == 6)
                        .map(|m| {
                            let mut out = IDENTITY;
                            for (slot, value) in out.iter_mut().zip(m) {
                                *slot = value.as_float().unwrap_or(0.0);
                            }
                            out
                        })
                        .unwrap_or(IDENTITY);
                    let form_resources = form
                        .dict
                        .get(b"Resources")
                        .ok()
                        .and_then(|o| self.document.dereference(o).ok())
                        .and_then(|(_, o)| o.as_dict().ok())
                        .or(resources);
                    let content = inflate(&self.budget, |limit| {
                        form.decompressed_content_with_limit(limit)
                    })?
                    .unwrap_or_else(|_| form.content.clone());
                    self.walk(&content, form_resources, multiply(matrix, ctm), depth + 1)?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn show(
        &mut self,
        shown: &[&Object],
        resources: Option<&'a Dictionary>,
        resources_key: Option<usize>,
        state: &TextState,
        tm: &mut Matrix,
        ctm: Matrix,
        marks: &mut [Mark],
    ) -> Result<(), DocumentsError> {
        let Some(font_name) = state.font.as_deref() else {
            return Ok(());
        };
        let Some(font_key) = self.font(resources, resources_key, font_name)? else {
            return Ok(());
        };
        let Some(font) = self.fonts.fonts.get(&font_key).map(|held| &held.value) else {
            return Ok(());
        };
        let encoding = match &font.decoding {
            Decoding::Own(encoding) => encoding.as_ref(),
            Decoding::Map(stream) => self
                .fonts
                .maps
                .get(&address(*stream))
                .and_then(|held| held.value.as_ref()),
        };
        let device = multiply(*tm, ctm);
        let size = state.size * (device[0] * device[3] - device[1] * device[2]).abs().sqrt();
        // Positions are measured along the baseline: rotated text (a
        // diagonal stamp) forms lines in its own direction.
        let angle = device[1].atan2(device[0]);
        let degrees = angle.to_degrees().round() as i32;
        let (sin, cos) = (-angle).sin_cos();
        let frame = |(x, y): (f32, f32)| (x * cos - y * sin, x * sin + y * cos);
        // Marked content with actual text replaces the glyphs it covers:
        // one item spanning them all.
        let actual = marks.iter().rposition(|m| m.actual_text.is_some());
        for item in shown {
            match item {
                Object::String(bytes, _) => {
                    for code in bytes.chunks(font.code_len) {
                        let start = frame(apply(multiply(*tm, ctm), 0.0, state.rise));
                        let value = code.iter().fold(0u32, |acc, b| acc * 256 + u32::from(*b));
                        let width = font.width(value);
                        let mut advance = width / 1000.0 * state.size + state.char_spacing;
                        if font.code_len == 1 && value == 32 {
                            advance += state.word_spacing;
                        }
                        *tm = multiply(translate(advance * state.scale, 0.0), *tm);
                        let end = frame(apply(multiply(*tm, ctm), 0.0, state.rise));
                        let (x0, x1) = if start.0 <= end.0 {
                            (start.0, end.0)
                        } else {
                            (end.0, start.0)
                        };
                        if let Some(index) = actual {
                            let mark = &mut marks[index];
                            match mark.chunk.and_then(|i| self.chunks.get_mut(i)) {
                                Some(chunk) => {
                                    chunk.x0 = chunk.x0.min(x0);
                                    chunk.x1 = chunk.x1.max(x1);
                                }
                                None => {
                                    if self.chunks.len() >= self.budget.limits.page_glyphs {
                                        return Err(too_many_glyphs(
                                            self.budget.limits.page_glyphs,
                                        ));
                                    }
                                    let text = mark.actual_text.clone().unwrap_or_default();
                                    self.budget.produce(text.len())?;
                                    mark.chunk = Some(self.chunks.len());
                                    self.chunks.push(Chunk {
                                        x0,
                                        x1,
                                        y: start.1,
                                        angle: degrees,
                                        size,
                                        text,
                                    });
                                }
                            }
                            continue;
                        }
                        let text = encoding
                            .and_then(|e| e.bytes_to_string(code).ok())
                            .unwrap_or_default();
                        if text.is_empty() || text == "\u{fffd}" {
                            continue;
                        }
                        if self.chunks.len() >= self.budget.limits.page_glyphs {
                            return Err(too_many_glyphs(self.budget.limits.page_glyphs));
                        }
                        self.budget.produce(text.len())?;
                        self.chunks.push(Chunk {
                            x0,
                            x1,
                            y: start.1,
                            angle: degrees,
                            size,
                            text,
                        });
                    }
                }
                other => {
                    let adjust = other.as_float().unwrap_or(0.0);
                    *tm = multiply(
                        translate(-adjust / 1000.0 * state.size * state.scale, 0.0),
                        *tm,
                    );
                }
            }
        }
        Ok(())
    }

    /// The font `name` stands for in `resources`, read if it is not held:
    /// the address of its dictionary, its key in [`fonts`](Self::fonts).
    fn font(
        &mut self,
        resources: Option<&'a Dictionary>,
        resources_key: Option<usize>,
        name: &[u8],
    ) -> Result<Option<usize>, DocumentsError> {
        let key = (resources_key, name.to_vec());
        let dictionary = match self.font_names.get(&key) {
            Some(found) => *found,
            None => {
                let found = resources.and_then(|r| self.font_dictionary(r, name));
                self.font_names.insert(key, found);
                found
            }
        };
        let Some(dictionary) = dictionary else {
            return Ok(None);
        };
        // A new run of text: what it reads is not dropped while it is shown.
        self.fonts.clock += 1;
        self.fonts
            .read(self.document, dictionary, &mut self.budget)?;
        Ok(Some(address(dictionary)))
    }

    fn font_dictionary(&self, resources: &'a Dictionary, name: &[u8]) -> Option<&'a Dictionary> {
        let doc = self.document;
        resources
            .get(b"Font")
            .ok()
            .and_then(|o| doc.dereference(o).ok())
            .and_then(|(_, o)| o.as_dict().ok())
            .and_then(|fonts| fonts.get(name).ok())
            .and_then(|o| doc.dereference(o).ok())
            .and_then(|(_, o)| o.as_dict().ok())
    }

    fn properties<'o>(
        &self,
        operand: &'o Object,
        resources: Option<&'a Dictionary>,
    ) -> Option<&'o Dictionary>
    where
        'a: 'o,
    {
        match operand {
            Object::Dictionary(dict) => Some(dict),
            Object::Name(name) => resources
                .and_then(|r| r.get(b"Properties").ok())
                .and_then(|o| self.document.dereference(o).ok())
                .and_then(|(_, o)| o.as_dict().ok())
                .and_then(|props| props.get(name).ok())
                .and_then(|o| self.document.dereference(o).ok())
                .and_then(|(_, o)| o.as_dict().ok()),
            _ => None,
        }
    }

    fn form(&self, resources: Option<&'a Dictionary>, name: &[u8]) -> Option<&'a lopdf::Stream> {
        let xobjects = resources?
            .get(b"XObject")
            .ok()
            .and_then(|o| self.document.dereference(o).ok())
            .and_then(|(_, o)| o.as_dict().ok())?;
        let (_, object) = self.document.dereference(xobjects.get(name).ok()?).ok()?;
        let stream = object.as_stream().ok()?;
        let subtype = stream.dict.get(b"Subtype").and_then(Object::as_name).ok()?;
        (subtype == b"Form").then_some(stream)
    }
}

fn to_unicode_too_large() -> DocumentsError {
    DocumentsError::TooLarge(format!(
        "a font's ToUnicode map inflates to more than {}",
        size(MAX_TO_UNICODE)
    ))
}

/// Reads `font` for decoding and widths, charging `budget` for it: what
/// it needs and the memory that holds. A font that decodes with its
/// `ToUnicode` map alone leaves the map to [`FontCache::read`], so that
/// fonts sharing a map read it once.
fn font_info<'a>(
    doc: &'a Document,
    font: &'a Dictionary,
    budget: &mut Budget,
) -> Result<(FontInfo<'a>, usize), DocumentsError> {
    let mut memory = FONT_INFO_MEMORY;
    let decoding = match map_alone(doc, font) {
        Some(map) => Decoding::Map(map),
        None => {
            if let Some(map) = map_under_differences(doc, font) {
                memory += map_memory(map, budget)?;
            }
            Decoding::Own(
                match font.get_font_encoding_with_limit(doc, MAX_TO_UNICODE) {
                    Ok(encoding) => Some(encoding),
                    Err(lopdf::Error::Decompress(DecompressError::MemoryLimitExceeded {
                        ..
                    })) => {
                        return Err(to_unicode_too_large());
                    }
                    Err(_) => None,
                },
            )
        }
    };
    let composite = font
        .get(b"Subtype")
        .and_then(Object::as_name)
        .is_ok_and(|s| s == b"Type0");
    let mut widths = HashMap::new();
    let mut default_width = 0.0;
    let mut standard = false;
    if composite {
        default_width = 1000.0;
        let descendant = font
            .get(b"DescendantFonts")
            .ok()
            .and_then(|o| doc.dereference(o).ok())
            .and_then(|(_, o)| o.as_array().ok())
            .and_then(|a| a.first())
            .and_then(|o| doc.dereference(o).ok())
            .and_then(|(_, o)| o.as_dict().ok());
        if let Some(descendant) = descendant {
            if let Ok(dw) = descendant.get(b"DW").and_then(Object::as_float) {
                default_width = dw;
            }
            let w = descendant
                .get(b"W")
                .ok()
                .and_then(|o| doc.dereference(o).ok())
                .and_then(|(_, o)| o.as_array().ok());
            if let Some(w) = w {
                parse_cid_widths(doc, w, &mut widths, budget)?;
            }
        }
    } else {
        let first = font
            .get(b"FirstChar")
            .and_then(Object::as_i64)
            .unwrap_or(0)
            .max(0) as u32;
        let list = font
            .get(b"Widths")
            .ok()
            .and_then(|o| doc.dereference(o).ok())
            .and_then(|(_, o)| o.as_array().ok());
        match list {
            Some(list) => {
                // Codes of one byte: the rest of the list is never used.
                for (offset, width) in list.iter().enumerate() {
                    let code = u64::from(first) + offset as u64;
                    if code > MAX_SIMPLE_CODE {
                        break;
                    }
                    let width = doc
                        .dereference(width)
                        .ok()
                        .and_then(|(_, o)| o.as_float().ok())
                        .unwrap_or(0.0);
                    widths.insert(code as u32, width);
                }
            }
            None => standard = true,
        }
    }
    memory += widths.len() * WIDTH_MEMORY;
    let info = FontInfo {
        decoding,
        code_len: if composite { 2 } else { 1 },
        widths,
        default_width,
        standard,
    };
    Ok((info, memory))
}

impl FontInfo<'_> {
    fn width(&self, code: u32) -> f32 {
        if self.standard {
            return crate::stamp::standard_width(code) as f32;
        }
        self.widths
            .get(&code)
            .copied()
            .unwrap_or(self.default_width)
    }
}

/// A CID font's `W` array: `c [w1 w2 ..]` or `c_first c_last w`, for the
/// codes a composite font can show (up to [`MAX_COMPOSITE_CODE`]); each
/// width set costs [`WIDTH_COST`].
fn parse_cid_widths(
    doc: &Document,
    w: &[Object],
    widths: &mut HashMap<u32, f32>,
    budget: &mut Budget,
) -> Result<(), DocumentsError> {
    let value = |o: &Object| doc.dereference(o).ok().and_then(|(_, o)| o.as_float().ok());
    let mut i = 0;
    while i < w.len() {
        let Some(first) = value(&w[i]) else {
            break;
        };
        let first = first as u32;
        match w.get(i + 1).map(|o| doc.dereference(o).map(|(_, o)| o)) {
            Some(Ok(Object::Array(list))) => {
                for (offset, width) in list.iter().enumerate() {
                    let code = u64::from(first) + offset as u64;
                    if code > u64::from(MAX_COMPOSITE_CODE) {
                        break;
                    }
                    if let Some(width) = value(width) {
                        budget.charge(WIDTH_COST)?;
                        widths.insert(code as u32, width);
                    }
                }
                i += 2;
            }
            Some(Ok(_)) => {
                let (Some(last), Some(width)) =
                    (w.get(i + 1).and_then(value), w.get(i + 2).and_then(value))
                else {
                    break;
                };
                let last = (last as u32)
                    .min(first.saturating_add(65_535))
                    .min(MAX_COMPOSITE_CODE);
                if first <= last {
                    budget.charge((last - first + 1) as usize * WIDTH_COST)?;
                    for code in first..=last {
                        widths.insert(code, width);
                    }
                }
                i += 3;
            }
            _ => break,
        }
    }
    Ok(())
}

/// The memory lopdf can take to read the ToUnicode map `cmap`, in bytes,
/// never below it. lopdf parses the map's lines, keeps a range for each
/// (each split of a range by a later line keeps a copy of its text, so a
/// long list of texts split many times is copied many times), and builds a
/// reverse lookup with an entry for every code of every range: one line of
/// twenty bytes can cover four billion codes.
///
/// The estimate counts [`MAP_TEXT_MEMORY`] for each byte of the map; for
/// each line, two ranges of [`MAP_RANGE_MEMORY`] holding the largest text
/// any range holds, and [`MAP_CODE_MEMORY`]; and for each code a range
/// covers, [`MAP_CODE_MEMORY`] and its text. A line lopdf reads as a range
/// is two codes in angle brackets with only spaces or tabs between them,
/// of the same length, then its text (`<..>` or `[..]`); every such pair is
/// counted wherever it is in the map, so that no comment or string can hide
/// one. A text with a comment inside, which real maps never have, counts as
/// the longest lopdf reads. Codes of one or two bytes are at most 65,792,
/// however many lines cover them.
fn to_unicode_memory(cmap: &[u8]) -> usize {
    /// Codes of one or two bytes there can be.
    const SHORT_CODES: u64 = 256 + 65_536;
    let length = cmap.len() as u64;
    let spaces = |at: usize| {
        at + cmap[at.min(cmap.len())..]
            .iter()
            .take_while(|&&b| b == b' ' || b == b'\t')
            .count()
    };
    let value = |digits: &[u8]| {
        std::str::from_utf8(digits)
            .ok()
            .and_then(|digits| u64::from_str_radix(digits, 16).ok())
    };
    // Texts in angle brackets, their UTF-16 units in bytes, and the most
    // memory one text a range keeps can take.
    let (mut texts, mut text_bytes, mut text_most) = (0u64, 0u64, 0u64);
    let mut commented = false;
    let (mut short, mut short_most, mut long) = (0u64, 0u64, 0u64);
    // The code before, if it has nothing but hex digits, and where it ended.
    let mut previous: Option<(&[u8], usize)> = None;
    let mut at = 0;
    while let Some(offset) = cmap[at..].iter().position(|&b| b == b'<') {
        let start = at + offset;
        let hex = match hex_text(cmap, start) {
            Text::Hex(hex) => hex,
            Text::Commented => {
                commented = true;
                at = start + 1;
                previous = None;
                continue;
            }
            // Not a text: go on from the next byte, which may start one.
            Text::Not => {
                at = start + 1;
                previous = None;
                continue;
            }
        };
        texts += 1;
        text_bytes += hex.digits / 2;
        text_most = text_most.max(text_memory(hex.digits / 4));
        let digits = &cmap[start + 1..hex.end - 1];
        if hex.pure
            && let Some((low, low_end)) = previous
            && spaces(low_end) == start
            && low.len() == digits.len()
            && matches!(digits.len(), 2 | 4 | 6 | 8)
            && let (Some((memory, units)), Some(low), Some(high)) =
                (range_text(cmap, spaces(hex.end)), value(low), value(digits))
            && high >= low
        {
            text_most = text_most.max(memory);
            let per_code = MAP_CODE_MEMORY + 2 * units;
            let codes = (high - low + 1).saturating_mul(per_code);
            if digits.len() <= 4 {
                short = short.saturating_add(codes);
                short_most = short_most.max(per_code);
            } else {
                long = long.saturating_add(codes);
            }
        }
        previous = hex.pure.then_some((digits, hex.end));
        at = hex.end;
    }
    if commented {
        text_most = text_most.max(text_memory(MAX_TEXT_UNITS));
        text_bytes = text_bytes.saturating_add(length / 2);
    }
    let short = short.min(SHORT_CODES.saturating_mul(short_most));
    // Every line has at least two texts in angle brackets.
    let lines = texts / 2 + 1;
    let total = (MAP_TEXT_MEMORY.saturating_mul(length))
        .saturating_add((2 * lines).saturating_mul(MAP_RANGE_MEMORY.saturating_add(text_most)))
        .saturating_add(lines.saturating_mul(MAP_CODE_MEMORY))
        .saturating_add(text_bytes)
        .saturating_add(short)
        .saturating_add(long);
    usize::try_from(total).unwrap_or(usize::MAX)
}

/// The most UTF-16 units lopdf reads in one text of a map.
const MAX_TEXT_UNITS: u64 = 256;

/// What lopdf keeps for a text of `units` UTF-16 units: one unit is kept
/// as a number.
fn text_memory(units: u64) -> u64 {
    if units <= 1 {
        0
    } else {
        MAP_TEXT_VALUE_MEMORY + 2 * units
    }
}

/// A text in angle brackets.
struct Hex {
    /// Its hex digits.
    digits: u64,
    /// Whether it has nothing but hex digits (a code must).
    pure: bool,
    /// Where it ends, after `>`.
    end: usize,
}

enum Text {
    Hex(Hex),
    /// `<` hex digits and spaces, then a comment.
    Commented,
    /// Not a text.
    Not,
}

/// The text in angle brackets at `at`: hex digits, maybe with spaces
/// between them. A comment inside ends the reading, so that it cannot hide
/// what follows.
fn hex_text(cmap: &[u8], at: usize) -> Text {
    if cmap.get(at) != Some(&b'<') {
        return Text::Not;
    }
    let (mut digits, mut pure) = (0u64, true);
    for (offset, &byte) in cmap[at + 1..].iter().enumerate() {
        match byte {
            b'>' => {
                return Text::Hex(Hex {
                    digits,
                    pure,
                    end: at + offset + 2,
                });
            }
            b'%' => return Text::Commented,
            b' ' | b'\t' | b'\r' | b'\n' | b'\0' | b'\x0C' => pure = false,
            byte if byte.is_ascii_hexdigit() => digits += 1,
            _ => return Text::Not,
        }
    }
    Text::Not
}

/// The text a range maps its codes to, at `at`: what lopdf keeps of it,
/// and the most UTF-16 units it gives one code. `None` where lopdf reads no
/// text, and then no map at all.
fn range_text(cmap: &[u8], at: usize) -> Option<(u64, u64)> {
    match cmap.get(at)? {
        b'<' => match hex_text(cmap, at) {
            Text::Hex(hex) if hex.digits > 0 && hex.digits.is_multiple_of(4) => {
                let units = hex.digits / 4;
                Some((text_memory(units), units))
            }
            Text::Commented => Some((text_memory(MAX_TEXT_UNITS), MAX_TEXT_UNITS)),
            _ => None,
        },
        // A list of texts, one for each code from the first: kept whole.
        b'[' => {
            let (mut memory, mut most) = (MAP_TEXT_VALUE_MEMORY, 0u64);
            let mut at = at + 1;
            loop {
                match cmap.get(at)? {
                    b']' => return Some((memory, most)),
                    b' ' | b'\t' | b'\r' | b'\n' | b'\0' | b'\x0C' => at += 1,
                    _ => match hex_text(cmap, at) {
                        Text::Hex(hex) => {
                            let units = hex.digits / 4;
                            memory += MAP_LIST_ITEM_MEMORY + 2 * units;
                            most = most.max(units);
                            at = hex.end;
                        }
                        // Texts that cannot be measured: as many as the rest
                        // of the map could hold, each the longest.
                        Text::Commented => {
                            let rest = (cmap.len() - at) as u64 / 3 + 1;
                            let each = MAP_LIST_ITEM_MEMORY + 2 * MAX_TEXT_UNITS;
                            return Some((memory + rest * each, MAX_TEXT_UNITS));
                        }
                        Text::Not => return None,
                    },
                }
            }
        }
        _ => None,
    }
}

/// The `ToUnicode` map lopdf decodes `font` with alone, as it decides:
/// a font with the map and no `Encoding`, or `Identity-H` or `Identity-V`
/// as its `Encoding`.
fn map_alone<'a>(document: &'a Document, font: &'a Dictionary) -> Option<&'a lopdf::Stream> {
    if !font.has_type(b"Font") {
        return None;
    }
    let map = font
        .get_deref(b"ToUnicode", document)
        .ok()?
        .as_stream()
        .ok()?;
    let Ok(mut encoding) = font.get(b"Encoding") else {
        return Some(map);
    };
    let mut seen = HashSet::new();
    loop {
        match encoding {
            Object::Reference(id) if seen.insert(*id) => {
                encoding = document.get_object(*id).ok()?;
            }
            Object::Name(name) if name == b"Identity-H" || name == b"Identity-V" => {
                return Some(map);
            }
            _ => return None,
        }
    }
}

/// The `ToUnicode` map lopdf reads with `font` when its `Encoding` is a
/// dictionary whose `BaseEncoding` is `Identity-H` or `Identity-V`.
fn map_under_differences<'a>(
    document: &'a Document,
    font: &'a Dictionary,
) -> Option<&'a lopdf::Stream> {
    let (_, encoding) = document.dereference(font.get(b"Encoding").ok()?).ok()?;
    let base = encoding
        .as_dict()
        .ok()?
        .get(b"BaseEncoding")
        .ok()?
        .as_name()
        .ok()?;
    if base != b"Identity-H" && base != b"Identity-V" {
        return None;
    }
    font.get_deref(b"ToUnicode", document)
        .ok()?
        .as_stream()
        .ok()
}

/// Counts what reading the `ToUnicode` map in `stream` takes, before lopdf
/// reads it: the memory, returned and held to the limit for fonts, and as
/// work the map's bytes and a share of the memory.
fn map_memory(stream: &lopdf::Stream, budget: &mut Budget) -> Result<usize, DocumentsError> {
    match stream.get_plain_content_with_limit(MAX_TO_UNICODE) {
        Ok(map) => {
            let memory = to_unicode_memory(&map);
            if memory > budget.limits.font_memory {
                return Err(font_too_large(budget.limits.font_memory));
            }
            budget.charge(map.len().saturating_add(memory / MEMORY_PER_WORK))?;
            Ok(memory)
        }
        Err(lopdf::Error::Decompress(DecompressError::MemoryLimitExceeded { .. })) => {
            Err(to_unicode_too_large())
        }
        // lopdf cannot read it either, and falls back.
        Err(_) => Ok(0),
    }
}

fn font_too_large(limit: usize) -> DocumentsError {
    DocumentsError::TooLarge(format!(
        "a font of a PDF takes more memory to read than the limit of {} for fonts",
        size(limit)
    ))
}

fn page_resources(document: &Document, page_id: ObjectId) -> Option<&Dictionary> {
    let mut node = document.get_dictionary(page_id).ok()?;
    for _ in 0..64 {
        if let Ok(resources) = node.get(b"Resources") {
            return document
                .dereference(resources)
                .ok()
                .and_then(|(_, o)| o.as_dict().ok());
        }
        let parent = node.get(b"Parent").and_then(Object::as_reference).ok()?;
        node = document.get_dictionary(parent).ok()?;
    }
    None
}

/// A PDF text string: UTF-16BE with a byte-order mark, UTF-8 with one, or
/// PDFDocEncoding (read as Latin-1).
pub(crate) fn decode_text_string(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xfe, 0xff]) {
        let units: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_be_bytes(*pair))
            .collect();
        String::from_utf16_lossy(&units)
    } else if let Some(rest) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        String::from_utf8_lossy(rest).into_owned()
    } else {
        bytes.iter().map(|&b| b as char).collect()
    }
}

/// Whether `c` is a strong right-to-left letter (Hebrew, Arabic, Syriac,
/// Thaana, N'Ko and their presentation forms; not the Arabic-Indic digits).
fn is_rtl_letter(c: char) -> bool {
    matches!(c as u32,
        0x0590..=0x05FF
        | 0x0600..=0x065F
        | 0x066A..=0x06EF
        | 0x06FA..=0x07FF
        | 0x0860..=0x08FF
        | 0xFB1D..=0xFDFF
        | 0xFE70..=0xFEFF
        | 0x10800..=0x10FFF
        | 0x1E800..=0x1EFFF)
}

/// (right-to-left letters, left-to-right letters) in `text`.
fn direction_counts(text: &str) -> (usize, usize) {
    text.chars().fold((0, 0), |(rtl, ltr), c| {
        if is_rtl_letter(c) {
            (rtl + 1, ltr)
        } else if c.is_alphabetic() {
            (rtl, ltr + 1)
        } else {
            (rtl, ltr)
        }
    })
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Class {
    Rtl,
    Ltr,
    Number,
    Neutral,
}

fn classify(text: &str) -> Class {
    let (rtl, ltr) = direction_counts(text);
    if rtl > 0 && rtl >= ltr {
        Class::Rtl
    } else if ltr > 0 {
        Class::Ltr
    } else if text.chars().any(|c| c.is_numeric()) {
        Class::Number
    } else {
        Class::Neutral
    }
}

fn too_many_glyphs(limit: usize) -> DocumentsError {
    DocumentsError::TooLarge(format!(
        "a PDF page shows more than {limit} glyphs, more than can be put in order"
    ))
}

/// What is left of a page's budget of comparisons.
struct Steps(usize);

impl Steps {
    /// Spends `count` comparisons, or fails when the page has used them
    /// all.
    fn spend(&mut self, count: usize) -> Result<(), DocumentsError> {
        self.0 = self.0.checked_sub(count).ok_or_else(|| {
            DocumentsError::TooLarge(
                "a PDF page has too many marks (accents, dots, vowel signs) to put on the letters \
                 they belong to"
                    .into(),
            )
        })?;
        Ok(())
    }
}

/// The glyphs of a page as lines of text; `steps` is the page's budget of
/// comparisons.
fn assemble(mut chunks: Vec<Chunk>, steps: usize) -> Result<String, DocumentsError> {
    let mut steps = Steps(steps);
    // Horizontal text first, top to bottom; then each other direction.
    chunks.sort_by(|a, b| {
        (a.angle != 0)
            .cmp(&(b.angle != 0))
            .then(a.angle.cmp(&b.angle))
            .then(b.y.total_cmp(&a.y))
    });
    let mut lines: Vec<(i32, f32, f32, Vec<Chunk>)> = Vec::new();
    for chunk in chunks {
        match lines.last_mut() {
            Some((angle, y, size, line))
                if *angle == chunk.angle
                    && (*y - chunk.y).abs() <= 0.5 * size.max(chunk.size).max(1.0) =>
            {
                *size = size.max(chunk.size);
                line.push(chunk);
            }
            _ => lines.push((chunk.angle, chunk.y, chunk.size, vec![chunk])),
        }
    }
    let mut out = Vec::with_capacity(lines.len());
    for (_, _, _, mut line) in lines {
        line.sort_by(|a, b| a.x0.total_cmp(&b.x0));
        let line = attach_marks(line, &mut steps)?;
        // Visual order, with a space where two glyphs do not touch.
        let mut visual: Vec<String> = Vec::with_capacity(line.len() * 2);
        let mut previous: Option<&Chunk> = None;
        for chunk in &line {
            if let Some(prev) = previous {
                let gap = chunk.x0 - prev.x1;
                let touching_space = prev.text.ends_with(char::is_whitespace)
                    || chunk.text.starts_with(char::is_whitespace);
                if gap > 0.15 * prev.size.max(chunk.size) && !touching_space {
                    visual.push(" ".into());
                }
            }
            visual.push(chunk.text.clone());
            previous = Some(chunk);
        }
        out.push(reading_order(visual).concat().trim().to_owned());
    }
    Ok(out.join("\n"))
}

/// Which glyphs of a line repeat another one: `duplicate[i]` is true when
/// some other glyph with the same text is wider than glyph `i` (or as wide,
/// and earlier) and spans its centre.
///
/// Within the glyphs that share a text, they are taken from the widest down
/// (earlier first among equals): each is a duplicate when the centre falls
/// in the union of the ones taken before it, kept as disjoint intervals.
/// That is the same answer as comparing every pair, in time proportional to
/// the number of glyphs times its logarithm.
fn duplicates(line: &[Chunk], steps: &mut Steps) -> Result<Vec<bool>, DocumentsError> {
    let mut duplicate = vec![false; line.len()];
    let mut by_text: HashMap<&str, Vec<usize>> = HashMap::new();
    for (index, chunk) in line.iter().enumerate() {
        by_text.entry(chunk.text.as_str()).or_default().push(index);
    }
    for group in by_text.into_values() {
        if group.len() < 2 {
            continue;
        }
        let ordered = group.iter().all(|&i| {
            let chunk = &line[i];
            chunk.x0.is_finite() && chunk.x1.is_finite()
        });
        if !ordered {
            // Coordinates that are not numbers have no order to sweep in:
            // compare each pair, as long as the page's budget lasts.
            for &i in &group {
                steps.spend(group.len())?;
                let a = &line[i];
                let centre = (a.x0 + a.x1) / 2.0;
                duplicate[i] = group.iter().any(|&j| {
                    let b = &line[j];
                    j != i
                        && b.x0 <= centre
                        && centre <= b.x1
                        && ((b.x1 - b.x0) > (a.x1 - a.x0)
                            || ((b.x1 - b.x0) == (a.x1 - a.x0) && j < i))
                });
            }
            continue;
        }
        let width = |i: usize| line[i].x1 - line[i].x0 + 0.0;
        let mut ranked = group;
        ranked.sort_by(|&a, &b| width(b).total_cmp(&width(a)).then(a.cmp(&b)));
        // Disjoint closed intervals, by start: start -> end.
        let mut covered: BTreeMap<i32, (f32, f32)> = BTreeMap::new();
        for i in ranked {
            let chunk = &line[i];
            let (start, end) = (chunk.x0 + 0.0, chunk.x1 + 0.0);
            let centre = (chunk.x0 + chunk.x1) / 2.0 + 0.0;
            duplicate[i] = covered
                .range(..=order_key(centre))
                .next_back()
                .is_some_and(|(_, &(_, to))| centre <= to);
            // Add [start, end], merging what it touches.
            let (mut from, mut to) = (start, end);
            if let Some((&key, &(before, until))) = covered.range(..=order_key(from)).next_back()
                && until >= from
            {
                from = before;
                to = to.max(until);
                covered.remove(&key);
            }
            let absorbed: Vec<i32> = covered
                .range(order_key(from)..=order_key(to))
                .map(|(&key, _)| key)
                .collect();
            for key in absorbed {
                if let Some((_, until)) = covered.remove(&key) {
                    to = to.max(until);
                }
            }
            covered.insert(order_key(from), (from, to));
        }
    }
    Ok(duplicate)
}

/// A key that sorts finite `f32`s in numeric order (`-0.0` and `0.0` as
/// one).
fn order_key(value: f32) -> i32 {
    let bits = (value + 0.0).to_bits() as i32;
    bits ^ (((bits >> 31) as u32) >> 1) as i32
}

/// Zero-width glyphs (marks: dots, vowel signs, accents drawn apart) of a
/// line in visual order, merged into the glyph they sit on: dropped when
/// they repeat its text (fonts that draw a letter's dots as a separate
/// glyph map both to the letter), appended to it otherwise.
fn attach_marks(line: Vec<Chunk>, steps: &mut Steps) -> Result<Vec<Chunk>, DocumentsError> {
    let is_mark = |c: &Chunk| (c.x1 - c.x0) < 0.05 * c.size.max(1.0);
    // A glyph centered inside another glyph with the same text is the same
    // character drawn twice (a letter's dots, or fake bold): keep the wider.
    let duplicate = duplicates(&line, steps)?;
    let line: Vec<Chunk> = line
        .into_iter()
        .zip(duplicate)
        .filter(|(_, duplicate)| !duplicate)
        .map(|(chunk, _)| chunk)
        .collect();
    let mut bases: Vec<Chunk> = Vec::with_capacity(line.len());
    let mut marks: Vec<Chunk> = Vec::new();
    for chunk in line {
        if is_mark(&chunk) && !chunk.text.trim().is_empty() {
            marks.push(chunk);
        } else {
            bases.push(chunk);
        }
    }
    for mark in marks {
        steps.spend(bases.len() + 1)?;
        let tolerance = 0.1 * mark.size.max(1.0);
        let base = bases
            .iter_mut()
            .filter(|b| b.x0 - tolerance <= mark.x0 && mark.x0 <= b.x1 + tolerance)
            .min_by(|a, b| {
                let da = (a.x0 + a.x1) / 2.0 - mark.x0;
                let db = (b.x0 + b.x1) / 2.0 - mark.x0;
                da.abs().total_cmp(&db.abs())
            });
        // Looking for the mark's text in the base's, and adding it, read
        // both: a byte is a step.
        if let Some(base) = &base {
            steps.spend(base.text.len() + mark.text.len())?;
        }
        match base {
            Some(base) if base.text == mark.text || base.text.contains(&mark.text) => {}
            Some(base) => base.text.push_str(&mark.text),
            None => {
                let at = bases.partition_point(|b| b.x0 <= mark.x0);
                bases.insert(at, mark);
            }
        }
    }
    Ok(bases)
}

/// Strings of a line in visual (left-to-right) order, each already in
/// logical order inside, put in reading order: a line that is mostly
/// right to left is read from the right, keeping left-to-right runs
/// (words, numbers) in order; in other lines, right-to-left runs are
/// reversed.
fn reading_order(visual: Vec<String>) -> Vec<String> {
    let classes: Vec<Class> = visual.iter().map(|s| classify(s)).collect();
    let (rtl, ltr) = visual
        .iter()
        .map(|s| direction_counts(s))
        .fold((0, 0), |(a, b), (c, d)| (a + c, b + d));
    let mut items: Vec<(String, Class)> = visual.into_iter().zip(classes).collect();
    if rtl > 0 && rtl >= ltr {
        items.reverse();
        reverse_runs(&mut items, |c| matches!(c, Class::Ltr | Class::Number));
    } else if rtl > 0 {
        reverse_runs(&mut items, |c| c == Class::Rtl);
    }
    items.into_iter().map(|(s, _)| s).collect()
}

/// Reverses every maximal run of items whose class is `member`, joined by
/// neutral items between two members.
fn reverse_runs(items: &mut [(String, Class)], member: impl Fn(Class) -> bool) {
    let mut i = 0;
    while i < items.len() {
        if !member(items[i].1) {
            i += 1;
            continue;
        }
        let start = i;
        let mut end = i;
        let mut j = i + 1;
        while j < items.len() {
            if member(items[j].1) {
                end = j;
                j += 1;
            } else if items[j].1 == Class::Neutral {
                j += 1;
            } else {
                break;
            }
        }
        items[start..=end].reverse();
        i = end + 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn right_to_left_lines_read_from_the_right() {
        // "مريم العلي" shown as two words, right to left.
        let visual = strings(&["العلي", " ", "مريم"]);
        assert_eq!(reading_order(visual).concat(), "مريم العلي");
    }

    #[test]
    fn left_to_right_runs_keep_their_order_in_right_to_left_lines() {
        // "مرحبا Ada Lovelace في الدورة 2026": mostly Arabic, so read from
        // the right; the name and the year stay left to right.
        let visual = strings(&[
            "2026",
            " ",
            "الدورة",
            " ",
            "في",
            " ",
            "Ada",
            " ",
            "Lovelace",
            " ",
            "مرحبا",
        ]);
        assert_eq!(
            reading_order(visual).concat(),
            "مرحبا Ada Lovelace في الدورة 2026"
        );
    }

    #[test]
    fn right_to_left_runs_are_reversed_in_left_to_right_lines() {
        // "Invoice for مرحبا عالم today": mostly Latin.
        let visual = strings(&[
            "Invoice",
            " ",
            "for",
            " ",
            "عالم",
            " ",
            "مرحبا",
            " ",
            "today",
        ]);
        assert_eq!(
            reading_order(visual).concat(),
            "Invoice for مرحبا عالم today"
        );
        let plain = strings(&["Hello", " ", "world"]);
        assert_eq!(reading_order(plain).concat(), "Hello world");
    }

    #[test]
    fn classes() {
        assert_eq!(classify("مرحبا"), Class::Rtl);
        assert_eq!(classify("abc"), Class::Ltr);
        assert_eq!(classify("42"), Class::Number);
        assert_eq!(classify("٤٢"), Class::Number);
        assert_eq!(classify(" - "), Class::Neutral);
        assert_eq!(classify("अनन्या"), Class::Ltr);
    }

    fn document_of(lines: &[&str]) -> Document {
        Document::load_mem(crate::Pdf::from_lines(lines).as_ref()).unwrap()
    }

    #[test]
    fn a_page_with_too_many_glyphs_is_refused() {
        let document = document_of(&["Hello, world"]);
        let tight = Limits {
            page_glyphs: 11,
            ..Limits::default()
        };
        let error = page_texts_within(&document, tight).unwrap_err();
        assert!(matches!(error, DocumentsError::TooLarge(_)), "{error:?}");
        let fits = Limits {
            page_glyphs: 12,
            ..Limits::default()
        };
        assert_eq!(
            page_texts_within(&document, fits).unwrap(),
            ["Hello, world"]
        );
    }

    #[test]
    fn a_page_that_takes_too_much_work_is_refused() {
        let document = document_of(&["Hello, world"]);
        let page_length = document
            .get_page_content(*document.get_pages().values().next().unwrap())
            .len();
        let tight = Limits {
            page_work: page_length - 1,
            ..Limits::default()
        };
        let error = page_texts_within(&document, tight).unwrap_err();
        assert!(matches!(error, DocumentsError::TooLarge(_)), "{error:?}");
        let fits = Limits {
            page_work: page_length,
            ..Limits::default()
        };
        assert_eq!(
            page_texts_within(&document, fits).unwrap(),
            ["Hello, world"]
        );
        // The pages together are held to their own limit.
        let two = document_of(&["one"]);
        let both = Limits {
            document_work: 1,
            ..Limits::default()
        };
        assert!(page_texts_within(&two, both).is_err());
    }

    /// `count` zero-width glyphs, 12 points apart, on one baseline: marks
    /// that sit on nothing, so each is kept as a glyph of its own.
    fn loose_marks(count: usize) -> Vec<Chunk> {
        (0..count)
            .map(|i| Chunk {
                x0: i as f32 * 12.0,
                x1: i as f32 * 12.0,
                y: 700.0,
                angle: 0,
                size: 12.0,
                text: "a".into(),
            })
            .collect()
    }

    #[test]
    fn marks_are_attached_within_a_budget_of_comparisons() {
        // 200 marks, each compared with the ones kept before it and itself:
        // 1 + 2 + ... + 200 = 20,100 comparisons.
        let text = assemble(loose_marks(200), 25_000).unwrap();
        assert_eq!(text.matches('a').count(), 200);
        let error = assemble(loose_marks(200), 15_000).unwrap_err();
        assert!(matches!(error, DocumentsError::TooLarge(_)), "{error:?}");
    }

    /// What `duplicates` computes, by comparing every glyph with every other.
    fn duplicates_by_comparison(line: &[Chunk]) -> Vec<bool> {
        line.iter()
            .enumerate()
            .map(|(i, a)| {
                let centre = (a.x0 + a.x1) / 2.0;
                line.iter().enumerate().any(|(j, b)| {
                    j != i
                        && b.text == a.text
                        && b.x0 <= centre
                        && centre <= b.x1
                        && ((b.x1 - b.x0) > (a.x1 - a.x0)
                            || ((b.x1 - b.x0) == (a.x1 - a.x0) && j < i))
                })
            })
            .collect()
    }

    #[test]
    fn duplicates_are_the_same_as_comparing_every_pair() {
        // A small generator of lines with many ties, touching ends,
        // zero widths and negative zeros: three texts, positions on a grid.
        let mut state = 0x2545_f491_4f6c_dd1du64;
        let mut next = move |bound: u64| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state % bound
        };
        for round in 0..400 {
            let count = 1 + next(40) as usize;
            let line: Vec<Chunk> = (0..count)
                .map(|_| {
                    let x0 = next(30) as f32 * 0.5 - 3.0;
                    let width = [0.0, 0.5, 1.0, 2.5, 6.0][next(5) as usize];
                    Chunk {
                        x0,
                        x1: x0 + width,
                        y: 0.0,
                        angle: 0,
                        size: 10.0,
                        text: ["a", "b", "c"][next(if round % 2 == 0 { 3 } else { 1 }) as usize]
                            .into(),
                    }
                })
                .collect();
            let mut steps = Steps(usize::MAX);
            assert_eq!(
                duplicates(&line, &mut steps).unwrap(),
                duplicates_by_comparison(&line),
                "round {round}: {line:?}"
            );
        }
    }

    #[test]
    fn glyphs_without_coordinates_are_still_compared() {
        let at = |x0: f32, x1: f32| Chunk {
            x0,
            x1,
            y: 0.0,
            angle: 0,
            size: 10.0,
            text: "a".into(),
        };
        let line = vec![
            at(0.0, 4.0),
            at(1.0, 3.0),
            at(f32::NAN, 2.0),
            at(f32::NEG_INFINITY, f32::INFINITY),
        ];
        let mut steps = Steps(usize::MAX);
        assert_eq!(
            duplicates(&line, &mut steps).unwrap(),
            duplicates_by_comparison(&line)
        );
    }

    #[test]
    fn text_strings_decode() {
        assert_eq!(decode_text_string(&[0xfe, 0xff, 0x06, 0x2a]), "ت");
        assert_eq!(decode_text_string(b"Plain"), "Plain");
    }

    use lopdf::{Stream, StringFormat, dictionary};

    /// A document of one page per entry of `contents`, all with the
    /// resources `resources` builds (it may add objects).
    fn pages_with(
        resources: impl FnOnce(&mut Document) -> Dictionary,
        contents: &[Vec<u8>],
    ) -> Document {
        let mut document = Document::with_version("1.7");
        let pages_id = document.new_object_id();
        let resources = resources(&mut document);
        let resources_id = document.add_object(resources);
        let kids: Vec<Object> = contents
            .iter()
            .map(|content| {
                let content_id =
                    document.add_object(Stream::new(Dictionary::new(), content.clone()));
                document
                    .add_object(dictionary! {
                        "Type" => "Page",
                        "Parent" => pages_id,
                        "MediaBox" => vec![0.into(), 0.into(), 600.into(), 800.into()],
                        "Resources" => resources_id,
                        "Contents" => content_id,
                    })
                    .into()
            })
            .collect();
        let count = kids.len() as i64;
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => kids, "Count" => count,
            }),
        );
        let catalog = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        document.trailer.set("Root", catalog);
        document
    }

    /// Resources with Helvetica as `/F1`.
    fn helvetica(document: &mut Document) -> Dictionary {
        let font = document.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        });
        dictionary! { "Font" => dictionary! { "F1" => font } }
    }

    fn too_large(result: Result<Vec<String>, DocumentsError>) -> String {
        match result {
            Err(DocumentsError::TooLarge(message)) => message,
            Err(other) => panic!("expected TooLarge, got {other:?}"),
            Ok(texts) => panic!(
                "expected TooLarge, got the text of {} pages, {} bytes",
                texts.len(),
                texts.iter().map(String::len).sum::<usize>()
            ),
        }
    }

    /// `count` marked-content sequences, each with the actual text
    /// `actual` (from the properties `/P0`) over one glyph.
    fn actual_text_page(count: usize, actual: usize) -> (Document, usize) {
        let content: Vec<u8> = (0..count)
            .flat_map(|i| {
                format!(
                    "/Span /P0 BDC BT /F1 12 Tf 1 0 0 1 {} 700 Tm (a) Tj ET EMC\n",
                    i * 20
                )
                .into_bytes()
            })
            .collect();
        // What the walk reads: the stream and the newline after it.
        let length = content.len() + 1;
        let document = pages_with(
            |document| {
                let mut resources = helvetica(document);
                let properties = document.add_object(dictionary! {
                    "ActualText" => Object::String(vec![b'x'; actual], StringFormat::Literal),
                });
                resources.set("Properties", dictionary! { "P0" => properties });
                resources
            },
            &[content],
        );
        (document, length)
    }

    #[test]
    fn actual_text_counts_toward_the_page_work() {
        // 200 sequences of 10,000 bytes of actual text each: 2 MB of text
        // from 12 KB of content and one 10 KB string.
        let (document, length) = actual_text_page(200, 10_000);
        let tight = Limits {
            page_work: length + 10_000,
            ..Limits::default()
        };
        too_large(page_texts_within(&document, tight));
        // A few sequences fit.
        let (few, length) = actual_text_page(2, 10_000);
        let fits = Limits {
            page_work: length + 4 * 10_000,
            ..Limits::default()
        };
        let texts = page_texts_within(&few, fits).unwrap();
        assert_eq!(texts[0].len(), 2 * 10_000 + 1, "{:?}", &texts[0][..20]);
    }

    #[test]
    fn a_page_may_produce_only_so_much_text() {
        let (document, _) = actual_text_page(200, 10_000);
        let tight = Limits {
            page_text: 1024 * 1024,
            ..Limits::default()
        };
        let message = too_large(page_texts_within(&document, tight));
        assert!(message.contains("1 MiB"), "{message}");
        let roomy = Limits {
            page_text: 4 * 1024 * 1024,
            ..Limits::default()
        };
        assert_eq!(
            page_texts_within(&document, roomy).unwrap()[0].len(),
            200 * 10_001 - 1
        );
    }

    #[test]
    fn a_document_may_produce_only_so_much_text() {
        let text = format!(
            "BT /F1 12 Tf 1 0 0 1 10 700 Tm ({}) Tj ET",
            "a".repeat(3000)
        );
        let document = pages_with(helvetica, &vec![text.into_bytes(); 4]);
        let tight = Limits {
            document_text: 10_000,
            ..Limits::default()
        };
        let message = too_large(page_texts_within(&document, tight));
        assert!(message.contains("10000 bytes"), "{message}");
        let fits = Limits {
            document_text: 12_000,
            ..Limits::default()
        };
        assert_eq!(page_texts_within(&document, fits).unwrap().len(), 4);
    }

    #[test]
    fn marked_content_nested_deeper_than_the_limit_is_refused() {
        let nested = |depth: usize| {
            let content = format!("{}BT /F1 12 Tf (a) Tj ET", "/P BMC ".repeat(depth));
            pages_with(helvetica, &[content.into_bytes()])
        };
        too_large(page_texts_within(&nested(257), Limits::default()));
        assert_eq!(
            page_texts_within(&nested(256), Limits::default()).unwrap(),
            ["a"]
        );
    }

    #[test]
    fn saved_states_nested_deeper_than_the_limit_are_refused() {
        let nested = |depth: usize| {
            let content = format!(
                "{}BT /F1 12 Tf (a) Tj ET {}",
                "q ".repeat(depth),
                "Q ".repeat(depth)
            );
            pages_with(helvetica, &[content.into_bytes()])
        };
        too_large(page_texts_within(&nested(257), Limits::default()));
        assert_eq!(
            page_texts_within(&nested(256), Limits::default()).unwrap(),
            ["a"]
        );
    }

    #[test]
    fn comparing_marks_with_long_text_spends_the_budget() {
        // A glyph whose text is 100,000 bytes, and 1,000 marks on it whose
        // text it holds only at its end: each comparison reads all of it.
        let mut chunks = vec![Chunk {
            x0: 0.0,
            x1: 1000.0,
            y: 700.0,
            angle: 0,
            size: 12.0,
            text: format!("{}b", "a".repeat(100_000)),
        }];
        chunks.extend((0..1000).map(|i| Chunk {
            x0: i as f32 * 0.5,
            x1: i as f32 * 0.5,
            y: 700.0,
            angle: 0,
            size: 12.0,
            text: "b".into(),
        }));
        let error = assemble(chunks.clone(), 1_000_000).unwrap_err();
        assert!(matches!(error, DocumentsError::TooLarge(_)), "{error:?}");
        assert_eq!(assemble(chunks, 200_000_000).unwrap().len(), 100_001);
    }

    /// A ToUnicode map of `codes` single-code mappings, from `<0000>`.
    fn to_unicode_map(codes: usize) -> String {
        let mut cmap = String::from(
            "/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n\
             /CMapName /Sample def\n\
             1 begincodespacerange <0000> <FFFF> endcodespacerange\n",
        );
        for block in 0..codes.div_ceil(100) {
            let count = (codes - block * 100).min(100);
            cmap.push_str(&format!("{count} beginbfchar\n"));
            for j in 0..count {
                let code = block * 100 + j;
                cmap.push_str(&format!("<{:04X}> <{:04X}>\n", code, 0x4E00 + code));
            }
            cmap.push_str("endbfchar\n");
        }
        cmap.push_str("endcmap CMapName currentdict /CMap defineresource pop end end\n");
        cmap
    }

    /// A font with `cmap` as its compressed ToUnicode map, under `names`
    /// font names.
    fn fonts_sharing_a_map(cmap: &str, names: usize) -> impl FnOnce(&mut Document) -> Dictionary {
        let cmap = cmap.to_owned();
        move |document| {
            let mut stream = Stream::new(Dictionary::new(), cmap.into_bytes());
            stream.compress().unwrap();
            let map = document.add_object(stream);
            let font = document.add_object(dictionary! {
                "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
                "ToUnicode" => map,
            });
            let mut fonts = Dictionary::new();
            for i in 0..names {
                fonts.set(format!("F{i}"), font);
            }
            dictionary! { "Font" => fonts }
        }
    }

    #[test]
    fn a_to_unicode_map_counts_toward_the_work_once_per_font() {
        let cmap = to_unicode_map(8000);
        let content: Vec<u8> = (0..100)
            .flat_map(|i| format!("BT /F{i} 12 Tf (a) Tj ET\n").into_bytes())
            .collect();
        let document = pages_with(
            fonts_sharing_a_map(&cmap, 100),
            std::slice::from_ref(&content),
        );
        // Less than the map itself: refused.
        let tight = Limits {
            page_work: content.len() + cmap.len() / 2,
            ..Limits::default()
        };
        too_large(page_texts_within(&document, tight));
        // Room for reading it once, not a hundred times: one font, read once.
        let once = Limits {
            page_work: content.len() + 1 + 50 * cmap.len(),
            ..Limits::default()
        };
        page_texts_within(&document, once).unwrap();
    }

    #[test]
    fn a_font_used_on_every_page_is_read_once_for_the_document() {
        let cmap = to_unicode_map(8000);
        let content = b"BT /F0 12 Tf (a) Tj ET".to_vec();
        let document = pages_with(fonts_sharing_a_map(&cmap, 1), &vec![content.clone(); 20]);
        let once = Limits {
            document_work: 20 * (content.len() + 1) + 50 * cmap.len(),
            ..Limits::default()
        };
        assert_eq!(page_texts_within(&document, once).unwrap().len(), 20);
    }

    /// One page showing a glyph of a composite font with `ToUnicode` as its
    /// map and `widths` as its descendant's `W`.
    fn composite_font_page(to_unicode: &str, widths: Vec<Object>) -> Document {
        let to_unicode = to_unicode.to_owned();
        pages_with(
            move |document| {
                let mut stream = Stream::new(Dictionary::new(), to_unicode.into_bytes());
                stream.compress().unwrap();
                let map = document.add_object(stream);
                let descendant = document.add_object(dictionary! {
                    "Type" => "Font", "Subtype" => "CIDFontType2", "BaseFont" => "Sample",
                    "W" => widths,
                });
                let font = document.add_object(dictionary! {
                    "Type" => "Font", "Subtype" => "Type0", "BaseFont" => "Sample",
                    "Encoding" => "Identity-H",
                    "DescendantFonts" => vec![descendant.into()],
                    "ToUnicode" => map,
                });
                dictionary! { "Font" => dictionary! { "F1" => font } }
            },
            &[b"BT /F1 12 Tf 1 0 0 1 10 700 Tm <0001> Tj ET".to_vec()],
        )
    }

    #[test]
    fn a_to_unicode_map_over_millions_of_codes_is_refused() {
        // One line of a range over 2,097,152 three-byte codes: reading the
        // map visits each of them.
        let cmap = "/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n\
                    /CMapName /Sample def\n\
                    1 begincodespacerange <000000> <FFFFFF> endcodespacerange\n\
                    1 beginbfrange\n<000000> <1FFFFF> <0041>\nendbfrange\n\
                    endcmap CMapName currentdict /CMap defineresource pop end end\n";
        too_large(page_texts_within(
            &composite_font_page(cmap, Vec::new()),
            Limits::default(),
        ));
        // The same map over a few codes is read.
        let small = cmap.replace("<1FFFFF>", "<0000FF>");
        page_texts_within(&composite_font_page(&small, Vec::new()), Limits::default()).unwrap();
    }

    #[test]
    fn character_widths_keep_only_the_codes_a_font_can_show() {
        let cmap = to_unicode_map(4);
        // The map is read: code 1 is U+4E01.
        assert_eq!(
            page_texts_within(&composite_font_page(&cmap, Vec::new()), Limits::default()).unwrap(),
            ["\u{4e01}"]
        );
        // Codes past the last one a font can have do not wrap around.
        let past_the_end = vec![
            Object::Integer(4_294_967_295),
            vec![500.into(), 500.into()].into(),
        ];
        page_texts_within(&composite_font_page(&cmap, past_the_end), Limits::default()).unwrap();
        // 40 ranges of 65,536 codes each, all past the codes a composite
        // font can show: nothing to keep.
        let far: Vec<Object> = (1..=40i64)
            .flat_map(|k| [k * 65_536, k * 65_536 + 65_535, 500])
            .map(Object::Integer)
            .collect();
        let tight = Limits {
            page_work: 1024 * 1024,
            ..Limits::default()
        };
        page_texts_within(&composite_font_page(&cmap, far), tight).unwrap();
        // 400 ranges over every code, each set again: refused once the
        // work runs out.
        let again: Vec<Object> = (0..400)
            .flat_map(|_| [0, 65_535, 500])
            .map(Object::Integer)
            .collect();
        too_large(page_texts_within(&composite_font_page(&cmap, again), tight));
        // A simple font's first code past the end does not wrap either.
        let document = pages_with(
            |document| {
                let font = document.add_object(dictionary! {
                    "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
                    "FirstChar" => 4_294_967_295i64,
                    "Widths" => vec![500.into(), 500.into()],
                });
                dictionary! { "Font" => dictionary! { "F1" => font } }
            },
            &[b"BT /F1 12 Tf (a) Tj ET".to_vec()],
        );
        page_texts_within(&document, Limits::default()).unwrap();
    }

    #[test]
    fn the_message_names_the_limit_that_ran_out() {
        let document = pages_with(helvetica, &[b"BT /F1 12 Tf (a) Tj ET".to_vec()]);
        let page = Limits {
            page_work: 10,
            ..Limits::default()
        };
        let message = too_large(page_texts_within(&document, page));
        assert!(
            message.contains("10 bytes") && message.contains("page"),
            "{message}"
        );
        assert!(!message.contains("0 MiB"), "{message}");
        let whole = Limits {
            document_work: 10,
            ..Limits::default()
        };
        let message = too_large(page_texts_within(&document, whole));
        assert!(
            message.contains("10 bytes") && message.contains("document"),
            "{message}"
        );
        assert!(!message.contains("128 MiB"), "{message}");
    }

    #[test]
    fn the_marks_message_reads_as_one_sentence() {
        let message = assemble(loose_marks(200), 1_000).unwrap_err().to_string();
        assert!(!message.contains("  "), "{message:?}");
    }

    const MAP_HEAD: &str = "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
                            /CMapName /Sample def\n\
                            1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n";
    const MAP_TAIL: &str = "endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n";

    /// The map of every two-byte code to itself that some writers emit:
    /// 256 lines of `<XX00> <XXFF> <XX00>`.
    fn identity_map() -> String {
        let mut cmap = String::from(MAP_HEAD);
        for block in 0..2 {
            cmap.push_str("128 beginbfrange\n");
            for high in block * 128..(block + 1) * 128 {
                cmap.push_str(&format!("<{high:02X}00> <{high:02X}FF> <{high:02X}00>\n"));
            }
            cmap.push_str("endbfrange\n");
        }
        cmap.push_str(MAP_TAIL);
        cmap
    }

    /// A range over every two-byte code whose text is a list of `texts`
    /// strings, then `codes` single codes inside it, each splitting it.
    fn split_list_map(texts: usize, codes: usize) -> String {
        let mut cmap = String::from(MAP_HEAD);
        cmap.push_str("1 beginbfrange\n<0000> <FFFF> [");
        cmap.push_str(&"<0041> ".repeat(texts));
        cmap.push_str("]\nendbfrange\n");
        cmap.push_str(&format!("{codes} beginbfchar\n"));
        for i in 0..codes {
            cmap.push_str(&format!("<{:04X}> <0042>\n", 2 * i + 1));
        }
        cmap.push_str("endbfchar\n");
        cmap.push_str(MAP_TAIL);
        cmap
    }

    /// `cmap` as a compressed stream.
    fn map_stream(document: &mut Document, cmap: &str) -> ObjectId {
        let mut stream = Stream::new(Dictionary::new(), cmap.as_bytes().to_vec());
        stream.compress().unwrap();
        document.add_object(stream)
    }

    /// A composite font whose ToUnicode map is `map`.
    fn composite_font(document: &mut Document, map: ObjectId) -> ObjectId {
        document.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type0", "BaseFont" => "Sample",
            "Encoding" => "Identity-H",
            "ToUnicode" => map,
        })
    }

    /// One page per entry of `contents`, each with its own resources from
    /// `resources`.
    fn pages_each_with(
        resources: impl FnOnce(&mut Document) -> Vec<Dictionary>,
        contents: &[Vec<u8>],
    ) -> Document {
        let mut document = pages_with(|_| Dictionary::new(), contents);
        let resources = resources(&mut document);
        for (page_id, resources) in document.get_pages().into_values().zip(resources) {
            document
                .get_object_mut(page_id)
                .and_then(Object::as_dict_mut)
                .unwrap()
                .set("Resources", resources);
        }
        document
    }

    /// Showing code `0041` in fonts `/F0` to `/F{fonts - 1}`, a line each.
    fn showing_each_font(fonts: usize) -> Vec<u8> {
        (0..fonts)
            .flat_map(|i| {
                format!(
                    "BT /F{i} 12 Tf 1 0 0 1 10 {} Tm <0041> Tj ET\n",
                    780 - i * 14
                )
                .into_bytes()
            })
            .collect()
    }

    #[test]
    fn a_to_unicode_range_split_by_many_codes_is_refused() {
        // A range whose text is a list of 1,000 strings, split by 1,000
        // single codes: each piece keeps a copy of the whole list, about
        // 60 MB from 21 KB of map.
        let page = |cmap: String| {
            pages_with(
                move |document| {
                    let map = map_stream(document, &cmap);
                    let font = composite_font(document, map);
                    dictionary! { "Font" => dictionary! { "F1" => font } }
                },
                &[b"BT /F1 12 Tf 1 0 0 1 10 700 Tm <0001> Tj ET".to_vec()],
            )
        };
        let tight = Limits {
            font_memory: 32 * 1024 * 1024,
            ..Limits::default()
        };
        let message = too_large(page_texts_within(&page(split_list_map(1000, 1000)), tight));
        assert!(message.contains("32 MiB"), "{message}");
        // A few splits are read as before: code 1 is one of them.
        assert_eq!(
            page_texts_within(&page(split_list_map(10, 10)), tight).unwrap(),
            ["B"]
        );
    }

    #[test]
    fn a_page_of_composite_fonts_with_full_identity_maps_reads() {
        // Sixteen fonts, each with its own map of all 65,536 codes.
        let fonts = 16;
        let document = pages_each_with(
            |document| {
                let mut names = Dictionary::new();
                for i in 0..fonts {
                    let map = map_stream(document, &identity_map());
                    names.set(format!("F{i}"), composite_font(document, map));
                }
                vec![dictionary! { "Font" => names }]
            },
            &[showing_each_font(fonts)],
        );
        let texts = page_texts_within(&document, Limits::default()).unwrap();
        assert_eq!(texts, [vec!["A"; fonts].join("\n")]);
    }

    #[test]
    fn composite_fonts_with_full_identity_maps_read_across_a_document() {
        // Twenty-four pages, each with a font of its own, then a page using
        // all of them. Memory for fonts holds three maps: the last page
        // reads most of them again, and that costs it little work.
        let fonts = 24;
        let mut contents: Vec<Vec<u8>> = (0..fonts).map(|_| showing_each_font(1)).collect();
        contents.push(showing_each_font(fonts));
        let document = pages_each_with(
            |document| {
                let ids: Vec<ObjectId> = (0..fonts)
                    .map(|_| {
                        let map = map_stream(document, &identity_map());
                        composite_font(document, map)
                    })
                    .collect();
                let mut all = Dictionary::new();
                for (i, id) in ids.iter().enumerate() {
                    all.set(format!("F{i}"), *id);
                }
                let mut resources: Vec<Dictionary> = ids
                    .iter()
                    .map(|id| dictionary! { "Font" => dictionary! { "F0" => *id } })
                    .collect();
                resources.push(dictionary! { "Font" => all });
                resources
            },
            &contents,
        );
        let limits = Limits {
            document_work: 64 * 1024 * 1024,
            font_memory: 40 * 1024 * 1024,
            ..Limits::default()
        };
        let texts = page_texts_within(&document, limits).unwrap();
        assert_eq!(texts.len(), fonts + 1);
        assert!(texts[..fonts].iter().all(|text| text == "A"), "{texts:?}");
        assert_eq!(texts[fonts], vec!["A"; fonts].join("\n"));
    }

    #[test]
    fn fonts_sharing_one_map_read_it_once() {
        // Thirty-two fonts, each its own dictionary, all with one map of
        // 16,384 codes.
        let fonts = 32;
        let cmap =
            format!("{MAP_HEAD}1 beginbfrange\n<0000> <3FFF> <4E00>\nendbfrange\n{MAP_TAIL}");
        let document = pages_each_with(
            |document| {
                let map = map_stream(document, &cmap);
                let mut names = Dictionary::new();
                for i in 0..fonts {
                    names.set(format!("F{i}"), composite_font(document, map));
                }
                vec![dictionary! { "Font" => names }]
            },
            &[showing_each_font(fonts)],
        );
        // Room for reading the map once.
        let once = Limits {
            page_work: 1024 * 1024,
            ..Limits::default()
        };
        let texts = page_texts_within(&document, once).unwrap();
        assert_eq!(texts, [vec!["\u{4e41}"; fonts].join("\n")]);
    }
}
