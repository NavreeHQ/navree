//! Paragraph text: style spans, bidi levels (unicode-bidi), a font face
//! per character, shaping (harfrust), line breaking (icu_segmenter, first
//! fit), visual reordering, alignment and justification.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use harfrust::{Buffer, BufferFlags, Language, ShapeOptions, ShaperFont};
use icu_segmenter::options::LineBreakOptions;
use icu_segmenter::{GraphemeClusterSegmenter, LineSegmenter};
use unicode_bidi::{BidiInfo, Level};

use super::fonts::{FaceId, FontBook};
use super::model::{Align, Color, Direction};

/// The character a forced line break (`<br>`, a newline) is stored as.
pub(crate) const LINE_BREAK: char = '\u{2028}';

/// Slack for floating-point widths, in points.
const EPSILON: f32 = 0.01;

/// A resolved text style: nothing left to inherit.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Style {
    pub(crate) families: Arc<[String]>,
    /// Points.
    pub(crate) size: f32,
    pub(crate) bold: bool,
    pub(crate) italic: bool,
    pub(crate) color: Color,
    pub(crate) underline: bool,
    pub(crate) strike: bool,
    /// A multiple of the size.
    pub(crate) line_height: f32,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            families: Arc::from(vec!["Noto Sans".to_owned()]),
            size: 11.0,
            bold: false,
            italic: false,
            color: Color::BLACK,
            underline: false,
            strike: false,
            line_height: 1.4,
        }
    }
}

/// A byte range of a paragraph's text in one style.
#[derive(Clone, Debug)]
pub(crate) struct Span {
    pub(crate) range: Range<usize>,
    pub(crate) style: Arc<Style>,
    pub(crate) link: Option<Arc<str>>,
}

/// A paragraph ready to be laid out.
#[derive(Clone, Debug)]
pub(crate) struct Para {
    pub(crate) text: String,
    /// Contiguous spans covering `text`.
    pub(crate) spans: Vec<Span>,
    /// The style of an empty paragraph and its empty lines.
    pub(crate) base: Arc<Style>,
    pub(crate) align: Align,
    /// `None`: from the first strong character.
    pub(crate) direction: Option<Direction>,
    pub(crate) lang: Option<Arc<str>>,
    /// Keep spaces at line starts (`<pre>`).
    pub(crate) preformatted: bool,
}

impl Para {
    /// An empty paragraph in `base`.
    pub(crate) fn new(base: Arc<Style>) -> Self {
        Self {
            text: String::new(),
            spans: Vec::new(),
            base,
            align: Align::Start,
            direction: None,
            lang: None,
            preformatted: false,
        }
    }

    /// Appends `text` in `style`. Newlines become line breaks and tabs
    /// spaces.
    pub(crate) fn push(&mut self, text: &str, style: Arc<Style>, link: Option<Arc<str>>) {
        let text: String = text
            .replace("\r\n", "\n")
            .chars()
            .map(|c| match c {
                '\n' | '\r' | '\u{2029}' | '\u{b}' | '\u{c}' | '\u{85}' => LINE_BREAK,
                '\t' => ' ',
                c => c,
            })
            .collect();
        if text.is_empty() {
            return;
        }
        let start = self.text.len();
        self.text.push_str(&text);
        let end = self.text.len();
        if let Some(last) = self.spans.last_mut()
            && last.range.end == start
            && last.style == style
            && last.link == link
        {
            last.range.end = end;
            return;
        }
        self.spans.push(Span {
            range: start..end,
            style,
            link,
        });
    }

    fn span_at(&self, byte: usize) -> usize {
        self.spans
            .partition_point(|span| span.range.end <= byte)
            .min(self.spans.len().saturating_sub(1))
    }

    fn style_at(&self, byte: usize) -> &Arc<Style> {
        if self.spans.is_empty() {
            return &self.base;
        }
        &self.spans[self.span_at(byte)].style
    }
}

/// One shaped glyph of a [`GlyphRun`].
#[derive(Clone, Debug)]
pub(crate) struct Glyph {
    pub(crate) id: u32,
    /// The bytes of the run's text the glyph's cluster covers.
    pub(crate) range: Range<usize>,
    /// Points.
    pub(crate) advance: f32,
    pub(crate) x_offset: f32,
    pub(crate) y_offset: f32,
}

/// Glyphs of one face, style and direction, placed on a line.
#[derive(Clone, Debug)]
pub(crate) struct GlyphRun {
    /// From the line's left edge, in points.
    pub(crate) x: f32,
    pub(crate) width: f32,
    pub(crate) face: FaceId,
    pub(crate) style: Arc<Style>,
    pub(crate) link: Option<Arc<str>>,
    /// Where the run's text starts in the paragraph (reading order).
    pub(crate) start: usize,
    /// The source text the glyphs come from.
    pub(crate) text: String,
    /// In visual order (left to right).
    pub(crate) glyphs: Vec<Glyph>,
}

/// One line of a paragraph.
#[derive(Clone, Debug)]
pub(crate) struct Line {
    pub(crate) runs: Vec<GlyphRun>,
    /// From the line's top to its baseline.
    pub(crate) ascent: f32,
    /// From the baseline to the line's bottom.
    pub(crate) descent: f32,
}

impl Line {
    pub(crate) fn height(&self) -> f32 {
        self.ascent + self.descent
    }

    /// The width the runs take, after justification (for tests).
    #[cfg(test)]
    pub(crate) fn width(&self) -> f32 {
        self.runs.iter().map(|run| run.width).sum()
    }

    /// The line's text in visual order (for tests).
    #[cfg(test)]
    pub(crate) fn visual_text(&self) -> String {
        self.runs.iter().map(|run| run.text.as_str()).collect()
    }
}

/// A shaped glyph, positioned by cluster in the paragraph's text.
#[derive(Clone, Debug)]
struct Shaped {
    id: u32,
    /// The byte of the paragraph's text its cluster starts at.
    cluster: usize,
    advance: f32,
    x_offset: f32,
    y_offset: f32,
}

/// A piece of the text with one span, one bidi level and one face.
#[derive(Debug)]
struct Item {
    range: Range<usize>,
    span: usize,
    level: Level,
    face: FaceId,
    glyphs: Vec<Shaped>,
}

/// A paragraph shaped as a whole, before line breaking.
struct Prepared<'t> {
    bidi: BidiInfo<'t>,
    items: Vec<Item>,
    /// Sum of the advances of the glyphs whose clusters start before each
    /// byte.
    prefix: Vec<f32>,
}

impl Prepared<'_> {
    fn width(&self, range: Range<usize>) -> f32 {
        (self.prefix[range.end] - self.prefix[range.start]).max(0.0)
    }
}

/// Shapes and breaks paragraphs with the faces of a [`FontBook`], caching
/// which face prints which character.
pub(crate) struct Shaper<'f> {
    fonts: &'f FontBook,
    /// (families, bold, italic, character) → face.
    selected: HashMap<(usize, bool, bool, char), Option<FaceId>>,
    covers: HashMap<(FaceId, char), bool>,
    languages: HashMap<Arc<str>, Option<Language>>,
}

impl<'f> Shaper<'f> {
    pub(crate) fn new(fonts: &'f FontBook) -> Self {
        Self {
            fonts,
            selected: HashMap::new(),
            covers: HashMap::new(),
            languages: HashMap::new(),
        }
    }

    pub(crate) fn fonts(&self) -> &'f FontBook {
        self.fonts
    }

    /// The paragraph's lines at `width` points.
    pub(crate) fn lines(&mut self, para: &Para, width: f32) -> Vec<Line> {
        if para.text.is_empty() || para.spans.is_empty() {
            return Vec::new();
        }
        let prepared = self.prepare(para);
        let breaks = break_lines(para, &prepared, width);
        let count = breaks.len();
        breaks
            .into_iter()
            .enumerate()
            .map(|(index, (range, hard))| {
                let last = index + 1 == count;
                self.line(para, &prepared, range, width, hard || last)
            })
            .collect()
    }

    /// The narrowest width the paragraph fits in without breaking inside a
    /// word, and its width on one line (per forced line).
    pub(crate) fn measure(&mut self, para: &Para) -> (f32, f32) {
        if para.text.is_empty() || para.spans.is_empty() {
            return (0.0, 0.0);
        }
        let prepared = self.prepare(para);
        let text = &para.text;
        let (mut min, mut max) = (0.0f32, 0.0f32);
        let mut segment_start = 0;
        let mut line_start = 0;
        for (pos, hard) in opportunities(text) {
            let end = trim_end(text, segment_start, pos);
            min = min.max(prepared.width(segment_start..end));
            segment_start = pos;
            if hard {
                let end = trim_end(text, line_start, pos);
                max = max.max(prepared.width(line_start..end));
                line_start = pos;
            }
        }
        (min, max.max(min))
    }

    /// The height of a line in `style` (an empty paragraph's line).
    pub(crate) fn empty_line(&mut self, style: &Style) -> (f32, f32) {
        let face = self
            .fonts
            .primary(&style.families, style.bold, style.italic);
        metrics(self.fonts, face, style)
    }

    fn prepare<'t>(&mut self, para: &'t Para) -> Prepared<'t> {
        let text = para.text.as_str();
        let base = para.direction.map(|direction| match direction {
            Direction::Ltr => Level::ltr(),
            Direction::Rtl => Level::rtl(),
        });
        let bidi = BidiInfo::new(text, base);
        let faces = self.faces(para);
        let lang = para.lang.clone().and_then(|lang| self.language(&lang));
        // Items: maximal runs of one span, one level and one face.
        let mut items: Vec<Item> = Vec::new();
        for (i, (byte, c)) in text.char_indices().enumerate() {
            let Some(face) = faces[i] else {
                continue;
            };
            let span = para.span_at(byte);
            let level = bidi.levels[byte];
            let end = byte + c.len_utf8();
            match items.last_mut() {
                Some(item)
                    if item.range.end == byte
                        && item.span == span
                        && item.level == level
                        && item.face == face =>
                {
                    item.range.end = end;
                }
                _ => items.push(Item {
                    range: byte..end,
                    span,
                    level,
                    face,
                    glyphs: Vec::new(),
                }),
            }
        }
        let mut prefix = vec![0.0; text.len() + 1];
        for item in &mut items {
            let size = para.spans[item.span].style.size;
            item.glyphs = self.shape(
                item.face,
                text,
                item.range.clone(),
                item.level.is_rtl(),
                size,
                lang.as_ref(),
            );
            for glyph in &item.glyphs {
                prefix[glyph.cluster + 1] += glyph.advance;
            }
        }
        for i in 1..prefix.len() {
            prefix[i] += prefix[i - 1];
        }
        Prepared {
            bidi,
            items,
            prefix,
        }
    }

    /// The face of every character (`None` for line breaks).
    fn faces(&mut self, para: &Para) -> Vec<Option<FaceId>> {
        let text = para.text.as_str();
        let chars: Vec<(usize, char)> = text.char_indices().collect();
        let mut faces: Vec<Option<FaceId>> = vec![None; chars.len()];
        // Characters with a script of their own pick by family order.
        for (i, &(byte, c)) in chars.iter().enumerate() {
            if c == LINE_BREAK || inherits(c) {
                continue;
            }
            let style = para.style_at(byte).clone();
            faces[i] = self.select(&style, c);
        }
        // Spaces, joiners and combining marks stay in the face around them.
        for (i, &(byte, c)) in chars.iter().enumerate() {
            if c == LINE_BREAK || faces[i].is_some() {
                continue;
            }
            let span = para.span_at(byte);
            let neighbour = (0..i)
                .rev()
                .take_while(|&j| para.span_at(chars[j].0) == span)
                .find_map(|j| faces[j])
                .filter(|&face| self.covers(face, c))
                .or_else(|| {
                    (i + 1..chars.len())
                        .take_while(|&j| para.span_at(chars[j].0) == span)
                        .find_map(|j| faces[j].filter(|_| !inherits(chars[j].1)))
                        .filter(|&face| self.covers(face, c))
                });
            faces[i] = neighbour.or_else(|| {
                let style = para.style_at(byte).clone();
                self.select(&style, c)
            });
        }
        faces
    }

    fn select(&mut self, style: &Style, c: char) -> Option<FaceId> {
        let key = (
            Arc::as_ptr(&style.families) as *const () as usize,
            style.bold,
            style.italic,
            c,
        );
        if let Some(face) = self.selected.get(&key) {
            return *face;
        }
        let face = self
            .fonts
            .select(&style.families, style.bold, style.italic, c);
        self.selected.insert(key, face);
        face
    }

    fn covers(&mut self, face: FaceId, c: char) -> bool {
        *self
            .covers
            .entry((face, c))
            .or_insert_with(|| self.fonts.face(face).covers(c))
    }

    fn language(&mut self, lang: &Arc<str>) -> Option<Language> {
        self.languages
            .entry(lang.clone())
            .or_insert_with(|| lang.parse::<Language>().ok())
            .clone()
    }

    /// Shapes `text[range]` with `face`, the text around it as context.
    fn shape(
        &self,
        face: FaceId,
        text: &str,
        range: Range<usize>,
        rtl: bool,
        size: f32,
        lang: Option<&Language>,
    ) -> Vec<Shaped> {
        let face = self.fonts.face(face);
        let shaper = ShaperFont::new(&face.font);
        let mut buffer = Buffer::new();
        buffer.push_str(&text[range.clone()]);
        buffer.set_pre_context(&text[..range.start]);
        buffer.set_post_context(&text[range.end..]);
        let mut flags = BufferFlags::empty();
        if range.start == 0 {
            flags |= BufferFlags::BEGINNING_OF_TEXT;
        }
        if range.end == text.len() {
            flags |= BufferFlags::END_OF_TEXT;
        }
        buffer.set_flags(flags);
        buffer.set_direction(if rtl {
            harfrust::Direction::RightToLeft
        } else {
            harfrust::Direction::LeftToRight
        });
        if let Some(lang) = lang {
            buffer.set_language(Some(lang.clone()));
        }
        buffer.guess_segment_properties();
        if harfrust::shape(&shaper, &mut buffer, ShapeOptions::new()).is_err() {
            return Vec::new();
        }
        let scale = size / face.units_per_em();
        buffer
            .glyph_infos()
            .iter()
            .zip(buffer.glyph_positions())
            .map(|(info, position)| Shaped {
                id: info.glyph_id,
                cluster: range.start + info.cluster as usize,
                advance: position.x_advance as f32 * scale,
                x_offset: position.x_offset as f32 * scale,
                y_offset: position.y_offset as f32 * scale,
            })
            .collect()
    }

    /// The line holding `range` of the paragraph, aligned in `width`.
    fn line(
        &mut self,
        para: &Para,
        prepared: &Prepared<'_>,
        range: Range<usize>,
        width: f32,
        last: bool,
    ) -> Line {
        let text = para.text.as_str();
        let mut start = range.start;
        if !para.preformatted {
            start +=
                text[start..range.end].len() - text[start..range.end].trim_start_matches(' ').len();
        }
        let end = trim_end(text, start, range.end).max(start);
        let lang = para.lang.clone().and_then(|lang| self.language(&lang));
        let mut runs: Vec<GlyphRun> = Vec::new();
        if start < end {
            let paragraph = prepared
                .bidi
                .paragraphs
                .iter()
                .find(|p| p.range.start <= start && start < p.range.end)
                .unwrap_or(&prepared.bidi.paragraphs[0]);
            let line_range = start..end.min(paragraph.range.end);
            let (levels, visual) = prepared.bidi.visual_runs(paragraph, line_range);
            for run in visual {
                let rtl = levels[run.start].is_rtl();
                let mut pieces: Vec<&Item> = prepared
                    .items
                    .iter()
                    .filter(|item| item.range.start < run.end && run.start < item.range.end)
                    .collect();
                if rtl {
                    pieces.reverse();
                }
                for item in pieces {
                    let s = item.range.start.max(run.start);
                    let e = item.range.end.min(run.end);
                    let style = para.spans[item.span].style.clone();
                    let whole =
                        s == item.range.start && e == item.range.end && item.level.is_rtl() == rtl;
                    let shaped = if whole {
                        item.glyphs.clone()
                    } else {
                        self.shape(item.face, text, s..e, rtl, style.size, lang.as_ref())
                    };
                    runs.push(glyph_run(
                        text,
                        s..e,
                        shaped,
                        item.face,
                        style,
                        para.spans[item.span].link.clone(),
                    ));
                }
            }
        }
        // Line metrics: the tallest run, or the style at the line's start.
        let (mut ascent, mut descent) = (0.0f32, 0.0f32);
        if runs.is_empty() {
            let style = para.style_at(range.start).clone();
            (ascent, descent) = self.empty_line(&style);
        }
        for run in &runs {
            let (a, d) = metrics(self.fonts, Some(run.face), &run.style);
            ascent = ascent.max(a);
            descent = descent.max(d);
        }
        // Positions, justification and alignment.
        let natural: f32 = runs.iter().map(|run| run.width).sum();
        let base_rtl = prepared
            .bidi
            .paragraphs
            .first()
            .is_some_and(|p| p.level.is_rtl());
        let mut line_width = natural;
        if para.align == Align::Justify && !last && natural < width {
            let spaces: usize = runs
                .iter()
                .map(|run| {
                    run.glyphs
                        .iter()
                        .filter(|g| is_space(&run.text[g.range.clone()]))
                        .count()
                })
                .sum();
            if spaces > 0 {
                let extra = (width - natural) / spaces as f32;
                for run in &mut runs {
                    for glyph in &mut run.glyphs {
                        if is_space(&run.text[glyph.range.clone()]) {
                            glyph.advance += extra;
                        }
                    }
                    run.width = run.glyphs.iter().map(|g| g.advance).sum();
                }
                line_width = width;
            }
        }
        let free = (width - line_width).max(0.0);
        let left = match (para.align, base_rtl) {
            (Align::Left, _) | (Align::Start | Align::Justify, false) | (Align::End, true) => 0.0,
            (Align::Right, _) | (Align::End, false) | (Align::Start | Align::Justify, true) => free,
            (Align::Center, _) => free / 2.0,
        };
        let mut x = left;
        for run in &mut runs {
            run.x = x;
            x += run.width;
        }
        Line {
            runs,
            ascent,
            descent,
        }
    }
}

/// A run's glyphs with cluster ranges relative to its own text.
fn glyph_run(
    text: &str,
    range: Range<usize>,
    shaped: Vec<Shaped>,
    face: FaceId,
    style: Arc<Style>,
    link: Option<Arc<str>>,
) -> GlyphRun {
    let mut clusters: Vec<usize> = shaped.iter().map(|g| g.cluster).collect();
    clusters.sort_unstable();
    clusters.dedup();
    let glyphs: Vec<Glyph> = shaped
        .iter()
        .map(|g| {
            let next = clusters
                .iter()
                .copied()
                .find(|&c| c > g.cluster)
                .unwrap_or(range.end);
            Glyph {
                id: g.id,
                range: g.cluster.saturating_sub(range.start)..next.saturating_sub(range.start),
                advance: g.advance,
                x_offset: g.x_offset,
                y_offset: g.y_offset,
            }
        })
        .collect();
    GlyphRun {
        x: 0.0,
        width: glyphs.iter().map(|g| g.advance).sum(),
        face,
        style,
        link,
        start: range.start,
        text: text[range].to_owned(),
        glyphs,
    }
}

/// (ascent, descent) of a line box in `style` with `face`: the font's
/// ascender and descender plus half the leading each.
fn metrics(fonts: &FontBook, face: Option<FaceId>, style: &Style) -> (f32, f32) {
    let (ascent, descent) = face
        .map(|face| {
            let face = fonts.face(face);
            (face.ascent, face.descent)
        })
        .unwrap_or((0.8, 0.2));
    let half_leading = (style.line_height - (ascent + descent)) / 2.0;
    (
        ((half_leading + ascent) * style.size).max(0.0),
        ((half_leading + descent) * style.size).max(0.0),
    )
}

/// Lines as (range, forced end): first fit at icu_segmenter's break
/// opportunities, splitting between grapheme clusters a word that is
/// wider than the line on its own.
fn break_lines(para: &Para, prepared: &Prepared<'_>, width: f32) -> Vec<(Range<usize>, bool)> {
    let text = para.text.as_str();
    let opportunities = opportunities(text);
    let mut lines = Vec::new();
    let mut start = 0;
    let mut fit: Option<usize> = None;
    let mut i = 0;
    while i < opportunities.len() {
        let (pos, hard) = opportunities[i];
        let end = trim_end(text, start, pos);
        if prepared.width(start..end) <= width + EPSILON {
            if hard {
                lines.push((start..pos, true));
                start = pos;
                fit = None;
            } else {
                fit = Some(pos);
            }
            i += 1;
        } else if let Some(at) = fit.take() {
            lines.push((start..at, false));
            start = at;
        } else {
            let cut = emergency_break(text, prepared, start, end, width);
            if cut >= end {
                // Nothing to cut: let it overflow.
                fit = Some(pos);
                if hard {
                    lines.push((start..pos, true));
                    start = pos;
                    fit = None;
                }
                i += 1;
            } else {
                lines.push((start..cut, false));
                start = cut;
            }
        }
    }
    if start < text.len() || lines.is_empty() {
        lines.push((start..text.len(), true));
    }
    lines
}

/// Where to cut `text[start..end]`, too wide for `width`: after the most
/// grapheme clusters that fit, at least one.
fn emergency_break(
    text: &str,
    prepared: &Prepared<'_>,
    start: usize,
    end: usize,
    width: f32,
) -> usize {
    let segmenter = GraphemeClusterSegmenter::new();
    let mut cut = end;
    let mut first = None;
    for boundary in segmenter.segment_str(&text[start..end]) {
        let at = start + boundary;
        if at == start {
            continue;
        }
        first.get_or_insert(at);
        if prepared.width(start..at) <= width + EPSILON {
            cut = at;
        } else {
            break;
        }
    }
    if cut == end {
        // Not even the first cluster fits: cut after it.
        return first.unwrap_or(end);
    }
    cut
}

/// Break opportunities after position 0: (byte, forced).
fn opportunities(text: &str) -> Vec<(usize, bool)> {
    static SEGMENTER: icu_segmenter::LineSegmenterBorrowed<'static> =
        LineSegmenter::new_for_non_complex_scripts(LineBreakOptions::default());
    let mut list: Vec<(usize, bool)> = SEGMENTER
        .segment_str(text)
        .filter(|&pos| pos > 0)
        .map(|pos| (pos, text[..pos].ends_with(LINE_BREAK) || pos == text.len()))
        .collect();
    // A break right after every forced break, whatever the segmenter says.
    for (byte, c) in text.char_indices() {
        let pos = byte + c.len_utf8();
        if c == LINE_BREAK && !list.iter().any(|&(p, _)| p == pos) {
            list.push((pos, true));
        }
    }
    list.sort_unstable_by_key(|&(pos, _)| pos);
    list.dedup_by_key(|(pos, _)| *pos);
    list
}

/// `end` moved back over trailing spaces and line breaks.
fn trim_end(text: &str, start: usize, end: usize) -> usize {
    let slice = &text[start..end];
    start
        + slice
            .trim_end_matches(|c: char| c.is_whitespace() || c == LINE_BREAK)
            .len()
}

/// Whether `c` takes the face of the text around it: spaces, joiners,
/// variation selectors and combining marks.
fn inherits(c: char) -> bool {
    c.is_whitespace()
        || matches!(c as u32,
            0x200B..=0x200D
            | 0x2060
            | 0xFE00..=0xFE0F
            | 0x0300..=0x036F
            | 0x1AB0..=0x1AFF
            | 0x1DC0..=0x1DFF
            | 0x20D0..=0x20FF
            | 0xFE20..=0xFE2F)
}

/// Whether a cluster is a space justification stretches.
fn is_space(cluster: &str) -> bool {
    matches!(cluster, " " | "\u{a0}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn para(text: &str) -> Para {
        let style = Arc::new(Style::default());
        let mut para = Para::new(style.clone());
        para.push(text, style, None);
        para
    }

    fn shaper() -> Shaper<'static> {
        let book: &'static FontBook = Box::leak(Box::new(FontBook::load(true, &[])));
        Shaper::new(book)
    }

    const LOREM: &str = "The quick brown fox jumps over the lazy dog and keeps running \
                         across the field until the evening light fades behind the hills.";

    #[test]
    fn lines_break_at_spaces_and_fit_the_width() {
        let mut shaper = shaper();
        let lines = shaper.lines(&para(LOREM), 200.0);
        assert!(lines.len() >= 3, "{}", lines.len());
        let mut rebuilt = String::new();
        for line in &lines {
            assert!(line.width() <= 200.0 + EPSILON, "{}", line.width());
            let text = line.visual_text();
            assert!(!text.starts_with(' ') && !text.ends_with(' '), "{text:?}");
            if !rebuilt.is_empty() {
                rebuilt.push(' ');
            }
            rebuilt.push_str(&text);
        }
        assert_eq!(rebuilt, LOREM);
        // First fit: the next word would not have fit on any line.
        for pair in lines.windows(2) {
            let next_word = pair[1].visual_text();
            let next_word = next_word.split(' ').next().unwrap();
            let widened = shaper.lines(
                &para(&format!("{} {next_word}", pair[0].visual_text())),
                1000.0,
            );
            assert!(widened[0].width() > 200.0, "{next_word}");
        }
    }

    #[test]
    fn a_word_wider_than_the_line_breaks_between_letters() {
        let mut shaper = shaper();
        let lines = shaper.lines(&para("Supercalifragilisticexpialidocious"), 60.0);
        assert!(lines.len() > 2, "{}", lines.len());
        for line in &lines {
            assert!(line.width() <= 60.0 + EPSILON);
        }
        let joined: String = lines.iter().map(Line::visual_text).collect();
        assert_eq!(joined, "Supercalifragilisticexpialidocious");
        // Narrower than one letter: one letter a line, no endless loop.
        let lines = shaper.lines(&para("Wide"), 1.0);
        assert_eq!(lines.len(), 4);
    }

    #[test]
    fn forced_breaks_end_lines() {
        let mut shaper = shaper();
        let lines = shaper.lines(&para("First line\nSecond line\n\nAfter a blank"), 500.0);
        let texts: Vec<String> = lines.iter().map(Line::visual_text).collect();
        assert_eq!(
            texts,
            vec!["First line", "Second line", "", "After a blank"]
        );
        assert!(lines[2].height() > 10.0, "an empty line keeps its height");
    }

    #[test]
    fn justified_lines_fill_the_width_except_the_last() {
        let mut shaper = shaper();
        let mut justified = para(LOREM);
        justified.align = Align::Justify;
        let lines = shaper.lines(&justified, 220.0);
        assert!(lines.len() >= 3);
        for line in &lines[..lines.len() - 1] {
            assert!((line.width() - 220.0).abs() < 0.1, "{}", line.width());
            let end = line.runs.last().map(|r| r.x + r.width).unwrap();
            assert!((end - 220.0).abs() < 0.1, "{end}");
        }
        let last = lines.last().unwrap();
        assert!(last.width() < 220.0 - 1.0);
        assert_eq!(last.runs[0].x, 0.0);
    }

    #[test]
    fn alignment_places_lines() {
        let mut shaper = shaper();
        let mut centered = para("Centered");
        centered.align = Align::Center;
        let line = &shaper.lines(&centered, 300.0)[0];
        let left = line.runs[0].x;
        assert!((left - (300.0 - line.width()) / 2.0).abs() < 0.01);
        let mut end = para("At the end");
        end.align = Align::End;
        let line = &shaper.lines(&end, 300.0)[0];
        assert!((line.runs[0].x + line.width() - 300.0).abs() < 0.01);
    }

    #[test]
    fn right_to_left_paragraphs_read_from_the_right() {
        let mut shaper = shaper();
        let mut arabic = para("مرحبا بالعالم 2026");
        arabic.direction = Some(Direction::Rtl);
        let line = &shaper.lines(&arabic, 400.0)[0];
        // Start alignment in RTL: flush right.
        let end = line.runs.last().map(|r| r.x + r.width).unwrap();
        assert!((end - 400.0).abs() < 0.01, "{end}");
        // The first word is the rightmost; the number stays left to right
        // and sits at the left end.
        let first = line.runs.iter().find(|r| r.text.contains("مرحبا")).unwrap();
        let number = line.runs.iter().find(|r| r.text.contains("2026")).unwrap();
        assert!(first.x > number.x, "{line:?}");
        // Right to left: glyphs from the logical end; left to right: from
        // the start.
        assert!(first.glyphs[0].range.start > first.glyphs[1].range.start);
        assert!(number.glyphs[0].range.start < number.glyphs[1].range.start);
        assert_eq!(&number.text[number.glyphs[0].range.clone()], "2");
        // Arabic letters join: fewer glyphs than characters is not
        // required, but the glyphs come right to left.
        let word = first;
        assert!(word.glyphs.first().unwrap().range.start > word.glyphs.last().unwrap().range.start);
    }

    #[test]
    fn mixed_direction_text_in_a_left_to_right_paragraph() {
        let mut shaper = shaper();
        let mut mixed = para("Invoice for שלום עולם today");
        mixed.direction = Some(Direction::Ltr);
        let line = &shaper.lines(&mixed, 500.0)[0];
        let shalom = line
            .runs
            .iter()
            .position(|r| r.text.contains("שלום"))
            .unwrap();
        let olam = line
            .runs
            .iter()
            .position(|r| r.text.contains("עולם"))
            .unwrap();
        // One right-to-left run holding both words, or "עולם" left of "שלום".
        assert!(olam <= shalom, "{line:?}");
        let invoice = line
            .runs
            .iter()
            .position(|r| r.text.contains("Invoice"))
            .unwrap();
        let today = line
            .runs
            .iter()
            .position(|r| r.text.contains("today"))
            .unwrap();
        assert!(invoice < shalom && shalom < today || invoice < olam && olam < today);
    }

    #[test]
    fn characters_fall_back_and_spaces_stay_with_their_words() {
        let mut shaper = shaper();
        let line = &shaper.lines(&para("Invoice 42 — فاتورة رقم — चालान"), 600.0)[0];
        let family = |needle: &str| {
            let run = line.runs.iter().find(|r| r.text.contains(needle)).unwrap();
            shaper.fonts().face(run.face).family.clone()
        };
        assert_eq!(family("Invoice"), "Noto Sans");
        assert_eq!(family("فاتورة"), "Noto Sans Arabic");
        assert_eq!(family("चालान"), "Noto Sans Devanagari");
        // The space between the two Arabic words is shaped with them.
        assert!(
            line.runs.iter().any(|r| r.text.contains("فاتورة رقم")),
            "{line:?}"
        );
    }

    #[test]
    fn measuring_gives_the_longest_word_and_the_whole_line() {
        let mut shaper = shaper();
        let (min, max) = shaper.measure(&para("a considerable amount"));
        let word = shaper.lines(&para("considerable"), 1000.0)[0].width();
        let whole = shaper.lines(&para("a considerable amount"), 1000.0)[0].width();
        assert!((min - word).abs() < 0.01, "{min} {word}");
        assert!((max - whole).abs() < 0.01, "{max} {whole}");
    }

    #[test]
    fn kerning_and_ligatures_are_applied() {
        let mut shaper = shaper();
        let av = shaper.lines(&para("AV"), 1000.0)[0].width();
        let a = shaper.lines(&para("A"), 1000.0)[0].width();
        let v = shaper.lines(&para("V"), 1000.0)[0].width();
        assert!(av < a + v - 0.1, "kerned: {av} < {a} + {v}");
        let office = &shaper.lines(&para("office"), 1000.0)[0];
        assert!(office.runs[0].glyphs.len() < 6, "the ffi ligature");
        let ligature = office.runs[0]
            .glyphs
            .iter()
            .find(|g| g.range.len() > 1)
            .unwrap();
        assert_eq!(&office.runs[0].text[ligature.range.clone()], "ffi");
    }
}
