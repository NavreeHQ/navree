//! Which times of day can be chosen.

use std::fmt;

use chrono::{NaiveTime, Timelike};
use serde_json::{Value, json};

use super::locale::DateLocale;

/// Why a time can't be chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TimeError {
    /// Before the earliest time.
    TooEarly(NaiveTime),
    /// After the latest time.
    TooLate(NaiveTime),
    /// In a span that is turned off.
    Unavailable,
    /// Not on a multiple of the step, where the step is strict.
    OffStep,
    /// Not a time of day the picker posts: a leap second (`16:59:60`) or a fraction of a second.
    NotATime,
}

impl TimeError {
    /// The message for a person, in the locale's words.
    pub fn message(&self, locale: &DateLocale) -> String {
        locale.labels.unavailable_time.to_string()
    }
}

impl fmt::Display for TimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooEarly(min) => write!(f, "choose {min} or a later time"),
            Self::TooLate(max) => write!(f, "choose {max} or an earlier time"),
            Self::Unavailable => f.write_str("that time isn't available"),
            Self::OffStep => f.write_str("that time isn't on the step"),
            Self::NotATime => f.write_str("that isn't a time of day the picker takes"),
        }
    }
}

impl std::error::Error for TimeError {}

/// The attributes of a native time field.
pub(crate) struct NativeLimits {
    pub min: Option<String>,
    pub max: Option<String>,
    /// In seconds.
    pub step: Option<u32>,
}

/// The minutes since midnight a step may be: any divisor of a day.
fn valid_step(minutes: u32) -> bool {
    (1..=720).contains(&minutes) && 1440 % minutes == 0
}

fn minute_of(time: NaiveTime) -> u32 {
    time.hour() * 60 + time.minute()
}

/// The times a picker lets a person choose, and the check that holds on the server.
///
/// ```
/// use navree_ui::dates::TimeRules;
/// use chrono::NaiveTime;
///
/// let t = |h, m| NaiveTime::from_hms_opt(h, m, 0).unwrap();
/// let rules = TimeRules::new().min(t(9, 0)).max(t(17, 0)).unavailable(t(12, 0), t(12, 59));
/// assert!(rules.check(t(9, 30)).is_ok());
/// assert!(rules.check(t(12, 30)).is_err());
/// assert!(rules.check(t(17, 30)).is_err());
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimeRules {
    min: Option<NaiveTime>,
    max: Option<NaiveTime>,
    unavailable: Vec<(NaiveTime, NaiveTime)>,
    step: u32,
    strict: bool,
}

impl Default for TimeRules {
    fn default() -> Self {
        Self::new()
    }
}

impl TimeRules {
    /// Every time, in a list at 30 minute steps.
    pub fn new() -> Self {
        Self {
            min: None,
            max: None,
            unavailable: Vec::new(),
            step: 30,
            strict: false,
        }
    }

    /// The earliest time.
    #[must_use]
    pub fn min(mut self, time: NaiveTime) -> Self {
        self.min = Some(time);
        self
    }

    /// The latest time.
    #[must_use]
    pub fn max(mut self, time: NaiveTime) -> Self {
        self.max = Some(time);
        self
    }

    /// Turns off a span of times, both ends included down to the minute: `12:00` to
    /// `12:59` turns off every time until `12:59:59`.
    #[must_use]
    pub fn unavailable(mut self, from: NaiveTime, to: NaiveTime) -> Self {
        self.unavailable.push((from.min(to), from.max(to)));
        self
    }

    /// The minutes between the times of the list. It has to divide a day (1, 2, 5,
    /// 10, 15, 20, 30, 60, 120 and so on up to 12 hours); another number is ignored
    /// and the list keeps its step.
    #[must_use]
    pub fn step_minutes(mut self, minutes: u32) -> Self {
        if valid_step(minutes) {
            self.step = minutes;
        }
        self
    }

    /// Refuses a time that is not on a multiple of the step. Without it a typed time
    /// off the step is accepted, as long as it is otherwise allowed.
    #[must_use]
    pub fn strict_step(mut self) -> Self {
        self.strict = true;
        self
    }

    /// The step in minutes.
    pub fn step(&self) -> u32 {
        self.step
    }

    /// Why `time` can't be chosen, or `Ok`.
    pub fn check(&self, time: NaiveTime) -> Result<(), TimeError> {
        // A picker posts whole seconds. A leap second or a fraction is refused whatever the
        // limits say: chrono reads `16:59:60`, and it sorts before 17:00:00.
        if time.nanosecond() != 0 {
            return Err(TimeError::NotATime);
        }
        if let Some(min) = self.min
            && time < min
        {
            return Err(TimeError::TooEarly(min));
        }
        if let Some(max) = self.max
            && time > max
        {
            return Err(TimeError::TooLate(max));
        }
        let minute = minute_of(time);
        if self
            .unavailable
            .iter()
            .any(|(from, to)| minute_of(*from) <= minute && minute <= minute_of(*to))
        {
            return Err(TimeError::Unavailable);
        }
        if self.strict && (time.second() != 0 || !minute.is_multiple_of(self.step)) {
            return Err(TimeError::OffStep);
        }
        Ok(())
    }

    /// Whether `time` can be chosen.
    pub fn allows(&self, time: NaiveTime) -> bool {
        self.check(time).is_ok()
    }

    /// The times of the list: every multiple of the step in the day that can be chosen.
    pub fn options(&self) -> Vec<NaiveTime> {
        (0..1440)
            .step_by(self.step as usize)
            .filter_map(|minute| NaiveTime::from_hms_opt(minute / 60, minute % 60, 0))
            .filter(|time| self.allows(*time))
            .collect()
    }

    /// What a native `type=time` field is told, so that what it takes is what the server
    /// takes and never less. The native step counts from the field's least time, the server
    /// from midnight, so a strict step puts the least on the step, and a step always comes
    /// with a least time (midnight when the rules have none). Whole seconds (when the
    /// picker takes them, a value has them, or a limit has them) need a step of one second.
    pub(crate) fn native_limits(&self, seconds: bool) -> NativeLimits {
        let has_seconds = |time: Option<NaiveTime>| time.is_some_and(|t| t.second() != 0);
        let whole = seconds || has_seconds(self.min) || has_seconds(self.max);
        let text = |time: NaiveTime| {
            time.format(if whole { "%H:%M:%S" } else { "%H:%M" })
                .to_string()
        };
        let step = if whole {
            Some(1)
        } else if self.strict {
            Some(self.step * 60)
        } else {
            None
        };
        let min = match self.min {
            Some(min) if self.strict && !whole => {
                let aligned = minute_of(min).div_ceil(self.step) * self.step;
                NaiveTime::from_hms_opt(aligned / 60, aligned % 60, 0).map(text)
            }
            Some(min) => Some(text(min)),
            None => None,
        };
        // A step with no least time is counted from the value the field holds, so a stored
        // 9:07 would refuse 9:15 and take 9:22. Whenever there is a step there is a least
        // time to count it from: midnight, where the server counts from, when the rules
        // give none or give one past the last step of the day.
        let min = min.or_else(|| step.map(|_| text(NaiveTime::MIN)));
        NativeLimits {
            min,
            max: self.max.map(text),
            step,
        }
    }

    /// What the browser needs.
    pub(crate) fn client(&self) -> Value {
        let text = |time: NaiveTime| time.format("%H:%M:%S").to_string();
        let mut out = json!({ "st": self.step });
        if let Some(min) = self.min {
            out["min"] = json!(text(min));
        }
        if let Some(max) = self.max {
            out["max"] = json!(text(max));
        }
        if !self.unavailable.is_empty() {
            out["u"] = json!(
                self.unavailable
                    .iter()
                    .map(|(from, to)| json!([minute_of(*from), minute_of(*to)]))
                    .collect::<Vec<_>>()
            );
        }
        if self.strict {
            out["strict"] = json!(1);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    #[test]
    fn every_rule_blocks() {
        let rules = TimeRules::new()
            .min(t(9, 0))
            .max(t(17, 0))
            .unavailable(t(12, 59), t(12, 0))
            .strict_step();
        assert_eq!(rules.check(t(8, 30)), Err(TimeError::TooEarly(t(9, 0))));
        assert_eq!(rules.check(t(17, 30)), Err(TimeError::TooLate(t(17, 0))));
        assert_eq!(rules.check(t(12, 30)), Err(TimeError::Unavailable));
        assert_eq!(rules.check(t(10, 10)), Err(TimeError::OffStep));
        assert_eq!(rules.check(t(10, 30)), Ok(()));
        assert_eq!(rules.check(t(9, 0)), Ok(()));
        assert_eq!(rules.check(t(17, 0)), Ok(()));
        // The end of an unavailable span runs to the end of its minute.
        let rules = TimeRules::new().unavailable(t(5, 30), t(7, 29));
        assert!(!rules.allows(NaiveTime::from_hms_opt(7, 29, 45).unwrap()));
        assert!(rules.allows(t(7, 30)));
        assert!(rules.allows(NaiveTime::from_hms_opt(5, 29, 59).unwrap()));
    }

    #[test]
    fn without_strictness_off_step_times_pass() {
        let rules = TimeRules::new().step_minutes(15);
        assert_eq!(rules.check(t(10, 7)), Ok(()));
    }

    #[test]
    fn steps_must_divide_a_day() {
        for good in [1, 2, 5, 10, 15, 20, 30, 45, 60, 90, 120, 360, 720] {
            assert_eq!(TimeRules::new().step_minutes(good).step(), good);
        }
        for bad in [0, 7, 11, 25, 42, 721, 1440, 100_000] {
            assert_eq!(TimeRules::new().step_minutes(bad).step(), 30, "{bad}");
        }
    }

    #[test]
    fn the_list_has_the_times_that_can_be_chosen() {
        assert_eq!(TimeRules::new().options().len(), 48);
        assert_eq!(TimeRules::new().step_minutes(60).options().len(), 24);
        assert_eq!(TimeRules::new().step_minutes(5).options().len(), 288);
        let rules = TimeRules::new()
            .min(t(9, 0))
            .max(t(10, 0))
            .unavailable(t(9, 30), t(9, 30));
        assert_eq!(rules.options(), vec![t(9, 0), t(10, 0)]);
    }

    #[test]
    fn the_browser_gets_the_rules() {
        let rules = TimeRules::new()
            .min(t(9, 0))
            .unavailable(t(12, 0), t(12, 59))
            .step_minutes(15)
            .strict_step();
        assert_eq!(
            rules.client(),
            json!({"st": 15, "min": "09:00:00", "u": [[720, 779]], "strict": 1})
        );
    }
}
