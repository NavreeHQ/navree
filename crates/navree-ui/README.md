# navree-ui

Navree's UI toolkit. Typed Rust components render to HTML on the server, a small
owned runtime handles behaviour, and a page with nothing interactive ships no
JavaScript. No per-tab server state, no WASM, no Node at run time or in
`cargo build`.

```rust
use navree::ui::*;

async fn index(shell: AppShell, query: TableQuery<DealRow, DealFilter>) -> Page {
    let (rows, total) = load(&query).await;
    shell
        .page("Deals")
        .actions(Button::primary("New deal").icon(icons::PLUS).href("/deals/new"))
        .body(DataTable::new("deals", &query, &rows).total(total))
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
- **Tables.** `#[derive(TableRow)]`, `#[derive(FilterBar)]`, `TableQuery` (sort,
  filter and page in the URL) and `DataTable` (a region that morphs in place).
  Rows write straight to the page rather than through the tree.
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
| A form, a table, a region, a command | `core`: delegation, commands, ops, morph, boosted links, pending states. |
| The app shell | `core`, `sidebar`. |

Everything is delegated from `document`, so a morph or a swap needs no re-wiring.
`tests/budgets.rs` holds the sizes, and a regression fails the build.

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

`apps/ui-proof` is a form, a 1,000-row table, a dashboard and every component in
its states, as one binary:

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
