//! What the pages draw, as plain functions of the data: the app's handlers call
//! them with rows from the database, and the website's previews call them with
//! the seed, so both draw the same thing.

use chrono::{DateTime, NaiveDate};
use navree::ui::flags::CountryCode;
use navree::ui::*;

use crate::data::{OFFSET, SHOP, STAFF};

pub mod dashboard;
pub mod inventory;
pub mod order;
pub mod orders;

/// The app's one shell.
pub fn shell() -> Shell {
    Shell::new(SHOP)
        .context("Store operations")
        .group(
            NavGroup::new("Store")
                .item(NavItem::new("Dashboard", "/").icon(icons::LAYOUT_DASHBOARD))
                .item(NavItem::new("Orders", "/orders").icon(icons::SHOPPING_BAG))
                .item(NavItem::new("Inventory", "/inventory").icon(icons::BOXES)),
        )
        .user(
            UserMenu::new(STAFF, "ines.varga@example.com")
                .link("Profile", "/")
                .action("Sign out", "/sign-out"),
        )
}

/// `1,240`.
pub fn thousands(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    if n < 0 {
        out.push('-');
    }
    for (at, digit) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// `€1,240.50` from cents.
pub fn money(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let cents = cents.unsigned_abs();
    format!(
        "{sign}€{}.{:02}",
        thousands(i64::try_from(cents / 100).unwrap_or(i64::MAX)),
        cents % 100
    )
}

/// `09:41` in the shop's zone.
pub fn clock(at: i64) -> String {
    DateTime::from_timestamp(at + i64::from(OFFSET), 0)
        .map(|time| time.format("%H:%M").to_string())
        .unwrap_or_default()
}

/// `4 Nov, 09:41` in the shop's zone.
pub fn moment(at: i64) -> String {
    DateTime::from_timestamp(at + i64::from(OFFSET), 0)
        .map(|time| time.format("%-d %b, %H:%M").to_string())
        .unwrap_or_default()
}

/// `Today, 09:41`, `Yesterday, 18:02` or `2 Nov, 14:20`.
pub fn when(at: i64, today: NaiveDate) -> String {
    let day = crate::data::day_of(at);
    if day == today {
        format!("Today, {}", clock(at))
    } else if today.pred_opt() == Some(day) {
        format!("Yesterday, {}", clock(at))
    } else {
        moment(at)
    }
}

/// `Portugal` for `PT`.
pub fn country_name(code: &str) -> &'static str {
    CountryCode::parse(code).map_or("Unknown", CountryCode::name)
}

/// `Porto, Portugal`.
pub fn place(city: &str, code: &str) -> String {
    let country = country_name(code);
    if city.is_empty() || city == country {
        country.to_owned()
    } else {
        format!("{city}, {country}")
    }
}

/// A flag with a city beside it, which keeps its words together.
pub fn city(code: &str, city: &str) -> Row {
    Row::gap(2).align(Align::Center).children((
        Flag::country(code).decorative(),
        Text::new(if city.is_empty() {
            country_name(code).to_owned()
        } else {
            city.to_owned()
        })
        .inline(),
    ))
}

/// `1 unit`, `3 units`.
pub fn units(n: i64) -> String {
    if n == 1 {
        "1 unit".to_owned()
    } else {
        format!("{} units", thousands(n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_read_the_way_people_write_them() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(1_240), "1,240");
        assert_eq!(thousands(-1_234_567), "-1,234,567");
        assert_eq!(money(0), "€0.00");
        assert_eq!(money(18_400), "€184.00");
        assert_eq!(money(124_050), "€1,240.50");
        assert_eq!(money(-4_005), "-€40.05");
        assert_eq!(units(1), "1 unit");
    }

    #[test]
    fn times_are_named_from_today() {
        // 4 November 2026, 12:00 in Amsterdam.
        let noon = 1_793_790_000;
        let today = crate::data::day_of(noon);
        assert_eq!(when(noon, today), "Today, 12:00");
        assert_eq!(when(noon - 86_400, today), "Yesterday, 12:00");
        assert_eq!(when(noon - 2 * 86_400, today), "2 Nov, 12:00");
    }
}
