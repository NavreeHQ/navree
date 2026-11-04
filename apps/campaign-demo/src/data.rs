//! What the app stores: the recipients of one email campaign and everything that
//! happened to each copy, with a seed that writes the same campaign on every run.
//!
//! Every name, address and company here is invented. Addresses are on
//! `example.com`, `example.org` and `example.net`, network addresses are from the
//! ranges kept for documentation, and phone numbers are made from each country's
//! example number.

use chrono::{DateTime, NaiveDate};
use navree::orm::Model;
use navree::phone::{NumberType, PhoneNumber, Region};
use navree::ui::{Tone, icons};

/// The campaign the app follows.
pub const CAMPAIGN: &str = "Spring launch";
/// Who sends it.
pub const SENDER: &str = "Larkspur <news@larkspur.example>";
/// Its subject line.
pub const SUBJECT: &str = "Larkspur 3 is here: plan the whole season in one view";
/// The line a mail app shows under the subject.
pub const PREHEADER: &str =
    "Shared timelines, a calendar that follows your team, and a 14-day Pro trial.";
/// How many people it went to.
pub const RECIPIENTS: i64 = 1_240;
/// The campaign's own time zone, which every page shows its times in.
pub const ZONE: &str = "Europe/London";
/// That zone's offset from UTC in November, in seconds.
pub const OFFSET: i32 = 0;
/// How long before "now" the campaign was sent.
pub const SENT_AGO: i64 = 30 * 3600;

/// The links in the email: what each says and where it goes.
pub const LINKS: [(&str, &str); 3] = [
    ("Watch the two-minute tour", "larkspur.example/tour"),
    ("See what's new in Larkspur 3", "larkspur.example/whats-new"),
    ("Start your Pro trial", "larkspur.example/pro"),
];

/// A recipient, as stored.
#[derive(Model, Clone, Debug, Default, serde::Serialize)]
#[model(table = "recipients")]
pub struct Recipient {
    pub id: i64,
    pub name: String,
    pub email: String,
    pub role: String,
    pub company: String,
    /// A two-letter country code.
    pub country: String,
    pub city: String,
    pub device: String,
    pub os: String,
    pub client: String,
    pub network: String,
    pub ip: String,
    pub source: String,
    /// In E.164, or empty.
    pub phone: String,
    pub customer_since: Option<NaiveDate>,
    /// One of [`Status`], as its key.
    pub status: String,
    pub opens: i64,
    pub clicks: i64,
    pub last_activity: Option<NaiveDate>,
}

/// Something that happened to one recipient's copy.
#[derive(Model, Clone, Debug, Default, serde::Serialize)]
#[model(table = "events")]
pub struct Event {
    pub id: i64,
    pub recipient_id: i64,
    /// One of [`Kind`], as its key.
    pub kind: String,
    /// Unix seconds.
    pub at: i64,
    pub device: String,
    pub os: String,
    pub client: String,
    pub country: String,
    pub city: String,
    pub ip: String,
    /// The link clicked, for a click.
    pub link: String,
    /// A line of detail: the mail server's answer, the reason.
    pub detail: String,
}

/// Where a recipient's copy got to: the furthest step, or how it ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Status {
    Delivered,
    Opened,
    Clicked,
    Converted,
    Unsubscribed,
    Bounced,
}

impl Status {
    /// Every status, in the order a reader looks for them.
    pub const ALL: [Status; 6] = [
        Status::Delivered,
        Status::Opened,
        Status::Clicked,
        Status::Converted,
        Status::Unsubscribed,
        Status::Bounced,
    ];

    pub const fn key(self) -> &'static str {
        match self {
            Self::Delivered => "delivered",
            Self::Opened => "opened",
            Self::Clicked => "clicked",
            Self::Converted => "converted",
            Self::Unsubscribed => "unsubscribed",
            Self::Bounced => "bounced",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Delivered => "Delivered",
            Self::Opened => "Opened",
            Self::Clicked => "Clicked",
            Self::Converted => "Converted",
            Self::Unsubscribed => "Unsubscribed",
            Self::Bounced => "Bounced",
        }
    }

    /// The colour a status is drawn in: green for the goal, amber and red for the
    /// ways a copy is lost.
    pub fn tone(self) -> Tone {
        match self {
            Self::Delivered => Tone::Neutral,
            Self::Opened => Tone::Info,
            Self::Clicked => Tone::named("violet"),
            Self::Converted => Tone::Success,
            Self::Unsubscribed => Tone::Warning,
            Self::Bounced => Tone::Danger,
        }
    }

    pub fn parse(key: &str) -> Option<Status> {
        Self::ALL.into_iter().find(|status| status.key() == key)
    }
}

/// What happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Sent,
    Delivered,
    /// A mail privacy proxy fetched the email's images on the reader's behalf: an
    /// "open" that says nothing about the reader, so it is set aside.
    ProxyOpen,
    Opened,
    Clicked,
    Converted,
    Unsubscribed,
    Bounced,
}

impl Kind {
    pub const fn key(self) -> &'static str {
        match self {
            Self::Sent => "sent",
            Self::Delivered => "delivered",
            Self::ProxyOpen => "proxy_open",
            Self::Opened => "opened",
            Self::Clicked => "clicked",
            Self::Converted => "converted",
            Self::Unsubscribed => "unsubscribed",
            Self::Bounced => "bounced",
        }
    }

    pub fn parse(key: &str) -> Kind {
        [
            Self::Sent,
            Self::Delivered,
            Self::ProxyOpen,
            Self::Opened,
            Self::Clicked,
            Self::Converted,
            Self::Unsubscribed,
            Self::Bounced,
        ]
        .into_iter()
        .find(|kind| kind.key() == key)
        .unwrap_or(Self::Sent)
    }

    pub fn tone(self) -> Tone {
        match self {
            Self::Sent | Self::Delivered | Self::ProxyOpen => Tone::Neutral,
            Self::Opened => Tone::Info,
            Self::Clicked => Tone::named("violet"),
            Self::Converted => Tone::Success,
            Self::Unsubscribed => Tone::Warning,
            Self::Bounced => Tone::Danger,
        }
    }

    pub fn icon(self) -> icons::Icon {
        match self {
            Self::Sent => icons::SEND,
            Self::Delivered => icons::MAIL_CHECK,
            Self::ProxyOpen => icons::SHIELD,
            Self::Opened => icons::MAIL_OPEN,
            Self::Clicked => icons::MOUSE_POINTER_CLICK,
            Self::Converted => icons::BADGE_CHECK,
            Self::Unsubscribed => icons::BELL_OFF,
            Self::Bounced => icons::MAIL_X,
        }
    }

    /// Whether a reader did it, rather than a mail server or a proxy.
    pub fn by_reader(self) -> bool {
        matches!(
            self,
            Self::Opened | Self::Clicked | Self::Converted | Self::Unsubscribed
        )
    }
}

/// A place a recipient reads from.
pub struct Place {
    pub country: &'static str,
    pub city: &'static str,
}

const fn place(country: &'static str, city: &'static str) -> Place {
    Place { country, city }
}

pub const PLACES: [Place; 28] = [
    place("GB", "London"),
    place("DE", "Berlin"),
    place("FR", "Lyon"),
    place("ES", "Madrid"),
    place("PT", "Lisbon"),
    place("NL", "Utrecht"),
    place("SE", "Stockholm"),
    place("IE", "Dublin"),
    place("IT", "Milan"),
    place("PL", "Kraków"),
    place("US", "Austin"),
    place("US", "Seattle"),
    place("CA", "Toronto"),
    place("MX", "Mexico City"),
    place("BR", "São Paulo"),
    place("AR", "Buenos Aires"),
    place("IN", "Bengaluru"),
    place("IN", "Pune"),
    place("JP", "Osaka"),
    place("KR", "Seoul"),
    place("SG", "Singapore"),
    place("AU", "Melbourne"),
    place("NZ", "Wellington"),
    place("ZA", "Cape Town"),
    place("KE", "Nairobi"),
    place("NG", "Lagos"),
    place("AE", "Dubai"),
    place("GB", "Manchester"),
];

/// A device and the mail app on it.
pub struct Device {
    pub device: &'static str,
    pub os: &'static str,
    pub client: &'static str,
    /// Apple's mail apps fetch images through a privacy proxy.
    pub proxied: bool,
    pub kind: &'static str,
}

const fn device(
    device: &'static str,
    os: &'static str,
    client: &'static str,
    proxied: bool,
    kind: &'static str,
) -> Device {
    Device {
        device,
        os,
        client,
        proxied,
        kind,
    }
}

pub const DEVICES: [Device; 10] = [
    device("iPhone 16", "iOS 26", "Apple Mail", true, "Phone"),
    device("Pixel 9", "Android 16", "Gmail", false, "Phone"),
    device("MacBook Air", "macOS 26", "Apple Mail", true, "Laptop"),
    device("ThinkPad X1", "Windows 11", "Outlook", false, "Laptop"),
    device("Galaxy S25", "Android 16", "Samsung Email", false, "Phone"),
    device("Surface Laptop", "Windows 11", "Edge", false, "Laptop"),
    device("MacBook Pro", "macOS 26", "Chrome", false, "Laptop"),
    device("iPad Air", "iPadOS 26", "Apple Mail", true, "Tablet"),
    device(
        "Desktop PC",
        "Ubuntu 26.04",
        "Thunderbird",
        false,
        "Desktop",
    ),
    device("iPhone 15", "iOS 26", "Gmail", false, "Phone"),
];

const FIRST: [&str; 40] = [
    "Maya", "Tomás", "Aiko", "Lena", "Ravi", "Claire", "Noor", "Dev", "Sofia", "Kwame", "Ingrid",
    "Mateo", "Priya", "Hana", "Oliver", "Amara", "Lukas", "Yuki", "Zara", "Diego", "Freya",
    "Arjun", "Chloé", "Emeka", "Nina", "Kenji", "Leila", "Marco", "Ana", "Felix", "Imani", "Jonas",
    "Mei", "Rafael", "Saoirse", "Tariq", "Elena", "Hugo", "Inês", "Kofi",
];

const LAST: [&str; 40] = [
    "Patel",
    "García",
    "Tanaka",
    "Fischer",
    "Menon",
    "Dubois",
    "Haddad",
    "Kapoor",
    "Rossi",
    "Mensah",
    "Larsen",
    "Silva",
    "Nair",
    "Kim",
    "Bennett",
    "Okafor",
    "Weber",
    "Sato",
    "Ahmed",
    "Morales",
    "Lindqvist",
    "Rao",
    "Martin",
    "Eze",
    "Novak",
    "Ito",
    "Rahimi",
    "Bianchi",
    "Costa",
    "Wagner",
    "Mwangi",
    "Berg",
    "Chen",
    "Alves",
    "Murphy",
    "Aziz",
    "Popescu",
    "Laurent",
    "Ferreira",
    "Boateng",
];

const COMPANIES: [&str; 20] = [
    "Northfield Studio",
    "Bluefin Logistics",
    "Copperleaf Health",
    "Quillon Labs",
    "Saltmarsh Foods",
    "Ridgeway Partners",
    "Lumen Architects",
    "Tidewell Energy",
    "Brightfork",
    "Oakhaven Schools",
    "Meridian Freight",
    "Pinecrest Clinic",
    "Silverline Media",
    "Driftwood Travel",
    "Kitebird",
    "Stonebridge Legal",
    "Harvest & Vine",
    "Cobalt Robotics",
    "Wrenfield",
    "Amberly Retail",
];

const ROLES: [&str; 12] = [
    "Product manager",
    "Head of operations",
    "Engineering lead",
    "Founder",
    "Design lead",
    "Marketing manager",
    "Project coordinator",
    "Customer success lead",
    "CTO",
    "Studio manager",
    "Data analyst",
    "Office manager",
];

/// How each recipient joined the list.
pub const SOURCES: [&str; 6] = [
    "Trial sign-up",
    "Webinar",
    "Product tour",
    "Newsletter form",
    "Partner referral",
    "Conference",
];

const NETWORKS: [&str; 4] = [
    "Home broadband",
    "Mobile data",
    "Office network",
    "Public Wi-Fi",
];

const DOMAINS: [&str; 3] = ["example.com", "example.org", "example.net"];

/// A small, fast generator: the same numbers from the same seed on every run.
#[derive(Clone, Debug)]
pub struct Dice(u64);

impl Dice {
    pub fn new(seed: u64) -> Self {
        Self(seed ^ 0x9e37_79b9_7f4a_7c15)
    }

    /// The next number (SplitMix64).
    pub fn draw(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A number in `0..n`.
    pub fn below(&mut self, n: u64) -> u64 {
        self.draw() % n.max(1)
    }

    /// True `percent` times in a hundred.
    pub fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }

    /// A number between `low` and `high`, weighted towards `low`: most people act
    /// soon, a few much later.
    pub fn soon(&mut self, low: i64, high: i64) -> i64 {
        let span = u64::try_from((high - low).max(1)).unwrap_or(1);
        let a = self.below(span);
        let b = self.below(span);
        let c = self.below(span);
        low + i64::try_from(a.min(b).min(c)).unwrap_or(0)
    }
}

/// `Tomás` as `tomas`, for an address.
fn ascii(word: &str) -> String {
    word.chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            other => other.to_ascii_lowercase(),
        })
        .filter(char::is_ascii_alphanumeric)
        .collect()
}

/// A valid number in `country`, made from its example mobile number.
fn phone_in(country: &str, dice: &mut Dice) -> String {
    let Some(region) = Region::parse(country) else {
        return String::new();
    };
    let Some(example) = region.example(NumberType::Mobile) else {
        return String::new();
    };
    let text = example.as_str().to_owned();
    let keep = text.len().saturating_sub(3);
    for _ in 0..8 {
        let tail = format!("{:03}", dice.below(1000));
        let candidate = format!("{}{tail}", &text[..keep]);
        if let Ok(number) = PhoneNumber::parse(&candidate, Some(region)) {
            return number.as_str().to_owned();
        }
    }
    text
}

/// The date of a unix time, in the campaign's zone.
pub fn day_of(at: i64) -> Option<NaiveDate> {
    DateTime::from_timestamp(at + i64::from(OFFSET), 0).map(|time| time.date_naive())
}

/// A network address from the ranges kept for documentation.
fn ip(dice: &mut Dice) -> String {
    let nets = ["192.0.2", "198.51.100", "203.0.113"];
    format!(
        "{}.{}",
        nets[usize::try_from(dice.below(3)).unwrap_or(0)],
        1 + dice.below(253)
    )
}

/// One recipient and the events of their copy.
pub struct Seeded {
    pub recipient: Recipient,
    pub events: Vec<Event>,
}

/// Everything the campaign holds when the app starts: the recipients and their
/// events, the campaign sent `SENT_AGO` before `now`. The same `now` writes the
/// same campaign.
pub fn seed(now: i64) -> (Vec<Recipient>, Vec<Event>) {
    let start = now - SENT_AGO;
    let mut recipients = Vec::with_capacity(usize::try_from(RECIPIENTS).unwrap_or(0));
    let mut events = Vec::new();
    for id in 1..=RECIPIENTS {
        let seeded = one(id, start, now);
        recipients.push(seeded.recipient);
        events.extend(seeded.events);
    }
    events.sort_by_key(|event| (event.at, event.recipient_id, event.kind.clone()));
    for (at, event) in events.iter_mut().enumerate() {
        event.id = i64::try_from(at).unwrap_or(0) + 1;
    }
    (recipients, events)
}

fn one(id: i64, start: i64, now: i64) -> Seeded {
    let n = usize::try_from(id - 1).unwrap_or(0);
    let mut dice = Dice::new(u64::try_from(id).unwrap_or(0) * 7_919);
    let (first, last) = (FIRST[n % 40], LAST[(n / 40 + n) % 40]);
    let place = &PLACES[usize::try_from(dice.below(PLACES.len() as u64)).unwrap_or(0)];
    let device = &DEVICES[usize::try_from(dice.below(DEVICES.len() as u64)).unwrap_or(0)];
    let domain = DOMAINS[n % 3];
    let mut recipient = Recipient {
        id,
        name: format!("{first} {last}"),
        email: format!("{}.{}@{domain}", ascii(first), ascii(last)),
        role: ROLES[usize::try_from(dice.below(12)).unwrap_or(0)].to_owned(),
        company: COMPANIES[usize::try_from(dice.below(20)).unwrap_or(0)].to_owned(),
        country: place.country.to_owned(),
        city: place.city.to_owned(),
        device: device.device.to_owned(),
        os: device.os.to_owned(),
        client: device.client.to_owned(),
        network: NETWORKS[usize::try_from(dice.below(4)).unwrap_or(0)].to_owned(),
        ip: ip(&mut dice),
        source: SOURCES[usize::try_from(dice.below(6)).unwrap_or(0)].to_owned(),
        phone: String::new(),
        customer_since: NaiveDate::from_ymd_opt(2022, 1, 10)
            .map(|day| day + chrono::Days::new(dice.below(4 * 365 + 200))),
        status: Status::Delivered.key().to_owned(),
        opens: 0,
        clicks: 0,
        last_activity: None,
    };
    recipient.phone = phone_in(place.country, &mut dice);
    let address = recipient.ip.clone();

    let mut events = Vec::new();
    let mut push = |kind: Kind, at: i64, detail: &str, link: &str, reader: bool| {
        events.push(Event {
            id: 0,
            recipient_id: id,
            kind: kind.key().to_owned(),
            at,
            device: if reader { device.device } else { "" }.to_owned(),
            os: if reader { device.os } else { "" }.to_owned(),
            client: if reader { device.client } else { "" }.to_owned(),
            country: if reader { place.country } else { "" }.to_owned(),
            city: if reader { place.city } else { "" }.to_owned(),
            ip: if reader {
                address.clone()
            } else {
                String::new()
            },
            link: link.to_owned(),
            detail: detail.to_owned(),
        });
    };

    let sent = start + (id - 1) * 2;
    push(Kind::Sent, sent, SENDER, "", false);
    let mut status = Status::Delivered;
    let mut last;
    if dice.chance(3) {
        let bounced = sent + 2 + dice.soon(0, 40);
        push(
            Kind::Bounced,
            bounced,
            "550 5.1.1 The mailbox does not exist",
            "",
            false,
        );
        status = Status::Bounced;
        last = bounced;
    } else {
        let delivered = sent + 1 + dice.soon(0, 6);
        push(
            Kind::Delivered,
            delivered,
            "250 2.0.0 Accepted by the recipient's mail server",
            "",
            false,
        );
        last = delivered;
        if device.proxied {
            let fetched = delivered + 20 + dice.soon(0, 240);
            push(
                Kind::ProxyOpen,
                fetched,
                "Images fetched by a mail privacy proxy",
                "",
                false,
            );
            // A proxy fetches the images again when the mailbox syncs, at any hour.
            if dice.chance(45) {
                let again = delivered + 2 * 3600 + dice.below(27 * 3600) as i64;
                push(
                    Kind::ProxyOpen,
                    again,
                    "Images fetched again by a mail privacy proxy",
                    "",
                    false,
                );
            }
        }
        if dice.chance(60) {
            let opened = delivered + 40 + dice.soon(0, 26 * 3600);
            push(Kind::Opened, opened, "", "", true);
            recipient.opens = 1;
            status = Status::Opened;
            last = opened;
            if dice.chance(38) {
                let link = LINKS[usize::try_from(dice.below(3)).unwrap_or(0)];
                let clicked = opened + 6 + dice.soon(0, 360);
                push(Kind::Clicked, clicked, link.0, link.1, true);
                recipient.clicks = 1;
                status = Status::Clicked;
                last = clicked;
                if dice.chance(26) {
                    let converted = clicked + 40 + dice.soon(0, 1500);
                    push(
                        Kind::Converted,
                        converted,
                        "Started a 14-day Pro trial",
                        "",
                        true,
                    );
                    status = Status::Converted;
                    last = converted;
                }
            } else if dice.chance(7) {
                let gone = opened + 15 + dice.soon(0, 120);
                push(
                    Kind::Unsubscribed,
                    gone,
                    "From the link in the footer",
                    "",
                    true,
                );
                status = Status::Unsubscribed;
                last = gone;
            }
            if status != Status::Unsubscribed && dice.chance(22) {
                let again = opened + 3 * 3600 + dice.soon(0, 18 * 3600);
                if again < now - 60 {
                    push(Kind::Opened, again, "", "", true);
                    recipient.opens += 1;
                    last = last.max(again);
                }
            }
        }
    }
    events.retain(|event| event.at <= now - 30);
    recipient.status = status.key().to_owned();
    recipient.last_activity = day_of(last.min(now));
    Seeded { recipient, events }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_793_793_600;

    #[test]
    fn the_seed_is_the_same_on_every_run() {
        let (a, b) = seed(NOW);
        let (c, d) = seed(NOW);
        assert_eq!(a.len(), c.len());
        assert_eq!(b.len(), d.len());
        assert_eq!(a[17].name, c[17].name);
        assert_eq!(b[200].at, d[200].at);
    }

    #[test]
    fn every_recipient_is_invented_and_reachable() {
        let (recipients, _) = seed(NOW);
        assert_eq!(recipients.len(), 1_240);
        let mut emails: Vec<&str> = recipients.iter().map(|r| r.email.as_str()).collect();
        emails.sort_unstable();
        emails.dedup();
        assert_eq!(emails.len(), 1_240, "one address each");
        for recipient in &recipients {
            assert!(
                ["@example.com", "@example.org", "@example.net"]
                    .iter()
                    .any(|domain| recipient.email.ends_with(domain)),
                "{}",
                recipient.email
            );
            assert!(Status::parse(&recipient.status).is_some());
        }
    }

    #[test]
    fn the_numbers_look_like_a_campaign() {
        let (recipients, events) = seed(NOW);
        let count = |status: Status| {
            recipients
                .iter()
                .filter(|r| r.status == status.key())
                .count()
        };
        let bounced = count(Status::Bounced);
        let converted = count(Status::Converted);
        assert!((10..80).contains(&bounced), "{bounced}");
        assert!((30..150).contains(&converted), "{converted}");
        assert!(events.iter().all(|event| event.at <= NOW));
        assert!(events.windows(2).all(|pair| pair[0].at <= pair[1].at));
        assert!(
            events
                .iter()
                .any(|event| event.kind == Kind::ProxyOpen.key())
        );
    }

    #[test]
    fn phone_numbers_are_valid_in_their_country() {
        let (recipients, _) = seed(NOW);
        for recipient in recipients.iter().take(120) {
            if recipient.phone.is_empty() {
                continue;
            }
            let region = Region::parse(&recipient.country);
            let number = PhoneNumber::parse(&recipient.phone, region);
            assert!(number.is_ok(), "{} {}", recipient.country, recipient.phone);
        }
    }
}
