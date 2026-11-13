//! What the pages draw, as plain functions of the data: the app's handlers call
//! them with rows from the database, and the website's previews call them with the
//! seed, so both draw the same thing.

use chrono::NaiveDate;
use navree::ui::*;

use crate::data::{PROJECT, Person, TEAM, person_icon};

pub mod board;
pub mod card;

/// `node` for a browser, and nothing for a native client: a part whose native form is
/// another part, or that only a keyboard reaches.
pub fn web_only(node: impl Into<Node>) -> Node {
    #[cfg(feature = "native-preview")]
    {
        navree::ui::native::web_only(node)
    }
    #[cfg(not(feature = "native-preview"))]
    {
        node.into()
    }
}

/// The app's one shell.
pub fn shell() -> Shell {
    Shell::new(TEAM)
        .context("Product")
        .group(
            NavGroup::new(PROJECT)
                .item(NavItem::new("Board", "/").icon(icons::KANBAN))
                .item(NavItem::new("Due this week", "/?due=week").icon(icons::CALENDAR_CLOCK))
                .item(NavItem::new("Bugs", "/?label=bug").icon(icons::BUG)),
        )
        .user(
            UserMenu::new("Rangla Looms product team", "team@example.com")
                .action("Sign out", "/sign-out"),
        )
}

/// How a due date reads beside today, and the tone it is drawn in: late is red,
/// today and tomorrow amber.
pub fn due_label(due: NaiveDate, today: NaiveDate) -> (String, Tone) {
    let days = (due - today).num_days();
    match days {
        ..=-2 => (format!("{} days late", -days), Tone::Danger),
        -1 => ("1 day late".to_owned(), Tone::Danger),
        0 => ("Due today".to_owned(), Tone::Warning),
        1 => ("Due tomorrow".to_owned(), Tone::Warning),
        _ => (due.format("Due %-d %b").to_string(), Tone::Neutral),
    }
}

/// A person as a picture: their icon and their name. Never letters.
pub fn avatar(person: &Person) -> Node {
    Row::gap(2)
        .align(Align::Center)
        .children((
            Avatar::placeholder(person.name.clone()).size(Size::Sm),
            Text::new(person.name.clone()),
        ))
        .into()
}

/// The person with `id`, if they are on the team.
pub fn person(people: &[Person], id: Option<i64>) -> Option<&Person> {
    id.and_then(|id| people.iter().find(|person| person.id == id))
}

/// `Tue 3 Nov, 19:35`, in India's time.
pub fn moment(at: i64) -> String {
    chrono::DateTime::from_timestamp(at, 0)
        .map(|time| {
            time.with_timezone(&crate::data::ist())
                .format("%a %-d %b, %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

/// The icon of someone's role, beside their name in a list of people.
pub fn role_icon(person: &Person) -> icons::Icon {
    person_icon(person)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn due_dates_read_from_today() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 4).unwrap_or_default();
        let at = |days: i64| today + chrono::Duration::days(days);
        assert_eq!(
            due_label(at(-3), today),
            ("3 days late".to_owned(), Tone::Danger)
        );
        assert_eq!(
            due_label(at(-1), today),
            ("1 day late".to_owned(), Tone::Danger)
        );
        assert_eq!(
            due_label(today, today),
            ("Due today".to_owned(), Tone::Warning)
        );
        assert_eq!(
            due_label(at(1), today),
            ("Due tomorrow".to_owned(), Tone::Warning)
        );
        assert_eq!(
            due_label(at(5), today),
            ("Due 9 Nov".to_owned(), Tone::Neutral)
        );
    }
}
