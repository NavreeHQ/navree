# navree-ui

The UI kit of Navree's server-rendered tools: Helm (the admin panel),
Lighthouse, Fleet, Logbook and Navigator's pages. One look for all of them,
with no Node in the build:

- **The app shell** as templates for [navree-view]: a sidebar in
  the shadcn/ui `sidebar-07` layout (app switcher, groups of 32px items with
  lucide icons, sub-items, counts, the user menu; collapsible to icons with
  `Ctrl/Cmd+B`, remembered; an off-canvas sheet on phones), a 48px top bar
  (trigger, breadcrumb, search, page actions), the command palette
  (`Ctrl/Cmd+K`) and toasts.
- **Components as macros**: page headers, cards, stat cards, charts, data
  tables (sortable headers, selection with bulk actions, row actions,
  pagination in Navree's page shape, empty and loading states), forms (with
  field errors in Navree's validation error shape), badges, tabs, menus,
  dialogs, alerts and more.
- **Embedded, versioned assets**: one stylesheet built from the site's tokens,
  one small vanilla script, the favicon and the fonts (Geist for the UI,
  Monaspace Neon for code), served by `UiModule` at
  `{mount}/{version}/{file}` with `Cache-Control: public, max-age=31536000,
  immutable`. The version is a hash of the files, so a new build gets new
  URLs.
- **A strict content security policy**: pages need no inline script or
  style. `HtmlPage` sends `PAGE_CSP` (`script-src 'self'`, `style-src 'self'`,
  no framing) and `Cache-Control: no-store`.

The design rules are `apps/website/DESIGN.md` section 9, "App UI": ink steps
for navigation, hover and selection; the green accent only for the primary
action, focus and positive data; 4px corners on controls, 6px on cards,
popovers and dialogs; no pills, no glow, no letter tiles; 14px text, 40px
table rows, 24px page padding.

## Using the shell in a tool crate

A tool (Helm, Lighthouse, Fleet, Logbook) adopts the kit in four steps.

### 1. The app includes `ViewModule` and `UiModule`

```rust
use navree_config::{ConfigModule, ConfigSources};
use navree_http::HttpModule;
use navree_kernel::Navree;
use navree_ui::UiModule;
use navree_view::ViewModule;

Navree::app()
    .module(ConfigModule::for_root(ConfigSources::defaults()))
    .module(HttpModule::from_config())
    .module(ViewModule::from_config())
    .module(UiModule::from_config()) // or UiModule::for_root(UiOptions { .. })
    .module(HelmModule)
    .run()
```

`UiModule` is global. It provides the `Ui` service, adds the `ui/*`
templates to the app's `Views`, and serves the assets. Through the facade it
is `navree::ui` (feature `ui`, part of `full`).

| Setting | Variable | Default | |
|---|---|---|---|
| `ui.mount` | `APP_UI__MOUNT` | `/_navree/ui` | Where the assets (and the gallery) are served |
| `ui.gallery` | `APP_UI__GALLERY` | on in debug builds | The component gallery at `{mount}/gallery` |

A tool can import it from its own module (`m.import(UiModule)`) rather than
ask every app to add it: the plain `UiModule` reads section `ui` at boot (so
the app needs `ConfigModule`), and an app that configures it with
`UiModule::for_root(..)` wins over every plain import (one module id,
registered once).

### 2. The tool's templates extend the shell

A tool registers its templates with navree-view (`m.templates(&[...])` in its
module, usually with `include_str!`), names them under its own prefix, and
extends `ui/app.html`:

```jinja
{% extends "ui/app.html" %}
{% import "ui/components.html" as ui %}

{% block actions %}{{ ui.button("New queue", variant="primary", icon="plus", size="sm", href="/fleet/queues/new") }}{% endblock %}

{% block content %}
{% call ui.page_header("Queues", "Every queue of the app, with its workers.") %}
  {{ ui.button("Pause all", icon="pause", size="sm", dialog="pause-all") }}
{% endcall %}
<div class="ui-stack">
  <div class="ui-grid ui-grid--stats">
    {{ ui.stat("Jobs per minute", stats.rate, delta="+12%", trend="up", spark=stats.rate_spark) }}
    {{ ui.stat("Failed", stats.failed, delta="-3", trend="down", tone="positive") }}
  </div>
  {{ ui.data_table(queues) }}
</div>
{% endblock %}

{% block dialogs %}
{{ ui.confirm_dialog("pause-all", "Pause every queue?", "Workers finish their current job and stop.", confirm="Pause all", variant="primary", action="/fleet/queues/pause", csrf=csrf) }}
{% endblock %}
```

Blocks of `ui/app.html`: `content` (the page), `actions` (the top bar's right
side, after search), `dialogs` (after the shell), `main_class` (extra classes
on `<main>`, such as ` ui-main--narrow`), and `title` and `head` from
`ui/base.html`. Pages outside the shell (sign-in, two-factor, password reset)
extend `ui/auth.html`, a centered panel with the app's mark, and fill
`content`.

### 3. A handler builds the `Shell` and renders

```rust
use http::HeaderMap;
use navree_http::AppError;
use navree_ui::{
    Crumb, HtmlPage, MenuItem, NavGroup, NavItem, Ui, UserMenu, render, sidebar_collapsed,
};
use navree_view::Views;
use serde_json::json;

async fn queues(views: &Views, ui: &Ui, headers: &HeaderMap) -> Result<HtmlPage, AppError> {
    let shell = ui
        .shell("Fleet") // the assets URL comes from the Ui service
        .context("Acme Inc.")
        .home("/fleet")
        .group(
            NavGroup::new("Queues")
                .item(NavItem::new("Overview", "/fleet").icon("layout-dashboard"))
                .item(NavItem::new("Queues", "/fleet/queues").icon("boxes").badge("12"))
                .item(
                    NavItem::new("Jobs", "/fleet/jobs")
                        .icon("list-checks")
                        .child("Pending", "/fleet/jobs/pending")
                        .child_with_badge("Failed", "/fleet/jobs/failed", "3"),
                ),
        )
        .user(
            UserMenu::new("Ada Lovelace", "ada@acme.test")
                .item(MenuItem::link("Sign out", "/fleet/logout").icon("log-out")),
        )
        .crumb(Crumb::link("Fleet", "/fleet"))
        .crumb(Crumb::page("Queues"))
        .title("Queues") // <title>Queues · Fleet</title>
        .collapsed(sidebar_collapsed(headers)) // the user's choice, without a flash
        .current("/fleet/queues"); // marks the best-matching item active

    render(views, "fleet/queues", &shell, json!({ "queues": table, "stats": stats }))
        .map_err(AppError::internal)
}
```

`render` puts the shell in the context as `shell` next to the fields of
`ctx`, and returns an `HtmlPage` (with the policy headers). Inject `Views`
and `Ui` into the controller as usual (`ProviderKey::of::<Views>()`,
`ProviderKey::of::<Ui>()`).

The rest of the `Shell`:

- `.switch(MenuItem)`: entries of the app switcher in the sidebar header
  (`MenuItem::label`, `::link(..).icon(..).description(..).shortcut(..).checked(..)`,
  `::separator`). Without entries the header is a plain link home.
- `.commands(CommandGroup)`: palette entries besides the sidebar's items
  (which the palette lists under "Go to" on its own): `Command::link` or
  `Command::dialog`, with `.icon`, `.hint` and `.keywords`.
- `.toast(Toast::success("Saved").description(".."))`: toasts shown when the
  page loads (a flash message after a redirect).
- `.search(None)` hides the search box; `.search(Some("Search users…"))`
  changes its placeholder.
- `.lang("de")` sets `<html lang>`.

### 4. Data comes in prepared

Components that show data take values built in Rust, so templates stay
simple and every page formats things the same way:

```rust
use navree_ui::{Action, Cell, Column, Empty, MenuItem, PageInfo, Row, Table, Tone};

let table = Table::new("queues")
    .caption("Queues")
    .column(Column::new("name", "Queue").sortable())
    .column(Column::new("status", "Status"))
    .column(Column::new("pending", "Pending").numeric().sortable())
    .column(Column::new("updated", "Updated").hide_on_mobile())
    .rows(queues.iter().map(|q| {
        Row::new(q.id.to_string())
            .cell(Cell::lines(&q.name, &q.connection).href(format!("/fleet/queues/{}", q.id)))
            .cell(Cell::status("Running", Tone::Success))
            .cell(Cell::number(q.pending.to_string()))
            .cell(Cell::muted(&q.updated))
            .action(MenuItem::link("Open", format!("/fleet/queues/{}", q.id)).icon("arrow-up-right"))
            .action(MenuItem::separator())
            .action(MenuItem::dialog("Clear", "clear-queue").icon("trash-2").danger())
    }))
    .selectable()
    .bulk(Action::dialog("Pause", "pause-selected").icon("pause"))
    .sort(&query.sort, "/fleet/queues?sort={sort}")
    .page(PageInfo::from_meta(&page.meta).unwrap_or(PageInfo::new(1, 15, 0)), "/fleet/queues?page={page}")
    .per_page("/fleet/queues?per_page={per_page}", &[15, 25, 50])
    .empty(Empty::new("No queues yet").description("Queues appear when the app dispatches its first job.").icon("boxes"));
```

- `PageInfo` is Navree's page shape (`current_page`, `per_page`, `total`,
  `last_page`); `PageInfo::from_meta` takes a `navree_db` page's `meta`.
- `.loading(rows)` renders skeleton rows instead of data.
- Selected rows post as `selected[]` when the table sits in a `<form>`.
- `FormErrors::from_app_error(&error)` (or `from_body` for an API response)
  turns a validation error into what the form macros show:
  `{{ ui.field("email", "Email", errors=errors) }}` marks the field invalid
  and prints its message; `{{ ui.form_errors(errors) }}` lists every message.
- `Chart::bars(labels)` / `Chart::lines(labels)` with `.series(name, tone,
  values)`, `.dashed_series`, `.highlight(index)`, `.prefix("$")`,
  `.suffix(" ms")`, `.size(ChartSize::Small)`, `.label_every(n)`; and
  `Sparkline::new(values)` for stat cards. Charts are SVG drawn from
  attributes (no inline styles); every bar or x position has a tooltip, and
  the figure has a text summary for screen readers. `SeriesTone::Accent` is
  for positive data only; `Muted` and `Bright` cover everything else.

## The components

`{% import "ui/components.html" as ui %}`, then:

| Macro | What it renders |
|---|---|
| `button(label, variant, size, icon, icon_end, href, type, dialog, shortcut, disabled, attrs)` | A button or link. Variants `primary` (the one green action of a view), `secondary`, `ghost`, `danger`, `dashed` (filters); sizes `sm` (28px), default (32px), `lg` (36px) |
| `icon_button(icon, label, ...)` | A square icon button with an accessible label |
| `badge(label, tone, icon, dot)` | A square-cornered label; tones `success`, `warning`, `danger`, `outline`, `mono`, neutral |
| `status(label, tone)` | A status dot and its label |
| `avatar(name, src, initials, size)`, `kbd(keys)` | A 32px square avatar; a key hint (`"mod+K"` shows `Ctrl` or `⌘`) |
| `page_header(title, description)` | The page's H1, a description, and actions (the `call` body) |
| `card(title, description, link, footer, flush)`, `section_title(title, link)` | A 6px card with a header; a heading inside a page |
| `stat(label, value, delta, trend, tone, note, icon, spark)` | A stat card with the change and a sparkline |
| `chart(data)`, `chart_card(title, data, description, value)` | A bar or line chart from a `Chart`, bare or in a card with its legend |
| `data_table(table)`, `search_input(...)` | A data table from a `Table`; a search box for its toolbar (`ui-table-toolbar`) |
| `pagination(page, url, per_page_url, options)` | Pagination from a `PageInfo` (the table renders it for you) |
| `field`, `textarea_field`, `select_field`, `checkbox_field`, `switch_field` | Form fields with a label, a hint and their errors |
| `form_errors(errors)` | Every message of a failed submission, in an alert |
| `tabs(id, items, selected, variant)`, `link_tabs(items, variant)` | Tabs with panels (`call(tab)`), or tabs as links; segmented or `line` |
| `dropdown(id, label, items, ...)`, `menu(id, items)` | A button with a menu of `MenuItem`s |
| `dialog(id, title, description, size)`, `confirm_dialog(...)` | A native modal `<dialog>`; a confirmation that can post a form with a CSRF field |
| `alert(title, body, tone, items)`, `progress(value, max, tone)`, `details(items)` | An alert; a progress bar; a description list |
| `empty_state(...)`, `loading(label)`, `skeleton(width)` | Empty and loading states |
| `icon(name, size, class, label)`, `mark(size)` | A lucide icon (stroke 1.75) from `ICONS`; the Navree mark |

Layout classes for page bodies: `ui-stack` (`--sm`, `--lg`), `ui-grid` with
`--stats` (four stat cards), `--2`, `--3` and `--main-side` (a wide column and
a side column), `ui-form` and `ui-form__row`, `ui-actions`, `ui-btn-group`,
`ui-main--narrow` (on `<main>` through `main_class`).

### Behavior

`navree-ui.js` wires everything through `data-*` attributes, so templates
never need a script of their own:

- `data-ui-dialog-open="id"` (or `dialog=` on a button or menu item) opens a
  dialog; `data-ui-dialog-close` closes it; focus returns to the opener.
- Menus open next to their trigger and move with the arrow keys, Home, End
  and Escape.
- `Ctrl/Cmd+B` toggles the sidebar (stored in `localStorage` and the
  `navree_ui_sidebar` cookie that `sidebar_collapsed` reads), `Ctrl/Cmd+K`
  opens the palette, Escape closes the sheet.
- Tabs move with the arrow keys; selecting rows shows the table's bulk bar
  while the table is on screen.
- `window.NavreeUI` exposes `toast({ title, description, variant })`,
  `openDialog(id)`, `closeDialog(dialog)`, `openCommand()`,
  `toggleSidebar()` and `setSidebarCollapsed(bool)`, for a tool's own
  script (served from the tool's own route, never inline).

## The gallery

Every component on one page, plus an example dashboard and a sign-in page:

- In development `UiModule` serves it at `{mount}/gallery` (`ui.gallery`).
- `cargo run -p navree-ui --example gallery -- <dir>` writes it to static
  HTML files with the assets, to open in a browser.
- `tests/gallery.rs` renders every page, checks that every macro of
  `ui/components.html` appears, and that no page has an inline script,
  style or event handler. Set `NAVREE_UI_GALLERY_DIR` to keep its output.

## Icons

The kit ships a set of lucide icons (`navree_ui::ICONS`), generated from the
website's `lucide-react` so the kit draws exactly the site's icons. To add
one, put its name in `scripts/icons.mjs` and run
`node crates/navree-ui/scripts/icons.mjs apps/website/node_modules/lucide-react`;
the output (`templates/ui/icons.html`, `src/icons.rs`) is committed, so
building the crate never needs Node. An unknown icon name fails the render.

## Tests

`cargo test -p navree-ui`: snapshot tests of every component's HTML
(`tests/components.rs`, insta), the module's routes and caching
(`tests/module.rs`), the gallery (`tests/gallery.rs`), and a check that the
stylesheet's palette equals the site's `globals.css` (`tests/tokens.rs`).

[navree-view]: ../navree-view
