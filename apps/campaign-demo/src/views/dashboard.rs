//! The campaign's overview: its figures, a chart and a funnel drawn as SVG with
//! plain elements, and its activity as it happens.

use navree::ui::*;

use crate::data::{CAMPAIGN, Event, Kind, LINKS, OFFSET, Recipient, Status, ZONE};
use crate::views::recipient::read_on;
use crate::views::{moment, percent, proxy_labels, thousands};

/// How far the campaign has got, counted per recipient.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub sent: u64,
    pub delivered: u64,
    pub opened: u64,
    pub clicked: u64,
    pub converted: u64,
    pub unsubscribed: u64,
    pub bounced: u64,
    /// Opens a mail privacy proxy made, which count for nothing.
    pub proxied: u64,
}

impl Counts {
    /// The counts of a list of recipients and their events.
    pub fn of(recipients: &[Recipient], events: &[Event]) -> Self {
        let status = |s: Status| {
            recipients
                .iter()
                .filter(|recipient| recipient.status == s.key())
                .count() as u64
        };
        let sent = recipients.len() as u64;
        let bounced = status(Status::Bounced);
        Self {
            sent,
            delivered: sent - bounced,
            opened: recipients.iter().filter(|r| r.opens > 0).count() as u64,
            clicked: recipients.iter().filter(|r| r.clicks > 0).count() as u64,
            converted: status(Status::Converted),
            unsubscribed: status(Status::Unsubscribed),
            bounced,
            proxied: events
                .iter()
                .filter(|event| event.kind == Kind::ProxyOpen.key())
                .count() as u64,
        }
    }
}

/// The five figures across the top, each with its rate.
pub fn figures(counts: &Counts) -> Grid {
    Grid::cols(5).gap(3).children((
        Stat::new("Sent", thousands(counts.sent)),
        Stat::new("Delivered", thousands(counts.delivered)).trend(
            format!("{} of sent", percent(counts.delivered, counts.sent)),
            true,
        ),
        Stat::new("Opened", thousands(counts.opened)).trend(
            format!("{} open rate", percent(counts.opened, counts.delivered)),
            true,
        ),
        Stat::new("Clicked", thousands(counts.clicked)).trend(
            format!("{} click rate", percent(counts.clicked, counts.delivered)),
            true,
        ),
        Stat::new("Converted", thousands(counts.converted)).trend(
            format!("{} of clicks", percent(counts.converted, counts.clicked)),
            true,
        ),
    ))
}

/// Opens and clicks in each hour since the campaign was sent.
pub fn hourly(events: &[Event], start: i64, now: i64) -> Vec<(u32, u32)> {
    let hours = usize::try_from(((now - start) / 3600 + 1).clamp(1, 96)).unwrap_or(1);
    let mut out = vec![(0_u32, 0_u32); hours];
    for event in events {
        let Ok(at) = usize::try_from((event.at - start) / 3600) else {
            continue;
        };
        if let Some(hour) = out.get_mut(at) {
            match Kind::parse(&event.kind) {
                Kind::Opened => hour.0 += 1,
                Kind::Clicked => hour.1 += 1,
                _ => {}
            }
        }
    }
    out
}

/// A line through `values`, `width` wide and 100 high, the highest value at 8.
fn line(values: &[u32], top: u32, width: usize) -> String {
    let step = width as f64 / (values.len().max(2) - 1) as f64;
    values
        .iter()
        .enumerate()
        .map(|(at, value)| {
            let y = 100.0 - f64::from(*value) * 92.0 / f64::from(top.max(1));
            format!("{:.1},{:.1}", at as f64 * step, y)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A small swatch for the chart's legend.
fn swatch(colour: &'static str) -> Element {
    el("svg")
        .attr("viewBox", "0 0 10 10")
        .attr("width", 10_i64)
        .attr("height", 10_i64)
        .attr("aria-hidden", "true")
        .child(
            el("rect")
                .attr("width", 10_i64)
                .attr("height", 10_i64)
                .attr("fill", colour),
        )
}

/// Opens and clicks by hour, drawn with plain SVG elements: the theme's colours
/// come in as CSS variables, so the chart follows the theme.
pub fn engagement_chart(hours: &[(u32, u32)], start: i64) -> Card {
    let width = 300;
    let opens: Vec<u32> = hours.iter().map(|hour| hour.0).collect();
    let clicks: Vec<u32> = hours.iter().map(|hour| hour.1).collect();
    let top = opens.iter().chain(&clicks).copied().max().unwrap_or(1);
    let peak = opens
        .iter()
        .enumerate()
        .max_by_key(|(_, value)| **value)
        .map_or(0, |(at, _)| at);
    let area = format!("0,100 {} {width},100", line(&opens, top, width));
    let grid: Vec<Element> = [25_i64, 50, 75]
        .iter()
        .map(|y| {
            el("line")
                .attr("x1", 0_i64)
                .attr("x2", i64::try_from(width).unwrap_or(300))
                .attr("y1", *y)
                .attr("y2", *y)
                .attr("stroke", "var(--line)")
                .attr("vector-effect", "non-scaling-stroke")
        })
        .collect();
    let chart = el("svg")
        .attr("viewBox", format!("0 0 {width} 100"))
        .attr("preserveAspectRatio", "none")
        .attr("width", "100%")
        .attr("height", 168_i64)
        .attr("role", "img")
        .attr(
            "aria-label",
            format!(
                "Opens and clicks in each hour since the campaign was sent. Opens peaked in hour {} at {}.",
                peak + 1,
                opens.get(peak).copied().unwrap_or(0)
            ),
        )
        .children((
            grid,
            el("polygon")
                .attr("points", area)
                .attr("fill", "var(--info)")
                .attr("fill-opacity", "0.14"),
            el("polyline")
                .attr("points", line(&opens, top, width))
                .attr("fill", "none")
                .attr("stroke", "var(--info)")
                .attr("stroke-width", "1.75")
                .attr("stroke-linejoin", "round")
                .attr("vector-effect", "non-scaling-stroke"),
            el("polyline")
                .attr("points", line(&clicks, top, width))
                .attr("fill", "none")
                .attr("stroke", "var(--violet)")
                .attr("stroke-width", "1.75")
                .attr("stroke-linejoin", "round")
                .attr("vector-effect", "non-scaling-stroke"),
        ));
    Card::titled("Engagement by hour")
        .description(format!("Since {}, in {ZONE} time", moment(start)))
        .body(Stack::gap(2).children(
            (
                chart,
                Row::gap(4).justify(Justify::Between).wrap().children(
                    (
                        Row::gap(4).children(
                            (
                                Row::gap(2).align(Align::Center).children((
                                    swatch("var(--info)"),
                                    Text::muted("Opens").inline(),
                                )),
                                Row::gap(2).align(Align::Center).children((
                                    swatch("var(--violet)"),
                                    Text::muted("Clicks").inline(),
                                )),
                            ),
                        ),
                        Text::muted(format!("{} hours", hours.len())).inline(),
                    ),
                ),
            ),
        ))
}

/// One step of the funnel: its name, its count and share, and a bar.
fn stage(name: &'static str, count: u64, of: u64, colour: &'static str) -> Stack {
    let width = (count * 1000).checked_div(of).unwrap_or(0);
    Stack::gap(1).children((
        Row::gap(2).justify(Justify::Between).children((
            Text::new(name).inline(),
            Text::muted(format!("{} · {}", thousands(count), percent(count, of))).inline(),
        )),
        el("svg")
            .attr("viewBox", "0 0 1000 8")
            .attr("preserveAspectRatio", "none")
            .attr("width", "100%")
            .attr("height", 8_i64)
            .attr("aria-hidden", "true")
            .children((
                el("rect")
                    .attr("width", 1000_i64)
                    .attr("height", 8_i64)
                    .attr("fill", "var(--ink-3)"),
                el("rect")
                    .attr("width", i64::try_from(width).unwrap_or(0))
                    .attr("height", 8_i64)
                    .attr("fill", colour),
            )),
    ))
}

/// How many readers got to each step, of everyone it was sent to.
pub fn funnel(counts: &Counts) -> Card {
    Card::titled("Funnel")
        .description("Each step as a share of everyone it was sent to")
        .body(
            Stack::gap(3).children((
                stage("Delivered", counts.delivered, counts.sent, "var(--fg-3)"),
                stage("Opened", counts.opened, counts.sent, "var(--info)"),
                stage("Clicked", counts.clicked, counts.sent, "var(--violet)"),
                stage("Converted", counts.converted, counts.sent, "var(--accent)"),
                Row::gap(2).wrap().children((
                    Badge::new(format!("{} unsubscribed", thousands(counts.unsubscribed)))
                        .tone(Tone::Warning)
                        .dot(),
                    Badge::new(format!("{} bounced", thousands(counts.bounced)))
                        .tone(Tone::Danger)
                        .dot(),
                    Badge::new(format!(
                        "{} proxy opens left out",
                        thousands(counts.proxied)
                    ))
                    .icon(icons::SHIELD),
                )),
            )),
        )
}

/// How far back the feed looks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Period {
    Hour,
    #[default]
    Day,
    All,
}

impl Period {
    pub fn parse(key: Option<&str>) -> Self {
        match key {
            Some("1h") => Self::Hour,
            Some("all") => Self::All,
            _ => Self::Day,
        }
    }

    pub const fn key(self) -> &'static str {
        match self {
            Self::Hour => "1h",
            Self::Day => "24h",
            Self::All => "all",
        }
    }

    /// The earliest time the period holds.
    pub const fn since(self, now: i64) -> i64 {
        match self {
            Self::Hour => now - 3600,
            Self::Day => now - 24 * 3600,
            Self::All => 0,
        }
    }

    const fn words(self) -> &'static str {
        match self {
            Self::Hour => "in the last hour",
            Self::Day => "in the last 24 hours",
            Self::All => "since launch",
        }
    }
}

fn period_select(period: Period) -> NativeSelect {
    NativeSelect::new("period")
        .attr("aria-label", "Period")
        .option("1h", "Last hour")
        .option("24h", "Last 24 hours")
        .option("all", "Since launch")
        .selected(period.key())
}

/// A row of the feed: an event and who it happened to.
#[derive(Clone, Debug)]
pub struct FeedEntry {
    pub event: Event,
    pub name: String,
}

/// One row: who did what, in the tone of what it was, with the device and the
/// place under it. An open a proxy made is quiet and set aside.
pub fn activity_item(entry: &FeedEntry) -> ActivityItem {
    let event = &entry.event;
    let kind = Kind::parse(&event.kind);
    let verb = match kind {
        Kind::Opened => "opened".to_owned(),
        Kind::Clicked => format!("clicked “{}” in", short_link(&event.link)),
        Kind::Converted => "started a Pro trial from".to_owned(),
        Kind::Unsubscribed => "unsubscribed from".to_owned(),
        Kind::Bounced => "could not be reached by".to_owned(),
        Kind::ProxyOpen => "had images fetched by a mail proxy for".to_owned(),
        Kind::Sent | Kind::Delivered => "received".to_owned(),
    };
    let mut item = ActivityItem::new(entry.name.clone(), verb)
        .object(CAMPAIGN)
        .tone(kind.tone())
        .icon(kind.icon())
        .at(event.at)
        .metas(read_on(event))
        .href(format!("/recipients/{}", event.recipient_id))
        .id(format!("event-{}", event.id));
    if kind == Kind::Bounced {
        item = item.meta(Meta::icon(icons::SERVER, event.detail.clone()));
    }
    if kind == Kind::ProxyOpen {
        item = item.muted().filtered();
    }
    item
}

/// `Watch the tour` for its link.
fn short_link(url: &str) -> &'static str {
    match LINKS.iter().position(|(_, link)| *link == url) {
        Some(0) => "Watch the tour",
        Some(1) => "What's new",
        _ => "Start your Pro trial",
    }
}

/// The live feed: newest first, in a region that a server-sent event stream
/// keeps current. Opens a mail privacy proxy made sit behind a notice.
pub fn activity_feed(entries: &[FeedEntry], period: Period, total: u64) -> Region {
    let newest = entries
        .iter()
        .map(|entry| entry.event.id)
        .max()
        .unwrap_or(0);
    ActivityFeed::new("activity", "Recent activity")
        .heading(2)
        .live()
        .summary(format!("{} events {}", thousands(total), period.words()))
        .time_zone(ZONE, OFFSET)
        .labels(proxy_labels())
        .height(FeedHeight::Lg)
        .period_select("/", "activity-region", period_select(period))
        .items(entries.iter().map(activity_item))
        .empty(Empty::new("Quiet in this period").description("Choose a longer period."))
        .in_region(
            "activity-region",
            format!("/fragments/activity?period={}", period.key()),
            "/events",
            newest.to_string(),
        )
        .live_event("activity")
}

/// The figures, live: they refetch when the stream says the campaign moved.
pub fn live_figures(counts: &Counts) -> Region {
    Region::new("figures")
        .src("/fragments/figures")
        .live_from("/events")
        .live_event("stats")
        .children(figures(counts))
}

/// The chart and the funnel, live.
pub fn live_engagement(counts: &Counts, hours: &[(u32, u32)], start: i64) -> Region {
    Region::new("engagement")
        .src("/fragments/engagement")
        .live_from("/events")
        .live_event("stats")
        .children(engagement(counts, hours, start))
}

/// The chart above the funnel.
pub fn engagement(counts: &Counts, hours: &[(u32, u32)], start: i64) -> Stack {
    Stack::gap(4).children((engagement_chart(hours, start), funnel(counts)))
}
