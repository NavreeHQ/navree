//! `DatePicker`, `TimePicker`, `DateTimePicker` and `DateRangePicker`.
//!
//! A date control with scripts is a text box, a button and a calendar in a
//! popup. Under it the page keeps the plain control a form would send: an
//! `<input type="date">` (or `time`, or `datetime-local`) that carries the
//! name, the value and the limits. That is what is read; the calendar, the
//! list of times and the presets are a browser's way to fill it in, and every
//! system has its own.

use std::borrow::Cow;

use navree_native::input as i;
use navree_native::node as n;

use super::super::lower::{
    Control, Cx, N, Parts, Read, attr, elements, fail, has, id, slot, text_of,
};
use super::without;
use crate::node::Element;

/// The hooks the root of a date control carries that this reader answers for.
pub(crate) const HOOKS: &[&str] = &["data-ui-dates", "data-ui-loc", "data-ui-trules"];

/// The hooks a date control's plain input carries that mark it as the one
/// the script fills in.
const NATIVE: &[&str] = &["data-ui-native", "data-ui-joint"];

/// A date, a time, or both: one field.
pub(crate) fn picker(cx: &mut Cx, element: &Element) -> Read {
    field(cx, element).map(|field| vec![N::Field(field)])
}

/// The field of a date, a time or a date and a time.
pub(crate) fn field(cx: &mut Cx, element: &Element) -> Read<n::Field> {
    let slot_name = slot(element);
    let ui = match slot_name.as_deref() {
        Some("date-picker") => "date-picker-ui",
        Some("time-picker") => "time-picker-ui",
        Some("date-time-picker") => "date-time-picker-ui",
        _ => return fail("a date control that is none of the three"),
    };
    let mut parts = Parts::of(element);
    let native = parts.slot("input");
    let popup = parts.slot(ui);
    let note = parts.slot("field-description");
    parts.done()?;
    let Some(native) = native else {
        return fail("a date control with no date field under it");
    };
    if popup.is_none() {
        return fail("a date control with nothing to choose by");
    }
    let off = match popup {
        Some(popup) => rules(cx, popup)?,
        None => Off::default(),
    };
    let mut field = plain(cx, native)?;
    switched_off(&mut field, &off);
    field.id = id(element).or(field.id);
    field.help = note.map(text_of);
    Ok(field)
}

/// The plain input of a date control, as a field.
fn plain(cx: &mut Cx, native: &Element) -> Read<n::Field> {
    if has(native, "readonly") {
        return fail("a date that can be read but not changed");
    }
    let marked = without(native, NATIVE);
    let Control::Field(field) = cx.control(&marked)? else {
        return fail("a date control whose input is not a field");
    };
    if !matches!(field.input, navree_native::input::Input::Date(_)) {
        return fail("a date control whose input is not a date");
    }
    Ok(*field)
}

/// The days a calendar switches off.
#[derive(Default)]
struct Off {
    days: Vec<String>,
    weekdays: Vec<i::Weekday>,
}

/// The most days a field is told of one by one; a calendar that switches off more
/// than a year of them says so in `dropped`.
const MOST_DAYS: usize = 366;

/// The days a calendar switches off: the weekdays, and the runs of days it was told
/// of around the month it shows. The limits (`min` and `max`) stand on the field.
fn rules(cx: &mut Cx, popup: &Element) -> Read<Off> {
    let mut off = Off::default();
    let Some(calendar) = crate::native::page::find(popup.child_nodes(), "calendar") else {
        return Ok(off);
    };
    let Some(json) = attr(calendar, "data-ui-rules") else {
        return Ok(off);
    };
    let Ok(rules) = serde_json::from_str::<serde_json::Value>(&json) else {
        return fail("rules for a calendar that are not readable");
    };
    // Sunday is 0, as a script counts the days.
    for day in rules["wd"].as_array().into_iter().flatten() {
        off.weekdays.push(match day.as_u64() {
            Some(0) => i::Weekday::Sunday,
            Some(1) => i::Weekday::Monday,
            Some(2) => i::Weekday::Tuesday,
            Some(3) => i::Weekday::Wednesday,
            Some(4) => i::Weekday::Thursday,
            Some(5) => i::Weekday::Friday,
            Some(6) => i::Weekday::Saturday,
            _ => return fail("a calendar that switches off a day of the week it cannot name"),
        });
    }
    for run in rules["r"].as_array().into_iter().flatten() {
        let day = |at: usize| {
            run.get(at)
                .and_then(serde_json::Value::as_str)
                .and_then(|text| chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").ok())
        };
        let (Some(from), Some(to)) = (day(0), day(1)) else {
            return fail("a calendar that switches off days it cannot name");
        };
        let mut at = from;
        while at <= to {
            if off.days.len() == MOST_DAYS {
                // More than a year of them one by one is not a list a field carries.
                cx.dropped.push("date-rules".to_owned());
                return Ok(off);
            }
            off.days.push(at.to_string());
            let Some(next) = at.succ_opt() else { break };
            at = next;
        }
    }
    Ok(off)
}

/// Tells a date field which days can't be picked.
fn switched_off(field: &mut n::Field, off: &Off) {
    if let i::Input::Date(date) = &mut field.input {
        date.off_days.clone_from(&off.days);
        date.off_weekdays.clone_from(&off.weekdays);
    }
}

/// `DateRangePicker`: two dates, the first and the last day.
pub(crate) fn range(cx: &mut Cx, element: &Element) -> Read {
    let mut parts = Parts::of(element);
    let natives = parts.slot("range-native");
    let popup = parts.slot("date-range-ui");
    parts.done()?;
    let (Some(natives), Some(popup)) = (natives, popup) else {
        return fail("a range of dates with no fields under it");
    };
    let off = rules(cx, popup)?;
    let mut fields = Vec::new();
    for label in elements(natives)? {
        if slot(label).as_deref() != Some("range-native-field") {
            return fail("something that is not a date in a range of dates");
        }
        let inside = elements(label)?;
        let [words, input] = inside.as_slice() else {
            return fail("a date of a range that is not words and a field");
        };
        let mut field = plain(cx, input)?;
        switched_off(&mut field, &off);
        field.label = Some(text_of(words));
        fields.push(N::Field(field));
    }
    let [N::Field(from), N::Field(to)] = fields.as_slice() else {
        return fail("a range of dates that is not two dates");
    };
    let (i::Input::Date(first), i::Input::Date(last)) = (&from.input, &to.input) else {
        return fail("a range of dates whose ends are not dates");
    };
    // The ends are each sent under their own name; an error for the range as a whole
    // comes under the first's.
    Ok(vec![N::Field(n::Field {
        id: id(element),
        name: from.name.clone(),
        required: from.required || to.required,
        disabled: from.disabled && to.disabled,
        input: i::Input::Range(i::RangeInput {
            kind: first.kind,
            from: i::RangeEnd {
                name: from.name.clone(),
                value: first.value.clone(),
            },
            to: i::RangeEnd {
                name: to.name.clone(),
                value: last.value.clone(),
            },
            min: first.min.clone(),
            max: first.max.clone(),
            off_days: first.off_days.clone(),
            off_weekdays: first.off_weekdays.clone(),
        }),
        ..n::Field::default()
    })])
}

/// The words a date control's field is named by, when the page gave none to its input.
#[allow(dead_code)]
fn named(element: &Element) -> Option<String> {
    attr(element, "aria-label").map(Cow::into_owned)
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use navree_native::input as i;

    use super::super::testing::{misses, read, tamper};
    use super::*;
    use crate::dates::{DatePicker, DateRangePicker, DateRules, TimePicker};
    use crate::node::Node;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap_or_default()
    }

    fn date_of(node: N) -> (n::Field, i::DateInput) {
        let N::Field(field) = node else {
            panic!("not a field: {node:?}");
        };
        let i::Input::Date(date) = field.input.clone() else {
            panic!("not a date: {:?}", field.input);
        };
        (field, date)
    }

    #[test]
    fn a_date_picker_is_a_date_with_its_name_value_and_limits() {
        let (field, date) = date_of(read(
            DatePicker::new("due")
                .label("Due on")
                .value(day(2026, 11, 20))
                .min(day(2026, 11, 1))
                .max(day(2026, 12, 31))
                .required(),
        ));
        assert_eq!(field.name, "due");
        assert_eq!(field.label.as_deref(), Some("Due on"));
        assert!(field.required);
        assert_eq!(field.id.as_deref(), Some("dp-due"));
        assert_eq!(date.kind, i::DateKind::Date);
        assert_eq!(date.value, "2026-11-20");
        assert_eq!(date.min.as_deref(), Some("2026-11-01"));
        assert_eq!(date.max.as_deref(), Some("2026-12-31"));
    }

    #[test]
    fn a_date_picker_that_is_switched_off_says_so_and_one_with_nothing_chosen_has_no_value() {
        let (field, date) = date_of(read(DatePicker::new("due").disabled(true)));
        assert!(field.disabled);
        assert_eq!(date.value, "");
    }

    #[test]
    fn the_days_a_calendar_switches_off_are_the_fields_days_off() {
        let (_, date) = date_of(read(
            DatePicker::new("due").rules(DateRules::new().min(day(2026, 11, 1))),
        ));
        assert!(date.off_days.is_empty() && date.off_weekdays.is_empty());
        let (_, date) = date_of(read(
            DatePicker::new("due")
                .rules(
                    DateRules::new()
                        .disable_dates([day(2026, 11, 14), day(2026, 11, 16)])
                        .disable_range(day(2026, 11, 20), day(2026, 11, 22))
                        .disable_weekdays([chrono::Weekday::Sun, chrono::Weekday::Sat]),
                )
                .today(day(2026, 11, 12)),
        ));
        assert_eq!(
            date.off_days,
            [
                "2026-11-14",
                "2026-11-16",
                "2026-11-20",
                "2026-11-21",
                "2026-11-22"
            ]
        );
        assert_eq!(
            date.off_weekdays,
            [i::Weekday::Sunday, i::Weekday::Saturday]
        );
    }

    #[test]
    fn a_range_has_the_same_days_off_on_both_ends() {
        let N::Field(field) = read(
            DateRangePicker::new("from", "to")
                .rules(DateRules::new().disable_weekdays([chrono::Weekday::Sun])),
        ) else {
            panic!("not a field");
        };
        let i::Input::Range(range) = &field.input else {
            panic!("not a range");
        };
        assert_eq!(range.off_weekdays, [i::Weekday::Sunday]);
    }

    #[test]
    fn more_than_a_year_of_days_off_is_said_not_listed() {
        let node: Node = DatePicker::new("due")
            .rules(DateRules::new().disable_range(day(2026, 1, 1), day(2027, 12, 31)))
            .min(day(2026, 1, 1))
            .max(day(2027, 12, 31))
            .into();
        let lowered = crate::native::lower(&node);
        assert!(lowered.is_native(), "{:?}", lowered.misses);
        assert_eq!(lowered.dropped, ["date-rules"]);
    }

    #[test]
    fn a_time_picker_is_a_time() {
        let (field, time) = date_of(read(
            TimePicker::new("at")
                .value(chrono::NaiveTime::from_hms_opt(9, 30, 0).unwrap_or_default())
                .label("Starts at"),
        ));
        assert_eq!(field.name, "at");
        assert_eq!(time.kind, i::DateKind::Time);
        assert_eq!(time.value, "09:30");
        assert_eq!(field.label.as_deref(), Some("Starts at"));
    }

    #[test]
    fn a_date_and_time_picker_is_one_field_of_both() {
        let at = day(2026, 11, 20).and_hms_opt(18, 45, 0).unwrap_or_default();
        let (field, both) = date_of(read(
            crate::dates::DateTimePicker::new("when")
                .value(at)
                .zone_note("Asia/Kolkata (IST)"),
        ));
        assert_eq!(field.name, "when");
        assert_eq!(both.kind, i::DateKind::Datetime);
        assert_eq!(both.value, "2026-11-20T18:45");
        assert_eq!(field.help.as_deref(), Some("Asia/Kolkata (IST)"));
    }

    #[test]
    fn a_range_of_dates_is_one_field_whose_ends_are_named_from_and_to() {
        let N::Field(field) = read(DateRangePicker::new("from", "to").value(
            crate::dates::DateRange::new(day(2026, 11, 1), day(2026, 11, 12)),
        )) else {
            panic!("not a field");
        };
        let i::Input::Range(range) = &field.input else {
            panic!("not a range");
        };
        assert_eq!(field.name, "from");
        assert_eq!(range.kind, i::DateKind::Date);
        assert_eq!(
            (range.from.name.as_str(), range.from.value.as_str()),
            ("from", "2026-11-01")
        );
        assert_eq!(
            (range.to.name.as_str(), range.to.value.as_str()),
            ("to", "2026-11-12")
        );
    }

    #[test]
    fn a_date_control_in_a_field_takes_the_fields_words_and_so_does_a_range() {
        let N::Field(field) = read(
            crate::Field::new("due", "Due on")
                .help("When it is owed")
                .error("Pick a day")
                .control(DatePicker::new("due")),
        ) else {
            panic!("not a field");
        };
        assert_eq!(field.label.as_deref(), Some("Due on"));
        assert_eq!(field.help.as_deref(), Some("When it is owed"));
        assert_eq!(field.error.as_deref(), Some("Pick a day"));
        // A range is one field: the words that name it are its own.
        let N::Field(range) = read(
            crate::Field::new("when", "Placed")
                .help("Between two days")
                .error("Pick two days")
                .control(DateRangePicker::new("from", "to")),
        ) else {
            panic!("not a field");
        };
        assert!(matches!(range.input, i::Input::Range(_)));
        assert_eq!(range.label.as_deref(), Some("Placed"));
        assert_eq!(range.help.as_deref(), Some("Between two days"));
        assert_eq!(range.error.as_deref(), Some("Pick two days"));
    }

    #[test]
    fn a_date_that_can_be_read_but_not_changed_is_a_miss() {
        let node: Node = DatePicker::new("due").read_only().into();
        assert_eq!(misses(node), ["slot:date-picker"]);
    }

    #[test]
    fn a_hook_no_reader_knows_inside_a_date_control_is_still_a_miss() {
        for (node, slot) in [
            (Node::from(DatePicker::new("due")), "date-picker"),
            (Node::from(TimePicker::new("at")), "time-picker"),
            (
                Node::from(DateRangePicker::new("from", "to")),
                "date-range-picker",
            ),
            (Node::from(DatePicker::new("due")), "input"),
        ] {
            assert_eq!(
                misses(tamper(node, slot, "data-ui-wizard")),
                ["hook:data-ui-wizard"],
                "{slot}"
            );
        }
    }
}
