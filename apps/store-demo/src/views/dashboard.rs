//! The dashboard: today's figures, the last 30 days of sales in charts, the order
//! activity as it happens, the shelves running low and today's top sellers. Every
//! part is a region that a server-sent event stream keeps current.

use navree::ui::charts::{Chart, Donut, Grain, NumberFormat, Report, Series};
use navree::ui::dates::DateRange;
use navree::ui::*;

use chrono::{DateTime, Days, FixedOffset, NaiveDate};

use crate::data::{
    Event, Fulfilment, Kind, Level, Line, OFFSET, Order, Product, Status, ZONE, day_of, warehouse,
};
use crate::views::order::product_with;
use crate::views::{money, place_of, thousands, units};

/// Today's figures, counted in the database.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Figures {
    /// What today's orders brought in, less what went back, in cents.
    pub revenue: i64,
    pub orders: i64,
    pub last_hour: i64,
    /// Open orders not packed yet.
    pub to_pack: i64,
    /// Packed orders waiting for the carrier.
    pub to_ship: i64,
    pub low: i64,
    pub out: i64,
}

impl Figures {
    /// The figures of a shop's orders and products at `now`.
    pub fn of(orders: &[Order], products: &[Product], now: i64) -> Self {
        let today = day_of(now);
        let placed: Vec<&Order> = orders
            .iter()
            .filter(|order| order.placed_on == today)
            .collect();
        let open = |step: Fulfilment| {
            orders
                .iter()
                .filter(|order| {
                    order.status == Status::Open.key() && order.fulfilment == step.key()
                })
                .count()
        };
        let level = |level: Level| {
            products
                .iter()
                .filter(|product| product.level == level.key())
                .count()
        };
        let count = |n: usize| i64::try_from(n).unwrap_or(0);
        Self {
            revenue: placed
                .iter()
                .filter(|order| order.status != Status::Cancelled.key())
                .map(|order| order.total - order.refunded)
                .sum(),
            orders: count(placed.len()),
            last_hour: count(
                placed
                    .iter()
                    .filter(|order| order.placed_at >= now - 3600)
                    .count(),
            ),
            to_pack: count(open(Fulfilment::Unfulfilled)),
            to_ship: count(open(Fulfilment::Packed)),
            low: count(level(Level::Low)),
            out: count(level(Level::Out)),
        }
    }
}

/// The four figures across the top.
pub fn figures(figures: &Figures) -> Grid {
    let average = figures.revenue.checked_div(figures.orders).unwrap_or(0);
    Grid::cols(4).gap(3).children((
        Stat::new("Revenue today", money(figures.revenue))
            .trend(format!("{} an order", money(average)), true),
        Stat::new("Orders today", thousands(figures.orders)).trend(
            format!("{} in the last hour", thousands(figures.last_hour)),
            true,
        ),
        Stat::new("To fulfil", thousands(figures.to_pack + figures.to_ship)).trend(
            format!(
                "{} to pack, {} to ship",
                thousands(figures.to_pack),
                thousands(figures.to_ship)
            ),
            // Keeping up while fewer wait to be packed than came in today.
            figures.to_pack <= figures.orders,
        ),
        Stat::new("Low on stock", thousands(figures.low + figures.out)).trend(
            if figures.out == 0 {
                "Nothing has run out".to_owned()
            } else {
                format!("{} out of stock", thousands(figures.out))
            },
            figures.out == 0,
        ),
    ))
}

/// The figures, live: they refetch when the stream says the shop moved.
pub fn live_figures(counts: &Figures) -> Region {
    Region::new("figures")
        .src("/fragments/figures")
        .live_on(crate::live::stream().on("stats"))
        .children(figures(counts))
}

/// How many days the sales charts cover, today the last of them.
pub const SALES_DAYS: u64 = 30;

/// The ways to pay, in the order the charts name them.
pub const METHODS: [&str; 3] = ["upi", "card", "cod"];

/// The days the sales charts cover: the last 30, a day at a time on India's clock,
/// today the last and still counting.
pub fn sales_report(today: NaiveDate) -> Report {
    let first = today
        .checked_sub_days(Days::new(SALES_DAYS - 1))
        .unwrap_or(today);
    // India keeps one offset all year, so a fixed one cuts the same days as the zone.
    match FixedOffset::east_opt(OFFSET) {
        Some(zone) => Report::new(DateRange::new(first, today), Grain::Day, &zone),
        None => Report::new(DateRange::new(first, today), Grain::Day, &chrono::Utc),
    }
}

/// The last 30 days of sales, a value for each day of the report.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sales {
    /// Orders placed each day, the ones cancelled since too.
    pub orders: Vec<u64>,
    /// What each day's orders brought in, in paise: cancelled orders left out and
    /// refunds taken off, as today's revenue figure counts it.
    pub revenue: Vec<i64>,
    /// The orders of all 30 days by how they are paid, in [`METHODS`]' order.
    pub methods: [u64; 3],
}

impl Sales {
    /// Nothing yet, for the days of `report`.
    pub fn empty(report: &Report) -> Self {
        let days = report.buckets().len();
        Self {
            orders: vec![0; days],
            revenue: vec![0; days],
            methods: [0; 3],
        }
    }

    /// Counts one order, when it was placed in the report's days.
    pub fn add(&mut self, report: &Report, order: &Order) {
        let Some(day) =
            DateTime::from_timestamp(order.placed_at, 0).and_then(|at| report.bucket_of(at))
        else {
            return;
        };
        let (Some(orders), Some(revenue)) = (self.orders.get_mut(day), self.revenue.get_mut(day))
        else {
            return;
        };
        *orders += 1;
        if order.status != Status::Cancelled.key() {
            *revenue += order.total - order.refunded;
        }
        if let Some(at) = METHODS.iter().position(|method| *method == order.method) {
            self.methods[at] += 1;
        }
    }

    /// The sales of `orders` over `report`'s days.
    pub fn of<'a>(report: &Report, orders: impl IntoIterator<Item = &'a Order>) -> Self {
        let mut sales = Self::empty(report);
        for order in orders {
            sales.add(report, order);
        }
        sales
    }
}

/// The sales charts: revenue and orders by day, and how customers pay. Each is
/// drawn on the server as SVG, in rupees with Indian digit groups (lakh and crore),
/// with its numbers as a table behind a switch.
pub fn sales_charts(report: &Report, sales: &Sales) -> Grid {
    let rupees = NumberFormat::currency("₹").max_decimals(0);
    let revenue = report
        .chart(Chart::area("Revenue by day"))
        .id("sales-revenue")
        .hide_title()
        .locale("en-IN")
        .height(176)
        .format(rupees)
        .series(Series::new(
            "Revenue",
            sales.revenue.iter().map(|paise| *paise as f64 / 100.0),
        ));
    let orders = report
        .chart(Chart::bar("Orders by day"))
        .id("sales-orders")
        .hide_title()
        .locale("en-IN")
        .height(176)
        .format(NumberFormat::integer())
        .series(Series::new("Orders", sales.orders.iter().copied()));
    let total: u64 = sales.methods.iter().sum();
    let mut methods = Donut::new("How customers paid")
        .id("sales-methods")
        .hide_title()
        .locale("en-IN")
        .size(152)
        .format(NumberFormat::integer())
        .center(
            thousands(i64::try_from(total).unwrap_or(i64::MAX)),
            "orders",
        );
    for (method, count) in METHODS.iter().zip(sales.methods) {
        methods = methods.part(method_words(method), count);
    }
    let days = format!("Last {SALES_DAYS} days, today so far");
    Grid::cols(3).gap(4).children((
        Card::titled("Revenue")
            .description(days.clone())
            .body(revenue),
        Card::titled("Orders").description(days).body(orders),
        Card::titled("How customers pay")
            .description(format!("Orders of the last {SALES_DAYS} days"))
            .body(methods),
    ))
}

/// The sales charts, live: they refetch when the stream says the shop moved, so
/// today's bar grows as orders come in.
pub fn live_sales(report: &Report, sales: &Sales) -> Region {
    Region::new("sales")
        .src("/fragments/sales")
        .live_from("/events")
        .live_event("stats")
        .children(sales_charts(report, sales))
}

/// A row of the feed: an event, and the order it belongs to when it has one.
#[derive(Clone, Debug, Default)]
pub struct FeedEntry {
    pub event: Event,
    pub number: String,
    pub country: String,
    pub state: String,
    pub city: String,
    pub items: i64,
    pub warehouse: String,
    /// How the order is paid: `upi`, `card` or `cod`.
    pub method: String,
}

impl FeedEntry {
    /// An event, with the order it belongs to found among `orders`.
    pub fn of(event: Event, orders: &[Order]) -> Self {
        let order = orders
            .iter()
            .find(|order| order.id == event.order_id && event.order_id > 0);
        Self {
            number: order.map(|o| o.number.clone()).unwrap_or_default(),
            country: order.map(|o| o.country.clone()).unwrap_or_default(),
            state: order.map(|o| o.state.clone()).unwrap_or_default(),
            city: order.map(|o| o.city.clone()).unwrap_or_default(),
            items: order.map_or(0, |o| o.items),
            warehouse: order.map(|o| o.warehouse.clone()).unwrap_or_default(),
            method: order.map(|o| o.method.clone()).unwrap_or_default(),
            event,
        }
    }
}

/// One row: who did what to which order or product, in the tone of what it was,
/// with the facts that matter under it.
pub fn activity_item(entry: &FeedEntry) -> ActivityItem {
    let event = &entry.event;
    let Some(kind) = Kind::parse(&event.kind) else {
        return ActivityItem::new("The shop", "changed").at(event.at);
    };
    let order = format!("/orders/{}", event.order_id);
    let to = || {
        Meta::flag(
            Flag::country(&entry.country).size(FlagSize::Xs),
            place_of(&entry.city, &entry.state, &entry.country),
        )
    };
    let house = warehouse(&entry.warehouse);
    let item = match kind {
        Kind::Placed => ActivityItem::new(event.actor.clone(), "placed")
            .object(entry.number.clone())
            .meta(Meta::icon(
                icons::SHOPPING_BAG,
                format!("{} · {}", units(entry.items), money(event.amount)),
            ))
            .meta(to())
            .href(order),
        Kind::Paid => ActivityItem::new(
            event.actor.clone(),
            if entry.method == "cod" {
                "took the cash for"
            } else {
                "paid for"
            },
        )
        .object(entry.number.clone())
        .meta(Meta::icon(
            method_icon(&entry.method),
            format!("{} · {}", money(event.amount), method_words(&entry.method)),
        ))
        .href(order),
        Kind::Packed => ActivityItem::new(format!("{} warehouse", house.city), "packed")
            .object(entry.number.clone())
            .meta(Meta::icon(icons::PACKAGE, units(event.amount)))
            .href(order),
        Kind::Shipped => ActivityItem::new(format!("{} warehouse", house.city), "shipped")
            .object(entry.number.clone())
            .meta(Meta::icon(icons::TRUCK, event.detail.clone()).ltr())
            .meta(to())
            .href(order),
        Kind::Delivered => ActivityItem::new(event.actor.clone(), "delivered")
            .object(entry.number.clone())
            .meta(to())
            .href(order),
        Kind::Refunded => ActivityItem::new(
            event.actor.clone(),
            format!("refunded {} on", money(event.amount)),
        )
        .object(entry.number.clone())
        .meta(Meta::new(event.detail.clone()))
        .href(order),
        Kind::Cancelled => ActivityItem::new(event.actor.clone(), "cancelled")
            .object(entry.number.clone())
            .meta(Meta::new(event.detail.clone()))
            .href(order),
        Kind::Resent => ActivityItem::new(event.actor.clone(), "sent again the confirmation of")
            .object(entry.number.clone())
            .href(order),
        Kind::LowStock => ActivityItem::new(
            event.actor.clone(),
            if event.amount <= 0 {
                "ran out"
            } else {
                "is running low"
            },
        )
        .tag(format!("{} left", thousands(event.amount)))
        .href("/inventory?filter%5Blevel%5D%5Bin%5D=low%2Cout"),
        Kind::Restocked => ActivityItem::new(
            event.actor.clone(),
            format!("added {} of", units(event.amount)),
        )
        .object(event.detail.clone())
        .href("/inventory"),
    };
    item.tone(kind.tone())
        .icon(kind.icon())
        .at(event.at)
        .id(format!("event-{}", event.id))
}

/// Where the feed has got to: the time and id of its newest row. The feed asks
/// for what came after it, so it only ever fetches what is new.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cursor {
    pub at: i64,
    pub id: i64,
}

impl Cursor {
    pub fn of(event: &Event) -> Self {
        Self {
            at: event.at,
            id: event.id,
        }
    }

    /// `1793793600-1042`.
    pub fn parse(text: &str) -> Option<Self> {
        let (at, id) = text.split_once('-')?;
        Some(Self {
            at: at.parse().ok()?,
            id: id.parse().ok()?,
        })
    }
}

impl std::fmt::Display for Cursor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}-{}", self.at, self.id)
    }
}

/// The live feed: newest first, in a region that a server-sent event stream
/// keeps current. An event only nudges it: the feed asks its own server for the
/// rows that came after the newest it has.
pub fn activity_feed(entries: &[FeedEntry], total: u64) -> Region {
    let newest = entries
        .first()
        .map_or_else(Cursor::default, |entry| Cursor::of(&entry.event));
    ActivityFeed::new("activity", "Order activity")
        .heading(2)
        .live()
        .summary(format!(
            "{} events in the last 24 hours",
            thousands(i64::try_from(total).unwrap_or(0))
        ))
        .time_zone(ZONE, OFFSET)
        .height(FeedHeight::Lg)
        .items(entries.iter().map(activity_item))
        .empty(Empty::new("Quiet so far").description("New orders show up here as they come in."))
        .in_region(
            "activity-region",
            "/fragments/activity",
            crate::live::stream().on("activity"),
            newest.to_string(),
        )
}

/// A product that sold today: how many units, and what they brought in.
#[derive(Clone, Debug)]
pub struct Seller {
    pub product: Product,
    pub units: i64,
    pub revenue: i64,
}

/// The icon of a way to pay.
pub fn method_icon(method: &str) -> icons::Icon {
    match method {
        "upi" => icons::SMARTPHONE,
        "cod" => icons::BANKNOTE,
        _ => icons::CREDIT_CARD,
    }
}

/// `UPI`, `Card` or `Cash on delivery`.
pub fn method_words(method: &str) -> &'static str {
    match method {
        "upi" => "UPI",
        "cod" => "Cash on delivery",
        _ => "Card",
    }
}

/// The shelves running low, emptiest first: five at most.
pub fn low_stock(products: &[Product]) -> Vec<Product> {
    let mut low: Vec<Product> = products
        .iter()
        .filter(|product| product.level != Level::InStock.key())
        .cloned()
        .collect();
    low.sort_by_key(|product| (product.stock, product.id));
    low.truncate(5);
    low
}

/// The five products that sold the most units in `lines`, the lines of today's
/// orders that weren't cancelled.
pub fn top_sellers(products: &[Product], lines: &[Line]) -> Vec<Seller> {
    let mut sellers: Vec<Seller> = products
        .iter()
        .filter_map(|product| {
            let (units, revenue) = lines
                .iter()
                .filter(|line| line.product_id == product.id)
                .fold((0, 0), |(units, revenue), line| {
                    (units + line.quantity, revenue + line.quantity * line.price)
                });
            (units > 0).then(|| Seller {
                product: product.clone(),
                units,
                revenue,
            })
        })
        .collect();
    sellers.sort_by(|a, b| b.units.cmp(&a.units).then(b.revenue.cmp(&a.revenue)));
    sellers.truncate(5);
    sellers
}

/// The whole dashboard under its title: the date, the figures, the sales charts,
/// and the feed beside the shelves.
#[allow(clippy::too_many_arguments)]
pub fn dashboard_body(
    counts: &Figures,
    report: &Report,
    sales: &Sales,
    entries: &[FeedEntry],
    total: u64,
    low: &[Product],
    sellers: &[Seller],
    today: NaiveDate,
) -> Stack {
    Stack::gap(4).children((
        Text::muted(format!(
            "{}. Times are in India Standard Time.",
            today.format("%A %-d %B")
        )),
        live_figures(counts),
        live_sales(report, sales),
        Grid::cols(3).gap(4).children((
            el("div")
                .attr("data-span", "2")
                .child(activity_feed(entries, total)),
            live_shelves(low, sellers),
        )),
    ))
}

/// The shelves running low and today's top sellers, side by side under the feed
/// on a phone.
pub fn shelves(low: &[Product], sellers: &[Seller]) -> Stack {
    // Two lines a product, so a narrow column keeps every word whole.
    let low_list: Vec<Node> = low
        .iter()
        .map(|item| {
            product_with(
                &item.icon,
                &item.name,
                Row::gap(2).align(Align::Center).wrap().children((
                    level(&item.level),
                    Text::muted(format!("{} left", thousands(item.stock))).inline(),
                )),
            )
            .into()
        })
        .collect();
    let seller_list: Vec<Node> = sellers
        .iter()
        .map(|seller| {
            product_with(
                &seller.product.icon,
                &seller.product.name,
                Text::muted(format!(
                    "{} · {}",
                    units(seller.units),
                    money(seller.revenue)
                )),
            )
            .into()
        })
        .collect();
    Stack::gap(4).children((
        Card::titled("Running low")
            .description("At or under the point each reorders at")
            .action(
                Button::outline("Inventory")
                    .size(Size::Sm)
                    .href("/inventory?filter%5Blevel%5D%5Bin%5D=low%2Cout"),
            )
            .body(if low_list.is_empty() {
                Node::from(Text::muted("Every shelf is stocked."))
            } else {
                Stack::gap(3).children(low_list).into()
            }),
        Card::titled("Top sellers today")
            .description("By units sold, cancelled orders left out")
            .body(if seller_list.is_empty() {
                Node::from(Text::muted("Nothing sold yet today."))
            } else {
                Stack::gap(3).children(seller_list).into()
            }),
    ))
}

/// The shelves, live.
pub fn live_shelves(low: &[Product], sellers: &[Seller]) -> Region {
    Region::new("shelves")
        .src("/fragments/shelves")
        .live_on(crate::live::stream().on("stats"))
        .children(shelves(low, sellers))
}

fn level(key: &str) -> Badge {
    let level = Level::parse(key);
    Badge::new(level.label()).tone(level.tone()).dot()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data;

    /// 4 November 2026, 12:00 in India.
    const NOON: i64 = 1_793_773_800;

    #[test]
    fn the_charts_cover_the_last_30_days_on_indias_clock() {
        let report = sales_report(data::day_of(NOON));
        let days = report.buckets();
        assert_eq!(days.len(), 30);
        let first = days.first().map(|day| day.local.to_string());
        let last = days.last().map(|day| day.local.to_string());
        assert_eq!(first.as_deref(), Some("2026-10-06 00:00:00"));
        assert_eq!(last.as_deref(), Some("2026-11-04 00:00:00"));
        // Midnight in India is 18:30 the day before in UTC.
        assert_eq!(days[0].start.to_rfc3339(), "2026-10-05T18:30:00+00:00");
        assert_eq!(days[29].end.to_rfc3339(), "2026-11-04T18:30:00+00:00");
    }

    #[test]
    fn todays_bar_and_point_are_todays_figures() {
        let products = data::products();
        let customers = data::customers(NOON);
        let (orders, _, _) = data::orders(
            data::ORDERS - 3_000 + 1,
            data::ORDERS,
            NOON,
            &products,
            &customers,
        );
        let report = sales_report(data::day_of(NOON));
        let sales = Sales::of(&report, &orders);
        let figures = Figures::of(&orders, &products, NOON);
        assert!(figures.orders > 0);
        assert_eq!(
            sales.orders.last().copied(),
            u64::try_from(figures.orders).ok()
        );
        assert_eq!(sales.revenue.last().copied(), Some(figures.revenue));
        // Each order of the window is paid one way.
        assert_eq!(
            sales.methods.iter().sum::<u64>(),
            sales.orders.iter().sum::<u64>()
        );
        // An order outside the window counts nowhere.
        let mut old = orders[0].clone();
        old.placed_at = NOON - 40 * 86_400;
        let mut again = sales.clone();
        again.add(&report, &old);
        assert_eq!(again, sales);
    }

    #[test]
    fn the_charts_write_rupees_in_lakh_and_crore() {
        let report = sales_report(data::day_of(NOON));
        let mut sales = Sales::empty(&report);
        sales.revenue[29] = 213_456_789;
        sales.revenue[28] = 1_250_000_000;
        sales.orders[29] = 1_204;
        sales.methods = [65_000, 35_000, 20_000];
        let html = render(&sales_charts(&report, &sales).into());
        assert!(html.contains("₹21,34,568"), "a day's revenue in full");
        assert!(
            html.contains("₹1,25,00,000"),
            "a crore and a quarter in full"
        );
        assert!(html.contains("₹1Cr"), "the axis in crore: {html}");
        assert!(html.contains("₹50L"), "and in lakh");
        assert!(html.contains("1,204"), "a day's orders");
        assert!(
            html.contains("1,20,000"),
            "the orders in the middle of the ring"
        );
        for name in ["UPI", "Card", "Cash on delivery"] {
            assert!(html.contains(name), "{name}");
        }
        assert!(!html.contains("style="), "no style attribute");
    }
}
