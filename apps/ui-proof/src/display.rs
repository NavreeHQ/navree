//! Timeline, ActivityFeed, FactCard and live regions, in every state, against a
//! real server-sent event stream.
//!
//! | Page | Shows |
//! |---|---|
//! | `/display` | every component, with `?case=` for one state (`empty`, `one`, `many`, `long`, `rtl`, `hostile`, `hidden`, `loading`, `failed`) |
//! | `/display/live` | a live feed and a live table on one stream; `?window=5|20|all` |
//! | `/display/people/{n}` | a recipient: outcome chips, fact cards and a timeline |
//! | `/display/events` | the event stream (`text/event-stream`) |
//! | `/display/fragments/feed?after=` | the rows newer than a cursor, as HTML, with `X-UI-Cursor` |
//!
//! The test hooks under `/display/test/` drive the stream from a browser test:
//! add activity, speak without adding any, cut every open stream, refuse the next
//! connections, send hostile payloads, count connections.

use std::collections::VecDeque;
use std::convert::Infallible;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};
use std::time::Duration;

use http::header::{CACHE_CONTROL, CONTENT_TYPE};
use http::{HeaderName, HeaderValue, Method, StatusCode};
use navree::http::extract::{Path, Query};
use navree::http::response::sse::{Event, KeepAlive, Sse};
use navree::http::response::{IntoResponse, Response};
use navree::http::{Controller, RouteDef};
use navree::kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree::ui::*;
use serde::Deserialize;
use tokio::sync::broadcast;

/// 2026-11-02 09:41:07 in Asia/Kolkata.
const T0: i64 = 1_793_592_667;
const ZONE: &str = "Asia/Kolkata";
const OFFSET: i32 = 19_800;

const NASTY: &str = "<script>window.__pwned=1</script><img src=x onerror=\"window.__pwned=1\"> &amp; \"quotes\" 'single' `tick` ${x} {{y}} javascript:alert(1)";
const LONG: &str = "Supercalifragilisticexpialidocious-and-then-some-more-without-any-spaces-at-all-so-that-nothing-can-wrap-except-by-force-1234567890";

// A small flag drawn as SVG: the flag slot takes any node.
fn flag(code: &str) -> Node {
    let shapes = match code {
        "DE" => {
            r##"<rect width="3" height=".67" fill="#111"/><rect y=".67" width="3" height=".67" fill="#d00"/><rect y="1.33" width="3" height=".67" fill="#fc0"/>"##
        }
        "FR" => {
            r##"<rect width="1" height="2" fill="#0055a4"/><rect x="1" width="1" height="2" fill="#fff"/><rect x="2" width="1" height="2" fill="#ef4135"/>"##
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

struct Person {
    name: &'static str,
    email: &'static str,
    device: &'static str,
    place: &'static str,
    flag: &'static str,
}

const PEOPLE: [Person; 8] = [
    Person {
        name: "Maya Patel",
        email: "maya.patel@example.com",
        device: "Safari on macOS",
        place: "Mumbai",
        flag: "IN",
    },
    Person {
        name: "Tomás García",
        email: "tomas.garcia@example.com",
        device: "Chrome on Android",
        place: "Berlin",
        flag: "DE",
    },
    Person {
        name: "Aiko Tanaka",
        email: "aiko.tanaka@example.com",
        device: "Firefox on Windows",
        place: "Paris",
        flag: "FR",
    },
    Person {
        name: "Lena Fischer",
        email: "lena.fischer@example.com",
        device: "Safari on iOS",
        place: "Berlin",
        flag: "DE",
    },
    Person {
        name: "Ravi Menon",
        email: "ravi.menon@example.com",
        device: "Chrome on Windows",
        place: "Pune",
        flag: "IN",
    },
    Person {
        name: "Claire Dubois",
        email: "claire.dubois@example.com",
        device: "Edge on Windows",
        place: "Paris",
        flag: "FR",
    },
    Person {
        name: "Noor Haddad",
        email: "noor.haddad@example.com",
        device: "Chrome on macOS",
        place: "Berlin",
        flag: "DE",
    },
    Person {
        name: "Dev Kapoor",
        email: "dev.kapoor@example.com",
        device: "Safari on iOS",
        place: "Delhi",
        flag: "IN",
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Delivered,
    Opened,
    Clicked,
    Replied,
    Unsubscribed,
    Bounced,
    Scanner,
}

impl Kind {
    fn tone(self) -> Tone {
        match self {
            Self::Delivered => Tone::Neutral,
            Self::Opened => Tone::Info,
            Self::Clicked => Tone::Success,
            Self::Replied => Tone::named("violet"),
            Self::Unsubscribed => Tone::Warning,
            Self::Bounced => Tone::Danger,
            Self::Scanner => Tone::Neutral,
        }
    }

    fn icon(self) -> icons::Icon {
        match self {
            Self::Delivered => icons::CHECK_CHECK,
            Self::Opened => icons::MAIL,
            Self::Clicked => icons::LINK,
            Self::Replied => icons::MESSAGE_SQUARE,
            Self::Unsubscribed => icons::BAN,
            Self::Bounced => icons::CIRCLE_X,
            Self::Scanner => icons::BOT,
        }
    }

    fn verb(self) -> &'static str {
        match self {
            Self::Delivered => "received",
            Self::Opened => "opened",
            Self::Clicked => "clicked the pricing link in",
            Self::Replied => "replied to",
            Self::Unsubscribed => "unsubscribed from",
            Self::Bounced => "bounced on",
            Self::Scanner => "was checked by a mail scanner for",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Delivered => "Delivered",
            Self::Opened => "Opened the email",
            Self::Clicked => "Clicked the pricing link",
            Self::Replied => "Replied",
            Self::Unsubscribed => "Unsubscribed",
            Self::Bounced => "Bounced",
            Self::Scanner => "Checked by a mail scanner",
        }
    }
}

#[derive(Clone, Debug)]
struct Ev {
    id: u64,
    person: usize,
    kind: Kind,
    at: i64,
}

fn seed() -> Vec<Ev> {
    use Kind::*;
    let plan = [
        (0, Delivered, 1),
        (1, Delivered, 1),
        (2, Delivered, 2),
        (3, Delivered, 2),
        (3, Scanner, 3),
        (0, Opened, 95),
        (4, Delivered, 100),
        (1, Opened, 140),
        (0, Clicked, 311),
        (5, Bounced, 330),
        (2, Opened, 410),
        (6, Delivered, 415),
        (2, Unsubscribed, 470),
        (1, Replied, 580),
    ];
    plan.iter()
        .enumerate()
        .map(|(at, (person, kind, secs))| Ev {
            id: at as u64 + 1,
            person: *person,
            kind: *kind,
            at: T0 + secs,
        })
        .collect()
}

#[derive(Clone, Debug)]
enum Msg {
    Changed,
    Table,
    Hostile,
    Drop,
}

struct State {
    events: Vec<Ev>,
    refuse: u32,
}

struct Hub {
    state: Mutex<State>,
    tx: broadcast::Sender<Msg>,
    attempts: AtomicU64,
    open: AtomicI64,
}

static HUB: LazyLock<Hub> = LazyLock::new(|| Hub {
    state: Mutex::new(State {
        events: seed(),
        refuse: 0,
    }),
    tx: broadcast::channel(64).0,
    attempts: AtomicU64::new(0),
    open: AtomicI64::new(0),
});

impl Hub {
    fn with<T>(&self, work: impl FnOnce(&mut State) -> T) -> T {
        work(&mut self.state.lock().unwrap_or_else(PoisonError::into_inner))
    }

    fn events(&self) -> Vec<Ev> {
        self.with(|state| state.events.clone())
    }

    fn add(&self, count: u32) {
        use Kind::*;
        self.with(|state| {
            for _ in 0..count {
                let id = state.events.last().map_or(1, |event| event.id + 1);
                let at = state.events.last().map_or(T0, |event| event.at) + 45;
                let kind = [Opened, Clicked, Delivered, Replied, Bounced][(id % 5) as usize];
                state.events.push(Ev {
                    id,
                    person: ((id * 3) % 8) as usize,
                    kind,
                    at,
                });
            }
        });
        let _ = self.tx.send(Msg::Changed);
        let _ = self.tx.send(Msg::Table);
    }

    fn reset(&self) {
        self.with(|state| {
            state.events = seed();
            state.refuse = 0;
        });
        self.attempts.store(0, Ordering::SeqCst);
    }
}

/// Counts an open stream while it lives.
struct Open;

impl Open {
    fn new() -> Self {
        HUB.open.fetch_add(1, Ordering::SeqCst);
        Self
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        HUB.open.fetch_sub(1, Ordering::SeqCst);
    }
}

// Rows

fn meta_for(person: &Person, kind: Kind) -> Vec<Meta> {
    if kind == Kind::Scanner {
        return Vec::new();
    }
    vec![
        Meta::icon(icons::MONITOR, person.device),
        Meta::flag(flag(person.flag), person.place),
    ]
}

fn item(event: &Ev, scanners_hidden: bool) -> ActivityItem {
    let person = &PEOPLE[event.person];
    let mut row = ActivityItem::new(person.name, event.kind.verb())
        .object("the Autumn launch email")
        .tone(event.kind.tone())
        .icon(event.kind.icon())
        .at(event.at)
        .tag("Autumn launch")
        .metas(meta_for(person, event.kind))
        .href(format!("/display/people/{}", event.person))
        .id(format!("event-{}", event.id));
    if event.kind == Kind::Scanner {
        row = row.muted();
        if scanners_hidden {
            row = row.filtered();
        }
    }
    row
}

fn timeline_event(event: &Ev) -> TimelineEvent {
    let person = &PEOPLE[event.person];
    let mut out = TimelineEvent::new(event.kind.title())
        .at(event.at)
        .tone(event.kind.tone())
        .metas(meta_for(person, event.kind));
    if event.kind == Kind::Scanner {
        out = out.muted().filtered();
    }
    out
}

fn status_of(person: usize, events: &[Ev]) -> &'static str {
    let mut status = "Queued";
    for event in events.iter().filter(|event| event.person == person) {
        status = match event.kind {
            Kind::Bounced => "Bounced",
            Kind::Unsubscribed => "Unsubscribed",
            Kind::Replied => "Replied",
            Kind::Clicked => "Clicked",
            Kind::Opened if status != "Clicked" && status != "Replied" => "Opened",
            Kind::Delivered if status == "Queued" => "Delivered",
            _ => status,
        };
    }
    status
}

fn status_tone(status: &&'static str) -> BadgeVariant {
    match *status {
        "Clicked" | "Replied" => BadgeVariant::Success,
        "Unsubscribed" => BadgeVariant::Warning,
        "Bounced" => BadgeVariant::Danger,
        _ => BadgeVariant::Neutral,
    }
}

#[derive(Clone, TableRow)]
#[table(per = 10)]
pub struct RecipientRow {
    #[col(skip)]
    pub id: i64,
    #[col(link = "/display/people/{id}", label = "Recipient")]
    pub name: String,
    #[col(hide_on_mobile, label = "Email")]
    pub email: &'static str,
    #[col(badge = status_tone, label = "Status")]
    pub status: &'static str,
}

fn recipients(events: &[Ev]) -> Vec<RecipientRow> {
    PEOPLE
        .iter()
        .enumerate()
        .map(|(at, person)| RecipientRow {
            id: at as i64,
            name: person.name.to_owned(),
            email: person.email,
            status: status_of(at, events),
        })
        .collect()
}

// The feed

#[derive(Deserialize, Default)]
struct Window {
    window: Option<String>,
}

fn limit(window: &Option<String>) -> usize {
    match window.as_deref() {
        Some("5") => 5,
        Some("20") => 20,
        _ => usize::MAX,
    }
}

fn period_select(window: &Option<String>) -> NativeSelect {
    NativeSelect::new("window")
        .attr("aria-label", "Period")
        .option("all", "Since launch")
        .option("20", "Last 20 events")
        .option("5", "Last 5 events")
        .selected(window.clone().unwrap_or_else(|| "all".to_owned()))
}

fn live_feed(window: &Option<String>) -> Region {
    let events = HUB.events();
    let newest = events.last().map_or(0, |event| event.id);
    let shown = events.iter().rev().take(limit(window));
    let src = match window.as_deref() {
        Some(w) => format!("/display/fragments/feed?window={w}"),
        None => "/display/fragments/feed".to_owned(),
    };
    ActivityFeed::new("activity", "Recent activity")
        .heading(2)
        .summary(format!("{} events", events.len()))
        .time_zone(ZONE, OFFSET)
        .height(FeedHeight::Md)
        .period_select("/display/live", "activity-region", period_select(window))
        .items(shown.map(|event| item(event, true)))
        .in_region(
            "activity-region",
            src,
            "/display/events",
            newest.to_string(),
        )
}

#[derive(Deserialize, Default)]
struct After {
    after: Option<u64>,
}

/// `GET /display/fragments/feed?after=`: only the rows newer than the cursor.
async fn fragment(Query(after): Query<After>) -> Response {
    let events = HUB.events();
    let after = after.after.unwrap_or(0);
    let fresh: Vec<&Ev> = events
        .iter()
        .filter(|event| event.id > after)
        .rev()
        .collect();
    let html = render(&ActivityFeed::rows(
        fresh.iter().map(|event| item(event, true)),
        OFFSET,
    ));
    let mut response = (
        [
            (CONTENT_TYPE, "text/html; charset=utf-8"),
            (CACHE_CONTROL, "no-store"),
        ],
        html,
    )
        .into_response();
    if let Some(newest) = fresh.first()
        && let Ok(value) = HeaderValue::from_str(&newest.id.to_string())
    {
        response
            .headers_mut()
            .insert(HeaderName::from_static("x-ui-cursor"), value);
    }
    response
}

// The stream

/// `GET /display/events`.
async fn events() -> Response {
    HUB.attempts.fetch_add(1, Ordering::SeqCst);
    let refused = HUB.with(|state| {
        let refused = state.refuse > 0;
        state.refuse = state.refuse.saturating_sub(1);
        refused
    });
    if refused {
        return (StatusCode::SERVICE_UNAVAILABLE, "try later").into_response();
    }
    let rx = HUB.tx.subscribe();
    let stream = futures_util::stream::unfold(
        (rx, Open::new(), VecDeque::<Event>::new()),
        |(mut rx, open, mut queue)| async move {
            loop {
                if let Some(event) = queue.pop_front() {
                    return Some((Ok::<_, Infallible>(event), (rx, open, queue)));
                }
                match rx.recv().await {
                    Ok(Msg::Changed) | Err(broadcast::error::RecvError::Lagged(_)) => {
                        queue.push_back(Event::default().data("changed"));
                    }
                    Ok(Msg::Table) => {
                        queue.push_back(Event::default().event("table").data("changed"));
                    }
                    Ok(Msg::Hostile) => queue.extend(hostile_events()),
                    Ok(Msg::Drop) | Err(broadcast::error::RecvError::Closed) => return None,
                }
            }
        },
    );
    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(10)))
        .into_response()
}

/// Everything a stream might say that a page must not believe.
fn hostile_events() -> Vec<Event> {
    vec![
        Event::default().data("<img src=x onerror=\"window.__pwned=1\">"),
        Event::default().data("<script>window.__pwned=1</script>"),
        Event::default().data("javascript:window.__pwned=1"),
        Event::default().data(
            "{\"__proto__\":{\"polluted\":1},\"target\":\"activity-region\",\"html\":\"<b>x</b>\"}",
        ),
        Event::default().data("x".repeat(512 * 1024)),
        Event::default()
            .event("__proto__")
            .data("{\"op\":\"remove\",\"target\":\"activity\"}"),
        Event::default().event("constructor").data("1"),
        Event::default()
            .event("activity-region")
            .data("<p id=activity-region>taken over</p>"),
        Event::default().id("../../etc/passwd").data("id"),
        Event::default()
            .retry(Duration::from_millis(1))
            .data("retry"),
    ]
}

// Pages

#[derive(Deserialize, Default)]
struct Case {
    case: Option<String>,
}

fn section(title: &'static str, content: impl IntoNodes) -> Element {
    el("section")
        .attr("data-case", title)
        .children((Heading::h2(title), Stack::gap(3).children(content)))
}

fn long_timeline() -> Timeline {
    let mut timeline = Timeline::new("many", "Forty steps").time_zone(ZONE, OFFSET);
    for step in 0..40_i64 {
        timeline = timeline.event(
            TimelineEvent::new(format!("Step {step}"))
                .at(T0 + step * step * 700)
                .tone(
                    [
                        Tone::Neutral,
                        Tone::Info,
                        Tone::Success,
                        Tone::Warning,
                        Tone::Danger,
                    ][(step % 5) as usize],
                ),
        );
    }
    timeline
}

fn nasty_timeline() -> Timeline {
    Timeline::new("hostile", NASTY)
        .time_zone(ZONE, OFFSET)
        .event(
            TimelineEvent::new(NASTY)
                .at(T0)
                .detail(NASTY)
                .tone(Tone::Danger)
                .meta(Meta::icon(icons::GLOBE, NASTY))
                .gap(NASTY),
        )
}

fn long_timeline_text() -> Timeline {
    Timeline::new("long", "Long text")
        .time_zone(ZONE, OFFSET)
        .event(
            TimelineEvent::new(LONG)
                .at(T0)
                .detail(LONG)
                .meta(Meta::icon(icons::MAIL, LONG).ltr())
                .meta(Meta::flag(flag("IN"), LONG))
                .meta(Meta::icon(icons::MONITOR, "one"))
                .meta(Meta::icon(icons::MONITOR, "two"))
                .meta(Meta::icon(icons::MONITOR, "three"))
                .meta(Meta::icon(icons::MONITOR, "four")),
        )
}

fn rtl_timeline() -> Timeline {
    Timeline::new("rtl", "الجدول الزمني")
        .time_zone(ZONE, OFFSET)
        .event(
            TimelineEvent::new("فتح البريد الإلكتروني")
                .at(T0)
                .tone(Tone::Info)
                .detail("מהמכשיר של מאיה")
                .meta(Meta::icon(icons::MAIL, "maya@example.com").ltr()),
        )
        .event(
            TimelineEvent::new("نقر على الرابط")
                .at(T0 + 90)
                .tone(Tone::Success),
        )
}

fn hidden_timeline() -> Timeline {
    let events = HUB.events();
    Timeline::new("hidden", "Dev Kapoor")
        .time_zone(ZONE, OFFSET)
        .events(events.iter().take(6).map(timeline_event))
}

fn facts_1() -> FactGrid {
    FactGrid::new().card(
        FactCard::new("Outcome", Badge::new("Clicked").tone(Tone::Success).dot())
            .icon(icons::CHECK_CHECK)
            .tone(Tone::Success)
            .detail("After 5 min 11 s"),
    )
}

fn facts_5() -> FactGrid {
    FactGrid::new().cards([
        FactCard::new("Outcome", Badge::new("Clicked").tone(Tone::Success).dot())
            .icon(icons::CHECK_CHECK)
            .tone(Tone::Success)
            .detail("After 5 min 11 s"),
        FactCard::new("Device", "MacBook Air")
            .icon(icons::MONITOR)
            .tone(Tone::Info)
            .detail("Safari on macOS")
            .detail("1440 x 900"),
        FactCard::new("Place", "Mumbai, India")
            .icon(icons::MAP_PIN)
            .tone(Tone::named("violet"))
            .detail("Jio Fiber"),
        FactCard::new("Speed", "5 min 11 s")
            .icon(icons::GAUGE)
            .tone(Tone::Warning)
            .detail("Faster than 72% of recipients"),
        FactCard::new("Consent", "Marketing emails")
            .icon(icons::SHIELD_CHECK)
            .tone(Tone::named("cyan"))
            .detail("Given 12 Sep 2026"),
    ])
}

fn facts_long() -> FactGrid {
    FactGrid::new().cards([
        FactCard::new(LONG, LONG).icon(icons::MAP_PIN).detail(LONG),
        FactCard::new("Nasty", NASTY).detail(NASTY),
    ])
}

fn sample_feed(id: &'static str) -> ActivityFeed {
    let events = HUB.events();
    ActivityFeed::new(id, "Recent activity")
        .live()
        .summary(format!("{} events", events.len()))
        .time_zone(ZONE, OFFSET)
        .period_select("/display", "none", period_select(&None))
        .height(FeedHeight::Md)
        .items(events.iter().rev().map(|event| item(event, true)))
}

fn long_feed() -> ActivityFeed {
    ActivityFeed::new("long-feed", LONG)
        .summary(LONG)
        .item(
            ActivityItem::new(LONG, LONG)
                .object(LONG)
                .tag(LONG)
                .tone(Tone::Info)
                .at(T0)
                .meta(Meta::icon(icons::MAIL, LONG).ltr())
                .href("/display"),
        )
        .item(
            ActivityItem::new(NASTY, NASTY)
                .object(NASTY)
                .tag(NASTY)
                .time_text(NASTY)
                .meta(Meta::new(NASTY)),
        )
}

fn many_feed() -> ActivityFeed {
    ActivityFeed::new("many-feed", "Sixty rows")
        .height(FeedHeight::Sm)
        .items((0..60).map(|n| {
            ActivityItem::new(PEOPLE[n % 8].name, "opened")
                .tone(Tone::Info)
                .icon(icons::MAIL)
                .at(T0 + n as i64 * 61)
        }))
}

fn rtl_feed() -> Direction {
    Direction::rtl(
        ActivityFeed::new("rtl-feed", "النشاط الأخير")
            .summary("٣ أحداث")
            .item(
                ActivityItem::new("مايا باتيل", "فتحت")
                    .object("رسالة الخريف")
                    .tone(Tone::Info)
                    .icon(icons::MAIL)
                    .at(T0)
                    .meta(Meta::icon(icons::MAIL, "maya@example.com").ltr())
                    .href("/display"),
            ),
    )
}

fn badges() -> Row {
    Row::gap(2).wrap().children((
        Badge::new("Queued").dot(),
        Badge::new("Delivered").tone(Tone::Success).dot(),
        Badge::new("Opened").tone(Tone::Info).dot(),
        Badge::new("Delayed").tone(Tone::Warning).dot(),
        Badge::new("Bounced").tone(Tone::Danger).dot(),
        Badge::new("Replied").tone(Tone::named("violet")).dot(),
        Badge::new("Forwarded")
            .tone(Tone::named("cyan"))
            .icon(icons::ARROW_RIGHT),
        Badge::new("Complained")
            .tone(Tone::named("rose"))
            .icon_end(icons::TRIANGLE_ALERT),
        Badge::new("Won").variant(BadgeVariant::Success),
        Badge::new("Draft").variant(BadgeVariant::Outline),
    ))
}

fn outcomes() -> Row {
    Row::gap(2).wrap().children((
        Badge::outcome("Delivered", true).tone(Tone::Success),
        Badge::outcome("Opened", true).tone(Tone::Info),
        Badge::outcome("Clicked", true).tone(Tone::Success),
        Badge::outcome("Replied", false),
        Badge::outcome("Unsubscribed", false),
    ))
}

fn everything() -> Vec<Element> {
    vec![
        section("Badges", (badges(), outcomes())),
        section("Fact cards", (facts_5(), facts_1())),
        section(
            "Timeline",
            (
                Timeline::new("default", "Autumn launch email to Maya Patel")
                    .time_zone(ZONE, OFFSET)
                    .events(
                        HUB.events()
                            .iter()
                            .filter(|e| e.person == 0 || e.person == 1)
                            .map(timeline_event),
                    )
                    .note("Nothing else happened."),
                hidden_timeline(),
            ),
        ),
        section("Activity feed", (sample_feed("feed"), many_feed())),
    ]
}

fn cases(name: &str) -> Vec<Element> {
    match name {
        "empty" => vec![section(
            "Empty",
            (
                Timeline::new("empty", "Nothing"),
                ActivityFeed::new("empty-feed", "Recent activity").live(),
            ),
        )],
        "one" => vec![section(
            "One",
            (
                Timeline::new("one", "One step")
                    .time_zone(ZONE, OFFSET)
                    .event(TimelineEvent::new("Sent").at(T0)),
                ActivityFeed::new("one-feed", "Recent activity")
                    .item(ActivityItem::new("Maya Patel", "opened").at(T0)),
                facts_1(),
            ),
        )],
        "many" => vec![section("Many", (long_timeline(), many_feed()))],
        "long" => vec![section(
            "Long text",
            (long_timeline_text(), long_feed(), facts_long()),
        )],
        "rtl" => vec![section(
            "Right to left",
            Direction::rtl((rtl_timeline(), rtl_feed())),
        )],
        "hostile" => vec![section(
            "Hostile text",
            (
                nasty_timeline(),
                long_feed(),
                facts_long(),
                Badge::outcome(NASTY, true).tone(Tone::named("x;y")),
                Badge::new(NASTY).dot(),
            ),
        )],
        "hidden" => vec![section(
            "Hidden",
            (hidden_timeline(), sample_feed("hidden-feed")),
        )],
        "loading" => vec![section(
            "Loading",
            (
                Timeline::new("loading", "Loading").loading(),
                ActivityFeed::new("loading-feed", "Recent activity").loading(),
            ),
        )],
        "failed" => vec![section(
            "Failed",
            ActivityFeed::new("failed-feed", "Recent activity")
                .failed("The activity could not load.")
                .retry("/display?case=failed"),
        )],
        _ => everything(),
    }
}

/// `GET /display`.
fn gallery(shell: &AppShell, case: &Case) -> AppPage {
    shell
        .page("Display")
        .body(Stack::gap(6).children(cases(case.case.as_deref().unwrap_or(""))))
}

/// `GET /display/live`.
async fn live(
    shell: AppShell,
    Query(window): Query<Window>,
    query: TableQuery<RecipientRow>,
) -> AppPage {
    let events = HUB.events();
    let rows = recipients(&events);
    let clicked = rows.iter().filter(|row| row.status == "Clicked").count();
    let total = rows.len() as u64;
    shell
        .page("Live activity")
        .actions(
            el("form")
                .attr("method", "post")
                .attr("action", "/display/emit")
                .attr("data-ui-form", "")
                .uses(Uses::CORE)
                .child(
                    Button::outline("Simulate activity")
                        .icon(icons::PLUS)
                        .submit(),
                ),
        )
        .body(
            Stack::gap(4).children((
                FactGrid::new().cols(3).cards([
                    FactCard::new("Events", events.len().to_string()).icon(icons::ACTIVITY),
                    FactCard::new("Recipients", rows.len().to_string()).icon(icons::USERS),
                    FactCard::new("Clicked", clicked.to_string())
                        .icon(icons::LINK)
                        .tone(Tone::Success),
                ]),
                Grid::cols(2).gap(4).children((
                    live_feed(&window.window),
                    DataTable::new("recipients", &query, &rows)
                        .total(total)
                        .caption("Recipients")
                        .live_from("/display/events")
                        .live_event("table"),
                )),
            )),
        )
}

/// `GET /display/people/{n}`.
async fn person(shell: AppShell, Path(n): Path<usize>) -> AppPage {
    let events = HUB.events();
    let who = &PEOPLE[n % PEOPLE.len()];
    let mine: Vec<&Ev> = events
        .iter()
        .filter(|event| event.person == n % PEOPLE.len())
        .collect();
    let happened = |kind: Kind| mine.iter().any(|event| event.kind == kind);
    let first = mine.first().map_or(T0, |event| event.at);
    let clicked = mine
        .iter()
        .find(|event| event.kind == Kind::Clicked)
        .map(|event| event.at);
    shell.page(who.name).body(
        Stack::gap(4).children((
            Row::gap(2).wrap().children((
                Badge::outcome("Delivered", happened(Kind::Delivered)).tone(Tone::Success),
                Badge::outcome("Opened", happened(Kind::Opened)).tone(Tone::Info),
                Badge::outcome("Clicked", happened(Kind::Clicked)).tone(Tone::Success),
                Badge::outcome("Replied", happened(Kind::Replied)).tone(Tone::named("violet")),
            )),
            FactGrid::new().cards([
                FactCard::new(
                    "Status",
                    Badge::new(status_of(n % PEOPLE.len(), &events)).dot(),
                )
                .icon(icons::CHECK_CHECK)
                .detail(who.email),
                FactCard::new("Device", who.device)
                    .icon(icons::MONITOR)
                    .tone(Tone::Info),
                FactCard::new("Place", who.place)
                    .icon(icons::MAP_PIN)
                    .tone(Tone::named("violet"))
                    .detail("Asia/Kolkata"),
                FactCard::new(
                    "Speed",
                    clicked.map_or("No click yet".to_owned(), |at| format!("{} s", at - first)),
                )
                .icon(icons::GAUGE)
                .tone(Tone::Warning),
            ]),
            Card::titled("Timeline").body(
                Timeline::new("person", format!("Steps for {}", who.name))
                    .time_zone(ZONE, OFFSET)
                    .events(mine.iter().map(|event| timeline_event(event)))
                    .note("Nothing else happened."),
            ),
        )),
    )
}

// Test hooks

#[derive(Deserialize, Default)]
struct Count {
    n: Option<u32>,
}

async fn emit() -> Reply {
    HUB.add(1);
    Reply::ops(Vec::new()).toast(ToastTone::Success, "Activity added")
}

async fn test_emit(Query(count): Query<Count>) -> Response {
    HUB.add(count.n.unwrap_or(1).min(200));
    StatusCode::NO_CONTENT.into_response()
}

#[derive(Deserialize, Default)]
struct Nudge {
    kind: Option<String>,
}

/// Speaks on the stream without adding any activity: `kind=table` or the unnamed event.
async fn test_nudge(Query(nudge): Query<Nudge>) -> Response {
    let _ = HUB.tx.send(match nudge.kind.as_deref() {
        Some("table") => Msg::Table,
        _ => Msg::Changed,
    });
    StatusCode::NO_CONTENT.into_response()
}

async fn test_drop() -> Response {
    let _ = HUB.tx.send(Msg::Drop);
    StatusCode::NO_CONTENT.into_response()
}

async fn test_refuse(Query(count): Query<Count>) -> Response {
    HUB.with(|state| state.refuse = count.n.unwrap_or(1));
    StatusCode::NO_CONTENT.into_response()
}

async fn test_hostile() -> Response {
    let _ = HUB.tx.send(Msg::Hostile);
    StatusCode::NO_CONTENT.into_response()
}

async fn test_reset() -> Response {
    HUB.reset();
    StatusCode::NO_CONTENT.into_response()
}

async fn test_stats() -> Response {
    (
        [
            (CONTENT_TYPE, "application/json"),
            (CACHE_CONTROL, "no-store"),
        ],
        format!(
            "{{\"attempts\":{},\"open\":{},\"events\":{}}}",
            HUB.attempts.load(Ordering::SeqCst),
            HUB.open.load(Ordering::SeqCst),
            HUB.events().len()
        ),
    )
        .into_response()
}

pub struct DisplayPages;

impl Injectable for DisplayPages {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for DisplayPages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(
                Method::GET,
                "!/display",
                |shell: AppShell, Query(case): Query<Case>| async move { gallery(&shell, &case) },
            )
            .public(),
            RouteDef::on(Method::GET, "!/display/live", live).public(),
            RouteDef::on(Method::GET, "!/display/people/{n}", person).public(),
            RouteDef::on(Method::GET, "!/display/events", events).public(),
            RouteDef::on(Method::GET, "!/display/fragments/feed", fragment).public(),
            RouteDef::on(Method::POST, "!/display/emit", emit).public(),
            RouteDef::on(Method::POST, "!/display/test/emit", test_emit).public(),
            RouteDef::on(Method::POST, "!/display/test/nudge", test_nudge).public(),
            RouteDef::on(Method::POST, "!/display/test/drop", test_drop).public(),
            RouteDef::on(Method::POST, "!/display/test/refuse", test_refuse).public(),
            RouteDef::on(Method::POST, "!/display/test/hostile", test_hostile).public(),
            RouteDef::on(Method::POST, "!/display/test/reset", test_reset).public(),
            RouteDef::on(Method::GET, "!/display/test/stats", test_stats).public(),
        ]
    }
}
