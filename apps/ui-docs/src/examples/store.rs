//! The store app's own views, drawn from its seed: what `/ui/` shows under
//! "Build anything" and the Showcase shows on the store's page. The code beside
//! each preview is the app's source, read from its files, so the website shows
//! what the app runs.

use navree::ui::*;
use store_demo::data::{self, Customer, Event, Line, Order, Product, Status};
use store_demo::views::dashboard::{
    FeedEntry, Figures, activity_item, dashboard_body, low_stock, top_sellers,
};
use store_demo::views::inventory::{ProductRow, inventory_table};
use store_demo::views::order::{OrderView, Parts, order_drawer};
use store_demo::views::orders::{OrderRow, empty_drawer, orders_table};
use store_demo::views::shell;

use crate::catalog::{self, Example, items};

/// The moment the previews are drawn at, fixed so the files don't change:
/// 4 November 2026, 12:00 in India.
const NOW: i64 = 1_793_773_800;

/// How many of the newest orders the previews draw from: the last few days,
/// which hold every order still on its way.
const RECENT: i64 = 3_000;

const ORDERS_RS: &str = include_str!("../../../store-demo/src/views/orders.rs");
const ORDER_RS: &str = include_str!("../../../store-demo/src/views/order.rs");
const DASHBOARD_RS: &str = include_str!("../../../store-demo/src/views/dashboard.rs");
const INVENTORY_RS: &str = include_str!("../../../store-demo/src/views/inventory.rs");

pub fn all() -> Vec<Example> {
    vec![
        Example::new(
            "build-anything",
            "store-drawer",
            "An order in a drawer over the orders",
            catalog::Kind::Single(drawer_page),
            items(
                ORDER_RS,
                &["pub fn order_drawer", "pub fn order_detail", "fn facts"],
            ),
        )
        .fill()
        .width(1440)
        .height(860)
        .open()
        .streaming("on"),
        Example::new(
            "build-anything",
            "store-orders",
            "The orders, on the server",
            catalog::Kind::Single(orders_page),
            items(ORDERS_RS, &["pub struct OrderRow", "pub fn orders_table"]),
        )
        .fill()
        .width(1440)
        .height(860)
        .streaming("on"),
        Example::new(
            "build-anything",
            "store-dashboard",
            "The dashboard, live",
            catalog::Kind::Single(dashboard_page),
            items(
                DASHBOARD_RS,
                &["pub fn activity_feed", "pub fn activity_item"],
            ),
        )
        .fill()
        .width(1440)
        .height(900)
        .streaming("on"),
        Example::new(
            "build-anything",
            "store-inventory",
            "Inventory with a restock in each row",
            catalog::Kind::Single(inventory_page),
            items(
                INVENTORY_RS,
                &[
                    "pub struct ProductRow",
                    "fn restock_cell",
                    "pub fn inventory_table",
                ],
            ),
        )
        .fill()
        .width(1440)
        .height(860),
    ]
}

/// The shop as the previews see it: every product and customer, and the newest
/// orders with their lines and events.
struct Shop {
    products: Vec<Product>,
    customers: Vec<Customer>,
    orders: Vec<Order>,
    lines: Vec<Line>,
    events: Vec<Event>,
}

fn shop() -> Shop {
    let products = data::products();
    let customers = data::customers(NOW);
    let (orders, lines, mut events) = data::orders(
        data::ORDERS - RECENT + 1,
        data::ORDERS,
        NOW,
        &products,
        &customers,
    );
    events.extend(data::stock_events(&products, NOW));
    for (at, event) in events.iter_mut().enumerate() {
        event.id = i64::try_from(at).unwrap_or(0) + 1;
    }
    Shop {
        products,
        customers,
        orders,
        lines,
        events,
    }
}

fn today() -> chrono::NaiveDate {
    data::day_of(NOW)
}

/// The newest orders as the table's first page draws them.
fn first_page(shop: &Shop) -> (TableQuery<OrderRow>, TablePage<OrderRow>) {
    let mut rows: Vec<OrderRow> = shop.orders.iter().cloned().map(OrderRow::from).collect();
    rows.sort_by_key(|row| std::cmp::Reverse((row.placed_on, row.id)));
    rows.truncate(10);
    let query = TableQuery::<OrderRow>::parse("/orders", "per=10");
    let page = TablePage::new(rows, u64::try_from(data::ORDERS).unwrap_or(0));
    (query, page)
}

/// How many orders hold each status: in the whole shop, the old orders have
/// all arrived, so they count as completed but for the cancelled ones.
fn status_counts(shop: &Shop) -> Vec<(Status, u64)> {
    let count = |status: Status| {
        shop.orders
            .iter()
            .filter(|order| order.status == status.key())
            .count() as u64
    };
    let older = u64::try_from(data::ORDERS - RECENT).unwrap_or(0);
    let cancelled = count(Status::Cancelled) * u64::try_from(data::ORDERS).unwrap_or(0)
        / u64::try_from(RECENT).unwrap_or(1);
    vec![
        (Status::Open, count(Status::Open)),
        (
            Status::Completed,
            count(Status::Completed) + older - (cancelled - count(Status::Cancelled)),
        ),
        (Status::Cancelled, cancelled),
    ]
}

/// An order with a story to show: several lines, shipped from Ludhiana to another
/// state, with a note from the customer and a few orders before it.
fn chosen(shop: &Shop) -> &Order {
    let lines = |order: &Order| {
        shop.lines
            .iter()
            .filter(|line| line.order_id == order.id)
            .count()
    };
    let story = |order: &&Order| {
        order.fulfilment == "shipped"
            && order.method != "cod"
            && order.warehouse == "LDH"
            && order.state != "PB"
            && order.country == "IN"
            && !order.note.is_empty()
            && lines(order) >= 3
    };
    shop.orders
        .iter()
        .rev()
        .find(story)
        .or_else(|| {
            shop.orders
                .iter()
                .rev()
                .find(|order| order.fulfilment == "shipped")
        })
        .or_else(|| shop.orders.last())
        .unwrap_or_else(|| panic!("the seed has orders"))
}

fn drawer_page() -> Node {
    let shop = shop();
    let order = chosen(&shop).clone();
    let lines: Vec<Line> = shop
        .lines
        .iter()
        .filter(|line| line.order_id == order.id)
        .cloned()
        .collect();
    let history: Vec<Event> = shop
        .events
        .iter()
        .filter(|event| event.order_id == order.id)
        .cloned()
        .collect();
    // The customer's orders across the whole year, found without rolling them all.
    let theirs: Vec<i64> = (1..=data::ORDERS)
        .filter(|id| data::customer_of(*id) == order.customer_id)
        .collect();
    let mut others: Vec<Order> = theirs
        .iter()
        .filter(|id| **id != order.id)
        .rev()
        .take(5)
        .filter_map(|id| {
            data::orders(*id, *id, NOW, &shop.products, &shop.customers)
                .0
                .into_iter()
                .next()
        })
        .collect();
    others.sort_by_key(|other| std::cmp::Reverse(other.placed_at));
    let customer = shop
        .customers
        .iter()
        .find(|customer| customer.id == order.customer_id)
        .cloned()
        .unwrap_or_default();
    let view = OrderView {
        order,
        lines,
        orders: theirs.len() as u64,
        since: customer.since,
        phone: customer.phone,
        now: NOW,
    };
    let (query, page) = first_page(&shop);
    let counts = status_counts(&shop);
    let drawer = order_drawer(
        &view,
        today(),
        Parts::Inline {
            history: &history,
            others: &others,
        },
    );
    shell()
        .page("Orders", "/orders")
        .crumb("Dashboard", "/")
        .body((
            orders_table(&query, &page, &counts, today()),
            el("div").flag("data-preview-open", true).child(drawer),
        ))
        .to_view()
        .body_node()
        .clone()
}

fn orders_page() -> Node {
    let shop = shop();
    let (query, page) = first_page(&shop);
    let counts = status_counts(&shop);
    shell()
        .page("Orders", "/orders")
        .crumb("Dashboard", "/")
        .body((
            orders_table(&query, &page, &counts, today()),
            empty_drawer(),
        ))
        .to_view()
        .body_node()
        .clone()
}

/// The dashboard as it first draws, and after it the rows a stream would bring,
/// which the site's own script adds one by one: the preview has no server to
/// stream from.
fn dashboard_page() -> Node {
    let shop = shop();
    let counts = Figures::of(&shop.orders, &shop.products, NOW);
    let mut recent: Vec<Event> = shop
        .events
        .iter()
        .filter(|event| event.at >= NOW - 24 * 3600)
        .cloned()
        .collect();
    recent.sort_by_key(|event| std::cmp::Reverse((event.at, event.id)));
    let total = recent.len() as u64;
    // The six newest are held back, to arrive one by one as the stream would.
    let held: Vec<i64> = recent.iter().take(6).map(|event| event.id).collect();
    let entry = |event: &Event| FeedEntry::of(event.clone(), &shop.orders);
    let shown: Vec<FeedEntry> = recent
        .iter()
        .filter(|event| !held.contains(&event.id))
        .take(30)
        .map(entry)
        .collect();
    let mut later: Vec<FeedEntry> = recent
        .iter()
        .filter(|event| held.contains(&event.id))
        .map(entry)
        .collect();
    later.reverse();
    let today_ids: Vec<i64> = shop
        .orders
        .iter()
        .filter(|order| order.placed_on == today() && order.status != Status::Cancelled.key())
        .map(|order| order.id)
        .collect();
    let lines: Vec<Line> = shop
        .lines
        .iter()
        .filter(|line| today_ids.contains(&line.order_id))
        .cloned()
        .collect();
    let page = shell()
        .page("Dashboard", "/")
        .actions(
            Button::outline("Orders")
                .icon(icons::SHOPPING_BAG)
                .href("/orders"),
        )
        .body(dashboard_body(
            &counts,
            &shown,
            total,
            &low_stock(&shop.products),
            &top_sellers(&shop.products, &lines),
            today(),
        ))
        .to_view()
        .body_node()
        .clone();
    Node::list((
        page,
        el("template")
            .attr("data-preview-replay", "activity-list")
            .child(ActivityFeed::rows(
                later.iter().map(activity_item),
                data::OFFSET,
            )),
    ))
}

fn inventory_page() -> Node {
    let shop = shop();
    let mut rows: Vec<ProductRow> = shop
        .products
        .iter()
        .cloned()
        .map(ProductRow::from)
        .collect();
    rows.sort_by_key(|row| (row.stock, row.id));
    rows.truncate(10);
    let query = TableQuery::<ProductRow>::parse("/inventory", "per=10");
    let page = TablePage::new(rows, shop.products.len() as u64);
    shell()
        .page("Inventory", "/inventory")
        .crumb("Dashboard", "/")
        .body(inventory_table(&query, &page))
        .to_view()
        .body_node()
        .clone()
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_code_shown_is_the_apps_own() {
        let code = super::items(super::ORDERS_RS, &["pub struct OrderRow"]);
        assert!(
            code.starts_with("/// An order, as the table shows it."),
            "{code}"
        );
        assert!(
            code.contains(
                "#[table(
    cursor,"
            ),
            "{code}"
        );
        assert!(code.ends_with('}'));
        let code = super::items(super::ORDER_RS, &["pub fn order_drawer"]);
        assert!(code.contains("SheetWidth::Xl"), "{code}");
    }

    #[test]
    fn the_drawer_is_drawn_open_over_its_page() {
        let html = navree::ui::render(&super::drawer_page());
        assert!(html.contains(r#"data-slot="data-table""#) || html.contains("id=\"orders\""));
        assert!(
            html.contains(r#"<div data-preview-open><dialog"#),
            "the drawer is marked open"
        );
        assert!(html.contains(r#"data-width="xl""#));
    }
}
