//! Native clients against a real app: the same routes answering a browser
//! with the HTML they always did and a native client with the envelope, and
//! the readers of the components that are built with derives.

use std::sync::Arc;

use http::{Method, StatusCode};
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
use navree_ui::native::{TestNative, lower};
use navree_ui::table::NoFilter;
use navree_ui::{
    AppPage, AppShell, Badge, Button, DataTable, Dialog, Empty, Form, NavGroup, NavItem, Op,
    Options, Reply, Sheet, Shell, TablePage, TableQuery, TableRow, Text, ToastTone, UiModule,
    UserMenu, Validate, View, assets, el, icons, render,
};
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
    let status = fields
        .iter()
        .find(|field| field.name == "filter[status][in]")
        .expect("a status filter");
    assert_eq!(status.label.as_deref(), Some("Status"));
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
        fields
            .iter()
            .any(|field| field.name == "per_page" && matches!(field.input, Input::Choice(_))),
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
    assert_eq!(envelope.build, assets::BUILD);
    assert_eq!(envelope.vocab, 1);
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
            "Mozilla/5.0 (Linux; Android 16; wv) Chrome/140 Mobile Safari/537.36 NavreeNative/0.1.0",
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
async fn a_client_too_old_for_the_page_is_sent_to_the_web_view() {
    let app = app().await;
    let response = app
        .client()
        .get("/orders/1")
        .header(NATIVE, "1")
        .header(CLIENT, "platform=ios; form=phone; vocab=0")
        .send()
        .await;
    let envelope = envelope(&response.text());
    assert_eq!(envelope.vocab, 0);
    assert!(envelope.screen.is_none());
    assert!(envelope.web.expect("the web view").why[0].starts_with("vocab:"));
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
    for path in ["/orders/1", "/orders", "/fragments/figures", "/fragments/feed"] {
        let browser = client.get(path).send().await;
        assert!(
            browser
                .header("content-type")
                .is_some_and(|kind| kind.starts_with("text/html")),
            "{path}"
        );
        let vary: Vec<String> = browser.headers().get_all("vary").iter().map(|v| v.to_str().unwrap().to_owned()).collect();
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
    let vary: Vec<String> = runtime.headers().get_all("vary").iter().map(|v| v.to_str().unwrap().to_owned()).collect();
    assert!(vary.iter().any(|v| varies(Some(v))), "{vary:?}");
    // The web view's page is neither.
    let web_view = client
        .get("/orders/1")
        .header("user-agent", "Mozilla/5.0 (iPhone) AppleWebKit/605.1.15 NavreeNative/0.1.0")
        .send()
        .await;
    let vary = web_view.header("vary").unwrap_or_default().to_owned();
    assert!(vary.contains("x-navree-native") && vary.contains("user-agent"), "{vary}");
    // An error page varies on it too.
    let missing = client.get("/nowhere").header("accept", "text/html").send().await;
    let vary: Vec<String> = missing.headers().get_all("vary").iter().map(|v| v.to_str().unwrap().to_owned()).collect();
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
