---
name: navree-native-design
description: "Makes a Navree screen belong on every system without writing it twice: the rules that hold on iPhone, iPad, Android phones and tablets, each with its reason, a check and a right and a wrong example in Navree UI code (one main action with a role, creation as a role, destructive actions that say so and are never the main action, cancel and confirm as roles, records as lists or tables with column roles, a form's own submit, a task versus a glance, search, filters and sort as the table's query, empty, loading and error states from the toolkit, named icons, a brand that gives an accent and nothing else, text that scales, layouts that mirror, labelled controls), and the review method: where to get each system's screenshots, and how to judge them beside the system's own apps, in light and dark, at the largest text and right to left. Use when designing or reviewing any screen of a Navree app that has a native client, when a native screen looks like a web page, or before calling a native screen done. Load `navree-native` first for the words an author has, then `navree-native-apple` or `navree-native-android` for one system's checklist."
---

# Native design rules

Navree Native draws a page with each system's own components. The server says
what a thing is; the client decides where it goes and how it looks, by that
system's current design guidelines (Apple's Human Interface Guidelines with
Liquid Glass, Material 3 on Android). This skill is the part that is the
same on every system: what a page must say so any client can do it right,
and how to check the result. It does not copy the guidelines; each system
skill summarises them and links their pages.

The words an author has (`Button::role`, `Button::presented`,
`#[col(role = ..)]`, `DataTable::row_opens`, `AppPage::actions`) are in
[`navree-native`](../navree-native/SKILL.md). The rules below say how to use
them. Where the guidelines and a brand disagree, the guidelines win.

## How the rules work

- A rule is something to **follow** and something to **check**: on the page's
  code, in `navree native:coverage`, in `TestNative`, and on a screenshot.
- The placement of each part lives in the client's code, one test per rule
  (`packages/apple/Tests/NavreeDesignTests`, the Android client's
  `DesignRulesTest`). Do not copy a placement into a page. If a client puts
  something in the wrong place, the fix is a rule in the client, reported
  with a screenshot, never a platform branch in the page.
- Different systems place the same role differently, on purpose. Creation is a
  floating action button on Android and a plus in the toolbar on iPhone.
  Do not compare screenshots of two systems for sameness; compare each with
  its own system's apps.

## The rules

### 1. One main action per screen, and it has a role

*Why:* Apple allows one prominent action per toolbar, at its trailing end;
Android puts the first action in the app bar and creation in the floating
action button. Two main actions have no place on either. An action with no
role has no place the client can choose, so the page is only `layout`.
*Check:* `screen.actions` has at most one `Primary`, `Create` or `Confirm`
button and none with role `None`. Coverage flags a missing role ("the action
“Copy link” has no role") but not a second main action: check that in a
`TestNative` test or by eye.

```rust
use navree::ui::{AppPage, AppShell, Button, Text, icons};

/// Right: one main action, the others secondary (`outline`).
pub fn right(shell: AppShell) -> AppPage {
    shell
        .page("Order CS-20481")
        .actions((
            Button::primary("Mark as packed").icon(icons::PACKAGE_CHECK).href("/orders/1/pack"),
            Button::outline("Print label").icon(icons::PRINTER).href("/orders/1/label"),
        ))
        .body(Text::new("Paid, ready to pack."))
}

/// Wrong: two main actions, and a default button that has no role.
pub fn wrong(shell: AppShell) -> AppPage {
    shell
        .page("Order CS-20481")
        .actions((
            Button::primary("Mark as packed").href("/orders/1/pack"),
            Button::primary("Print label").href("/orders/1/label"),
            Button::new("Copy link").href("/orders/1"),
        ))
        .body(Text::new("Paid, ready to pack."))
}
```

### 2. Creation is a role, never a styled button

*Why:* every system has a place for "make a new one" (Android: the floating
action button; iPhone: a plus in the top bar; a Mac: a toolbar item and a menu
command). A big button in the content, or a plus drawn as text, is a web
idiom the clients cannot move.
*Check:* the creating button has `Role::Create`; no `block()` button in the
body whose job is to create.

```rust
use navree::ui::{Button, Role, icons};

/// Right: the role says what it is; each system draws it its way.
pub fn right() -> Button {
    Button::primary("New contact").icon(icons::PLUS).href("/contacts/new").role(Role::Create)
}

/// Wrong: a full-width button placed in the content.
pub fn wrong() -> Button {
    Button::primary("+ New contact").href("/contacts/new").block()
}
```

### 3. Destructive actions say so, are never the main action, and say whether they can be undone

*Why:* Apple's guidelines forbid the primary role for a destructive action
and put it in a section of its own at the end of the content; Android keeps it
out of the floating button and the bar and ends the overflow menu with it, in
the error colour. Both ask first for what cannot be undone, and neither asks
for what can (Apple: "avoid alerts for common, undoable actions"; Android: do
it and offer Undo in the snackbar).
*Check:* a `Button::destructive` (or `Role::Destructive`) is never `primary`;
the question names the thing ("Cancel order CS-20481?") and the answer repeats
the action's words, never "OK".

Navree UI has no word yet for "this can be undone" (the protocol has
`undoable`, the clients honour it). Until it does, every destructive action
asks first. So **reserve the destructive role for what cannot be undone**, and
make a reversible action a plain secondary action that answers with a toast
carrying an Undo action.

```rust
use navree::ui::{AlertDialog, Button, Element, Toast, ToastAction, ToastTone, Uses, el};

/// A form that posts with the runtime, or as a plain form with scripts off.
fn post_form(action: String, button: Button) -> Element {
    el("form")
        .attr("method", "post")
        .attr("action", action)
        .flag("data-ui-form", true)
        .uses(Uses::CORE)
        .child(button)
}

/// Right: final, so it says so, and the question is in the action's own words.
pub fn cancel_order() -> (Button, AlertDialog) {
    (
        Button::destructive("Cancel order").opens_dialog("cancel"),
        AlertDialog::new(
            "cancel",
            "Cancel order CS-20481?",
            "₹5,949 goes back to the customer's UPI account. This can't be undone.",
        )
        .cancel("Keep the order")
        .action(post_form("/orders/1/cancel".to_owned(), Button::destructive("Cancel order").submit())),
    )
}

/// Right, for what can be undone: a plain action, and Undo in the toast it answers with.
pub fn archive_done() -> Toast {
    Toast::new(ToastTone::Success, "Order CS-20481 archived")
        .action(ToastAction::post("Undo", "/orders/1/restore"))
}

/// Wrong: the main action of the screen is the one that deletes.
pub fn wrong() -> Button {
    Button::destructive("Delete order").href("/orders/1/delete")
}
```

### 4. Cancel and confirm are roles, so each system orders them its way

*Why:* Apple puts Cancel at the sheet's top leading corner and Done at the top
trailing corner; Android puts the affirming button at a dialog's end and the
button that backs out before it. A page that orders them for one system is
wrong on the other. An `AlertDialog` already marks its Cancel; a button of your
own says which it is.
*Check:* no screen draws "Cancel" and a confirming button side by side by hand
in a sheet; custom answers carry `Role::Cancel` and `Role::Confirm`.

```rust
use navree::ui::{Button, Role};

/// Right: the roles; the client orders them.
pub fn answers() -> (Button, Button) {
    (
        Button::outline("Keep editing").role(Role::Cancel),
        Button::primary("Send it").role(Role::Confirm),
    )
}
```

### 5. Records are lists or tables with column roles, never cards in a grid

*Why:* a phone shows a record as a row (a title, a line under it, its state,
what stands at its end), a wide window as columns. A card per record in a
grid is the web's habit; no guideline has a place for it on a phone, and it
cannot be sorted, searched or paged.
*Check:* a table has a `role = title` column and its rows open something;
the grade is not `layout` for "has no title column" or "rows open nothing".

```rust
use navree::ui::{Card, Grid, Item, Text, el};

/// Right: a list of records. Each row is one touch target.
pub fn right() -> navree::ui::Element {
    el("ul").children(el("li").child(
        Item::new("Gurpreet Singh").description("Ludhiana · 14 orders").aside("₹1,24,050").href("/customers/7"),
    ))
}

/// Wrong: a card per record in a grid.
pub fn wrong() -> Grid {
    Grid::cols(3).children(Card::titled("Gurpreet Singh").body(Text::new("Ludhiana · 14 orders")))
}
```

For a table, `#[col(role = title)]`, `subtitle`, `status`, `trailing` and
`DataTable::row_opens` are in `navree-native`.

### 6. A form's submit belongs to the form

*Why:* Apple puts a form's confirming action in the toolbar (`Done`, `Save`,
trailing) and forbids a full-width Submit at the bottom of a sheet or pushed
screen; Android finishes a form that is the whole screen from the app bar,
where the keyboard cannot cover it, and from the foot of a dialog. The client
can only do that when the form owns its submit.
*Check:* the submit is `FormView::submit`, not a `Button` you place beside the
form.

```rust
use navree::ui::form::FormView;
use navree::ui::{AppPage, AppShell, Button, Card, Form, Validate};
use serde::Deserialize;

#[derive(Debug, Deserialize, Validate, Form)]
pub struct RestockInput {
    /// Quantity
    #[validate(range(min = 1))]
    pub quantity: i64,
}

/// Right: the form's own submit.
pub fn right(shell: &AppShell, form: FormView<RestockInput>) -> AppPage {
    shell
        .page("Restock")
        .body(Card::new().body(form.post("/inventory/1/restock").submit("Restock")))
}

/// Wrong: a block button of your own under the form.
pub fn wrong(shell: &AppShell, form: FormView<RestockInput>) -> AppPage {
    shell.page("Restock").body(Card::new().body((
        form.post("/inventory/1/restock"),
        Button::primary("Restock").block().href("/inventory/1/restock"),
    )))
}
```

Field errors come back under their field (`Rejected::respond`); do not move
them into a banner. Each system shows them at the field and focuses the first
one in error.

### 7. A task or a glance

*Why:* a sheet that holds a form is a task: the person finishes or cancels it,
so it gets its Cancel and Done and is dismissed with a question when edits
would be lost. A sheet that only shows is a glance: a swipe puts it away. On a
wide window the platforms' own apps show a glance in the detail pane beside
its list and a task over it. The server already sends which a sheet is
(`task` on a sheet that holds a form); neither client uses the difference
yet, so today both draw a sheet as a sheet. Write the page as if they did.
*Check:* a sheet with a form is meant as a task; a record's sheet holds no
form. `Button::presented(Presented::Task)` or `Presented::Sheet` says which
when the shell's routes do not.

```rust
use navree::ui::{Button, Presented, Sheet, Text};

/// Right: a glance holds no form, and the link says it is one.
pub fn glance_link() -> Button {
    Button::outline("Details").href("/orders/1/peek").presented(Presented::Sheet)
}

pub fn glance() -> Sheet {
    Sheet::new("order", "Order CS-20481").body(Text::new("Paid, packed, ships tomorrow."))
}
```

### 8. Search, filters and sort are the table's query

*Why:* the client draws them as its system does (a search bar and filter
chips on Android; a search field under the title or in the toolbar, and a
sort and filter menu, on Apple). They can only do that when they are the
table's own query, because then each field knows what it is for.
*Check:* the table has `search`, `filter` and `sortable` columns and
`.filter_bar()`; there is no loose `Input` or `Select` above it that posts a
form to filter.

```rust
use navree::ui::{DataTable, TablePage, TableQuery, TableRow};

#[derive(Clone, TableRow)]
#[table(sort = "name", per = 25)]
pub struct CustomerRow {
    #[col(skip)]
    pub id: i64,
    #[col(sortable, search, strong, role = title, link = "/customers/{id}")]
    pub name: String,
    #[col(sortable, filter = text, role = subtitle)]
    pub city: String,
}

/// Right: search, filter and sort are the table's own.
pub fn right<'a>(query: &'a TableQuery<CustomerRow>, page: &'a TablePage<CustomerRow>) -> DataTable<'a, CustomerRow> {
    DataTable::new("customers", query, page).caption("Customers").filter_bar()
}
```

### 9. Empty, loading and error states come from the toolkit

*Why:* Apple draws an empty screen with its content-unavailable layout (a
symbol, a title, a description, an action) and names the search in an empty
result; Android draws a centred state with an icon, a title and at most two
actions, a progress indicator after a moment, and a snackbar when a refresh
fails. Only the toolkit's parts say which is which.
*Check:* every list has `.empty(..)`; a region that can fail has a failed
state; nothing is a bare `Text` saying "No results".

```rust
use navree::ui::{Button, Empty, icons};

/// Right: a title, a line, and the one next step.
pub fn no_orders() -> Empty {
    Empty::new("No orders yet")
        .icon(icons::SHOPPING_BAG)
        .description("Orders you take show here.")
        .actions(Button::primary("New order").href("/orders/new"))
}
```

A table has `.empty(..)`, `.loading()` and `.failed(message)`; an error route
is an `ErrorPage` in the shell (`navree-ui`).

### 10. Icons are named so each system uses its own symbol

*Why:* both systems map an icon's name to their own symbol set (SF Symbols,
Material Symbols), filled for the selected tab; a hand-drawn glyph beside
them looks foreign. Standard actions (share, delete, add, edit, search, done,
cancel, more) take the system's symbol whatever the page sent.
*Check:* icons are `icons::` constants; a standard action uses the standard
icon; an icon-only button has a name. The envelope carries each toolkit
icon's name with its SF Symbol and its Material Symbol (`"plus"` is `plus` on
Apple and `add` on Android).

```rust
use navree::ui::{Button, icons};

/// Right: a named icon and the accessible name icon-only buttons need.
pub fn search() -> Button {
    Button::icon_only(icons::SEARCH, "Search orders").href("/orders?focus=search")
}
```

Do not draw your own SVG (`icon_file!`) for an action a system has a symbol
for. Give every top-level `NavItem` an icon: the tab bar and the navigation
bar show only that.

### 11. A brand gives an accent colour and the system keeps its own materials, type and shapes

*Why:* Apple's guidelines move the brand into the content layer and the accent;
bars, tab bars and sheets keep the system's materials (Liquid Glass). Android
builds a Material 3 palette from the brand's seed and keeps the system's type
and shapes. A native client takes the theme's accent, for light and for dark,
and nothing else.
*Check:* the theme is one of the built-in themes or a `Theme::custom` that
passes contrast; no page asks for glass, blur, corner radii or fonts for bars.

```rust
use navree::ui::theme::{Appearance, Mode, Theme};
use navree::ui::{NavGroup, NavItem, Shell, icons};

/// The accent comes from the theme.
pub fn shell() -> Shell {
    Shell::new("Sutlej Supply")
        .appearance(Appearance::new(Theme::OCEAN, Mode::System))
        .group(NavGroup::new("Store").item(NavItem::new("Orders", "/orders").icon(icons::SHOPPING_BAG)))
}
```

### 12. Text scales, layouts mirror, every control has a label

*Why:* both systems scale text with the person's setting (Apple's Dynamic
Type up to the largest accessibility size, Android's 200% font scale) and
mirror layouts for right-to-left languages; screen readers read the labels.
*Check:* nothing important is cut at the largest text size; nothing is
left- or right-aligned by hand; every field has a label and every icon-only
button a name; a status is words, not a colour alone (`Badge::new("Overdue")`
with a tone). Use `Stack`, `Row` and `Grid`, which reflow; do not fix widths.

## What a client does differently on purpose

| A page says | Apple (iPhone, iPad) | Android (phone, tablet) |
|---|---|---|
| `Role::Create` | a plus in the top bar | the floating action button; beside a rail, at its head |
| `Primary` action | the toolbar's one prominent item, trailing | the first icon in the app bar |
| `Destructive` | a section of its own, in red, at the end of the content | the end of the overflow menu, in the error colour |
| a toast | a line of status in the screen (an alert only for a failure) | a snackbar, with the toast's action |
| a table | rows on a phone; columns in a wide window | a list below 600 dp; rows of cells above |
| `Cancel` and `Confirm` | top leading and top trailing of a sheet | before and at the end of a dialog |
| search | under the title, or in the toolbar on a wide window | a search bar in the app bar's place |

## Review a native screen

1. **Grade it.** `navree native:coverage` must say `idiom` for the page. A
   `layout` or `web` row is not ready to review: fix what it names.
2. **Get the screenshots.** `.github/workflows/native.yml` runs for changes to
   the clients and the protocol and keeps each run's screens as artifacts:
   `android-screens-<device>` (a phone and a tablet emulator, each in the
   looks light, dark, large text at 200% and right to left, files named
   `<device>-<look>-<screen>.png`) and `apple-screens-xcode-<version>` (an
   iPhone and an iPad simulator, light, dark, light and dark at the largest
   text, right to left). List runs with `gh run list --workflow native.yml`,
   download with `gh run download <run id> -p 'android-screens-*' -D <dir>`.
   For your own app, the jobs' scripts are the model: boot a simulator or
   emulator, set the look (`simctl ui` and the launch arguments on Apple;
   `cmd uimode night` and `font_scale` on Android), run the app, take
   screenshots. These are simulator and emulator pictures; they prove the
   layout, not the feel on a device.
3. **Judge each screenshot** beside the system's own apps, not beside the
   web page or the other system: Mail, Notes, Contacts and Settings on Apple;
   Gmail, Google Play, Contacts and Settings on Android. Does a person who
   knows that system find every control where they expect it?
4. **Run the system skill's checklist**: the yes/no checks in
   `navree-native-apple` and `navree-native-android`. Every "no" is a defect
   unless the check says otherwise.
5. **Run it in every look**: light and dark; the largest text; right to left;
   and, on Apple, Increase Contrast and Reduce Transparency. A screen that
   passes only in light is not done.
6. **Record the result**: for each defect, the screenshot, the check number
   and the fix. Fix it in this order: the page's words (a role, a column
   role, `presented`), then a missing word (report it), then the client's
   rule (with a test). Never by hand-placing on the page.

## Common mistakes

- Reviewing the web page in a phone-sized browser. It is not the native
  screen.
- Fixing a native screen by adding `native::only` parts. Say what the parts
  are.
- Asking for an Apple look on Android or the reverse. Each is judged on its
  own.
- Using a brand's whole palette on bars. The client takes the accent only.
- Calling a screen done on the simulator's light pass alone.
- Writing "Cancel" and "OK" buttons into a sheet's body.
- Making "Delete" the one prominent button because it is the common choice.
- Building a custom tab bar, toolbar or sheet out of web parts. The system
  has one.

## Docs

- https://navree.com/docs/ui/native/
- https://navree.com/docs/ui/overview/
