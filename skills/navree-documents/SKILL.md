---
name: navree-documents
description: "PDF and Word documents with navree-documents: the opt-in `documents` facade feature, `DocumentsModule` and the `documents` config section, `Documents::pdf(name, &data)` and `render(&Render)`, Typst templates (`.typ`, data at `/data.json`, `sys.inputs` locale and dir, bundled Noto fonts for Arabic and Devanagari, font dirs, local packages) and HTML templates printed by headless Chrome (navree-view syntax, `locale`/`dir` variables, assets from the template tree, `/_navree/fonts.css`, blocked network, `PrintOptions`, the page pool, `documents.chrome.path`/`endpoint`, `NAVREE_BROWSER_CHROME`), engine choice, right-to-left text, `Pdf` responses and `download`/`inline`, `Pdf::merge`, `stamp`, `overlay`, `text()` in tests, `Docx` (headings, paragraphs, lists, tables, images, page numbers, RTL), `save`, `queue` with `QueuedDocument` and `status`, the `documents.render` job and its `.failed` record, `m.document_files`, the `documents:render` and `documents:fonts` commands, and `DocumentsFake`. Use when generating certificates, invoices, reports or exports as PDF or DOCX, adding a document template, rendering Arabic or other right-to-left documents, stamping or merging PDFs, queueing a long export, testing a document's text, or debugging TemplateNotFound, a template error, a 503 from the Chrome engine, or tofu boxes instead of letters."
---

# Documents

`navree::documents` (crate `navree-documents`, facade feature `documents`,
**opt-in**: not part of `full`) renders PDF from templates and data with two
engines, writes DOCX, and edits PDFs. Everything below is in the code: when in
doubt read `crates/navree-documents/src` and its tests in
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
   `documents.templates`). Pick the engine by the extension:
   - `certificates/course.typ`: Typst, compiled in process (the default
     choice; nothing to install).
   - `invoices/show.html`: HTML printed by headless Chrome (Chrome or
     Chromium must be installed, or reachable at `documents.chrome.endpoint`).

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
   `navree documents:render certificates/course --data resources/documents/data.json --locale ar`.

## Typst templates

```text
#let data = json("/data.json")                 // the render's data
#let dir = sys.inputs.at("dir", default: "ltr") // "rtl" with an RTL locale
#set page(paper: "a4", flipped: true, margin: 2cm)

#align(center)[
  #text(size: 28pt, weight: "bold")[Certificate of completion]
  #data.name completed #data.course on #data.completed_on.
]
```

- Data is `json("/data.json")`. A sample `data.json` at the root of the
  template directory makes the Typst CLI/editor preview work; renders replace it.
- `#import`, `#include`, `image`, `read`, `json` resolve in the template tree:
  `/`-paths from its root, others relative to the file. No file outside it.
- `.locale("ar")` sets `text.lang`, the direction, and `sys.inputs.locale` /
  `sys.inputs.dir`.
- Fonts: text falls back through Noto Sans, Noto Sans Arabic, Noto Sans
  Devanagari (bundled). More from `documents.typst.font_dirs`;
  `navree documents:fonts` lists families. `bundled_fonts = false` drops them.
- Packages: `#import "@preview/x:1.0.0"` reads
  `<documents.typst.packages_dir>/preview/x/1.0.0/`; nothing downloads.
- `datetime.today()` follows the app's `dyn Clock`.
- A render over `documents.typst.timeout_secs` (60) is `DocumentsError::Timeout` (503).

## HTML templates (Chrome)

- navree-view syntax (MiniJinja): escaped values, undefined variables are
  errors, `money` and `date` filters, `{% extends "layouts/document.html" %}`
  with full paths.
- Variables: the data's fields plus `locale` (default `en`) and `dir`
  (`rtl`/`ltr`), unless the data has those keys. Write
  `<html lang="{{ locale }}" dir="{{ dir }}">`.
- The page loads from `https://documents.navree.invalid/<path>`:
  `<img src="images/logo.png">` and `<link href="/styles/doc.css">` come from
  the template tree; `<link rel="stylesheet" href="/_navree/fonts.css">` gives
  the bundled Noto fonts (`font-family: "Noto Sans", "Noto Sans Arabic",
  "Noto Sans Devanagari"`); every other origin is **blocked** unless
  `documents.chrome.allow_network = true`.
- Page settings: `Render::print(PrintOptions::a4().landscape().margin(Margins::all(15.0)).footer(..))`,
  else `documents.chrome.print`; CSS `@page { size }` wins while
  `prefer_css_page_size`. Header/footer templates use the classes `pageNumber`,
  `totalPages`, `date`, `title`, `url`, styled inline.
- The browser: `documents.chrome.path`, else `NAVREE_BROWSER_CHROME`, else the
  usual install paths; `endpoint` (`ws://..` or `http://host:9222`) uses a
  running one. One browser, a fresh context per document, at most `pool_size`
  (2) pages at once, `timeout_secs` (30) per render, restarted after a crash,
  closed at shutdown. Root in a container: `no_sandbox = true`.
- `ChromeEngine::render_html(&render)` returns the HTML without a browser.

## Engine choice and errors

- `Render::engine(..)` wins; else `.typ`/`.html` in the name; else whichever
  file exists; both: `documents.engine` (default `typst`).
  `documents.engine_for(&render)` tells.
- `TemplateNotFound { name, tried }` lists the files tried.
- `Template { file, line, column, message }`: `certificates/course.typ:3:13: unknown variable: nmae`.
- Returned with `?`: 500 (`EngineUnavailable` and `Timeout`: 503); the body
  never contains the template's details.

## Editing PDFs and reading them

```rust
use navree::documents::{Pages, Pdf, Position, Stamp};

let both = Pdf::merge([&invoice, &terms])?;                          // page by page
let paid = both.stamp(&Stamp::text("PAID").color(0.0, 0.5, 0.0))?;  // Latin-1, Helvetica Bold
let copy = paid.stamp(&Stamp::text("Copy 1 of 2").at(Position::TopRight).pages(Pages::First))?;
let sealed = copy.overlay(&seal_rendered_with_typst, Pages::Last)?;  // any script
let text = sealed.text()?;        // pages joined by '\u{c}', RTL in reading order
let pages = sealed.page_texts()?; // one string per page
```

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
images; use it in tests.

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

## Templates shipped by a module

```rust
#[module(controllers = [CertificatesController], register = Self::wire)]
pub struct CertificatesModule;

impl CertificatesModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.document_files([
            ("certificates/course.typ", include_bytes!("documents/course.typ").as_slice()),
            ("images/seal.png", include_bytes!("documents/seal.png").as_slice()),
        ]);
    }
}
```

A file at the same path in a template directory overrides the embedded one.

## Testing

Assert on the text; Typst renders fast enough for unit and HTTP tests:

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
let fake = DocumentsFake::new().fail("reports/audit", "broken"); // a failing template
```

The fake's PDF text is `"<template>\nkey: value\n..."` (Latin-1). Chrome tests
in the crate run against a mock DevTools endpoint; real-browser tests read
`NAVREE_BROWSER_CHROME`.

## Checks before you finish

- [ ] `features = ["documents"]` on the `navree` dependency.
- [ ] `DocumentsModule` in the app root; `StorageModule` when you `save` or `queue`.
- [ ] Templates under a `documents.templates` directory (or embedded with `m.document_files`).
- [ ] HTML templates print every variable they use (undefined is an error) and link `/_navree/fonts.css` when they print Arabic or Devanagari.
- [ ] Chrome installed in the production image (or `documents.chrome.endpoint`), `no_sandbox` when it runs as root.
- [ ] Handlers return `pdf.download(name)` (or the `Pdf`); queued exports return the `QueuedDocument` and poll `status`.
- [ ] A test asserts on the document's text (or a `DocumentsFake` records the render).

## Gotchas

- Tofu boxes in Chrome output: the page uses system fonts; link
  `/_navree/fonts.css` and set the Noto families.
- An image or stylesheet missing in a Chrome PDF: it is on another origin
  (blocked) or not in the template tree (404); check the logs at debug level
  (`navree_documents::chrome`).
- A syntax error in any `.html` template fails every HTML render until fixed
  (the error names the file and line).
- `Stamp::text` with non-Latin-1 text is an error: render the stamp with Typst
  and `overlay` it.
- `save`/`queue` without `StorageModule`: `DocumentsError::Queue` naming
  `StorageModule`. A path with `..` is `DocumentsError::Storage` before
  anything is queued.
- `documents.disk` naming a disk `storage.disks` lacks fails the boot.
- The `documents` feature is opt-in: `navree::documents` is missing without it.
