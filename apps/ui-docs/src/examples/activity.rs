//! Timeline, ActivityFeed, FactCard and live regions.

use navree::ui::*;

use crate::catalog::{Example, gallery, single};

const SOURCE: &str = include_str!("activity.rs");

pub fn all() -> Vec<Example> {
    vec![
        single!("timeline", "thumb", "A timeline", timeline_thumb).start(),
        single!(
            "timeline",
            "default",
            "Days, times and gaps",
            timeline_default
        )
        .start(),
        gallery!("timeline", "tones", "Tones", timeline_tones).wide(),
        single!(
            "timeline",
            "states",
            "Where an event stands",
            timeline_states
        )
        .start(),
        single!("timeline", "meta", "Facts under an event", timeline_meta).start(),
        single!("timeline", "hidden", "Events set aside", timeline_hidden).start(),
        gallery!("timeline", "empty", "Empty and loading", timeline_empty).wide(),
        single!("timeline", "text", "Times written by hand", timeline_text).start(),
        single!("activity-feed", "thumb", "A feed", feed_thumb).start(),
        single!("activity-feed", "default", "Recent activity", feed_default)
            .start()
            .height(420)
            .streaming("on"),
        single!("activity-feed", "hidden", "Rows left out", feed_hidden)
            .start()
            .height(300),
        gallery!(
            "activity-feed",
            "states",
            "Loading, failed and empty",
            feed_states
        )
        .wide(),
        single!("fact-card", "thumb", "A fact card", fact_thumb).start(),
        single!("fact-card", "default", "A grid of facts", fact_default).start(),
        single!("fact-card", "single", "One card", fact_single).start(),
        single!("badge", "dot", "A dot and a tone", badge_dot),
        gallery!("badge", "tones", "Tones", badge_tones).narrow(),
        single!("badge", "outcome", "Outcome chips", badge_outcome),
        single!(
            "regions",
            "live",
            "A feed fed by a stream, which runs in your app",
            regions_live
        )
        .start()
        .streaming("on"),
    ]
}

/// A small flag, drawn as SVG: the flag slot takes any node.
fn flag(code: &str) -> Node {
    let shapes = match code {
        "DE" => {
            r##"<rect width="3" height=".67" fill="#111"/><rect y=".67" width="3" height=".67" fill="#d00"/><rect y="1.33" width="3" height=".67" fill="#fc0"/>"##
        }
        _ => {
            r##"<rect width="3" height=".67" fill="#f93"/><rect y=".67" width="3" height=".67" fill="#fff"/><rect y="1.33" width="3" height=".67" fill="#138808"/>"##
        }
    };
    el("svg")
        .attr("viewBox", "0 0 3 2")
        .child(Node::Static(shapes))
        .into()
}

fn timeline_thumb() -> Timeline {
    Timeline::new("thumb", "Order")
        .event(
            TimelineEvent::new("Placed")
                .at(1_793_592_667)
                .tone(Tone::Success),
        )
        .event(
            TimelineEvent::new("Paid")
                .at(1_793_592_727)
                .tone(Tone::Info),
        )
        .event(TimelineEvent::new("Packed").at(1_793_592_787))
}

fn timeline_default() -> Timeline {
    Timeline::new("order", "Order 4821 for Lena Fischer")
        .time_zone("Asia/Kolkata", 19_800)
        .event(
            TimelineEvent::new("Order placed")
                .at(1_793_592_667)
                .tone(Tone::Neutral),
        )
        .event(
            TimelineEvent::new("Payment accepted")
                .at(1_793_592_668)
                .tone(Tone::Success)
                .detail("Card ending 4242"),
        )
        .event(
            TimelineEvent::new("Packed")
                .at(1_793_594_468)
                .tone(Tone::Info)
                .meta(Meta::icon(icons::PACKAGE, "2 boxes"))
                .meta(Meta::flag(flag("IN"), "Pune warehouse")),
        )
        .event(
            TimelineEvent::new("Handed to the courier")
                .at(1_793_598_068)
                .tone(Tone::Success)
                .meta(Meta::icon(icons::TRUCK, "Swiftline Express")),
        )
        .event(
            TimelineEvent::new("Refund requested")
                .at(1_793_679_700)
                .tone(Tone::Warning)
                .detail("Ordered the wrong size")
                .meta(Meta::flag(flag("DE"), "Berlin")),
        )
}

fn timeline_tones() -> impl IntoIterator<Item = (&'static str, Timeline)> {
    [
        ("Neutral", Tone::Neutral),
        ("Info", Tone::Info),
        ("Success", Tone::Success),
        ("Warning", Tone::Warning),
        ("Danger", Tone::Danger),
        ("Violet", Tone::named("violet")),
    ]
    .map(|(name, tone)| {
        (
            name,
            Timeline::new(name, name)
                .event(TimelineEvent::new("An event").time_text("09:41").tone(tone)),
        )
    })
}

fn timeline_states() -> Timeline {
    Timeline::new("states", "Where each step stands")
        .event(
            TimelineEvent::new("Pull request approved")
                .time_text("Mon")
                .tone(Tone::Success),
        )
        .event(
            TimelineEvent::new("Merged to main")
                .time_text("Tue")
                .tone(Tone::Success)
                .icon(icons::GIT_MERGE),
        )
        .event(
            TimelineEvent::new("Deploying to staging")
                .time_text("Now")
                .current(),
        )
        .event(
            TimelineEvent::new("Release to production")
                .time_text("Thu")
                .upcoming(),
        )
        .event(
            TimelineEvent::new("Checked by the build bot")
                .time_text("Tue")
                .muted()
                .icon(icons::BOT),
        )
}

fn timeline_meta() -> Timeline {
    Timeline::new("meta", "A parcel scan")
        .time_zone("Europe/Berlin", 3600)
        .event(
            TimelineEvent::new("Scanned at the sorting hub")
                .at(1_793_620_000)
                .tone(Tone::Info)
                .meta(Meta::icon(icons::TRUCK, "Swiftline Express"))
                .meta(Meta::flag(flag("DE"), "Leipzig, Germany"))
                .meta(Meta::icon(icons::PACKAGE, "2 of 2 boxes"))
                .meta(Meta::icon(icons::TAG, "SL-90417-DE").ltr()),
        )
}

fn timeline_hidden() -> Timeline {
    Timeline::new("hidden", "Recent steps")
        .event(TimelineEvent::new("Pull request merged").at(1_793_592_667))
        .event(
            TimelineEvent::new("Checked by the build bot")
                .at(1_793_592_668)
                .muted()
                .filtered(),
        )
        .event(
            TimelineEvent::new("Labelled by the build bot")
                .at(1_793_592_669)
                .muted()
                .filtered(),
        )
        .event(
            TimelineEvent::new("Deployed to staging")
                .at(1_793_592_900)
                .tone(Tone::Info),
        )
}

fn timeline_empty() -> impl IntoIterator<Item = (&'static str, Timeline)> {
    [
        ("Empty", Timeline::new("none", "Steps")),
        ("Loading", Timeline::new("wait", "Steps").loading()),
    ]
}

fn timeline_text() -> Timeline {
    Timeline::new("text", "Project history")
        .day(
            "Last week",
            [
                TimelineEvent::new("Kick-off").time_text("Mon"),
                TimelineEvent::new("Design review").time_text("Wed"),
            ],
        )
        .day(
            "This week",
            [TimelineEvent::new("Launch").time_text("Tue").current()],
        )
        .note("Nothing else is planned.")
}

fn feed_rows() -> Vec<ActivityItem> {
    vec![
        ActivityItem::new("Dev Mehta", "replied to")
            .object("Ticket 2041")
            .tone(Tone::Success)
            .icon(icons::MESSAGE_SQUARE_REPLY)
            .at(1_793_592_667)
            .tag("Billing")
            .meta(Meta::icon(icons::TIMER, "First reply in 12 min"))
            .meta(Meta::icon(icons::BUILDING_COMPLEX, "Fernwood Supply"))
            .href("#ticket-2041"),
        ActivityItem::new("Tomás García", "assigned")
            .object("Ticket 2038 to Dev Mehta")
            .tone(Tone::Info)
            .icon(icons::USER_PLUS)
            .at(1_793_592_600)
            .meta(Meta::icon(icons::FLAG, "High priority"))
            .href("#ticket-2038"),
        ActivityItem::new("Aiko Tanaka", "escalated")
            .object("Ticket 2031")
            .tone(Tone::Warning)
            .icon(icons::TRIANGLE_ALERT)
            .at(1_793_592_540)
            .meta(Meta::icon(icons::CLOCK, "Misses the SLA in 20 min"))
            .meta(Meta::icon(icons::USER_ROUND, "mira@example.com").ltr())
            .href("#ticket-2031"),
        ActivityItem::new("The routing bot", "tagged")
            .object("Ticket 2041")
            .icon(icons::BOT)
            .at(1_793_592_500)
            .muted(),
    ]
}

fn feed_thumb() -> ActivityFeed {
    ActivityFeed::new("thumb", "Recent activity")
        .item(
            ActivityItem::new("Dev Mehta", "replied to a ticket")
                .tone(Tone::Success)
                .icon(icons::MESSAGE_SQUARE_REPLY)
                .time_text("2 min ago"),
        )
        .item(
            ActivityItem::new("Tomás García", "assigned a ticket")
                .tone(Tone::Info)
                .icon(icons::USER_PLUS)
                .time_text("5 min ago"),
        )
}

fn feed_default() -> ActivityFeed {
    ActivityFeed::new("activity", "Recent activity")
        .live()
        .summary("09:30 to 10:30 · 4 events")
        .time_zone("Asia/Kolkata", 19_800)
        .period_select(
            "/activity",
            "activity-region",
            NativeSelect::new("window")
                .attr("aria-label", "Period")
                .option("15", "Last 15 minutes")
                .option("60", "Last hour")
                .option("day", "Today")
                .selected("60"),
        )
        .height(FeedHeight::Md)
        .items(feed_rows())
}

fn feed_hidden() -> ActivityFeed {
    ActivityFeed::new("hidden", "Recent activity")
        .summary("3 events")
        .item(
            ActivityItem::new("Dev Mehta", "replied to a ticket")
                .tone(Tone::Success)
                .icon(icons::MESSAGE_SQUARE_REPLY)
                .time_text("2 min ago"),
        )
        .item(
            ActivityItem::new("The routing bot", "tagged")
                .object("Ticket 2041")
                .icon(icons::BOT)
                .time_text("3 min ago")
                .muted()
                .filtered(),
        )
        .item(
            ActivityItem::new("The help desk", "sent an auto-reply on")
                .object("Ticket 2041")
                .icon(icons::BOT)
                .time_text("4 min ago")
                .muted()
                .filtered(),
        )
}

fn feed_states() -> impl IntoIterator<Item = (&'static str, ActivityFeed)> {
    [
        (
            "Loading",
            ActivityFeed::new("loading", "Recent activity").loading(),
        ),
        (
            "Failed",
            ActivityFeed::new("failed", "Recent activity")
                .failed("The activity could not load.")
                .retry("/activity"),
        ),
        (
            "Empty",
            ActivityFeed::new("empty", "Recent activity").empty(
                Empty::new("Quiet for the last hour")
                    .description("Try a longer period.")
                    .icon(icons::ACTIVITY),
            ),
        ),
    ]
}

fn fact_thumb() -> FactCard {
    FactCard::new("Customer", "Fernwood Supply")
        .icon(icons::BUILDING_COMPLEX)
        .detail("Business plan since March 2024")
}

fn fact_default() -> FactGrid {
    FactGrid::new().cols(2).cards([
        FactCard::new("Status", Badge::new("Resolved").tone(Tone::Success).dot())
            .icon(icons::CHECK_CHECK)
            .tone(Tone::Success)
            .detail("After 5 h 33 min"),
        FactCard::new("Customer", "Fernwood Supply")
            .icon(icons::BUILDING_COMPLEX)
            .tone(Tone::Info)
            .detail("Business plan")
            .detail("14 users"),
        FactCard::new("Assigned to", "Dev Mehta")
            .icon(icons::USER_ROUND)
            .tone(Tone::named("violet"))
            .detail("Billing team"),
        FactCard::new("First reply", "12 min")
            .icon(icons::GAUGE)
            .tone(Tone::Warning)
            .detail("2 min over the 10 min target"),
    ])
}

fn fact_single() -> FactCard {
    FactCard::new("Shipment", "Out for delivery")
        .icon(icons::TRUCK)
        .tone(Tone::Success)
        .detail("Left the depot 07:15")
        .detail("Due today by 18:00")
}

fn badge_dot() -> Row {
    Row::gap(2).wrap().children((
        Badge::new("Paid").tone(Tone::Success).dot(),
        Badge::new("Packed").tone(Tone::Info).dot(),
        Badge::new("Payment failed").tone(Tone::Danger).dot(),
        Badge::new("Pending").dot(),
    ))
}

fn badge_tones() -> impl IntoIterator<Item = (&'static str, Badge)> {
    [
        ("Neutral", Badge::new("Pending")),
        ("Info", Badge::new("Packed").tone(Tone::Info)),
        ("Success", Badge::new("Paid").tone(Tone::Success)),
        ("Warning", Badge::new("Low stock").tone(Tone::Warning)),
        ("Danger", Badge::new("Out of stock").tone(Tone::Danger)),
        ("Violet", Badge::new("Preorder").tone(Tone::named("violet"))),
        ("Cyan", Badge::new("In transit").tone(Tone::named("cyan"))),
        ("Rose", Badge::new("Refunded").tone(Tone::named("rose"))),
    ]
}

fn badge_outcome() -> Row {
    Row::gap(2).wrap().children((
        Badge::outcome("Paid", true).tone(Tone::Success),
        Badge::outcome("Packed", true).tone(Tone::Info),
        Badge::outcome("Shipped", false),
        Badge::outcome("Refunded", false),
    ))
}

fn regions_live() -> Region {
    ActivityFeed::new("live-activity", "Live activity")
        .summary("New rows arrive as the stream speaks")
        .item(
            ActivityItem::new("Priya Nair", "replied to")
                .object("Ticket 2044")
                .tone(Tone::Success)
                .icon(icons::MESSAGE_SQUARE_REPLY)
                .time_text("just now")
                .id("event-41"),
        )
        .in_region(
            "live-region",
            "/fragments/activity",
            "/events/activity",
            "41",
        )
}
