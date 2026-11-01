# navree-ui

Navree's UI toolkit. Typed Rust components render to HTML on the server, a small
owned runtime handles behaviour, and a page with nothing interactive ships no
JavaScript. No per-tab server state, no WASM, no Node at run time or in
`cargo build`.

```rust
use navree::ui::*;

async fn index(shell: AppShell, uow: Uow, query: TableQuery<Deal>) -> Result<AppPage> {
    let page = query.load(Deal::query(), &uow).await?;
    Ok(shell
        .page("Deals")
        .actions(Button::primary("New deal").icon(icons::PLUS).href("/deals/new"))
        .body(DataTable::new("deals", &query, &page)))
}
```

## What's here

- **The tree.** Every component builds one non-generic `Node`. Variants are Rust
  enums, so a typo can't compile. `render` writes a tree in one pass to one
  `String`, sized ahead of time. Text goes through one escaping path; URLs go
  through `Href`, which refuses `javascript:` and every other unknown scheme.
- **The attribute contract.** `data-slot` says what a part is, `data-variant`,
  `data-size` and `data-tone` are closed choices, `data-open`, `data-invalid`,
  `data-pending` and the like are states, `data-ui-*` are the runtime's hooks. The
  stylesheet and the runtime read nothing else.
- **Components.** Static: Alert, AspectRatio, Attachment, Avatar, Badge, Breadcrumb,
  Bubble, Button, ButtonGroup, Card, Checkbox, Direction, Empty, Field, Input,
  InputGroup, Item, Kbd, Label, Marker, Message, NativeSelect, Pagination, Progress,
  RadioGroup, Separator, Skeleton, Spinner, Switch, Table, Textarea, Toggle, Heading,
  Text. Native overlays: Accordion, AlertDialog, Collapsible, Dialog, Drawer,
  HoverCard, Popover, ScrollArea, Sheet, Tooltip. Layout: Stack, Row, Grid, Container.
  Shell: `Shell`, `AppShell`, `Page`. Components carry no outer margin: layout
  belongs to the layout primitives.
- **Forms.** `#[derive(Validate, Form)]` on one struct: doc comments are labels, types
  pick widgets, rules set `required`, `maxlength` and `min` in the browser and run
  again on the server. `FormView` draws it, `Submit<T>` reads it back and answers
  field errors under the inputs, or the page again with JavaScript off.
- **Tables.** One definition, `#[derive(TableRow)]`, drives the URL, the query, the
  drawing, the export and bulk actions. See [The data table](#the-data-table).
- **The wire.** `Cmd` is the closed list of what an element does in the browser with
  no request. `Op` is the closed list of what a server action sends back, with
  rendered HTML as payload. `Reply` carries ops and a fallback for JavaScript off.
- **Assets.** One stylesheet and the runtime's modules, embedded with brotli and gzip
  forms, at content-hash URLs, immutable. Rendered pages of 32 KiB or more go out in
  brotli quality 4; smaller ones in the HTTP pipeline's gzip.
- **Tests.** `TestView` drives a page in a plain test. `e2e/ui` runs morph properties,
  axe in every state and keyboard scripts in real browsers.

## What ships to the browser

| Page | Script |
|---|---|
| Nothing interactive | None. Links are prefetched from a `Speculation-Rules` header. |
| A dialog, an accordion, a switch | None: `<dialog>`, `command`, `<details>`, `<input>`. |
| A popover, tooltip or hover card | `overlay`: placing and hover delay. |
| A form, a region, a command | `core`: delegation, commands, ops, morph, boosted links, pending states. |
| A data table | `core`, `table`, `overlay`. |
| The app shell | `core`, `sidebar`. |

Everything is delegated from `document`, so a morph or a swap needs no re-wiring.
`tests/budgets.rs` holds the sizes, and a regression fails the build.

## The data table

```rust
#[derive(Clone, Model, TableRow, serde::Serialize)]
#[table(sort = "-closed_on,title", per = 25, per_pages(10, 25, 50, 100), version = "version")]
struct Deal {
    #[col(skip)]
    id: i64,
    #[col(sortable, search, sticky, filter = text, link = "/deals/{id}")]
    title: String,
    #[col(sortable, filter = multi, options = STAGES, badge = tone)]
    stage: String,
    #[col(sortable, numeric, filter = range)]
    amount: Option<i64>,
    #[col(sortable, filter = date)]
    closed_on: Option<chrono::NaiveDate>,
    #[col(filter = boolean)]
    active: bool,
    #[col(skip)]
    version: i64,
}

async fn index(shell: AppShell, uow: Uow, query: TableQuery<Deal>) -> Result<AppPage> {
    let page = query.load(Deal::query(), &uow).await?;
    Ok(shell.page("Deals").body(
        DataTable::new("deals", &query, &page)
            .caption("Deals")
            .sticky_header()
            .export("/deals/export")
            .bulk("/deals/bulk", [
                BulkAction::new("archive", "Archive"),
                BulkAction::new("delete", "Delete").destructive().confirm("Delete {n} deals?"),
            ]),
    ))
}

async fn bulk(uow: Uow, request: BulkRequest<Deal>) -> Result<Reply> {
    let target = match request.resolve(Deal::query(), &uow).await? {
        Ok(target) => target,
        Err(refusal) => return Ok(request.refused(StatusCode::CONFLICT, refusal.message())),
    };
    let count = target.delete(&uow).await?;
    Ok(request.done(format!("Deleted {count}.")))
}
```

| Column option | Means |
|---|---|
| `sortable`, `search`, `filter = text\|range\|date\|select\|multi\|boolean\|relation` | what can be done with it; `kind = int` when the cell shows something other than the column queried; `source = "companies.name"` when the column has another name |
| `sticky`, `sticky_end`, `hidden`, `locked`, `hide_on_mobile`, `width = 240`, `group = "Address"` | where it sits |
| `numeric`, `align = center`, `strong`, `truncate`, `wrap`, `link = "..."`, `badge = tone`, `options = LIST` | how it reads |

| On the table | Means |
|---|---|
| `#[table(cursor)]` | keyset pages, for huge tables |
| `#[table(capped = 10000)]`, `#[table(no_count)]` | count no further than this; or don't, with Previous and Next only |
| `#[table(version = "col")]` | a column that changes with a row, compared before a bulk action |
| `.filter_row()` | filters under the headers, not in a popover from each |
| `.detail(fn)`, `.row_actions(fn)`, `.row_href(fn)`, `.sticky_rows(fn)`, `.selectable_if(fn)` | per row |
| `.keyboard_grid()` | one tab stop, arrow keys, Home, End, Page Up and Down |
| `.views(&views, url)`, `.export(url)`, `.foot(col, cell)`, `.labels(&labels)`, `.can_select(bool)`, `.failed(msg)`, `.loading()` | the rest |

**The URL holds everything but the selection:** `q`, `sort=-amount,title`, `page`, `per`, `after`, `before`, `f.<col>`, `f.<col>.min`, `.max`, `.empty`, `cols`, `w`, `density`, `open`. Back, forward, reload and a shared link restore it, and it all works with JavaScript off. Bad values are ignored one at a time; sort and filter columns come only from the definition; values always bind, typed by the column.

**Selection** is per tab, in `sessionStorage`. Ticked keys survive pages, sort and filter changes. "All that match" survives pages and sort, ends on any filter change, and takes exclusions. Actions post the state, the exclusions and the count the user saw; `resolve` runs the same definition on your scoped query and refuses if the count, or the version, differs. Nothing a hover can fire changes anything: whatever writes is a POST.

**NULL is the smallest value on every store.** Ascending NULLs first, descending NULLs last, because that's what MySQL, SQLite and MongoDB do by themselves and one index serves both directions. On PostgreSQL write the index as `(col NULLS FIRST, id)`. Ties go by key, in the direction of the last term. Text sorts by the column's collation, so accents and punctuation order as the database says.

**A huge table** wants an index per sort column, ending in the key. Deep numbered pages read from the nearer end of the index and find keys before rows; `#[table(cursor)]` costs the same at any depth. A search or text filter is `contains`, which reads every row unless the column has a trigram index on PostgreSQL. On numbered pages a row added or deleted between pages moves what follows by one: cursor pages don't.

**Stored views** are SQL only (`SavedViewModel`, table `navree_ui_table_views`: `id, owner, table_id, name, query, is_default`; create it in a migration). Save through `save_view`, read through `list_views`, and open with `query.or_default_view(&views)`.

**One table per URL.** State lives in plain parameters, so two tables on one page would share it: give the second its own route or region.

## The browser side

`runtime/src` holds plain ES modules with JSDoc types, `styles/` the stylesheet's
sources. Their build output is committed in `dist/`, with `src/assets_gen.rs`, so
`cargo build` needs no Node. After changing either:

```sh
cd crates/navree-ui/runtime
npm ci
node build.mjs            # --check fails when the committed output is stale
```

`src/tokens.rs` is the design tokens' one source, and a test fails when
`styles/tokens.css` differs from it. `runtime/icons.mjs` generates `src/icons.rs` from
lucide-react.

## Proof pages

`apps/ui-table-proof` is the data table against a real database, and `apps/ui-proof`
is a form, a 1,000-row table, a dashboard and every component in its states, as one
binary:

```sh
cargo run --release -p ui-proof -- serve    # http://127.0.0.1:8090; PORT sets the port
cargo run --release -p ui-proof -- bench    # render times, no server
```

## Known limits

- Behind navree-session a plain form proves itself with the token field:
  `FormView::csrf(session.csrf_token())`, or `form::csrf_field` in any other form.
  The session layer reads that field from a urlencoded body of at most 256 KiB;
  a multipart form or a larger body has to send the header, which the runtime does.
- A wrong-typed value reports only the first such field per submit; rules report
  every field once the form deserializes.
- `Toggle` is a checkbox underneath rather than a button with `aria-pressed`, so that
  it posts and keeps state with no script.

See `THIRD_PARTY.md` for where the structure, the icons and the font come from.
