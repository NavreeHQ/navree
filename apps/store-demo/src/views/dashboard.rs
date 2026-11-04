//! The dashboard: today's figures, the order activity as it happens, the shelves
//! running low and today's top sellers. Every part is a region that a
//! server-sent event stream keeps current.

use navree::ui::*;

use chrono::NaiveDate;

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

/// Where the sales charts go. The charts module draws them on the server; until
/// it is in the toolkit this slot draws nothing, and the page closes up around it.
pub fn charts(_figures: &Figures) -> Option<Node> {
    None
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
        .href("/inventory?f.level=low&f.level=out"),
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

/// The whole dashboard under its title: the date, the figures, the charts when
/// there are any, and the feed beside the shelves.
pub fn dashboard_body(
    counts: &Figures,
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
        charts(counts),
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
                    .href("/inventory?f.level=low&f.level=out"),
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
