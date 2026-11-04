//! One order: what was bought, how far it has got, who bought it and how they
//! paid, the route from the warehouse, the notes, and what staff can do to it.
//! The drawer over the orders table and the order's own page both draw this.

use chrono::NaiveDate;
use navree::ui::form::{FormView, Money};
use navree::ui::*;
use serde::Deserialize;

use crate::data::{
    Event, Fulfilment, Kind, Line, OFFSET, Order, Payment, Status, ZONE, product_icon, warehouse,
};
use crate::shop;
use crate::views::orders::{DRAWER, fulfilment_badge, payment_badge, status_badge};
use crate::views::{city, country_name, money, thousands, units, when};

/// The id of the element that holds an order on its own page.
pub const PAGE: &str = "order-page";

/// Everything an order's view needs.
#[derive(Clone, Debug)]
pub struct OrderView {
    pub order: Order,
    pub lines: Vec<Line>,
    /// How many orders its customer has placed, this one too.
    pub orders: u64,
    /// When the customer joined.
    pub since: Option<NaiveDate>,
    /// Now, for the actions that depend on the time.
    pub now: i64,
}

/// What a refund says: how much, and why.
#[derive(Debug, Deserialize, Validate, Form)]
#[form(id = "refund")]
pub struct RefundForm {
    /// Amount
    #[validate(custom(above_zero))]
    #[form(
        help = "Up to what is left to refund. All of it refunds the order in full.",
        error = "Enter an amount above zero, like 24.00."
    )]
    pub amount: Money,
    /// Reason
    #[validate(skip)]
    pub reason: Reason,
}

fn above_zero(amount: &Money) -> Result<(), String> {
    if amount.0 > 0 {
        Ok(())
    } else {
        Err("Enter an amount above zero, like 24.00.".to_owned())
    }
}

/// Why money goes back.
#[derive(Clone, Copy, Debug, Default, Deserialize, Options, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    /// Returned by the customer
    #[default]
    Returned,
    /// Arrived damaged
    Damaged,
    /// Arrived late
    Late,
    /// A goodwill gesture
    Goodwill,
}

impl Reason {
    /// The words the order's history keeps.
    pub const fn words(self) -> &'static str {
        match self {
            Self::Returned => "Returned by the customer",
            Self::Damaged => "Arrived damaged",
            Self::Late => "Arrived late",
            Self::Goodwill => "A goodwill gesture",
        }
    }
}

/// A cancel or a resend: the button is the whole form.
#[derive(Debug, Deserialize, Validate, Form)]
#[form(id = "confirm")]
pub struct ConfirmForm {}

/// A product's picture: its icon on a tile, drawn as SVG with the theme's
/// colours, so it follows the theme and needs no image file.
pub fn thumbnail(key: &str) -> Node {
    let glyph = product_icon(key);
    el("svg")
        .attr("viewBox", "0 0 40 40")
        .attr("width", 40_i64)
        .attr("height", 40_i64)
        .attr("aria-hidden", "true")
        .attr("focusable", "false")
        .children((
            el("rect")
                .attr("x", "0.5")
                .attr("y", "0.5")
                .attr("width", 39_i64)
                .attr("height", 39_i64)
                .attr("rx", 6_i64)
                .attr("fill", "var(--ink-2)")
                .attr("stroke", "var(--line)"),
            el("svg")
                .attr("x", 10_i64)
                .attr("y", 10_i64)
                .attr("width", 20_i64)
                .attr("height", 20_i64)
                .attr("viewBox", glyph.view_box())
                .attr("fill", "none")
                .attr("stroke", "var(--fg-1)")
                .attr("stroke-width", "1.75")
                .attr("stroke-linecap", "round")
                .attr("stroke-linejoin", "round")
                .child(Node::Static(glyph.shapes())),
        ))
        .into()
}

/// A product with its picture, its name and its SKU.
pub fn product(icon: &str, name: &str, sku: &str) -> Row {
    Row::gap(3).align(Align::Center).children((
        thumbnail(icon),
        Stack::gap(0).children((Text::new(name.to_owned()), Text::muted(sku.to_owned()))),
    ))
}

/// The lines, and what they add up to.
fn lines_card(view: &OrderView, level: u8) -> Card {
    let order = &view.order;
    let rows = view.lines.iter().map(|line| {
        TableRowEl::new((
            TableCell::new(product(&line.icon, &line.name, &line.sku)),
            TableCell::new(thousands(line.quantity)).numeric(),
            TableCell::new(money(line.price)).numeric().hide_on_mobile(),
            TableCell::new(money(line.price * line.quantity)).numeric(),
        ))
        .key(line.id)
    });
    let sum = |label: &'static str, value: String, strong: bool| {
        Row::gap(4).justify(Justify::Between).children((
            if strong {
                Text::new(label).inline()
            } else {
                Text::muted(label).inline()
            },
            if strong {
                Text::new(value).inline()
            } else {
                Text::muted(value).inline()
            },
        ))
    };
    Card::titled("Items")
        .heading(level)
        .description(format!(
            "{}, from the {} warehouse",
            units(order.items),
            warehouse(&order.warehouse).city
        ))
        .body(
            Stack::gap(3).children((
                Table::new()
                    .head((
                        TableHead::new("Product"),
                        TableHead::new("Qty").numeric(),
                        TableHead::new("Price").numeric().hide_on_mobile(),
                        TableHead::new("Total").numeric(),
                    ))
                    .rows(rows.collect::<Vec<_>>()),
                Separator::horizontal(),
                Stack::gap(1).children((
                    sum("Subtotal", money(order.subtotal), false),
                    sum(
                        "Shipping",
                        if order.shipping == 0 {
                            "Free".to_owned()
                        } else {
                            money(order.shipping)
                        },
                        false,
                    ),
                    (order.refunded > 0)
                        .then(|| sum("Refunded", format!("-{}", money(order.refunded)), false)),
                    sum("Total", money(order.total - order.refunded), true),
                )),
            )),
        )
}

/// Who bought it, how they paid, and where it goes.
fn facts(view: &OrderView) -> FactGrid {
    let order = &view.order;
    let house = warehouse(&order.warehouse);
    let customer = FactCard::new("Customer", order.customer.clone())
        .icon(icons::USER)
        .tone(Tone::Info)
        .detail(order.email.clone())
        .detail(match view.since {
            Some(day) => format!(
                "{} {} since {}",
                thousands(i64::try_from(view.orders).unwrap_or(0)),
                if view.orders == 1 { "order" } else { "orders" },
                day.format("%b %Y")
            ),
            None => "First order".to_owned(),
        });
    let paid = Payment::parse(&order.payment);
    let payment = if order.method == "card" {
        FactCard::new(
            "Payment",
            format!("{} ending {}", order.card_brand, order.card_last4),
        )
        .icon(icons::CREDIT_CARD)
    } else {
        FactCard::new("Payment", "Bank transfer").icon(icons::LANDMARK)
    }
    .tone(paid.tone())
    .detail(match paid {
        Payment::Pending => format!("{} expected", money(order.total)),
        _ => format!("{} paid", money(order.total)),
    })
    .detail(match paid {
        Payment::Refunded => "Refunded in full".to_owned(),
        Payment::PartiallyRefunded => format!("{} refunded", money(order.refunded)),
        Payment::Pending => "Waiting for the transfer".to_owned(),
        Payment::Paid => "Nothing refunded".to_owned(),
    });
    let from = FactCard::new("Ships from", city(house.country, house.city))
        .icon(icons::WAREHOUSE)
        .detail(format!("{} warehouse", country_name(house.country)));
    let step = Fulfilment::parse(&order.fulfilment);
    let to = FactCard::new("Ships to", city(&order.country, &order.city))
        .icon(icons::MAP_PIN)
        .tone(Tone::named("violet"))
        .detail(country_name(&order.country))
        .detail(if order.tracking.is_empty() {
            match step {
                _ if Status::parse(&order.status) == Status::Cancelled => "Not shipping".to_owned(),
                _ => format!("{}, once packed", house.carrier),
            }
        } else {
            format!("{} · {}", house.carrier, order.tracking)
        });
    FactGrid::new().cols(2).cards([customer, payment, from, to])
}

/// The steps an order goes through, the ones to come drawn as rings, and what
/// else happened to it in time order.
pub fn steps(order: &Order, history: &[Event], level: u8) -> Timeline {
    let house = warehouse(&order.warehouse);
    let cancelled = Status::parse(&order.status) == Status::Cancelled;
    let reached = |kind: Kind| history.iter().any(|event| event.kind == kind.key());
    let mut events: Vec<TimelineEvent> = history
        .iter()
        .filter_map(|event| {
            let kind = Kind::parse(&event.kind)?;
            let step = match kind {
                Kind::Placed => TimelineEvent::new("Placed").detail(format!(
                    "{} for {}",
                    units(order.items),
                    money(event.amount)
                )),
                Kind::Paid => TimelineEvent::new("Paid").detail(if order.method == "card" {
                    format!("{} ending {}", order.card_brand, order.card_last4)
                } else {
                    "Bank transfer received".to_owned()
                }),
                Kind::Packed => {
                    TimelineEvent::new("Packed").detail(format!("At the {} warehouse", house.city))
                }
                Kind::Shipped => TimelineEvent::new("Shipped")
                    .detail(format!("With {}", house.carrier))
                    .meta(Meta::icon(icons::TRUCK, event.detail.clone()).ltr()),
                Kind::Delivered => TimelineEvent::new("Delivered").meta(Meta::flag(
                    Flag::country(&order.country).size(FlagSize::Xs),
                    order.city.clone(),
                )),
                Kind::Refunded => TimelineEvent::new(format!("Refunded {}", money(event.amount)))
                    .detail(format!("{}, by {}", event.detail, event.actor)),
                Kind::Cancelled => {
                    TimelineEvent::new("Cancelled").detail(format!("By {}", event.actor))
                }
                Kind::Resent => TimelineEvent::new("Confirmation sent again")
                    .detail(format!("To {}", event.detail)),
                Kind::LowStock | Kind::Restocked => return None,
            };
            Some(step.at(event.at).tone(kind.tone()).icon(kind.icon()))
        })
        .collect();
    if !cancelled {
        let next = [
            (Kind::Paid, "Payment", "Waiting for the transfer"),
            (
                Kind::Packed,
                "Packing",
                "Picked and packed at the warehouse",
            ),
            (Kind::Shipped, "Shipping", "Handed to the carrier"),
            (Kind::Delivered, "Delivery", "At the door"),
        ];
        let mut first = true;
        for (kind, title, detail) in next {
            if reached(kind) {
                continue;
            }
            let step = TimelineEvent::new(title)
                .detail(detail)
                .icon(kind.icon())
                .time_text(if first { "Next" } else { "Later" });
            events.push(if first {
                step.current()
            } else {
                step.upcoming()
            });
            first = false;
        }
    }
    Timeline::new("order-steps", format!("The steps of {}", order.number))
        .heading(level)
        .time_zone(ZONE, OFFSET)
        .events(events)
}

/// What the customer wrote, and what the shop can't do any more.
fn notes(view: &OrderView, level: u8) -> Card {
    let order = &view.order;
    let note = if order.note.is_empty() {
        Text::muted("The customer left no note.")
    } else {
        Text::new(format!("“{}”", order.note))
    };
    Card::titled("Notes")
        .heading(level)
        .description(format!("From {} at checkout", order.customer))
        .body(note)
}

/// Why each action can't be taken now, for the line under the buttons.
fn held_back(view: &OrderView) -> Vec<String> {
    let order = &view.order;
    let mut out = Vec::new();
    if let Err(refusal) = shop::refundable(order) {
        out.push(refusal.message(&order.number));
    }
    if let Err(refusal) = shop::check_cancel(order)
        && refusal != shop::Refusal::Cancelled
    {
        out.push(refusal.message(&order.number));
    }
    out
}

/// The three actions, each opening a dialog that asks first.
pub fn actions(view: &OrderView) -> Row {
    let order = &view.order;
    Row::gap(2).wrap().children((
        Button::outline("Resend confirmation")
            .icon(icons::SEND)
            .disabled(shop::check_resend(order, view.now).is_err())
            .opens_dialog("resend"),
        Button::outline("Refund")
            .icon(icons::UNDO_2)
            .disabled(shop::refundable(order).is_err())
            .opens_dialog("refund"),
        Button::destructive("Cancel order")
            .icon(icons::BAN)
            .disabled(shop::check_cancel(order).is_err())
            .opens_dialog("cancel"),
    ))
}

/// The dialogs the actions open. Each posts to the server, which checks again.
pub fn dialogs(view: &OrderView) -> Node {
    let order = &view.order;
    let id = order.id;
    let left = shop::refundable(order).unwrap_or(0);
    let to = if order.method == "card" {
        format!("{} ending {}", order.card_brand, order.card_last4)
    } else {
        "the account it came from".to_owned()
    };
    let refund = Dialog::new("refund", format!("Refund {}", order.number))
        .description(format!("Up to {} can go back to {to}.", money(left)))
        .body(
            FormView::new()
                .of(&RefundForm {
                    amount: Money(left),
                    reason: Reason::default(),
                })
                .post(format!("/orders/{id}/refund"))
                .one_column()
                .submit("Refund"),
        );
    let back = shop::refundable(order).unwrap_or(0);
    let cancel = AlertDialog::new(
        "cancel",
        format!("Cancel {}?", order.number),
        if back > 0 {
            format!(
                "{} goes back to {to}, and {} go back in stock. This can't be undone.",
                money(back),
                units(order.items)
            )
        } else {
            format!(
                "Nothing was paid yet, and {} go back in stock. This can't be undone.",
                units(order.items)
            )
        },
    )
    .cancel("Keep the order")
    .action(post_form(
        format!("/orders/{id}/cancel"),
        Button::destructive("Cancel order").submit(),
    ));
    let resend = AlertDialog::new(
        "resend",
        "Send the confirmation again?",
        format!(
            "The confirmation of {} goes to {} once more. It can go out once a minute at most.",
            order.number, order.email
        ),
    )
    .cancel("Not now")
    .action(post_form(
        format!("/orders/{id}/resend"),
        Button::primary("Send it").submit(),
    ));
    Node::list((refund, cancel, resend))
}

/// A form that posts with the runtime, or as a plain form with JavaScript off.
fn post_form(action: String, button: Button) -> Element {
    el("form")
        .attr("method", "post")
        .attr("action", action)
        .flag("data-ui-form", true)
        .uses(Uses::CORE)
        .child(button)
}

/// The badges and the moment, across the top.
fn summary(view: &OrderView, today: NaiveDate) -> Row {
    let order = &view.order;
    Row::gap(3)
        .align(Align::Center)
        .justify(Justify::Between)
        .wrap()
        .children((
            Row::gap(2).wrap().children((
                status_badge(&order.status),
                fulfilment_badge(&order.fulfilment),
                payment_badge(&order.payment),
            )),
            Text::muted(format!(
                "Placed {} · {}",
                when(order.placed_at, today).to_lowercase_first(),
                money(order.total)
            ))
            .inline(),
        ))
}

/// `today, 09:41` from `Today, 09:41`: the moment inside a sentence.
trait LowerFirst {
    fn to_lowercase_first(self) -> String;
}

impl LowerFirst for String {
    fn to_lowercase_first(self) -> String {
        if self.starts_with("Today") || self.starts_with("Yesterday") {
            let mut chars = self.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_lowercase().chain(chars).collect()
            })
        } else {
            format!("on {self}")
        }
    }
}

/// The three actions with the ways to see the order, and under them why any
/// action is held back. At the top, so a drawer that opens takes the focus at its
/// top and starts where a reader reads.
fn toolbar(view: &OrderView, open_as_page: bool) -> Stack {
    let mut reasons = held_back(view);
    reasons.dedup();
    Stack::gap(2).children((
        Row::gap(2)
            .align(Align::Center)
            .justify(Justify::Between)
            .wrap()
            .children((
                actions(view),
                open_as_page.then(|| {
                    Button::ghost("Open as a page")
                        .icon(icons::EXTERNAL_LINK)
                        .href(format!("/orders/{}", view.order.id))
                }),
            )),
        reasons.into_iter().map(Text::muted).collect::<Vec<_>>(),
    ))
}

/// How an order's slower parts are drawn: its history, and the customer's other
/// orders.
#[derive(Clone, Copy, Debug)]
pub enum Parts<'a> {
    /// Fetched once the drawer has opened, each into its own region, so the
    /// drawer opens on one small answer.
    Lazy,
    /// Drawn with the rest, for the order's own page, which works with
    /// JavaScript off.
    Inline {
        /// The order's events, oldest first.
        history: &'a [Event],
        /// The customer's latest other orders, newest first.
        others: &'a [Order],
    },
}

/// The id of the region that holds an order's history.
pub const STEPS: &str = "order-steps";
/// The id of the region that holds the customer's other orders.
pub const OTHERS: &str = "order-others";

/// The customer's latest other orders: each a link with its date, its badge and
/// its total.
pub fn other_orders(others: &[Order], today: NaiveDate) -> Node {
    if others.is_empty() {
        return Text::muted("This is their first order.").into();
    }
    Stack::gap(3)
        .children(
            others
                .iter()
                .map(|other| {
                    Row::gap(3)
                        .align(Align::Center)
                        .justify(Justify::Between)
                        .wrap()
                        .children((
                            Stack::gap(0).children((
                                Button::new(other.number.clone())
                                    .variant(ButtonVariant::Link)
                                    .href(format!("/orders/{}", other.id)),
                                Text::muted(format!(
                                    "{} · {}",
                                    when(other.placed_at, today),
                                    units(other.items)
                                )),
                            )),
                            Row::gap(3).align(Align::Center).children((
                                fulfilment_badge(&other.fulfilment),
                                Text::new(money(other.total)).inline(),
                            )),
                        ))
                        .into()
                })
                .collect::<Vec<Node>>(),
        )
        .into()
}

/// Everything about one order: the summary and the actions, four facts, the
/// items, the steps, the customer's other orders and the notes. `level` is the
/// level of the headings inside, under the drawer's title or the page's.
pub fn order_detail(
    view: &OrderView,
    level: u8,
    today: NaiveDate,
    open_as_page: bool,
    parts: Parts<'_>,
) -> Stack {
    let order = &view.order;
    let id = order.id;
    let history: Node = match parts {
        Parts::Lazy => Region::new(STEPS)
            .src(format!("/orders/{id}/steps"))
            .lazy()
            .into(),
        Parts::Inline { history, .. } => steps(order, history, level + 1).into(),
    };
    let others: Node = match parts {
        Parts::Lazy => Region::new(OTHERS)
            .src(format!("/orders/{id}/others"))
            .lazy()
            .into(),
        Parts::Inline { others, .. } => other_orders(others, today),
    };
    Stack::gap(4).children((
        summary(view, today),
        toolbar(view, open_as_page),
        facts(view),
        lines_card(view, level),
        Card::titled("Progress")
            .heading(level)
            .description(match Fulfilment::parse(&order.fulfilment) {
                _ if Status::parse(&order.status) == Status::Cancelled => {
                    "Cancelled before it shipped".to_owned()
                }
                Fulfilment::Delivered => "Delivered".to_owned(),
                step => format!("{} so far", step.label()),
            })
            .body(history),
        Card::titled(format!("More from {}", order.customer))
            .heading(level)
            .description(format!(
                "{} {} in all",
                thousands(i64::try_from(view.orders).unwrap_or(0)),
                if view.orders == 1 { "order" } else { "orders" }
            ))
            .body(others),
        notes(view, level),
    ))
}

/// The drawer a row of the table opens, filled for one order: over the table,
/// 60% of the screen wide.
pub fn order_drawer(view: &OrderView, today: NaiveDate, parts: Parts<'_>) -> Sheet {
    let order = &view.order;
    Sheet::new(DRAWER, format!("Order {}", order.number))
        .width(SheetWidth::Xl)
        .description(format!(
            "{} for {} in {}",
            units(order.items),
            order.customer,
            country_name(&order.country)
        ))
        .body((order_detail(view, 3, today, true, parts), dialogs(view)))
}

/// The order on a page of its own: the same detail and the same dialogs.
pub fn order_page(view: &OrderView, today: NaiveDate, parts: Parts<'_>) -> Element {
    el("div")
        .attr("id", PAGE)
        .child((order_detail(view, 2, today, false, parts), dialogs(view)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_moment_reads_inside_a_sentence() {
        assert_eq!(
            "Today, 09:41".to_owned().to_lowercase_first(),
            "today, 09:41"
        );
        assert_eq!(
            "2 Nov, 14:20".to_owned().to_lowercase_first(),
            "on 2 Nov, 14:20"
        );
    }

    #[test]
    fn a_thumbnail_is_a_picture_of_the_products_icon_in_the_themes_colours() {
        let html = render(&thumbnail("lamp"));
        assert!(html.starts_with("<svg viewBox=\"0 0 40 40\""), "{html}");
        assert!(html.contains("fill=\"var(--ink-2)\""));
        assert!(html.contains(icons::LAMP_DESK.shapes()));
        assert!(!html.contains("style="));
    }
}
