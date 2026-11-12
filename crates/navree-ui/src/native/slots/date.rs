//! `DatePicker`, `TimePicker`, `DateTimePicker` and `DateRangePicker`.
//!
//! A date control with scripts is a text box, a button and a calendar in a
//! popup. Under it the page keeps the plain control a form would send: an
//! `<input type="date">` (or `time`, or `datetime-local`) that carries the
//! name, the value and the limits. That is what is read; the calendar, the
//! list of times and the presets are a browser's way to fill it in, and every
//! system has its own.

use std::borrow::Cow;

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
    if let Some(popup) = popup {
        rules(cx, popup)?;
    }
    let mut field = plain(cx, native)?;
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

/// Notes the days a calendar switches off, which a client's date field has no
/// way to say: the limits (`min` and `max`) stand on the field itself.
fn rules(cx: &mut Cx, popup: &Element) -> Read<()> {
    let Some(calendar) = crate::native::page::find(popup.child_nodes(), "calendar") else {
        return Ok(());
    };
    let Some(json) = attr(calendar, "data-ui-rules") else {
        return Ok(());
    };
    let Ok(rules) = serde_json::from_str::<serde_json::Value>(&json) else {
        return fail("rules for a calendar that are not readable");
    };
    let off = rules.as_object().is_some_and(|rules| {
        rules
            .keys()
            .any(|key| !matches!(key.as_str(), "min" | "max"))
    });
    if off {
        cx.dropped.push("date-rules".to_owned());
    }
    Ok(())
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
    rules(cx, popup)?;
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
        field.label = Some(text_of(words));
        fields.push(N::Field(field));
    }
    if fields.len() != 2 {
        return fail("a range of dates that is not two dates");
    }
    Ok(vec![N::Stack(n::Stack {
        id: id(element),
        children: fields,
        ..n::Stack::default()
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
    fn the_days_a_calendar_switches_off_are_left_out_and_said() {
        let node: Node = DatePicker::new("due")
            .rules(DateRules::new().min(day(2026, 11, 1)))
            .into();
        assert!(crate::native::lower(&node).dropped.is_empty());
        let node: Node = DatePicker::new("due")
            .rules(DateRules::new().disable_dates([day(2026, 11, 14)]))
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
    fn a_range_of_dates_is_two_dates_named_from_and_to() {
        let N::Stack(stack) = read(DateRangePicker::new("from", "to").value(
            crate::dates::DateRange::new(day(2026, 11, 1), day(2026, 11, 12)),
        )) else {
            panic!("not a stack");
        };
        let [N::Field(from), N::Field(to)] = stack.children.as_slice() else {
            panic!("not two fields");
        };
        assert_eq!((from.name.as_str(), to.name.as_str()), ("from", "to"));
        assert_eq!(from.label.as_deref(), Some("From"));
        assert_eq!(to.label.as_deref(), Some("To"));
        let (i::Input::Date(first), i::Input::Date(last)) = (&from.input, &to.input) else {
            panic!("not dates");
        };
        assert_eq!(
            (first.value.as_str(), last.value.as_str()),
            ("2026-11-01", "2026-11-12")
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
        // A range is two fields: the words that name it stand over them and under them.
        let N::Stack(range) = read(
            crate::Field::new("when", "Placed")
                .help("Between two days")
                .control(DateRangePicker::new("from", "to")),
        ) else {
            panic!("not a stack");
        };
        assert_eq!(range.children.len(), 4);
        assert_eq!(
            range.children[0],
            N::Text(n::Text {
                value: "Placed".into(),
                style: n::TextStyle::Caption,
                muted: true,
                ..n::Text::default()
            })
        );
        assert!(matches!(range.children[1], N::Field(_)));
        assert!(matches!(&range.children[3], N::Text(text) if text.value == "Between two days"));
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
