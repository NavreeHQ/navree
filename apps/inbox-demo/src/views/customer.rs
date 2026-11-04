//! The customer pane: who they are, what they pay and have paid, what else they have
//! open, their tags, and the story of their account.

use navree::ui::*;

use crate::data::{
    Conversation, Customer, CustomerEvent, EventKind, OFFSET, Plan, Status, ZONE,
    thousands_of_cents,
};
use crate::views::inbox::CustomerView;
use crate::views::{Filters, ago, place, thousands};

/// The pane's region.
pub const CUSTOMER: &str = "customer-pane";

/// The heading: the person, where they work, and how to reach them.
pub fn customer_head(customer: &Customer, back: &str) -> Node {
    Stack::gap(2)
        .children((
            SplitPane::stacked_only(
                Row::gap(2).children(
                    Button::ghost("Conversation")
                        .icon(icons::ARROW_LEFT)
                        .size(Size::Sm)
                        .href(back.to_owned()),
                ),
            ),
            Row::gap(3).align(Align::Center).children((
                Avatar::placeholder(customer.name.clone()),
                Stack::gap(0).children((
                    Heading::h2(customer.name.clone()).id("customer-name"),
                    Text::muted(format!("{} at {}", customer.role, customer.company)),
                    Text::muted(customer.email.clone()),
                )),
            )),
        ))
        .into()
}

/// The facts an agent checks first: the plan, the money, and what else is open.
fn facts(view: &CustomerView) -> FactGrid {
    let customer = &view.customer;
    let plan = Plan::parse(&customer.plan);
    let monthly = plan.seat_cents() * customer.seats;
    FactGrid::new().cards([
        FactCard::new("Plan", plan.label())
            .icon(icons::LAYERS)
            .tone(Tone::Info)
            .detail(format!(
                "{} seats · customer since {}",
                customer.seats,
                customer
                    .since
                    .map_or_else(String::new, |day| day.format("%b %Y").to_string())
            )),
        FactCard::new(
            "Lifetime value",
            format!("${}", thousands_of_cents(customer.lifetime_cents)),
        )
        .icon(icons::WALLET)
        .tone(Tone::Success)
        .detail(format!("${} a month", thousands_of_cents(monthly))),
        FactCard::new("Open conversations", thousands(view.open))
            .icon(icons::MESSAGES_SQUARE)
            .tone(if view.overdue > 0 {
                Tone::Danger
            } else {
                Tone::Warning
            })
            .detail(match view.overdue {
                0 => "None past due".to_owned(),
                1 => "1 past due".to_owned(),
                n => format!("{} past due", thousands(n)),
            }),
        FactCard::new(
            "Location",
            Row::gap(2).align(Align::Center).children((
                Flag::country(&customer.country).decorative(),
                Text::new(customer.city.clone()).inline(),
            )),
        )
        .icon(icons::MAP_PIN)
        .tone(Tone::named("violet"))
        .detail(place(&customer.city, &customer.country)),
    ])
}

/// The customer's other conversations, newest first, each a link.
fn others(conversations: &[Conversation], current: i64, filters: &Filters, now: i64) -> Node {
    let rows: Vec<Node> = conversations
        .iter()
        .filter(|conversation| conversation.id != current)
        .take(6)
        .map(|conversation| {
            let status = Status::parse(&conversation.status).unwrap_or(Status::Open);
            el("li")
                .child(
                    Item::new(conversation.subject.clone())
                        .href(filters.href(&format!("/inbox/{}", conversation.id)))
                        .aside(ago(conversation.last_at, now))
                        .footer(Badge::new(status.label()).tone(status.tone()).dot())
                        .truncate(),
                )
                .into()
        })
        .collect();
    if rows.is_empty() {
        return Text::muted("No other conversations.").into();
    }
    el("ul").slot("stack").attr("data-gap", "1").child(rows).into()
}

/// One step of the account's story.
fn step(event: &CustomerEvent) -> TimelineEvent {
    let kind = EventKind::parse(&event.kind);
    TimelineEvent::new(event.title.clone())
        .at(event.at)
        .detail(event.detail.clone())
        .tone(kind.tone())
        .icon(kind.icon())
}

/// The pane's live part: the facts, the tags, the other conversations and the story.
pub fn customer_body(view: &CustomerView, current: i64, filters: &Filters, now: i64) -> Node {
    let customer = &view.customer;
    let tags: Vec<Node> = customer
        .tags
        .split(',')
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .map(|tag| {
            Badge::new(tag.to_owned())
                .icon(icons::TAG)
                .variant(BadgeVariant::Outline)
                .into()
        })
        .collect();
    Stack::gap(4)
        .children((
            facts(view),
            Stack::gap(2).children((
                Heading::h3("Tags"),
                if tags.is_empty() {
                    Text::muted("No tags.").into()
                } else {
                    Node::from(Row::gap(2).wrap().children(tags))
                },
            )),
            Stack::gap(2).children((
                Heading::h3("Other conversations"),
                others(&view.theirs, current, filters, now),
            )),
            Timeline::new("customer-timeline", "Account history")
                .heading(3)
                .newest_first()
                .time_zone(ZONE, OFFSET)
                .events(view.history.iter().map(step)),
        ))
        .into()
}
