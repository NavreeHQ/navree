---
name: navree-ui
description: "Builds Navree UI pages from scratch, with no design to start from: turns on `UiModule`, registers one `Shell`, writes page routes (`AppShell`, `AppPage`) with an access attribute each, forms from `#[derive(Validate, Form)]` read back with `Submit<T>` (rejected forms and field errors under the inputs), lists with `DataTable` and `#[derive(TableRow)]` (filters, sorting, bulk actions, CSV export, cursor pages), regions that load, poll or follow a stream, long work shown with `Progress`, error pages (`ErrorPage`, `SignInRequired`, `Forbidden`, `NotFound`, `EmptyState`, the `page` and `json_only` marks on a route), themes in light, dark and system mode with the theme menu, dates and times (`Calendar`, `DatePicker`, `DateRangePicker`, `TimePicker`, `DateTimePicker`), the selects (`Select`, `Combobox`, `MultiSelect`, `Command`), `PhoneInput`, `LazyList`, `SplitPane`, `Timeline`, `ActivityFeed`, `Sortable`, the wizard, stepper, toolbar, tree and slider, and page tests with `TestView` and no browser. Use when asked to add a page, a list page, a form, a table, a dashboard or a settings screen in Navree UI (`navree-ui`, `navree::ui`), to change one, or to find which Navree UI component fits a need. Do not call a component missing before checking the index in this skill. For a job that starts from a design, mock-up or screenshots, use `navree-design-to-ui`."
---

# Build Navree UI pages

Navree UI (`navree::ui`, the `navree-ui` crate) draws screens in Rust on the
server. A handler returns typed components, the toolkit renders them to
HTML, and a page with nothing interactive ships no JavaScript. This skill
builds pages from nothing: a shell, a page, a list, a form, and the parts
around them.

- A design, mock-up or screenshots to match: use
  [`navree-design-to-ui`](../navree-design-to-ui/SKILL.md). It maps every
  element of a design to the components listed here and checks the pages
  beside the design.
- Screens in React or Vue: use `navree-frontend-next` or
  `navree-frontend-nuxt`. Both talk to the same API.
- The pages of an admin panel: `navree-helm` draws them for you.

## The rules

1. **Use components that exist.** The index at the end of this skill lists
   every one. A need that is not in it is a gap: say so, and do not make a
   stand-in from raw elements, a stylesheet or a script. The toolkit's
   stylesheet reads only `data-*` attributes, so a `class` does nothing
   unless the app's own stylesheet (`AppSheet`) styles it, and `style` is
   dropped. The content security policy refuses inline script and style.
2. **Every route has an access attribute.** A page route still needs
   `#[public]`, `#[authenticated]`, `#[roles(..)]`, `#[permissions(..)]` or
   `#[can(..)]`, or it does not compile.
3. **The server decides.** Lists page, sort, filter and search on the
   server. Options for a large set come from a route that searches. Every
   rule runs on the server, even when the browser also shows it.
4. **A page route starts with `!`** (`#[get("!/contacts")]`), so it sits at
   the site's root. A route without `!` sits under `http.api_prefix`
   (`/api`), where errors are JSON.
5. **Corners are 4 px on controls and 6 px on surfaces. No pills, no letter
   tiles, no glow.** The components already follow this. Do not override it.
6. **Every page has its other states**: empty, loading, failed, read-only and
   forbidden. Build them with the components below.
7. **Test each page** with `TestView` (no browser), and look at it at 1440
   and at 390 pixels wide.

## 1. Turn it on

The `ui` feature of `navree` is on by default. Add `UiModule::from_config()`
after `HttpModule::from_config()`, and register one `Shell` as a provider.
Name `UiModule` yourself: it serves the toolkit's files, answers each
`Reply` the way the request asked, and draws error pages. A module that
borrows the toolkit with `UiOnDemand` keeps JSON errors.

```rust
use navree::ui::{NavGroup, NavItem, Shell, UiModule, UserMenu, icons};

/// The app's one shell: the sidebar, the top bar and the user menu.
pub fn shell() -> Shell {
    Shell::new("Sutlej CRM")
        .context("Production")
        .group(
            NavGroup::new("Sales")
                .item(NavItem::new("Home", "/").icon(icons::LAYOUT_DASHBOARD))
                .item(NavItem::new("Contacts", "/contacts").icon(icons::USERS)),
        )
        .user(
            UserMenu::new("Gurpreet Singh", "gurpreet@example.com")
                .link("Profile", "/profile")
                .action("Sign out", "/sign-out"), // a POST: a hover fetches nothing that signs anyone out
        )
}

#[module(register = Self::wire)]
pub struct FrameModule;

impl FrameModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.factory(Factory::<Shell>::new(|_| async { Ok(Arc::new(shell())) }))
            .exports::<Shell>();
    }
}

/// The app root: the toolkit comes after the HTTP module.
pub fn app(sources: ConfigSources) -> AppBuilder {
    Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(HttpModule::from_config())
        .module(UiModule::from_config())
        .module(FrameModule)
}
```

Give every top-level `NavItem` an icon: the collapsed sidebar shows only
that. `Shell::search(node)` puts a search form or a command palette in the
top bar. A module that serves pages of its own and must not register a
shell for the whole app builds its own `Shell` and uses
`Shell::for_request(path, &headers)`.

## 2. A first page

A handler takes the `AppShell` extractor and returns an `AppPage`.

```rust
use navree::ui::{AppPage, AppShell, Button, Card, Text};

#[injectable]
pub struct HomePages;

#[controller]
impl HomePages {
    #[get("!/")]
    #[public]
    async fn home(&self, shell: AppShell) -> Result<AppPage> {
        Ok(shell
            .page("Home")
            .actions(Button::primary("Contacts").href("/contacts"))
            .body(Card::titled("Welcome").body(Text::new("Sutlej Supply's customers, in one place."))))
    }
}

#[module(controllers = [HomePages])]
pub struct HomeModule;
```

`shell.page(title)` builds the page; `.crumb(label, href)` adds a
breadcrumb, `.actions(..)` the buttons beside the title (one primary action
per page) and `.body(..)` the content. A page outside the shell (a sign-in
page, a receipt) is `View::new(title).body(..)`. Add `HomeModule` to the app
root with `.module(HomeModule)`. Components carry no outer margin: space
between them comes from `Stack`, `Row` and `Grid`.

## 3. A list page and a form page

The order of work for a screen that lists records and adds new ones:

1. The model and its migration (`navree-orm`, `navree-migrations`).
2. Who may read and create it (`navree-access-control`).
3. A `TableRow` struct for the list and a `Form` struct for the form. Keep
   them apart when the list shows less than the model, or names from other
   tables.
4. The page controller, the module that lists it, and a link in the shell.
5. Tests, then a look at the page at 1440 and 390 pixels.

```rust
use chrono::{DateTime, Utc};
use navree::access::{Accessible, Create, Read};
use navree::db::Uow;
use navree::http::response::{IntoResponse, Response};
use navree::orm::Record;
use navree::orm::access::AccessibleQuery as _;
use navree::ui::form::{FormView, Options as _, Rejected};
use navree::ui::{
    AppPage, AppShell, Button, Card, DataTable, Empty, Form, Options, Reply, Submit, TableQuery,
    TableRow, ToastTone, Validate, icons,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Where a contact lives. The value stored is the snake case name.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Options)]
#[serde(rename_all = "snake_case")]
pub enum City {
    #[default]
    Ludhiana,
    Jalandhar,
    Amritsar,
    Chandigarh,
    Jaipur,
    Delhi,
}

#[derive(Model, Clone, Debug, Default, Serialize)]
#[model(table = "contacts", timestamps, subject = "Contact", permission_prefix = "contacts")]
pub struct Contact {
    pub id: Uuid,
    pub name: String,
    pub email: String,
    #[model(cast = "string_enum")]
    pub city: City,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// The city filter's choices: the values stored, with their labels.
pub const CITIES: &[(&str, &str)] = City::OPTIONS;

/// A contact as the list shows it. The columns say what each can do.
#[derive(Clone, TableRow)]
#[table(sort = "name", per = 25, per_pages(10, 25, 50, 100))]
pub struct ContactRow {
    #[col(skip)]
    pub id: Uuid,
    /// Name
    #[col(sortable, search, sticky, strong, filter = text, link = "/contacts/{id}")]
    pub name: String,
    /// Email
    #[col(sortable, search, filter = text)]
    pub email: String,
    #[col(sortable, filter = multi, options = CITIES)]
    pub city: &'static str,
    /// Added
    #[col(sortable, hide_on_mobile)]
    pub created_at: DateTime<Utc>,
}

impl ContactRow {
    pub fn from_record(record: Record<Contact>) -> Self {
        let contact = record.into_inner();
        Self {
            id: contact.id,
            name: contact.name,
            email: contact.email,
            city: contact.city.option_value(),
            created_at: contact.created_at,
        }
    }
}

/// The new-contact form. Doc comments are the labels, types pick the
/// controls, and the rules run in the browser and again on the server.
#[derive(Debug, Deserialize, Validate, Form)]
pub struct ContactInput {
    /// Full name
    #[validate(length(min = 1, max = 120))]
    #[form(placeholder = "Gurpreet Singh", autofocus, wide, error = "Enter the contact's name.")]
    pub name: String,
    /// Email
    #[validate(email)]
    #[form(widget = email, placeholder = "gurpreet@example.com", error = "Enter an email address like gurpreet@example.com.")]
    pub email: String,
    /// City
    #[validate(skip)]
    pub city: City,
}

/// The new-contact page, empty or sent back with errors.
fn form_page(shell: &AppShell, form: FormView<ContactInput>) -> AppPage {
    shell
        .page("New contact")
        .crumb("Contacts", "/contacts")
        .body(Card::new().body(form.post("/contacts").submit("Add contact").cancel("Cancel", "/contacts")))
}

#[injectable]
pub struct ContactPages;

#[controller]
impl ContactPages {
    /// The list: search, filters, sort and pages live in the address.
    #[get("!/contacts")]
    #[can(Read, Contact)]
    async fn index(
        &self,
        shell: AppShell,
        uow: Uow,
        scope: Accessible<Contact, Read>,
        query: TableQuery<ContactRow>,
    ) -> Result<AppPage> {
        let page = query.load_as(scope.query(), &uow, ContactRow::from_record).await?;
        Ok(shell
            .page("Contacts")
            .actions(Button::primary("New contact").icon(icons::PLUS).href("/contacts/new"))
            .body(
                DataTable::new("contacts", &query, &page)
                    .caption("Contacts")
                    .sticky_header()
                    .empty(
                        Empty::new("No contacts yet")
                            .icon(icons::USERS)
                            .description("Contacts you add show here.")
                            .actions(Button::primary("New contact").href("/contacts/new")),
                    ),
            ))
    }

    #[get("!/contacts/new")]
    #[can(Create, Contact)]
    async fn new(&self, shell: AppShell) -> Result<AppPage> {
        Ok(form_page(&shell, FormView::new()))
    }

    /// Saves the form. With errors, the same page again, each message under its field.
    #[post("!/contacts")]
    #[can(Create, Contact)]
    async fn store(
        &self,
        shell: AppShell,
        uow: Uow,
        submit: Result<Submit<ContactInput>, Rejected<ContactInput>>,
    ) -> Result<Response> {
        match submit {
            Ok(Submit(input)) => {
                let contact = Contact::create(
                    &uow,
                    Contact {
                        id: Uuid::now_v7(),
                        name: input.name,
                        email: input.email,
                        city: input.city,
                        ..Contact::default()
                    },
                )
                .await?;
                Ok(Reply::redirect("/contacts")
                    .toast(ToastTone::Success, format!("{} added", contact.name))
                    .into_response())
            }
            Err(rejected) => Ok(rejected.respond(|form| form_page(&shell, form))),
        }
    }
}

#[module(
    imports = [AccessModule::for_feature::<Contact>()],
    controllers = [ContactPages],
)]
pub struct ContactsModule;
```

What each part does:

- `TableQuery<ContactRow>` reads `q`, `sort`, `page`, `per` and the filters
  from the address. `load_as` runs them on your scoped query, so the user
  sees only the rows they may read. Back, forward, reload and a shared link
  restore the list, with JavaScript on or off.
- `Submit<ContactInput>` refuses a body from another site (403), a body
  that is not a form (415) and a `GET` (405). It reads only the struct's
  own fields. `Rejected` draws your page again with what was typed and each
  error under its field: `422` with the page when JavaScript is off, `422`
  with an `errors` op when it is on.
- `Reply::redirect` stays on this site, and `.toast(..)` shows on the next
  page, with JavaScript on or off.
- Behind a session, give the form its token for the case of JavaScript off:
  `FormView::csrf(session.csrf_token())`. The runtime sends the token as a
  header itself.
- A page that needs a company picked from thousands of rows, an edit page
  and a detail page with a part that loads later are built in
  [`building-pages.md`](../navree-design-to-ui/references/building-pages.md),
  in code that compiles.

## 4. Errors on a page route

`UiModule` registers the renderer of error pages (the `ErrorPages` trait of
navree-http, contributed with `m.error_pages(..)`). A request for a page that
ends in a 401, 403, 404, 405, 409, 410, 413, 419, 422, 429, a 5xx or a refused
CSRF token gets a page, in the shell for a signed-in user, with the next step
and a reference to quote. An API route keeps Navree's JSON error. A route's
mark decides; for a route with no mark the `Accept` header decides.

| The request | The answer |
|---|---|
| A route marked `#[json_only]` (`RouteDef::json_only()`) | the JSON error, for every client |
| A client that asks for JSON alone | the JSON error |
| A route under `http.api_prefix` with no `#[page]` mark | the JSON error, even for a browser |
| A route marked `#[page]` (`RouteDef::page()`) | a page, unless the client asks for JSON alone |
| A browser opening a page | a page |
| A region fetching its content | the state alone, where the region was |
| A form posted through the runtime | an ops answer with one toast |

```rust
use navree::ui::{AppPage, AppShell, Text};
use uuid::Uuid;

#[injectable]
pub struct ExportPages;

#[controller]
#[page]
impl ExportPages {
    /// Marked `#[page]` for the whole controller: its errors are pages.
    #[get("!/exports")]
    #[authenticated]
    async fn index(&self, shell: AppShell) -> Result<AppPage> {
        Ok(shell.page("Exports").body(Text::new("Your exports show here.")))
    }

    /// A download inside a page controller. Its errors stay JSON.
    #[get("!/exports/{id}/file")]
    #[authenticated]
    #[json_only]
    async fn file(&self, Path(id): Path<Uuid>) -> Result<String> {
        Ok(format!("export {id}"))
    }
}
```

Set where "Sign in" and "Home" lead, and the words, when you add the module.
The `[ui]` section of the configuration (`sign_in`, `sign_in_redirect`,
`debug`) overrides the options. A 5xx never shows its source, SQL or paths.
In `production` the development details are never shown.

```rust
use navree::ui::UiModule;
use navree::ui::pages::PagesOptions;

pub fn ui() -> impl Module {
    UiModule::with_pages(PagesOptions::default().sign_in("/sign-in").home("/contacts").redirect_guests(false))
}
```

A signed-in user gets the error page inside the shell. An app that builds
its shell for each request contributes an `ErrorShell`, which gets the
request's `ErrorContext` and has one second to answer:

```rust
use navree::ui::{ErrorShell, NavGroup, NavItem, Shell, icons};
use uuid::Uuid;

async fn shell_for(user: Option<Uuid>) -> Option<Shell> {
    user.map(|_| {
        Shell::new("Sutlej CRM")
            .group(NavGroup::plain().item(NavItem::new("Contacts", "/contacts").icon(icons::USERS)))
    })
}

#[module(register = Self::wire)]
pub struct ErrorsModule;

impl ErrorsModule {
    fn wire(&self, m: &mut ModuleBuilder) {
        m.contribute(ErrorShell::new(|request| async move { shell_for(request.user).await }));
    }
}
```

The same markup fits places inside a page. `Forbidden` is for a card, tab
or row the user may not see (it never says whether the thing exists),
`NotFound` for a record that is gone, `SignInRequired` for a part that
needs a user, `EmptyState` for a list with nothing in it, and `ErrorPage`
for a whole page. In a table, `.row(columns)` on a state draws it as a row.

```rust
use navree::ui::{Button, Card, EmptyState, Forbidden, NotFound, SignInRequired, Stack, icons};

pub fn states() -> Stack {
    Stack::gap(4).children((
        Card::titled("Revenue").body(Forbidden::new().what("report")),
        Card::titled("Owner").body(NotFound::new().what("deal")),
        Card::titled("Your deals").body(SignInRequired::new("/sign-in").what("your deals")),
        EmptyState::nothing_yet("deals")
            .description("Create a deal to start tracking your pipeline.")
            .action(Button::primary("New deal").icon(icons::PLUS)),
        EmptyState::no_results().clear_filters("/deals"),
    ))
}
```

Your sign-in page reads `next` with `ReturnTo`, which accepts only a path on
your site. A 419 is the page-expired state: return `AppError::page_expired()`
for a 419 of your own. The JSON side is in `navree-errors-and-responses`.

## 5. Forms

One struct is the form, its rules, its errors and its input. The field type
picks the control:

| Field type | Control |
|---|---|
| `String` | a text input; `widget = email \| url \| tel \| password \| search \| textarea` change it |
| integers, floats, `Decimal` | a number input |
| `bool` | a checkbox; `widget = switch` |
| `Money` | an amount typed like `1,00,000.50`, stored in its smallest unit |
| `NaiveDate`, `NaiveTime`, `NaiveDateTime` | the browser's own input; `widget = date_picker` or `time_picker` for the pickers ([`pickers-and-dates.md`](references/pickers-and-dates.md)) |
| `Phone`, `PhoneNumber` | a `PhoneInput` (same file) |
| an enum with `#[derive(Options)]` | a select of its variants; `widget = radio` for radio buttons |
| `String` with `widget = select`, `listbox` or `combobox` | a select over choices from `FormView::options`, or a searched list |
| `Many<T>` | a `MultiSelect` |
| `Option<T>` | the same control, and it may be left empty |

`#[form(...)]` takes `label`, `placeholder`, `help`, `prompt`, `error`
(replaces any failed rule's message), `addon = "field"`, `rows`, `max`,
`autofocus`, `wide` and `skip` (never drawn, never read). `FormView` draws
the form: `.post(url)`, `.submit(label)`, `.cancel(label, href)`,
`.options(field, choices)`, `.choices(field, choices)`, `.search(field,
url)`, `.one_column()`, `.csrf(token)`.

A picker reads back as text, and the form does not know the value was one of
the options. Check it on the server before you save:
`OptionSource::all_pickable(scoped_query, values, &uow, OptionSource::as_uuid)`,
or `Choice::all_among(values, &choices)` for a list held in the page, and
answer a failure with `Rejected::after_decode`. `building-pages.md`
(linked above) has the whole route.

A rejected form keeps what was typed, never writes a password back and
holds names and errors, not values, in its `Debug`. Rules run once every
field has the right type, so type errors come first.

## 6. Tables

`DataTable` draws a `TableQuery` and a `TablePage`. `#[derive(TableRow)]`
says what each column can do, and the same definition parses the address,
builds the query with an allow-list, draws the table, exports the file and
checks bulk actions.

| Ask | Use |
|---|---|
| Sort, several columns at once | `#[col(sortable)]`; the address holds `sort=-amount,title` |
| Search box | `#[col(search)]` on the searched columns |
| Filter per column | `#[col(filter = text \| range \| date \| select \| multi \| boolean \| relation)]`, choices from `options = LIST` or `.options(col, Vec<FilterOption>)` |
| Filters under the headers or in a bar | `.filter_row()` or `.filter_bar()` |
| Page size | `#[table(per = 25, per_pages(10, 25, 50, 100))]` |
| Huge tables | `#[table(cursor)]` for keyset pages; `capped = n` or `no_count` to stop counting |
| Status badge, link, row link | `badge = fn`, `link = "/contacts/{id}"`, `.row_href(fn)` |
| Row menu | `.row_actions(fn)` with `RowAction::link`, `RowAction::post`, `.destructive()` |
| Select rows, act on them | `.bulk(url, [BulkAction::new(id, label)..])` and `BulkRequest` |
| Export | `.export(url)` and `TableQuery::export_csv` |
| Saved views | `.views(&views, url)`; the table `navree_ui_table_views` comes from your migration |
| Empty, loading, failed | `.empty(..)`, `.loading()`, `.failed(message)` |

```rust
use http::StatusCode;
use navree::ui::{BulkAction, BulkRequest, RowAction, TablePage};

fn contact_actions(row: &ContactRow) -> Vec<RowAction> {
    vec![
        RowAction::link("Edit", format!("/contacts/{}/edit", row.id)),
        RowAction::post("Delete", format!("/contacts/{}/delete", row.id)).destructive(),
    ]
}

/// The list page with its filters in a bar, a menu on each row, bulk delete and an export.
pub fn contacts_page(shell: &AppShell, query: &TableQuery<ContactRow>, page: &TablePage<ContactRow>) -> AppPage {
    shell.page("Contacts").body(
        DataTable::new("contacts", query, page)
            .caption("Contacts")
            .filter_bar()
            .export("/contacts/export")
            .bulk(
                "/contacts/bulk",
                [BulkAction::new("delete", "Delete").destructive().confirm("Delete {n} contacts?")],
            )
            .row_actions(contact_actions)
            .keyboard_grid(),
    )
}

#[injectable]
pub struct ContactTools;

#[controller]
impl ContactTools {
    /// The rows of the current search, filters and sort, as a CSV file.
    #[get("!/contacts/export")]
    #[can(Read, Contact)]
    async fn export(
        &self,
        uow: Uow,
        scope: Accessible<Contact, Read>,
        query: TableQuery<ContactRow>,
    ) -> Result<Response> {
        Ok(query
            .export_csv(scope.query(), &uow, "contacts.csv", ContactRow::from_record)
            .await?)
    }

    /// Acts on the selection. `resolve` runs the table's own definition on your scoped
    /// query and refuses when the count the user saw has changed.
    #[post("!/contacts/bulk")]
    #[can(Delete, Contact)]
    async fn bulk(
        &self,
        uow: Uow,
        scope: Accessible<Contact, Delete>,
        request: BulkRequest<ContactRow>,
    ) -> Result<Reply> {
        let target = match request.resolve(scope.query(), &uow).await? {
            Ok(target) => target,
            Err(refusal) => return Ok(request.refused(StatusCode::CONFLICT, refusal.message())),
        };
        let count = target.delete(&uow).await?;
        Ok(request.done(format!("Deleted {count} contacts.")))
    }
}
```

Facts to keep in mind:

- **One table per URL.** Its state lives in plain query parameters. Give a
  second table its own route or a `Region`.
- **Keep it paged.** 25 rows a page is the default. A table that must hold
  millions of rows uses `#[table(cursor)]` and an index per sort column.
- **A bulk action writes with one statement** over the selected rows. A
  statement like that skips the change events the audit log records (see
  `navree-orm`). When the log must see each change, load the records and save
  or delete each one through the unit of work.
- **NULL is the smallest value** on every store: ascending puts NULLs first.
- **Excel**: hand the rows from `export_rows` to `navree-sheets`.
- **A cell drawn by you**: `#[col(cell = draw)]` or `.cell(name, fn)`;
  `EditCell::text`, `number`, `date` and `select` edit in place.

## 7. The rest of the toolkit

Each file below has code that compiles, for the components it names.

| Need | Read |
|---|---|
| A part of a page that loads after paint, polls or follows a stream (`Region`); what a button does with no request (`Cmd`); what an action answers with (`Reply`, ops, toasts); long work followed with `Progress` | [`references/updating-parts.md`](references/updating-parts.md) |
| Light, dark and the device's mode; the seven themes and the theme menu; shell layouts; your own theme; icons and flags | [`references/themes.md`](references/themes.md) |
| `Select`, `Combobox`, `MultiSelect`, the `Command` palette; `Calendar`, `DatePicker`, `DateRangePicker`, `TimePicker`, `DateTimePicker`; `PhoneInput` | [`references/pickers-and-dates.md`](references/pickers-and-dates.md) |
| `LazyList`, `SplitPane`, `Timeline`, `ActivityFeed`; `Sortable` lists and boards; `Wizard`, `Stepper`, `Slider`, `TreeView`, `Toolbar` | [`references/display-and-flow.md`](references/display-and-flow.md) |
| Rings, gauges and the day clock; meters, mini columns, colour strips and range scales; heat maps, the relation list and the scatter chart; gradients, colour by value, motion, axes, reference lines and zones | [`references/charts.md`](references/charts.md) |
| The app's own stylesheet, fonts and mark; figure tiles, the info tip, chart skeletons, the breathing guide, the dock, the day pager, the copy field, page titles; a marketing site, its SEO, sitemap and robots file; one address for guests and signed-in readers; the sign-in pages in the app's look | [`references/app-and-site-blocks.md`](references/app-and-site-blocks.md) |

## 8. Test without a browser

`TestView` drives a page in a plain test: what it shows, where a click goes,
what a form would post, and the form read back as your struct. Build the
page from a function that takes the shell, so a test can make one with
`Shell::for_request`.

```rust
use http::HeaderMap;
use navree::ui::form::FormView;
use navree::ui::test_view::Clicked;
use navree::ui::{Button, Empty, Shell, TestView};

#[test]
fn the_contact_form_refuses_a_bad_email_and_reads_a_good_one() {
    let app_shell = shell().for_request("/contacts/new", &HeaderMap::new());
    let page = TestView::of(&form_page(&app_shell, FormView::new()).to_view());
    assert!(page.sees("Full name"));

    let bad = page.clone().fill("name", "Gurpreet Singh").fill("email", "not an email");
    assert!(bad.submit_as::<ContactInput>().unwrap_err().contains_key("email"));

    let good = page
        .fill("name", "Gurpreet Singh")
        .fill("email", "gurpreet@example.com")
        .submit_as::<ContactInput>()
        .unwrap();
    assert_eq!(good.name, "Gurpreet Singh");
}

#[test]
fn the_empty_list_offers_the_first_action() {
    let view = Shell::new("Sutlej CRM")
        .page("Contacts", "/contacts")
        .body(Empty::new("No contacts yet").actions(Button::primary("New contact").href("/contacts/new")))
        .to_view();
    let page = TestView::of(&view);
    assert!(page.sees("No contacts yet"));
    assert_eq!(page.click("New contact"), Clicked::Link("/contacts/new".to_owned()));
}
```

`shows_error(Some("email"))` says whether a field shows an error, and
`TestView::of_node(node)` tests a part of a page. Routes, access and the
database get HTTP tests with `TestApp` (`navree-testing`): each page as each
role answers 200, 403 or 404, each form posts and redirects, each bulk action
refuses a stale count. What only a browser can prove (layout, focus, the
phone width) belongs to `navree-browser-testing`.

## Checks to run

```sh
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
navree check          # the module graph
navree routes         # every route with its access rule
navree dev            # then open the page, at 1440 and at 390 pixels wide
```

For each page, also check: the keyboard reaches every control in order with
a visible focus ring; the list works with JavaScript off; the empty, failed,
read-only and forbidden states show; nothing scrolls sideways at 390 pixels.

## Signing in, workspaces and the person in the shell

A browser signs in on the session cookie, through the same steps as a token
client (the throttle, two-factor authentication, the sign-in policies, the
audit trail). Add the pages to the app root:

```rust,ignore
use navree::auth::{AuthModule, AuthPages};
use navree::session::SessionModule;

Navree::app()
    // ConfigModule, CryptoModule, HttpModule, DatabaseModule ...
    .module(UiModule::from_config())
    .module(SessionModule::from_config())
    .module(AuthModule::from_config())
    .module(AuthPages::new().app_name("Sutlej CRM"))
```

`AuthPages` draws `/sign-in` (with "remember me"), `/sign-in/two-factor`,
`/forgot-password`, `/reset-password`, `POST /sign-out`, the setup page
`/account/two-factor` (a secret to scan, the first code, recovery codes shown
once; a user a two-factor policy requires it of is sent there after signing
in) and `POST /impersonate/stop`. With `auth.magic_link.enabled`,
`auth.otp.enabled` or `auth.passkeys.enabled` it adds `/sign-in/link` (a
mailed link that opens a page with one button), `/sign-in/code` and
`/sign-in/passkey`, and the sign-in page links to them. Settings that matter:
`auth.password_reset.url` (the reset page's address), `auth.magic_link.url`
(`https://app.example.com/sign-in/link/verify`), `session.secure = true` and
`session.host_prefix` in production, `ui.sign_in_redirect = true` so a guest
who asks for a page is sent to sign in and back. A page route is
`.authenticated().page()`; its forms carry the session's token (`FormView::csrf`).

A single-page app that keeps its browser on the cookie posts JSON to
`POST /api/auth/session/login` (`{email, password, remember?}`; the answer is
the user, or `{"two_factor_required": true}`) and
`POST /api/auth/session/2fa/verify`, with the `X-XSRF-TOKEN` header; no token
is issued. An administrator with the impersonation permission calls
`POST /api/auth/impersonate/{user}` from a signed-in browser and the session
becomes the user's (the shell shows "Stop impersonating", a post to
`/impersonate/stop`).

Workspaces: add `WorkspaceModule::from_config()` and set
`workspace.resolvers = ["session", "header"]` and
`workspace.session_default = true` (a browser that has chosen none is in the
first workspace its account belongs to). `WorkspaceSwitcher` is an
extractor and a component (`AppPage::actions(switcher)`); `CurrentWorkspace`
gives the workspace, the role and `has_permission`; read and write a
workspace's rows through `WorkspaceRepo::<Model>`.

The shell is registered once and static, so draw the person and the
workspace per request: clone the registered `Shell`, set
`.context(workspace.name)` and `.user(UserMenu::new(name, email)
.link("Profile", "/profile").action("Sign out", "/sign-out"))`, and apply it
with `shell.for_request(path, &headers)`. An app made by `navree new` does this
in one extractor, `Desk` (`src/ui/shell.rs`): take `Desk` where this skill says
`AppShell`.

## What is not shipped

Do not write code for these, and do not stand in for them with raw elements.
Tell the owner they are coming.

- **A workspace switcher in the shell's sidebar header.**
  `navree_workspace::WorkspaceSwitcher` draws one; put it beside the page
  heading (`AppPage::actions`) or in the shell's search slot, and show the
  workspace's name under the app's with `Shell::context`.
- **A user menu that follows the request.** `Shell::user` is the same for
  every request: build the shell per request (see above).
- **Rich text editing and islands** (a part of a page with its own
  JavaScript component).
- **Static files past the app's own stylesheet, fonts and pictures.** The
  content security policy loads styles, scripts, fonts and images only from
  the app's own origin: an `AppSheet`, `AppFont` or `AppImage` is compiled in
  and served by the toolkit (see
  [`references/app-and-site-blocks.md`](references/app-and-site-blocks.md)).

Navigator (`navree-navigator`) reads the UI docs through `search_docs` and
`read_doc` (slugs such as `ui/forms` and `components/date-picker`),
`list_components` lists the components of the toolkit, `list_routes` with
`pages_only` lists the pages, and `render_page` outlines what a page draws
for a guest.

## Every component

Check this index before you call anything missing. The group and the page
come from the component catalog of the docs
(https://navree.com/docs/components/overview/).

| Group | Components |
|---|---|
| Actions | `Button`, `ButtonGroup`, `Toggle`, `ToggleGroup`, `DropdownMenu`, `ContextMenu`, `Menubar` |
| Inputs | `Field`, `Input`, `InputGroup`, `Textarea`, `NativeSelect`, `Select`, `Combobox`, `MultiSelect`, `Checkbox`, `Switch`, `RadioGroup`, `PhoneInput`, `InputOtp`, `ColorPicker`, `Upload`, `Questionnaire`, and Forms (`FormView`, `Submit`) |
| Date and time | `Calendar`, `DatePicker`, `DateRangePicker`, `TimePicker`, `DateTimePicker` |
| Display | `Card`, `Badge`, `Avatar`, `AvatarGroup`, `Item`, Heading and Text, `Kbd`, `Attachment`, Message and Bubble, `Timeline`, `ActivityFeed`, `Trace`, FactCard and FactGrid, `Markdown`, `CodeBlock` |
| Feedback | `Alert`, `Toast`, `Empty`, `EmptyState`, `ErrorPage`, `Forbidden`, `NotFound`, `SignInRequired`, `Progress`, `Skeleton`, `Spinner` |
| Overlays | `Dialog`, `AlertDialog`, `Sheet`, `Drawer`, `Popover`, `Tooltip`, `HoverCard` |
| Layout | Stack, Row, Grid and Container, `SplitPane`, `Accordion`, `Collapsible`, `ScrollArea`, `Separator`, `AspectRatio`, `Direction`, `Carousel` |
| Navigation | the App shell (`Shell`, `AppShell`, `AppPage`), `Tabs`, `Breadcrumb`, `Pagination`, `Command`, `NavigationMenu` |
| Steps, ranges and trees | `Stepper`, `Wizard`, `Slider`, `TreeView`, `Toolbar` |
| Data | `Table`, `DataTable`, `LazyList`, Stat and Bars, `Sortable` |
| App blocks | App styles (`AppSheet`, `AppFont`, `AppImage`), the figure tile (`Stat` with a `Trend`), the info tip (`InfoTip`), the chart skeleton (`ChartSkeleton`), the breathing guide (`BreathingGuide`), `Dock`, the day pager (`DayPager`), the copy field (`CopyField`), the auth layout (`AuthLayout`), `Picture` |
| Marketing site | `SitePage`, `SiteHeader`, `Hero`, `Section`, `LogoCloud`, `FeatureGrid`, `FeatureRows`, `StatBand`, device frames (`BrowserFrame`, `PhoneFrame`), `Testimonials`, `Pricing`, `Faq`, `CtaBand`, `Newsletter`, `LegalPage`, `SiteFooter`; in `navree::ui::marketing` |
| Page SEO | `View::seo` with `Seo`, JSON-LD (`JsonLd`, `Organization`, `WebSite`, `FaqPage`), `Sitemap`, `RobotsTxt`; in `navree::ui::seo` |
| Sign-in pages | `AuthPages` (navree-auth): sign-in, sign-up, two-factor and password pages in the app's look, in an `AuthLayout` |
| Assistant | `Conversation`, `ChatMessage`, `PromptInput`, `ToolCallCard` and `Reasoning`, `ApprovalCard`, `SurfaceFrame`, `Suggestions`, `Sources` and `Citation`, `ThreadList` (the parts of a page where people talk with an assistant; see https://navree.com/docs/ai/generative-ui/) |
| Charts | the line chart, area chart and bar chart (`Chart::line`, `area`, `bar`), the donut chart (`Donut`, and a pie), `Funnel`, `Sparkline`, and Reports (`Report`, for a date window); radial: `Ring`, `Rings`, `Gauge`, the day clock (`DayClock`); linear: `Meter`, mini columns (`MiniColumns`), a colour strip (`ColorStrip`), a range scale (`RangeScale`); grids: the calendar heat map (`CalendarHeatmap`), the matrix heat map (`MatrixHeatmap`), the relation list (`RelationList`), the scatter chart (`Scatter`); in `navree::ui::charts` |
| Foundations | Foundations (tokens, sizes), Themes, the Theme builder, Icons, Flags, Commands (`Cmd`, `Reply`, `Op`), Regions, Errors and states |

Notes on a few: `Tabs` panels can load on first show with `Tab::lazy(src)`;
`Toast` has four tones, a title and one action; `Message` and `Bubble` draw
a chat; `Empty` is the plain version of `EmptyState`; `Table` is the small
static table and `DataTable` the paged, sorted, filtered one; `Pagination`
puts page links under a static list; `Stat` shows a figure with its change
and `Bars` a small bar chart drawn on the server. A real chart is a
`Chart` (line, area or bar, with series, stacks and a table of the same
numbers), a `Donut`, a `Funnel` or a `Sparkline` under a `Stat`; a `Report`
reads its window from the address and counts per period. `AvatarGroup`
overlaps avatars and counts the rest in a `+N` tile; `Trace` lists the steps
of a run (an agent, an import) with a spoken state each; `ColorPicker` is the
browser's colour field, posting `#rrggbb`; `DateTimePicker` puts a date and a
time side by side and posts one value (`2026-11-12T14:30`); `Questionnaire` asks one question
at a time, each drawn again by the server, so it needs no script. A `Ring`
shows one score or a value toward a goal, `Rings` up to six against their
own goals, a `Gauge` a value on an open arc whose scale may start above zero,
and the day clock a value per hour round a face; a `Meter` splits one whole
into parts, mini columns split several items the same way, a colour strip
colours each slot of time and a range scale places one reading between two
ends; the calendar heat map shows a habit over months, the matrix heat map
and the relation list how measures move together, and the scatter chart a
cloud of pairs ([`references/charts.md`](references/charts.md)). The
marketing blocks draw a public site from the same app with no script, and
`View::seo` writes its head
([`references/app-and-site-blocks.md`](references/app-and-site-blocks.md)).

## Common mistakes

- A page route without the `!` prefix: it lands under `/api`, and its
  errors are JSON.
- No `UiModule` in the app root, or `UiOnDemand` standing in for it: there
  are no toolkit files and no error pages.
- `style=""`, a `class`, an `on*` attribute or a `<script>` in a page. They
  are dropped or refused. Use a component, a `Cmd`, or record a gap.
- Drawing every row of a big table into the page. Page it, or use a
  `LazyList`.
- Two `DataTable`s on one page. They would share the address. Give the
  second its own route or a `Region`.
- Putting every option of a growing set in a `Select`. Search on the server
  with a `Combobox`.
- Trusting the option a form posted. Check it with `all_pickable`.
- Using `Utc::now()` for "today" in a picker. Pass the date in, in the
  person's zone (`navree-time-zones`).
- Marking a link `.prefetch()` whose `GET` changes something.
- Reading a redirect target from the request without `ReturnTo`: it is an
  open redirect.
- Skipping the empty, failed, read-only and forbidden states.
- Telling the owner a component is missing without checking the index and
  `crates/navree-ui/README.md`.

## Docs

- https://navree.com/docs/ui/overview/
- https://navree.com/docs/ui/choosing-a-ui/
- https://navree.com/docs/ui/app-shell/
- https://navree.com/docs/ui/forms/
- https://navree.com/docs/ui/data-table/
- https://navree.com/docs/ui/regions/
- https://navree.com/docs/ui/commands-and-ops/
- https://navree.com/docs/ui/errors-and-states/
- https://navree.com/docs/ui/themes/
- https://navree.com/docs/ui/theme-builder/
- https://navree.com/docs/ui/dates/
- https://navree.com/docs/ui/drag-and-drop/
- https://navree.com/docs/ui/foundations/
- https://navree.com/docs/ui/icons/
- https://navree.com/docs/ui/flags/
- https://navree.com/docs/ui/app-styles/
- https://navree.com/docs/ui/marketing-sites/
- https://navree.com/docs/ui/seo/
- https://navree.com/docs/ui/coming-next/
- https://navree.com/docs/components/overview/
- https://navree.com/docs/basics/errors/
