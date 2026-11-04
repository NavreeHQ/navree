//! What the app stores: the support team, the customers, their conversations and
//! everything said in them, and a seed that writes the same desk on every run.
//!
//! Every name, company and address here is invented. Addresses are on
//! `example.com`, `example.org` and `example.net`.

use chrono::{DateTime, NaiveDate};
use navree::orm::Model;
use navree::ui::{Tone, icons};

/// The company whose support desk this is: a software company in Mohali.
pub const COMPANY: &str = "Sukhna Labs";
/// What it sells: booking software for clinics, studios and academies.
pub const PRODUCT: &str = "Sukhna Bookings";
/// The desk's own time zone, which every page shows its times in.
pub const ZONE: &str = "Asia/Kolkata";
/// India Standard Time's offset from UTC, in seconds: the same all year.
pub const OFFSET: i32 = 19_800;
/// The agent a visitor works as until they choose another.
pub const DEFAULT_AGENT: i64 = 1;

/// Someone on the support team.
#[derive(Model, Clone, Debug, Default, serde::Serialize)]
#[model(table = "agents")]
pub struct Agent {
    pub id: i64,
    pub name: String,
    pub email: String,
    pub team: String,
    pub active: bool,
}

/// A customer: a person at a business that books with the product.
#[derive(Model, Clone, Debug, Default, serde::Serialize)]
#[model(table = "customers")]
pub struct Customer {
    pub id: i64,
    pub name: String,
    pub email: String,
    /// A mobile number, made up: `+91 98765 4` and four digits.
    pub phone: String,
    pub company: String,
    pub role: String,
    /// One of [`Plan`], as its key.
    pub plan: String,
    /// Everything they have paid, in paise.
    pub lifetime_paise: i64,
    pub seats: i64,
    /// A two-letter country code.
    pub country: String,
    pub city: String,
    pub since: Option<NaiveDate>,
    /// Comma separated.
    pub tags: String,
}

/// A conversation with a customer.
#[derive(Model, Clone, Debug, Default, serde::Serialize)]
#[model(table = "conversations")]
pub struct Conversation {
    pub id: i64,
    pub customer_id: i64,
    pub subject: String,
    /// One of [`Channel`], as its key.
    pub channel: String,
    /// One of [`Status`], as its key.
    pub status: String,
    /// One of [`Priority`], as its key.
    pub priority: String,
    pub assignee_id: Option<i64>,
    /// Unix seconds.
    pub created_at: i64,
    /// When anything last happened.
    pub last_at: i64,
    /// When an agent must answer by: set by the customer's latest message, cleared by
    /// an answer. Unix seconds.
    pub sla_due: Option<i64>,
    /// When the clock started, to say how much of it is left.
    pub sla_from: Option<i64>,
    pub snoozed_until: Option<NaiveDate>,
    /// Customer messages no agent has opened yet.
    pub unread: i64,
    /// The latest message, short, for the list.
    pub preview: String,
    /// The desk's revision when it last changed: a live list asks what changed since.
    pub rev: i64,
    /// How many messages it holds.
    pub messages: i64,
    /// Which list it is in: `open` (open or pending), `snoozed` or `closed`.
    pub inbox: String,
}

impl Conversation {
    /// Where it sorts in a list, newest activity first: its last activity and its id,
    /// padded so the text sorts as the numbers do.
    pub fn cursor(&self) -> String {
        format!("{:012}{:012}", self.last_at.max(0), self.id.max(0))
    }
}

/// A conversation's cursor read back: its last activity and its id.
pub fn read_cursor(cursor: &str) -> Option<(i64, i64)> {
    if cursor.len() != 24 || !cursor.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((cursor[..12].parse().ok()?, cursor[12..].parse().ok()?))
}

/// A message's cursor: its id, padded.
pub fn message_cursor(id: i64) -> String {
    format!("{:012}", id.max(0))
}

/// A message's cursor read back.
pub fn read_message_cursor(cursor: &str) -> Option<i64> {
    if cursor.is_empty() || cursor.len() > 12 || !cursor.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    cursor.parse().ok()
}

/// The list a status puts a conversation in.
pub fn inbox_of(status: &str) -> &'static str {
    match Status::parse(status) {
        Some(Status::Snoozed) => "snoozed",
        Some(Status::Closed) => "closed",
        _ => "open",
    }
}

/// One thing said, or done, in a conversation.
#[derive(Model, Clone, Debug, Default, serde::Serialize)]
#[model(table = "messages")]
pub struct Message {
    pub id: i64,
    pub conversation_id: i64,
    /// One of [`Author`], as its key.
    pub kind: String,
    /// Who: the customer's name or the agent's.
    pub author: String,
    pub agent_id: Option<i64>,
    pub body: String,
    /// Unix seconds.
    pub at: i64,
}

/// A file sent with a message.
#[derive(Model, Clone, Debug, Default, serde::Serialize)]
#[model(table = "attachments")]
pub struct Attachment {
    pub id: i64,
    pub message_id: i64,
    pub conversation_id: i64,
    pub name: String,
    pub bytes: i64,
}

/// Something in a customer's history.
#[derive(Model, Clone, Debug, Default, serde::Serialize)]
#[model(table = "customer_events")]
pub struct CustomerEvent {
    pub id: i64,
    pub customer_id: i64,
    /// One of [`EventKind`], as its key.
    pub kind: String,
    pub at: i64,
    pub title: String,
    pub detail: String,
}

/// The sizes of a person's panes, kept as text.
#[derive(Model, Clone, Debug, Default, serde::Serialize)]
#[model(table = "layouts")]
pub struct Layout {
    pub id: i64,
    pub agent_id: i64,
    pub split: String,
    pub sizes: String,
}

/// Where a conversation stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Waiting for an agent.
    Open,
    /// Waiting for the customer.
    Pending,
    /// Put aside until a day.
    Snoozed,
    Closed,
}

impl Status {
    pub const ALL: [Status; 4] = [Self::Open, Self::Pending, Self::Snoozed, Self::Closed];

    pub const fn key(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Pending => "pending",
            Self::Snoozed => "snoozed",
            Self::Closed => "closed",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::Pending => "Pending",
            Self::Snoozed => "Snoozed",
            Self::Closed => "Closed",
        }
    }

    pub fn tone(self) -> Tone {
        match self {
            Self::Open => Tone::Info,
            Self::Pending => Tone::named("violet"),
            Self::Snoozed => Tone::Warning,
            Self::Closed => Tone::Neutral,
        }
    }

    pub fn icon(self) -> icons::Icon {
        match self {
            Self::Open => icons::CIRCLE_DOT,
            Self::Pending => icons::HOURGLASS,
            Self::Snoozed => icons::ALARM_CLOCK,
            Self::Closed => icons::CIRCLE_CHECK,
        }
    }

    pub fn parse(key: &str) -> Option<Status> {
        Self::ALL.into_iter().find(|status| status.key() == key)
    }
}

/// How soon a conversation needs an answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Urgent,
    High,
    Normal,
    Low,
}

impl Priority {
    pub const ALL: [Priority; 4] = [Self::Urgent, Self::High, Self::Normal, Self::Low];

    pub const fn key(self) -> &'static str {
        match self {
            Self::Urgent => "urgent",
            Self::High => "high",
            Self::Normal => "normal",
            Self::Low => "low",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Urgent => "Urgent",
            Self::High => "High",
            Self::Normal => "Normal",
            Self::Low => "Low",
        }
    }

    pub fn tone(self) -> Tone {
        match self {
            Self::Urgent => Tone::Danger,
            Self::High => Tone::Warning,
            Self::Normal => Tone::Neutral,
            Self::Low => Tone::Neutral,
        }
    }

    pub fn icon(self) -> icons::Icon {
        match self {
            Self::Urgent => icons::SIREN,
            Self::High => icons::SIGNAL_HIGH,
            Self::Normal => icons::SIGNAL_MEDIUM,
            Self::Low => icons::SIGNAL_LOW,
        }
    }

    /// How long an agent has to answer a customer's message.
    pub const fn target(self) -> i64 {
        match self {
            Self::Urgent => 60 * 60,
            Self::High => 4 * 3600,
            Self::Normal => 12 * 3600,
            Self::Low => 24 * 3600,
        }
    }

    pub fn parse(key: &str) -> Option<Priority> {
        Self::ALL.into_iter().find(|priority| priority.key() == key)
    }
}

/// Where a conversation came in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Chat,
    HelpCentre,
    Phone,
    InApp,
    Api,
}

impl Channel {
    pub const ALL: [Channel; 5] = [
        Self::Chat,
        Self::HelpCentre,
        Self::Phone,
        Self::InApp,
        Self::Api,
    ];

    pub const fn key(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::HelpCentre => "help-centre",
            Self::Phone => "phone",
            Self::InApp => "in-app",
            Self::Api => "api",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Chat => "Chat",
            Self::HelpCentre => "Help centre",
            Self::Phone => "Phone",
            Self::InApp => "In the app",
            Self::Api => "Developer API",
        }
    }

    pub fn icon(self) -> icons::Icon {
        match self {
            Self::Chat => icons::MESSAGES_SQUARE,
            Self::HelpCentre => icons::LIFE_BUOY,
            Self::Phone => icons::PHONE,
            Self::InApp => icons::APP_WINDOW,
            Self::Api => icons::CODE_XML,
        }
    }

    pub fn parse(key: &str) -> Option<Channel> {
        Self::ALL.into_iter().find(|channel| channel.key() == key)
    }
}

/// Who wrote a message, or what kind of line it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Author {
    Customer,
    Agent,
    /// An internal note: the team sees it, the customer doesn't.
    Note,
    /// Something that happened: assigned, closed, snoozed.
    Event,
}

impl Author {
    pub const fn key(self) -> &'static str {
        match self {
            Self::Customer => "customer",
            Self::Agent => "agent",
            Self::Note => "note",
            Self::Event => "event",
        }
    }

    pub fn parse(key: &str) -> Author {
        match key {
            "agent" => Self::Agent,
            "note" => Self::Note,
            "event" => Self::Event,
            _ => Self::Customer,
        }
    }
}

/// A customer's plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plan {
    Starter,
    Growth,
    Scale,
    Enterprise,
}

impl Plan {
    pub const ALL: [Plan; 4] = [Self::Starter, Self::Growth, Self::Scale, Self::Enterprise];

    pub const fn key(self) -> &'static str {
        match self {
            Self::Starter => "starter",
            Self::Growth => "growth",
            Self::Scale => "scale",
            Self::Enterprise => "enterprise",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Starter => "Starter",
            Self::Growth => "Growth",
            Self::Scale => "Scale",
            Self::Enterprise => "Enterprise",
        }
    }

    /// The monthly price per seat, in paise: ₹499, ₹999, ₹1,999 and ₹3,499.
    pub const fn seat_paise(self) -> i64 {
        match self {
            Self::Starter => 49_900,
            Self::Growth => 99_900,
            Self::Scale => 199_900,
            Self::Enterprise => 349_900,
        }
    }

    pub fn parse(key: &str) -> Plan {
        Self::ALL
            .into_iter()
            .find(|plan| plan.key() == key)
            .unwrap_or(Self::Starter)
    }
}

/// What happened in a customer's history.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    SignedUp,
    PlanChanged,
    PaymentFailed,
    InvoicePaid,
    SeatsAdded,
    TicketOpened,
    TicketClosed,
    Survey,
}

impl EventKind {
    pub const fn key(self) -> &'static str {
        match self {
            Self::SignedUp => "signed_up",
            Self::PlanChanged => "plan_changed",
            Self::PaymentFailed => "payment_failed",
            Self::InvoicePaid => "invoice_paid",
            Self::SeatsAdded => "seats_added",
            Self::TicketOpened => "ticket_opened",
            Self::TicketClosed => "ticket_closed",
            Self::Survey => "survey",
        }
    }

    pub fn parse(key: &str) -> EventKind {
        [
            Self::SignedUp,
            Self::PlanChanged,
            Self::PaymentFailed,
            Self::InvoicePaid,
            Self::SeatsAdded,
            Self::TicketOpened,
            Self::TicketClosed,
            Self::Survey,
        ]
        .into_iter()
        .find(|kind| kind.key() == key)
        .unwrap_or(Self::TicketOpened)
    }

    pub fn tone(self) -> Tone {
        match self {
            Self::SignedUp | Self::InvoicePaid => Tone::Success,
            Self::PlanChanged | Self::SeatsAdded => Tone::Info,
            Self::PaymentFailed => Tone::Danger,
            Self::TicketOpened => Tone::Warning,
            Self::TicketClosed => Tone::Neutral,
            Self::Survey => Tone::named("violet"),
        }
    }

    pub fn icon(self) -> icons::Icon {
        match self {
            Self::SignedUp => icons::SPARKLES,
            Self::PlanChanged => icons::ARROW_UP_RIGHT,
            Self::PaymentFailed => icons::CREDIT_CARD,
            Self::InvoicePaid => icons::RECEIPT,
            Self::SeatsAdded => icons::USER_PLUS,
            Self::TicketOpened => icons::MESSAGE_SQUARE_PLUS,
            Self::TicketClosed => icons::MESSAGE_SQUARE_X,
            Self::Survey => icons::STAR,
        }
    }
}

/// A reply an agent can drop into the composer.
pub struct SavedReply {
    pub key: &'static str,
    pub title: &'static str,
    pub body: &'static str,
}

pub const SAVED: [SavedReply; 6] = [
    SavedReply {
        key: "looking",
        title: "Looking into it",
        body: "Thanks for the detail. I'm looking into this now and will write back within the hour with what I find.",
    },
    SavedReply {
        key: "screenshots",
        title: "Ask for screenshots",
        body: "Could you send a screenshot of what you see, and the time it happened? That lets me find the exact request in our logs.",
    },
    SavedReply {
        key: "refund",
        title: "Refund issued",
        body: "I've refunded the charge in full. It shows on your statement in 5 to 10 working days, depending on your bank.",
    },
    SavedReply {
        key: "escalated",
        title: "Passed to engineering",
        body: "I've passed this to our engineers with everything you sent. I'll keep this conversation open and tell you as soon as they have a fix.",
    },
    SavedReply {
        key: "limits",
        title: "API limits explained",
        body: "Each API key may make 600 requests a minute. A 429 answer carries a Retry-After header: wait that many seconds and try again.",
    },
    SavedReply {
        key: "resolved",
        title: "Closing: resolved",
        body: "I'm glad that's working again. I'll close this conversation now; reply here any time and it opens again.",
    },
];

/// The saved reply called `key`.
pub fn saved(key: &str) -> Option<&'static SavedReply> {
    SAVED.iter().find(|reply| reply.key == key)
}

/// The day of a moment in the desk's zone.
pub fn day_of(at: i64) -> Option<NaiveDate> {
    DateTime::from_timestamp(at + i64::from(OFFSET), 0).map(|time| time.date_naive())
}

/// Where a conversation's clock stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sla {
    /// Seconds left; below zero when it is past due.
    pub left: i64,
    /// How long the clock ran in all.
    pub span: i64,
}

impl Sla {
    /// The clock of `conversation` at `now`, while an answer is owed.
    pub fn of(conversation: &Conversation, now: i64) -> Option<Sla> {
        if !matches!(Status::parse(&conversation.status), Some(Status::Open)) {
            return None;
        }
        let due = conversation.sla_due?;
        let from = conversation.sla_from.unwrap_or(due - 3600);
        Some(Sla {
            left: due - now,
            span: (due - from).max(60),
        })
    }

    /// Past due.
    pub fn breached(self) -> bool {
        self.left <= 0
    }

    /// Close to due: under 15 minutes, or under a fifth of the time it had.
    pub fn at_risk(self) -> bool {
        !self.breached() && (self.left < 15 * 60 || self.left * 5 < self.span)
    }

    pub fn tone(self) -> Tone {
        if self.breached() {
            Tone::Danger
        } else if self.at_risk() {
            Tone::Warning
        } else {
            Tone::Neutral
        }
    }

    /// `12 min left`, `2 h 5 min left`, `Overdue 4 min`.
    pub fn text(self) -> String {
        let span = short_span(self.left.abs());
        if self.breached() {
            format!("Overdue {span}")
        } else {
            format!("{span} left")
        }
    }
}

/// `40 s`, `12 min`, `2 h 5 min`, `3 d 4 h`: how long, in its two largest units.
pub fn short_span(seconds: i64) -> String {
    let s = seconds.max(0);
    match s {
        0..=59 => "under 1 min".to_owned(),
        60..=3599 => format!("{} min", s / 60),
        3600..=86_399 if (s % 3600) / 60 == 0 => format!("{} h", s / 3600),
        3600..=86_399 => format!("{} h {} min", s / 3600, (s % 3600) / 60),
        _ if (s % 86_400) / 3600 == 0 => format!("{} d", s / 86_400),
        _ => format!("{} d {} h", s / 86_400, (s % 86_400) / 3600),
    }
}

/// The first line of `body`, cut to about 120 characters on a word.
pub fn preview_of(body: &str) -> String {
    let line = body
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim();
    if line.chars().count() <= 120 {
        return line.to_owned();
    }
    let cut: String = line.chars().take(120).collect();
    let end = cut.rfind(' ').unwrap_or(cut.len());
    format!("{}…", cut[..end].trim_end())
}

/// A source of numbers that is the same on every run (SplitMix64).
pub struct Dice(u64);

impl Dice {
    pub fn new(seed: u64) -> Self {
        Self(seed ^ 0x9e37_79b9_7f4a_7c15)
    }

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

    /// An index into a list of `len`.
    pub fn pick(&mut self, len: usize) -> usize {
        usize::try_from(self.below(len as u64)).unwrap_or(0)
    }

    /// True `percent` times in a hundred.
    pub fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }
}

/// The team.
pub const AGENTS: [(&str, &str); 24] = [
    ("Gurpreet Singh", "Customer success"),
    ("Priya Verma", "Billing"),
    ("Harleen Kaur", "Technical"),
    ("Manpreet Gill", "Technical"),
    ("Rohit Malhotra", "Onboarding"),
    ("Simran Sandhu", "Billing"),
    ("Vikram Sethi", "Technical"),
    ("Neha Kapoor", "Customer success"),
    ("Amandeep Brar", "Onboarding"),
    ("Ananya Mehra", "Technical"),
    ("Jaspreet Dhillon", "Billing"),
    ("Kavya Bhatia", "Customer success"),
    ("Aditya Arora", "Technical"),
    ("Ravneet Grewal", "Onboarding"),
    ("Kulwinder Bains", "Technical"),
    ("Pooja Agarwal", "Customer success"),
    ("Siddharth Khanna", "Billing"),
    ("Meera Joshi", "Onboarding"),
    ("Ishaan Gupta", "Technical"),
    ("Harpreet Sidhu", "Customer success"),
    ("Rajveer Mann", "Billing"),
    ("Tanvi Saxena", "Technical"),
    ("Sukhjinder Cheema", "Onboarding"),
    ("Ritika Bansal", "Customer success"),
];

/// Businesses that book with the product: the business, the person who writes in, and
/// the city. All in India.
const BUSINESSES: [(&str, &str, &str); 24] = [
    ("Sukoon Yoga Studio", "Jasleen Kaur", "Chandigarh"),
    ("Doaba Physiotherapy", "Inderjit Randhawa", "Jalandhar"),
    ("Ghuman Dental Care", "Gurleen Ghuman", "Ludhiana"),
    ("Phulkari Hair and Beauty", "Mehak Chawla", "Amritsar"),
    ("Gyan Path Tutorials", "Karan Bhalla", "Patiala"),
    ("Shivalik Fitness Studio", "Baljeet Virk", "Mohali"),
    ("Pashu Seva Vet Clinic", "Sukhpreet Chahal", "Bathinda"),
    ("Kamal Pilates Studio", "Ritika Anand", "Gurugram"),
    ("Rasoi Cooking School", "Varun Sabharwal", "Delhi"),
    ("Surakshit Driving School", "Tejinder Aulakh", "Noida"),
    ("Aaram Ayurveda Centre", "Shreya Mathur", "Jaipur"),
    ("Chaupal Coworking", "Abhishek Srivastava", "Lucknow"),
    ("Tulsi Day Spa", "Nidhi Rawat", "Dehradun"),
    ("Sur Sangam Music Academy", "Gaurav Mishra", "Varanasi"),
    ("Tarang Swimming Academy", "Ankita Tiwari", "Kanpur"),
    ("Akhara Strength Gym", "Rohan Thakur", "Shimla"),
    ("Manthan Counselling Centre", "Sneha Bhardwaj", "Pune"),
    ("Kesh Kala Gents Salon", "Harjot Sekhon", "Mumbai"),
    ("Chitra Photography Studio", "Divya Chaudhary", "Bengaluru"),
    ("Vani Speech Therapy", "Prabhjot Toor", "Chandigarh"),
    ("Veer Martial Arts Academy", "Mandeep Bajwa", "Mohali"),
    ("Ankur Fertility Clinic", "Alka Goyal", "Ludhiana"),
    ("Bhasha Language Institute", "Parminder Sohal", "Amritsar"),
    (
        "Bhangra Beats Dance Academy",
        "Gagandeep Hundal",
        "Jalandhar",
    ),
];

/// The state of each city the desk's customers are in; none for a city that is its
/// own state or territory, such as Delhi and Chandigarh.
const STATES: [(&str, &str); 22] = [
    ("Ludhiana", "Punjab"),
    ("Amritsar", "Punjab"),
    ("Jalandhar", "Punjab"),
    ("Patiala", "Punjab"),
    ("Mohali", "Punjab"),
    ("Bathinda", "Punjab"),
    ("Hoshiarpur", "Punjab"),
    ("Moga", "Punjab"),
    ("Kapurthala", "Punjab"),
    ("Zirakpur", "Punjab"),
    ("Gurugram", "Haryana"),
    ("Panchkula", "Haryana"),
    ("Noida", "Uttar Pradesh"),
    ("Lucknow", "Uttar Pradesh"),
    ("Varanasi", "Uttar Pradesh"),
    ("Kanpur", "Uttar Pradesh"),
    ("Jaipur", "Rajasthan"),
    ("Dehradun", "Uttarakhand"),
    ("Shimla", "Himachal Pradesh"),
    ("Mumbai", "Maharashtra"),
    ("Pune", "Maharashtra"),
    ("Bengaluru", "Karnataka"),
];

/// The state `city` is in, when it is one the desk knows.
pub fn state_of(city: &str) -> Option<&'static str> {
    STATES
        .iter()
        .find(|(name, _)| *name == city)
        .map(|(_, state)| *state)
}

/// A customer's made-up mobile number: `+91 98765 4` and four digits of their id.
pub fn phone_of(id: i64) -> String {
    format!("+91 98765 4{:04}", id.rem_euclid(10_000))
}

const ROLES: [&str; 6] = [
    "Owner",
    "Practice manager",
    "Front desk lead",
    "Operations manager",
    "Studio manager",
    "Office administrator",
];

const TAGS: [&str; 10] = [
    "vip",
    "multi-location",
    "api",
    "trial-converted",
    "annual",
    "at-risk",
    "sso",
    "beta",
    "migrating",
    "partner",
];

/// One line of a scripted conversation.
pub struct Line {
    pub author: Author,
    pub body: &'static str,
    pub file: Option<(&'static str, i64)>,
}

const fn c(body: &'static str) -> Line {
    Line {
        author: Author::Customer,
        body,
        file: None,
    }
}

const fn cf(body: &'static str, name: &'static str, bytes: i64) -> Line {
    Line {
        author: Author::Customer,
        body,
        file: Some((name, bytes)),
    }
}

const fn a(body: &'static str) -> Line {
    Line {
        author: Author::Agent,
        body,
        file: None,
    }
}

const fn n(body: &'static str) -> Line {
    Line {
        author: Author::Note,
        body,
        file: None,
    }
}

/// What a conversation is about, and how it goes.
pub struct Topic {
    pub subject: &'static str,
    pub channel: Channel,
    pub priority: Priority,
    pub team: &'static str,
    pub lines: &'static [Line],
}

pub const TOPICS: [Topic; 16] = [
    Topic {
        subject: "Card declined when we upgrade to Growth",
        channel: Channel::InApp,
        priority: Priority::High,
        team: "Billing",
        lines: &[
            c(
                "Hi, we tried to move from Starter to Growth this morning and the card was declined twice. The bank says nothing was blocked on their side. Can you see what went wrong?",
            ),
            a(
                "Thanks for flagging this. I can see two attempts at 09:12 and 09:14, both answered \"do not honour\" by the card's issuer. That answer usually comes from a fraud check on the bank's side rather than from us.",
            ),
            n(
                "Issuer answered 05 twice. No retries queued. If they try a third card, check the bank's OTP step loads for them.",
            ),
            cf(
                "We called the bank again and they insist it's fine. Here's the screen we get.",
                "upgrade-error.png",
                184_320,
            ),
            a(
                "Thank you, that helps. The screenshot shows the bank's OTP window closed before it finished. Could you try once more in a private window, so no extension blocks the bank's page?",
            ),
            c("That worked! We're on Growth now. Thanks for the patience."),
        ],
    },
    Topic {
        subject: "Reminder texts go out five and a half hours early",
        channel: Channel::Chat,
        priority: Priority::Urgent,
        team: "Technical",
        lines: &[
            c(
                "Since Monday every reminder SMS reaches clients five and a half hours before it should. Two clients called at 5 in the morning about their 10:30 appointment. What changed?",
            ),
            n(
                "Location zone is UTC, not Asia/Kolkata: it was reset when they merged accounts on Monday. Fix is to set the zone in Settings > Locations.",
            ),
            a(
                "I'm sorry about the early messages. Your location's time zone was reset to UTC when the accounts were merged, so reminders went out on UTC time. I've set it to India Standard Time; reminders from now on go at the right time.",
            ),
            c("Thanks. Will the ones already queued for tomorrow be right too?"),
        ],
    },
    Topic {
        subject: "Single sign-on fails with \"invalid audience\" after our identity provider change",
        channel: Channel::HelpCentre,
        priority: Priority::High,
        team: "Technical",
        lines: &[
            cf(
                "We moved to a new identity provider on Friday. Since then nobody can sign in with single sign-on: the error says \"invalid audience\". Metadata attached.",
                "idp-metadata.xml",
                7_168,
            ),
            a(
                "Thanks for the metadata. The audience your new provider sends is the old entity ID. In your provider's app settings, set the audience to the value shown on your Sukhna SSO page, then try again.",
            ),
            c("Updated it, same error. Is there a cache on your side?"),
            n("Their SAML config is cached for 15 min per tenant. Cleared it by hand at 10:40."),
            a(
                "There is, for fifteen minutes. I've cleared it for your account now, so the next sign-in reads the new settings.",
            ),
        ],
    },
    Topic {
        subject: "Can we bring our bookings over from the old system?",
        channel: Channel::Phone,
        priority: Priority::Normal,
        team: "Onboarding",
        lines: &[
            c(
                "We're moving from our old booking tool next month. Can we bring two years of bookings and client records with us?",
            ),
            a(
                "Yes. Send us an export as CSV, one file for clients and one for bookings, and we'll check the columns before anything is imported. Most moves take two working days.",
            ),
            cf(
                "Here are the two files. The bookings one is big.",
                "bookings-2024-2026.csv",
                9_437_184,
            ),
            n("41k rows. Dates are day-first; set the importer to DMY."),
        ],
    },
    Topic {
        subject: "Webhook deliveries answered 410 since Friday",
        channel: Channel::Api,
        priority: Priority::High,
        team: "Technical",
        lines: &[
            c(
                "Our booking.created webhook gets nothing since Friday. Your delivery log shows 410 Gone from our endpoint, but the endpoint is up.",
            ),
            a(
                "A 410 makes us stop sending to that address, as the docs describe. It came from your server at 17:02 on Friday, perhaps during a deploy. I've switched deliveries back on; nothing is lost, because events wait for 72 hours.",
            ),
            c("Ah, the deploy. Thank you. Can we replay what was missed?"),
        ],
    },
    Topic {
        subject: "Refund for a double charge on invoice INV-20418",
        channel: Channel::HelpCentre,
        priority: Priority::Normal,
        team: "Billing",
        lines: &[
            cf(
                "We were charged twice for October. Invoice attached.",
                "INV-20418.pdf",
                96_256,
            ),
            n(
                "Two captures 4 s apart on 1 Oct; the gateway retried after a timeout. Second capture to refund.",
            ),
            a(
                "You're right, and I'm sorry. A timeout made our payment provider take the payment twice. I've refunded the second charge in full.",
            ),
        ],
    },
    Topic {
        subject: "How do we add a second location?",
        channel: Channel::Chat,
        priority: Priority::Low,
        team: "Onboarding",
        lines: &[
            c(
                "We're opening a second studio in March. Can one account run both, with separate calendars?",
            ),
            a(
                "It can. Under Settings > Locations, add the new address. Each location gets its own calendar, rooms and opening hours, and staff can work at one or both.",
            ),
            c("Perfect. Does the second location cost extra?"),
        ],
    },
    Topic {
        subject: "429 answers from the availability endpoint",
        channel: Channel::Api,
        priority: Priority::Normal,
        team: "Technical",
        lines: &[
            c(
                "Our booking widget gets 429 Too Many Requests from /v2/availability at busy times. We only have about 40 visitors at once.",
            ),
            n(
                "Their widget polls every 2 s per visitor: 40 visitors = 1,200/min, double the limit.",
            ),
            a(
                "Your widget asks for availability every two seconds for each visitor, which is about 1,200 requests a minute at busy times. Asking when a visitor picks a day, rather than on a timer, keeps you far under the limit.",
            ),
        ],
    },
    Topic {
        subject: "Export every client record as a spreadsheet",
        channel: Channel::HelpCentre,
        priority: Priority::Low,
        team: "Customer success",
        lines: &[
            c(
                "Our accountant needs a list of all clients with their total spend this year. Is there an export?",
            ),
            a(
                "There is: Reports > Clients, set the dates to this year, then Export. The file has each client's bookings and spend.",
            ),
        ],
    },
    Topic {
        subject: "Staff can see each other's schedules",
        channel: Channel::InApp,
        priority: Priority::Normal,
        team: "Customer success",
        lines: &[
            c(
                "Our therapists can see each other's appointments, including client names. For privacy we'd like each person to see only their own. Possible?",
            ),
            a(
                "Yes. Under Settings > Roles, edit the Therapist role and set Calendar to \"Own bookings only\". It takes effect the next time each person loads the calendar.",
            ),
            c("Done, thank you. Can managers still see everything?"),
        ],
    },
    Topic {
        subject: "Booking page shows prices in the wrong currency",
        channel: Channel::Chat,
        priority: Priority::High,
        team: "Billing",
        lines: &[
            c(
                "Clients see prices in dollars on our booking page, but we charge in rupees. Card and UPI payments are in rupees, so people are confused.",
            ),
            n(
                "Location currency is INR but the booking page theme has a hard-coded $ from an old template.",
            ),
            a(
                "The prices are charged in rupees; the dollar sign comes from your page's old theme, which writes it as text. I've switched you to the current theme, which shows each location's currency.",
            ),
        ],
    },
    Topic {
        subject: "Cancel our subscription at the end of the term",
        channel: Channel::Phone,
        priority: Priority::Normal,
        team: "Customer success",
        lines: &[
            c("We're closing the clinic in April and need to cancel. Our term ends on 31 March."),
            n("Long-standing customer. Offer the pause option before cancelling."),
            a(
                "I'm sorry to hear about the clinic. I can set the subscription to end on 31 March, so you keep everything until then. If you might reopen, we can also pause the account and keep your records for free for a year.",
            ),
        ],
    },
    Topic {
        subject: "Waiting list for classes that are full",
        channel: Channel::InApp,
        priority: Priority::Low,
        team: "Customer success",
        lines: &[
            c(
                "Our evening classes fill up within an hour. Could clients join a waiting list and get the place when someone cancels?",
            ),
            a(
                "That's a good idea, and many studios have asked for it. I've added your vote to the request our product team is working through, and I'll let you know when it's planned.",
            ),
        ],
    },
    Topic {
        subject: "Our invoices need our GSTIN",
        channel: Channel::HelpCentre,
        priority: Priority::Low,
        team: "Billing",
        lines: &[
            c(
                "Can our GSTIN be printed on the invoices you send us? Our accountant needs it to claim the input tax credit.",
            ),
            a(
                "Of course. Add it under Billing > Company details and every invoice from then on carries it. Would you like me to issue the last three again with it?",
            ),
            c("Yes please, that would save a lot of back and forth."),
        ],
    },
    Topic {
        subject: "Deposit wasn't taken for a no-show",
        channel: Channel::Chat,
        priority: Priority::Normal,
        team: "Billing",
        lines: &[
            c(
                "A client didn't turn up yesterday and the no-show fee wasn't charged, though our policy is on.",
            ),
            a(
                "The policy charges the card held for the booking. This booking was made by phone through your front desk, so no card was held. Bookings made online keep a card, and those are charged.",
            ),
            c("Can the front desk take a card on the phone then?"),
        ],
    },
    Topic {
        subject: "Booking widget is blank on our site after a theme update",
        channel: Channel::Chat,
        priority: Priority::Urgent,
        team: "Technical",
        lines: &[
            cf(
                "We updated our website theme and now the booking widget is an empty box. Console error attached. We're losing bookings!",
                "console-log.txt",
                2_048,
            ),
            n(
                "Their new theme sends a Content-Security-Policy without our widget's origin in frame-src.",
            ),
            a(
                "I can see why: your new theme sends a content security policy that doesn't allow our widget's address. Add book.example.net to frame-src in your theme's settings and it shows again.",
            ),
        ],
    },
];

/// The seed: the same desk on every run.
pub struct Seed {
    pub agents: Vec<Agent>,
    pub customers: Vec<Customer>,
    pub conversations: Vec<Conversation>,
    pub messages: Vec<Message>,
    pub attachments: Vec<Attachment>,
    pub events: Vec<CustomerEvent>,
}

/// An address on one of the documentation domains.
fn address(name: &str, at: usize) -> String {
    let local: String = name
        .to_lowercase()
        .chars()
        .map(|ch| match ch {
            'á' | 'à' | 'â' | 'ã' => 'a',
            'é' | 'è' | 'ê' => 'e',
            'í' => 'i',
            'ó' | 'ô' => 'o',
            'ú' => 'u',
            'ñ' => 'n',
            'ç' => 'c',
            'ł' => 'l',
            'ń' => 'n',
            'ś' => 's',
            'ź' | 'ż' => 'z',
            ' ' => '.',
            other => other,
        })
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '.')
        .collect();
    let domain = ["example.com", "example.org", "example.net"][at % 3];
    format!("{local}@{domain}")
}

/// Writes the desk as it stands at `now`.
pub fn seed(now: i64) -> Seed {
    let mut dice = Dice::new(2_611);
    let today = day_of(now).unwrap_or_default();
    let agents: Vec<Agent> = AGENTS
        .iter()
        .enumerate()
        .map(|(at, (name, team))| Agent {
            id: at as i64 + 1,
            name: (*name).to_owned(),
            email: address(name, 0).replace("@example.com", "@support.example.com"),
            team: (*team).to_owned(),
            // Two people are away, and can't be given work.
            active: !matches!(at, 21 | 23),
        })
        .collect();

    let mut customers = Vec::with_capacity(BUSINESSES.len());
    let mut events = Vec::new();
    for (at, (company, person, city)) in BUSINESSES.iter().enumerate() {
        let name = (*person).to_owned();
        let plan = Plan::ALL[(at * 5 + 1) % 4];
        let seats = match plan {
            Plan::Starter => 2 + dice.below(3) as i64,
            Plan::Growth => 4 + dice.below(6) as i64,
            Plan::Scale => 10 + dice.below(16) as i64,
            Plan::Enterprise => 30 + dice.below(50) as i64,
        };
        let months = 3 + dice.below(40) as i64;
        let since = today
            .checked_sub_signed(chrono::Duration::days(months * 30))
            .unwrap_or(today);
        let lifetime = plan.seat_paise() * seats * months;
        let mut tags: Vec<&str> = Vec::new();
        for _ in 0..1 + dice.below(3) {
            let tag = TAGS[dice.pick(TAGS.len())];
            if !tags.contains(&tag) {
                tags.push(tag);
            }
        }
        let id = at as i64 + 1;
        customers.push(Customer {
            id,
            name: name.clone(),
            email: address(&name, at),
            phone: phone_of(id),
            company: (*company).to_owned(),
            role: ROLES[at % ROLES.len()].to_owned(),
            plan: plan.key().to_owned(),
            lifetime_paise: lifetime,
            seats,
            country: "IN".to_owned(),
            city: (*city).to_owned(),
            since: Some(since),
            tags: tags.join(","),
        });
        // Their history: from signing up to now.
        let start = now - months * 30 * 86_400;
        let mut history = vec![(
            EventKind::SignedUp,
            start,
            "Signed up".to_owned(),
            format!("Started a 14-day trial with {} seats", seats.min(3)),
        )];
        history.push((
            EventKind::PlanChanged,
            start + 13 * 86_400,
            format!("Moved to {}", plan.label()),
            format!("{} seats, paid monthly", seats),
        ));
        if dice.chance(50) {
            history.push((
                EventKind::SeatsAdded,
                start + (months / 2).max(1) * 30 * 86_400,
                "Added seats".to_owned(),
                format!("From {} to {} seats", (seats - 2).max(1), seats),
            ));
        }
        if dice.chance(30) {
            history.push((
                EventKind::PaymentFailed,
                now - (5 + dice.below(40) as i64) * 86_400,
                "A payment failed".to_owned(),
                "Card expired; paid on the second try two days later".to_owned(),
            ));
        }
        history.push((
            EventKind::InvoicePaid,
            now - (1 + dice.below(25) as i64) * 86_400,
            "Paid the monthly invoice".to_owned(),
            format!("{} by UPI AutoPay", rupees(plan.seat_paise() * seats)),
        ));
        if dice.chance(45) {
            history.push((
                EventKind::Survey,
                now - (2 + dice.below(60) as i64) * 86_400,
                "Answered a survey".to_owned(),
                format!("Rated support {} out of 5", 4 + dice.below(2)),
            ));
        }
        for (kind, at, title, detail) in history {
            events.push(CustomerEvent {
                id: 0,
                customer_id: id,
                kind: kind.key().to_owned(),
                at,
                title,
                detail,
            });
        }
    }

    let mut conversations = Vec::new();
    let mut messages: Vec<Message> = Vec::new();
    let mut attachments = Vec::new();
    // Each topic is told a few times, to different customers, at different stages.
    let mut next_message = 1_i64;
    for round in 0..3_usize {
        for (t, topic) in TOPICS.iter().enumerate() {
            if round == 2 && t % 2 == 1 {
                continue;
            }
            let id = conversations.len() as i64 + 1;
            let customer = &customers[(t * 3 + round * 7) % customers.len()];
            // Newer rounds are more recent: round 0 is today, round 2 days back.
            let age = match round {
                0 => 8 * 60 + dice.below(6 * 3600) as i64,
                1 => 6 * 3600 + dice.below(40 * 3600) as i64,
                _ => 2 * 86_400 + dice.below(4 * 86_400) as i64,
            };
            let created = now - age;
            // How far the story has got: at least the first line.
            let told = 1 + dice.pick(topic.lines.len());
            let lines = &topic.lines[..told];
            let gap = (age / (told as i64 + 1)).max(60);
            let team: Vec<&Agent> = agents
                .iter()
                .filter(|agent| agent.team == topic.team && agent.active)
                .collect();
            let assignee = if dice.chance(78) {
                // Some go to the agent a visitor works as, so "Mine" is never empty.
                if dice.chance(30) {
                    agents.first()
                } else {
                    team.get(dice.pick(team.len())).copied()
                }
            } else {
                None
            };
            let mut at = created;
            let mut last_customer = created;
            let mut last_agent: Option<i64> = None;
            let mut last_body = String::new();
            for (k, line) in lines.iter().enumerate() {
                if k > 0 {
                    at += gap / 2 + dice.below(u64::try_from(gap).unwrap_or(60)) as i64 / 2;
                }
                at = at.min(now - 60);
                let (author, agent_id) = match line.author {
                    Author::Customer => (customer.name.clone(), None),
                    _ => match assignee {
                        Some(agent) => (agent.name.clone(), Some(agent.id)),
                        None => (agents[1].name.clone(), Some(agents[1].id)),
                    },
                };
                messages.push(Message {
                    id: next_message,
                    conversation_id: id,
                    kind: line.author.key().to_owned(),
                    author,
                    agent_id,
                    body: line.body.to_owned(),
                    at,
                });
                if let Some((name, bytes)) = line.file {
                    attachments.push(Attachment {
                        id: attachments.len() as i64 + 1,
                        message_id: next_message,
                        conversation_id: id,
                        name: name.to_owned(),
                        bytes,
                    });
                }
                next_message += 1;
                match line.author {
                    Author::Customer => last_customer = at,
                    Author::Agent => last_agent = Some(at),
                    _ => {}
                }
                if line.author != Author::Note {
                    last_body = line.body.to_owned();
                }
            }
            let owed = last_agent.is_none_or(|agent| agent < last_customer);
            let finished = told == topic.lines.len();
            let status = if round == 2 && finished {
                Status::Closed
            } else if round == 2 && t % 4 == 0 {
                Status::Snoozed
            } else if owed {
                Status::Open
            } else {
                Status::Pending
            };
            let target = topic.priority.target();
            // The clock runs from the customer's last word; a few are made close to due, or
            // past it, so the list shows every state.
            let (sla_from, sla_due) = if status == Status::Open {
                let due = match (round, t % 5) {
                    (0, 0) => now + 9 * 60,
                    (0, 2) => now - 6 * 60,
                    _ => last_customer + target,
                };
                (Some(due - target), Some(due))
            } else {
                (None, None)
            };
            let unread = if status == Status::Open && round == 0 && t % 3 != 1 {
                1 + i64::from(t % 4 == 0)
            } else {
                0
            };
            conversations.push(Conversation {
                rev: 1,
                messages: i64::try_from(lines.len()).unwrap_or(0),
                inbox: inbox_of(status.key()).to_owned(),
                id,
                customer_id: customer.id,
                subject: topic.subject.to_owned(),
                channel: topic.channel.key().to_owned(),
                status: status.key().to_owned(),
                priority: topic.priority.key().to_owned(),
                assignee_id: if status == Status::Open && round == 0 && t % 5 == 3 {
                    None
                } else {
                    assignee.map(|agent| agent.id)
                },
                created_at: created,
                last_at: at,
                sla_due,
                sla_from,
                snoozed_until: (status == Status::Snoozed)
                    .then(|| today.checked_add_signed(chrono::Duration::days(3)))
                    .flatten(),
                unread,
                preview: preview_of(&last_body),
            });
            events.push(CustomerEvent {
                id: 0,
                customer_id: customer.id,
                kind: EventKind::TicketOpened.key().to_owned(),
                at: created,
                title: format!("Wrote in: {}", topic.subject),
                detail: format!("Through {}", topic.channel.label().to_lowercase()),
            });
            if status == Status::Closed {
                events.push(CustomerEvent {
                    id: 0,
                    customer_id: customer.id,
                    kind: EventKind::TicketClosed.key().to_owned(),
                    at: at + 300,
                    title: format!("Resolved: {}", topic.subject),
                    detail: "Closed by the team".to_owned(),
                });
            }
        }
    }
    events.sort_by_key(|event| (event.customer_id, event.at));
    for (at, event) in events.iter_mut().enumerate() {
        event.id = at as i64 + 1;
    }
    Seed {
        agents,
        customers,
        conversations,
        messages,
        attachments,
        events,
    }
}

/// `1,17,56,640`: digits grouped the Indian way, three then twos (lakh, crore).
pub fn indian_digits(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 2 + 1);
    let head = digits.len().saturating_sub(3);
    for (at, digit) in digits[..head].chars().enumerate() {
        if at > 0 && (head - at).is_multiple_of(2) {
            out.push(',');
        }
        out.push(digit);
    }
    if head > 0 {
        out.push(',');
    }
    out.push_str(&digits[head..]);
    if n < 0 { format!("-{out}") } else { out }
}

/// `₹1,25,000` for 1,25,00,000 paise: whole rupees, grouped the Indian way.
pub fn rupees(paise: i64) -> String {
    format!("₹{}", indian_digits(paise / 100))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_793_793_600;

    #[test]
    fn the_seed_is_the_same_every_time_and_holds_together() {
        let one = seed(NOW);
        let two = seed(NOW);
        assert_eq!(one.conversations.len(), two.conversations.len());
        assert_eq!(one.messages.len(), two.messages.len());
        assert!(one.conversations.len() >= 40, "{}", one.conversations.len());
        for conversation in &one.conversations {
            assert!(
                one.customers
                    .iter()
                    .any(|c| c.id == conversation.customer_id)
            );
            assert!(Status::parse(&conversation.status).is_some());
            assert!(Priority::parse(&conversation.priority).is_some());
            assert!(Channel::parse(&conversation.channel).is_some());
            assert!(
                one.messages
                    .iter()
                    .any(|m| m.conversation_id == conversation.id)
            );
            if let Some(agent) = conversation.assignee_id {
                assert!(one.agents.iter().any(|a| a.id == agent && a.active));
            }
        }
        for message in &one.messages {
            assert!(message.at <= NOW, "nothing is said in the future");
        }
        // Every state is in the list: open, pending, snoozed, closed, unassigned, unread,
        // a clock close to due and one past it.
        let has = |status: Status| one.conversations.iter().any(|c| c.status == status.key());
        assert!(Status::ALL.into_iter().all(has));
        assert!(
            one.conversations
                .iter()
                .any(|c| c.assignee_id.is_none() && c.status == "open")
        );
        assert!(one.conversations.iter().any(|c| c.unread > 0));
        assert!(
            one.conversations
                .iter()
                .any(|c| c.assignee_id == Some(DEFAULT_AGENT))
        );
        let clocks: Vec<Sla> = one
            .conversations
            .iter()
            .filter_map(|c| Sla::of(c, NOW))
            .collect();
        assert!(clocks.iter().any(|sla| sla.breached()));
        assert!(clocks.iter().any(|sla| sla.at_risk()));
        assert!(clocks.iter().any(|sla| !sla.at_risk() && !sla.breached()));
        // Addresses are on the documentation domains; every customer is in India.
        for customer in &one.customers {
            assert_eq!(customer.country, "IN");
            assert!(
                customer.phone.starts_with("+91 98765 4"),
                "{}",
                customer.phone
            );
            assert!(
                customer.email.ends_with(".example.com")
                    || customer.email.ends_with("@example.com")
                    || customer.email.ends_with("@example.org")
                    || customer.email.ends_with("@example.net"),
                "{}",
                customer.email
            );
        }
    }

    #[test]
    fn a_clock_says_how_long_is_left() {
        let mut conversation = Conversation {
            status: "open".to_owned(),
            sla_due: Some(NOW + 600),
            sla_from: Some(NOW - 3000),
            ..Conversation::default()
        };
        let sla = Sla::of(&conversation, NOW).unwrap();
        assert_eq!(sla.text(), "10 min left");
        assert!(sla.at_risk());
        conversation.sla_due = Some(NOW - 4 * 60);
        assert_eq!(Sla::of(&conversation, NOW).unwrap().text(), "Overdue 4 min");
        conversation.sla_due = Some(NOW + 7_500);
        conversation.sla_from = Some(NOW - 600);
        let sla = Sla::of(&conversation, NOW).unwrap();
        assert_eq!(sla.text(), "2 h 5 min left");
        assert!(!sla.at_risk());
        conversation.status = "pending".to_owned();
        assert_eq!(
            Sla::of(&conversation, NOW),
            None,
            "the clock stops while the customer owes the answer"
        );
    }

    #[test]
    fn previews_are_one_line_cut_on_a_word() {
        assert_eq!(preview_of("\n  Hello there\nSecond"), "Hello there");
        let long = "word ".repeat(40);
        let cut = preview_of(&long);
        assert!(cut.ends_with('…') && cut.chars().count() <= 121, "{cut}");
        assert_eq!(rupees(49_900), "₹499");
        assert_eq!(rupees(12_500_000), "₹1,25,000");
        assert_eq!(rupees(1_175_664_000), "₹1,17,56,640");
        assert_eq!(indian_digits(999), "999");
        assert_eq!(indian_digits(1_000), "1,000");
        assert_eq!(indian_digits(100_000), "1,00,000");
        assert_eq!(indian_digits(10_000_000), "1,00,00,000");
        assert_eq!(phone_of(7), "+91 98765 40007");
        assert_eq!(state_of("Ludhiana"), Some("Punjab"));
        assert_eq!(state_of("Delhi"), None);
        assert_eq!(short_span(30), "under 1 min");
        assert_eq!(short_span(90_000), "1 d 1 h");
    }
}
