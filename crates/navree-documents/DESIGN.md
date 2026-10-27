# navree-documents: design notes

## PDF engines

Two engines sit behind `Documents`, both reading the same `.html`
templates (rendered with navree-view's MiniJinja):

- **native** (feature `native`, on by default): lays the document out in
  process and writes the PDF itself. No external program, no network.
  It understands a documented subset of HTML (below) and a typed builder,
  `PdfDocument`.
- **chrome** (feature `chrome`): prints the same HTML in headless Chrome,
  with full CSS.

`documents.engine` (default `native`) picks the engine for a render that
names none, so one template can go to either engine.

## Why a native engine

The engine it replaces compiled Typst templates with `typst`, `typst-pdf`
and `typst-layout` 0.15.1, the latest release. Through them the workspace
carried six crates that osv-scanner reports as unmaintained: `bincode`,
`paste`, `rustybuzz`, `ttf-parser`, `unsafe-libyaml` and `yaml-rust`
(through syntect, hayagriva, biblatex, serde_yaml, fontdb, usvg and
krilla's `simple-text` feature), plus `postcard`, `heapless` 0.7 and
`atomic-polyfill` through `icu_provider_blob`. Typst depends on
`rustybuzz` and `ttf-parser` directly, so no feature selection removes
them. The engine was replaced instead of allow-listing the findings.

## Choosing the stack

Each candidate was built as a scratch crate (`target/scratch/pdf-engine/`
in the worktree), its lockfile scanned with osv-scanner 2.7.0
(`osv-scanner scan -L <Cargo.lock>`), and the same sample drawn with it:
Latin with kerning pairs and ligatures, an invoice line, Arabic, and
Devanagari, in the bundled Noto fonts. The PDFs were rasterized with hayro
0.8.0 (pure Rust; its own lockfile scans clean) and compared.

| Candidate | Packages | osv-scanner | Output |
|---|---|---|---|
| (a) krilla 0.8.2 (`default-features = false`, `raster-images`), harfrust 0.15.0, skrifa 0.49.0, unicode-bidi 0.3.18, icu_segmenter 2.4.0 | 92 | no issues | Kerned Latin (`AV`, `Wa`, `To.`), `fi`/`ffi` ligatures, joined right-to-left Arabic, Devanagari conjuncts and reph placed correctly. Fonts embedded as subset CID fonts with `ToUnicode` maps; tagged PDF supported. |
| (b1) printpdf 0.12.8 without features (`png`, `jpeg`) | 118 | no issues | No shaping: glyphs come from the cmap one character at a time. No kerning, Arabic letters unjoined and in logical (wrong) order, Devanagari with visible viramas and misplaced vowel signs. |
| (b2) printpdf 0.12.8 with `text_layout` only | 137 | no issues | Shaping lives in azul-layout's text3 engine, reachable only through the HTML pipeline; no public shaping API without it. |
| (b3) printpdf 0.12.8 with its default `html` feature | 144 | **RUSTSEC-2025-0141** (`bincode` 1.3.3, unmaintained, through `hyphenation` in azul-layout) | Not usable: the finding is what this work removes. |

Stack (a) has zero findings and is the only one with correct shaping, so
it was chosen. krilla brings its own skrifa 0.42 for subsetting while
harfrust and the engine use skrifa 0.49 / read-fonts 0.46; the two
versions coexist without exchanging types (glyph ids are plain `u32`).

## The native engine

```text
HTML template ──navree-view──▶ HTML ──html.rs──▶ DOM ──convert.rs──▶ PdfDocument
                                                                         │
typed builder (PdfDocument) ─────────────────────────────────────────────┤
                                                                         ▼
          text.rs: style runs → bidi (unicode-bidi) → font fallback → shaping
          (harfrust) → line breaking (icu_segmenter, greedy) → justification
                                                                         ▼
          layout.rs: blocks → pieces (lines, images, table rows) → pages,
          headers and footers with page numbers, tag tree, outline
                                                                         ▼
          write.rs: krilla: subset CID fonts, images, links, tagged PDF,
          metadata
```

- **Fonts** (`fonts.rs`): the bundled Noto Sans (four styles), Noto Serif
  (four styles), Noto Sans Mono (regular, bold), Noto Sans Arabic, Hebrew
  and Devanagari (regular, bold), plus every font file under the
  configured directories. A face is chosen per character: the requested
  families first (closest weight, then style), then the bundled fallback
  chain, then any face that has the character. Spaces, digits and
  punctuation stay in the face of the text around them.
- **Text** (`text.rs`): a paragraph is one string with style ranges.
  Bidi levels come from unicode-bidi with the paragraph's base direction;
  the text is split into items of one style, one level and one face, and
  each item is shaped with harfrust. Lines break at icu_segmenter's
  opportunities, first fit, with a break between grapheme clusters for a
  word wider than the line; each line is reshaped where a break cuts an
  item, reordered visually per UAX #9 and aligned. Justified lines stretch
  their spaces; the last line and lines ending in `<br>` do not.
- **Layout** (`layout.rs`): every block becomes pieces that cannot be
  split (a line, an image, a table row). Pages fill top to bottom; the
  space between blocks collapses and is dropped at the top of a page; a
  heading moves to the next page with the first line after it; tables
  repeat their header rows on every page they continue on, and a row
  taller than a page splits between the lines of its cells. Headers and
  footers are laid out per page once the page count is known, so
  `Page 2 of 5` is exact.
- **PDF** (`write.rs`): krilla writes the pages. Text is drawn glyph run
  by glyph run with the source text, so fonts are embedded as subsets with
  `ToUnicode` maps and `Pdf::text` reads them back in reading order. The
  document is tagged: `H1`–`H6`, `P`, `L`/`LI`/`Lbl`/`LBody`,
  `Table`/`THead`/`TBody`/`TR`/`TH`/`TD`, `Figure` with alt text and
  `Link`; headers, footers, repeated table headers, borders and rules are
  artifacts. Headings h1–h3 become the outline. Title, author and
  language go into the metadata (and XMP).

### The HTML subset

`h1`–`h6`, `p`, `strong`/`b`, `em`/`i`, `u`, `s`/`del`, `a href`,
`span`, `code`, `small`, `br`, `ul`/`ol`/`li` (nested, `start`, `type`),
`table`/`thead`/`tbody`/`tfoot`/`tr`/`th`/`td` (`colspan`, `align`,
`valign`, `width`; `border`, `cellpadding` on the table), `img` (`src`
in the template tree or a `data:` URI, `width`, `height`, `alt`), `hr`,
`pre`, `blockquote`, `div` and the sectioning elements, `header` and
`footer` as children of `body` (running header and footer, with
`pageNumber`, `totalPages`, `title` and `date` spans), `title` and
`meta name="author"`, `lang` and `dir` attributes, a `page-break` class.

Inline `style` attributes: `font-weight`, `font-style`, `color`,
`font-size`, `font-family`, `text-align`, `text-decoration`,
`line-height`, `direction`, `background-color` (table cells and rows),
`width`/`height` (images, cells), `break-before`/`break-after` and the
`page-break-*` forms. In `<style>`, only `@page { size; margin }` is
read. Everything else is ignored, not an error: the same template prints
with full CSS in Chrome.

### What it does not do

No CSS selectors, floats, flex or grid; no `rowspan`; no inline images in
running text (an `img` inside a paragraph becomes its own block); no
hyphenation; no dictionary line breaking for Thai, Lao, Khmer or Myanmar;
no synthetic bold or italic (Arabic, Hebrew and Devanagari have no italic
face and print upright); no remote images.

## Tests

- Unit tests next to the code: the HTML tokenizer and tree builder, the
  CSS values, line breaking, justification, bidi order, table splitting,
  header and footer placement, page numbering.
- `tests/documents/native.rs`: the conformance suite, PDF assertions
  (page count, fonts embedded and subset, text in reading order,
  metadata, tags), the HTML subset, and the samples (an invoice, a long
  report, Arabic/Hebrew/Hindi). With `NAVREE_DOCUMENTS_SAMPLES=<dir>` the
  samples are written there too.
