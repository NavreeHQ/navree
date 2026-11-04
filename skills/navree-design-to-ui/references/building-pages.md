# Building pages

Phase 5. The code here compiles against the current `navree` crate. It
builds the pages of one module from its page specs: a list, a form for
new records, a detail page, and the shell around them. Each page spec row
names the part below that builds it.

The app needs the `ui` feature (and `orm` with a database driver for
tables that read models), `UiModule::from_config()` after `HttpModule`,
and one `Shell` registered as a provider.

## The shell: the design's navigation

The design's sidebar, top bar and user menu become one `Shell`. Each
`NavGroup` is a labelled group of links; each `NavItem` takes an icon
from `icons` (lucide's set, by name in capitals) and an optional count.
Register it once, in the module that owns the app's frame.

```rust
use navree::ui::{NavGroup, NavItem, Shell, UserMenu, icons};

/// The app's one shell, from the design's sidebar (DC-01 in COMPONENTS.md).
pub fn shell() -> Shell {
    Shell::new("Acme CRM")
        .context("Production")
        .group(
            NavGroup::new("Sales")
                .item(NavItem::new("Overview", "/").icon(icons::LAYOUT_DASHBOARD))
                .item(NavItem::new("Deals", "/deals").icon(icons::HANDSHAKE)),
        )
        .group(NavGroup::new("Settings").item(NavItem::new("Team", "/team").icon(icons::USERS)))
        .user(
            UserMenu::new("Ada Lovelace", "ada@acme.test")
                .link("Profile", "/profile")
                .action("Sign out", "/sign-out"), // a POST: nothing a hover fetches signs anyone out
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
```

The app root adds the toolkit after the HTTP module:

```rust
use navree::ui::UiModule;

pub fn app(sources: ConfigSources) -> AppBuilder {
    Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(HttpModule::from_config())
        .module(UiModule::from_config())
        .module(FrameModule)
}
```

## The model, the row and the form

One model holds the data. A `TableRow` struct says what the list shows,
and a `Form` struct says what the form edits. Keep them apart when the
list shows less than the model, or names from other tables.

```rust
use chrono::{DateTime, NaiveDate, Utc};
use navree::orm::Record;
use navree::ui::form::{Money, Options as _};
use navree::ui::{BadgeVariant, Form, Options, TableRow, Validate};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A deal's stage, stored as its value ("lead", "won", ...). Its labels are
/// the design's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Options)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    #[default]
    Lead,
    Proposal,
    #[option(label = "Closed won")]
    Won,
    #[option(label = "Closed lost")]
    Lost,
}

#[derive(Model, Clone, Debug, Default, Serialize)]
#[model(table = "deals", timestamps, subject = "Deal", permission_prefix = "deals")]
pub struct Deal {
    pub id: Uuid,
    pub title: String,
    pub company_id: Uuid,
    #[model(cast = "string_enum")]
    pub stage: Stage,
    pub amount: i64, // minor units
    pub closes_on: Option<NaiveDate>,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// The stage filter's choices: the values stored, with the design's labels.
pub const STAGES: &[(&str, &str)] = Stage::OPTIONS;

/// The badge tone of each stage, from the design's status colours (TOKENS.md).
fn tone(stage: &&'static str) -> BadgeVariant {
    match *stage {
        "won" => BadgeVariant::Success,
        "lost" => BadgeVariant::Danger,
        "proposal" => BadgeVariant::Warning,
        _ => BadgeVariant::Neutral,
    }
}

/// A deal as the list shows it: the columns of the design's table.
#[derive(Clone, TableRow)]
#[table(sort = "-updated_at,title", per = 25, per_pages(10, 25, 50, 100))]
pub struct DealRow {
    #[col(skip)]
    pub id: Uuid,
    /// Deal
    #[col(sortable, search, sticky, strong, filter = text, link = "/deals/{id}")]
    pub title: String,
    #[col(sortable, filter = multi, options = STAGES, badge = tone)]
    pub stage: &'static str,
    #[col(sortable, numeric, filter = range, kind = money)]
    pub amount: Money,
    /// Closes
    #[col(sortable, hide_on_mobile, filter = date)]
    pub closes_on: Option<NaiveDate>,
    /// Updated
    #[col(sortable, hide_on_mobile)]
    pub updated_at: DateTime<Utc>,
}

impl DealRow {
    pub fn from_record(record: Record<Deal>) -> Self {
        let deal = record.into_inner();
        Self {
            id: deal.id,
            title: deal.title,
            stage: deal.stage.option_value(),
            amount: Money(deal.amount),
            closes_on: deal.closes_on,
            updated_at: deal.updated_at,
        }
    }
}

/// The new-deal form: the design's fields, in its order. The doc comments
/// are the labels; the rules run in the browser and again on the server.
#[derive(Debug, Deserialize, Validate, Form)]
pub struct DealInput {
    /// Deal name
    #[validate(length(min = 1, max = 200))]
    #[form(placeholder = "Annual platform license", autofocus, wide)]
    pub title: String,
    /// Company
    #[validate(length(min = 1))]
    #[form(widget = combobox, placeholder = "Search companies", error = "Choose the company.")]
    pub company_id: String,
    /// Value
    #[validate(skip)]
    #[form(placeholder = "12,500")]
    pub amount: Money,
    #[validate(skip)]
    pub stage: Stage,
    /// Expected close
    #[validate(skip)]
    pub closes_on: Option<NaiveDate>,
    #[validate(length(max = 2000))]
    #[form(widget = textarea, rows = 4, help = "Only your team sees this.")]
    pub notes: Option<String>,
}
```

The type picks the control: `String` a text input, a number a number
input, `bool` a checkbox, `Option<NaiveDate>` a date input, `Money` an
amount typed like `12,500.50`, an enum with `Options` a select (or
`widget = radio`), a `Many<T>` a multi-select. `widget = select` (the
browser's) or `listbox` (the styled `Select`) on a `String` takes its
choices from the server through `FormView::options`: for a short list. The
company here is a large, growing table, so it is `widget = combobox`: the
choices are searched on the server, through `FormView::search` and an
options route, as below.

## The pages

One controller serves the module's pages. Page routes start with `!`, so
they sit at the site's root rather than under `/api`. Every route still
needs its access attribute.

```rust
use http::request::Parts;
use navree::access::{Accessible, Create, Read};
use navree::db::Uow;
use navree::http::response::{IntoResponse, Response};
use navree::orm::access::AccessibleQuery as _;
use navree::ui::form::{FieldErrors, FormView, Rejected};
use navree::ui::{
    AppPage, AppShell, Button, Card, Choice, ChoiceList, DataTable, Empty, OptionQuery,
    OptionSource, Reply, Submit, TableQuery, ToastTone,
};

#[derive(Model, Clone, Debug, Default, Serialize)]
#[model(table = "companies", subject = "Company", permission_prefix = "companies")]
pub struct Company {
    pub id: Uuid,
    pub name: String,
}

/// Where the company combobox gets its options: the id is the value, the name
/// is the label and what the typed text is looked for in. The columns are
/// named here, in the code, never taken from the request.
const COMPANIES: OptionSource = OptionSource::new("id", "name");

/// The new-deal page, empty or sent back with errors (deals-new in pages/).
fn form_page(shell: &AppShell, form: FormView<DealInput>) -> AppPage {
    shell
        .page("New deal")
        .crumb("Deals", "/deals")
        .body(
            Card::new().body(
                form.post("/deals")
                    .submit("Create deal")
                    .cancel("Cancel", "/deals")
                    .search("company_id", "/companies/options"),
            ),
        )
}

#[injectable]
pub struct DealPages;

#[controller]
impl DealPages {
    /// `deals-index`: the list, with its search, filters, sort and pages in the URL.
    #[get("!/deals")]
    #[can(Read, Deal)]
    async fn index(
        &self,
        shell: AppShell,
        uow: Uow,
        scope: Accessible<Deal, Read>,
        query: TableQuery<DealRow>,
    ) -> Result<AppPage> {
        let page = query.load_as(scope.query(), &uow, DealRow::from_record).await?;
        Ok(shell
            .page("Deals")
            .actions(Button::primary("New deal").icon(icons::PLUS).href("/deals/new"))
            .body(
                DataTable::new("deals", &query, &page)
                    .caption("Deals")
                    .sticky_header()
                    .empty(
                        Empty::new("No deals yet")
                            .icon(icons::HANDSHAKE)
                            .description("Deals you add show here.")
                            .actions(Button::primary("New deal").href("/deals/new")),
                    ),
            ))
    }

    /// `deals-new`.
    #[get("!/deals/new")]
    #[can(Create, Deal)]
    async fn new(&self, shell: AppShell) -> Result<AppPage> {
        Ok(form_page(&shell, FormView::new()))
    }

    /// The company combobox's options: the companies this user may read whose
    /// name contains the text, a page at a time.
    #[get("!/companies/options")]
    #[can(Read, Company)]
    async fn companies(
        &self,
        uow: Uow,
        scope: Accessible<Company, Read>,
        query: OptionQuery,
    ) -> Result<ChoiceList> {
        let choices = COMPANIES
            .find(scope.query(), &query, &uow, "/companies/options", |record| {
                let company = record.into_inner();
                Choice::new(company.id.to_string(), company.name)
            })
            .await?;
        Ok(choices)
    }

    /// Saves the form; with errors, the same page with each message under its field.
    #[post("!/deals")]
    #[can(Create, Deal)]
    async fn store(
        &self,
        parts: Parts,
        shell: AppShell,
        uow: Uow,
        companies: Accessible<Company, Read>,
        submit: Result<Submit<DealInput>, Rejected<DealInput>>,
    ) -> Result<Response> {
        match submit {
            Ok(Submit(input)) => {
                // The form read the company as text. Nothing a browser posts is known
                // to be one of the options it was shown, so ask the same scoped query
                // the search answers with: another workspace's company, one this user
                // may not read, and one that never existed are all refused.
                let picked = [input.company_id.as_str()];
                let pickable = COMPANIES
                    .all_pickable(companies.query(), picked, &uow, OptionSource::as_uuid)
                    .await?;
                if !pickable {
                    let errors = FieldErrors::from([(
                        "company_id".to_owned(),
                        vec!["Choose the company.".to_owned()],
                    )]);
                    return Ok(Rejected::after_decode(&input, errors, &parts)
                        .respond(|form| form_page(&shell, form)));
                }
                let company_id = input
                    .company_id
                    .parse::<Uuid>()
                    .map_err(|_| AppError::bad_request("Choose the company."))?;
                let deal = Deal::create(
                    &uow,
                    Deal {
                        id: Uuid::now_v7(),
                        title: input.title,
                        company_id,
                        stage: input.stage,
                        amount: input.amount.0,
                        closes_on: input.closes_on,
                        notes: input.notes,
                        ..Deal::default()
                    },
                )
                .await?;
                Ok(Reply::redirect(format!("/deals/{}", deal.id))
                    .toast(ToastTone::Success, "Deal created")
                    .into_response())
            }
            Err(rejected) => Ok(rejected.respond(|form| form_page(&shell, form))),
        }
    }
}

#[module(
    imports = [AccessModule::for_feature::<Deal>(), AccessModule::for_feature::<Company>()],
    controllers = [DealPages],
)]
pub struct DealsModule;
```

What each part answers for:

- `TableQuery<DealRow>` reads `search`, `sort`, `page`, `per_page` and the
  `filter[...]` parameters from the URL, and `load_as` runs them on the
  server, on the rows this user may read. Back, forward, reload and a shared link restore the
  list, with JavaScript on or off.
- `Submit<DealInput>` refuses a body from another site (403), a body that
  isn't a form (415) and a `GET` (405), and reads only the struct's
  fields. `Rejected` draws the same page again with what was typed and
  each error under its field: 422 with JavaScript off, ops with it on.
- The company combobox asks `GET /companies/options?q=text`; `OptionQuery`
  clips the text, caps the page at 50 and never fails. `OptionSource` runs
  the search on `scope.query()`, the companies this user may read, so
  workspace scoping and row-level security hold, and answers a `ChoiceList`
  (a full page ends with "Show more"). `store` asks `all_pickable` with the
  same scoped query before it saves, because a hand-made post can name any
  company. `Rejected::after_decode` answers a failure as a failed rule
  does: ops under the field with script on, the form again with what was
  sent with it off. For a fixed list in the page, `Choice::all_among(values,
  &choices)` is the same check with no database.
- An edit page gives the combobox the label of the company already
  chosen, `form.options("company_id", [(company.id.to_string(), name)])`,
  or it shows the id. With script off a searched combobox shows that
  company and can't change it: note it in the page spec, or use a select
  where the table is short.
- `Reply::redirect` stays on this site; `.toast(...)` shows on the next
  page, with JavaScript on or off.
- Behind a session, give the form its token with JavaScript off:
  `FormView::csrf(session.csrf_token())` (`navree-auth`, `navree::session`).

## States

Every page spec lists five states besides the one the design shows. Build
each from a component; never leave one to chance.

| State | Build it with |
|---|---|
| Empty | `DataTable::empty(Empty::new(..).description(..).actions(..))`; on a page that isn't a table, `Empty` in the body |
| Loading | a page renders whole on the server; a part fetched after paint is a `Region::new(id).src(url).lazy()` holding `Skeleton`s; a table's first load is `DataTable::loading()` |
| Error | `DataTable::failed("Couldn't load deals. Try again.")`; elsewhere `Alert::new(AlertTone::Danger).title(..).body(..)` with a retry link |
| Read-only | the same page without the actions: `DataTable::can_select(false)`, no `row_actions`, no New button, inputs `disabled(true)`; decided from the user's abilities |
| Forbidden | the route's access attribute: `#[can(..)]` answers 403 or 404 before the page runs, and `UiModule` draws it as a page in the shell; inside a page, `Forbidden::new().what(..)` for a card, a tab or a row |

A detail page with its loading and error parts:

```rust
use navree::access::Authorized;
use navree::ui::table::option_label;
use navree::ui::{Badge, Grid, Region, Skeleton, SkeletonShape, Stack, Stat, Text, View};

#[injectable]
pub struct DealDetailPages;

#[controller]
impl DealDetailPages {
    /// `deals-show`.
    #[get("!/deals/{id}")]
    #[can(Read, Deal, from = "id")]
    async fn show(&self, shell: AppShell, deal: Authorized<Deal, Read>) -> Result<AppPage> {
        let deal = deal.into_inner().into_inner();
        Ok(shell
            .page(deal.title.clone())
            .crumb("Deals", "/deals")
            .actions(Button::outline("Edit").href(format!("/deals/{}/edit", deal.id)))
            .body(
                Stack::gap(4).children((
                    Grid::cols(3).gap(4).children((
                        Stat::new("Value", Money(deal.amount).display()),
                        Stat::new("Stage", Badge::new(option_label(STAGES, deal.stage.option_value()))),
                        Stat::new("Closes", deal.closes_on.map_or("Not set".to_owned(), |day| day.to_string())),
                    )),
                    Card::titled("Activity").body(
                        Region::new("activity")
                            .src(format!("/deals/{}/activity", deal.id))
                            .lazy()
                            .children((
                                Skeleton::new(SkeletonShape::Line),
                                Skeleton::new(SkeletonShape::Short),
                            )),
                    ),
                    Card::titled("Notes").body(Text::new(deal.notes.unwrap_or_default())),
                )),
            ))
    }

    /// The activity part, fetched after the page paints.
    #[get("!/deals/{id}/activity")]
    #[can(Read, Deal, from = "id")]
    async fn activity(&self, _deal: Authorized<Deal, Read>) -> Result<View> {
        Ok(View::new("Activity").body(Empty::new("No activity yet")))
    }
}
```

`Region::poll(Duration)` refreshes such a part while the page is visible;
`DataTable::live(every)` does the same for a table.
