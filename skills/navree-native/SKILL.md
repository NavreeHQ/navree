---
name: navree-native
description: "Makes a Navree UI app answer native clients (iPhone, iPad, Android phones and tablets) from the same pages, so each system draws them with its own components: turns on the preview (`ui-native-preview`), says what a page's parts are (`AppPage::actions` and `Button::role` for a screen's actions, `Button::presented` for how the screen a button or a link leads to opens, `Button::undoable`, `Button::standard` and `Button::reason`, `#[col(role = ..)]` and `DataTable::row_opens` for records, `Item` lists, a sheet that is a task or a glance, `native::only` and `native::web_only`), reads `navree native:coverage` (the grades idiom, layout and web) and `native.toml`, tests a page as a native client with `TestNative`, and says plainly what falls back to the web view and what does not exist yet (device features from a page, watch and TV, widgets, Windows and Linux clients, the app on the device). Use when asked to make a Navree app a native app or an iOS or Android app, to make a page look right on a phone, to read or raise native coverage, or to ask whether a part is native. This is the entry point: it says which of `navree-native-design`, `navree-native-apple` and `navree-native-android` to load next."
---

# Navree Native (preview)

Navree Native lets the Navree UI pages you already have answer a native app.
The same handler, the same typed components, one codebase:

- A browser gets HTML, byte for byte what it got before. A native client sends
  `X-Navree-Native: 1` and gets the page as a tree of semantic parts (screen,
  list, form, field, button, menu, sheet).
- A client for each system draws the tree with that system's own components,
  SwiftUI on Apple systems and Jetpack Compose with Material 3 on Android, and
  puts each part where that system's design guidelines put it.
- The server says what a thing is. The client decides where it goes and how it
  looks. A page never says "iOS" or "Android".
- A page with a part that has no native meaning yet is shown whole in the
  system web view inside the native bars, so the app works while coverage grows.
- It is a preview: it sits behind a feature whose name says it may change, and
  what it answers may change in a minor release, outside the 1.x promise.

## The rules

1. **Say what a thing is, not where it goes.** A main action has a role, a
   table's column has a role, a row says what it opens. Never branch on the
   platform to move a button.
2. **Write the page once.** If a screen has to be written twice, stop: say
   what its parts are instead. `native::only` and `native::web_only` keep a
   part for one side; they are for parts that only make sense there. Only a
   screen that deserves a design of its own (a day at a glance, a wall of
   tiles) is written for native clients, with Navree Native UI (below): reach
   for it when saying what the parts are cannot make the screen belong, never
   for a list, a record or a form.
3. **Use the toolkit's components** (the index of `navree-ui`). Hand-written
   HTML, a canvas or a stylesheet trick cannot be read: the page falls back
   to the web view.
4. **Run `navree native:coverage`** before calling a page done. Only `idiom`
   belongs to its system.
5. **Do not ask for a system's look from the server.** No glass, no corner
   radii, no fonts for bars. A brand gives an accent colour; the system keeps
   its own materials, type and shapes (see `navree-native-design`).
6. **Nothing here is verified on a device by this skill.** The clients were
   run in cloud simulators and emulators; say what you ran.

## 1. Turn it on

Add the facade feature, either on the `navree` dependency or as a feature of
your own app, so a plain build stays as it was:

```toml
[features]
native-preview = ["navree/ui-native-preview"]
```

With it on, `UiModule` answers a request that says `X-Navree-Native` (and
describes itself in `X-Navree-Client`) with the envelope of the protocol
(`navree::ui::native::protocol`), serves the app's shell as data at
`/_navree/native/shell` (the destinations, the user menu, the theme's accent,
which routes open as sheets) and registers the app command
`native:coverage`. An unreadable client descriptor is a 400, an unknown
protocol major a 406. With the feature off nothing changes, and with it on a
browser's pages are the same bytes: the demo apps' tests compare them.

Add the shell the usual way (`navree-ui`, section 1) and give every top-level
`NavItem` an icon. The shell becomes the tab bar, the sidebar or the
navigation rail; nothing else names the structure.

## 2. Say what the parts are

These are the words an author has. Each is a field of a component that writes
no HTML, so a browser never sees it.

### A screen's main actions

`AppPage::actions` is what stands beside a page's title. A native client puts
them where its system puts a screen's actions. The look of a button decides
its role unless you say it: `Button::primary` is the main action,
`Button::outline` a secondary one and `Button::destructive` a destructive one.
A default or ghost button in `actions` has no role, and the page is only
`layout`. One main action per screen: two (`Primary` or `Create`) are `layout`
too, and the report says which ("the screen has two main actions: “Mark as
packed” and “Print label”").

```rust
use navree::ui::{AppPage, AppShell, Button, Card, Role, Text, icons};

/// The orders page: one action the screen is for, one more beside it.
pub fn orders_page(shell: AppShell) -> AppPage {
    shell
        .page("Orders")
        .actions((
            Button::primary("New order")
                .icon(icons::PLUS)
                .href("/orders/new")
                .role(Role::Create),
            Button::outline("Export").icon(icons::DOWNLOAD).href("/orders/export"),
        ))
        .body(Card::titled("Open orders").body(Text::new("14 orders wait to be packed.")))
}
```

### A button's role, and how its screen opens

`Button::role(Role)` says what a button is for when its look does not:
`Primary`, `Secondary`, `Destructive`, `Cancel`, `Confirm`, `Create`,
`Navigation`. Creation is a role, never a styled button: an Android phone
draws it as the floating action button, an iPhone as a plus in the toolbar.
`Button::presented(Presented)` says how the screen a link leads to opens:
`Push`, `Replace`, `Root`, `Sheet`, `Task`, `Full`. Say it only when the page
knows; the shell's routes (`/*/new` and `/*/*/edit` open as sheets, the
sign-in page covers everything) decide otherwise.

```rust
use navree::ui::{Button, Presented, Role};

/// Opens the refund form over this screen as a task to finish or cancel.
pub fn refund_button(order: i64) -> Button {
    Button::outline("Refund")
        .href(format!("/orders/{order}/refund"))
        .role(Role::Secondary)
        .presented(Presented::Task)
}
```

### What else a button can say

Each of these is a field of the button that writes no HTML.

- `Button::undoable()`: what it does can be undone (a toast restores it). A
  system whose guidelines say so asks nothing first and offers Undo after.
  Without it a destructive action asks first.
- `Button::standard(Standard)`: it is a standard action by meaning (`Share`,
  `Delete`, `Add`, `Edit`, `Search`, `Done`, `Cancel`, `More`), so each system
  draws its own symbol and label for it.
- `Button::reason(text)`: why a disabled button is disabled, in the page's
  words. A disabled button whose `title` says it is read the same way.
- `presented(Presented)` is also on `Item` and on any link written as an
  element (`el("a").href(..).presented(..)`).
- `FormView::cancel` gives its link the `Cancel` role by itself.

```rust
use navree::ui::{Button, Item, Presented, Standard};

/// Final, so it asks first: the destructive role and a question.
pub fn cancel_order() -> Button {
    Button::destructive("Cancel order").opens_dialog("cancel")
}

/// Can be undone, because a toast restores it: no question first.
pub fn archive() -> Button {
    Button::destructive("Archive").undoable()
}

/// A standard action by its meaning: each system draws its own symbol and label.
pub fn share() -> Button {
    Button::outline("Send the invoice").standard(Standard::Share)
}

/// Off, and why, in the page's own words.
pub fn resend() -> Button {
    Button::outline("Resend")
        .disabled(true)
        .reason("It went out at 09:41. It can go again after 09:46.")
}

/// A row that is a link says how its screen opens, as a button does.
pub fn order_row() -> Item {
    Item::new("Order CS-20481").href("/orders/1").presented(Presented::Sheet)
}
```

### Records: column roles and what a row opens

A table is a grid on a wide screen and a list of records on a phone. The
roles say which column is the record's name, a line under it, its state and
what stands at its end. `row_opens` (a drawer or sheet that answers ops) and
`row_href` (a page) say what a tap on the row opens; a row with a link in its
naming cell opens that.

```rust
use chrono::NaiveDate;
use navree::ui::{DataTable, Empty, TablePage, TableQuery, TableRow, icons};

/// An order as the table shows it.
#[derive(Clone, TableRow)]
#[table(cursor, sort = "-placed_on", per = 25, per_pages(10, 25, 50, 100))]
pub struct OrderRow {
    #[col(skip)]
    pub id: i64,
    #[col(sortable, search, sticky, label = "Order", role = title)]
    pub number: String,
    #[col(search, hidden, role = subtitle)]
    pub customer: String,
    #[col(sortable, filter = relation, role = status)]
    pub status: String,
    #[col(sortable, numeric, role = trailing)]
    pub total: i64,
    #[col(sortable, filter = date, label = "Placed")]
    pub placed_on: NaiveDate,
}

/// The orders table. A tap on a row opens the order over the list.
pub fn orders_table<'a>(
    query: &'a TableQuery<OrderRow>,
    page: &'a TablePage<OrderRow>,
) -> DataTable<'a, OrderRow> {
    DataTable::new("orders", query, page)
        .caption("Orders")
        .row_opens(|row| format!("/orders/{}/peek", row.id))
        .filter_bar()
        .empty(
            Empty::new("No order matches")
                .icon(icons::SEARCH)
                .description("Clear a filter, or search for an order number."),
        )
}
```

The roles are `title`, `subtitle`, `status` and `trailing`. A hidden column
may still be a role: the customer above shows nowhere on a wide web table and
is the line under the order's number on a phone. The columns' search, filters
and sort become the screen's search bar, filter chips and sort menu (see
`navree-native-design`); you write none of them twice.

### Lists of records that are not tables

Use `Item` for a short list of things (lines of an order, linked records). A
row that is a link is one touch target on every system.

```rust
use navree::ui::{Item, el};

/// The lines of an order, as a system draws a list of records.
pub fn order_lines() -> navree::ui::Element {
    el("ul").children((
        el("li").child(
            Item::new("Pashmina shawl, Patiala")
                .description("CLO-102-021 · 1 × ₹5,949")
                .aside("₹5,949")
                .href("/products/102"),
        ),
        el("li").child(
            Item::new("Phulkari dupatta, Amritsar")
                .description("CLO-118-004 · 2 × ₹1,250")
                .aside("₹2,500")
                .href("/products/118"),
        ),
    ))
}
```

### A sheet: a task or a glance

A sheet that holds a form is a **task**: the person finishes or cancels it, so
a system gives it its Cancel and Done places. A sheet that holds none is a
**glance**: a look at something, put away with a swipe. The reader tells them
apart by the form (`task` is on the sheet the client gets); you decide by what
you put in. Neither client uses the difference yet, so today both draw a
sheet as a sheet, but write the page as if they did. Do not put a form in a
glance only to get a button. A sheet's own buttons (Cancel, Done, Refund) go in
its footer (`Sheet::footer`, `Dialog::footer`), not in its body: a client draws
the footer's buttons as the sheet's actions, by role, and the buttons of the
body as content. Do the same for a page: what is the page's own is in
`AppPage::actions`; the body keeps the content.

```rust
use navree::ui::{FactCard, FactGrid, Sheet, Stack, Text};

/// A glance at a product: it holds no form.
pub fn product_glance(name: &str, stock: i64) -> Sheet {
    Sheet::new("product", name.to_owned()).body(Stack::gap(3).children((
        FactGrid::new().cols(2).cards([FactCard::new("In stock", stock.to_string())]),
        Text::muted("Reorder at 14."),
    )))
}
```

### Parts for one side

`native::only(node)` is the node for a native client and nothing for a
browser; `native::web_only(node)` is the reverse. Use them for what only makes
sense there: a drag handle's hint, a keyboard palette, a second layout of the
same records. The page is built while the client is current, so call them in
the handler.

```rust
use navree::ui::{Node, Text, native};

/// A hint a keyboard needs and a phone does not.
pub fn shortcuts_hint() -> Node {
    native::web_only(Text::muted("Press / to search."))
}
```

A handler whose screen really differs by system reads the `Client` extractor
(`Client::current().platform()`, `.form()`, `.is_compact()`). Reach for it
last: it breaks the rule that lowering depends on the tree only.

## 3. Grades and the coverage report

Every page route is graded for the client that asks:

| Grade | Means |
|---|---|
| `idiom` | The parts say what they are, so the client lays the screen out as its own system does: records as rows with a title, rows that open something, actions with a role. |
| `layout` | Native parts laid out as the web page is. It works and looks like a web page drawn with the system's controls. |
| `web` | The client shows the page in its web view, because a part has no native meaning yet. |
| `designed` | A screen written for native clients with Navree Native UI, not read from a page. The report lists beside it what such a screen should fix. |

`navree native:coverage` asks every page route in process, as an Android phone
by default (`--platform ios --form tablet` for another), and prints the grade
and, in words, what is missing. This is the store demo (`apps/store-demo`),
run on the current code:

```text
Native coverage of store-demo, asked as android on phone

ROUTE                                       GRADE     WHAT IS MISSING
/                                           designed
/fragments/figures                          idiom
/fragments/sales                            idiom
/fragments/shelves                          idiom
/orders                                     idiom
/orders/1                                   designed
/orders/1/steps                             idiom
/orders/1/others                            idiom
/shelves                                    designed
/inventory                                  idiom
/orders?filter[status][in]=open&sort=total  idiom
/inventory?page=2                           idiom

9 idiom, 3 designed, 0 layout, 0 web: 12 of 12 pages are native screens

Skipped (4):
  /app/{key}: needs a value for {key}; add `key = ...` under [coverage.params] in native.toml
  /fragments/activity: answered operations, not a screen
  /orders/{id}/peek (/orders/1/peek): answered operations, not a screen
  /inventory/{id}/peek (/inventory/1/peek): answered operations, not a screen

Not pages, so not asked: the toolkit's and the framework's own files (4); not a GET (7); answers that are not pages (1).
```

That table is the end state: every page says what its parts are, and three
screens (the day, an order, the shelves) are written for native clients. In
the demo each read page got there by saying roles, `row_opens` and column
roles; a page that has not yet gets a `layout` row whose last column says what
it lacks, and a `designed` row lists there what the screen should fix.

How to read it: a `layout` row names what keeps it from `idiom` (the grader's
words are like "the action “Export” has no role", "the orders table has no
title column", "the orders table's rows open nothing", "the screen has no
title"); a `web` row names the part that could not be read (for a handler that
writes its own markup: "the handler writes its HTML by hand, so it cannot be
read; build it with the toolkit's components"). Fix what it names, run it
again.
`--fail-on web` (or `layout`) exits 1 when a page has that grade or worse, for
CI; a run that asked no page fails too. A `designed` screen holds the `layout`
gate unless it has something to fix. Full options are in
`navree-cli-generators`.

## 4. native.toml

A route such as `/orders/{id}` needs an example value before it can be asked.
They live in `native.toml` at the app's root, beside `dock.toml`, never in
`config/` (every file there is a section of the app's settings).

```toml
[coverage.params]
id = 1                                  # every `{id}` in a route

[coverage]
also = ["/orders?filter[status][in]=open&sort=total"]   # addresses with a query
skip = ["/admin/*"]                                     # left out on purpose

[coverage.headers]
cookie = "navree_session=${COVERAGE_SESSION}"           # a signed-in person
```

A route with no value is listed as skipped with the line to add. The grade of
a route depends on its data: an empty table is `idiom`, a full one needs a
title column. `apps/store-demo/native.toml` is a worked example, and
`navree native:doctor` checks what a build needs (the preview feature, app
ids and association files, push keys, the toolchains) without changing
anything.

## 5. Test a page as a native client

`TestNative` drives a page in a plain test, with no browser and no device:
what a native client would see, what a tap does, and the grade. Build the
page inside `TestNative::build`, so the page is made while a native client
is the one asking (a table builds its rows another way then).

```rust
use http::HeaderMap;
use navree::ui::native::protocol::node::{self as n, Node as N};
use navree::ui::native::protocol::{Client, FormFactor, Platform};
use navree::ui::native::{Grade, TestNative, graded};
use navree::ui::Shell;

#[test]
fn the_orders_page_says_what_its_actions_are() {
    let phone = Client::new(Platform::Android, FormFactor::Phone);
    let page = TestNative::build(phone, || {
        let shell = Shell::new("Sutlej Supply").for_request("/orders", &HeaderMap::new());
        orders_page(shell).to_view()
    });
    assert!(page.is_native(), "{:?}", page.why());
    assert!(page.sees("14 orders wait to be packed."));
    let roles: Vec<_> = page
        .screen()
        .actions
        .iter()
        .filter_map(|node| match node {
            N::Button(button) => Some((button.label.as_str(), button.role)),
            _ => None,
        })
        .collect();
    assert_eq!(roles, [("New order", n::Role::Create), ("Export", n::Role::Secondary)]);
    assert_eq!(graded(page.screen()).0, Grade::Idiom);
}
```

`TestNative::of(&view)` reads a finished view, `of_node` a part; `sees`,
`tap(label)`, `find(id)`, `why()` (what could not be read, each once) and
`json()` (the envelope as it goes on the wire) are the rest. Route tests with
`TestApp` send the headers themselves (`x-navree-native: 1` and
`x-navree-client: platform=android; form=phone`); `apps/store-demo/tests/native.rs`
shows the coverage table asserted in a test.

## 6. What falls back to the web view, and why that is fine

A page that holds a part the client cannot read is not an error. The client
shows the page itself, in the system web view inside its own bars; the web
chrome is left out because the client draws it; a link in it opens the native
screen if there is one. The reader never guesses: it lists what it could not
read (`why` in a test, the "WHAT IS MISSING" column in the report). Known
causes today: hand-written HTML or a canvas, a `data-ui-*` hook nothing reads,
`PhoneInput`, a sparkline inside a table cell, a page nested deeper than 64
parts. That is fine because the app always works and coverage grows part by
part; it is a gap to close before a store build, because a store reviews
screens. The web view has no script bridge to the device: a fallback page gets
what a browser tab on the same site gets.

## Writing a screen for native clients

Three ways to get a native screen, each more work and more control; an app
mixes them screen by screen:

1. Let the page be read (nothing to do).
2. Say what the page's parts are (section 2).
3. Write the screen for native clients, with **Navree Native UI**
   (`navree::native::ui`): the protocol's own parts with builders. One piece
   of Rust; SwiftUI and Jetpack Compose both draw it.

A screen is a plain function of its data. It takes no request, no database
handle and no HTML, and a component of your own is a function that returns a
part. Parts come in three levels, and a screen mixes them:

- **Structure the system places:** `Screen` (title, way back, actions by
  `Role`, `pager`), `Section`, `List`, `Row`, `Form`, `Field`, `Sheet`,
  `Dialog`, `Region`.
- **Content the system styles:** `Figure`, `Ring`, `Rings`, `Chart`,
  `DayClock`, `Timeline`, `FeedRow`, `Badge`, `Progress`, `Facts`.
- **Free composition:** `Stack` (`down`, `across`, `gap`, `pad`, `grow`,
  `wide`), `Grid::fit(min)`, `Card` (`tint`, `opens`, `zoom`, `menu`),
  `Layers`, `Shape`, `Carousel`, `Adaptive`, `Text` (`style`, `weight`,
  `colour`, `tabular`, `change`), `Icon::named`.

Every part has builders (`Table`, `Tabs`, `Split`, `Rich`, `Message`,
`Gauge`, `Meter`, `ColorStrip`, `CalendarHeatmap` and the rest), and so has
every control of a form (`TextInput::email()`, `ToggleInput::switch()`,
`ChoiceInput::of(..)`, `DateInput::date()`): never write a struct literal.

```rust
use navree::http::response::Response;
use navree::native::ui::{Card, Change, Grid, Hue, Screen, Stack, Text, TextStyle};
use navree::ui::native::Answer;
use navree::ui::{AppShell, Text as Words};

/// A tile: a component of your own is a function that returns a part.
fn tile(name: &str, count: u32, hue: Hue) -> Card {
    Card::new().tint(hue).parts((
        Text::new(name).style(TextStyle::Caption).muted(),
        Text::new(count.to_string()).style(TextStyle::Title2).change(Change::Count),
    ))
}

/// The screen: a function of what the handler loaded.
fn today(revenue: &str, to_pack: u32, to_ship: u32) -> Screen {
    Screen::titled("Today").refresh().body(Stack::down().gap(4).parts((
        Text::new(revenue).style(TextStyle::Display).change(Change::Count),
        Grid::fit(150).gap(3).parts((
            tile("To pack", to_pack, Hue::Amber),
            tile("To ship", to_ship, Hue::Blue),
        )),
    )))
}

/// One route: the page for a browser, the screen for a native client.
async fn dashboard(shell: AppShell) -> Response {
    let revenue = "₹1,24,050".to_owned();
    Answer::in_shell(&shell).each(
        || shell.page("Dashboard").body(Words::new(revenue.clone())),
        || today(&revenue, 12, 7),
    )
}
```

What to hold to:

- **Only the side that is asked for runs**, in the handler, so both closures
  borrow what the handler loaded. The layer adds the destination, a toast
  left for the screen, the shell's stamp, the session's token in posts and
  the shapes of icons named with `Icon::named(..)`.
- **Actions go to the page's own handlers, and a form is not written
  twice.** `native::form(&view)` says the `FormView` the page draws to a
  native client: the id `#[derive(Form)]` gave it, where it posts, and each
  field with the page's label, help, control, value and message, without the
  form's own buttons. Send it with a button inside it (`.submits()`) or one
  elsewhere (`.submits_form("refund")`). Its errors come back at its fields.
- **A reply is built per client.** `Reply::each(|reply| .., |reply| ..)` goes
  on with the reply for the client that asked: markup for a browser, and for
  a native client parts with `native::morph`, `replace`, `append` and
  `prepend`. What both are told (a close, a toast, the status) is said once.
- **A region or a list of the screen** fetches from a route that answers the
  same way: the native closure may return a `Screen`, a part
  (`Node::from(..)`) or `Vec<Op>` (a list's next rows or its changes).
- **A route with only a native side** is `Answer::in_shell(&shell).native(..)`;
  a browser gets a small page (status `406`) that says the screen is in the
  app, by the app's name, with the way home.
- **A card grows into a screen only when its tap opens one.** `.zoom()` is
  for a card with an id whose action is a navigation (a push, or a screen
  presented as a sheet or full). A card whose tap asks the server for
  changes to this screen opens no screen and does not grow.
- **Room is in steps, type is a text style, colour is a `Hue`.** A `Size` in
  points is only for what has no size of its own (the box a symbol stands
  in). There are no offsets, no colours or corner radii as numbers, no
  fonts, no shadows, no blur, and nothing is composed into a bar.
- **What depends on the room is said in the tree**, not asked of the server:
  `Grid::fit` and `Carousel::min` show as many parts as fit, `Stack::wide`
  turns a stack in a wide room, `Adaptive` holds two sets of parts. Give a
  part with an id to one room only.
- **Lists stay lists.** Records are `List` and `Row`, loaded by cursor, even
  on a designed screen.
- **Test the function**: call it and assert the tree; `native::designed(&screen)`
  lists what it should fix (a tapped card with no name, a card that grows
  into a screen it does not open or has no id, a card in a card, a carousel
  in a carousel, more than one display figure, an id in both rooms of an
  adaptive part, markup no part draws) and must be empty.

The store demo has three such screens beside their pages
(`apps/store-demo/src/screens.rs`): the day, an order and the shelves.

## 7. What does not exist yet

Say these plainly to the owner; do not build around them.

- **Starting a device feature from a page.** There is no page-side way to ask
  for the camera, location, a scanner or a share sheet. A server reply may
  start only a few safe things (a haptic). The clients have some features of
  their own (see the system skills' `references/availability.md`).
- **Watch, TV and Wear OS.** No client. They have no web view, so they will
  take a strict subset and an "open on your phone" screen.
- **Widgets, complications, live activities.** Not built. They are written for
  their surface, not lowered from a page.
- **Windows and Linux clients.** Not built.
- **The app on the device** (the Rust server running inside the app, offline).
  Not built; it needs a trust design of its own.
- **Project scaffolding.** There is no `navree native:init`, `native:run` or
  `native:build`. The clients are packages in the repository
  (`packages/android`, `packages/apple`, each with a demo app); a store build
  of your own app is not packaged yet.

## 8. Which skill next

| You are about to | Load |
|---|---|
| Decide where an action goes, what a screen should contain, review screenshots | `navree-native-design` |
| Work on an iPhone or iPad app, or review it | `navree-native-apple` |
| Work on an Android phone or tablet app, or review it | `navree-native-android` |
| Build or change the pages themselves | `navree-ui` |
| Start from a design or mock-up | `navree-design-to-ui` |
| Sign in from an app: app ids, association files, the sign-in hand-over, confirming an action | `navree-auth` |
| Sign in with Apple or Google from the system sheet | `navree-social` |
| Push notifications to the device | `navree-notification-channels` |
| The commands `native:coverage` and `native:doctor` | `navree-cli-generators` |

## Common mistakes

- Turning the preview on and judging by the first screenshot. Run
  `navree native:coverage` first: it names every gap.
- A page whose `actions` hold default or ghost buttons: they have no role.
- Two `primary` buttons on one screen. The second goes to a menu.
- A creation button written as a big block button in the body.
- A table with no `role = title` column: a phone shows its first column and
  loses the rest.
- Cards in a grid for records. They become rows, but a table says more.
- A form with a hand-placed Save button. Use the form's own submit.
- `native.toml` in `config/`: it is read as a section of the app's settings.
- Using `native::only` to build a second screen. Fix the page's words instead.
- Asking the web view for the camera or location: there is no bridge.
- Claiming an app works on a device because the coverage is green. Coverage
  proves the page reads; the screenshots in `navree-native-design` prove it
  looks right.

## Docs

- https://navree.com/docs/ui/native/
- https://navree.com/docs/digging-deeper/cli/
- https://navree.com/docs/ui/overview/
