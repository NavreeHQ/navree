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
        single!("regions", "live", "A feed fed by a stream", regions_live)
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
    Timeline::new("thumb", "Message")
        .event(
            TimelineEvent::new("Sent")
                .at(1_793_592_667)
                .tone(Tone::Success),
        )
        .event(
            TimelineEvent::new("Opened")
                .at(1_793_592_727)
                .tone(Tone::Info),
        )
        .event(TimelineEvent::new("Clicked a link").at(1_793_592_787))
}

fn timeline_default() -> Timeline {
    Timeline::new("message", "Autumn launch email to Maya Patel")
        .time_zone("Asia/Kolkata", 19_800)
        .event(
            TimelineEvent::new("Queued")
                .at(1_793_592_667)
                .tone(Tone::Neutral),
        )
        .event(
            TimelineEvent::new("Delivered")
                .at(1_793_592_668)
                .tone(Tone::Success)
                .detail("Accepted by the recipient's mail server"),
        )
        .event(
            TimelineEvent::new("Opened")
                .at(1_793_592_788)
                .tone(Tone::Info)
                .meta(Meta::icon(icons::MONITOR, "Safari on macOS"))
                .meta(Meta::flag(flag("IN"), "Mumbai")),
        )
        .event(
            TimelineEvent::new("Clicked the pricing link")
                .at(1_793_593_100)
                .tone(Tone::Success)
                .meta(Meta::icon(icons::LINK, "autumn.example.com/pricing")),
        )
        .event(
            TimelineEvent::new("Unsubscribed")
                .at(1_793_679_700)
                .tone(Tone::Warning)
                .detail("From the footer link")
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
            TimelineEvent::new("Brief approved")
                .time_text("Mon")
                .tone(Tone::Success),
        )
        .event(
            TimelineEvent::new("Audience built")
                .time_text("Tue")
                .tone(Tone::Success)
                .icon(icons::USERS),
        )
        .event(TimelineEvent::new("Sending").time_text("Now").current())
        .event(
            TimelineEvent::new("Reminder to openers")
                .time_text("Thu")
                .upcoming(),
        )
        .event(
            TimelineEvent::new("Link check by a mail scanner")
                .time_text("Mon")
                .muted()
                .icon(icons::BOT),
        )
}

fn timeline_meta() -> Timeline {
    Timeline::new("meta", "A click")
        .time_zone("Europe/Berlin", 3600)
        .event(
            TimelineEvent::new("Clicked the pricing link")
                .at(1_793_592_667)
                .tone(Tone::Success)
                .meta(Meta::icon(icons::MONITOR, "Firefox on Windows"))
                .meta(Meta::flag(flag("DE"), "Berlin, Germany"))
                .meta(Meta::icon(icons::GLOBE, "Telekom"))
                .meta(Meta::icon(icons::MAIL, "anna@example.com").ltr()),
        )
}

fn timeline_hidden() -> Timeline {
    Timeline::new("hidden", "Recent steps")
        .event(TimelineEvent::new("Sent").at(1_793_592_667))
        .event(
            TimelineEvent::new("Opened by a mail scanner")
                .at(1_793_592_668)
                .muted()
                .filtered(),
        )
        .event(
            TimelineEvent::new("Link checked by a mail scanner")
                .at(1_793_592_669)
                .muted()
                .filtered(),
        )
        .event(
            TimelineEvent::new("Opened")
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
        ActivityItem::new("Maya Patel", "clicked the pricing link in")
            .object("Autumn launch")
            .tone(Tone::Success)
            .icon(icons::LINK)
            .at(1_793_592_667)
            .tag("Newsletter")
            .meta(Meta::icon(icons::MONITOR, "Safari on macOS"))
            .meta(Meta::flag(flag("IN"), "Mumbai"))
            .href("#maya"),
        ActivityItem::new("Tomás García", "opened")
            .object("Autumn launch")
            .tone(Tone::Info)
            .icon(icons::MAIL)
            .at(1_793_592_600)
            .meta(Meta::icon(icons::MONITOR, "Chrome on Android"))
            .meta(Meta::flag(flag("DE"), "Berlin"))
            .href("#tomas"),
        ActivityItem::new("Aiko Tanaka", "unsubscribed from")
            .object("Autumn launch")
            .tone(Tone::Warning)
            .icon(icons::BAN)
            .at(1_793_592_540)
            .meta(Meta::icon(icons::MAIL, "aiko@example.com").ltr())
            .href("#aiko"),
        ActivityItem::new("A mail scanner", "checked a link in")
            .object("Autumn launch")
            .icon(icons::BOT)
            .at(1_793_592_500)
            .muted(),
    ]
}

fn feed_thumb() -> ActivityFeed {
    ActivityFeed::new("thumb", "Recent activity")
        .item(
            ActivityItem::new("Maya Patel", "opened")
                .tone(Tone::Info)
                .icon(icons::MAIL)
                .time_text("2 min ago"),
        )
        .item(
            ActivityItem::new("Tomás García", "clicked")
                .tone(Tone::Success)
                .icon(icons::LINK)
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
                .option("all", "Since launch")
                .selected("60"),
        )
        .height(FeedHeight::Md)
        .items(feed_rows())
}

fn feed_hidden() -> ActivityFeed {
    ActivityFeed::new("hidden", "Recent activity")
        .summary("3 events")
        .item(
            ActivityItem::new("Maya Patel", "opened")
                .tone(Tone::Info)
                .icon(icons::MAIL)
                .time_text("2 min ago"),
        )
        .item(
            ActivityItem::new("A mail scanner", "checked a link in")
                .object("Autumn launch")
                .icon(icons::BOT)
                .time_text("3 min ago")
                .muted()
                .filtered(),
        )
        .item(
            ActivityItem::new("A mail scanner", "opened")
                .object("Autumn launch")
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
    FactCard::new("Device", "iPhone 15")
        .icon(icons::MONITOR)
        .detail("Safari 18 on iOS 19")
}

fn fact_default() -> FactGrid {
    FactGrid::new().cols(2).cards([
        FactCard::new("Outcome", Badge::new("Clicked").tone(Tone::Success).dot())
            .icon(icons::CHECK_CHECK)
            .tone(Tone::Success)
            .detail("After 5 min 33 s"),
        FactCard::new("Device", "MacBook Air")
            .icon(icons::MONITOR)
            .tone(Tone::Info)
            .detail("Safari 18 on macOS")
            .detail("1440 x 900"),
        FactCard::new("Place", "Mumbai, India")
            .icon(icons::MAP_PIN)
            .tone(Tone::named("violet"))
            .detail("Jio Fiber"),
        FactCard::new("Speed", "5 min 33 s")
            .icon(icons::GAUGE)
            .tone(Tone::Warning)
            .detail("Faster than 72% of recipients"),
    ])
}

fn fact_single() -> FactCard {
    FactCard::new("Delivery", "Delivered")
        .icon(icons::SEND)
        .tone(Tone::Success)
        .detail("Sent 09:41:07")
        .detail("Accepted 09:41:08")
}

fn badge_dot() -> Row {
    Row::gap(2).wrap().children((
        Badge::new("Delivered").tone(Tone::Success).dot(),
        Badge::new("Opened").tone(Tone::Info).dot(),
        Badge::new("Bounced").tone(Tone::Danger).dot(),
        Badge::new("Queued").dot(),
    ))
}

fn badge_tones() -> impl IntoIterator<Item = (&'static str, Badge)> {
    [
        ("Neutral", Badge::new("Queued")),
        ("Info", Badge::new("Opened").tone(Tone::Info)),
        ("Success", Badge::new("Clicked").tone(Tone::Success)),
        ("Warning", Badge::new("Delayed").tone(Tone::Warning)),
        ("Danger", Badge::new("Bounced").tone(Tone::Danger)),
        ("Violet", Badge::new("Replied").tone(Tone::named("violet"))),
        ("Cyan", Badge::new("Forwarded").tone(Tone::named("cyan"))),
        ("Rose", Badge::new("Complained").tone(Tone::named("rose"))),
    ]
}

fn badge_outcome() -> Row {
    Row::gap(2).wrap().children((
        Badge::outcome("Delivered", true).tone(Tone::Success),
        Badge::outcome("Opened", true).tone(Tone::Info),
        Badge::outcome("Clicked", false),
        Badge::outcome("Replied", false),
    ))
}

fn regions_live() -> Region {
    ActivityFeed::new("live-activity", "Live activity")
        .summary("New rows arrive as the stream speaks")
        .item(
            ActivityItem::new("Maya Patel", "opened")
                .tone(Tone::Info)
                .icon(icons::MAIL)
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
