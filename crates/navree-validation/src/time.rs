//! [`Validate`] for navree-time's types (feature `time`): a `Zone` or a
//! `LocalDay` is valid once it deserializes, so a field of either type can
//! be `nested` (or `skip`ped); a `DayRange` must not end before it starts,
//! so `Valid<Query<DayRange>>` takes a report window from a request.

use navree_time::{DayRange, LocalDay, Zone};

use crate::{FieldPath, Validate, Violation, Violations};

/// Parsing already checked the name.
impl Validate for Zone {
    fn validate_at(&self, _path: &FieldPath<'_>, _out: &mut Violations) {}
}

/// Parsing already checked the day.
impl Validate for LocalDay {
    fn validate_at(&self, _path: &FieldPath<'_>, _out: &mut Violations) {}
}

/// `to` is not before `from` (`to must not be before from`, on `to`).
impl Validate for DayRange {
    fn validate_at(&self, path: &FieldPath<'_>, out: &mut Violations) {
        if self.to < self.from {
            out.push(
                &path.field("to"),
                Violation::text(":attribute must not be before from"),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_range_must_not_end_before_it_starts() {
        let day = |text: &str| text.parse::<LocalDay>().unwrap();
        assert!(DayRange::single(day("2026-10-01")).validate().is_ok());
        let backwards = DayRange {
            from: day("2026-10-31"),
            to: day("2026-10-01"),
        };
        let violations = backwards.validate().unwrap_err();
        assert_eq!(violations.to_string(), "to must not be before from");
        assert!(Zone::parse("Europe/London").unwrap().validate().is_ok());
    }
}
