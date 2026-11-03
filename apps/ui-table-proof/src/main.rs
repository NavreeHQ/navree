//! `ui-table-proof`: the data table on a real database, in real pages.
//!
//! ```text
//! DT_DATABASE_URL=postgres://... ui-table-proof seed 1000000   # creates and fills the tables
//! DT_DATABASE_URL=postgres://... PORT=8097 ui-table-proof serve
//! ```
//!
//! | Page | Shows |
//! |---|---|
//! | `/deals` | everything: search, filter popovers, multi sort, selection, bulk actions, row actions, details, export, saved views |
//! | `/deals?mode=row` is `/deals-row` | the same with the filter row under the headers |
//! | `/viewer` | a read-only user's table: no boxes, no actions |
//! | `/empty`, `/one`, `/exact` | 0 rows, 1 row, exactly one page |
//! | `/big` | 1,000,000 rows by cursor, with a capped count |
//! | `/big-pages` | 1,000,000 rows by page number, with an exact count |
//! | `/thousand` | the same rows, 100 or 1,000 to a page, each with a box, a menu of actions and details |
//! | `/wide` | 30 columns |
//! | `/tasks` | a model as its own row, in the simplest form |
//! | `/extras` | the rest: a card with bleed, sticky rows, a footer, row links, rows that can't be ticked, the grid keys, header groups |
//! | `/rtl` | right to left, in another language |
//! | `/rich` | custom cells: an avatar and tags, a progress bar, cells that edit in place, a filter bar |
//! | `/live`, `/live-stream` | a table that refreshes by polling, or when an event stream speaks |
//! | `/deals/export.xlsx` | the same state as an Excel file |
//! | `/simple` | no count: Previous and Next only |
//! | `/skeleton`, `/failed` | a first load on its way, and rows that couldn't load |
//!
//! `/__t/...` are the controls the browser tests use: reset the data, delete or add
//! rows behind the page's back, take a permission away.
#![forbid(unsafe_code)]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use http::{Method, StatusCode};
use navree::db::{DatabaseModule, Db, DbOptions, Uow};
use navree::http::extract::{Form, Path};
use navree::http::response::Response;
use navree::http::{AppError, Controller, RouteDef};
use navree::kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree::orm::{Model, Record, Value, col};
use navree::prelude::*;
use navree::ui::form::Money;
use navree::ui::table::{
    BulkMode, EditCell, NoFilter, ViewOp, ViewPost, delete_view, list_views, save_view,
};
use navree::ui::{
    AppPage, AppShell, Badge, BadgeVariant, BulkAction, BulkRequest, Button, Card, DataTable,
    Empty, FilterOption, NavGroup, NavItem, Node, Progress, Reply, RowAction, Shell, TableHeight,
    TableLabels, TableQuery, TableRow, ToastTone, UiModule, el, icons,
};

mod seed;

/// Whether the user can still act.
static CAN_ACT: AtomicBool = AtomicBool::new(true);
/// Whether the tables' reads are refused, as for a user who lost access.
static CAN_READ: AtomicBool = AtomicBool::new(true);

const STAGES: &[(&str, &str)] = &[
    ("lead", "Lead"),
    ("qualified", "Qualified"),
    ("proposal", "Proposal"),
    ("won", "Won"),
    ("lost", "Lost"),
];

/// A deal, as stored.
#[derive(Model, Clone, Debug, Default, serde::Serialize)]
#[model(table = "dt_deals")]
pub struct Deal {
    pub id: i64,
    pub title: String,
    pub stage: String,
    pub amount: Option<i64>,
    pub company_id: Option<i64>,
    pub closed_on: Option<chrono::NaiveDate>,
    pub active: bool,
    pub notes: Option<String>,
    pub version: i64,
}

/// A company, as stored.
#[derive(Model, Clone, Debug, Default, serde::Serialize)]
#[model(table = "dt_companies")]
pub struct Company {
    pub id: i64,
    pub name: String,
}

#[allow(clippy::ptr_arg)]
fn tone(stage: &String) -> BadgeVariant {
    match stage.as_str() {
        "won" => BadgeVariant::Success,
        "lost" => BadgeVariant::Danger,
        "proposal" => BadgeVariant::Warning,
        _ => BadgeVariant::Neutral,
    }
}

/// A deal, as the table shows it.
#[derive(Clone, TableRow)]
#[table(
    sort = "-closed_on,title",
    per = 25,
    per_pages(10, 25, 50, 100),
    version = "version"
)]
pub struct DealRow {
    #[col(skip)]
    pub id: i64,
    /// Deal
    #[col(sortable, search, sticky, locked, strong, truncate, filter = text, link = "/deal/{id}")]
    pub title: String,
    /// Company
    #[col(sortable, filter = relation, kind = int, source = "company_id")]
    pub company: Option<String>,
    #[col(sortable, filter = multi, options = STAGES, badge = tone)]
    pub stage: String,
    #[col(sortable, numeric, filter = range, kind = money)]
    pub amount: Option<Money>,
    /// Closed
    #[col(sortable, hide_on_mobile, filter = date)]
    pub closed_on: Option<chrono::NaiveDate>,
    #[col(hide_on_mobile, filter = boolean, align = center)]
    pub active: bool,
    #[col(hidden, wrap)]
    pub notes: Option<String>,
}

async fn companies(uow: &Uow) -> navree::orm::Result<Vec<Company>> {
    Ok(Company::query()
        .order_by("name")
        .limit(500)
        .get(uow)
        .await?
        .into_vec()
        .into_iter()
        .map(Record::into_inner)
        .collect())
}

fn to_row(names: Vec<Company>) -> impl Fn(Record<Deal>) -> DealRow {
    move |record| {
        let deal = record.into_inner();
        let company = deal.company_id.and_then(|id| {
            names
                .iter()
                .find(|company| company.id == id)
                .map(|company| company.name.clone())
        });
        DealRow {
            id: deal.id,
            title: deal.title,
            company,
            stage: deal.stage,
            amount: deal.amount.map(Money),
            closed_on: deal.closed_on,
            active: deal.active,
            notes: deal.notes,
        }
    }
}

fn shell() -> Shell {
    Shell::new("Table proof")
        .context("DataTable")
        .group(
            NavGroup::new("Deals")
                .item(NavItem::new("All features", "/deals").icon(icons::TABLE))
                .item(NavItem::new("Filter row", "/deals-row").icon(icons::LIST_FILTER))
                .item(NavItem::new("Read-only", "/viewer").icon(icons::EYE))
                .item(NavItem::new("Extras", "/extras").icon(icons::SPARKLES))
                .item(NavItem::new("Rich cells", "/rich").icon(icons::PENCIL))
                .item(NavItem::new("Live", "/live").icon(icons::ACTIVITY))
                .item(NavItem::new("Right to left", "/rtl").icon(icons::GLOBE)),
        )
        .group(
            NavGroup::new("Sizes")
                .item(NavItem::new("No rows", "/empty").icon(icons::INBOX))
                .item(NavItem::new("One row", "/one").icon(icons::FILE))
                .item(NavItem::new("One page", "/exact").icon(icons::LIST))
                .item(NavItem::new("A million, cursor", "/big").icon(icons::DATABASE))
                .item(NavItem::new("A million, pages", "/big-pages").icon(icons::DATABASE))
                .item(NavItem::new("A thousand to a page", "/thousand").icon(icons::TABLE))
                .item(NavItem::new("Thirty columns", "/wide").icon(icons::COLUMNS_3)),
        )
}

fn lost_access() -> navree::http::Response {
    use navree::http::response::IntoResponse as _;
    (StatusCode::FORBIDDEN, "You no longer have access.").into_response()
}

fn deal_actions(row: &DealRow) -> Vec<RowAction> {
    vec![
        RowAction::link("View", format!("/deal/{}", row.id)).icon(icons::EYE),
        RowAction::post("Archive", format!("/deal/{}/archive", row.id)).icon(icons::ARCHIVE),
        RowAction::post("Delete", format!("/deal/{}/delete", row.id))
            .icon(icons::TRASH)
            .destructive(),
    ]
}

fn deal_detail(row: &DealRow) -> Node {
    el("div")
        .children((
            el("strong").child(row.title.clone()),
            el("p").child(row.notes.clone().unwrap_or_else(|| "No notes.".to_owned())),
        ))
        .into()
}

async fn deals_page(
    shell: &AppShell,
    uow: &Uow,
    query: TableQuery<DealRow>,
    row_mode: bool,
) -> Result<Response, AppError> {
    if !CAN_READ.load(Ordering::SeqCst) {
        return Ok(lost_access());
    }
    let views = list_views(uow, "demo", "deals").await?;
    let query = query.or_default_view(&views);
    let names = companies(uow).await?;
    let options: Vec<FilterOption> = names
        .iter()
        .map(|company| FilterOption::new(company.id, company.name.clone()))
        .collect();
    let page = query.load_as(Deal::query(), uow, to_row(names)).await?;
    let can = CAN_ACT.load(Ordering::SeqCst);
    let mut table = DataTable::new("deals", &query, &page)
        .caption("Deals in the pipeline")
        .prefetch()
        .options("company", options)
        .sticky_header()
        .height(TableHeight::Fill)
        .bulk(
            "/deals/bulk",
            [
                BulkAction::new("archive", "Archive").icon(icons::ARCHIVE),
                BulkAction::new("delete", "Delete")
                    .icon(icons::TRASH)
                    .destructive()
                    .confirm("Delete {n} deals? This can't be undone."),
            ],
        )
        .can_select(can)
        .export("/deals/export")
        .views(&views, "/deals/views")
        .detail(deal_detail)
        .empty(Empty::new("No deals yet.").icon(icons::HANDSHAKE));
    if can {
        table = table.row_actions(deal_actions);
    }
    if row_mode {
        table = table.filter_row();
    }
    use navree::http::response::IntoResponse as _;
    Ok(shell
        .page("Deals")
        .actions(Button::primary("New deal").icon(icons::PLUS).href("/deals"))
        .body(table)
        .into_response())
}

async fn bulk(uow: Uow, request: BulkRequest<DealRow>) -> Result<Reply, AppError> {
    if !CAN_ACT.load(Ordering::SeqCst) {
        return Ok(request.refused(StatusCode::FORBIDDEN, "You can't do this any more."));
    }
    let target = match request.resolve(Deal::query(), &uow).await? {
        Ok(target) => target,
        Err(refusal) => return Ok(request.refused(StatusCode::CONFLICT, refusal.message())),
    };
    let count = match request.action() {
        "archive" => {
            target
                .update(
                    &uow,
                    [
                        col("active").set(Value::Bool(false)),
                        Deal::VERSION
                            .set_expr(navree::orm::Expr::raw("version + 1", Vec::<Value>::new())),
                    ],
                )
                .await?
        }
        "delete" => target.delete(&uow).await?,
        _ => return Ok(request.refused(StatusCode::UNPROCESSABLE_ENTITY, "No such action.")),
    };
    let what = match request.mode() {
        BulkMode::All => "all selected",
        BulkMode::Ids => "selected",
    };
    Ok(request.done(format!("Done: {count} deals ({what}).")))
}

async fn views(uow: Uow, post: ViewPost<DealRow, NoFilter>) -> Result<Reply, AppError> {
    let text = match post.op() {
        ViewOp::Save => {
            save_view(
                &uow,
                "demo",
                "deals",
                post.name(),
                post.query(),
                post.is_default(),
            )
            .await?;
            "View saved."
        }
        ViewOp::Delete => {
            delete_view(&uow, "demo", post.view()).await?;
            "View deleted."
        }
    };
    Ok(post.done(text))
}

async fn export(uow: Uow, query: TableQuery<DealRow>) -> Result<Response, AppError> {
    if !CAN_READ.load(Ordering::SeqCst) {
        return Ok(lost_access());
    }
    let names = companies(&uow).await?;
    Ok(query
        .export_csv(Deal::query(), &uow, "deals.csv", to_row(names))
        .await?)
}

/// `/viewer`: the same table for someone who can only read.
async fn viewer(
    shell: AppShell,
    uow: Uow,
    query: TableQuery<DealRow>,
) -> Result<AppPage, AppError> {
    let names = companies(&uow).await?;
    let page = query.load_as(Deal::query(), &uow, to_row(names)).await?;
    Ok(shell.page("Deals, read-only").body(
        DataTable::new("deals", &query, &page)
            .caption("Deals")
            .bulk("/deals/bulk", [BulkAction::new("archive", "Archive")])
            .can_select(false),
    ))
}

/// A table whose rows are a fixed few, to prove the sizes.
async fn sized(
    shell: &AppShell,
    uow: &Uow,
    query: &TableQuery<DealRow>,
    title: &'static str,
    limit: i64,
) -> Result<AppPage, AppError> {
    let names = companies(uow).await?;
    let page = query
        .load_as(
            Deal::query().where_(col("id").lte(Value::I64(limit))),
            uow,
            to_row(names),
        )
        .await?;
    Ok(shell.page(title).body(
        DataTable::new("sized", query, &page)
            .caption(title)
            .bulk("/deals/bulk", [BulkAction::new("archive", "Archive")]),
    ))
}

/// A million rows: its own model, its own definition.
#[derive(Model, Clone, Debug, Default, serde::Serialize, TableRow)]
#[model(table = "dt_big")]
#[table(sort = "-amount", per = 50, per_pages(50, 100), cursor, capped = 10000)]
pub struct Big {
    #[col(skip)]
    pub id: i64,
    #[col(sortable, search, filter = text, sticky)]
    pub title: String,
    #[col(sortable, filter = select, options = STAGES)]
    pub stage: String,
    #[col(sortable, numeric, filter = range)]
    pub amount: Option<i64>,
    #[col(sortable, filter = date)]
    pub created_on: Option<chrono::NaiveDate>,
}

/// The same million, by page number.
#[derive(Clone, TableRow)]
#[table(sort = "-amount", per = 50, per_pages(50, 100, 1000))]
pub struct BigRow {
    #[col(skip)]
    pub id: i64,
    #[col(sortable, search, filter = text, sticky)]
    pub title: String,
    #[col(sortable, filter = select, options = STAGES)]
    pub stage: String,
    #[col(sortable, numeric, filter = range)]
    pub amount: Option<i64>,
    #[col(sortable, filter = date)]
    pub created_on: Option<chrono::NaiveDate>,
}

/// The million again, a thousand to a page, with everything a row can carry: a box,
/// a menu of actions that post, details. What a busy page costs the browser.
#[derive(Clone, TableRow)]
#[table(sort = "-amount", per = 100, per_pages(25, 100, 1000))]
pub struct ThousandRow {
    #[col(skip)]
    pub id: i64,
    #[col(sortable, search, filter = text, sticky, strong)]
    pub title: String,
    #[col(sortable, filter = multi, options = STAGES, badge = tone)]
    pub stage: String,
    #[col(sortable, numeric, filter = range)]
    pub amount: Option<i64>,
    #[col(sortable, filter = date)]
    pub created_on: Option<chrono::NaiveDate>,
}

async fn thousand(
    shell: &AppShell,
    uow: &Uow,
    query: &TableQuery<ThousandRow>,
) -> Result<AppPage, AppError> {
    let page = query
        .load_as(Big::query(), uow, |record| {
            let big = record.into_inner();
            ThousandRow {
                id: big.id,
                title: big.title,
                stage: big.stage,
                amount: big.amount,
                created_on: big.created_on,
            }
        })
        .await?;
    Ok(shell.page("A thousand to a page").body(
        DataTable::new("thousand", query, &page)
            .caption("A thousand to a page")
            .sticky_header()
            .bulk(
                "/thousand/bulk",
                [BulkAction::new("archive", "Archive").icon(icons::ARCHIVE)],
            )
            .row_actions(|row: &ThousandRow| {
                vec![
                    RowAction::post("Archive", format!("/thousand/{}/archive", row.id))
                        .icon(icons::ARCHIVE),
                    RowAction::post("Delete", format!("/thousand/{}/delete", row.id))
                        .icon(icons::TRASH)
                        .destructive(),
                ]
            })
            .detail(|row: &ThousandRow| {
                el("p")
                    .child(format!("{} is at the {} stage.", row.title, row.stage))
                    .into()
            }),
    ))
}

/// A row with thirty columns, drawn from memory.
macro_rules! wide {
    ($($field:ident),+) => {
        #[derive(Clone, TableRow)]
        #[table(per = 25, per_pages(25))]
        pub struct WideRow {
            #[col(skip)]
            pub id: i64,
            #[col(sticky, sortable, label = "Name")]
            pub name: String,
            $(pub $field: String,)+
            #[col(sticky_end)]
            pub last: String,
        }

        fn wide_rows() -> Vec<WideRow> {
            (1..=25).map(|id| WideRow { id, name: format!("Row {id}"), $($field: format!("{} {id}", stringify!($field)),)+ last: "End".to_owned() }).collect()
        }
    };
}

wide!(
    c01, c02, c03, c04, c05, c06, c07, c08, c09, c10, c11, c12, c13, c14, c15, c16, c17, c18, c19,
    c20, c21, c22, c23, c24, c25, c26, c27, c28
);

/// `/tasks`: the simplest table there is.
#[derive(Model, Clone, Debug, Default, serde::Serialize, TableRow)]
#[model(table = "dt_tasks")]
pub struct Task {
    #[col(skip)]
    pub id: i64,
    #[col(sortable, search, filter = text)]
    pub title: String,
    #[col(sortable, filter = boolean)]
    pub done: bool,
}

/// `/extras`: a row with grouped headings.
#[derive(Clone, TableRow)]
#[table(sort = "title", per = 25, per_pages(25))]
pub struct ExtraRow {
    #[col(skip)]
    pub id: i64,
    #[col(sortable, sticky, strong, label = "Deal")]
    pub title: String,
    #[col(group = "Money", numeric, label = "Amount")]
    pub amount: Option<Money>,
    #[col(group = "Money", align = end, label = "Cents")]
    pub cents: Option<i64>,
    #[col(group = "State", align = center, label = "Stage", options = STAGES)]
    pub stage: String,
    #[col(group = "State", label = "Active")]
    pub active: bool,
}

async fn extras(
    shell: AppShell,
    uow: Uow,
    query: TableQuery<ExtraRow>,
) -> Result<AppPage, AppError> {
    let page = query
        .load_as(
            Deal::query().where_(col("id").gt(Value::I64(8))),
            &uow,
            |record| {
                let deal = record.into_inner();
                ExtraRow {
                    id: deal.id,
                    title: deal.title,
                    amount: deal.amount.map(Money),
                    cents: deal.amount,
                    stage: deal.stage,
                    active: deal.active,
                }
            },
        )
        .await?;
    let total: i64 = page.rows.iter().filter_map(|row| row.cents).sum();
    let table = DataTable::new("extras", &query, &page)
        .caption("Everything else")
        .bleed()
        .scroll_to("extras")
        .keyboard_grid()
        .bulk(
            "/deals/bulk",
            [
                BulkAction::new("archive", "Archive").denied("Your role can't archive."),
                BulkAction::new("x", "Mark"),
            ],
        )
        .selectable_if(|row| row.active)
        .sticky_rows(|row| row.id == 110)
        .row_href(|row| format!("/deal/{}", row.id))
        .foot("title", "On this page")
        .foot("amount", Money(total).display())
        .height(TableHeight::Md);
    Ok(shell
        .page("Extras")
        .body(Card::titled("In a card").body(table)))
}

/// `/rich`: columns drawn from the whole row.
#[derive(Clone, TableRow)]
#[table(sort = "title", per = 10, per_pages(10, 25))]
pub struct RichRow {
    #[col(skip)]
    pub id: i64,
    #[col(sortable, search, filter = text, label = "Deal")]
    pub title: String,
    #[col(sortable, filter = multi, options = STAGES)]
    pub stage: String,
    #[col(sortable, numeric, filter = range, label = "Amount", kind = money)]
    pub amount: Option<Money>,
    #[col(filter = boolean)]
    pub active: bool,
    #[col(cell = owner_cell, label = "Owner")]
    pub owner: navree::ui::table::Computed,
    #[col(cell = progress_cell, label = "Progress")]
    pub progress: navree::ui::table::Computed,
    #[col(skip)]
    pub notes: Option<String>,
}

fn owner_cell(row: &RichRow) -> Node {
    let names = ["Ada Lovelace", "Alan Turing", "<b>Grace</b> Hopper"];
    let name = names[usize::try_from(row.id % 3).unwrap_or(0)];
    let tags = ["vip", "renewal", "eu", "q3", "new"];
    let count = usize::try_from(row.id % 5).unwrap_or(0) + 1;
    let shown: Vec<Node> = tags
        .iter()
        .take(count.min(2))
        .map(|tag| Badge::new(*tag).variant(BadgeVariant::Outline).into())
        .collect();
    let more = (count > 2).then(|| el("span").child(format!("+{}", count - 2)));
    el("div")
        .attr("data-owner", "")
        .children((el("bdi").child(name.to_owned()), " ", shown, " ", more))
        .into()
}

fn progress_cell(row: &RichRow) -> Node {
    Progress::new(
        u32::try_from(row.id % 100).unwrap_or(0),
        100,
        format!("Progress of {}", row.title),
    )
    .into()
}

fn to_rich(record: Record<Deal>) -> RichRow {
    let deal = record.into_inner();
    RichRow {
        id: deal.id,
        title: deal.title,
        stage: deal.stage,
        amount: deal.amount.map(Money),
        active: deal.active,
        owner: navree::ui::table::Computed,
        progress: navree::ui::table::Computed,
        notes: deal.notes,
    }
}

async fn rich(shell: AppShell, uow: Uow, query: TableQuery<RichRow>) -> Result<AppPage, AppError> {
    let page = query
        .load_as(
            Deal::query().where_(col("id").gt(Value::I64(8))),
            &uow,
            to_rich,
        )
        .await?;
    let table = DataTable::new("rich", &query, &page)
        .caption("Rich cells")
        .filter_bar()
        .cell("title", |row| {
            EditCell::text(row.title.clone(), format!("/deal/{}/field/title", row.id))
                .label(format!("Name of deal {}", row.id))
        })
        .cell("stage", |row| {
            EditCell::select(
                row.stage.clone(),
                format!("/deal/{}/field/stage", row.id),
                STAGES.iter().map(|(value, label)| (*value, *label)),
            )
            .label(format!("Stage of deal {}", row.id))
        });
    Ok(shell.page("Rich cells").body(table))
}

#[derive(serde::Deserialize)]
struct FieldPost {
    value: String,
}

/// One cell's post: refused with a reason, or saved.
async fn edit_field(
    uow: Uow,
    Path((id, field)): Path<(i64, String)>,
    Form(post): Form<FieldPost>,
) -> Result<Reply, AppError> {
    let value = post.value.trim();
    let refuse = |why: String| {
        Reply::ops_or_redirect("/rich")
            .toast(ToastTone::Danger, why)
            .status(StatusCode::UNPROCESSABLE_ENTITY)
    };
    if value.is_empty() {
        return Ok(refuse("A deal needs a name.".to_owned()));
    }
    if value == "taken" {
        return Ok(refuse("Another deal already has that name.".to_owned()));
    }
    let column = match field.as_str() {
        "title" => "title",
        "stage" if STAGES.iter().any(|(stage, _)| *stage == value) => "stage",
        _ => return Ok(refuse("That can't be changed here.".to_owned())),
    };
    Deal::query()
        .where_(col("id").eq(Value::I64(id)))
        .update(&uow, [col(column).set(Value::String(value.to_owned()))])
        .await?;
    Ok(Reply::ops_or_redirect("/rich").toast(ToastTone::Success, "Saved."))
}

/// `/live` and `/live-stream`.
async fn live_page(
    shell: &AppShell,
    uow: &Uow,
    query: &TableQuery<DealRow>,
    stream: bool,
) -> Result<AppPage, AppError> {
    let names = companies(uow).await?;
    let page = query.load_as(Deal::query(), uow, to_row(names)).await?;
    let table = DataTable::new("deals", query, &page)
        .caption("Live")
        .bulk("/deals/bulk", [BulkAction::new("archive", "Archive")]);
    let table = if stream {
        table.live_from("/__t/events")
    } else {
        table.live(Duration::from_millis(700))
    };
    Ok(shell.page("Live").body(table))
}

/// `/deals/export.xlsx`: the table's walker, handed to the spreadsheet writer.
async fn export_xlsx(uow: Uow, query: TableQuery<DealRow>) -> Result<Response, AppError> {
    use navree::http::response::IntoResponse as _;
    use navree::sheets::{Cell, Column as SheetColumn, write_xlsx};
    use navree::ui::table::{TableRow as _, ValueKind};
    let names = companies(&uow).await?;
    let mut rows: Vec<DealRow> = Vec::new();
    query
        .export_rows(
            Deal::query(),
            &uow,
            navree::sheets::XLSX_MAX_ROWS,
            to_row(names),
            &mut rows,
        )
        .await?;
    let shown = query.visible_columns();
    let columns: Vec<SheetColumn> = shown
        .iter()
        .map(|at| SheetColumn::new(DealRow::COLUMNS[*at].name, DealRow::COLUMNS[*at].label))
        .collect();
    let headers: Vec<String> = shown
        .iter()
        .map(|at| DealRow::COLUMNS[*at].label.to_owned())
        .collect();
    let cells: Vec<Vec<Cell>> = rows
        .iter()
        .map(|row| {
            shown
                .iter()
                .map(|at| {
                    let mut text = String::new();
                    row.write_text(*at, &mut text);
                    match DealRow::COLUMNS[*at].kind {
                        _ if text.is_empty() => Cell::Empty,
                        ValueKind::Money | ValueKind::Float | ValueKind::Decimal => text
                            .replace(',', "")
                            .parse()
                            .map_or(Cell::Text(text), Cell::Float),
                        // The company column binds as a key but shows a name.
                        ValueKind::Int => text.parse().map_or(Cell::Text(text), Cell::Int),
                        ValueKind::Date => chrono::NaiveDate::parse_from_str(&text, "%Y-%m-%d")
                            .map_or(Cell::Text(text), Cell::Date),
                        _ => Cell::Text(text),
                    }
                })
                .collect()
        })
        .collect();
    let file = write_xlsx("Deals", &columns, &headers, &cells)
        .map_err(|error| AppError::internal(error.to_string()))?;
    let mut response = (StatusCode::OK, file).into_response();
    response.headers_mut().insert(
        http::header::CONTENT_TYPE,
        http::HeaderValue::from_static(
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        ),
    );
    response.headers_mut().insert(
        http::header::CONTENT_DISPOSITION,
        http::HeaderValue::from_static("attachment; filename=\"deals.xlsx\""),
    );
    Ok(response)
}

/// `/simple`: no count at all.
#[derive(Clone, TableRow)]
#[table(sort = "title", per = 10, per_pages(10), no_count)]
pub struct SimpleRow {
    #[col(skip)]
    pub id: i64,
    #[col(sortable, search, filter = text)]
    pub title: String,
}

/// `/rtl`: the same table in Arabic, right to left.
#[derive(Clone, TableRow)]
#[table(sort = "title", per = 10, per_pages(10, 25))]
pub struct RtlRow {
    #[col(skip)]
    pub id: i64,
    #[col(sortable, sticky, search, filter = text, label = "الصفقة")]
    pub title: String,
    #[col(sortable, numeric, filter = range, label = "المبلغ", kind = money)]
    pub amount: Option<Money>,
    #[col(filter = multi, options = STAGES, label = "المرحلة")]
    pub stage: String,
    #[col(label = "ملاحظات", wrap)]
    pub notes: Option<String>,
}

fn arabic() -> TableLabels {
    TableLabels {
        search: "بحث".into(),
        columns: "الأعمدة".into(),
        apply: "تطبيق".into(),
        rows_per_page: "عدد الصفوف".into(),
        showing: "عرض {from} إلى {to} من {total}".into(),
        selected: "تم اختيار {n}".into(),
        clear_selection: "إلغاء الاختيار".into(),
        select_row: "اختيار {label}".into(),
        select_page: "اختيار الصفحة".into(),
        ..TableLabels::default()
    }
}

async fn rtl(shell: AppShell, uow: Uow, query: TableQuery<RtlRow>) -> Result<AppPage, AppError> {
    let labels = arabic();
    let page = query
        .load_as(Deal::query(), &uow, |record| {
            let deal = record.into_inner();
            RtlRow {
                id: deal.id,
                title: deal.title,
                amount: deal.amount.map(Money),
                stage: deal.stage,
                notes: deal.notes,
            }
        })
        .await?;
    Ok(shell.page("بالعربية").body(
        el("div").attr("dir", "rtl").child(
            DataTable::new("rtl", &query, &page)
                .caption("الصفقات")
                .labels(&labels)
                .bulk("/deals/bulk", [BulkAction::new("archive", "أرشفة")]),
        ),
    ))
}

pub struct Pages;

impl Injectable for Pages {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for Pages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "!/", || async { Reply::redirect("/deals") }).public(),
            RouteDef::on(
                Method::GET,
                "!/deals",
                |shell: AppShell, uow: Uow, query: TableQuery<DealRow>| async move {
                    deals_page(&shell, &uow, query, false).await
                },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "!/deals-row",
                |shell: AppShell, uow: Uow, query: TableQuery<DealRow>| async move {
                    deals_page(&shell, &uow, query, true).await
                },
            )
            .public(),
            RouteDef::on(Method::POST, "!/deals/bulk", bulk).public(),
            RouteDef::on(Method::POST, "!/deals/views", views).public(),
            RouteDef::on(Method::GET, "!/deals/export", export).public(),
            RouteDef::on(Method::GET, "!/viewer", viewer).public(),
            RouteDef::on(
                Method::GET,
                "!/empty",
                |shell: AppShell, uow: Uow, query: TableQuery<DealRow>| async move {
                    sized(&shell, &uow, &query, "No rows", 0).await
                },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "!/one",
                |shell: AppShell, uow: Uow, query: TableQuery<DealRow>| async move {
                    sized(&shell, &uow, &query, "One row", 1).await
                },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "!/exact",
                |shell: AppShell, uow: Uow, query: TableQuery<DealRow>| async move {
                    sized(&shell, &uow, &query, "One page", 25).await
                },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "!/big",
                |shell: AppShell, uow: Uow, query: TableQuery<Big>| async move {
                    let page = query.load(Big::query(), &uow).await?;
                    Ok::<_, AppError>(
                        shell.page("A million rows, by cursor").body(
                            DataTable::new("big", &query, &page)
                                .caption("A million rows")
                                .sticky_header(),
                        ),
                    )
                },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "!/big-pages",
                |shell: AppShell, uow: Uow, query: TableQuery<BigRow>| async move {
                    let page = query
                        .load_as(Big::query(), &uow, |record| {
                            let big = record.into_inner();
                            BigRow {
                                id: big.id,
                                title: big.title,
                                stage: big.stage,
                                amount: big.amount,
                                created_on: big.created_on,
                            }
                        })
                        .await?;
                    Ok::<_, AppError>(
                        shell.page("A million rows, by page").body(
                            DataTable::new("big", &query, &page)
                                .caption("A million rows")
                                .sticky_header(),
                        ),
                    )
                },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "!/thousand",
                |shell: AppShell, uow: Uow, query: TableQuery<ThousandRow>| async move {
                    thousand(&shell, &uow, &query).await
                },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "!/thousand/bulk",
                |uow: Uow, request: BulkRequest<ThousandRow>| async move {
                    let target = match request.resolve(Big::query(), &uow).await? {
                        Ok(target) => target,
                        Err(refusal) => {
                            return Ok(request.refused(StatusCode::CONFLICT, refusal.message()));
                        }
                    };
                    let count = target
                        .update(&uow, [col("stage").set(Value::String("lost".into()))])
                        .await?;
                    Ok::<_, AppError>(request.done(format!("Archived {count}.")))
                },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "!/thousand/{id}/archive",
                |uow: Uow, Path(id): Path<i64>| async move {
                    Big::query()
                        .where_(col("id").eq(Value::I64(id)))
                        .update(&uow, [col("stage").set(Value::String("lost".into()))])
                        .await?;
                    Ok::<_, AppError>(
                        Reply::ops_or_redirect("/thousand")
                            .toast(ToastTone::Success, "Archived.")
                            .op(navree::ui::Op::Invalidate {
                                target: "thousand".into(),
                            }),
                    )
                },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "!/thousand/{id}/delete",
                |uow: Uow, Path(id): Path<i64>| async move {
                    Big::query()
                        .where_(col("id").eq(Value::I64(id)))
                        .delete(&uow)
                        .await?;
                    Ok::<_, AppError>(
                        Reply::ops_or_redirect("/thousand")
                            .toast(ToastTone::Success, "Deleted.")
                            .op(navree::ui::Op::Invalidate {
                                target: "thousand".into(),
                            }),
                    )
                },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "!/wide",
                |shell: AppShell, query: TableQuery<WideRow>| async move {
                    let rows = wide_rows();
                    shell.page("Thirty columns").body(
                        DataTable::new("wide", &query, &rows)
                            .caption("Thirty columns")
                            .bulk("/deals/bulk", [BulkAction::new("archive", "Archive")])
                            .row_actions(|row| {
                                vec![RowAction::link("View", format!("/deal/{}", row.id))]
                            }),
                    )
                },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "!/tasks",
                |shell: AppShell, uow: Uow, query: TableQuery<Task>| async move {
                    let page = query.load(Task::query(), &uow).await?;
                    Ok::<_, AppError>(
                        shell
                            .page("Tasks")
                            .body(DataTable::new("tasks", &query, &page).caption("Tasks")),
                    )
                },
            )
            .public(),
            RouteDef::on(Method::GET, "!/extras", extras).public(),
            RouteDef::on(Method::GET, "!/rich", rich).public(),
            RouteDef::on(Method::POST, "!/deal/{id}/field/{field}", edit_field).public(),
            RouteDef::on(Method::GET, "!/deals/export.xlsx", export_xlsx).public(),
            RouteDef::on(
                Method::GET,
                "!/live",
                |shell: AppShell, uow: Uow, query: TableQuery<DealRow>| async move {
                    live_page(&shell, &uow, &query, false).await
                },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "!/live-stream",
                |shell: AppShell, uow: Uow, query: TableQuery<DealRow>| async move {
                    live_page(&shell, &uow, &query, true).await
                },
            )
            .public(),
            RouteDef::on(Method::GET, "!/__t/events", || async {
                use navree::http::response::IntoResponse as _;
                let mut response = "retry: 500\ndata: changed\n\n".into_response();
                response.headers_mut().insert(
                    http::header::CONTENT_TYPE,
                    http::HeaderValue::from_static("text/event-stream"),
                );
                response
            })
            .public(),
            RouteDef::on(
                Method::GET,
                "!/simple",
                |shell: AppShell, uow: Uow, query: TableQuery<SimpleRow>| async move {
                    let page = query
                        .load_as(Deal::query(), &uow, |record| {
                            let deal = record.into_inner();
                            SimpleRow {
                                id: deal.id,
                                title: deal.title,
                            }
                        })
                        .await?;
                    Ok::<_, AppError>(
                        shell
                            .page("No count")
                            .body(DataTable::new("simple", &query, &page).caption("No count")),
                    )
                },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "!/skeleton",
                |shell: AppShell, query: TableQuery<DealRow>| async move {
                    let rows: Vec<DealRow> = Vec::new();
                    shell.page("Loading").body(
                        DataTable::new("deals", &query, &rows)
                            .caption("Deals")
                            .loading(),
                    )
                },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "!/failed",
                |shell: AppShell, query: TableQuery<DealRow>| async move {
                    let rows: Vec<DealRow> = Vec::new();
                    shell.page("Failed").body(
                        DataTable::new("deals", &query, &rows)
                            .caption("Deals")
                            .failed("The deals couldn't load."),
                    )
                },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "!/big/export",
                |uow: Uow, query: TableQuery<Big>| async move {
                    Ok::<_, AppError>(
                        query
                            .export_csv(Big::query(), &uow, "big.csv", Record::into_inner)
                            .await?,
                    )
                },
            )
            .public(),
            RouteDef::on(Method::GET, "!/rtl", rtl).public(),
            RouteDef::on(
                Method::GET,
                "!/deal/{id}",
                |shell: AppShell, uow: Uow, Path(id): Path<i64>| async move {
                    let deal = Deal::find_or_fail(&uow, id).await?;
                    Ok::<_, AppError>(
                        shell.page("Deal").body(
                            Card::titled(deal.title.clone())
                                .body(el("p").child(deal.notes.clone().unwrap_or_default())),
                        ),
                    )
                },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "!/deal/{id}/archive",
                |uow: Uow, Path(id): Path<i64>| async move {
                    Deal::query()
                        .where_(col("id").eq(Value::I64(id)))
                        .update(&uow, [col("active").set(Value::Bool(false))])
                        .await?;
                    Ok::<_, AppError>(
                        Reply::ops_or_redirect("/deals")
                            .toast(ToastTone::Success, "Archived.")
                            .op(navree::ui::Op::Invalidate {
                                target: "deals".into(),
                            }),
                    )
                },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "!/deal/{id}/delete",
                |uow: Uow, Path(id): Path<i64>| async move {
                    Deal::query()
                        .where_(col("id").eq(Value::I64(id)))
                        .delete(&uow)
                        .await?;
                    Ok::<_, AppError>(
                        Reply::ops_or_redirect("/deals")
                            .toast(ToastTone::Success, "Deleted.")
                            .op(navree::ui::Op::Invalidate {
                                target: "deals".into(),
                            }),
                    )
                },
            )
            .public(),
            // What the browser tests use.
            RouteDef::on(Method::POST, "!/__t/reset", |uow: Uow| async move {
                seed::reset_deals(&uow).await?;
                CAN_ACT.store(true, Ordering::SeqCst);
                CAN_READ.store(true, Ordering::SeqCst);
                Ok::<_, AppError>("ok")
            })
            .public(),
            RouteDef::on(
                Method::POST,
                "!/__t/permission/{what}",
                |Path(what): Path<String>| async move {
                    match what.as_str() {
                        "no-act" => CAN_ACT.store(false, Ordering::SeqCst),
                        "no-read" => CAN_READ.store(false, Ordering::SeqCst),
                        _ => {
                            CAN_ACT.store(true, Ordering::SeqCst);
                            CAN_READ.store(true, Ordering::SeqCst);
                        }
                    }
                    "ok"
                },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "!/__t/delete-above/{id}",
                |uow: Uow, Path(id): Path<i64>| async move {
                    let count = Deal::query()
                        .where_(col("id").gt(Value::I64(id)))
                        .delete(&uow)
                        .await?;
                    Ok::<_, AppError>(count.to_string())
                },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "!/__t/delete/{id}",
                |uow: Uow, Path(id): Path<i64>| async move {
                    let count = Deal::query()
                        .where_(col("id").eq(Value::I64(id)))
                        .delete(&uow)
                        .await?;
                    Ok::<_, AppError>(count.to_string())
                },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "!/__t/add/{id}",
                |uow: Uow, Path(id): Path<i64>| async move {
                    let deal = Deal {
                        id,
                        title: format!("Added {id}"),
                        stage: "won".into(),
                        amount: Some(1),
                        company_id: Some(1),
                        closed_on: None,
                        active: true,
                        notes: None,
                        version: 1,
                    };
                    Deal::query().insert(&uow, [deal]).await?;
                    Ok::<_, AppError>("ok")
                },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "!/__t/count",
                |uow: Uow, query: TableQuery<DealRow>| async move {
                    let count = query.apply(Deal::query()).count(&uow).await?;
                    Ok::<_, AppError>(count.to_string())
                },
            )
            .public(),
            RouteDef::on(Method::GET, "!/__t/archived", |uow: Uow| async move {
                let keys: Vec<i64> = Deal::query()
                    .where_(col("active").eq(Value::Bool(false)))
                    .where_(col("version").gt(Value::I64(1)))
                    .order_by("id")
                    .pluck(&uow, navree::orm::col_as::<i64>("id"))
                    .await?;
                let list: Vec<String> = keys.iter().map(ToString::to_string).collect();
                Ok::<_, AppError>(list.join(","))
            })
            .public(),
        ]
    }
}

struct ProofModule;

impl Module for ProofModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.factory(Factory::<Shell>::new(|_| async { Ok(Arc::new(shell())) }))
            .exports::<Shell>();
        m.controller::<Pages>();
    }
}

fn main() -> Result<(), BoxError> {
    let url = std::env::var("DT_DATABASE_URL")
        .unwrap_or_else(|_| "sqlite://target/ui-table-proof.db?mode=rwc".to_owned());
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some("seed") {
        let big: u64 = args
            .next()
            .and_then(|count| count.parse().ok())
            .unwrap_or(0);
        let runtime = tokio::runtime::Runtime::new()?;
        return runtime.block_on(async move {
            let db = Db::connect(&DbOptions {
                url,
                ..DbOptions::default()
            })
            .await?;
            seed::create(&db).await?;
            seed::big(&db, big).await?;
            db.close().await?;
            Ok(())
        });
    }
    let port = std::env::var("PORT").unwrap_or_else(|_| "8097".to_owned());
    let sources = ConfigSources::new().env_vars(
        "APP_",
        [
            ("APP_HTTP__ADDR".to_owned(), format!("127.0.0.1:{port}")),
            (
                "APP_HTTP__PER_IP_RATE_LIMIT".to_owned(),
                "100000000".to_owned(),
            ),
            ("APP_DATABASE__URL".to_owned(), url),
        ],
    );
    Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(HttpModule::from_config())
        .module(DatabaseModule::from_config())
        .module(UiModule::from_config())
        .module(ProofModule)
        .run()
}
