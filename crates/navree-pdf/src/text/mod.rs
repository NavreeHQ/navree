//! Text extraction for navree-documents' `Pdf::text`: what each page
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

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use crate::budget::LOOKUP_COST;
use crate::document::{ContentError, Document};
use crate::error::{PdfError, size};
use crate::object::{Dictionary, Object, ObjectId, Stream};

use self::cmap::ToUnicode;
use self::encoding::Encoding;

mod cmap;
mod encoding;
mod tables;

pub use self::encoding::decode_text_string as strict_text_string;

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
/// How many comparisons attaching marks cost one byte of work (16), so that
/// many pages of them are held to the document's work too.
const STEPS_PER_WORK: usize = 16;
/// What each character width a font sets costs, in bytes of work (16).
const WIDTH_COST: usize = 16;
/// What each code a table of widths has room for holds in memory (4 bytes).
const WIDTH_MEMORY: usize = 4;
/// What a font holds besides its widths and maps (256 bytes).
const FONT_INFO_MEMORY: usize = 256;
/// What reading a font costs in work besides its widths, maps and encoding, the
/// first time or again after it was dropped to make room (1 KiB, as drawing a
/// form does).
const FONT_READ_COST: usize = 1024;
/// What each code an encoding's `Differences` names is counted as holding (160
/// bytes: an entry of a hash map, and the text of a glyph name, 93 bytes at
/// most).
const DIFFERENCE_MEMORY: usize = 160;
/// The codes `Differences` can name: however long the array, it builds no more.
const DIFFERENCE_CODES: usize = 256;
/// What every operation of a content stream costs besides its bytes, in bytes
/// of work (4), whatever the operator: it is charged before the operator is
/// looked at.
const OPERATION_COST: usize = 4;
/// What each operation that shows text costs besides that (16): finding the
/// font, and marking it used.
const SHOW_COST: usize = 16;
/// What each `Do` costs besides that (32), before it is known to be a form;
/// the references it follows cost what each reference costs.
const DO_COST: usize = 32;
/// What each character code shown costs (16), whether it shows anything or not.
const CODE_COST: usize = 16;
/// What each line a page's glyphs form costs when they are put together (32).
const LINE_COST: usize = 32;
/// What the operands of one operation may hold before they cost anything
/// themselves (eight of them: real operations have few). Past that, a byte of
/// work for each [`OPERANDS_MEMORY_PER_WORK`].
const OPERANDS_FREE: usize = 8 * std::mem::size_of::<Object>();
/// The memory of operands that costs a byte of work (32).
const OPERANDS_MEMORY_PER_WORK: usize = 32;
/// What each page costs (256), even with nothing on it.
const PAGE_COST: usize = 256;
/// The font names a page remembers the font of (4,096); past that it starts
/// again, and looks each name up again when it is used.
const MAX_FONT_NAMES: usize = 4096;
/// The longest name of a font in a content stream (127 bytes, the limit the
/// PDF reference gives on a name); one longer names no font.
const MAX_FONT_NAME: usize = 127;
/// What a map may build while it is read, for each of its bytes, at most.
const MAP_READ_MEMORY: usize = 64;

/// The highest code a composite font can show: codes have two bytes.
const MAX_COMPOSITE_CODE: u32 = 0xFFFF;
/// The highest code a simple font can show: codes have one byte.
const MAX_SIMPLE_CODE: u64 = 0xFF;

/// How much work reading the text of a PDF may take, so that a small file
/// cannot make a call take more than a bounded time or memory. The time is in
/// proportion to [`document_work`](Self::document_work) (see the crate's
/// documentation for the figures measured). Every figure is per call unless it
/// says otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextLimits {
    /// Bytes of work one page may take (128 MiB): its content decoded,
    /// counting a form again every time it is drawn; `PAGE_COST` for the
    /// page, `OPERATION_COST` for each operation of whatever kind, and more
    /// for those that do more: `SHOW_COST` for each that shows text,
    /// `CODE_COST` for each character code shown, `DO_COST` for each `Do`
    /// and `FORM_DRAW_COST` when it draws a form; 32 bytes for each object
    /// looked up on the way, through however many references; every byte a
    /// filter reads and writes, and 1 KiB for each filter, whether the content
    /// could be used or not; the fonts it reads (`FONT_READ_COST`, a
    /// `ToUnicode` map's bytes, inflating them counted the same way, and one
    /// byte for every `MEMORY_PER_WORK` of the memory reading it builds; for
    /// an encoding's `Differences` 8 bytes for each item and 8 for each byte of
    /// a glyph name; `WIDTH_COST` for each width looked at); the actual text
    /// of marked content each time it is decoded; `LINE_COST` for each line
    /// the glyphs form, and one byte for every `STEPS_PER_WORK` comparisons
    /// attaching marks.
    pub page_work: usize,
    /// The same, over all the pages (1 GiB).
    pub document_work: usize,
    /// Character codes one page may show (1,000,000), including those that
    /// read as nothing.
    pub page_glyphs: usize,
    /// Comparisons one page may spend attaching marks to the glyphs they
    /// sit on, counting the bytes of text compared (50,000,000).
    pub page_steps: usize,
    /// Bytes of text one page may produce, before lines are put together
    /// (8 MiB). A dense page of text is tens of kilobytes; a million glyphs
    /// of Latin text is about 1 MB, of Chinese about 3 MB.
    pub page_text: usize,
    /// The same, over all the pages (64 MiB): some fifteen thousand pages of
    /// 4,000 characters, though the work of a document (1 GiB) runs out first,
    /// at about nine thousand pages of that kind (a book of 3,211 pages used 35%
    /// of it). The text is returned whole, so this is memory as well.
    pub document_text: usize,
    /// Memory the fonts and `ToUnicode` maps read for a document may hold
    /// at once (256 MiB), as they are counted: what reading a map builds (a
    /// range of codes is one entry however many it covers, and the pieces a
    /// later line splits it in share its text), 160 bytes for each code an
    /// encoding's `Differences` names, and 4 bytes for each code a table of
    /// widths has room for. Past the limit, what was used longest ago is dropped
    /// and read again when a page needs it; what the run of text being
    /// shown uses is not, so the total can pass the limit by one font's
    /// widths. One map or font that would take more on its own is refused,
    /// as is a map over one of the caps (`cmap::MAX_CODESPACE_RANGES` and
    /// the rest), as soon as the reader gets there.
    pub font_memory: usize,
}

impl Default for TextLimits {
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

/// The text of every page, in page order, within the default
/// [`TextLimits`].
pub fn page_texts(document: &Document) -> Result<Vec<String>, PdfError> {
    page_texts_within(document, TextLimits::default())
}

/// [`page_texts`] under `limits`. An error when one of them runs out, or
/// one of the document's [`ReadLimits`](crate::ReadLimits) does.
pub fn page_texts_within(document: &Document, limits: TextLimits) -> Result<Vec<String>, PdfError> {
    page_texts_counted(document, limits).map(|(texts, _)| texts)
}

/// [`page_texts_within`], with the work it charged: what the time it took is in
/// proportion to.
pub fn page_texts_counted(
    document: &Document,
    limits: TextLimits,
) -> Result<(Vec<String>, usize), PdfError> {
    let mut texts = Vec::new();
    let (mut work, mut text) = (0usize, 0usize);
    // Fonts and their maps are kept for the whole document, within
    // `limits.font_memory`.
    let mut fonts = FontCache::default();
    for page_id in document.pages() {
        let mut walker = Walker {
            document,
            chunks: Vec::new(),
            fonts,
            font_names: HashMap::new(),
            budget: Budget::new(document, limits, work, text),
        };
        walker.budget.charge(PAGE_COST)?;
        let content = page_content(document, page_id, &mut walker.budget)?;
        let resources = page_resources(document, page_id);
        walker.walk(&content, resources, IDENTITY, 0)?;
        let chunks = std::mem::take(&mut walker.chunks);
        let (page_text, steps, lines) = assemble_counted(chunks, limits.page_steps)?;
        walker
            .budget
            .charge((steps / STEPS_PER_WORK).saturating_add(lines.saturating_mul(LINE_COST)))?;
        work += walker.budget.work;
        text += walker.budget.text;
        fonts = walker.fonts;
        texts.push(page_text);
        document.check()?;
    }
    document.check()?;
    Ok((texts, work))
}

/// What a page may still spend: work and text, each held to the page's
/// limit and to what is left of the document's. Every charge also counts the
/// objects the document looked up since the last one, and stops when the
/// document has run out of a limit of its own: what follows a reference pays
/// for it without saying so.
struct Budget<'d> {
    document: &'d Document,
    /// The objects the document had looked up at the last charge.
    looked_up: usize,
    limits: TextLimits,
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
    /// The character codes this page showed so far.
    codes: usize,
    /// The actual text that the marked content now open holds, over all the
    /// forms the page draws.
    marked: usize,
}

impl<'d> Budget<'d> {
    /// A page's budget, after the pages before it did `work` and produced
    /// `text`.
    fn new(document: &'d Document, limits: TextLimits, work: usize, text: usize) -> Self {
        let work_left = limits.document_work.saturating_sub(work);
        let text_left = limits.document_text.saturating_sub(text);
        Self {
            document,
            looked_up: document.lookups(),
            limits,
            work: 0,
            work_allowed: limits.page_work.min(work_left),
            work_by_document: work_left < limits.page_work,
            text: 0,
            text_allowed: limits.page_text.min(text_left),
            text_by_document: text_left < limits.page_text,
            codes: 0,
            marked: 0,
        }
    }

    /// Counts `amount` bytes of actual text held by a mark that opens, or
    /// fails when the marks open hold more than a page may have of text.
    fn open_mark(&mut self, amount: usize) -> Result<(), PdfError> {
        self.marked = self.marked.saturating_add(amount);
        if self.marked > self.limits.page_text {
            return Err(PdfError::TooLarge(format!(
                "a PDF page holds more actual text in the marked content it has open than the \
                 limit of {} for the text of a page",
                size(self.limits.page_text)
            )));
        }
        Ok(())
    }

    /// Lets go of the `amount` bytes of actual text a mark that closes held.
    fn close_mark(&mut self, amount: usize) {
        self.marked = self.marked.saturating_sub(amount);
    }

    /// The work the page may still do.
    fn work_left(&self) -> usize {
        self.work_allowed.saturating_sub(self.work)
    }

    /// Counts `amount` bytes of work and the objects looked up since the last
    /// charge, or fails when the page (or the document) has used all it may.
    fn charge(&mut self, amount: usize) -> Result<(), PdfError> {
        let lookups = self.document.lookups();
        let looked_up = lookups.saturating_sub(self.looked_up);
        self.looked_up = lookups;
        self.work = self
            .work
            .saturating_add(amount)
            .saturating_add(looked_up.saturating_mul(LOOKUP_COST));
        if self.work > self.work_allowed {
            return Err(self.out_of_work());
        }
        self.document.check()
    }

    /// Counts one character code shown, whatever it shows.
    fn code(&mut self) -> Result<(), PdfError> {
        self.codes += 1;
        if self.codes > self.limits.page_glyphs {
            return Err(too_many_glyphs(self.limits.page_glyphs));
        }
        self.charge(CODE_COST)
    }

    fn out_of_work(&self) -> PdfError {
        let counted = "counting the content each time a form is drawn, the fonts' ToUnicode \
                       maps and widths, and the actual text of marked content";
        PdfError::TooLarge(if self.work_by_document {
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
    fn produce(&mut self, amount: usize) -> Result<(), PdfError> {
        self.text = self.text.saturating_add(amount);
        if self.text > self.text_allowed {
            return Err(PdfError::TooLarge(if self.text_by_document {
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

/// Decodes `stream` within the work the page has left past `held`, what it
/// will still be charged for (and within `room`), so that nothing is inflated
/// that the page could not pay for. What every filter reads and writes is
/// charged, whether the result can be used or not, but for the result itself:
/// the caller walks it, and that charges it. Going over is an error; other
/// errors are returned inside, for the caller to decide.
fn decode_charged<'a>(
    document: &'a Document,
    stream: &'a Stream,
    budget: &mut Budget<'_>,
    room: usize,
    held: usize,
) -> Result<Result<Vec<u8>, ContentError>, PdfError> {
    let work = budget.work_left().saturating_sub(held);
    let limit = work.min(room);
    let mut spent = 0;
    let decoded = document.decode_counted(stream, limit, &mut spent);
    let result = decoded.as_ref().map_or(0, Vec::len);
    budget.charge(spent.saturating_sub(result))?;
    match decoded {
        Err(ContentError::Over) => Err(if work < room {
            budget.out_of_work()
        } else {
            PdfError::TooLarge(format!(
                "a PDF content stream inflates to more than {}",
                size(MAX_CONTENT)
            ))
        }),
        other => Ok(other),
    }
}

/// A page's content: its streams decoded and joined, each followed by a new
/// line, within [`MAX_CONTENT`] and the work the page has left. A stream that
/// does not decode is taken as it is stored, if that fits.
fn page_content(
    document: &Document,
    page: ObjectId,
    budget: &mut Budget<'_>,
) -> Result<Vec<u8>, PdfError> {
    let mut content = Vec::new();
    for id in document.page_contents(page) {
        let Some(stream) = document.object(id).and_then(Object::as_stream) else {
            continue;
        };
        let room = MAX_CONTENT.saturating_sub(content.len());
        match decode_charged(document, stream, budget, room, content.len())? {
            Ok(data) => content.extend_from_slice(&data),
            Err(_) if stream.content.len() <= room.min(budget.work_left()) => {
                content.extend_from_slice(&stream.content)
            }
            Err(_) => {
                return Err(if budget.work_left() < room {
                    budget.out_of_work()
                } else {
                    PdfError::TooLarge(format!(
                        "a PDF content stream inflates to more than {}",
                        size(MAX_CONTENT)
                    ))
                });
            }
        }
        content.push(b'\n');
    }
    Ok(content)
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
    /// Its `ToUnicode` map, which comes first: the stream, read once for every
    /// font that has it.
    map: Option<&'a Stream>,
    /// What its codes read as where the map does not say, or when it has no
    /// map or one that cannot be read.
    encoding: Option<Encoding<'a>>,
    /// Whether a code the map leaves out reads through `encoding`: when the
    /// font names an encoding, and not an identity one, whose codes mean
    /// nothing else.
    reads_past_map: bool,
    /// Bytes per character code: 2 for composite fonts, 1 otherwise.
    code_len: usize,
    widths: Widths,
    default_width: f32,
    /// A standard font without a `Widths` array.
    standard: bool,
}

/// The widths a font sets, by code: a table as long as the highest code set,
/// at most 65,536 entries, since codes have one or two bytes.
#[derive(Default)]
struct Widths(Vec<f32>);

impl Widths {
    fn insert(&mut self, code: u32, width: f32) {
        let at = code as usize;
        if at >= self.0.len() {
            self.0.resize(at + 1, f32::NAN);
        }
        if let Some(slot) = self.0.get_mut(at) {
            *slot = if width.is_nan() { 0.0 } else { width };
        }
    }

    fn get(&self, code: u32) -> Option<f32> {
        self.0.get(code as usize).copied().filter(|w| !w.is_nan())
    }

    /// What the table holds.
    fn memory(&self) -> usize {
        self.0.len() * WIDTH_MEMORY
    }
}

/// Saved with `q`, and restored with `Q`: the name of the font is shared, not
/// copied.
#[derive(Clone)]
struct TextState {
    font: Option<Rc<[u8]>>,
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

/// What the cache holds, with the memory it takes and when it was last used.
struct Held<T> {
    value: T,
    memory: usize,
    used: u64,
}

/// An entry of the cache: a font or a map, by its address.
#[derive(Clone, Copy)]
enum Key {
    Font(usize),
    Map(usize),
}

/// The fonts and `ToUnicode` maps read so far in a document, by the address
/// of their dictionary or stream (which stays put while the document is
/// read), within [`Limits::font_memory`].
#[derive(Default)]
struct FontCache<'a> {
    fonts: HashMap<usize, Held<FontInfo<'a>>>,
    maps: HashMap<usize, Held<Option<ToUnicode>>>,
    /// What is held, by when it was last used: the first was used longest
    /// ago, and is dropped first.
    order: BTreeMap<u64, Key>,
    /// The memory all of it holds.
    memory: usize,
    /// Counts uses: each use has a number of its own.
    clock: u64,
    /// The first use of the run of text being shown: what the run uses is
    /// not dropped.
    run: u64,
}

fn address<T>(value: &T) -> usize {
    value as *const T as usize
}

impl<'a> FontCache<'a> {
    /// Starts a run of text: what it reads from now on is kept while it is
    /// shown.
    fn start_run(&mut self) {
        self.run = self.clock + 1;
    }

    /// The number of a use now.
    fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    /// Makes sure `font`, and the map it decodes with, are read, reading
    /// what is not (again, if it was dropped), and marks them used now.
    /// What reading builds is counted, and room made for it, before it reads.
    fn read(
        &mut self,
        document: &'a Document,
        font: &'a Dictionary,
        budget: &mut Budget<'_>,
    ) -> Result<(), PdfError> {
        let limit = budget.limits.font_memory;
        let key = address(font);
        let now = self.tick();
        if let Some(held) = self.fonts.get_mut(&key) {
            self.order.remove(&held.used);
            held.used = now;
        } else {
            budget.charge(FONT_READ_COST)?;
            let before = font_before(document, font);
            self.make_room(before, limit)?;
            let (value, widths) = font_info(document, font, budget)?;
            let memory = before.saturating_add(widths);
            self.memory += memory;
            self.fonts.insert(
                key,
                Held {
                    value,
                    memory,
                    used: now,
                },
            );
        }
        self.order.insert(now, Key::Font(key));
        let Some(stream) = self.fonts[&key].value.map else {
            return Ok(());
        };
        let map_key = address(stream);
        let now = self.tick();
        if let Some(held) = self.maps.get_mut(&map_key) {
            self.order.remove(&held.used);
            held.used = now;
        } else {
            let (value, memory) = self.read_map(document, stream, budget)?;
            self.memory += memory;
            self.maps.insert(
                map_key,
                Held {
                    value,
                    memory,
                    used: now,
                },
            );
        }
        self.order.insert(now, Key::Map(map_key));
        Ok(())
    }

    /// Reads the `ToUnicode` map in `stream`, with room made first for what
    /// reading it can build and then for what it is counted as holding, which
    /// is returned with it. A map that cannot be inflated builds nothing: the
    /// font reads through its encoding.
    fn read_map(
        &mut self,
        document: &'a Document,
        stream: &'a Stream,
        budget: &mut Budget<'_>,
    ) -> Result<(Option<ToUnicode>, usize), PdfError> {
        let limit = budget.limits.font_memory;
        let room = budget.work_left();
        let mut spent = 0;
        let decoded = document.decode_counted(stream, MAX_TO_UNICODE.min(room), &mut spent);
        budget.charge(spent)?;
        let map = match decoded {
            Ok(map) => map,
            Err(ContentError::Over) if room < MAX_TO_UNICODE => return Err(budget.out_of_work()),
            Err(ContentError::Over) => return Err(to_unicode_too_large()),
            Err(_) => return Ok((None, 0)),
        };
        budget.charge(map.len())?;
        self.make_room(map.len().saturating_mul(MAP_READ_MEMORY).min(limit), limit)?;
        let (read, memory) = ToUnicode::read_within(&map, limit)?;
        let memory = usize::try_from(memory).unwrap_or(usize::MAX);
        if memory > limit {
            return Err(font_too_large(limit));
        }
        budget.charge(memory / MEMORY_PER_WORK)?;
        self.make_room(memory, limit)?;
        Ok((Some(read), memory))
    }

    /// Drops what was used longest ago, but not what the run of text being
    /// shown uses, until `memory` more fits under `limit`; fails when it
    /// cannot fit at all.
    fn make_room(&mut self, memory: usize, limit: usize) -> Result<(), PdfError> {
        if memory > limit {
            return Err(font_too_large(limit));
        }
        while self.memory.saturating_add(memory) > limit {
            let Some((&used, &key)) = self.order.first_key_value() else {
                break;
            };
            if used >= self.run {
                break;
            }
            self.order.remove(&used);
            let dropped = match key {
                Key::Font(key) => self.fonts.remove(&key).map(|held| held.memory),
                Key::Map(key) => self.maps.remove(&key).map(|held| held.memory),
            };
            self.memory -= dropped.unwrap_or(0);
        }
        Ok(())
    }
}

struct Walker<'a> {
    document: &'a Document,
    chunks: Vec<Chunk>,
    fonts: FontCache<'a>,
    /// The font dictionary a font name stands for, by the address of the
    /// resources it is looked up in.
    font_names: HashMap<(Option<usize>, Vec<u8>), Option<&'a Dictionary>>,
    budget: Budget<'a>,
}

fn too_deep(what: &str) -> PdfError {
    PdfError::TooLarge(format!("a PDF page {what} more than {MAX_NESTING} deep"))
}

impl<'a> Walker<'a> {
    fn walk(
        &mut self,
        content: &[u8],
        resources: Option<&'a Dictionary>,
        base: Matrix,
        depth: usize,
    ) -> Result<(), PdfError> {
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
        let mut operations = crate::content::operations(content);
        while let Some(op) = operations.next() {
            let op = op?;
            let held = operations.operands_memory().saturating_sub(OPERANDS_FREE);
            self.budget
                .charge(OPERATION_COST + held / OPERANDS_MEMORY_PER_WORK)?;
            let operands = &op.operands;
            let number = |i: usize| operands.get(i).and_then(Object::as_float).unwrap_or(0.0);
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
                        .and_then(Object::as_name)
                        .filter(|name| name.len() <= MAX_FONT_NAME)
                        .map(Rc::from);
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
                    self.budget.charge(SHOW_COST)?;
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
                            .and_then(Object::as_array)
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
                        .and_then(|p| p.get(b"ActualText"))
                        .and_then(|o| self.document.resolve(o))
                        .and_then(Object::as_str);
                    if let Some(text) = actual_text {
                        self.budget.charge(text.len())?;
                    }
                    // The text is held as decoded, which may be longer.
                    let actual_text = actual_text.map(decode_text_string);
                    self.budget
                        .open_mark(actual_text.as_ref().map_or(0, String::len))?;
                    marks.push(Mark {
                        actual_text,
                        chunk: None,
                    });
                }
                "EMC" => {
                    if let Some(mark) = marks.pop() {
                        self.budget
                            .close_mark(mark.actual_text.as_ref().map_or(0, String::len));
                    }
                }
                "Do" if depth < MAX_FORM_DEPTH => {
                    self.budget.charge(DO_COST)?;
                    let Some(name) = operands.first().and_then(Object::as_name) else {
                        continue;
                    };
                    let Some(form) = self.form(resources, name) else {
                        continue;
                    };
                    self.budget.charge(FORM_DRAW_COST)?;
                    let document = self.document;
                    let matrix = document
                        .get(&form.dict, b"Matrix")
                        .and_then(Object::as_array)
                        .filter(|m| m.len() == 6)
                        .map(|m| {
                            let mut out = IDENTITY;
                            for (slot, value) in out.iter_mut().zip(m) {
                                *slot = document
                                    .resolve(value)
                                    .and_then(Object::as_float)
                                    .unwrap_or(0.0);
                            }
                            out
                        })
                        .unwrap_or(IDENTITY);
                    let form_resources = form
                        .dict
                        .get(b"Resources")
                        .and_then(|o| self.document.resolve(o))
                        .and_then(Object::as_dict)
                        .or(resources);
                    // What this operation holds is not held through the form it
                    // draws: that form has operations of its own.
                    drop(op);
                    let content =
                        match decode_charged(document, form, &mut self.budget, MAX_CONTENT, 0)? {
                            Ok(content) => Cow::Owned(content),
                            // Taken as it is stored; walking it charges it.
                            Err(_) => Cow::Borrowed(&form.content[..]),
                        };
                    self.walk(&content, form_resources, multiply(matrix, ctm), depth + 1)?;
                }
                _ => {}
            }
        }
        // Marks left open end with the content.
        for mark in marks {
            self.budget
                .close_mark(mark.actual_text.as_ref().map_or(0, String::len));
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
    ) -> Result<(), PdfError> {
        let Some(font_name) = state.font.as_deref() else {
            return Ok(());
        };
        let Some(font_key) = self.font(resources, resources_key, font_name)? else {
            return Ok(());
        };
        let Some(font) = self.fonts.fonts.get(&font_key).map(|held| &held.value) else {
            return Ok(());
        };
        let map = font
            .map
            .and_then(|stream| self.fonts.maps.get(&address(stream)))
            .and_then(|held| held.value.as_ref());
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
                        self.budget.code()?;
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
                        let text = font.text(map, code);
                        if text.is_empty() || text == "\u{fffd}" {
                            continue;
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
    ) -> Result<Option<usize>, PdfError> {
        let key = (resources_key, name.to_vec());
        let dictionary = match self.font_names.get(&key) {
            Some(found) => *found,
            None => {
                let found = resources.and_then(|r| self.font_dictionary(r, name));
                if self.font_names.len() >= MAX_FONT_NAMES {
                    self.font_names.clear();
                }
                self.font_names.insert(key, found);
                found
            }
        };
        let Some(dictionary) = dictionary else {
            return Ok(None);
        };
        // A new run of text: what it reads is not dropped while it is shown.
        self.fonts.start_run();
        self.fonts
            .read(self.document, dictionary, &mut self.budget)?;
        Ok(Some(address(dictionary)))
    }

    fn font_dictionary(&self, resources: &'a Dictionary, name: &[u8]) -> Option<&'a Dictionary> {
        let doc = self.document;
        resources
            .get(b"Font")
            .and_then(|o| doc.resolve(o))
            .and_then(Object::as_dict)
            .and_then(|fonts| fonts.get(name))
            .and_then(|o| doc.resolve(o))
            .and_then(Object::as_dict)
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
                .and_then(|r| r.get(b"Properties"))
                .and_then(|o| self.document.resolve(o))
                .and_then(Object::as_dict)
                .and_then(|props| props.get(name))
                .and_then(|o| self.document.resolve(o))
                .and_then(Object::as_dict),
            _ => None,
        }
    }

    fn form(&self, resources: Option<&'a Dictionary>, name: &[u8]) -> Option<&'a Stream> {
        let xobjects = resources?
            .get(b"XObject")
            .and_then(|o| self.document.resolve(o))
            .and_then(Object::as_dict)?;
        let object = self.document.resolve(xobjects.get(name)?)?;
        let stream = object.as_stream()?;
        let subtype = self.document.get(&stream.dict, b"Subtype")?.as_name()?;
        (subtype == b"Form").then_some(stream)
    }
}

fn to_unicode_too_large() -> PdfError {
    PdfError::TooLarge(format!(
        "a font's ToUnicode map inflates to more than {}",
        size(MAX_TO_UNICODE)
    ))
}

/// Reads `font` for decoding and widths, charging `budget` for the widths:
/// what it needs, and the memory its widths hold (the rest is counted by
/// [`font_before`]). Its `ToUnicode` map is left to [`FontCache::read`], so
/// that fonts sharing a map read it once.
fn font_info<'a>(
    doc: &'a Document,
    font: &'a Dictionary,
    budget: &mut Budget<'_>,
) -> Result<(FontInfo<'a>, usize), PdfError> {
    let map = font_map(doc, font);
    let encoding = encoding::font_encoding(doc, font, budget)?;
    let composite = doc
        .get(font, b"Subtype")
        .and_then(Object::as_name)
        .is_some_and(|s| s == b"Type0");
    let identity = matches!(
        encoding::encoding_name(doc, font),
        Some(b"Identity-H" | b"Identity-V")
    );
    let mut widths = Widths::default();
    let mut default_width = 0.0;
    let mut standard = false;
    if composite {
        default_width = 1000.0;
        let descendant = font
            .get(b"DescendantFonts")
            .and_then(|o| doc.resolve(o))
            .and_then(Object::as_array)
            .and_then(|a| a.first())
            .and_then(|o| doc.resolve(o))
            .and_then(Object::as_dict);
        if let Some(descendant) = descendant {
            if let Some(dw) = doc.get(descendant, b"DW").and_then(Object::as_float) {
                default_width = dw;
            }
            let w = descendant
                .get(b"W")
                .and_then(|o| doc.resolve(o))
                .and_then(Object::as_array);
            if let Some(w) = w {
                parse_cid_widths(doc, w, &mut widths, budget)?;
            }
        }
    } else {
        let first = doc
            .get(font, b"FirstChar")
            .and_then(Object::as_i64)
            .unwrap_or(0)
            .max(0) as u32;
        let list = font
            .get(b"Widths")
            .and_then(|o| doc.resolve(o))
            .and_then(Object::as_array);
        match list {
            Some(list) => {
                // Codes of one byte: the rest of the list is never used.
                for (offset, width) in list.iter().enumerate() {
                    let code = u64::from(first) + offset as u64;
                    if code > MAX_SIMPLE_CODE {
                        break;
                    }
                    let width = doc.resolve(width).and_then(Object::as_float).unwrap_or(0.0);
                    budget.charge(WIDTH_COST)?;
                    widths.insert(code as u32, width);
                }
            }
            None => standard = true,
        }
    }
    let memory = widths.memory();
    // Making the table is work in proportion to it.
    budget.charge(memory / MEMORY_PER_WORK)?;
    let info = FontInfo {
        map,
        encoding,
        reads_past_map: font.has(b"Encoding") && !identity,
        code_len: if composite { 2 } else { 1 },
        widths,
        default_width,
        standard,
    };
    Ok((info, memory))
}

impl FontInfo<'_> {
    /// The text of `code`: what `map`, its `ToUnicode` map once read, says, or
    /// else what its encoding does.
    fn text(&self, map: Option<&ToUnicode>, code: &[u8]) -> String {
        if let Some(map) = map {
            let text = map.decode(code);
            if !self.reads_past_map || !(text.is_empty() || text == "\u{fffd}") {
                return text;
            }
        }
        self.encoding
            .as_ref()
            .and_then(|encoding| encoding.decode(code))
            .unwrap_or_default()
    }

    fn width(&self, code: u32) -> f32 {
        if self.standard {
            return standard_width(code) as f32;
        }
        self.widths.get(code).unwrap_or(self.default_width)
    }
}

/// A CID font's `W` array: `c [w1 w2 ..]` or `c_first c_last w`, for the
/// codes a composite font can show (up to [`MAX_COMPOSITE_CODE`]); each
/// width looked at costs [`WIDTH_COST`], as one list may be named many times.
fn parse_cid_widths(
    doc: &Document,
    w: &[Object],
    widths: &mut Widths,
    budget: &mut Budget<'_>,
) -> Result<(), PdfError> {
    let value = |o: &Object| doc.resolve(o).and_then(Object::as_float);
    let mut i = 0;
    while i < w.len() {
        budget.charge(WIDTH_COST)?;
        let Some(first) = value(&w[i]) else {
            break;
        };
        let first = first as u32;
        match w.get(i + 1).map(|o| doc.resolve(o)) {
            Some(Some(Object::Array(list))) => {
                for (offset, width) in list.iter().enumerate() {
                    let code = u64::from(first) + offset as u64;
                    if code > u64::from(MAX_COMPOSITE_CODE) {
                        break;
                    }
                    budget.charge(WIDTH_COST)?;
                    if let Some(width) = value(width) {
                        widths.insert(code as u32, width);
                    }
                }
                i += 2;
            }
            Some(Some(_)) => {
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

/// The `ToUnicode` map of `font`: it comes before the font's `Encoding`,
/// whatever that says.
fn font_map<'a>(document: &'a Document, font: &'a Dictionary) -> Option<&'a Stream> {
    if !encoding::is_font(document, font) {
        return None;
    }
    document.get(font, b"ToUnicode")?.as_stream()
}

/// The entries of the `Differences` read for `font`: those of the
/// dictionary its `Encoding` leads to, through references.
fn differences(document: &Document, font: &Dictionary) -> usize {
    document
        .get(font, b"Encoding")
        .and_then(Object::as_dict)
        .and_then(|encoding| document.get(encoding, b"Differences"))
        .and_then(Object::as_array)
        .map_or(0, Vec::len)
}

/// What reading `font` builds, counted before it is read: the font itself
/// and the maps built of the `Differences`, one entry for each code they can
/// name. Its `ToUnicode` map is left to the cache.
fn font_before(document: &Document, font: &Dictionary) -> usize {
    let codes = differences(document, font).min(DIFFERENCE_CODES);
    FONT_INFO_MEMORY.saturating_add(codes.saturating_mul(DIFFERENCE_MEMORY))
}

fn font_too_large(limit: usize) -> PdfError {
    PdfError::TooLarge(format!(
        "a font of a PDF takes more memory to read than the limit of {} for fonts",
        size(limit)
    ))
}

fn page_resources(document: &Document, page_id: ObjectId) -> Option<&Dictionary> {
    let mut node = document.dictionary(page_id)?;
    for _ in 0..64 {
        if let Some(resources) = node.get(b"Resources") {
            return document.resolve(resources).and_then(Object::as_dict);
        }
        node = document.resolve(node.get(b"Parent")?)?.as_dict()?;
    }
    None
}

/// A PDF text string: UTF-16BE with a byte-order mark, UTF-8 with one, or
/// otherwise each byte as the character of that number (Latin-1). Never
/// fails: what does not decode becomes replacement characters.
pub fn decode_text_string(bytes: &[u8]) -> String {
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

/// The width of character `code` of the bold Helvetica that stamps are
/// written in, and that a standard font without widths is read with, in
/// thousandths of the font size: printable ASCII from the font's metrics,
/// 611 for the rest.
pub fn standard_width(code: u32) -> u32 {
    const ASCII: [u16; 95] = [
        278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278,
        278, // ' '..'/'
        556, 556, 556, 556, 556, 556, 556, 556, 556, 556, // 0..9
        333, 333, 584, 584, 584, 611, 975, // ':'..'@'
        722, 722, 722, 722, 667, 611, 778, 722, 278, 556, 722, 611, 833, 722, 778, 667, 778, 722,
        667, 611, 722, 667, 944, 667, 667, 611, // A..Z
        333, 278, 333, 584, 556, 333, // '['..'`'
        556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556, 278, 889, 611, 611, 611, 611, 389,
        556, 333, 611, 556, 778, 556, 556, 500, // a..z
        389, 280, 389, 584, // '{'..'~'
    ];
    match code {
        0x20..=0x7e => ASCII
            .get((code - 0x20) as usize)
            .map_or(611, |&width| u32::from(width)),
        _ => 611,
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

fn too_many_glyphs(limit: usize) -> PdfError {
    PdfError::TooLarge(format!(
        "a PDF page shows more than {limit} glyphs, more than can be put in order"
    ))
}

/// What is left of a page's budget of comparisons.
struct Steps(usize);

impl Steps {
    /// Spends `count` comparisons, or fails when the page has used them
    /// all.
    fn spend(&mut self, count: usize) -> Result<(), PdfError> {
        self.0 = self.0.checked_sub(count).ok_or_else(|| {
            PdfError::TooLarge(
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
#[cfg(test)]
fn assemble(chunks: Vec<Chunk>, steps: usize) -> Result<String, PdfError> {
    assemble_counted(chunks, steps).map(|(text, ..)| text)
}

/// [`assemble`], with how many of `allowed` steps it spent, and the lines it
/// made.
fn assemble_counted(
    mut chunks: Vec<Chunk>,
    allowed: usize,
) -> Result<(String, usize, usize), PdfError> {
    let mut steps = Steps(allowed);
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
    let lines = out.len();
    Ok((out.join("\n"), allowed - steps.0, lines))
}

/// A glyph waiting in the sweep of [`duplicates`], greater the better it
/// ranks: the wider, and the earlier among equals.
struct Ranked {
    width: f32,
    index: usize,
}

impl PartialEq for Ranked {
    fn eq(&self, other: &Ranked) -> bool {
        self.cmp(other).is_eq()
    }
}

impl Eq for Ranked {}

impl PartialOrd for Ranked {
    fn partial_cmp(&self, other: &Ranked) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Ranked {
    fn cmp(&self, other: &Ranked) -> std::cmp::Ordering {
        self.width
            .total_cmp(&other.width)
            .then(other.index.cmp(&self.index))
    }
}

/// Which glyphs of a line repeat another one: `duplicate[i]` is true when
/// some other glyph with the same text is wider than glyph `i` (or as wide,
/// and earlier) and spans its centre.
///
/// Within the glyphs that share a text, a glyph is a duplicate when some glyph
/// that ranks better (the widest first, the earlier first among equals) spans
/// its centre. That is found in one sweep along the line: the centres in order,
/// the glyphs that start before each one waiting in a heap by rank, those that
/// have ended thrown away when they reach its top. It is the same answer as
/// comparing every pair, in time proportional to the number of glyphs times its
/// logarithm, and it reads the glyphs in the order of the line and not in the
/// order of their widths.
fn duplicates(line: &[Chunk], steps: &mut Steps) -> Result<Vec<bool>, PdfError> {
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
        let ranked = |i: usize| Ranked {
            width: line[i].x1 - line[i].x0 + 0.0,
            index: i,
        };
        let centre = |i: usize| (line[i].x0 + line[i].x1) / 2.0 + 0.0;
        let mut starts = group.clone();
        starts.sort_unstable_by(|&a, &b| line[a].x0.total_cmp(&line[b].x0).then(a.cmp(&b)));
        let mut centres = group;
        centres.sort_unstable_by(|&a, &b| centre(a).total_cmp(&centre(b)).then(a.cmp(&b)));
        let mut waiting = std::collections::BinaryHeap::new();
        let mut next = 0;
        for i in centres {
            let at = centre(i);
            while let Some(&started) = starts.get(next) {
                if line[started].x0 > at {
                    break;
                }
                waiting.push(ranked(started));
                next += 1;
            }
            // Those that ended before this centre end before the later ones.
            while let Some(best) = waiting.peek() {
                if line[best.index].x1 >= at {
                    break;
                }
                waiting.pop();
            }
            duplicate[i] = waiting.peek().is_some_and(|best| *best > ranked(i));
        }
    }
    Ok(duplicate)
}

/// Zero-width glyphs (marks: dots, vowel signs, accents drawn apart) of a
/// line in visual order, merged into the glyph they sit on: dropped when
/// they repeat its text (fonts that draw a letter's dots as a separate
/// glyph map both to the letter), appended to it otherwise.
fn attach_marks(line: Vec<Chunk>, steps: &mut Steps) -> Result<Vec<Chunk>, PdfError> {
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
        // "مرحبا Anu Lakhotia في الدورة 2026": mostly Arabic, so read from
        // the right; the name and the year stay left to right.
        let visual = strings(&[
            "2026",
            " ",
            "الدورة",
            " ",
            "في",
            " ",
            "Anu",
            " ",
            "Lakhotia",
            " ",
            "مرحبا",
        ]);
        assert_eq!(
            reading_order(visual).concat(),
            "مرحبا Anu Lakhotia في الدورة 2026"
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

    /// A document of `lines` of 11-point Helvetica on one page.
    fn document_of(lines: &[&str]) -> Document {
        let mut operations = vec![
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec!["F1".into(), 11.into()]),
            Operation::new("TL", vec![13.into()]),
            Operation::new("Td", vec![50.into(), 800.into()]),
        ];
        for (index, line) in lines.iter().enumerate() {
            if index > 0 {
                operations.push(Operation::new("T*", vec![]));
            }
            operations.push(Operation::new(
                "Tj",
                vec![Object::string_literal(line.as_bytes())],
            ));
        }
        operations.push(Operation::new("ET", vec![]));
        let content = crate::content::encode(&operations);
        pages_with(
            |document| {
                let font = document.add(dictionary! {
                    "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
                    "Encoding" => "WinAnsiEncoding",
                });
                dictionary! { "Font" => dictionary! { "F1" => font } }
            },
            &[content],
        )
    }

    #[test]
    fn a_page_with_too_many_glyphs_is_refused() {
        let document = document_of(&["Hello, world"]);
        let tight = TextLimits {
            page_glyphs: 11,
            ..TextLimits::default()
        };
        let error = page_texts_within(&document, tight).unwrap_err();
        assert!(matches!(error, PdfError::TooLarge(_)), "{error:?}");
        let fits = TextLimits {
            page_glyphs: 12,
            ..TextLimits::default()
        };
        assert_eq!(
            page_texts_within(&document, fits).unwrap(),
            ["Hello, world"]
        );
    }

    #[test]
    fn a_page_that_takes_too_much_work_is_refused() {
        let document = document_of(&["Hello, world"]);
        // At least its content, and what its operations and codes cost.
        let (_, page_length) = page_texts_counted(&document, TextLimits::default()).unwrap();
        let content = document
            .page_content(document.pages()[0], usize::MAX)
            .unwrap()
            .len();
        assert!(page_length > content, "{page_length} for {content}");
        let tight = TextLimits {
            page_work: page_length - 1,
            ..TextLimits::default()
        };
        let error = page_texts_within(&document, tight).unwrap_err();
        assert!(matches!(error, PdfError::TooLarge(_)), "{error:?}");
        let fits = TextLimits {
            page_work: page_length,
            ..TextLimits::default()
        };
        assert_eq!(
            page_texts_within(&document, fits).unwrap(),
            ["Hello, world"]
        );
        // The pages together are held to their own limit.
        let two = document_of(&["one"]);
        let both = TextLimits {
            document_work: 1,
            ..TextLimits::default()
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
        assert!(matches!(error, PdfError::TooLarge(_)), "{error:?}");
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

    use crate::content::Operation;
    use crate::{Objects, ReadLimits, StringFormat, dictionary};

    /// The document `objects` save to, read back.
    fn loaded(objects: Objects) -> Document {
        Document::load(objects.save().unwrap(), ReadLimits::default()).unwrap()
    }

    /// `stream`, compressed.
    fn compressed(stream: Stream) -> Stream {
        let mut dict = stream.dict;
        dict.set("Filter", "FlateDecode");
        Stream::new(dict, crate::filters::deflate(&stream.content))
    }

    /// A document of one page per entry of `contents`, all with the
    /// resources `resources` builds (it may add objects).
    fn pages_with(
        resources: impl FnOnce(&mut Objects) -> Dictionary,
        contents: &[Vec<u8>],
    ) -> Document {
        loaded(objects_with(resources, contents))
    }

    /// The objects of [`pages_with`], not yet saved.
    fn objects_with(
        resources: impl FnOnce(&mut Objects) -> Dictionary,
        contents: &[Vec<u8>],
    ) -> Objects {
        let mut document = Objects::new();
        let pages_id = document.reserve();
        let resources = resources(&mut document);
        let resources_id = document.add(resources);
        let kids: Vec<Object> = contents
            .iter()
            .map(|content| {
                let content_id = document.add(Stream::new(Dictionary::new(), content.clone()));
                document
                    .add(dictionary! {
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
        document.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => kids, "Count" => count,
            }),
        );
        let catalog = document.add(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        document.trailer.set("Root", catalog);
        document
    }

    /// Resources with Helvetica as `/F1`.
    fn helvetica(document: &mut Objects) -> Dictionary {
        let font = document.add(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        });
        dictionary! { "Font" => dictionary! { "F1" => font } }
    }

    fn too_large(result: Result<Vec<String>, PdfError>) -> String {
        match result {
            Err(PdfError::TooLarge(message)) => message,
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
                let properties = document.add(dictionary! {
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
        let tight = TextLimits {
            page_work: length + 10_000,
            ..TextLimits::default()
        };
        too_large(page_texts_within(&document, tight));
        // A few sequences fit.
        let (few, length) = actual_text_page(2, 10_000);
        let fits = TextLimits {
            page_work: length + 4 * 10_000,
            ..TextLimits::default()
        };
        let texts = page_texts_within(&few, fits).unwrap();
        assert_eq!(texts[0].len(), 2 * 10_000 + 1, "{:?}", &texts[0][..20]);
    }

    #[test]
    fn a_page_may_produce_only_so_much_text() {
        let (document, _) = actual_text_page(200, 10_000);
        let tight = TextLimits {
            page_text: 1024 * 1024,
            ..TextLimits::default()
        };
        let message = too_large(page_texts_within(&document, tight));
        assert!(message.contains("1 MiB"), "{message}");
        let roomy = TextLimits {
            page_text: 4 * 1024 * 1024,
            ..TextLimits::default()
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
        let tight = TextLimits {
            document_text: 10_000,
            ..TextLimits::default()
        };
        let message = too_large(page_texts_within(&document, tight));
        assert!(message.contains("10000 bytes"), "{message}");
        let fits = TextLimits {
            document_text: 12_000,
            ..TextLimits::default()
        };
        assert_eq!(page_texts_within(&document, fits).unwrap().len(), 4);
    }

    #[test]
    fn marked_content_nested_deeper_than_the_limit_is_refused() {
        let nested = |depth: usize| {
            let content = format!("{}BT /F1 12 Tf (a) Tj ET", "/P BMC ".repeat(depth));
            pages_with(helvetica, &[content.into_bytes()])
        };
        too_large(page_texts_within(&nested(257), TextLimits::default()));
        assert_eq!(
            page_texts_within(&nested(256), TextLimits::default()).unwrap(),
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
        too_large(page_texts_within(&nested(257), TextLimits::default()));
        assert_eq!(
            page_texts_within(&nested(256), TextLimits::default()).unwrap(),
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
        assert!(matches!(error, PdfError::TooLarge(_)), "{error:?}");
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
    fn fonts_sharing_a_map(cmap: &str, names: usize) -> impl FnOnce(&mut Objects) -> Dictionary {
        let cmap = cmap.to_owned();
        move |document| {
            let stream = compressed(Stream::new(Dictionary::new(), cmap.into_bytes()));
            let map = document.add(stream);
            let font = document.add(dictionary! {
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
        let tight = TextLimits {
            page_work: content.len() + cmap.len() / 2,
            ..TextLimits::default()
        };
        too_large(page_texts_within(&document, tight));
        // Room for reading it once, not a hundred times: one font, read once.
        let once = TextLimits {
            page_work: content.len() + 1 + 50 * cmap.len(),
            ..TextLimits::default()
        };
        page_texts_within(&document, once).unwrap();
    }

    #[test]
    fn a_font_used_on_every_page_is_read_once_for_the_document() {
        let cmap = to_unicode_map(8000);
        let content = b"BT /F0 12 Tf (a) Tj ET".to_vec();
        let document = pages_with(fonts_sharing_a_map(&cmap, 1), &vec![content.clone(); 20]);
        let once = TextLimits {
            document_work: 20 * (content.len() + 1) + 50 * cmap.len(),
            ..TextLimits::default()
        };
        assert_eq!(page_texts_within(&document, once).unwrap().len(), 20);
    }

    /// One page showing a glyph of a composite font with `ToUnicode` as its
    /// map and `widths` as its descendant's `W`.
    fn composite_font_page(to_unicode: &str, widths: Vec<Object>) -> Document {
        let to_unicode = to_unicode.to_owned();
        pages_with(
            move |document| {
                let stream = compressed(Stream::new(Dictionary::new(), to_unicode.into_bytes()));
                let map = document.add(stream);
                let descendant = document.add(dictionary! {
                    "Type" => "Font", "Subtype" => "CIDFontType2", "BaseFont" => "Sample",
                    "W" => widths,
                });
                let font = document.add(dictionary! {
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
    fn a_to_unicode_range_over_millions_of_codes_is_one_range() {
        // One line of a range over 2,097,152 three-byte codes, or over every
        // four-byte code there is: a range is one entry, which no code is
        // visited to read, so the map is counted for its line.
        for (codespace, first, last) in [
            ("<000000> <FFFFFF>", "<000000>", "<1FFFFF>"),
            ("<00000000> <FFFFFFFF>", "<00000000>", "<FFFFFFFF>"),
        ] {
            let cmap = format!(
                "/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n\
                 /CMapName /Sample def\n\
                 1 begincodespacerange {codespace} endcodespacerange\n\
                 1 beginbfrange\n{first} {last} <0041>\nendbfrange\n\
                 endcmap CMapName currentdict /CMap defineresource pop end end\n"
            );
            let (map, counted) = ToUnicode::read_within(cmap.as_bytes(), usize::MAX).unwrap();
            assert!(counted < 64 * 1024, "{counted} bytes for one line");
            drop(map);
            // A font whose limit is a few kilobytes more than that reads it.
            let tight = TextLimits {
                font_memory: 64 * 1024,
                ..TextLimits::default()
            };
            page_texts_within(&composite_font_page(&cmap, Vec::new()), tight).unwrap();
        }
    }

    #[test]
    fn a_to_unicode_map_with_many_lines_is_counted_for_the_ranges_it_builds() {
        // 100,000 lines of single codes: two ranges a line at the most.
        let counted = |lines: usize| {
            let mut cmap = String::from(MAP_HEAD);
            for block in 0..lines.div_ceil(100) {
                let count = (lines - block * 100).min(100);
                cmap.push_str(&format!("{count} beginbfchar\n"));
                for i in 0..count {
                    cmap.push_str(&format!("<{:04X}> <0041>\n", block * 100 + i));
                }
                cmap.push_str("endbfchar\n");
            }
            cmap.push_str(MAP_TAIL);
            ToUnicode::read_within(cmap.as_bytes(), usize::MAX)
                .unwrap()
                .1
        };
        let (small, large) = (counted(1_000), counted(100_000));
        assert!(
            large > 90 * small,
            "{small} for 1,000 lines, {large} for 100,000"
        );
        assert!(large < 64 * 1024 * 1024, "{large}");
        // A font limit under what the large map is counted as refuses it.
        let tight = TextLimits {
            font_memory: usize::try_from(large).unwrap() / 2,
            ..TextLimits::default()
        };
        let mut cmap = String::from(MAP_HEAD);
        for block in 0..1_000 {
            cmap.push_str("100 beginbfchar\n");
            for i in 0..100 {
                cmap.push_str(&format!("<{:04X}> <0041>\n", block * 100 + i));
            }
            cmap.push_str("endbfchar\n");
        }
        cmap.push_str(MAP_TAIL);
        too_large(page_texts_within(
            &composite_font_page(&cmap, Vec::new()),
            tight,
        ));
    }

    #[test]
    fn character_widths_keep_only_the_codes_a_font_can_show() {
        let cmap = to_unicode_map(4);
        // The map is read: code 1 is U+4E01.
        assert_eq!(
            page_texts_within(
                &composite_font_page(&cmap, Vec::new()),
                TextLimits::default()
            )
            .unwrap(),
            ["\u{4e01}"]
        );
        // Codes past the last one a font can have do not wrap around.
        let past_the_end = vec![
            Object::Integer(4_294_967_295),
            vec![500.into(), 500.into()].into(),
        ];
        page_texts_within(
            &composite_font_page(&cmap, past_the_end),
            TextLimits::default(),
        )
        .unwrap();
        // 40 ranges of 65,536 codes each, all past the codes a composite
        // font can show: nothing to keep.
        let far: Vec<Object> = (1..=40i64)
            .flat_map(|k| [k * 65_536, k * 65_536 + 65_535, 500])
            .map(Object::Integer)
            .collect();
        let tight = TextLimits {
            page_work: 1024 * 1024,
            ..TextLimits::default()
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
                let font = document.add(dictionary! {
                    "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
                    "FirstChar" => 4_294_967_295i64,
                    "Widths" => vec![500.into(), 500.into()],
                });
                dictionary! { "Font" => dictionary! { "F1" => font } }
            },
            &[b"BT /F1 12 Tf (a) Tj ET".to_vec()],
        );
        page_texts_within(&document, TextLimits::default()).unwrap();
    }

    #[test]
    fn the_message_names_the_limit_that_ran_out() {
        let document = pages_with(helvetica, &[b"BT /F1 12 Tf (a) Tj ET".to_vec()]);
        let page = TextLimits {
            page_work: 10,
            ..TextLimits::default()
        };
        let message = too_large(page_texts_within(&document, page));
        assert!(
            message.contains("10 bytes") && message.contains("page"),
            "{message}"
        );
        assert!(!message.contains("0 MiB"), "{message}");
        let whole = TextLimits {
            document_work: 10,
            ..TextLimits::default()
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
    fn map_stream(document: &mut Objects, cmap: &str) -> ObjectId {
        document.add(compressed(Stream::new(
            Dictionary::new(),
            cmap.as_bytes().to_vec(),
        )))
    }

    /// A composite font whose ToUnicode map is `map`.
    fn composite_font(document: &mut Objects, map: ObjectId) -> ObjectId {
        document.add(dictionary! {
            "Type" => "Font", "Subtype" => "Type0", "BaseFont" => "Sample",
            "Encoding" => "Identity-H",
            "ToUnicode" => map,
        })
    }

    /// One page per entry of `contents`, each with its own resources from
    /// `resources`.
    fn pages_each_with(
        resources: impl FnOnce(&mut Objects) -> Vec<Dictionary>,
        contents: &[Vec<u8>],
    ) -> Document {
        let mut document = objects_with(|_| Dictionary::new(), contents);
        let resources = resources(&mut document);
        for (page_id, resources) in document.pages().into_iter().zip(resources) {
            document
                .dictionary_mut(page_id)
                .unwrap()
                .set("Resources", resources);
        }
        loaded(document)
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
    fn a_to_unicode_range_split_by_many_codes_shares_its_text() {
        // A range whose text is a list of 256 strings, split by 1,000
        // single codes: the pieces share the list, so the map is counted for
        // its lines and the list once.
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
        let (_, split) =
            ToUnicode::read_within(split_list_map(256, 1000).as_bytes(), usize::MAX).unwrap();
        let (_, once) =
            ToUnicode::read_within(split_list_map(256, 0).as_bytes(), usize::MAX).unwrap();
        // 1,000 lines of `<code> <0042>`, a range for each, and the list once.
        assert!(
            split < once + 1_000 * 400,
            "{split} bytes, {once} without the splits"
        );
        let tight = TextLimits {
            font_memory: 1024 * 1024,
            ..TextLimits::default()
        };
        // Code 1 is one of the splits.
        assert_eq!(
            page_texts_within(&page(split_list_map(256, 1000)), tight).unwrap(),
            ["B"]
        );
        // A font limit under what the map is counted as refuses it.
        let message = too_large(page_texts_within(
            &page(split_list_map(256, 1000)),
            TextLimits {
                font_memory: 64 * 1024,
                ..TextLimits::default()
            },
        ));
        assert!(message.contains("64 KiB"), "{message}");
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
        let texts = page_texts_within(&document, TextLimits::default()).unwrap();
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
        let limits = TextLimits {
            document_work: 64 * 1024 * 1024,
            font_memory: 40 * 1024 * 1024,
            ..TextLimits::default()
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
        let once = TextLimits {
            page_work: 1024 * 1024,
            ..TextLimits::default()
        };
        let texts = page_texts_within(&document, once).unwrap();
        assert_eq!(texts, [vec!["\u{4e41}"; fonts].join("\n")]);
    }

    /// The start of a map, without a codespace.
    const MAP_START: &str = "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
                             /CMapName /Sample def\n";

    /// A page showing code `0001` of a composite font whose `Encoding` is a
    /// dictionary over `Identity-H` reached through `hops` references, and
    /// whose map covers every two-byte code.
    fn encoding_through_references(hops: usize) -> Document {
        let cmap =
            format!("{MAP_HEAD}1 beginbfrange\n<0000> <FFFF> <4E00>\nendbfrange\n{MAP_TAIL}");
        pages_with(
            move |document| {
                let map = map_stream(document, &cmap);
                let mut target = document.add(dictionary! { "BaseEncoding" => "Identity-H" });
                for _ in 1..hops {
                    target = document.add(Object::Reference(target));
                }
                let font = document.add(dictionary! {
                    "Type" => "Font", "Subtype" => "Type0", "BaseFont" => "Sample",
                    "Encoding" => Object::Reference(target),
                    "ToUnicode" => map,
                });
                dictionary! { "Font" => dictionary! { "F1" => font } }
            },
            &[b"BT /F1 12 Tf 1 0 0 1 10 700 Tm <0001> Tj ET".to_vec()],
        )
    }

    #[test]
    fn a_map_read_through_a_long_chain_of_encodings_is_counted() {
        // The chain is followed to the end, then the map is read.
        let tight = TextLimits {
            font_memory: 4 * 1024,
            page_work: 4096,
            ..TextLimits::default()
        };
        for hops in [1, 128, 129, 300] {
            too_large(page_texts_within(&encoding_through_references(hops), tight));
        }
        assert_eq!(
            page_texts_within(&encoding_through_references(129), TextLimits::default()).unwrap(),
            ["\u{4e01}"]
        );
    }

    /// A simple font whose map has `ranges` codespace ranges, and a page
    /// showing `glyphs` of `B`, which none of them covers.
    fn codespace_ranges(ranges: usize, glyphs: usize) -> Document {
        let mut cmap = format!("{MAP_START}{ranges} begincodespacerange\n");
        cmap.push_str(&"<00> <00>\n".repeat(ranges));
        cmap.push_str("endcodespacerange\n1 beginbfchar\n<41> <0041>\nendbfchar\n");
        cmap.push_str(MAP_TAIL);
        let content = format!(
            "BT /F1 12 Tf 1 0 0 1 10 700 Tm ({}A) Tj ET",
            "B".repeat(glyphs)
        );
        pages_with(
            move |document| {
                let map = map_stream(document, &cmap);
                let font = document.add(dictionary! {
                    "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
                    "ToUnicode" => map,
                });
                dictionary! { "Font" => dictionary! { "F1" => font } }
            },
            &[content.into_bytes()],
        )
    }

    #[test]
    fn a_map_with_more_codespace_ranges_than_the_cap_is_refused() {
        // Every range is checked for each code decoded.
        let message = too_large(page_texts_within(
            &codespace_ranges(65, 10),
            TextLimits::default(),
        ));
        assert!(
            message.contains("codespace") && message.contains("64"),
            "{message}"
        );
        assert_eq!(
            page_texts_within(&codespace_ranges(64, 10), TextLimits::default()).unwrap(),
            ["A"]
        );
    }

    /// A page showing code `0001` of a composite font with `cmap`.
    fn page_of_map(cmap: String) -> Document {
        pages_with(
            move |document| {
                let map = map_stream(document, &cmap);
                let font = composite_font(document, map);
                dictionary! { "Font" => dictionary! { "F1" => font } }
            },
            &[b"BT /F1 12 Tf 1 0 0 1 10 700 Tm <0001> Tj ET".to_vec()],
        )
    }

    /// `ranges` lines `<XX00> <XXFF> [..]` of `texts` texts each.
    fn lists_map(ranges: usize, texts: usize) -> String {
        let mut cmap = String::from(MAP_HEAD);
        for block in 0..ranges.div_ceil(100) {
            let count = (ranges - block * 100).min(100);
            cmap.push_str(&format!("{count} beginbfrange\n"));
            for line in block * 100..block * 100 + count {
                let high = line % 256;
                cmap.push_str(&format!("<{high:02X}00> <{high:02X}FF> ["));
                for low in 0..texts {
                    let unit = 0x4E00 + ((high * 256 + low) % 0x5000);
                    cmap.push_str(&format!("<{unit:04X}> "));
                }
                cmap.push_str("]\n");
            }
            cmap.push_str("endbfrange\n");
        }
        cmap.push_str(MAP_TAIL);
        cmap
    }

    #[test]
    fn a_full_map_written_as_lists_reads() {
        // Every two-byte code, 256 lines of 256 texts: 463 KB, counted as
        // about 8 MB.
        let texts = page_texts_within(&page_of_map(lists_map(256, 256)), TextLimits::default());
        assert_eq!(texts.unwrap(), ["\u{4e01}"]);
    }

    #[test]
    fn maps_past_their_caps_are_refused() {
        // A list of more texts than a range has codes for.
        let message = too_large(page_texts_within(
            &page_of_map(lists_map(1, 257)),
            TextLimits::default(),
        ));
        assert!(
            message.contains("257") || message.contains("256"),
            "{message}"
        );
        // More texts in lists than there are codes of two bytes.
        too_large(page_texts_within(
            &page_of_map(lists_map(258, 256)),
            TextLimits::default(),
        ));
        // More lines than twice every code of one and two bytes.
        let mut cmap = String::from(MAP_HEAD);
        let lines: usize = 131_073;
        for block in 0..lines.div_ceil(100) {
            let count = (lines - block * 100).min(100);
            cmap.push_str(&format!("{count} beginbfchar\n"));
            for line in block * 100..block * 100 + count {
                cmap.push_str(&format!("<{:04X}> <0041>\n", line % 65_536));
            }
            cmap.push_str("endbfchar\n");
        }
        cmap.push_str(MAP_TAIL);
        let message = too_large(page_texts_within(&page_of_map(cmap), TextLimits::default()));
        assert!(message.contains("131072"), "{message}");
    }

    #[test]
    fn the_differences_of_a_simple_font_count_toward_font_memory() {
        let names: Vec<String> = (b'a'..=b'z')
            .chain(b'A'..=b'Z')
            .map(|c| (c as char).to_string())
            .collect();
        let document = pages_with(
            |document| {
                let mut differences: Vec<Object> = vec![0.into()];
                differences.extend((0..256).map(|c| Object::name(&names[c % names.len()])));
                let encoding = document.add(dictionary! {
                    "Type" => "Encoding", "Differences" => differences,
                });
                let font = document.add(dictionary! {
                    "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
                    "Encoding" => encoding,
                });
                dictionary! { "Font" => dictionary! { "F1" => font } }
            },
            &[b"BT /F1 12 Tf (a) Tj ET".to_vec()],
        );
        // Code 97 is the 98th name: `T`.
        assert_eq!(
            page_texts_within(&document, TextLimits::default()).unwrap(),
            ["T"]
        );
        let tight = TextLimits {
            font_memory: 16 * 1024,
            ..TextLimits::default()
        };
        too_large(page_texts_within(&document, tight));
    }

    /// `fonts` simple fonts with 256 widths each, shown in turn `rounds`
    /// times.
    fn fonts_in_turn(fonts: usize, rounds: usize) -> Document {
        let content: Vec<u8> = (0..rounds)
            .flat_map(|_| {
                (0..fonts).flat_map(|i| format!("BT /F{i} 12 Tf (a) Tj ET\n").into_bytes())
            })
            .collect();
        pages_with(
            move |document| {
                let widths = document.add(Object::Array(vec![500.into(); 256]));
                let mut names = Dictionary::new();
                for i in 0..fonts {
                    let font = document.add(dictionary! {
                        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
                        "FirstChar" => 0, "Widths" => widths,
                    });
                    names.set(format!("F{i}"), font);
                }
                dictionary! { "Font" => names }
            },
            &[content],
        )
    }

    #[test]
    fn reading_a_dropped_font_again_costs_work() {
        let (fonts, rounds) = (50, 40);
        let document = fonts_in_turn(fonts, rounds);
        let page = document.pages()[0];
        let content = document.page_content(page, usize::MAX).unwrap().len();
        // Each font read once: at least its widths.
        let (_, budget) = page_texts_counted(&document, TextLimits::default()).unwrap();
        assert!(budget > content + fonts * 256 * WIDTH_COST, "{budget}");
        let held = TextLimits {
            page_work: budget,
            ..TextLimits::default()
        };
        page_texts_within(&document, held).unwrap();
        // Room for ten fonts: every run of text reads its font again.
        let one_font = FONT_INFO_MEMORY + 256 * WIDTH_MEMORY;
        let dropped = TextLimits {
            page_work: budget,
            font_memory: 10 * one_font,
            ..TextLimits::default()
        };
        too_large(page_texts_within(&document, dropped));
    }

    /// One range of `<0000>` to `<00FF>` with a list of `texts` texts, then
    /// `codes` single codes after it.
    fn one_list_and_codes(texts: usize, codes: usize) -> String {
        let mut cmap = String::from(MAP_HEAD);
        cmap.push_str(
            "1 beginbfrange
<0000> <00FF> [",
        );
        cmap.push_str(&"<0041> ".repeat(texts));
        cmap.push_str(
            "]
endbfrange
",
        );
        for block in 0..codes.div_ceil(100) {
            let count = (codes - block * 100).min(100);
            cmap.push_str(&format!(
                "{count} beginbfchar
"
            ));
            for line in block * 100..block * 100 + count {
                cmap.push_str(&format!(
                    "<{:04X}> <{:04X}>
",
                    0x0100 + line,
                    0x4E00 + line % 0x5000
                ));
            }
            cmap.push_str(
                "endbfchar
",
            );
        }
        cmap.push_str(MAP_TAIL);
        cmap
    }

    #[test]
    fn maps_with_one_long_list_and_many_codes_read() {
        // No line lands inside the list's range, so the list is never
        // copied: each map is counted as a few megabytes.
        for (texts, codes) in [(256, 9_000), (64, 40_000), (50, 65_000)] {
            let document = page_of_map(one_list_and_codes(texts, codes));
            let texts = page_texts_within(&document, TextLimits::default());
            assert_eq!(texts.unwrap(), ["A"], "{codes} codes");
        }
    }
}
