//! What the pages draw, as plain functions of the data: the app's handlers call
//! them with rows from the database, and the website's previews call them with
//! the seed, so both draw the same thing.

use chrono::{DateTime, NaiveDate};
use navree::ui::flags::CountryCode;
use navree::ui::{DisplayLabels, NavGroup, NavItem, Shell, UserMenu, icons};

use crate::data::{DEVICES, OFFSET};

pub mod contact;
pub mod dashboard;
pub mod recipient;
pub mod recipients;

/// The app's one shell.
pub fn shell() -> Shell {
    Shell::new("Larkspur")
        .context("Marketing")
        .group(
            NavGroup::new("Spring launch")
                .item(NavItem::new("Overview", "/").icon(icons::LAYOUT_DASHBOARD))
                .item(
                    NavItem::new("Recipients", "/recipients")
                        .icon(icons::USERS)
                        .count("1,240"),
                ),
        )
        .group(
            NavGroup::new("Audience")
                .item(NavItem::new("Edit a contact", "/recipients/1/edit").icon(icons::CONTACT)),
        )
        .user(
            UserMenu::new("Robin Hale", "robin@larkspur.example")
                .link("Profile", "/recipients/1")
                .action("Sign out", "/sign-out"),
        )
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

/// `60.1%` of `whole`; `0%` of nothing.
pub fn percent(part: u64, whole: u64) -> String {
    if whole == 0 {
        return "0%".to_owned();
    }
    let tenths = (part * 1000 + whole / 2) / whole;
    format!("{}.{}%", tenths / 10, tenths % 10)
}

/// `20 s`, `6 min`, `2 h 5 min`, `1 d 3 h`: how long, in its two largest units.
pub fn span(seconds: i64) -> String {
    let s = seconds.max(0);
    match s {
        0..=59 => format!("{s} s"),
        60..=3599 if s % 60 == 0 || s >= 600 => format!("{} min", s / 60),
        60..=3599 => format!("{} min {} s", s / 60, s % 60),
        3600..=86_399 if (s % 3600) / 60 == 0 => format!("{} h", s / 3600),
        3600..=86_399 => format!("{} h {} min", s / 3600, (s % 3600) / 60),
        _ if (s % 86_400) / 3600 == 0 => format!("{} d", s / 86_400),
        _ => format!("{} d {} h", s / 86_400, (s % 86_400) / 3600),
    }
}

/// `09:41` in the campaign's zone.
pub fn clock(at: i64) -> String {
    DateTime::from_timestamp(at + i64::from(OFFSET), 0)
        .map(|time| time.format("%H:%M").to_string())
        .unwrap_or_default()
}

/// `3 Nov, 09:41` in the campaign's zone.
pub fn moment(at: i64) -> String {
    DateTime::from_timestamp(at + i64::from(OFFSET), 0)
        .map(|time| time.format("%-d %b, %H:%M").to_string())
        .unwrap_or_default()
}

/// `Today`, `Yesterday` or `2 Nov`.
pub fn day_label(day: Option<NaiveDate>, today: NaiveDate) -> String {
    match day {
        Some(day) if day == today => "Today".to_owned(),
        Some(day) if today.pred_opt() == Some(day) => "Yesterday".to_owned(),
        Some(day) => day.format("%-d %b").to_string(),
        None => "Never".to_owned(),
    }
}

/// `Portugal` for `PT`.
pub fn country_name(code: &str) -> &'static str {
    CountryCode::parse(code).map_or("Unknown", CountryCode::name)
}

/// `Lisbon, Portugal`; `Singapore` once; the country alone with no city.
pub fn place(city: &str, code: &str) -> String {
    let country = country_name(code);
    if city.is_empty() || city == country {
        country.to_owned()
    } else {
        format!("{city}, {country}")
    }
}

/// The icon of a device: a phone, a tablet, a laptop or a desktop.
pub fn device_icon(device: &str) -> icons::Icon {
    let kind = DEVICES
        .iter()
        .find(|known| known.device == device)
        .map_or("Laptop", |known| known.kind);
    match kind {
        "Phone" => icons::SMARTPHONE,
        "Tablet" => icons::TABLET,
        "Desktop" => icons::MONITOR,
        _ => icons::LAPTOP,
    }
}

/// The words for opens a mail privacy proxy made, which the feed and the
/// timeline set aside.
pub fn proxy_labels() -> DisplayLabels {
    DisplayLabels {
        hidden_one: "1 open came through a mail privacy proxy.".into(),
        hidden_many: "{n} opens came through a mail privacy proxy.".into(),
        ..DisplayLabels::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_read_the_way_people_write_them() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_240), "1,240");
        assert_eq!(thousands(1_234_567), "1,234,567");
        assert_eq!(percent(742, 1_209), "61.4%");
        assert_eq!(percent(1, 0), "0%");
        assert_eq!(span(20), "20 s");
        assert_eq!(span(90), "1 min 30 s");
        assert_eq!(span(720), "12 min");
        assert_eq!(span(7_500), "2 h 5 min");
        assert_eq!(span(90_000), "1 d 1 h");
    }

    #[test]
    fn days_are_named_from_today() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 4).unwrap_or_default();
        assert_eq!(day_label(Some(today), today), "Today");
        assert_eq!(day_label(today.pred_opt(), today), "Yesterday");
        assert_eq!(
            day_label(NaiveDate::from_ymd_opt(2026, 11, 1), today),
            "1 Nov"
        );
    }
}
