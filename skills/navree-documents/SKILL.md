---
name: navree-documents
description: "PDF and Word documents with navree-documents: the opt-in `documents` facade feature, `DocumentsModule` and the `documents` config section, `Documents::pdf(name, &data)` and `render(&Render)`, `.html` templates (navree-view syntax, `locale`/`dir` variables) for two engines: the native engine (default, in process: the HTML subset it reads, inline styles, `@page`, `header`/`footer` elements with `pageNumber`/`totalPages` spans, tables with repeating headers, images from the template tree, bundled Noto Sans/Serif/Mono, Arabic, Hebrew and Devanagari fonts, font dirs, tagged PDF) and headless Chrome (full CSS, assets from the template tree, `/_navree/fonts.css`, blocked network, the page pool, `documents.chrome.path`/`endpoint`, `NAVREE_BROWSER_CHROME`), `PrintOptions` for both, `PdfDocument` built in code and `render_document`, engine choice (`documents.engine`, `Render::engine`), right-to-left text, `Pdf` responses and `download`/`inline`, `Pdf::merge`, `stamp`, `overlay`, `text()` in tests, `Docx`, `DocxFile::from_bytes` and `DocxLimits`, `save`, `queue` with `QueuedDocument` and `status`, `queue_operation` (a queued export behind a navree-queue operation users follow, `RENDER_FAILED_MESSAGE`), the `documents.render` job, its progress and its `.failed` record, `m.document_files`, the `documents:render` and `documents:fonts` commands, and `DocumentsFake`. Use when generating certificates, invoices, reports or exports as PDF or DOCX, adding a document template, building a PDF in code, rendering Arabic, Hebrew or other right-to-left documents, stamping or merging PDFs, queueing a long export, testing a document's text, or debugging TemplateNotFound, a template error, a 503 from an engine, CSS the native engine ignores, or tofu boxes instead of letters."
---

# Documents

`navree::documents` (crate `navree-documents`, facade feature `documents`,
**opt-in**: not part of `full`) renders PDF from `.html` templates and data
with two engines, builds PDFs in code, writes DOCX, and edits PDFs. Everything
below is in the code: when in doubt read `crates/navree-documents/src` (the
native engine is `src/native/`, its design notes `DESIGN.md`) and its tests in
`crates/navree-documents/tests/documents/`, and
`crates/navree/tests/documents.rs` (the docs examples through the facade).
The docs page is `apps/website/content/docs/packages/documents.mdx`.

## Steps: add documents to an app

1. Turn the feature on (on top of the default `full`, which leaves it out):

```toml
[dependencies]
navree = { git = "https://github.com/NavreeHQ/navree", features = ["documents"] }
```

   This brings `navree::documents` and puts `DocumentsModule` and
   `ModuleBuilderDocumentsExt` in the prelude.

2. Add the module to the app root. `StorageModule` is needed for `save` and
   `queue`; `QueueModule` makes `queue` run on a worker (without it `queue`
   renders at once):

```rust
Navree::app()
    .module(ConfigModule::for_root(sources))
    .module(HttpModule::from_config())
    .module(StorageModule::from_config())
    .module(QueueModule::from_config())
    .module(DocumentsModule::from_config()) // section `documents`; or DocumentsModule::for_root(DocumentsOptions { .. })
```

3. Write a template under `resources/documents/` (the default
   `documents.templates`): `certificates/course.html`. It renders with the
   native engine by default (in process, nothing to install). For CSS the
   native engine doesn't read, render it with Chrome instead (see Engine
   choice); Chrome or Chromium must be installed, or reachable at
   `documents.chrome.endpoint`.

4. Render from a handler or service with `Arc<Documents>`:

```rust
use navree::documents::{Documents, Download};

#[injectable]
pub struct CertificatesController {
    documents: Arc<Documents>,
}

#[controller("/certificates", version = "1")]
impl CertificatesController {
    #[get("/{name}")]
    #[authenticated]
    async fn show(&self, Path(name): Path<String>) -> Result<Download> {
        let completion = json!({ "name": name, "course": "Security awareness", "completed_on": "27 October 2026" });
        let pdf = self.documents.pdf("certificates/course", &completion).await?;
        Ok(pdf.download("certificate.pdf")) // or return `pdf` itself (inline)
    }
}
```

5. Test it by its text (see Testing below), then preview by hand:
   `navree documents:render certificates/course --data sample.json --locale ar`.

## Templates (both engines)

```html
<!doctype html>
<html lang="{{ locale }}" dir="{{ dir }}">
<head>
  <title>Certificate</title>
  <link rel="stylesheet" href="/_navree/fonts.css">   <!-- Chrome: the bundled fonts -->
  <style>@page { size: A4 landscape; margin: 20mm; }</style>
</head>
<body style="text-align: center">
  <h1>Certificate of completion</h1>
  <p><strong>{{ name }}</strong> completed {{ course }} on {{ completed_on }}.</p>
  <footer><p>Page <span class="pageNumber"></span> of <span class="totalPages"></span></p></footer>
</body>
</html>
```

- navree-view syntax (Jinja): escaped values, undefined variables are
  errors, `money` and `date` filters, `{% extends "layouts/document.html" %}`
  with full paths. A syntax error in any `.html` template fails every render
  until fixed (the error names the file and line).
- Variables: the data's fields plus `locale` (default `en`) and `dir`
  (`rtl`/`ltr`), unless the data has those keys.
- `Render::html(html)` renders inline HTML (no template engine).

## The native engine (default)

Lays the HTML out in process (HarfRust shaping, unicode-bidi, icu_segmenter
line breaking, krilla PDF). It reads this subset and skips everything else:

- Blocks: `h1`–`h6` (h1–h3 make the outline), `p`, `div`/sectioning,
  `blockquote`, `pre` (monospace, spaces kept), `hr`, `ul`/`ol`/`li` (nested;
  `start`, `type` 1/a/A/i/I), `table` (below), `img` (own line).
- Inline: `strong`/`b`, `em`/`i`, `u`, `s`/`del`, `small`, `code`, `span`,
  `a href` (blue, underlined, clickable), `br`.
- `style` attribute: `font-weight`, `font-style`, `color`, `font-size`,
  `font-family` (`serif` = Noto Serif, `monospace` = Noto Sans Mono),
  `text-align`, `text-decoration`, `line-height`, `direction`; cells and rows
  also `background-color`, `width`, `vertical-align`; images `width`/`height`.
  Attributes `align`, `valign`, `width`, `bgcolor`, `dir`, `lang`.
  `html`/`body` styles are the document's base.
- From `<style>` only `@page { size; margin }` (`A3/A4/A5/letter/legal` or two
  lengths, `landscape`/`portrait`). No selectors, margins/padding, floats,
  flex, grid, positioning; no `rowspan`; no hyphenation.
- Tables: `thead` rows (or a first row of `th`) are bold on a gray band and
  repeat on every page; rows don't split unless taller than a page; a line
  under each row by default, `border="1"` grid, `border="0"` or
  `role="presentation"` none; `cellpadding` (px), `colspan`, `width="30%"`
  on cells or `col`. RTL documents put the first column on the right.
- Images: PNG/JPEG/GIF/WebP from the template tree (relative to the
  document, `/` from the root) or `data:` URIs; other origins never fetched;
  missing ones skipped; 96 dpi unless sized; `alt` = figure alt text.
- Page breaks: `class="page-break"`, `break-after: page`, `break-before: page`.
- `header`/`footer` children of `body` print on every page (9pt gray unless
  styled), with `pageNumber`, `totalPages`, `title`, `date` spans.
  `PrintOptions::header/footer` HTML works the same.
- Pages: `Render::print(PrintOptions)` else `documents.native.print` (A4,
  10 mm); `@page` wins while `prefer_css_page_size`.
- Fonts: bundled Noto Sans, Serif, Mono, Sans Arabic, Hebrew, Devanagari;
  each character falls back to a family that has it. More from
  `documents.native.font_dirs`; `navree documents:fonts` lists families.
  `bundled_fonts = false` with no font dir → `EngineUnavailable`.
- Output: subset fonts with ToUnicode (extractable text), tagged PDF
  (H/P/L/Table/TH/TD/Figure/Link; headers, footers, repeats are artifacts),
  outline, title/author/lang metadata, creation date from the `dyn Clock`.
- Over `documents.native.timeout_secs` (60): `DocumentsError::Timeout` (503).
  A template-tree image that doesn't decode: `Template { file: "images/x.png" }`.
- `documents.native.max_image_pixels` (25,000,000; 0 fails the boot, from config
  or `DocumentsModule::for_root`): a PNG,
  GIF (screen and first frame) or WebP whose header declares more pixels is
  `DocumentsError::TooLarge` (413) before it is decoded; the message names the
  limit, the file goes to the log. JPEGs are embedded undecoded and not counted.
  The GIF decoder's own 50 MB frame limit (about 12.5 M pixels) still applies.

## Building a PDF in code

```rust
use navree::documents::native::{Align, Cell, List, Paragraph, Run, Table};
use navree::documents::PdfDocument;

let document = PdfDocument::new()
    .title("Quarterly audit report")      // metadata + Run::title()
    .locale("ar")                         // language; RTL paragraphs/lists/tables
    .header(Paragraph::new().run(Run::title()).align(Align::End))
    .footer_page_numbers()                // "Page N of M"
    .heading(1, "Quarterly audit report")
    .paragraph(Paragraph::new().text("Prepared for ").bold("Acme Ltd").link("site", "https://acme.example"))
    .list(List::numbered().item("Rotate keys").item_with("Restore", List::bullets().item("Database")))
    .table(Table::new().header(["Control", "Status"])
        .row([Cell::new("MFA"), Cell::new("Passed").align(Align::End)])
        .widths([3.0, 1.0]))
    .image(navree::documents::native::Image::new(png_bytes).width_mm(40.0).alt("Logo"))
    .page_break();
let pdf = self.documents.render_document(&document).await?; // configured fonts + app clock
let pdf = document.build()?;                                  // outside an app: bundled fonts
```

`NativeEngine::new(templates)` (`.font_dirs`, `.print`, `.clock`, `.timeout`)
renders templates (`render`, `render_html`) and documents (`render_document`).

## Chrome

- Full CSS. Choose per render with `.engine(EngineKind::Chrome)`, or for every
  render with `documents.engine = "chrome"`.
- The page loads from `https://documents.navree.invalid/<path>`:
  `<img src="images/logo.png">` and `<link href="/styles/doc.css">` come from
  the template tree; `<link rel="stylesheet" href="/_navree/fonts.css">` gives
  the bundled Noto fonts (`font-family: "Noto Sans", "Noto Sans Arabic",
  "Noto Sans Hebrew", "Noto Sans Devanagari"`); every other origin is
  **blocked** unless `documents.chrome.allow_network = true`.
- Page settings: `PrintOptions` (render's, else `documents.chrome.print`); CSS
  `@page` wins while `prefer_css_page_size`. Header/footer templates use the
  classes `pageNumber`, `totalPages`, `date`, `title`, `url`, styled inline.
- The browser: `documents.chrome.path`, else `NAVREE_BROWSER_CHROME`, else the
  usual install paths; `endpoint` (`ws://..` or `http://host:9222`) uses a
  running one. One browser, a fresh context per document, at most `pool_size`
  (2) pages at once, `timeout_secs` (30) per render, restarted after a crash,
  closed at shutdown. Root in a container: `no_sandbox = true`.
- `ChromeEngine::render_html(&render)` returns the HTML without a browser.

## Engine choice and errors

- `Render::engine(..)` wins; else `documents.engine` (default `native`;
  `chrome` in a build without the `native` feature).
  `documents.engine_for(&render)` tells.
- `TemplateNotFound { name, tried }` names `<name>.html`.
- `Template { file, line, column, message }`: `certificates/course.html:3: undefined value`.
- Returned with `?`: 500 (`EngineUnavailable` and `Timeout`: 503); the body
  never contains the template's details. `Pdf(..)` and `InvalidDocx(..)` (bytes
  that are not a readable PDF or Word file) are a 400 and `TooLarge(..)` (an
  image, PDF or Word file over a limit) a 413; all send their message.
  Writing an edited PDF failing is `Engine { engine: "pdf" }`, and an edit the
  app should not ask for (merging nothing, a stamp Helvetica can't show) is
  `Edit(..)`: both 500.

## Editing PDFs and reading them

```rust
use navree::documents::{Pages, Pdf, Position, Render, Stamp};

let both = Pdf::merge([&invoice, &terms])?;                          // page by page
let paid = both.stamp(&Stamp::text("PAID").color(0.0, 0.5, 0.0))?;  // Latin-1, Helvetica Bold
let copy = paid.stamp(&Stamp::text("Copy 1 of 2").at(Position::TopRight).pages(Pages::First))?;
let seal = documents.render(&Render::html(r#"<p style="text-align: right">مدفوع — PAID</p>"#)).await?;
let sealed = copy.overlay(&seal, Pages::Last)?;  // any script
let text = sealed.text()?;        // pages joined by '\u{c}', RTL in reading order
let pages = sealed.page_texts()?; // one string per page
```

`text()` and `page_texts()` allow a page 128 MiB of work and the document
1 GiB: content decoded (a form again each time it is drawn, plus 1 KiB per
draw), every operation (4 bytes; 16 more to show text; 32 for `Do`), every
character code (12), every object looked up (16, however many references),
everything a filter reads and writes whether the content can be used or not, 1
KiB per filter, fonts read (1 KiB each, `ToUnicode` inflated, up to 8 MiB, plus
1/16 of the memory reading it builds; 16 bytes per width looked at; 8 per
`Differences` item and per byte of a glyph name), and marked content's actual
text each time it is decoded. The time is in proportion: all of it takes about
22 s on one core on the slowest shape found. Reading text (here and in navree-media's `text::extract`, which
uses the same code), a font's `ToUnicode` map comes before its `Encoding`, and
its one reader refuses it past 64 codespace ranges, 131,072 lines, 256 texts in
one list or 65,792 in all lists. Glyph names may be `uniXXXX` or `uXXXXXX`; one
that cannot be read, or over 127 bytes, loses one code; a map's text over 512
bytes is skipped. Fonts and maps are kept per document in 256
MiB, counted (~256 B per code, two copies
of a range's text per split it can have, 160 B per code `Differences` names, 24
B per width; 17 MB for a full two-byte map, fifteen fit), plus at most one font's
widths; a map is read once for the fonts sharing it; past 256 MiB the least
recently used is dropped and read again; one map or font over 256 MiB is
refused. A page may show 1,000,000 character codes, produce 8 MiB of text (the document
64 MiB), nest `q` and marked content 256 deep, and spend 50,000,000 mark
comparisons (a byte of text compared is one). Over any:
`DocumentsError::TooLarge` (413). These are fixed, not config keys.

Reading a PDF (navree-pdf, under every `Pdf` method) is lazy and budgeted per
document: 256 MiB of objects as they are built (object and cross-reference
streams inflate to 8 MiB each; a bigger one is skipped with its objects, so
`Pdf::from_bytes` refuses a PDF whose page tree is in it), of which the process
holds at most twice that, and 1 GiB of bytes looked at, including what a failed
parse reached, what each filter (8 at most on a stream) reads and writes, and 16
bytes per object looked up; a chain of references stops at 8.
Objects sharing their bytes, deep or wide nesting (100 levels) and
cross-references that rescan the same bytes are all counted: a small file that
would take gigabytes is `TooLarge` (413). `page_count`, `text`, `title` read
what they need; `merge`, `stamp`, `overlay`, `with_metadata` read everything,
write at most the files' bytes plus 256 MiB each and add what is counted as 256
MiB, about 100,000 pages stamped (`TooLarge` past that, before it is held), and
refuse a file when an object its page tree, contents or resources lead to cannot
be read (`DocumentsError::Pdf`); another that cannot be read is dropped, and an
empty object reads as null.
A stream ends before the next object starts, whatever its `Length` says.
Broken cross-references are rebuilt; files encrypted without a password to open
(RC4, AES 40 to 256 bits) are read and edited unencrypted; one needing a
password is `DocumentsError::Pdf`.
`stamp`, `overlay` and `merge` keep shared or inherited resources one object,
and place what they draw on what the page shows (`MediaBox` cut to `CropBox`).

## Word documents

```rust
use navree::documents::Docx;
use navree::documents::docx::{Align, Image, Paragraph, Run, Table};

let file = Docx::new()
    .title("Quarterly audit report")
    .locale("ar")                       // RTL paragraphs and tables
    .heading(1, "تقرير التدقيق")
    .paragraph(Paragraph::new().text("Prepared for ").bold("Acme Ltd"))
    .bullets(["one", "two"])
    .numbered(["first", "second"])
    .table(Table::new().header(["Control", "Status"]).row(["MFA", "Passed"]))
    .image(Image::new(png_bytes).width_mm(40.0))
    .page_break()
    .footer_page_numbers()
    .build()?;                          // DocxFile
Ok(file.download("audit.docx"))
```

`file.text()` gives one line per paragraph, cells tab-separated, `[image]` for
images; use it in tests. The text of hyperlinks, inserted runs and content
controls is part of its paragraph; deleted text, headers, footers, notes and
text boxes are not.

`DocxFile::from_bytes(bytes)` reads a Word file within `DocxLimits`: 10,000
entries, 64 MiB for a part, 128 MiB for all the parts read, 64 MiB of text,
256 levels of elements. `from_bytes_within(bytes, &limits)` and
`text_within(&limits)` take your own. Over a limit is `TooLarge` (413); bytes
that are not a Word document that can be read are `InvalidDocx` (a 400, like an
unreadable PDF's `Pdf`; `Docx` is for writing one failing, a 500). Reading is
synchronous CPU work, seconds at the limits: run a client's file with
`spawn_blocking`.

## Saving and queueing

```rust
let stored = documents.save(&render, "reports/q3.pdf").await?;          // documents.disk or the default disk
let link = stored.temporary_url(Utc::now() + Duration::hours(1)).await?;

let queued = documents.queue(render, "exports/audit-q3.pdf").await?;    // QueuedDocument {id, disk, path, job_id}
match documents.status(&queued).await? {
    DocumentStatus::Pending => {}
    DocumentStatus::Ready { size } => {}
    DocumentStatus::Failed { error } => {} // log `error`; show your own message
}
```

- Job `documents.render` (`RenderDocument`), dispatched through `dyn
  Dispatcher` (queued only if the request's transaction commits). Engine,
  timeout and disk errors retry (3 attempts, 300 s each); template and data
  errors are recorded at once without retrying.
- Failures are written to `<path>.failed` on the same disk; a later success
  deletes it.
- The job reports progress (`ctx.progress`: "Rendering the document",
  "Saving the document") and stops when its operation or batch is cancelled.

### Behind an operation (what users follow)

```rust
// needs QueueModule + DatabaseModule; facade: `documents` (opt-in) + `queue` (in `full`)
#[post("/audit")]
#[authenticated]
async fn export(&self, uow: Uow, CurrentPrincipal(me): CurrentPrincipal, Json(input): Json<ExportInput>)
    -> Result<Json<OperationRecord>> {
    let render = Render::template("reports/audit").data(json!({ "quarter": input.quarter }));
    let op = self.documents
        .queue_operation(&self.operations, &uow, Operation::new("audit.export").owner(me.user_id),
                         render, format!("exports/audit-{}.pdf", input.quarter))
        .await?;
    Ok(Json(op)) // client follows GET /api/operations/{id} (or /events, or channel operations.{id})
}
```

- Succeeded: `result = {"disk", "path", "size"}`; serve the file from your
  own route (or a `temporary_url`).
- Template/data error: `failed` at once with `RENDER_FAILED_MESSAGE` ("The
  document could not be rendered."), no retries; details in logs and
  `<path>.failed`. Engine/disk errors retry, then the queue fails it.
- Cancel (`DELETE /api/operations/{id}`) stops it before saving.
- Bad path: `DocumentsError::Storage`, no operation started.
- On `navree-documents` directly this is the crate feature `operations`.

## Templates shipped by a module

```rust
#[module(controllers = [CertificatesController], register = Self::wire)]
pub struct CertificatesModule;

impl CertificatesModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.document_files([
            ("certificates/course.html", include_bytes!("documents/course.html").as_slice()),
            ("images/seal.png", include_bytes!("documents/seal.png").as_slice()),
        ]);
    }
}
```

A file at the same path in a template directory overrides the embedded one.

## Testing

Assert on the text; the native engine renders fast enough for unit and HTTP
tests:

```rust
let response = app.client().get("/api/v1/certificates/Ada").send().await.assert_status(200);
let pdf = Pdf::from_bytes(response.bytes().clone()).unwrap();
assert!(pdf.text().unwrap().contains("Ada"));
```

For RTL output compare `pdf.text()` with a golden file. When the document is
not under test, install the fake:

```rust
use navree::documents::DocumentsFake;

let fake = DocumentsFake::new(); // with_templates(templates) to require existing names
let app = TestApp::builder()
    .module(DocumentsModule::from_config())
    .module(CertificatesModule)
    .override_value(Arc::new(fake.documents()))
    .build()
    .await;
// ... exercise the app ...
fake.assert_rendered_with("certificates/course", |render| render.data["name"] == "Ada");
fake.assert_queued("reports/audit");     // queue() records, does not render
// queue_operation() starts the operation (stays pending) and records the render;
// fake.queued()[0].document.operation_id == Some(op.id)
let fake = DocumentsFake::new().fail("reports/audit", "broken"); // a failing template
```

The fake's PDF text is `"<template>\nkey: value\n..."` (Latin-1);
`render_document` on the fake's `Documents` lays out with the bundled fonts.
Chrome tests in the crate run against a mock DevTools endpoint; real-browser
tests read `NAVREE_BROWSER_CHROME`.

## Checks before you finish

- [ ] `features = ["documents"]` on the `navree` dependency.
- [ ] `DocumentsModule` in the app root; `StorageModule` when you `save` or `queue`.
- [ ] Templates under a `documents.templates` directory (or embedded with `m.document_files`).
- [ ] Templates print every variable they use (undefined is an error).
- [ ] A native-rendered template uses only the HTML subset (or its render names Chrome).
- [ ] Chrome templates link `/_navree/fonts.css` when they print Arabic, Hebrew or Devanagari; Chrome installed in the production image (or `documents.chrome.endpoint`), `no_sandbox` when it runs as root.
- [ ] Handlers return `pdf.download(name)` (or the `Pdf`); queued exports return the `OperationRecord` from `queue_operation` (or the `QueuedDocument` from `queue`, polled with `status`).
- [ ] A test asserts on the document's text (or a `DocumentsFake` records the render).

## Gotchas

- A native render looks unstyled: the engine reads inline `style` attributes
  and `@page`, not stylesheet rules or classes. Inline the styles that matter,
  or render that document with Chrome.
- A table used for layout (logo beside an address) shows row lines: give it
  `border="0"` or `role="presentation"`.
- Tofu boxes in Chrome output: the page uses system fonts; link
  `/_navree/fonts.css` and set the Noto families. In the native engine every
  bundled script falls back on its own; another script needs a font in
  `documents.native.font_dirs`.
- An image missing from a native PDF: it is not in the template tree or is on
  another origin (never fetched); in Chrome, blocked or a 404. Check the logs
  at debug level (`navree_documents::native`, `navree_documents::chrome`).
- `Stamp::text` with non-Latin-1 text is an error: render the stamp with the
  native engine and `overlay` it.
- `save`/`queue` without `StorageModule`: `DocumentsError::Queue` naming
  `StorageModule`. A path with `..` is `DocumentsError::Storage` before
  anything is queued.
- `documents.disk` naming a disk `storage.disks` lacks fails the boot.
- The `documents` feature is opt-in: `navree::documents` is missing without it.
