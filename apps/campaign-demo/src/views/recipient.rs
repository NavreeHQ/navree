//! One recipient: who they are, how far their copy got, the facts about how they
//! read it, and everything that happened to it, day by day.

use navree::ui::*;

use crate::data::{
    CAMPAIGN, Event, Kind, LINKS, OFFSET, PREHEADER, Recipient, SENDER, SUBJECT, Status, ZONE,
};
use crate::views::recipients::SHEET;
use crate::views::{country_name, device_icon, moment, place, proxy_labels, span};

/// The first event of `kind`, by time.
fn first(events: &[Event], kind: Kind) -> Option<&Event> {
    events
        .iter()
        .filter(|event| event.kind == kind.key())
        .min_by_key(|event| event.at)
}

/// The facts under an event that a reader made: the device and the place.
pub fn read_on(event: &Event) -> Vec<Meta> {
    if event.device.is_empty() {
        return Vec::new();
    }
    vec![
        Meta::icon(
            device_icon(&event.device),
            format!("{} · {}", event.device, event.client),
        ),
        Meta::flag(
            Flag::country(&event.country).size(FlagSize::Xs),
            place(&event.city, &event.country),
        ),
    ]
}

/// One step of the timeline.
fn step(event: &Event) -> TimelineEvent {
    let kind = Kind::parse(&event.kind);
    let step = match kind {
        Kind::Sent => TimelineEvent::new("Sent").detail(format!("From {SENDER}")),
        Kind::Delivered => TimelineEvent::new("Delivered").detail(event.detail.clone()),
        Kind::ProxyOpen => TimelineEvent::new("Opened by a mail privacy proxy")
            .detail(event.detail.clone())
            .muted()
            .filtered(),
        Kind::Opened => TimelineEvent::new("Opened the email"),
        Kind::Clicked => TimelineEvent::new(format!("Clicked “{}”", event.detail))
            .meta(Meta::icon(icons::LINK, event.link.clone()).ltr()),
        Kind::Converted => TimelineEvent::new(event.detail.clone()),
        Kind::Unsubscribed => TimelineEvent::new("Unsubscribed").detail(event.detail.clone()),
        Kind::Bounced => TimelineEvent::new("Bounced").detail(event.detail.clone()),
    };
    step.at(event.at).tone(kind.tone()).metas(read_on(event))
}

/// Everything that happened to one copy, grouped by day, with the gap between
/// steps and the opens a proxy made set aside behind a notice. `level` is the
/// level of the day headings.
pub fn recipient_timeline(person: &Recipient, events: &[Event], level: u8) -> Timeline {
    Timeline::new(
        "recipient-timeline",
        format!("{CAMPAIGN} to {}", person.name),
    )
    .heading(level)
    .time_zone(ZONE, OFFSET)
    .labels(proxy_labels())
    .events(events.iter().map(step))
}

/// The outcome chips: what did and didn't happen.
fn outcomes(events: &[Event]) -> Row {
    let did = |kind: Kind| first(events, kind).is_some();
    let mut chips = vec![
        Badge::outcome("Delivered", did(Kind::Delivered)).tone(Tone::Neutral),
        Badge::outcome("Opened", did(Kind::Opened)).tone(Tone::Info),
        Badge::outcome("Clicked", did(Kind::Clicked)).tone(Tone::named("violet")),
        Badge::outcome("Converted", did(Kind::Converted)).tone(Tone::Success),
    ];
    if did(Kind::Unsubscribed) {
        chips.push(Badge::outcome("Unsubscribed", true).tone(Tone::Warning));
    }
    if did(Kind::Bounced) {
        chips.push(Badge::outcome("Bounced", true).tone(Tone::Danger));
    }
    Row::gap(2).wrap().children(chips)
}

/// How fast the reader went from one step to the next.
fn speed(events: &[Event]) -> FactCard {
    let delivered = first(events, Kind::Delivered);
    let opened = first(events, Kind::Opened);
    let clicked = first(events, Kind::Clicked);
    let card = match (delivered, opened, clicked) {
        (_, Some(opened), Some(clicked)) => {
            FactCard::new("Engagement speed", span(clicked.at - opened.at)).detail(format!(
                "Clicked {} after opening",
                span(clicked.at - opened.at)
            ))
        }
        (Some(delivered), Some(opened), None) => FactCard::new("Engagement speed", "No click")
            .detail(format!(
                "Opened {} after delivery",
                span(opened.at - delivered.at)
            )),
        _ => FactCard::new("Engagement speed", "Not opened").detail("Nothing to time yet"),
    };
    let card = match (delivered, opened) {
        (Some(delivered), Some(opened)) if clicked.is_some() => card.detail(format!(
            "Opened {} after delivery",
            span(opened.at - delivered.at)
        )),
        _ => card,
    };
    card.icon(icons::GAUGE).tone(Tone::Warning)
}

/// The four facts at the top: the device, the place, the speed and the source.
pub fn recipient_facts(person: &Recipient, events: &[Event]) -> FactGrid {
    FactGrid::new().cols(2).cards([
        FactCard::new("Device", person.device.clone())
            .icon(device_icon(&person.device))
            .tone(Tone::Info)
            .detail(format!("{} on {}", person.client, person.os)),
        FactCard::new(
            "Location",
            Row::gap(2).align(Align::Center).children((
                Flag::country(&person.country).decorative(),
                Text::new(if person.city.is_empty() {
                    country_name(&person.country).to_owned()
                } else {
                    person.city.clone()
                })
                .inline(),
            )),
        )
        .icon(icons::MAP_PIN)
        .tone(Tone::named("violet"))
        .detail(country_name(&person.country)),
        speed(events),
        FactCard::new("Source", person.source.clone())
            .icon(icons::USER_PLUS)
            .tone(Tone::named("cyan"))
            .detail(match person.customer_since {
                Some(day) => format!("Customer since {}", day.format("%-d %b %Y")),
                None => "Not a customer yet".to_owned(),
            }),
    ])
}

/// The device and the network the reader used.
fn device_tab(person: &Recipient) -> FactGrid {
    FactGrid::new().cols(2).cards([
        FactCard::new("Device", person.device.clone())
            .icon(device_icon(&person.device))
            .detail(person.os.clone()),
        FactCard::new("Mail app", person.client.clone())
            .icon(icons::MAIL)
            .detail(if matches!(person.client.as_str(), "Apple Mail") {
                "Fetches images through a privacy proxy"
            } else {
                "Opens are counted as they happen"
            }),
        FactCard::new("Network", person.network.clone())
            .icon(icons::WIFI)
            .detail(Text::code(person.ip.clone())),
        FactCard::new(
            "Place",
            Row::gap(2).align(Align::Center).children((
                Flag::country(&person.country).decorative(),
                Text::new(place(&person.city, &person.country)).inline(),
            )),
        )
        .icon(icons::GLOBE)
        .detail("From the network address, not a GPS fix"),
    ])
}

/// The email as it was sent, and which of its links this reader followed.
fn email_tab(events: &[Event], level: u8) -> Stack {
    let clicked = |link: &str| events.iter().any(|event| event.link == link);
    let sent = first(events, Kind::Sent).map_or_else(String::new, |event| moment(event.at));
    Stack::gap(4).children((
        FactGrid::new().cols(2).cards([
            FactCard::new("Subject", SUBJECT)
                .icon(icons::MAIL)
                .detail(PREHEADER),
            FactCard::new("Sent", sent)
                .icon(icons::SEND)
                .detail(SENDER)
                .detail(format!("Times in {ZONE}")),
        ]),
        Card::titled("Links in the email")
            .heading(level)
            .body(Stack::gap(2).children(LINKS.map(|(text, url)| {
                Item::new(text)
                    .media(icon(icons::LINK))
                    .description(url)
                    .actions(if clicked(url) {
                        Badge::new("Clicked").tone(Tone::named("violet")).dot()
                    } else {
                        Badge::new("Not clicked").variant(BadgeVariant::Outline)
                    })
            }))),
    ))
}

/// The headers and the mail server's answers, for whoever needs to check them.
fn technical_tab(person: &Recipient, events: &[Event]) -> Table {
    let answer = first(events, Kind::Bounced)
        .or_else(|| first(events, Kind::Delivered))
        .map_or_else(String::new, |event| event.detail.clone());
    let row = |name: &'static str, value: String| {
        TableRowEl::new((TableCell::new(name), TableCell::new(Text::code(value))))
    };
    Table::new()
        .caption("Delivery details")
        .head((TableHead::new("Field"), TableHead::new("Value")))
        .rows([
            row(
                "Message ID",
                format!("<{}.spring-launch@larkspur.example>", person.id),
            ),
            row("To", person.email.clone()),
            row("Mail server's answer", answer),
            row("SPF", "pass".to_owned()),
            row("DKIM", "pass (larkspur.example)".to_owned()),
            row("DMARC", "pass".to_owned()),
            row("Reader's address", person.ip.clone()),
        ])
}

/// Everything about one recipient: who they are, the outcome chips, four facts,
/// and tabs for the timeline, the device, the email and the details. The sheet
/// and the recipient's own page both draw this; `level` is the level of the
/// headings inside, under the sheet's title or the page's.
pub fn recipient_detail(person: &Recipient, events: &[Event], level: u8) -> Stack {
    let status = Status::parse(&person.status).unwrap_or(Status::Delivered);
    Stack::gap(4).children((
        Row::gap(3)
            .align(Align::Center)
            .justify(Justify::Between)
            .wrap()
            .children((
                Row::gap(3).align(Align::Center).children((
                    Avatar::placeholder(person.name.clone()).size(Size::Lg),
                    Stack::gap(0).children((
                        Text::new(person.email.clone()),
                        Text::muted(format!("{} at {}", person.role, person.company)),
                        navree::validation::Phone::parse(&person.phone, None)
                            .ok()
                            .map(|phone| Text::muted(phone.international())),
                    )),
                )),
                Row::gap(2).children((
                    Badge::new(CAMPAIGN)
                        .icon(icons::MAIL)
                        .variant(BadgeVariant::Outline),
                    Badge::new(status.label()).tone(status.tone()).dot(),
                )),
            )),
        outcomes(events),
        recipient_facts(person, events),
        Tabs::new("recipient-tabs", "About this recipient")
            .param("section")
            .tab(
                Tab::new("timeline", "Timeline")
                    .count(events.len().to_string())
                    .panel(recipient_timeline(person, events, level)),
            )
            .tab(Tab::new("device", "Device and network").panel(device_tab(person)))
            .tab(Tab::new("email", "Email").panel(email_tab(events, level)))
            .tab(Tab::new("technical", "Technical").panel(technical_tab(person, events))),
    ))
}

/// The sheet a row of the table opens, filled for one recipient.
pub fn recipient_sheet(person: &Recipient, events: &[Event]) -> Sheet {
    Sheet::new(SHEET, person.name.clone())
        .width(SheetWidth::Lg)
        .body(recipient_detail(person, events, 3))
        .footer((
            Button::outline("Open as a page").href(format!("/recipients/{}", person.id)),
            Button::primary("Edit contact")
                .icon(icons::PENCIL)
                .href(format!("/recipients/{}/edit", person.id)),
        ))
}
