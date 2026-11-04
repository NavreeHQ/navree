//! What the pages draw, as plain functions of the data: the app's handlers call them
//! with rows from the database, and the website's previews call them with the seed,
//! so both draw the same thing.

use chrono::DateTime;
use navree::ui::flags::CountryCode;
use navree::ui::{NavGroup, NavItem, Shell, UserMenu, icons};

use crate::data::{Agent, COMPANY, OFFSET};

pub mod customer;
pub mod inbox;
pub mod list;
pub mod thread;

/// Which conversations the list shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum View {
    /// Open and pending, everyone's.
    #[default]
    All,
    /// Open and pending, assigned to the agent at the desk.
    Mine,
    /// Open, assigned to nobody.
    Unassigned,
    Snoozed,
    Closed,
}

impl View {
    pub const ALL: [View; 5] = [
        Self::All,
        Self::Mine,
        Self::Unassigned,
        Self::Snoozed,
        Self::Closed,
    ];

    pub const fn key(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Mine => "mine",
            Self::Unassigned => "unassigned",
            Self::Snoozed => "snoozed",
            Self::Closed => "closed",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::All => "All open",
            Self::Mine => "Assigned to me",
            Self::Unassigned => "Unassigned",
            Self::Snoozed => "Snoozed",
            Self::Closed => "Closed",
        }
    }

    pub fn icon(self) -> icons::Icon {
        match self {
            Self::All => icons::INBOX,
            Self::Mine => icons::USER_ROUND,
            Self::Unassigned => icons::USER_X,
            Self::Snoozed => icons::ALARM_CLOCK,
            Self::Closed => icons::CIRCLE_CHECK,
        }
    }

    /// The view called `key`; the default for anything else.
    pub fn parse(key: Option<&str>) -> View {
        Self::ALL
            .into_iter()
            .find(|view| Some(view.key()) == key)
            .unwrap_or_default()
    }
}

/// What the list is narrowed to, as the URL holds it. A value that doesn't read is
/// left out, one at a time.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Filters {
    pub view: View,
    /// Words to find in the subject, the customer's name or their company.
    pub q: String,
    /// A channel's key, or empty.
    pub channel: String,
    /// A priority's key, or empty.
    pub priority: String,
}

impl Filters {
    /// From the query's parts. The search is trimmed and cut to 100 characters.
    pub fn read(
        view: Option<&str>,
        q: Option<&str>,
        channel: Option<&str>,
        priority: Option<&str>,
    ) -> Self {
        let q: String = q.unwrap_or("").trim().chars().take(100).collect();
        let channel = channel
            .and_then(crate::data::Channel::parse)
            .map_or_else(String::new, |c| c.key().to_owned());
        let priority = priority
            .and_then(crate::data::Priority::parse)
            .map_or_else(String::new, |p| p.key().to_owned());
        Self {
            view: View::parse(view),
            q,
            channel,
            priority,
        }
    }

    /// `view=mine&q=refund`: what goes after `?`, empty parts left out, the default view
    /// too.
    pub fn query(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.view != View::All {
            parts.push(format!("view={}", self.view.key()));
        }
        if !self.q.is_empty() {
            parts.push(format!("q={}", encode(&self.q)));
        }
        if !self.channel.is_empty() {
            parts.push(format!("channel={}", self.channel));
        }
        if !self.priority.is_empty() {
            parts.push(format!("priority={}", self.priority));
        }
        parts.join("&")
    }

    /// `path` with this state: `/inbox/7?view=mine`.
    pub fn href(&self, path: &str) -> String {
        let query = self.query();
        if query.is_empty() {
            path.to_owned()
        } else {
            format!("{path}?{query}")
        }
    }

    /// The same with another view, the rest dropped: a fresh look.
    pub fn with_view(view: View) -> Self {
        Self {
            view,
            ..Self::default()
        }
    }

    /// Whether anything narrows the view.
    pub fn narrowed(&self) -> bool {
        !self.q.is_empty() || !self.channel.is_empty() || !self.priority.is_empty()
    }
}

/// Percent-encodes `text` for a query string.
pub fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 3);
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(byte));
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// How many conversations each view holds, for the sidebar.
#[derive(Clone, Copy, Debug, Default)]
pub struct ViewCounts {
    pub all: u64,
    pub mine: u64,
    pub unassigned: u64,
    pub snoozed: u64,
}

/// The app's shell for the agent `me`, with the views and their counts.
pub fn shell(me: &Agent, team: &[Agent], counts: ViewCounts, current: View) -> Shell {
    let item = |view: View, count: Option<u64>| {
        let item = NavItem::new(view.label(), Filters::with_view(view).href("/inbox"))
            .icon(view.icon())
            .current(view == current);
        match count {
            Some(n) => item.count(n.to_string()),
            None => item,
        }
    };
    let mut menu = UserMenu::new(me.name.clone(), me.email.clone());
    // No sign-in in a demo: work as someone else instead.
    for other in team.iter().filter(|other| other.id != me.id && other.active).take(4) {
        menu = menu.action(format!("Work as {}", other.name), format!("/as/{}", other.id));
    }
    Shell::new(COMPANY)
        .context("Support")
        .home("/inbox")
        .group(
            NavGroup::new("Inbox")
                .item(item(View::All, Some(counts.all)))
                .item(item(View::Mine, Some(counts.mine)))
                .item(item(View::Unassigned, Some(counts.unassigned)))
                .item(item(View::Snoozed, Some(counts.snoozed)))
                .item(item(View::Closed, None)),
        )
        .user(menu)
}

/// `1,240`.
pub fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (at, digit) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// When a message was written, for its footer: `Today, 09:41`, `Yesterday, 18:02`,
/// `3 Nov, 09:41`, or `12 Mar 2025, 09:41` in another year.
pub fn written(at: i64, now: i64) -> String {
    let (Some(day), Some(today)) = (crate::data::day_of(at), crate::data::day_of(now)) else {
        return String::new();
    };
    let time = clock(at);
    if day == today {
        format!("Today, {time}")
    } else if today.pred_opt() == Some(day) {
        format!("Yesterday, {time}")
    } else if day.format("%Y").to_string() == today.format("%Y").to_string() {
        format!("{}, {time}", day.format("%-d %b"))
    } else {
        format!("{}, {time}", day.format("%-d %b %Y"))
    }
}

/// `Priya` for `Priya Raman`.
pub fn only_first_name(name: &str) -> String {
    name.split_whitespace().next().unwrap_or(name).to_owned()
}

/// `09:41` in the desk's zone.
pub fn clock(at: i64) -> String {
    DateTime::from_timestamp(at + i64::from(OFFSET), 0)
        .map(|time| time.format("%H:%M").to_string())
        .unwrap_or_default()
}

/// `3 Nov, 09:41` in the desk's zone.
pub fn moment(at: i64) -> String {
    DateTime::from_timestamp(at + i64::from(OFFSET), 0)
        .map(|time| time.format("%-d %b, %H:%M").to_string())
        .unwrap_or_default()
}

/// How long ago, short, for a list: `now`, `4 min`, `2 h`, `3 d`, `12 Oct`.
pub fn ago(at: i64, now: i64) -> String {
    let s = (now - at).max(0);
    match s {
        0..=59 => "now".to_owned(),
        60..=3599 => format!("{} min", s / 60),
        3600..=86_399 => format!("{} h", s / 3600),
        86_400..=604_799 => format!("{} d", s / 86_400),
        _ => DateTime::from_timestamp(at + i64::from(OFFSET), 0)
            .map(|time| time.format("%-d %b").to_string())
            .unwrap_or_default(),
    }
}

/// `Bristol, United Kingdom`.
pub fn place(city: &str, code: &str) -> String {
    let country = CountryCode::parse(code).map_or("Unknown", CountryCode::name);
    if city.is_empty() || city == country {
        country.to_owned()
    } else {
        format!("{city}, {country}")
    }
}

/// `2.4 MB`, `96 KB`, `812 bytes`.
pub fn file_size(bytes: i64) -> String {
    let bytes = bytes.max(0);
    if bytes >= 1_048_576 {
        let tenths = bytes * 10 / 1_048_576;
        format!("{}.{} MB", tenths / 10, tenths % 10)
    } else if bytes >= 1024 {
        format!("{} KB", bytes / 1024)
    } else {
        format!("{bytes} bytes")
    }
}

/// The icon for a file, by its name.
pub fn file_icon(name: &str) -> icons::Icon {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".png") || lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        icons::FILE_IMAGE
    } else if lower.ends_with(".csv") || lower.ends_with(".xlsx") {
        icons::FILE_SPREADSHEET
    } else if lower.ends_with(".xml") || lower.ends_with(".json") || lower.ends_with(".txt") {
        icons::FILE_CODE
    } else {
        icons::FILE_TEXT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_read_what_they_can_and_write_the_rest() {
        let filters = Filters::read(Some("mine"), Some("  card declined "), Some("chat"), Some("nope"));
        assert_eq!(filters.view, View::Mine);
        assert_eq!(filters.query(), "view=mine&q=card+declined&channel=chat");
        assert_eq!(filters.href("/inbox/7"), "/inbox/7?view=mine&q=card+declined&channel=chat");
        let plain = Filters::read(Some("<script>"), None, Some("x"), None);
        assert_eq!(plain, Filters::default());
        assert_eq!(plain.href("/inbox"), "/inbox");
        let long = "é".repeat(500);
        assert_eq!(Filters::read(None, Some(&long), None, None).q.chars().count(), 100);
        assert_eq!(encode("a&b=c/ü"), "a%26b%3Dc%2F%C3%BC");
    }

    #[test]
    fn times_and_sizes_read_the_way_people_say_them() {
        let now = 1_793_793_600;
        assert_eq!(ago(now - 20, now), "now");
        assert_eq!(ago(now - 600, now), "10 min");
        assert_eq!(ago(now - 7_300, now), "2 h");
        assert_eq!(ago(now - 3 * 86_400, now), "3 d");
        assert_eq!(ago(now - 30 * 86_400, now), "5 Oct");
        assert_eq!(file_size(9_437_184), "9.0 MB");
        assert_eq!(file_size(96_256), "94 KB");
        assert_eq!(file_size(812), "812 bytes");
        assert_eq!(place("Bristol", "GB"), "Bristol, United Kingdom");
    }
}
