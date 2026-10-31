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

use std::collections::{BTreeMap, HashMap};

use lopdf::Encoding;
use lopdf::content::Content;
use lopdf::{Dictionary, Document, Object, ObjectId};

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

/// How much work reading the text of a PDF may take, so that a small file
/// cannot make a call run for hours or fill the memory. Every figure is
/// per call unless it says otherwise.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Limits {
    /// Bytes of content decoded for one page, counting a form again every
    /// time it is drawn, and [`FORM_DRAW_COST`] for each draw (128 MiB).
    pub(crate) page_work: usize,
    /// The same, over all the pages (1 GiB).
    pub(crate) document_work: usize,
    /// Glyphs one page may show (1,000,000).
    pub(crate) page_glyphs: usize,
    /// Comparisons one page may spend attaching marks to the glyphs they
    /// sit on (50,000,000).
    pub(crate) page_steps: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            page_work: 128 * 1024 * 1024,
            document_work: 1024 * 1024 * 1024,
            page_glyphs: 1_000_000,
            page_steps: 50_000_000,
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
    let mut document_work = 0usize;
    for page_id in document.get_pages().into_values() {
        let mut walker = Walker {
            document,
            chunks: Vec::new(),
            fonts: HashMap::new(),
            limits,
            work: 0,
            work_allowed: limits
                .page_work
                .min(limits.document_work.saturating_sub(document_work)),
        };
        let content = document
            .get_page_content_with_limit(page_id, MAX_CONTENT)
            .map_err(|e| DocumentsError::Pdf(e.to_string()))?;
        let resources = page_resources(document, page_id);
        walker.walk(&content, resources, IDENTITY, 0)?;
        document_work += walker.work;
        texts.push(assemble(walker.chunks, limits.page_steps)?);
    }
    Ok(texts)
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
    encoding: Option<Encoding<'a>>,
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

struct Walker<'a> {
    document: &'a Document,
    chunks: Vec<Chunk>,
    fonts: HashMap<(Option<usize>, Vec<u8>), Option<FontInfo<'a>>>,
    limits: Limits,
    /// The work done on this page so far, and the most it may take.
    work: usize,
    work_allowed: usize,
}

impl<'a> Walker<'a> {
    fn walk(
        &mut self,
        content: &[u8],
        resources: Option<&'a Dictionary>,
        base: Matrix,
        depth: usize,
    ) -> Result<(), DocumentsError> {
        self.charge(content.len())?;
        let content = Content::decode(content).map_err(|e| DocumentsError::Pdf(e.to_string()))?;
        let resources_key = resources.map(|r| r as *const Dictionary as usize);
        let mut ctm = base;
        let mut stack: Vec<(Matrix, TextState)> = Vec::new();
        let mut state = TextState::default();
        let mut tm = IDENTITY;
        let mut tlm = IDENTITY;
        let mut marks: Vec<Mark> = Vec::new();
        for op in &content.operations {
            let operands = &op.operands;
            let number = |i: usize| {
                operands
                    .get(i)
                    .and_then(|o| o.as_float().ok())
                    .unwrap_or(0.0)
            };
            match op.operator.as_str() {
                "q" => stack.push((ctm, state.clone())),
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
                    let properties = operands.get(1).and_then(|o| self.properties(o, resources));
                    let actual_text = properties
                        .and_then(|p| p.get(b"ActualText").ok())
                        .and_then(|o| self.document.dereference(o).ok())
                        .and_then(|(_, o)| o.as_str().ok())
                        .map(decode_text_string);
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
                    self.charge(FORM_DRAW_COST)?;
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
                    let content = form
                        .decompressed_content_with_limit(MAX_CONTENT)
                        .unwrap_or_else(|_| form.content.clone());
                    self.walk(&content, form_resources, multiply(matrix, ctm), depth + 1)?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Counts `amount` bytes of work, or fails when the page (or the
    /// document) has used all it may.
    fn charge(&mut self, amount: usize) -> Result<(), DocumentsError> {
        self.work = self.work.saturating_add(amount);
        if self.work > self.work_allowed {
            return Err(DocumentsError::TooLarge(format!(
                "a PDF page takes more work to read than allowed: more than {} MiB of content, \
                 counting every time a form is drawn",
                self.limits.page_work / (1024 * 1024)
            )));
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
        let Some(font_name) = state.font.clone() else {
            return Ok(());
        };
        let key = (resources_key, font_name.clone());
        if !self.fonts.contains_key(&key) {
            let info = match resources {
                Some(resources) => self.font_info(resources, &font_name)?,
                None => None,
            };
            self.fonts.insert(key.clone(), info);
        }
        let Some(Some(font)) = self.fonts.get(&key) else {
            return Ok(());
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
                                    if self.chunks.len() >= self.limits.page_glyphs {
                                        return Err(too_many_glyphs(self.limits.page_glyphs));
                                    }
                                    mark.chunk = Some(self.chunks.len());
                                    self.chunks.push(Chunk {
                                        x0,
                                        x1,
                                        y: start.1,
                                        angle: degrees,
                                        size,
                                        text: mark.actual_text.clone().unwrap_or_default(),
                                    });
                                }
                            }
                            continue;
                        }
                        let text = font
                            .encoding
                            .as_ref()
                            .and_then(|e| e.bytes_to_string(code).ok())
                            .unwrap_or_default();
                        if text.is_empty() || text == "\u{fffd}" {
                            continue;
                        }
                        if self.chunks.len() >= self.limits.page_glyphs {
                            return Err(too_many_glyphs(self.limits.page_glyphs));
                        }
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

    fn properties(
        &self,
        operand: &'a Object,
        resources: Option<&'a Dictionary>,
    ) -> Option<&'a Dictionary> {
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

    fn font_info(
        &self,
        resources: &'a Dictionary,
        name: &[u8],
    ) -> Result<Option<FontInfo<'a>>, DocumentsError> {
        let doc = self.document;
        let font = resources
            .get(b"Font")
            .ok()
            .and_then(|o| doc.dereference(o).ok())
            .and_then(|(_, o)| o.as_dict().ok())
            .and_then(|fonts| fonts.get(name).ok())
            .and_then(|o| doc.dereference(o).ok())
            .and_then(|(_, o)| o.as_dict().ok());
        let Some(font) = font else {
            return Ok(None);
        };
        let encoding = match font.get_font_encoding_with_limit(doc, MAX_TO_UNICODE) {
            Ok(encoding) => Some(encoding),
            Err(lopdf::Error::Decompress(lopdf::DecompressError::MemoryLimitExceeded {
                ..
            })) => {
                return Err(DocumentsError::TooLarge(format!(
                    "a font's ToUnicode map inflates to more than {} MiB",
                    MAX_TO_UNICODE / (1024 * 1024)
                )));
            }
            Err(_) => None,
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
                    parse_cid_widths(doc, w, &mut widths);
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
                    for (offset, width) in list.iter().enumerate() {
                        let width = doc
                            .dereference(width)
                            .ok()
                            .and_then(|(_, o)| o.as_float().ok())
                            .unwrap_or(0.0);
                        widths.insert(first + offset as u32, width);
                    }
                }
                None => standard = true,
            }
        }
        Ok(Some(FontInfo {
            encoding,
            code_len: if composite { 2 } else { 1 },
            widths,
            default_width,
            standard,
        }))
    }
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

/// A CID font's `W` array: `c [w1 w2 ..]` or `c_first c_last w`.
fn parse_cid_widths(doc: &Document, w: &[Object], widths: &mut HashMap<u32, f32>) {
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
                    if let Some(width) = value(width) {
                        widths.insert(first + offset as u32, width);
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
                for code in first..=(last as u32).min(first + 65_535) {
                    widths.insert(code, width);
                }
                i += 3;
            }
            _ => break,
        }
    }
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
                "a PDF page has too many marks (accents, dots, vowel signs) to put on the letters                  they belong to"
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
}
