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
    #[form(widget = select, prompt = "Choose a company", error = "Choose the company.")]
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
`widget = radio`). `widget = select` on a `String` takes its choices from
the server through `FormView::options`, as below.

## The pages

One controller serves the module's pages. Page routes start with `!`, so
they sit at the site's root rather than under `/api`. Every route still
needs its access attribute.

```rust
use navree::access::{Accessible, Create, Read};
use navree::db::Uow;
use navree::http::response::{IntoResponse, Response};
use navree::orm::access::AccessibleQuery as _;
use navree::ui::form::{FormView, Rejected};
use navree::ui::{
    AppPage, AppShell, Button, Card, DataTable, Empty, Reply, Submit, TableQuery, ToastTone,
};

#[derive(Model, Clone, Debug, Default, Serialize)]
#[model(table = "companies", subject = "Company", permission_prefix = "companies")]
pub struct Company {
    pub id: Uuid,
    pub name: String,
}

/// The company choices, from the database: never typed into the page.
async fn company_options(uow: &Uow) -> Result<Vec<(String, String)>> {
    let companies = Company::query().order_by("name").limit(500).get(uow).await?;
    Ok(companies
        .into_vec()
        .into_iter()
        .map(|company| (company.id.to_string(), company.name.clone()))
        .collect())
}

/// The new-deal page, empty or sent back with errors (deals-new in pages/).
fn form_page(shell: &AppShell, form: FormView<DealInput>, companies: Vec<(String, String)>) -> AppPage {
    shell
        .page("New deal")
        .crumb("Deals", "/deals")
        .body(
            Card::new().body(
                form.post("/deals")
                    .submit("Create deal")
                    .cancel("Cancel", "/deals")
                    .options("company_id", companies),
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
    async fn new(&self, shell: AppShell, uow: Uow) -> Result<AppPage> {
        Ok(form_page(&shell, FormView::new(), company_options(&uow).await?))
    }

    /// Saves the form; with errors, the same page with each message under its field.
    #[post("!/deals")]
    #[can(Create, Deal)]
    async fn store(
        &self,
        shell: AppShell,
        uow: Uow,
        submit: Result<Submit<DealInput>, Rejected<DealInput>>,
    ) -> Result<Response> {
        match submit {
            Ok(Submit(input)) => {
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
            Err(rejected) => {
                let companies = company_options(&uow).await?;
                Ok(rejected.respond(|form| form_page(&shell, form, companies)))
            }
        }
    }
}

#[module(
    imports = [AccessModule::for_feature::<Deal>()],
    controllers = [DealPages],
)]
pub struct DealsModule;
```

What each part answers for:

- `TableQuery<DealRow>` reads `q`, `sort`, `page`, `per` and the filters
  from the URL, and `load_as` runs them on the server, on the rows this
  user may read. Back, forward, reload and a shared link restore the
  list, with JavaScript on or off.
- `Submit<DealInput>` refuses a body from another site (403), a body that
  isn't a form (415) and a `GET` (405), and reads only the struct's
  fields. `Rejected` draws the same page again with what was typed and
  each error under its field: 422 with JavaScript off, ops with it on.
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
| Forbidden | the route's access attribute: `#[can(..)]` answers 403 or 404 before the page runs (see GAPS: HTML error pages) |

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
