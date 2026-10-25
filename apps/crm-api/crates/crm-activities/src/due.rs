//! Due dates relative to "today" in UTC (spec §9.5): the `due` filter of
//! `GET /api/v1/activities`, the dashboard's task counts and the daily
//! reminders all use the same day boundaries.

use chrono::{DateTime, NaiveTime, TimeDelta, Utc};
use crm_core::ActivityKind;
use crm_core::entities::activity;
use sea_orm::{ColumnTrait, Condition};
use serde::Deserialize;
use utoipa::ToSchema;

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

/// `?due=` on `GET /api/v1/activities` (spec §9.5).
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
    /// The SQL condition on `activities` for `today`.
    pub fn condition(self, today: Today) -> Condition {
        let due_at = activity::Column::DueAt;
        match self {
            Due::Overdue => open_tasks().add(due_at.lt(today.start)),
            Due::Today => Condition::all()
                .add(due_at.gte(today.start))
                .add(due_at.lt(today.end)),
            Due::Upcoming => Condition::all().add(due_at.gte(today.end)),
        }
    }
}

/// Tasks that are not completed.
pub fn open_tasks() -> Condition {
    Condition::all()
        .add(activity::Column::Kind.eq(ActivityKind::Task))
        .add(activity::Column::CompletedAt.is_null())
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

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
        }
        assert!(serde_json::from_value::<Due>("Today".into()).is_err());
    }
}
