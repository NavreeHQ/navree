//! The shop's screens for native clients, written once with Navree Native UI:
//! the day (`/`), an order (`/orders/{id}`) and the shelves that need a hand
//! (`/shelves`). SwiftUI draws them on Apple systems and Jetpack Compose on
//! Android.
//!
//! Each is a plain function of the same data its page is drawn from, and
//! knows nothing of the request: the handlers in [`pages`](crate::pages)
//! answer a browser with the page and a native client with the screen. What
//! a screen does goes to the handlers the pages use.

use chrono::NaiveDate;
use navree::native::op::Edge;
use navree::native::ui::*;

use crate::data::{
    Event, Fulfilment, Kind, Level, Line, Order, Payment, Product, Status, phone_text, state_name,
    warehouse,
};
use crate::shop;
use crate::views::dashboard::{Cursor, FeedEntry, Figures, Sales, Seller, method_words};
use crate::views::inventory;
use crate::views::order::{OrderView, PAGE, back_to, paid_with};
use crate::views::{clock, money, place_of, thousands, units, when};

/// The name of a product's icon in the toolkit's set, by its key.
fn product_symbol(key: &str) -> Icon {
    Icon::named(match key {
        "dupatta" => "ribbon",
        "jutti" => "footprints",
        "lota" => "milk",
        "tiffin" => "soup",
        "kadhai" => "cooking-pot",
        "ghee" => "droplet",
        "rice" => "wheat",
        "shawl" => "layers",
        "kurta" | "salwar" => "shirt",
        "dabba" => "salad",
        "bottle" => "flask-round",
        "dhurrie" => "layout-grid",
        "diya" => "flame",
        "kulhad" => "coffee",
        "bedsheet" => "bed",
        "stand" => "lamp",
        "belan" => "utensils-crossed",
        "oil" => "droplets",
        "jaggery" => "candy",
        "pinni" => "gift",
        "papad" => "cookie",
        "bowl" => "palette",
        "thali" => "utensils",
        _ => "package",
    })
}

/// The icon of a kind of event, with what its colour means.
fn kind_symbol(kind: Kind) -> (Icon, Tone) {
    let (name, tone) = match kind {
        Kind::Placed => ("shopping-bag", Tone::Info),
        Kind::Paid => ("credit-card", Tone::Success),
        Kind::Packed => ("package", Tone::Info),
        Kind::Shipped => ("truck", Tone::Accent),
        Kind::Delivered => ("package-check", Tone::Success),
        Kind::Refunded => ("undo-2", Tone::Warning),
        Kind::Cancelled => ("ban", Tone::Neutral),
        Kind::Resent => ("send", Tone::Neutral),
        Kind::LowStock => ("triangle-alert", Tone::Warning),
        Kind::Restocked => ("package-plus", Tone::Success),
    };
    (Icon::named(name), tone)
}

/// A heading inside a card.
fn heading(words: &str) -> Text {
    Text::new(words).style(TextStyle::Title2).weight(Weight::Bold).heading(2)
}

/// A small label over a value.
fn label(words: &str) -> Text {
    Text::new(words).style(TextStyle::Caption).weight(Weight::Semibold).muted()
}

// ---------------------------------------------------------------------------
// The day
// ---------------------------------------------------------------------------

/// What the day's screen is drawn from: what the dashboard is drawn from.
pub struct Today<'a> {
    /// `Friday 13 November`.
    pub date: String,
    pub counts: &'a Figures,
    pub sales: &'a Sales,
    /// A label for each day of the sales: `6 Oct`.
    pub days: Vec<String>,
    pub entries: &'a [FeedEntry],
    /// How many events the last 24 hours hold.
    pub events: u64,
    /// The stream that says when the shop moved.
    pub stream: String,
    pub today: NaiveDate,
}

/// The day at a glance: what it brought in, its figures, what needs a hand,
/// the last 30 days in charts, and what happened last.
pub fn today(day: &Today<'_>) -> Screen {
    Screen::titled("Today")
        .subtitle(format!("{} · India Standard Time", day.date))
        .refresh()
        .action(
            Button::new("Orders")
                .icon(Icon::named("shopping-bag"))
                .role(Role::Secondary)
                .to("/orders"),
        )
        .body(Stack::down().id("today").gap(4).parts((
            // Each part that the shop's stream keeps current fetches itself again.
            Region::new("figures", "/fragments/figures")
                .live(&day.stream, "stats")
                .parts(figures(day.counts, &day.sales.revenue)),
            attention(day.counts),
            Region::new("sales", "/fragments/sales")
                .live(&day.stream, "stats")
                .parts(charts(&day.days, day.sales)),
            activity(day),
        )))
}

/// The day's revenue, set large beside the shape of its last 30 days, over
/// the day's other figures: as many beside each other as fit.
pub fn figures(counts: &Figures, revenue: &[i64]) -> Stack {
    let average = counts.revenue.checked_div(counts.orders).unwrap_or(0);
    let waiting = counts.to_pack + counts.to_ship;
    let short = counts.low + counts.out;
    let hero = Card::new().id("revenue").parts(
        Stack::down().gap(3).align(Align::Center).wide(Axis::Horizontal).parts((
            Stack::down().gap(1).grow().parts((
                label("Revenue today"),
                Text::new(money(counts.revenue))
                    .id("revenue-today")
                    .style(TextStyle::Display)
                    .change(Change::Count),
                Text::new(format!("{} an order", money(average)))
                    .style(TextStyle::Callout)
                    .muted(),
            )),
            Chart::sparkline(
                "Revenue by day",
                revenue.iter().map(|paise| *paise as f64 / 100.0),
            )
            .id("revenue-days")
            .summary("Revenue of each of the last 30 days, today the last."),
        )),
    );
    let tiles = Grid::fit(150).gap(3).id("day-figures").parts((
        Figure::new("Orders today", thousands(counts.orders))
            .id("figure-orders")
            .icon(Icon::named("shopping-bag"), Hue::Blue)
            .trend(
                if counts.last_hour > 0 { Direction::Up } else { Direction::Flat },
                format!("{} in the last hour", thousands(counts.last_hour)),
                true,
            )
            .to("/orders"),
        Figure::new("To fulfil", thousands(waiting))
            .id("figure-fulfil")
            .icon(Icon::named("package"), Hue::Amber)
            .detail(format!(
                "{} to pack, {} to ship",
                thousands(counts.to_pack),
                thousands(counts.to_ship)
            ))
            .status(if counts.to_pack <= counts.orders {
                Badge::new("Keeping up").tone(Tone::Success)
            } else {
                Badge::new("Falling behind").tone(Tone::Warning)
            })
            .to("/orders?filter[status][in]=open"),
        Figure::new("Low on stock", thousands(short))
            .id("figure-stock")
            .icon(Icon::named("boxes"), Hue::Rose)
            .detail(if counts.out == 0 {
                "Nothing has run out".to_owned()
            } else {
                format!("{} out of stock", thousands(counts.out))
            })
            .status(if counts.out == 0 {
                Badge::new("Stocked").tone(Tone::Success)
            } else {
                Badge::new("Shelves empty").tone(Tone::Danger)
            })
            .to("/shelves"),
    ));
    Stack::down().id("figures-parts").gap(4).parts((hero, tiles))
}

/// What needs a hand now, each line leading to where it is done. Rows the
/// system draws as its own list: nothing here is composed.
fn attention(counts: &Figures) -> Section {
    let line = |count: i64, words: &str, icon: &str, hue: Hue, href: &str| {
        (count > 0).then(|| {
            Row::new(format!("{} {words}", thousands(count)))
                .leading(chip(Icon::named(icon), hue))
                .to(href)
        })
    };
    let lines = [
        line(
            counts.to_pack,
            "to pack",
            "package",
            Hue::Amber,
            "/orders?filter[status][in]=open&filter[fulfilment][in]=unfulfilled",
        ),
        line(
            counts.to_ship,
            "to hand to the carrier",
            "truck",
            Hue::Violet,
            "/orders?filter[status][in]=open&filter[fulfilment][in]=packed",
        ),
        line(counts.out, "out of stock", "triangle-alert", Hue::Danger, "/shelves"),
        line(counts.low, "running low", "boxes", Hue::Warning, "/shelves"),
    ];
    let section = Section::titled("Needs attention").id("attention").inset();
    if lines.iter().all(Option::is_none) {
        section.parts(Text::new("Nothing needs you right now.").muted())
    } else {
        section.parts(List::new().id("attention-list").parts(lines))
    }
}

/// The last 30 days as three charts in a run that scrolls across: one at a
/// time on a phone, all three where they fit.
pub fn charts(days: &[String], sales: &Sales) -> Carousel {
    let card = |title: &str, about: String, chart: Chart| {
        Card::new().parts((
            heading(title),
            Text::new(about).style(TextStyle::Caption).muted(),
            chart,
        ))
    };
    let window = format!("Last {} days, today so far", days.len());
    let orders: u64 = sales.methods.iter().sum();
    Carousel::new()
        .id("sales-charts")
        .min(300)
        .gap(3)
        .label("Sales of the last 30 days")
        .parts((
            card(
                "Revenue",
                window.clone(),
                Chart::new(ChartKind::Area, "Revenue by day")
                    .id("sales-revenue")
                    .labels(days)
                    .series("Revenue", sales.revenue.iter().map(|paise| *paise as f64 / 100.0))
                    .shown(sales.revenue.iter().map(|paise| money(*paise)))
                    .colour(Hue::Accent)
                    .summary("What each day's orders brought in, refunds taken off."),
            ),
            card(
                "Orders",
                window,
                Chart::new(ChartKind::Bar, "Orders by day")
                    .id("sales-orders")
                    .labels(days)
                    .series("Orders", sales.orders.iter().copied())
                    .colour(Hue::Blue)
                    .summary("Orders placed each day."),
            ),
            card(
                "How customers pay",
                format!("{} orders of the last {} days", thousands(i64::try_from(orders).unwrap_or(0)), days.len()),
                Chart::new(ChartKind::Donut, "How customers paid")
                    .id("sales-methods")
                    .labels(crate::views::dashboard::METHODS.iter().map(|method| method_words(method)))
                    .series("Orders", sales.methods)
                    .summary("Orders by how they are paid: UPI, card or cash on delivery."),
            ),
        ))
}

/// What happened last, newest first: a list the shop's stream keeps current.
/// It asks only for what came after the newest row it holds.
fn activity(day: &Today<'_>) -> Section {
    let newest = day
        .entries
        .first()
        .map_or_else(Cursor::default, |entry| Cursor::of(&entry.event));
    Section::titled("Latest activity")
        .id("activity-section")
        .subtitle(format!(
            "{} events in the last 24 hours",
            thousands(i64::try_from(day.events).unwrap_or(0))
        ))
        .parts(
            List::new()
                .id(ACTIVITY)
                .live(newest.to_string(), "/fragments/activity", &day.stream, "activity")
                .parts(day.entries.iter().map(|entry| activity_row(entry, day.today))),
        )
}

/// The id of the day's list of events: what the rows that came since are
/// merged into.
pub const ACTIVITY: &str = "activity";

/// One event: who did what to which order or product, and when.
pub fn activity_row(entry: &FeedEntry, today: NaiveDate) -> FeedRow {
    let event = &entry.event;
    let Some(kind) = Kind::parse(&event.kind) else {
        return FeedRow::new("The shop", "changed", "").id(format!("event-{}", event.id));
    };
    let house = warehouse(&entry.warehouse);
    let order = format!("/orders/{}", event.order_id);
    let to = place_of(&entry.city, &entry.state, &entry.country);
    let (who, what, which, about, href) = match kind {
        Kind::Placed => (
            event.actor.clone(),
            "placed".to_owned(),
            entry.number.clone(),
            format!("{} · {} · {to}", units(entry.items), money(event.amount)),
            order,
        ),
        Kind::Paid => (
            event.actor.clone(),
            if entry.method == "cod" { "took the cash for" } else { "paid for" }.to_owned(),
            entry.number.clone(),
            format!("{} · {}", money(event.amount), method_words(&entry.method)),
            order,
        ),
        Kind::Packed => (
            format!("{} warehouse", house.city),
            "packed".to_owned(),
            entry.number.clone(),
            units(event.amount),
            order,
        ),
        Kind::Shipped => (
            format!("{} warehouse", house.city),
            "shipped".to_owned(),
            entry.number.clone(),
            format!("{} · {to}", event.detail),
            order,
        ),
        Kind::Delivered => (event.actor.clone(), "delivered".to_owned(), entry.number.clone(), to, order),
        Kind::Refunded => (
            event.actor.clone(),
            format!("refunded {} on", money(event.amount)),
            entry.number.clone(),
            event.detail.clone(),
            order,
        ),
        Kind::Cancelled => (
            event.actor.clone(),
            "cancelled".to_owned(),
            entry.number.clone(),
            event.detail.clone(),
            order,
        ),
        Kind::Resent => (
            event.actor.clone(),
            "sent again the confirmation of".to_owned(),
            entry.number.clone(),
            String::new(),
            order,
        ),
        Kind::LowStock => (
            event.actor.clone(),
            if event.amount <= 0 { "ran out" } else { "is running low" }.to_owned(),
            String::new(),
            format!("{} left", thousands(event.amount)),
            "/shelves".to_owned(),
        ),
        Kind::Restocked => (
            event.actor.clone(),
            format!("added {} of", units(event.amount)),
            event.detail.clone(),
            String::new(),
            "/shelves".to_owned(),
        ),
    };
    let (icon, tone) = kind_symbol(kind);
    FeedRow::new(who, what, which)
        .id(format!("event-{}", event.id))
        .icon(icon, tone)
        .time(when(event.at, today))
        .detail((!about.is_empty()).then(|| Text::new(about).style(TextStyle::Caption).muted()))
        .to(href)
}

/// The rows that came since a client's newest, as the change to its list: the
/// events to merge in at the top, and the cursor the list is at now.
pub fn activity_since(entries: &[FeedEntry], asked: &str, today: NaiveDate) -> Vec<Op> {
    let since = entries
        .first()
        .map_or_else(|| asked.to_owned(), |entry| Cursor::of(&entry.event).to_string());
    vec![Op::Merge {
        target: ACTIVITY.to_owned(),
        nodes: entries.iter().map(|entry| activity_row(entry, today).into()).collect(),
        gone: Vec::new(),
        since,
        at: Edge::Start,
        more: false,
    }]
}

// ---------------------------------------------------------------------------
// An order
// ---------------------------------------------------------------------------

/// What an order's screen is drawn from: what its page is drawn from.
pub struct OrderScreen<'a> {
    pub view: &'a OrderView,
    /// What happened to it, oldest first.
    pub history: &'a [Event],
    /// The customer's latest other orders.
    pub others: &'a [Order],
    pub today: NaiveDate,
}

/// An order as a record: its state, what was bought, who bought it and where
/// it goes, how far it has got, and what staff may do to it, each by its role.
pub fn order(shown: &OrderScreen<'_>) -> Screen {
    let order = &shown.view.order;
    let mut screen = Screen::titled(format!("Order {}", order.number))
        .back("Orders", "/orders")
        .nav("/orders")
        .refresh()
        .action(order_actions(shown.view))
        .body(order_body(shown));
    screen.trail = vec![
        Back {
            title: "Today".to_owned(),
            href: "/".to_owned(),
        },
        Back {
            title: "Orders".to_owned(),
            href: "/orders".to_owned(),
        },
    ];
    screen
}

/// What staff may do to an order. The client places each by its role; one
/// that can't be taken now says why, and one that can't be undone asks first
/// in the system's own words.
pub fn order_actions(view: &OrderView) -> Vec<Button> {
    let order = &view.order;
    let held = |button: Button, held: Result<(), shop::Refusal>| match held {
        Ok(()) => button,
        Err(refusal) => button.unavailable(refusal.message(&order.number)),
    };
    let nothing = Vec::<(String, String)>::new;
    let back = shop::refundable(order).unwrap_or(0);
    vec![
        held(
            Button::new("Resend confirmation")
                .id("order-resend")
                .icon(Icon::named("send"))
                .role(Role::Secondary)
                .ask(
                    format!("Send the confirmation of {} again?", order.number),
                    Some(format!("It goes to {}.", order.email)),
                    "Send",
                )
                .post(format!("/orders/{}/resend", order.id), nothing()),
            shop::check_resend(order, view.now),
        ),
        held(
            Button::new("Refund")
                .id("order-refund")
                .icon(Icon::named("undo-2"))
                .role(Role::Secondary)
                .opens("refund"),
            shop::refundable(order).map(|_| ()),
        ),
        held(
            Button::new("Cancel order")
                .id("order-cancel")
                .icon(Icon::named("ban"))
                .role(Role::Destructive)
                .ask(
                    format!("Cancel {}?", order.number),
                    Some(if back > 0 {
                        format!(
                            "{} goes back to {}, and {} go back in stock. This can't be undone.",
                            money(back),
                            back_to(order),
                            units(order.items)
                        )
                    } else {
                        format!("{} go back in stock. This can't be undone.", units(order.items))
                    }),
                    "Cancel order",
                )
                .post(format!("/orders/{}/cancel", order.id), nothing()),
            shop::check_cancel(order),
        ),
    ]
}

/// The order's content: what an answer to an action draws again.
pub fn order_body(shown: &OrderScreen<'_>) -> Stack {
    let view = shown.view;
    Stack::down().id(PAGE).gap(4).parts((
        state(view, shown.today),
        items(view),
        customer(view),
        delivery(view),
        Section::titled("Progress").id("order-progress").parts(steps(&view.order, shown.history, shown.today)),
        (!shown.others.is_empty()).then(|| {
            Section::titled("Other orders").id("order-others").inset().parts(
                List::new().parts(shown.others.iter().map(|other| {
                    Row::new(other.number.clone())
                        .subtitle(when(other.placed_at, shown.today))
                        .value(money(other.total))
                        .to(format!("/orders/{}", other.id))
                })),
            )
        }),
        refund_dialog(view),
    ))
}

/// Where the order stands: its three states in words, its total set large,
/// and when it was placed.
fn state(view: &OrderView, today: NaiveDate) -> Card {
    let order = &view.order;
    let badge = |words: &str, tone: Tone| Badge::new(words).tone(tone).dot();
    let status = Status::parse(&order.status);
    let step = Fulfilment::parse(&order.fulfilment);
    let paid = Payment::parse(&order.payment);
    let tone = |good: bool, waiting: bool| match (good, waiting) {
        (true, _) => Tone::Success,
        (false, true) => Tone::Warning,
        (false, false) => Tone::Neutral,
    };
    Card::new().id("order-state").parts((
        Stack::across().gap(2).wrap().parts((
            badge(status.label(), tone(status == Status::Completed, status == Status::Open)),
            badge(paid.label(), tone(paid == Payment::Paid, paid == Payment::Pending)),
            badge(
                step.label(),
                tone(step == Fulfilment::Delivered, step == Fulfilment::Unfulfilled),
            ),
        )),
        Stack::across().gap(3).align(Align::Baseline).justify(Justify::Between).parts((
            Text::new(money(order.total - order.refunded))
                .id("order-total")
                .style(TextStyle::Display)
                .change(Change::Count),
            Text::new(units(order.items)).style(TextStyle::Callout).muted(),
        )),
        Text::new(format!("Placed {}", when(order.placed_at, today).to_lowercase_first()))
            .style(TextStyle::Callout)
            .muted(),
    ))
}

/// `Today, 09:41` in the middle of a sentence.
trait LowerFirst {
    fn to_lowercase_first(self) -> String;
}

impl LowerFirst for String {
    fn to_lowercase_first(self) -> String {
        match self.strip_prefix("Today") {
            Some(rest) => format!("today{rest}"),
            None => match self.strip_prefix("Yesterday") {
                Some(rest) => format!("yesterday{rest}"),
                None => format!("on {self}"),
            },
        }
    }
}

/// What was bought: a row a line, and the sums under them.
fn items(view: &OrderView) -> Section {
    let order = &view.order;
    let line = |line: &Line| {
        Row::new(line.name.clone())
            .leading(chip(product_symbol(&line.icon), Hue::Neutral))
            .subtitle(format!("{} × {}", thousands(line.quantity), money(line.price)))
            .trailing(Text::new(money(line.quantity * line.price)).tabular())
    };
    let mut sums = Facts::new()
        .id("order-sums")
        .fact("Subtotal", money(order.subtotal))
        .fact(
            "Shipping",
            if order.shipping == 0 { "Free".to_owned() } else { money(order.shipping) },
        );
    if order.refunded > 0 {
        sums = sums.fact("Refunded", format!("-{}", money(order.refunded)));
    }
    Section::titled("Items")
        .id("order-items")
        .inset()
        .parts(List::new().parts(view.lines.iter().map(line)))
        .footer(sums.fact("Total", money(order.total - order.refunded)))
}

/// Who bought it, and how to reach them.
fn customer(view: &OrderView) -> Section {
    let order = &view.order;
    let known = match view.since {
        Some(day) => format!(
            "{} {} since {}",
            thousands(i64::try_from(view.orders).unwrap_or(0)),
            if view.orders == 1 { "order" } else { "orders" },
            day.format("%b %Y")
        ),
        None => "First order".to_owned(),
    };
    let reach = |words: String, icon: &str, href: String| {
        Row::new(words)
            .leading(Icon::named(icon))
            .action(Action::present(href, Present::External))
    };
    Section::titled("Customer").id("order-customer").inset().parts(List::new().parts((
        Row::new(order.customer.clone())
            .leading(Avatar {
                name: order.customer.clone(),
                ..Avatar::default()
            })
            .subtitle(known),
        (!view.phone.is_empty()).then(|| reach(phone_text(&view.phone), "phone", format!("tel:{}", view.phone))),
        reach(order.email.clone(), "mail", format!("mailto:{}", order.email)),
    )))
}

/// Where it goes, where it comes from, and how it is paid.
fn delivery(view: &OrderView) -> Section {
    let order = &view.order;
    let house = warehouse(&order.warehouse);
    let to = if order.pin.is_empty() {
        place_of(&order.city, &order.state, &order.country)
    } else {
        format!(
            "{}, {}, PIN {}",
            order.city,
            state_name(&order.state),
            order.pin
        )
    };
    let carried = if order.tracking.is_empty() {
        format!("{}, once packed", house.carrier)
    } else {
        format!("{} · {}", house.carrier, order.tracking)
    };
    let mut facts = Facts::new()
        .id("order-delivery-facts")
        .fact("Ships to", to)
        .fact("Ships from", format!("{} warehouse", house.city))
        .fact("Carrier", carried)
        .fact("Paid with", paid_with(order));
    if !order.note.is_empty() {
        facts = facts.fact("Note", format!("“{}”", order.note));
    }
    Section::titled("Delivery and payment").id("order-delivery").inset().parts(facts)
}

/// The steps an order goes through: what happened, and what is still to come.
fn steps(order: &Order, history: &[Event], today: NaiveDate) -> Timeline {
    let house = warehouse(&order.warehouse);
    let cancelled = Status::parse(&order.status) == Status::Cancelled;
    let reached = |kind: Kind| history.iter().any(|event| event.kind == kind.key());
    let mut moments: Vec<Moment> = history
        .iter()
        .filter_map(|event| {
            let kind = Kind::parse(&event.kind)?;
            let (title, about) = match kind {
                Kind::Placed => (
                    "Placed".to_owned(),
                    format!("{} for {}", units(order.items), money(event.amount)),
                ),
                Kind::Paid => (
                    "Paid".to_owned(),
                    match order.method.as_str() {
                        "cod" => format!("In cash, to {}", event.actor),
                        _ => paid_with(order),
                    },
                ),
                Kind::Packed => ("Packed".to_owned(), format!("At the {} warehouse", house.city)),
                Kind::Shipped => (
                    "Shipped".to_owned(),
                    format!("With {} · {}", house.carrier, event.detail),
                ),
                Kind::Delivered => ("Delivered".to_owned(), order.city.clone()),
                Kind::Refunded => (
                    format!("Refunded {}", money(event.amount)),
                    format!("{}, by {}", event.detail, event.actor),
                ),
                Kind::Cancelled => ("Cancelled".to_owned(), format!("By {}", event.actor)),
                Kind::Resent => (
                    "Confirmation sent again".to_owned(),
                    format!("To {}", event.detail),
                ),
                Kind::LowStock | Kind::Restocked => return None,
            };
            let (icon, tone) = kind_symbol(kind);
            Some(
                Moment::new(title)
                    .subtitle(about)
                    .time(when(event.at, today))
                    .state(RowState::Done)
                    .icon(icon, tone),
            )
        })
        .collect();
    if !cancelled {
        let next = [
            (Kind::Paid, "Payment", "Waiting for the transfer"),
            (Kind::Packed, "Packing", "Picked and packed at the warehouse"),
            (Kind::Shipped, "Shipping", "Handed to the carrier"),
            (Kind::Delivered, "Delivery", "At the door"),
        ];
        let mut first = true;
        for (kind, title, about) in next {
            if reached(kind) {
                continue;
            }
            let (icon, _) = kind_symbol(kind);
            moments.push(
                Moment::new(title)
                    .subtitle(about)
                    .time(if first { "Next" } else { "Later" })
                    .state(if first { RowState::Current } else { RowState::Upcoming })
                    .icon(icon, Tone::Neutral),
            );
            first = false;
        }
    }
    Timeline::new().id("order-steps").moments(moments)
}

/// The refund: an amount, which starts at what is left, and a reason. It is
/// sent to the handler the page's form is sent to, and what is wrong with a
/// field comes back at the field.
pub fn refund_dialog(view: &OrderView) -> Dialog {
    let order = &view.order;
    let left = shop::refundable(order).unwrap_or(0);
    Dialog::new("refund")
        .title(format!("Refund {}", order.number))
        .text(format!("Up to {} can go back to {}.", money(left), back_to(order)))
        .parts(
            Form::post(format!("/orders/{}/refund", order.id)).id("refund").parts((
                Field::amount("amount", "₹")
                    .label("Amount")
                    .help("Up to what is left to refund. All of it refunds the order in full.")
                    .required()
                    .value(format!("{}.{:02}", left / 100, left % 100)),
                Field::choice(
                    "reason",
                    ChoiceKind::Menu,
                    [
                        ("returned", "Returned by the customer"),
                        ("damaged", "Arrived damaged"),
                        ("late", "Arrived late"),
                        ("goodwill", "A goodwill gesture"),
                    ],
                )
                .label("Reason")
                .value("returned"),
            )),
        )
        .action((
            Button::new("Keep").role(Role::Cancel).dismisses(),
            Button::new("Refund").role(Role::Confirm).action(Action::Submit {
                form: Some("refund".to_owned()),
                href: None,
                with: None,
            }),
        ))
}

// ---------------------------------------------------------------------------
// The shelves
// ---------------------------------------------------------------------------

/// What the shelves' screen is drawn from.
pub struct Shelves<'a> {
    /// The shelves that need a hand, emptiest first.
    pub low: &'a [Product],
    pub sellers: &'a [Seller],
    /// How many products are out of stock, low, and stocked.
    pub out: i64,
    pub running_low: i64,
    pub stocked: i64,
    /// When the shelves were counted.
    pub now: i64,
}

/// A shelf's level in its hue, its tone and its words: the words are always
/// there, so the colour never speaks alone.
fn level(product: &Product) -> (Hue, Tone, &'static str) {
    match Level::parse(&product.level) {
        Level::Out => (Hue::Danger, Tone::Danger, "Out of stock"),
        Level::Low => (Hue::Warning, Tone::Warning, "Running low"),
        Level::InStock => (Hue::Success, Tone::Success, "In stock"),
    }
}

/// The shelves that need a hand: how many are empty, low and stocked, the
/// emptiest as a wall of tiles, and what sold today. A table of products
/// can't say this at a glance, so the screen is composed.
pub fn shelves(shown: &Shelves<'_>) -> Screen {
    let count = |id: &str, count: i64, words: &str, hue: Hue, weight: Weight| {
        Card::new().id(id).tint(hue).parts(
            Stack::across().gap(3).align(Align::Center).parts((
                Shape::capsule().fill(hue).size(Size::new(6, 36).scaled()),
                Stack::down().grow().parts((
                    Text::new(thousands(count))
                        .style(TextStyle::Title)
                        .weight(weight)
                        .change(Change::Count),
                    label(words),
                )),
            )),
        )
    };
    let wall = Stack::down().gap(3).grow().parts((
        heading("The emptiest shelves"),
        if shown.low.is_empty() {
            Node::from(Text::new("Every shelf is stocked.").muted())
        } else {
            Node::from(Grid::fit(160).gap(3).id("wall").parts(shown.low.iter().map(shelf_tile)))
        },
    ));
    let seller = |seller: &Seller| {
        Card::new().pad(3).parts((
            chip(product_symbol(&seller.product.icon), Hue::Accent),
            Text::new(seller.product.name.clone())
                .style(TextStyle::Callout)
                .weight(Weight::Medium)
                .lines(2),
            Text::new(format!("{} · {}", units(seller.units), money(seller.revenue)))
                .style(TextStyle::Caption)
                .muted(),
        ))
    };
    // A run that scrolls across on a phone; a list beside the wall in a wide room.
    let sellers = Adaptive::new()
        .compact(Stack::down().gap(3).parts((
            heading("Top sellers today"),
            if shown.sellers.is_empty() {
                Node::from(Text::new("Nothing sold yet today.").muted())
            } else {
                Node::from(
                    Carousel::new()
                        .min(148)
                        .gap(3)
                        .label("Top sellers today")
                        .parts(shown.sellers.iter().map(seller)),
                )
            },
        )))
        .wide(Section::titled("Top sellers today").inset().parts(List::new().parts(
            shown.sellers.iter().map(|seller| {
                Row::new(seller.product.name.clone())
                    .leading(product_symbol(&seller.product.icon))
                    .subtitle(units(seller.units))
                    .value(money(seller.revenue))
            }),
        )));
    Screen::titled("Shelves")
        .back("Today", "/")
        .nav("/inventory")
        .refresh()
        .action(
            Button::new("Inventory")
                .icon(Icon::named("boxes"))
                .role(Role::Secondary)
                .to("/inventory"),
        )
        .body(Stack::down().id("shelves").gap(4).parts((
            Stack::down().parts((
                Text::new(thousands(shown.out + shown.running_low))
                    .id("needing")
                    .style(TextStyle::Display)
                    .change(Change::Count),
                Text::new("shelves need a hand").style(TextStyle::Callout).muted(),
                Text::new(format!("Counted at {}", clock(shown.now)))
                    .style(TextStyle::Caption)
                    .change(Change::Fade)
                    .muted(),
            )),
            Grid::fit(104).most(3).gap(3).id("levels").parts((
                count("level-out", shown.out, "Out of stock", Hue::Danger, Weight::Bold),
                count("level-low", shown.running_low, "Running low", Hue::Warning, Weight::Bold),
                count("level-in", shown.stocked, "Stocked", Hue::Success, Weight::Regular),
            )),
            Stack::down().gap(4).align(Align::Start).wide(Axis::Horizontal).parts((wall, sellers)),
            // Where a tap on a tile opens the product: the server fills it.
            Sheet::new(inventory::SHEET),
        )))
}

/// One shelf: a tile that opens the product where the reader is and grows
/// into it, with a restock on a long press. An answer to a restock draws it
/// again.
pub fn shelf_tile(product: &Product) -> Card {
    let (hue, tone, words) = level(product);
    let left = match product.stock {
        stock if stock <= 0 => format!("None left · reorders at {}", thousands(product.reorder_at)),
        stock => format!(
            "{} left · reorders at {}",
            thousands(stock),
            thousands(product.reorder_at)
        ),
    };
    Card::new()
        .id(format!("shelf-{}", product.id))
        .opens(
            format!("{}, {}", product.name, words.to_lowercase()),
            Action::request(format!("/inventory/{}/peek", product.id)),
        )
        .zoom()
        .appear(Appear::Scale)
        .menu(
            Button::new("Restock 24")
                .icon(Icon::named("package-plus"))
                .post(format!("/inventory/{}/restock", product.id), [("quantity", "24")]),
        )
        .parts((
            Stack::across().justify(Justify::Between).align(Align::Start).parts((
                // The product's mark, with a dot of its level at its corner.
                Layers::new().place(Place::BottomEnd).parts((
                    chip(product_symbol(&product.icon), hue),
                    Shape::circle().fill(hue).size(Size::square(12)).label(words),
                )),
                Badge::new(words).tone(tone),
            )),
            Text::new(product.name.clone()).style(TextStyle::Headline).lines(2),
            Text::new(left).style(TextStyle::Caption).muted(),
            Progress::of(product.stock.max(0) as f64, product.reorder_at.max(1) as f64)
                .label("Stock against the point it reorders at"),
        ))
}
