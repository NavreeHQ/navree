---
name: navree-sheets
description: "CSV and Excel imports and exports with navree-sheets: the `sheets` facade feature (in `full`), `SheetsModule` and the `sheets` config section (`sheets.csv`, `sheets.exports`, `sheets.imports`), export rows with `#[derive(Exportable)]` (`sheet`, `header`, `format`, `width`, `with`, `skip`), `Cell` and `IntoCell`, `Download::csv`/`csv_stream`/`xlsx` (byte-order mark, formula escaping, typed XLSX cells), the `Exports` service, translated headers (`HeaderCatalog`, `HeaderTranslator`, Fluent with `i18n`), named exports (`Export`, `m.export::<E>()`, `GET`/`POST /api/exports/{name}`, queued to a disk with a signed link through an operation), import rows with `#[derive(Deserialize, Validate, Importable)]` (`header`, `aliases`, `required`, `optional`, `skip`), importers (`Import`, `m.importer::<I>()`, `key`, `existing`, `insert`, `update`, `completed`, `RowRejected`), uploads (`POST /api/imports`, `Imports::upload`, `Upload`, `UploadOptions`), encoding detection (UTF-16, Windows-1256, Shift_JIS), CSV, TSV, XLSX, XLS and ODS reading, header mapping (automatic, saved, chosen; `needs_mapping`, `PUT /api/imports/{id}/mapping`), the staging preview (`import_rows`, `GET /api/imports/{id}/rows`), `DuplicateMode` skip/update/fail, batched apply with progress, error files, templates, `sheets:prune`, and tests with the memory queue and disk. Use when adding a CSV or Excel export or a download button, queueing a big export, importing users, contacts or any records from a spreadsheet, building a column-mapping or import-preview screen, handling Arabic or other non-UTF-8 files from Excel, or debugging garbled characters, an import stuck in `needs_mapping`, a 409 on apply, or rows that were skipped."
---

# Imports and exports

`navree::sheets` (crate `navree-sheets`, facade feature `sheets`, in `full`)
writes CSV and XLSX from typed rows and runs import pipelines: upload,
encoding detection, reading, header mapping, validation per row, a
staging table with a preview, batched apply with duplicate modes, and an
error file. Everything below is in the code: when in doubt read
`crates/navree-sheets/src` and its tests in `crates/navree-sheets/tests/sheets/`,
and `crates/navree/tests/sheets_docs.rs` (the docs examples through the
facade). The docs page is
`apps/website/content/docs/packages/imports-and-exports.mdx`.

## Steps: add an export

1. Describe a row. Fields become columns in order:

```rust
use navree::sheets::{Cell, Exportable};

#[derive(Exportable)]
#[export(sheet = "Contacts")]              // the XLSX sheet name (default Sheet1)
struct ContactRow {
    #[export(header = "Name")]             // or a translation key
    name: String,
    #[export(header = "Email", width = 30)]
    email: String,
    company: Option<String>,               // header "Company", None is an empty cell
    #[export(format = "#,##0.00")]         // Excel number format (XLSX)
    balance: f64,
    #[export(with = "cents_to_cell")]      // fn(&i64) -> Cell instead of IntoCell
    amount_cents: i64,
    #[export(skip)]
    internal_note: String,
}

fn cents_to_cell(cents: &i64) -> Cell {
    Cell::Float(*cents as f64 / 100.0) // shown with two decimals; stored as cents
}
```

   `IntoCell` covers strings, integers, floats, `bool`, chrono dates and
   date-times, `Uuid`, `serde_json::Value` and `Option<T>`; implement it for
   your own types.

2. Return a `Download` from a handler:

```rust,ignore
#[get("/export.csv")]
#[permissions("contacts.export")]
async fn export_csv(&self) -> Result<Download> {
    let contacts = Contact::query().order_by(Contact::EMAIL).get(&*self.db).await?.into_models();
    Ok(self.exports.csv("contacts", contacts.into_iter().map(ContactRow::from)))
}
```

   - `Download::csv(name, rows)`, `Download::xlsx(name, rows)?` (an error
     above 1,048,575 rows) and `Download::csv_stream(name, stream)` use the
     default options and the headers as declared.
   - `Arc<Exports>` (`exports.csv`, `xlsx`, `csv_stream`, `file(format, ..)`)
     applies `sheets.csv` and translates headers in the request's locale.
     Prefer it in apps.
   - Add `SheetsModule::from_config()` to the app root. Downloads need
     nothing else.

3. For big exports, or ones a front end lists and runs by name, implement
   `Export` and register it with `m.export::<ContactsExport>()`:

```rust,ignore
impl Export for ContactsExport {
    const NAME: &'static str = "contacts";       // GET/POST /api/exports/contacts
    type Row = ContactRow;
    type Params = ContactFilter;                 // query string, or `params` of a queued export; () for none

    fn filename(&self, filter: &ContactFilter) -> String { "contacts".into() }

    async fn rows(&self, _ctx: &ExportContext, filter: &ContactFilter, page: ExportPage)
        -> Result<Vec<ContactRow>, BoxError> {
        // ORDER BY something stable; .offset(page.offset).limit(page.limit)
    }
    // Optional: count() refuses early above sheets.exports.max_rows and gives real progress.
    // Optional: permission() (default "{NAME}.export"; None = any signed-in user).
}
```

   `GET /api/exports/contacts?format=xlsx` builds the file page by page;
   `POST /api/exports/contacts {"format":"xlsx","params":{..}}` answers 202
   with an operation (`export.contacts`) whose job writes the file to
   `sheets.exports.disk` and finishes with `ExportResult {url, filename,
   format, rows, bytes, expires_at, disk, path}`: a signed link valid for
   `sheets.exports.link_ttl_secs`. From code: `exports.download::<E>(params,
   ExportRequest::new(SheetFormat::Xlsx).user(id))` and
   `exports.queue::<E>(&uow, params, request)`. Queued exports need
   `DatabaseModule`, `QueueModule` and `StorageModule`.

## Steps: add an import

1. Describe a row: serde reads it, its `#[validate]` rules check it, `Importable` lists its
   columns.

```rust
use navree::sheets::Importable;
use navree::validation::Validate;
use serde::Deserialize;

#[derive(Deserialize, Validate, Importable)]
struct ContactInput {
    #[import(header = "Email", aliases = ["E-mail", "Email address"])]
    #[validate(email)]
    email: String,
    #[validate(length(min = 1, max = 100))]
    name: String,                         // header "Name", required
    #[validate(length(max = 100))]
    company: Option<String>,              // optional: an Option or #[serde(default)]
    #[serde(default)]
    #[validate(skip)]
    vip: bool,
}
```

2. Implement `Import` and register it with `m.importer::<ContactsImport>()`
   (it is a provider of your module, so it can hold `Arc<Db>` and services):

```rust,ignore
impl Import for ContactsImport {
    const NAME: &'static str = "contacts";     // `importer` of uploads; permission "contacts.import"
    type Row = ContactInput;

    fn key(row: &ContactInput) -> Option<String> { Some(row.email.trim().to_lowercase()) }

    async fn existing(&self, _ctx: &ImportContext, keys: &[String]) -> Result<HashSet<String>, BoxError> {
        // SELECT email FROM contacts WHERE email IN (keys): the keys the app already has
    }

    async fn insert(&self, _ctx: &ImportContext, uow: &Uow, row: ContactInput) -> Result<(), BoxError> {
        // Save through `uow` (Contact::create(uow, ..)).
        // Err(RowRejected::new("reason").into()) refuses this row with a message.
    }

    // Optional: update(ctx, uow, row) for DuplicateMode::Update; completed(ctx, import).
}
```

3. Add `DatabaseModule`, `QueueModule`, `StorageModule` and
   `SheetsModule::from_config()` to the app, run the `sheets` migrations
   (`imports`, `import_rows`, `import_mappings`) and a `queue:work` worker.

4. The flow a front end drives (all under `{http.api_prefix}`, signed-in
   users, in the request's workspace):

| Step | Route | Result |
|---|---|---|
| Upload | `POST /imports` multipart `importer`, `file`, optional `encoding`, `sheet`, `mapping` (JSON field → header), `save_mapping` | 202, status `uploaded`, `operation_id` to follow |
| Wait | `GET /operations/{id}/events` then `GET /imports/{id}` | `staged`, or `needs_mapping` with `headers`, `mapping`, `missing` |
| Map | `PUT /imports/{id}/mapping` `{mapping, save}` | 202, staged again; 400 `validation_failed` with errors by field |
| Preview | `GET /imports/{id}/rows?status=invalid&page=1&per_page=50` | rows with `row`, `status`, `values`, `errors` |
| Apply | `POST /imports/{id}/apply` `{"duplicates": "skip" \| "update" \| "fail"}` | 202, `applying`, a batch of `chunk_size` jobs |
| Errors | `GET /imports/{id}/errors` | `<upload name>-errors.csv`: Row, the file's own headers, Errors (404 when every row was saved) |
| Template | `GET /imports/templates/{importer}?format=xlsx` | an empty file with the headers |

   `Imports` has the same calls for code: `upload(&uow, name,
   Upload::new(filename, bytes), UploadOptions::default().owner(id).workspace(ws))`,
   `find`, `list`, `rows(id, &RowQuery)`, `map(&uow, id, Mapping::new().map("email", "Mail"), save)`,
   `apply(&uow, id, DuplicateMode::Skip)`, `discard`, `error_file`,
   `template`, `columns`.

## What staging does

- **Encoding:** a byte-order mark (UTF-8, UTF-16LE/BE), UTF-16 without one,
  valid UTF-8, then chardetng's guess (Windows-1256 for Arabic,
  Windows-1252, Shift_JIS, GBK, ...). The upload's `encoding` wins.
- **Format:** workbooks by content (XLSX, XLSM, XLSB, XLS, ODS), else text;
  dates and times are `Cell::Date`/`DateTime` in every workbook format (ODS
  by its null date), to the millisecond; a number past the largest double
  (`1e999`, `INF`) is the largest double of its sign and `NaN` is refused; an
  error cell is its text (`#Div0`, `#SPILL!`); ODS `text:tab` is `\t`,
  `text:line-break` `\n`, a shape in a cell is not its text;
  the delimiter is sniffed from `,` `;` tab `|`; `.tsv`/`.tab` are tab.
- **Headers:** the first non-empty row; blanks become `Column 3`, repeats
  get ` (2)`. Row numbers match the spreadsheet's.
- **Mapping:** the given one, else a saved one (same importer, workspace and
  header set in any order and case), else automatic (header, field name or
  alias, letters and digits only, lower case).
- **Rows:** cells read as people type them (any script's digits, decimal
  comma, yes/no, ISO dates, enum names in any case), then the rules. Keys that
  repeat in the file are `repeated`; keys `existing` knows are `duplicate`.

## Testing templates

```rust,ignore
let app = TestApp::builder()
    .temp_database()
    .config("queue.driver", "memory")
    .config("storage", json!({ "default": "files", "disks": { "files": { "driver": "memory" } } }))
    .module(DatabaseModule::from_config())
    .module(QueueModule::from_config())
    .module(StorageModule::from_config())
    .module(SheetsModule::from_config())
    .module(ContactsModule)
    .build()
    .await;
let imports = app.get::<Imports>().await;
let db = app.get::<Db>().await;

let uow = db.uow(Some(ADMIN), None).await;
let import = imports
    .upload(&uow, "contacts", Upload::new("contacts.csv", "Email,Name\nada@example.com,Ada\n"),
            UploadOptions::default().owner(ADMIN))
    .await?;
uow.commit().await?;
app.get::<Queue>().await.run_pending().await?;         // runs staging
assert_eq!(imports.find(import.id).await?.status, ImportStatus::Staged);

let uow = db.uow(Some(ADMIN), None).await;
imports.apply(&uow, import.id, DuplicateMode::Update).await?;
uow.commit().await?;
app.get::<Queue>().await.run_pending().await?;         // runs the apply batch and the finish job
assert_eq!(imports.find(import.id).await?.counts.created, 1);
```

- `temp_database()` creates a SQLite file: the test build needs the `sqlite`
  feature in `[dev-dependencies]` (`navree new` writes it).
- Legacy encodings: `encoding_rs::WINDOWS_1256.encode(text).0` for Arabic
  CSV, `[0xFF, 0xFE]` followed by `text.encode_utf16()` little-endian bytes
  for Excel's "Unicode Text".
- Over HTTP, post a `multipart/form-data` body with `importer` and `file`
  (see `contacts_are_uploaded_and_exported_over_the_api` in
  `crates/navree/tests/sheets_docs.rs`); routes need an authenticator and
  the importer's permission (`AccessModule` abilities in tests).
- CSV downloads start with `\u{FEFF}` and end lines with `\r\n`; assert on
  the exact text. XLSX starts with `PK`; read it back with `zip` and
  `quick-xml` (see `crates/navree-sheets/tests/sheets/xlsx.rs`).

## Checks before you finish

- [ ] Every export and importer is registered once (`m.export`, `m.importer`)
      in the module that owns its data.
- [ ] `rows` orders by something stable; `count` is implemented for big
      exports.
- [ ] `insert` and `update` write through the given `uow` (each row has its
      own; a failing row never rolls back another).
- [ ] `key` normalizes the same way `existing` compares (case, spaces).
- [ ] Users have `{name}.import` / `{name}.export`, or `permission()`
      returns `None` on purpose.
- [ ] The `sheets` migrations ran; a `queue:work` worker runs in production;
      `sheets:prune` is scheduled (daily).
- [ ] Tests cover a valid file, an invalid row, a duplicate and the error
      file.

## Gotchas

- Garbled Arabic or accented text in Excel: the file was written without a
  byte-order mark (`sheets.csv.bom = false`) or by hand; use `Download` or
  `Exports`.
- A cell starting with `=`, `+`, `-`, `@`, tab or CR is written with a
  leading `'` (formula injection). Numbers are never escaped. Turn it off
  only for files no spreadsheet opens (`sheets.csv.escape_formulas`).
- `#[serde(rename_all)]` on an import row is a compile error; rename fields
  one by one with `#[serde(rename = "..")]`.
- An import stuck in `needs_mapping`: a required column had no matching
  header. Add `aliases`, or send a mapping with `save: true` so later files
  map themselves.
- 409 `import_has_duplicates` on apply: `fail` refuses while duplicates or
  repeated rows exist. Repeated rows are never applied in any mode.
- 409 `import_not_ready`: apply needs `staged`; mapping needs
  `needs_mapping` or `staged`; delete is refused while `applying`.
- Duplicates are found at staging; a record created before apply is caught
  by the database's unique constraint and the row fails with `insert`'s
  error.
- Staging holds the whole file in memory: `sheets.imports.max_upload_bytes`
  bounds the file, not what a workbook unpacks to (a zip can unpack a
  thousand-fold; the XLSX and XLSB readers keep the shared strings and a
  cell's text whole; an XLS file's workbook stream is copied and held):
  `sheets.imports.max_inflated_bytes` (1 GiB) refuses a workbook (XLSX, XLSB,
  ODS, however detected, even behind another header) with a part that declares more than that unpacked, before reading it, and an XLS file whose workbook stream is larger. Opening can't be interrupted, so it is bounded: everything it keeps counts against one budget of that size, at most 65,535 sheets, 16 MiB for one element, string or XLSB record, 256 levels of nesting, and at most four times the limit unpacked in all; an XLS stream is gone through at most twice, and sheets listed at one place or overlapping make the file unreadable. Shared strings are kept
  trimmed in one text (4 bytes per string besides it) and count against the
  same limit; a declared count of strings reserves nothing;
  the zip reader stops a part at its declared size, so a lie in either
  direction fails. `sheets.imports.max_text_bytes` (256 MiB) bounds the text
  the table keeps, counted per cell (a shared string counts again for each
  cell it is in); a file past it fails the import. For CSV, TSV, XLSX, XLSB
  and ODS it is counted before the text is copied (ODS: every copy of a
  repeated row or cell; spaces of `text:s` before they are made, never the
  ones that end a cell, and none for a row or cell that gives nothing) and XLS
  (whose shared strings also count against it as they are decoded). Rows are padded to the header's width, so
  `sheets.imports.max_columns` (16,384) and `sheets.imports.max_cells`
  (10,000,000, rows times columns) bound the table too; a file past either
  fails the import with a message, and `max_rows` is checked while reading.
  That holds for every format (read a record or cell at a time; a
  workbook's sheet takes memory for its cells, not its area, and an ODS row
  or cell that repeats is read once). A sheet whose cells are not in row
  order (Excel never writes one) is held only up to the limits (`max_rows` + 1 rows,
  `max_cells` cells, cells past `max_columns` as marks): over that it is
  refused, even if the table would have fitted. After a refusal the rest of
  a workbook's sheet is still read through, keeping nothing, until the job's
  timeout stops it. The file is read on the blocking pool, and the reading
  stops when the job's timeout drops it: a workbook's sheet within 64 KiB of
  its part (an XLS sheet within 1,024 records) whether or not it gives a cell,
  a text file at its next record.
- The `/api/exports` and `/api/imports` routes mount only when the app has
  an authenticator and `sheets.*.routes` is true.
- With the `i18n` feature, headers come from Fluent (`contacts.email` is the
  `email` attribute of message `contacts`) and queued work runs in the
  locale that queued it.
