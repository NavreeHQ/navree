//! Due dates relative to "today" in UTC: the `due` filter of
//! `GET /api/v1/activities`, the dashboard's task counts and the daily
//! reminders all use the same day boundaries.

use chrono::{DateTime, NaiveTime, TimeDelta, Utc};
use crm_core::models::{Activity, ActivityScopes};
use navree::openapi::ToSchema;
use navree::orm::Builder;
use serde::Deserialize;

/// The UTC day containing a given instant: `[start, end)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Today {
    /// Midnight UTC that started the day.
    pub start: DateTime<Utc>,
    /// Midnight UTC that ends it (the start of tomorrow).
    pub end: DateTime<Utc>,
}

impl Today {
    /// The UTC day of `now`.
    pub fn of(now: DateTime<Utc>) -> Self {
        let start = now.date_naive().and_time(NaiveTime::MIN).and_utc();
        Today {
            start,
            end: start + TimeDelta::days(1),
        }
    }
}

/// `filter[due]` on `GET /api/v1/activities`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
#[schema(as = ActivityDueFilter)]
pub enum Due {
    /// Open tasks (`kind = task`, not completed) due before today.
    Overdue,
    /// Due today.
    Today,
    /// Due after today.
    Upcoming,
}

impl Due {
    /// The words `filter[due]` takes.
    pub const WORDS: &'static [&'static str] = &["overdue", "today", "upcoming"];

    /// The window a word of [`Due::WORDS`] names.
    pub fn from_word(word: &str) -> Option<Due> {
        match word {
            "overdue" => Some(Due::Overdue),
            "today" => Some(Due::Today),
            "upcoming" => Some(Due::Upcoming),
            _ => None,
        }
    }

    /// `query` restricted to the activities due in this window of `today`.
    pub fn apply(self, query: Builder<Activity>, today: Today) -> Builder<Activity> {
        match self {
            Due::Overdue => query.open_tasks().where_(Activity::DUE_AT.lt(today.start)),
            Due::Today => query
                .where_(Activity::DUE_AT.gte(today.start))
                .where_(Activity::DUE_AT.lt(today.end)),
            Due::Upcoming => query.where_(Activity::DUE_AT.gte(today.end)),
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use navree::orm::{Dialect, Model};

    use super::*;

    fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, s).unwrap()
    }

    #[test]
    fn today_runs_from_midnight_to_midnight_utc() {
        let expected = Today {
            start: at(2026, 10, 26, 0, 0, 0),
            end: at(2026, 10, 27, 0, 0, 0),
        };
        assert_eq!(Today::of(at(2026, 10, 26, 0, 0, 0)), expected);
        assert_eq!(Today::of(at(2026, 10, 26, 10, 30, 0)), expected);
        assert_eq!(Today::of(at(2026, 10, 26, 23, 59, 59)), expected);
        // Month and year ends.
        assert_eq!(
            Today::of(at(2026, 12, 31, 18, 0, 0)).end,
            at(2027, 1, 1, 0, 0, 0)
        );
    }

    #[test]
    fn due_values_are_lowercase_words() {
        for (text, due) in [
            ("overdue", Due::Overdue),
            ("today", Due::Today),
            ("upcoming", Due::Upcoming),
        ] {
            assert_eq!(serde_json::from_value::<Due>(text.into()).unwrap(), due);
            assert_eq!(Due::from_word(text), Some(due));
            assert!(Due::WORDS.contains(&text));
        }
        assert!(serde_json::from_value::<Due>("Today".into()).is_err());
        assert_eq!(Due::from_word("Today"), None);
        assert_eq!(Due::WORDS.len(), 3);
    }

    #[test]
    fn overdue_means_open_tasks_due_before_today() {
        let today = Today::of(at(2026, 10, 26, 10, 0, 0));
        let sql = Due::Overdue
            .apply(Activity::query(), today)
            .to_sql(Dialect::Sqlite)
            .unwrap()
            .sql;
        for condition in [
            r#""activities"."kind" = ?"#,
            r#""activities"."completed_at" IS NULL"#,
            r#""activities"."due_at" < ?"#,
        ] {
            assert!(sql.contains(condition), "{condition} in {sql}");
        }
    }
}
