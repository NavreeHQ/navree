//! Native clients against a real app: the same routes answering a browser
//! with the HTML they always did and a native client with the envelope, and
//! the readers of the components that are built with derives.

use std::sync::Arc;

use http::{Method, StatusCode};
use navree_http::AppError;
use navree_http::extract::Path;
use navree_http::response::IntoResponse;
use navree_http::{Controller, HttpModule, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{
    BoxFuture, Factory, Injectable, KernelError, Module, ModuleBuilder, ProviderKey, Resolver,
};
use navree_native::input::{Input, TextKind};
use navree_native::node::{self as n, Node as N};
use navree_native::{Action, Client, Command, Envelope, FormFactor, Op as NativeOp, Platform};
use navree_testing::TestApp;
use navree_ui::form::{FormView, Money, Rejected, Submit};
use navree_ui::native::{Stamp, TestNative, lower};
use navree_ui::select::{Choice, ChoiceList};
use navree_ui::sortable::{Move, MoveRejected};
use navree_ui::table::BulkRequest;
use navree_ui::table::NoFilter;
use navree_ui::upload::Received;
use navree_ui::{
    AppPage, AppShell, Badge, Button, DataTable, Dialog, Empty, Form, NavGroup, NavItem, Op,
    Options, Reply, Sheet, Shell, TablePage, TableQuery, TableRow, Text, ToastTone, UiModule,
    UserMenu, Validate, View, assets, el, icons, render,
};
use navree_ui::{LazyAsk, LazyItem, LazyPage, LazyQuery};
use serde::Deserialize;

const NATIVE: &str = "x-navree-native";
const CLIENT: &str = "x-navree-client";
const PHONE: &str =
    "platform=android; os=16; form=phone; app=0.1.0; vocab=1; locale=en-IN; tz=Asia/Kolkata";

fn phone() -> Client {
    Client::new(Platform::Android, FormFactor::Phone)
}

/// What a refund says.
#[derive(Debug, Deserialize, Validate, Form)]
#[form(id = "refund-form")]
struct Refund {
    /// Amount
    #[validate(skip)]
    #[form(help = "Up to what is left to refund.")]
    amount: Money,
    /// Reason
    #[validate(skip)]
    reason: Reason,
    /// Tell the customer
    #[validate(skip)]
    notify: bool,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Options, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Reason {
    /// Returned by the customer
    #[default]
    Returned,
    /// Arrived late
    Late,
}

fn refund_form() -> FormView<Refund> {
    FormView::new()
        .of(&Refund {
            amount: Money(784_800),
            reason: Reason::Late,
            notify: true,
        })
        .post("/orders/1/refund")
        .submit("Refund")
}

/// The fields among `nodes`, through the stacks that group them.
fn fields_in(nodes: &[N]) -> Vec<&n::Field> {
    let mut out = Vec::new();
    for node in nodes {
        match node {
            N::Field(field) => out.push(field),
            N::Stack(stack) => out.extend(fields_in(&stack.children)),
            _ => {}
        }
    }
    out
}

#[test]
fn a_form_view_reads_as_a_form_of_fields_with_their_labels_and_help() {
    let lowered = lower(&refund_form().into());
    assert!(lowered.is_native(), "{:?}", lowered.misses);
    let N::Form(form) = lowered.node else {
        panic!("not a form");
    };
    assert_eq!(form.id.as_deref(), Some("refund-form"));
    assert_eq!(form.method, n::Method::Post);
    assert_eq!(form.action, "/orders/1/refund");
    let fields = fields_in(&form.children);
    let [amount, reason, notify] = fields.as_slice() else {
        panic!("not three fields: {:?}", form.children);
    };
    assert_eq!(amount.name, "amount");
    assert_eq!(amount.label.as_deref(), Some("Amount"));
    assert_eq!(
        amount.help.as_deref(),
        Some("Up to what is left to refund.")
    );
    assert_eq!(amount.id.as_deref(), Some("refund-form-amount"));
    let Input::Text(typed) = &amount.input else {
        panic!("not typed");
    };
    assert_eq!(typed.kind, TextKind::Decimal);
    assert_eq!(typed.value, "7,848.00");
    assert_eq!(reason.label.as_deref(), Some("Reason"));
    let Input::Choice(choice) = &reason.input else {
        panic!("not a choice");
    };
    assert_eq!(choice.value.as_deref(), Some("late"));
    assert_eq!(
        choice
            .options
            .iter()
            .map(|option| option.label.as_str())
            .collect::<Vec<_>>(),
        ["Returned by the customer", "Arrived late"]
    );
    assert_eq!(notify.label.as_deref(), Some("Tell the customer"));
    assert!(matches!(&notify.input, Input::Toggle(toggle) if toggle.on));
    let Some(N::Stack(actions)) = form.children.last() else {
        panic!("no actions: {:?}", form.children.last());
    };
    let [N::Button(submit)] = actions.children.as_slice() else {
        panic!("not one button");
    };
    assert_eq!(submit.label, "Refund");
    assert_eq!(submit.role, n::Role::Primary);
    assert_eq!(submit.action, Some(Action::submit()));
}

/// The orders' statuses a filter offers.
const STATUSES: &[(&str, &str)] = &[("open", "Open"), ("completed", "Completed")];

#[derive(Clone, TableRow)]
#[table(sort = "-placed_on", per = 2, per_pages(2, 10))]
struct OrderRow {
    #[col(skip)]
    id: i64,
    #[col(sortable, search, sticky, label = "Order")]
    number: String,
    #[col(filter = multi, options = STATUSES)]
    status: String,
    #[col(sortable, numeric, hide_on_mobile)]
    total: i64,
    #[col(sortable, filter = date, label = "Placed")]
    placed_on: chrono::NaiveDate,
}

fn order(id: i64, status: &str) -> OrderRow {
    OrderRow {
        id,
        number: format!("CS-{}", 20_480 + id),
        status: status.to_owned(),
        total: 507 * id,
        placed_on: chrono::NaiveDate::from_ymd_opt(2026, 11, 12).expect("a date"),
    }
}

fn orders_table<'a>(
    query: &'a TableQuery<OrderRow, NoFilter>,
    page: &'a TablePage<OrderRow>,
) -> DataTable<'a, OrderRow, NoFilter> {
    DataTable::new("orders", query, page)
        .caption("Orders")
        .cell("number", |row| {
            Button::new(row.number.clone())
                .variant(navree_ui::ButtonVariant::Link)
                .href(format!("/orders/{}", row.id))
        })
        .cell("status", |row| {
            Badge::new(row.status.clone())
                .tone(navree_ui::Tone::Info)
                .dot()
        })
        .filter_bar()
        .empty(Empty::new("No order matches").icon(icons::SEARCH_X))
}

fn orders_view(query: &str) -> View {
    let query: TableQuery<OrderRow, NoFilter> = TableQuery::parse("/orders", query);
    let page = TablePage::new(vec![order(1, "open"), order(2, "completed")], 5);
    View::new("Orders").body(orders_table(&query, &page))
}

#[test]
fn a_data_table_built_for_a_native_client_reads_as_a_table_with_the_form_that_asks_for_other_rows()
{
    let page = TestNative::build(phone(), || orders_view("filter[status][in]=open"));
    assert!(page.is_native(), "{:?}", page.page().misses);
    let Some(N::Region(region)) = page.find("orders") else {
        panic!("no region: {:?}", page.screen().body);
    };
    assert!(region.src.starts_with("/orders"));
    let [N::Table(table)] = region.children.as_slice() else {
        panic!("not one table");
    };
    assert_eq!(table.caption.as_deref(), Some("Orders"));
    let columns: Vec<(&str, &str)> = table
        .columns
        .iter()
        .map(|column| (column.key.as_deref().unwrap_or(""), column.label.as_str()))
        .collect();
    assert_eq!(
        columns,
        [
            ("number", "Order"),
            ("status", "Status"),
            ("total", "Total"),
            ("placed_on", "Placed"),
        ]
    );
    // The column that stays in view names the record; the one a phone may leave out says so.
    assert_eq!(table.columns[0].role, Some(n::ColumnRole::Title));
    assert!(table.columns[0].sort.is_some());
    assert!(table.columns[1].sort.is_none());
    assert!(table.columns[2].numeric && table.columns[2].secondary);
    assert_eq!(
        table.columns[3].sort.as_ref().and_then(|sort| sort.dir),
        Some(n::SortDir::Desc)
    );
    // The rows: a cell a column, each the component the page drew or the value's text.
    assert_eq!(table.rows.len(), 2);
    assert_eq!(table.rows[0].id.as_deref(), Some("1"));
    let [
        N::Button(number),
        N::Badge(status),
        N::Text(total),
        N::Text(placed),
    ] = table.rows[0].cells.as_slice()
    else {
        panic!("not the cells: {:?}", table.rows[0].cells);
    };
    assert_eq!(number.label, "CS-20481");
    assert_eq!(number.action, Some(Action::navigate("/orders/1")));
    assert_eq!(status.text, "open");
    assert_eq!(total.value, "507");
    assert_eq!(placed.value, "2026-11-12");
    // The form that asks for other rows: the search, a field a filter, the rows a page.
    let form = table.query.as_ref().expect("a query");
    assert_eq!(form.method, n::Method::Get);
    assert_eq!(form.action, "/orders");
    let fields: Vec<&n::Field> = form
        .children
        .iter()
        .filter_map(|node| match node {
            N::Field(field) => Some(field),
            _ => None,
        })
        .collect();
    let search = fields
        .iter()
        .find(|field| field.name == "search")
        .expect("a search");
    assert!(matches!(&search.input, Input::Text(text) if text.kind == TextKind::Search));
    assert_eq!(search.purpose, Some(n::Purpose::Search));
    let status = fields
        .iter()
        .find(|field| field.name == "filter[status][in]")
        .expect("a status filter");
    assert_eq!(status.label.as_deref(), Some("Status"));
    assert_eq!(status.purpose, Some(n::Purpose::Filter));
    let Input::Choices(choices) = &status.input else {
        panic!("not choices");
    };
    assert_eq!(
        choices
            .options
            .iter()
            .map(|option| (option.value.as_str(), option.label.as_str()))
            .collect::<Vec<_>>(),
        [("open", "Open"), ("completed", "Completed")]
    );
    assert_eq!(choices.values, ["open"]);
    assert!(
        fields
            .iter()
            .any(|field| field.name == "filter[placed_on][gte]"
                && matches!(field.input, Input::Date(_))),
        "{fields:?}"
    );
    assert!(
        fields.iter().any(|field| field.name == "per_page"
            && matches!(field.input, Input::Choice(_))
            && field.purpose == Some(n::Purpose::PageSize)),
        "{fields:?}"
    );
    // What only arranges the web table is not a field.
    assert!(
        !fields
            .iter()
            .any(|field| field.name == "cols" || field.name == "fb")
    );
    // Five orders, two a page: the next page has an address.
    assert!(
        table
            .more
            .as_deref()
            .is_some_and(|more| more.contains("page=2"))
    );
    assert_eq!(table.prev, None);
    assert!(table.summary.is_some());
}

#[test]
fn a_data_table_with_no_rows_reads_its_empty_state() {
    let page = TestNative::build(phone(), || {
        let query: TableQuery<OrderRow, NoFilter> = TableQuery::parse("/orders", "search=zzz");
        let page = TablePage::new(Vec::new(), 0);
        View::new("Orders").body(orders_table(&query, &page))
    });
    assert!(page.is_native(), "{:?}", page.page().misses);
    let Some(N::Region(region)) = page.find("orders") else {
        panic!("no region");
    };
    let [N::Table(table)] = region.children.as_slice() else {
        panic!("not one table");
    };
    assert!(table.rows.is_empty());
    assert!(
        matches!(table.empty.as_deref(), Some(N::Empty(_))),
        "{:?}",
        table.empty
    );
}

#[test]
fn a_data_table_built_for_a_browser_writes_its_rows_as_markup_which_no_reader_reads() {
    let view = orders_view("");
    let page = TestNative::of(&view);
    assert_eq!(page.why(), ["slot:data-table"]);
    assert_eq!(
        page.page().misses[0].why,
        "rows written as markup, with no tree to read"
    );
}

#[test]
fn a_page_built_while_a_native_client_asks_is_the_same_html_for_a_browser() {
    // What a browser gets does not depend on who else is asking: only the
    // table's rows are built another way, and they write the same bytes.
    let for_browser = orders_view("").to_html();
    let mut for_native = String::new();
    TestNative::build(phone(), || {
        let view = orders_view("");
        for_native = view.to_html();
        view
    });
    let rows = |html: &str| html.matches("<tr").count();
    assert_eq!(rows(&for_browser), rows(&for_native));
    assert!(for_browser.contains("CS-20481") && for_native.contains("CS-20481"));
}

fn shell() -> Shell {
    Shell::new("Charkha Supply Co.")
        .context("Store operations")
        .group(
            NavGroup::new("Store")
                .item(NavItem::new("Dashboard", "/").icon(icons::LAYOUT_DASHBOARD))
                .item(NavItem::new("Orders", "/orders").icon(icons::SHOPPING_BAG)),
        )
        .user(
            UserMenu::new("Ravneet Kaur", "ravneet.kaur@example.com")
                .action("Sign out", "/sign-out"),
        )
}

fn order_body(paid: &'static str) -> navree_ui::Element {
    el("div").id("order-page").children((
        Badge::new(paid).tone(navree_ui::Tone::Success).dot(),
        Button::outline("Refund").opens_dialog("refund"),
        Dialog::new("refund", "Refund CS-20481")
            .description("Up to ₹7,848 can go back.")
            .body(refund_form()),
    ))
}

fn order_page(shell: &AppShell) -> AppPage {
    shell
        .page("Order CS-20481")
        .crumb("Dashboard", "/")
        .crumb("Orders", "/orders")
        .actions(Button::primary("Print").href("/orders/1/print"))
        .body(order_body("Paid"))
}

struct Pages;

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
            RouteDef::on(Method::GET, "!/orders", |shell: AppShell| async move {
                let query: TableQuery<OrderRow, NoFilter> = TableQuery::parse("/orders", "");
                let page = TablePage::new(vec![order(1, "open"), order(2, "completed")], 5);
                shell
                    .page("Orders")
                    .crumb("Dashboard", "/")
                    .body((orders_table(&query, &page), Sheet::new("order", "Order")))
            })
            .public(),
            RouteDef::on(
                Method::GET,
                "!/orders/{id}",
                |shell: AppShell, Path(_id): Path<i64>| async move { order_page(&shell) },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "!/orders/{id}/refund",
                |Path(id): Path<i64>| async move {
                    Reply::ops_or_redirect(format!("/orders/{id}"))
                        .op(Op::Close {
                            target: "refund".to_owned(),
                        })
                        .morph("order-page", order_body("Refunded"))
                        .toast(ToastTone::Success, "₹7,848 went back.")
                },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "!/refunds",
                |sent: Result<Submit<Refund>, Rejected<Refund>>| async move {
                    match sent {
                        Ok(Submit(refund)) => Reply::ops(vec![Op::toast(
                            ToastTone::Success,
                            format!("{} paise went back.", refund.amount.0),
                        )])
                        .into_response(),
                        Err(rejected) => rejected.into_response(),
                    }
                },
            )
            .public(),
            RouteDef::on(Method::GET, "!/board", |shell: AppShell| async move {
                shell.page("Board").body((Text::new("Cards"), el("canvas")))
            })
            .public(),
            RouteDef::on(Method::POST, "!/board/move", || async {
                Reply::ops_or_redirect("/board").morph("board", el("canvas").id("board"))
            })
            .public(),
            RouteDef::on(Method::GET, "!/columns/doing", || async {
                // A handler that has ops only for the browser's runtime.
                Reply::ops_or_redirect("/board?show=20")
            })
            .public(),
            RouteDef::on(Method::GET, "!/fragments/feed", || async {
                (
                    [(http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
                    render(&Text::new("An order was packed").into()),
                )
            })
            .public(),
            RouteDef::on(Method::GET, "!/fragments/figures", || async {
                View::new("Figures").body(Text::new("952 orders today"))
            })
            .public(),
            RouteDef::on(
                Method::GET,
                "!/things/rows",
                |query: LazyQuery| async move {
                    let items = |from: i64| {
                        (from..from + 2).map(|id| {
                            LazyItem::new(
                                id.to_string(),
                                format!("{id:04}"),
                                Text::new(format!("Order CS-{}", 20_480 + id)),
                            )
                        })
                    };
                    match query.ask() {
                        LazyAsk::Since(_) => LazyPage::changes(items(1), "r9", ["7".to_owned()]),
                        LazyAsk::Before(_) => LazyPage::new(items(1), false),
                        _ => LazyPage::new(items(3), true),
                    }
                },
            )
            .public(),
            RouteDef::on(Method::GET, "!/agents/options", || async {
                ChoiceList::new()
                    .choice(Choice::new("1", "Harpreet Kaur").description("Ludhiana"))
                    .option("2", "Manpreet Singh")
                    .more("/agents/options?page=2")
            })
            .public(),
            RouteDef::on(
                Method::POST,
                "!/board/moves",
                |moved: Result<Move, MoveRejected>| async move {
                    match moved {
                        Ok(_) => Reply::ops(Vec::new()).into_response(),
                        Err(refused) => refused.into_response(),
                    }
                },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "!/files",
                |headers: http::HeaderMap| async move {
                    Received::default().respond(&headers, "/files")
                },
            )
            .public(),
            RouteDef::on(
                Method::POST,
                "!/orders/bulk",
                |bulk: BulkRequest<OrderRow>| async move { bulk.done("2 orders packed.") },
            )
            .public(),
            RouteDef::on(Method::GET, "!/orders/gone", || async {
                Err::<View, AppError>(AppError::not_found())
            })
            .public(),
            RouteDef::on(Method::POST, "!/orders/{id}/pack", || async {
                Err::<View, AppError>(AppError::conflict("Someone packed it a moment ago."))
            })
            .public(),
            RouteDef::on(Method::GET, "!/account", || async {
                Err::<View, AppError>(AppError::unauthenticated())
            })
            .public(),
            RouteDef::on(Method::GET, "!/deep", || async {
                let mut node: navree_ui::Node = Text::new("the bottom").into();
                for level in 0..100 {
                    node = el("section")
                        .children((Text::new(format!("level {level}")), node))
                        .into();
                }
                View::new("Deep").body(node)
            })
            .public(),
            RouteDef::on(Method::GET, "!/big", || async {
                View::new("Big").body(
                    (0..2000)
                        .map(|at| Text::new(format!("Line {at} of a long page")).into())
                        .collect::<Vec<navree_ui::Node>>(),
                )
            })
            .public(),
        ]
    }
}

struct App;

impl Module for App {
    fn register(&self, m: &mut ModuleBuilder) {
        m.factory(Factory::<Shell>::new(|_| async { Ok(Arc::new(shell())) }))
            .exports::<Shell>();
        m.controller::<Pages>();
    }
}

async fn app() -> TestApp {
    TestApp::builder()
        .module(HttpModule::for_root(Default::default()))
        .module(UiModule)
        .module(App)
        .build()
        .await
}

fn envelope(body: &str) -> Envelope {
    serde_json::from_str(body).unwrap_or_else(|error| panic!("{error}: {body}"))
}

#[tokio::test]
async fn a_native_client_gets_the_page_as_a_screen() {
    let app = app().await;
    let response = app
        .client()
        .get("/orders/1")
        .header(NATIVE, "1")
        .header(CLIENT, PHONE)
        .send()
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.header("content-type"),
        Some("application/vnd.navree.native+json")
    );
    assert_eq!(response.header("vary"), Some("x-navree-native"));
    assert_eq!(response.header("cache-control"), Some("no-store"));
    let envelope = envelope(&response.text());
    assert_eq!(envelope.navree, 1);
    // The app's stamp beside the web's build.
    assert_eq!(envelope.build, assets::BUILD);
    assert_eq!(envelope.app, Stamp::of(None).as_str());
    assert_eq!(envelope.vocab, navree_native::VOCAB);
    assert!(envelope.web.is_none());
    let screen = envelope.screen.expect("a screen");
    assert_eq!(screen.title, "Order CS-20481");
    assert_eq!(screen.nav.as_deref(), Some("/orders"));
    assert_eq!(
        screen.back.as_ref().map(|back| back.href.as_str()),
        Some("/orders")
    );
    assert_eq!(screen.trail.len(), 2);
    assert_eq!(screen.id.as_deref(), Some("order-page"));
    let [N::Button(print)] = screen.actions.as_slice() else {
        panic!("not one action");
    };
    assert_eq!(print.role, n::Role::Primary);
    assert_eq!(print.action, Some(Action::navigate("/orders/1/print")));
    let N::Stack(body) = &screen.body else {
        panic!("not a stack: {:?}", screen.body);
    };
    let [N::Badge(paid), N::Button(refund), N::Dialog(dialog)] = body.children.as_slice() else {
        panic!("not the parts: {:?}", body.children);
    };
    assert_eq!(paid.text, "Paid");
    assert_eq!(
        refund.action,
        Some(Action::command(Command::Open {
            id: "refund".into()
        }))
    );
    assert_eq!(dialog.title.as_deref(), Some("Refund CS-20481"));
    assert!(matches!(dialog.children.as_slice(), [N::Form(_)]));
}

#[tokio::test]
async fn a_table_page_asked_for_by_a_native_client_is_native_with_its_rows() {
    let app = app().await;
    let response = app
        .client()
        .get("/orders")
        .header(NATIVE, "1")
        .header(CLIENT, PHONE)
        .send()
        .await;
    let envelope = envelope(&response.text());
    let screen = envelope
        .screen
        .unwrap_or_else(|| panic!("{:?}", envelope.web));
    let table = screen.body.find("orders").expect("the table's region");
    let N::Region(region) = table else {
        panic!("not a region");
    };
    let [N::Table(table)] = region.children.as_slice() else {
        panic!("not one table");
    };
    assert_eq!(table.rows.len(), 2);
    assert!(matches!(screen.body.find("order"), Some(N::Sheet(_))));
    // The address of the next rows asks for rows, not a screen: the answer adds them to
    // the table, and says where the ones after them are.
    let more = table.more.clone().expect("more rows");
    assert!(more.contains("page=2") && more.ends_with("&_rows=orders"), "{more}");
    let response = app
        .client()
        .get(&more)
        .header(NATIVE, "1")
        .header(CLIENT, PHONE)
        .send()
        .await;
    let next: Envelope = serde_json::from_str(&response.text()).expect("an envelope");
    assert!(next.screen.is_none() && next.web.is_none(), "{next:?}");
    let [
        NativeOp::Rows {
            target,
            rows,
            at,
            more,
        },
    ] = next.ops.as_slice()
    else {
        panic!("not one op of rows: {:?}", next.ops);
    };
    assert_eq!((target.as_str(), rows.len()), ("orders-table", 2));
    assert_eq!(*at, navree_native::op::Edge::End);
    assert!(
        more.as_deref().is_some_and(|more| more.ends_with("&_rows=orders")),
        "{more:?}"
    );
    // Asked as a browser asks, the address is the page it always was.
    let html = app.client().get("/orders?page=2").send().await;
    assert!(html.text().contains("data-slot=\"data-table\""));
}

#[tokio::test]
async fn a_browser_gets_the_html_it_always_got() {
    let app = app().await;
    let response = app.client().get("/orders/1").send().await;
    assert_eq!(
        response.header("content-type"),
        Some("text/html; charset=utf-8")
    );
    // A cache keeps it apart from what a native client gets for the same URL.
    assert_eq!(response.header("vary"), Some("x-navree-native"));
    let expected = order_page(&shell().for_request("/orders/1", &http::HeaderMap::new()))
        .to_view()
        .to_html();
    assert_eq!(response.text(), expected);
    // The table page, whose rows are written as markup for a browser.
    let response = app.client().get("/orders").send().await;
    let query: TableQuery<OrderRow, NoFilter> = TableQuery::parse("/orders", "");
    let page = TablePage::new(vec![order(1, "open"), order(2, "completed")], 5);
    let expected = shell()
        .for_request("/orders", &http::HeaderMap::new())
        .page("Orders")
        .crumb("Dashboard", "/")
        .body((orders_table(&query, &page), Sheet::new("order", "Order")))
        .to_view()
        .to_html();
    assert_eq!(response.text(), expected);
    // And the runtime gets its ops as markup.
    let response = app
        .client()
        .post("/orders/1/refund")
        .header("x-navree-ui", "1")
        .send()
        .await;
    assert_eq!(
        response.header("content-type"),
        Some("application/vnd.navree.ops+json")
    );
    let body = response.text();
    assert!(
        body.contains(r#"{"op":"morph","target":"order-page","html":"<div id=\"order-page\">"#),
        "{body}"
    );
}

#[tokio::test]
async fn a_reply_is_ops_with_nodes_for_a_native_client() {
    let app = app().await;
    let response = app
        .client()
        .post("/orders/1/refund")
        .header(NATIVE, "1")
        .header(CLIENT, PHONE)
        .send()
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.header("content-type"),
        Some("application/vnd.navree.native+json")
    );
    // The toast is on its way as an op, so no cookie carries it to the next page.
    assert_eq!(response.header("set-cookie"), None);
    assert_eq!(response.header("location"), None);
    let envelope = envelope(&response.text());
    assert!(envelope.screen.is_none());
    let [
        NativeOp::Close { target },
        NativeOp::Morph {
            target: morphed,
            node,
        },
        NativeOp::Toast(toast),
    ] = envelope.ops.as_slice()
    else {
        panic!("not the ops: {:?}", envelope.ops);
    };
    assert_eq!(target, "refund");
    assert_eq!(morphed, "order-page");
    assert_eq!(node.id(), Some("order-page"));
    let N::Stack(body) = node else {
        panic!("not a stack");
    };
    assert!(matches!(&body.children[0], N::Badge(badge) if badge.text == "Refunded"));
    assert_eq!(toast.text, "₹7,848 went back.");
    assert_eq!(toast.tone, n::Tone::Success);
}

#[tokio::test]
async fn a_form_that_is_refused_answers_a_native_client_with_the_messages_at_its_fields() {
    let app = app().await;
    let post = |body: &'static str, native: bool| {
        let mut request = app
            .client()
            .post("/refunds")
            .header("content-type", "application/x-www-form-urlencoded");
        if native {
            request = request.header(NATIVE, "1").header(CLIENT, PHONE);
        }
        request.body(body).send()
    };
    let refused = post("amount=abc&reason=late", true).await;
    assert_eq!(refused.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        refused.header("content-type"),
        Some("application/vnd.navree.native+json")
    );
    let envelope = envelope(&refused.text());
    let [NativeOp::Errors { form, fields }] = envelope.ops.as_slice() else {
        panic!("not errors: {:?}", envelope.ops);
    };
    assert_eq!(form, "refund-form");
    assert!(fields.contains_key("amount"), "{fields:?}");
    // What was right goes through, as ops.
    let taken = post("amount=10.00&reason=late", true).await;
    assert_eq!(taken.status(), StatusCode::OK);
    assert!(matches!(
        self::envelope(&taken.text()).ops.as_slice(),
        [NativeOp::Toast(toast)] if toast.text == "1000 paise went back."
    ));
    // A browser without scripts still gets the form on a page, with what was typed.
    let page = post("amount=abc&reason=late", false).await;
    assert_eq!(page.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        page.header("content-type"),
        Some("text/html; charset=utf-8")
    );
    assert!(page.text().contains(r#"value="abc""#));
}

#[tokio::test]
async fn a_reply_that_can_not_be_said_natively_answers_its_fallback() {
    let app = app().await;
    let response = app
        .client()
        .post("/board/move")
        .header(NATIVE, "1")
        .header(CLIENT, PHONE)
        .send()
        .await;
    // What a browser without scripts gets: the client follows it to the page.
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.header("location"), Some("/board"));
    // A reply with no ops at all has nothing to say but its fallback.
    let response = app
        .client()
        .get("/columns/doing")
        .header(NATIVE, "1")
        .header(CLIENT, PHONE)
        .send()
        .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.header("location"), Some("/board?show=20"));
}

#[tokio::test]
async fn a_page_with_a_part_no_reader_reads_is_sent_to_the_web_view_with_why() {
    let app = app().await;
    let response = app
        .client()
        .get("/board?lane=2")
        .header(NATIVE, "1")
        .header(CLIENT, PHONE)
        .send()
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let envelope = envelope(&response.text());
    assert!(envelope.screen.is_none());
    let web = envelope.web.expect("the web view");
    assert_eq!(web.url, "/board?lane=2");
    assert_eq!(web.title.as_deref(), Some("Board"));
    assert_eq!(web.why, ["tag:canvas"]);
}

#[tokio::test]
async fn markup_a_handler_rendered_itself_never_reaches_a_native_client_as_it_is() {
    let app = app().await;
    let response = app
        .client()
        .get("/fragments/feed")
        .header(NATIVE, "1")
        .header(CLIENT, PHONE)
        .send()
        .await;
    assert_eq!(
        response.header("content-type"),
        Some("application/vnd.navree.native+json")
    );
    let envelope = envelope(&response.text());
    let web = envelope.web.expect("the web view");
    assert_eq!(web.url, "/fragments/feed");
    assert_eq!(web.why, ["html"]);
    // A fragment that is a `View` is a screen with no shell around it.
    let response = app
        .client()
        .get("/fragments/figures")
        .header(NATIVE, "1")
        .header(CLIENT, PHONE)
        .send()
        .await;
    let envelope = self::envelope(&response.text());
    let screen = envelope.screen.expect("a screen");
    assert_eq!(screen.title, "Figures");
    assert_eq!(screen.body, N::text("952 orders today"));
    assert_eq!(screen.nav, None);
}

#[tokio::test]
async fn the_web_view_of_a_native_client_gets_the_page_without_the_chrome() {
    let app = app().await;
    // The first load may say so with a header; every load ends its user agent with the token.
    let first = app
        .client()
        .get("/board")
        .header("x-navree-embed", "1")
        .send()
        .await
        .text();
    let response = app
        .client()
        .get("/board")
        .header(
            "user-agent",
            "Mozilla/5.0 (Linux; Android 16; wv) Chrome/140 Mobile Safari/537.36 NavreeNative/1 (platform=android; lib=0.1.0)",
        )
        .send()
        .await;
    assert_eq!(
        response.header("content-type"),
        Some("text/html; charset=utf-8")
    );
    assert_eq!(
        response.header("vary"),
        Some("x-navree-native, x-navree-embed, user-agent")
    );
    let html = response.text();
    assert_eq!(html, first);
    assert!(html.contains("<canvas></canvas>"), "{html}");
    assert!(html.contains(r#"<main data-slot="page""#), "{html}");
    assert!(html.contains("<title>Board</title>"), "{html}");
    for chrome in [
        r#"data-slot="sidebar""#,
        r#"data-slot="topbar""#,
        r#"data-slot="page-title""#,
        "sidebar-",
        "data-ui-boost",
    ] {
        assert!(!html.contains(chrome), "{chrome}: {html}");
    }
    // The same address for a browser has it all.
    let whole = app.client().get("/board").send().await.text();
    assert!(whole.contains(r#"data-slot="sidebar""#));
    assert!(whole.len() > html.len());
}

#[tokio::test]
async fn the_shell_is_answered_as_a_document() {
    let app = app().await;
    let response = app.client().get("/_navree/native/shell").send().await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.header("content-type"),
        Some("application/vnd.navree.native+json")
    );
    let shell: navree_native::Shell = serde_json::from_str(&response.text()).expect("a shell");
    assert_eq!(shell.navree, 1);
    assert_eq!(shell.build, assets::BUILD);
    // The document's stamp is the stamp of what it says.
    assert_eq!(shell.stamp, shell.clone().stamped().stamp);
    assert!(!shell.stamp.is_empty());
    // A page drawn in the shell says the same stamp, so the client knows its shell is current.
    let page = app
        .client()
        .get("/orders/1")
        .header(NATIVE, "1")
        .header(CLIENT, PHONE)
        .send()
        .await;
    let page = envelope(&page.text());
    assert_eq!(page.shell, shell.stamp);
    assert_eq!(page.app, Stamp::of(None).as_str());
    assert_eq!(shell.name, "Charkha Supply Co.");
    assert_eq!(shell.context.as_deref(), Some("Store operations"));
    let [store] = shell.nav.groups.as_slice() else {
        panic!("not one group");
    };
    assert_eq!(
        store
            .items
            .iter()
            .map(|item| (item.label.as_str(), item.href.as_str()))
            .collect::<Vec<_>>(),
        [("Dashboard", "/"), ("Orders", "/orders")]
    );
    assert!(store.items.iter().all(|item| item.icon.is_some()));
    let user = shell.user.expect("a user");
    assert_eq!(user.name, "Ravneet Kaur");
    assert!(matches!(
        user.menu.as_slice(),
        [N::Button(out)] if out.label == "Sign out"
    ));
}

#[tokio::test]
async fn a_client_the_server_can_not_read_is_refused_before_a_handler_runs() {
    let app = app().await;
    let response = app
        .client()
        .get("/orders/1")
        .header(NATIVE, "2")
        .header(CLIENT, PHONE)
        .send()
        .await;
    assert_eq!(response.status(), StatusCode::NOT_ACCEPTABLE);
    let response = app
        .client()
        .get("/orders/1")
        .header(NATIVE, "1")
        .header(CLIENT, "platform=beos; form=phone")
        .send()
        .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(response.text().contains("beos"));
    let response = app
        .client()
        .get("/orders/1")
        .header(NATIVE, "1")
        .send()
        .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_page_deeper_than_a_client_reads_is_sent_to_the_web_view() {
    let app = app().await;
    let response = app
        .client()
        .get("/deep")
        .header(NATIVE, "1")
        .header(CLIENT, "platform=ios; form=phone; vocab=0")
        .send()
        .await;
    let envelope = envelope(&response.text());
    assert!(envelope.screen.is_none());
    let web = envelope.web.expect("the web view");
    assert_eq!(web.why, ["depth"]);
    assert_eq!(web.url, "/deep");
}

#[tokio::test]
async fn a_big_envelope_goes_out_in_brotli_to_a_client_that_takes_it() {
    let app = app().await;
    let plain = app
        .client()
        .get("/big")
        .header(NATIVE, "1")
        .header(CLIENT, PHONE)
        .send()
        .await;
    assert_eq!(plain.header("content-encoding"), None);
    let size = plain.text().len();
    assert!(size > 64 * 1024, "{size}");
    let squeezed = app
        .client()
        .get("/big")
        .header(NATIVE, "1")
        .header(CLIENT, PHONE)
        .header("accept-encoding", "br")
        .send()
        .await;
    assert_eq!(squeezed.header("content-encoding"), Some("br"));
}

#[tokio::test]
async fn both_answers_vary_on_who_asked() {
    let app = app().await;
    let client = app.client();
    let varies = |vary: Option<&str>| {
        vary.unwrap_or_default()
            .split(',')
            .any(|name| name.trim().eq_ignore_ascii_case("x-navree-native"))
    };
    for path in [
        "/orders/1",
        "/orders",
        "/fragments/figures",
        "/fragments/feed",
    ] {
        let browser = client.get(path).send().await;
        assert!(
            browser
                .header("content-type")
                .is_some_and(|kind| kind.starts_with("text/html")),
            "{path}"
        );
        let vary: Vec<String> = browser
            .headers()
            .get_all("vary")
            .iter()
            .map(|v| v.to_str().unwrap().to_owned())
            .collect();
        assert!(vary.iter().any(|v| varies(Some(v))), "{path}: {vary:?}");
        let native = client
            .get(path)
            .header(NATIVE, "1")
            .header(CLIENT, PHONE)
            .send()
            .await;
        assert_eq!(
            native.header("content-type"),
            Some("application/vnd.navree.native+json"),
            "{path}"
        );
        assert!(varies(native.header("vary")), "{path}");
    }
    // The runtime's ops, and the native client's, for one action.
    let runtime = client
        .post("/orders/1/refund")
        .header("x-navree-ui", "1")
        .send()
        .await;
    let vary: Vec<String> = runtime
        .headers()
        .get_all("vary")
        .iter()
        .map(|v| v.to_str().unwrap().to_owned())
        .collect();
    assert!(vary.iter().any(|v| varies(Some(v))), "{vary:?}");
    // The web view's page is neither.
    let web_view = client
        .get("/orders/1")
        .header(
            "user-agent",
            "Mozilla/5.0 (iPhone) AppleWebKit/605.1.15 NavreeNative/1 (platform=ios; lib=0.1.0)",
        )
        .send()
        .await;
    let vary = web_view.header("vary").unwrap_or_default().to_owned();
    assert!(
        vary.contains("x-navree-native") && vary.contains("user-agent"),
        "{vary}"
    );
    // An error page varies on it too.
    let missing = client
        .get("/nowhere")
        .header("accept", "text/html")
        .send()
        .await;
    let vary: Vec<String> = missing
        .headers()
        .get_all("vary")
        .iter()
        .map(|v| v.to_str().unwrap().to_owned())
        .collect();
    assert!(vary.iter().any(|v| varies(Some(v))), "{vary:?}");
}

#[tokio::test]
async fn a_native_request_is_recorded_as_the_runtime_s_are() {
    use navree_telemetry::{TelemetryRecorder, UiOperationKind};

    let recorder = TelemetryRecorder::new();
    let app = TestApp::builder()
        .module(HttpModule::for_root(Default::default()))
        .module(UiModule)
        .module(App)
        .module(recorder.module())
        .build()
        .await;
    let client = app.client();
    client
        .get("/orders/1")
        .header(NATIVE, "1")
        .header(CLIENT, PHONE)
        .send()
        .await;
    client
        .post("/orders/1/refund")
        .header(NATIVE, "1")
        .header(CLIENT, PHONE)
        .header("content-type", "application/x-www-form-urlencoded")
        .send()
        .await;
    client
        .get("/fragments/figures")
        .header(NATIVE, "1")
        .header(CLIENT, PHONE)
        .header("x-navree-region", "figures")
        .send()
        .await;
    // A fetch ahead of a tap is not an operation, nor is a browser's page.
    client
        .get("/orders/1")
        .header(NATIVE, "1")
        .header(CLIENT, PHONE)
        .header("x-navree-prefetch", "1")
        .send()
        .await;
    client.get("/orders/1").send().await;
    let operations = recorder.ui_operations();
    let seen: Vec<(&str, UiOperationKind, u16)> = operations
        .iter()
        .map(|op| (op.name.as_str(), op.kind, op.status))
        .collect();
    assert_eq!(
        seen,
        [
            ("/orders/{id}", UiOperationKind::Fragment, 200),
            ("/orders/{id}/refund", UiOperationKind::FormPost, 200),
            ("/fragments/figures", UiOperationKind::LiveRegion, 200),
        ]
    );
    assert!(operations[1].ops >= 3, "the close, the morph and the toast");
    app.shutdown().await.unwrap();
}

/// A native client's request for `path`, with `pairs` beside its own headers.
async fn ask_native(
    app: &TestApp,
    method: Method,
    path: &str,
    pairs: &[(&str, &str)],
    body: &str,
) -> (StatusCode, Envelope) {
    let client = app.client();
    let mut request = if method == Method::POST {
        client
            .post(path)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(body.to_owned())
    } else {
        client.get(path)
    };
    request = request.header(NATIVE, "1").header(CLIENT, PHONE);
    for (name, value) in pairs {
        request = request.header(*name, *value);
    }
    let response = request.send().await;
    let status = response.status();
    let kind = response.header("content-type").map(str::to_owned);
    let text = response.text();
    assert_eq!(
        kind.as_deref(),
        Some("application/vnd.navree.native+json"),
        "{path}: {text}"
    );
    (status, envelope(&text))
}

#[tokio::test]
async fn a_lazy_list_page_is_rows_for_a_native_client() {
    let app = app().await;
    let region = [("x-navree-region", "orders-items")];
    let (status, after) = ask_native(
        &app,
        Method::GET,
        "/things/rows?view=open&dir=end&cursor=0002",
        &region,
        "",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let [
        NativeOp::Append {
            target,
            nodes,
            more,
        },
    ] = after.ops.as_slice()
    else {
        panic!("not one append: {:?}", after.ops);
    };
    assert_eq!(target, "orders-items");
    assert_eq!(
        nodes.iter().map(|node| node.id()).collect::<Vec<_>>(),
        [Some("3"), Some("4")]
    );
    assert!(matches!(&nodes[0], N::Text(text) if text.value == "Order CS-20483"));
    // The rows after these are at the last one's cursor, the list's own query kept.
    assert_eq!(
        more.as_deref(),
        Some("/things/rows?view=open&dir=end&cursor=0004")
    );
    // The rows before go at the start, and a live ask has no native form yet:
    // the client fetches the list again.
    let (_, before) = ask_native(
        &app,
        Method::GET,
        "/things/rows?dir=start&cursor=0003",
        &region,
        "",
    )
    .await;
    assert!(
        matches!(before.ops.as_slice(), [NativeOp::Prepend { nodes, .. }] if nodes.len() == 2),
        "{:?}",
        before.ops
    );
    let (_, live) = ask_native(
        &app,
        Method::GET,
        "/things/rows?dir=live&since=r1",
        &region,
        "",
    )
    .await;
    assert!(
        matches!(live.ops.as_slice(), [NativeOp::Invalidate { target }] if target == "orders-items"),
        "{:?}",
        live.ops
    );
    // A browser's runtime gets the rows as markup, as before.
    let web = app
        .client()
        .get("/things/rows?dir=end&cursor=0002")
        .send()
        .await;
    assert_eq!(web.header("x-ui-more"), Some("1"));
    assert!(web.text().starts_with(r#"<li data-key="3""#));
}

#[tokio::test]
async fn an_option_query_is_a_choice_for_a_native_client() {
    let app = app().await;
    let (_, answer) = ask_native(&app, Method::GET, "/agents/options?q=kaur", &[], "").await;
    let screen = answer.screen.expect("a screen");
    let N::Stack(stack) = &screen.body else {
        panic!("not a stack: {:?}", screen.body);
    };
    let [N::Field(field), N::Link(more)] = stack.children.as_slice() else {
        panic!("not a choice and a link: {:?}", stack.children);
    };
    let Input::Choice(choice) = &field.input else {
        panic!("not a choice");
    };
    let options: Vec<(&str, &str, Option<&str>)> = choice
        .options
        .iter()
        .map(|option| {
            (
                option.value.as_str(),
                option.label.as_str(),
                option.detail.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        options,
        [
            ("1", "Harpreet Kaur", Some("Ludhiana")),
            ("2", "Manpreet Singh", None)
        ]
    );
    assert!(
        matches!(&more.action, Action::Navigate { href, .. } if href == "/agents/options?page=2")
    );
    let web = app.client().get("/agents/options?q=kaur").send().await;
    assert!(web.text().contains(r#"data-slot="option-page""#));
}

#[tokio::test]
async fn a_refused_move_is_a_toast_for_a_native_client() {
    let app = app().await;
    let (status, answer) = ask_native(&app, Method::POST, "/board/moves", &[], "item=").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        matches!(answer.ops.as_slice(), [NativeOp::Toast(toast)] if toast.text.contains("couldn't be read")),
        "{:?}",
        answer.ops
    );
}

#[tokio::test]
async fn an_upload_and_a_bulk_action_answer_a_native_client_with_ops() {
    let app = app().await;
    let (_, upload) = ask_native(
        &app,
        Method::POST,
        "/files",
        &[("accept", "application/json")],
        "",
    )
    .await;
    assert!(
        upload.ops.iter().any(|op| matches!(op, NativeOp::Toast(_)))
            && upload
                .ops
                .iter()
                .any(|op| matches!(op, NativeOp::Navigate { href, .. } if href == "/files")),
        "{:?}",
        upload.ops
    );
    let (_, bulk) = ask_native(
        &app,
        Method::POST,
        "/orders/bulk",
        &[],
        "action=pack&ids=1&ids=2&_table=orders&_path=/orders",
    )
    .await;
    assert!(
        matches!(bulk.ops.as_slice(), [NativeOp::Toast(toast), NativeOp::Invalidate { target }]
            if toast.text == "2 orders packed." && target == "orders"),
        "{:?}",
        bulk.ops
    );
}

#[tokio::test]
async fn an_error_page_is_a_screen_and_an_action_s_error_a_toast() {
    let app = app().await;
    let (status, gone) = ask_native(&app, Method::GET, "/orders/gone", &[], "").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let screen = gone.screen.unwrap_or_else(|| panic!("{:?}", gone.web));
    assert!(!screen.title.is_empty());
    let (status, refused) = ask_native(&app, Method::POST, "/orders/1/pack", &[], "").await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(
        matches!(refused.ops.as_slice(), [NativeOp::Toast(toast)] if toast.text == "Someone packed it a moment ago."),
        "{:?}",
        refused.ops
    );
}

#[tokio::test]
async fn a_signed_out_native_client_is_sent_to_sign_in() {
    let app = app().await;
    let (status, answer) = ask_native(&app, Method::GET, "/account?tab=orders", &[], "").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // One op the client follows: the sign-in screen, which comes back here after.
    assert_eq!(
        answer.ops,
        [NativeOp::Navigate {
            href: "/sign-in?next=%2Faccount%3Ftab%3Dorders".to_owned(),
            present: navree_native::Present::Auto,
        }]
    );
    assert!(answer.screen.is_none() && answer.web.is_none());
    // The shell says how a client presents it: over everything, with no chrome.
    let shell: navree_native::Shell = serde_json::from_str(
        &app.client()
            .get("/_navree/native/shell")
            .send()
            .await
            .text(),
    )
    .expect("a shell");
    let sign_in = shell
        .routes
        .iter()
        .find(|route| route.matches == "/sign-in")
        .expect("a route for sign-in");
    assert_eq!(sign_in.present, Some(navree_native::action::Present::Full));
    assert!(!sign_in.chrome);
}

/// An app that builds its shell for each request, as the error pages do, and
/// registers no one shell for everyone.
struct PerRequest;

impl Module for PerRequest {
    fn register(&self, m: &mut ModuleBuilder) {
        m.contribute(navree_ui::ErrorShell::new(|request| async move {
            let who = if request.user.is_some() {
                "Signed in"
            } else {
                "Guest"
            };
            Some(
                Shell::new("Sukhna Support")
                    .context(who)
                    .group(NavGroup::plain().item(NavItem::new("Inbox", "/inbox").count("3"))),
            )
        }));
    }
}

#[tokio::test]
async fn the_shell_comes_from_the_error_pages_builder() {
    let app = TestApp::builder()
        .module(HttpModule::for_root(Default::default()))
        .module(UiModule)
        .module(PerRequest)
        .build()
        .await;
    let response = app.client().get("/_navree/native/shell").send().await;
    let shell: navree_native::Shell = serde_json::from_str(&response.text()).expect("a shell");
    assert_eq!(shell.name, "Sukhna Support");
    assert_eq!(shell.context.as_deref(), Some("Guest"));
    assert_eq!(shell.nav.groups[0].items[0].badge.as_deref(), Some("3"));
    assert!(shell.routes.iter().any(|route| route.matches == "/sign-in"));
    assert!(
        shell
            .theme
            .as_ref()
            .is_some_and(|theme| theme.accent.is_some() && theme.accent_dark.is_some())
    );
}

/// A customer, as a table that says what its columns are to a record.
#[derive(Clone, TableRow)]
#[table(sort = "name", per = 2, per_pages(2, 10))]
struct CustomerRow {
    #[col(skip)]
    id: i64,
    #[col(sortable, sticky, role = title)]
    name: String,
    #[col(hidden, role = subtitle)]
    city: String,
    #[col(role = status)]
    state: String,
    #[col(numeric, role = trailing)]
    spent: i64,
}

/// The same table, saying nothing of its columns.
#[derive(Clone, TableRow)]
#[table(sort = "name", per = 2, per_pages(2, 10))]
struct PlainCustomerRow {
    #[col(skip)]
    id: i64,
    #[col(sortable, sticky)]
    name: String,
    #[col(hidden)]
    city: String,
    state: String,
    #[col(numeric)]
    spent: i64,
}

fn customers() -> Vec<CustomerRow> {
    [
        (1, "Gurpreet Sandhu", "Amritsar", "Active", 124_500),
        (2, "Simran Grewal", "Jalandhar", "Paused", 8_400),
    ]
    .into_iter()
    .map(|(id, name, city, state, spent)| CustomerRow {
        id,
        name: name.to_owned(),
        city: city.to_owned(),
        state: state.to_owned(),
        spent,
    })
    .collect()
}

fn customers_view(roles: bool, opens: bool) -> View {
    let query: TableQuery<CustomerRow, NoFilter> = TableQuery::parse("/customers", "");
    let page = TablePage::new(customers(), 2);
    let mut table =
        DataTable::new("customers", &query, &page).row_href(|row| format!("/customers/{}", row.id));
    if opens {
        table = table.row_opens(|row| format!("/customers/{}/peek", row.id));
    }
    let new = Button::primary("New customer").href("/customers/new");
    let new = if roles {
        new.role(navree_ui::Role::Create)
    } else {
        new
    };
    View::new("Customers").body((new, table))
}

#[test]
fn a_button_says_its_role_only_to_a_native_client() {
    let build = || {
        View::new("x").body((
            Button::primary("New customer")
                .href("/customers/new")
                .role(navree_ui::Role::Create),
            Button::outline("Back")
                .href("/")
                .role(navree_ui::Role::Navigation),
            Button::new("Keep"),
        ))
    };
    let page = TestNative::build(phone(), build);
    let roles: Vec<(String, n::Role)> = page
        .screen()
        .body
        .children()
        .into_iter()
        .filter_map(|node| match node {
            N::Button(button) => Some((button.label.clone(), button.role)),
            _ => None,
        })
        .collect();
    assert_eq!(
        roles,
        [
            ("New customer".to_owned(), n::Role::Create),
            ("Back".to_owned(), n::Role::Navigation),
            ("Keep".to_owned(), n::Role::None),
        ]
    );
    // Built for a browser, the page carries no word of it.
    let plain = View::new("x").body((
        Button::primary("New customer").href("/customers/new"),
        Button::outline("Back").href("/"),
        Button::new("Keep"),
    ));
    assert_eq!(build().to_html(), plain.to_html());
}

#[test]
fn a_table_says_its_column_roles_and_what_a_row_opens() {
    let page = TestNative::build(phone(), || customers_view(true, true));
    assert!(page.is_native(), "{:?}", page.why());
    let Some(N::Region(region)) = page.find("customers") else {
        panic!("no table");
    };
    let [N::Table(table)] = region.children.as_slice() else {
        panic!("not one table");
    };
    let columns: Vec<(&str, Option<n::ColumnRole>, bool)> = table
        .columns
        .iter()
        .map(|column| (column.label.as_str(), column.role, column.secondary))
        .collect();
    assert_eq!(
        columns,
        [
            ("Name", Some(n::ColumnRole::Title), false),
            ("State", Some(n::ColumnRole::Status), false),
            ("Spent", Some(n::ColumnRole::Trailing), false),
            // The hidden column the page named as each record's line under its name.
            ("City", Some(n::ColumnRole::Subtitle), true),
        ]
    );
    let first = &table.rows[0];
    assert_eq!(first.cells.len(), table.columns.len());
    assert!(matches!(&first.cells[3], N::Text(text) if text.value == "Amritsar"));
    // A tap on a row opens the record where the reader is.
    assert_eq!(
        first.action,
        Some(Action::Request {
            method: n::Method::Get,
            href: "/customers/1/peek".to_owned(),
            fields: Vec::new(),
        })
    );
    // With nothing it opens, a row leads where its link goes.
    let leads = TestNative::build(phone(), || customers_view(true, false));
    let Some(N::Region(region)) = leads.find("customers") else {
        panic!("no table");
    };
    let [N::Table(table)] = region.children.as_slice() else {
        panic!("not one table");
    };
    assert_eq!(table.rows[0].action, Some(Action::navigate("/customers/1")));
}

#[test]
fn hints_never_change_the_html() {
    // Built for a browser, a page with every hint is the page without them.
    let with = customers_view(true, true).to_html();
    let without = customers_view(false, false).to_html();
    assert_eq!(with, without);
    // And a table whose derive says roles writes what the same table without them writes.
    let query: TableQuery<PlainCustomerRow, NoFilter> = TableQuery::parse("/customers", "");
    let rows = customers()
        .into_iter()
        .map(|row| PlainCustomerRow {
            id: row.id,
            name: row.name,
            city: row.city,
            state: row.state,
            spent: row.spent,
        })
        .collect();
    let page = TablePage::new(rows, 2);
    let plain = View::new("Customers")
        .body((
            Button::primary("New customer").href("/customers/new"),
            DataTable::new("customers", &query, &page)
                .row_href(|row| format!("/customers/{}", row.id)),
        ))
        .to_html();
    assert_eq!(with, plain);
}

#[test]
fn a_reused_node_writes_the_html_it_always_did() {
    // A page and a button built while a native client asked, kept, and written
    // for a browser later.
    let mut kept_view = None;
    let mut kept_button = None;
    TestNative::build(phone(), || {
        kept_button = Some(navree_ui::Node::from(
            Button::primary("Refund")
                .href("/orders/1/refund")
                .role(navree_ui::Role::Destructive),
        ));
        let view = order_page(&shell().for_request("/orders/1", &http::HeaderMap::new())).to_view();
        kept_view = Some(view.clone());
        view
    });
    let browser = order_page(&shell().for_request("/orders/1", &http::HeaderMap::new()))
        .to_view()
        .to_html();
    assert_eq!(kept_view.expect("a view").to_html(), browser);
    assert_eq!(
        render(&kept_button.expect("a button")),
        render(&Button::primary("Refund").href("/orders/1/refund").into())
    );
    // A part built outside any request is read as markup, and nothing panics.
    let at_start = Button::primary("Scan")
        .href("/scan")
        .role(navree_ui::Role::Primary);
    let page = TestNative::of(&View::new("x").body(at_start));
    assert!(page.is_native());
}

#[tokio::test]
async fn a_reused_page_answers_a_browser_with_its_html() {
    // A view an app kept from a native client's request, answered to a browser.
    let app = TestApp::builder()
        .module(HttpModule::for_root(Default::default()))
        .module(UiModule)
        .module(App)
        .module(Reuse)
        .build()
        .await;
    let native = app
        .client()
        .get("/kept")
        .header(NATIVE, "1")
        .header(CLIENT, PHONE)
        .send()
        .await;
    assert!(envelope(&native.text()).screen.is_some());
    let browser = app.client().get("/kept").send().await.text();
    let expected = order_page(&shell().for_request("/orders/1", &http::HeaderMap::new()))
        .to_view()
        .to_html();
    assert_eq!(browser, expected);
}

/// A route that keeps the view it built for its first request, whoever asked,
/// and answers every later request with it.
struct Reuse;

impl Module for Reuse {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<KeptPage>();
    }
}

struct KeptPage;

impl Injectable for KeptPage {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for KeptPage {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        static KEPT: std::sync::OnceLock<View> = std::sync::OnceLock::new();
        vec![
            RouteDef::on(Method::GET, "!/kept", || async {
                KEPT.get_or_init(|| {
                    order_page(&shell().for_request("/orders/1", &http::HeaderMap::new())).to_view()
                })
                .clone()
            })
            .public(),
        ]
    }
}

/// A layer where the unit of work stands that runs the rest of every request
/// on a task of its own, as the unit of work does when a client goes away.
struct ElsewhereTask;

impl Module for ElsewhereTask {
    fn register(&self, m: &mut ModuleBuilder) {
        m.http_layer(navree_http::GlobalLayer::new(
            navree_http::LayerSlot::Uow,
            "test.elsewhere",
            |_, router| {
                Ok(router.layer(navree_http::middleware::from_fn(
                    |request: navree_http::extract::Request,
                     next: navree_http::middleware::Next| async move {
                        tokio::spawn(next.run(request))
                            .await
                            .expect("the request's task")
                    },
                )))
            },
        ));
    }
}

#[tokio::test]
async fn who_asked_is_current_where_the_handler_runs_even_on_another_task() {
    let app = TestApp::builder()
        .module(HttpModule::for_root(Default::default()))
        .module(UiModule)
        .module(App)
        .module(ElsewhereTask)
        .build()
        .await;
    let response = app
        .client()
        .get("/orders")
        .header(NATIVE, "1")
        .header(CLIENT, PHONE)
        .send()
        .await;
    let envelope = envelope(&response.text());
    // The table's rows are built as parts only when the handler knows a
    // native client asked.
    let screen = envelope
        .screen
        .unwrap_or_else(|| panic!("{:?}", envelope.web));
    let Some(N::Region(region)) = screen.body.find("orders") else {
        panic!("no table");
    };
    let [N::Table(table)] = region.children.as_slice() else {
        panic!("not one table");
    };
    assert_eq!(table.rows.len(), 2);
}

/// The orders with a box to select each row, a menu of its actions, and a detail
/// that opens under it.
fn full_table<'a>(
    query: &'a TableQuery<OrderRow, NoFilter>,
    page: &'a TablePage<OrderRow>,
) -> DataTable<'a, OrderRow, NoFilter> {
    orders_table(query, page)
        .csrf("tok-7")
        .bulk(
            "/orders/bulk",
            [navree_ui::table::BulkAction::new("archive", "Archive")],
        )
        .row_actions(|row| {
            vec![
                navree_ui::RowAction::link("Open", format!("/orders/{}", row.id)),
                navree_ui::RowAction::post("Cancel", format!("/orders/{}/cancel", row.id))
                    .destructive(),
            ]
        })
        .detail(|row| Text::new(format!("Notes on {}", row.number)).into())
}

fn full_view(query: &str) -> View {
    let query: TableQuery<OrderRow, NoFilter> = TableQuery::parse("/orders", query);
    let page = TablePage::new(vec![order(1, "open"), order(2, "completed")], 5);
    View::new("Orders").body(full_table(&query, &page))
}

#[test]
fn a_data_table_with_a_box_a_menu_and_a_detail_reads_as_the_parts_it_has() {
    let page = TestNative::build(phone(), || full_view("open=1"));
    assert!(page.is_native(), "{:?}", page.page().misses);
    let Some(N::Region(region)) = page.find("orders") else {
        panic!("no table");
    };
    // The form that applies an action to the rows chosen stands over the table.
    let [N::Form(bulk), N::Table(table)] = region.children.as_slice() else {
        panic!("not a form and a table: {:?}", region.children);
    };
    assert_eq!(bulk.method, n::Method::Post);
    assert_eq!(bulk.action, "/orders/bulk");
    assert!(
        bulk.hidden
            .iter()
            .any(|hidden| hidden.name == "_csrf" && hidden.value == "tok-7")
    );
    let archive = bulk
        .children
        .iter()
        .find_map(|node| match node {
            N::Button(button) => Some(button),
            _ => None,
        })
        .expect("the action");
    assert_eq!(archive.label, "Archive");
    // The boxes are the table's: it says how a chosen row is sent, and with which form.
    assert_eq!(
        table.select,
        Some(n::Selection {
            name: "ids".into(),
            form: Some("orders-b".into())
        })
    );
    // The columns: the four shown. The box, the control for the detail and the menu of a
    // row's actions are the table's and the row's own, not columns.
    let labels: Vec<&str> = table
        .columns
        .iter()
        .map(|column| column.label.as_str())
        .collect();
    assert_eq!(
        labels,
        ["Order", "Status", "Total", "Placed"]
    );
    // Each row opens its detail itself, with no request; the row says whether it is chosen.
    assert_eq!(table.rows.len(), 2);
    let first = &table.rows[0];
    assert_eq!(first.cells.len(), 4);
    assert_eq!(first.id.as_deref(), Some("1"));
    assert!(!first.selected);
    assert_eq!(first.detail, [N::text("Notes on CS-20481")]);
    assert_eq!(table.rows[1].detail, [N::text("Notes on CS-20482")]);
    // The menu: a link and a post that carries the token the table was given.
    let [N::Menu(menu)] = first.actions.as_slice() else {
        panic!("not a menu: {:?}", first.actions);
    };
    assert_eq!(menu.label, "Actions for CS-20481");
    let [N::Button(link), N::Button(post)] = menu.items.as_slice() else {
        panic!("not two items: {:?}", menu.items);
    };
    assert_eq!(link.action, Some(Action::navigate("/orders/1")));
    assert_eq!(post.role, n::Role::Destructive);
    assert_eq!(
        post.action,
        Some(Action::Request {
            method: n::Method::Post,
            href: "/orders/1/cancel".into(),
            fields: vec![navree_native::action::Pair {
                name: "_csrf".into(),
                value: "tok-7".into()
            }],
        })
    );
}

#[test]
fn a_table_built_for_a_native_client_and_written_for_a_browser_writes_the_browsers_rows() {
    let plain = full_view("open=1").to_html();
    let mut during = String::new();
    TestNative::build(phone(), || {
        let view = full_view("open=1");
        during = view.to_html();
        view
    });
    assert_eq!(plain, during);
    // Not an attribute for a client, not a row of a tree: the string.
    assert!(!during.contains("data-role"));
    assert!(during.contains(r#"data-slot="cell-select""#));
}

#[test]
fn rows_a_native_client_was_never_given_are_still_a_miss() {
    // A table built for a browser and read on its own has no tree to read.
    let page = TestNative::of(&full_view("open=1"));
    assert_eq!(page.why(), ["slot:data-table"]);
}

#[test]
fn an_action_that_asks_first_carries_its_question_and_a_hook_no_reader_knows_is_a_miss() {
    let page = TestNative::build(phone(), || {
        let query: TableQuery<OrderRow, NoFilter> = TableQuery::parse("/orders", "");
        let page = TablePage::new(vec![order(1, "open")], 1);
        View::new("Orders").body(
            orders_table(&query, &page).bulk(
                "/orders/bulk",
                [navree_ui::table::BulkAction::new("delete", "Delete")
                    .destructive()
                    .confirm("Delete these orders?")],
            ),
        )
    });
    // An action that asks first carries the question, in the words of the page's dialog.
    assert!(page.is_native(), "{:?}", page.page().misses);
    let Some(N::Region(region)) = page.find("orders") else {
        panic!("no table");
    };
    let Some(N::Form(bulk)) = region.children.first() else {
        panic!("no form: {:?}", region.children);
    };
    let delete = bulk
        .children
        .iter()
        .find_map(|node| match node {
            N::Button(button) => Some(button),
            _ => None,
        })
        .expect("the action");
    let ask = delete.ask.as_ref().expect("a question");
    assert_eq!(ask.text.as_deref(), Some("Delete these orders?"));
    assert!(!ask.title.is_empty() && !ask.confirm.is_empty(), "{ask:?}");
    // A question with no dialog to ask it in is still a miss, and so is a hook nothing reads.
    let node: navree_ui::Node = el("button")
        .slot("button")
        .attr("data-ui-confirm", "Sure?")
        .child("Delete")
        .into();
    assert_eq!(
        lower(&node).misses[0].why,
        "a question to ask first, with no dialog to ask it in"
    );
    let node: navree_ui::Node = el("div")
        .slot("data-table")
        .flag("data-ui-wizard", true)
        .into();
    assert_eq!(lower(&node).why(), ["hook:data-ui-wizard"]);
}
