//! Calendar, DatePicker, DateRangePicker, TimePicker.

use chrono::{Datelike, NaiveDate, NaiveTime, Weekday};
use navree::ui::dates::*;
use navree::ui::*;

use crate::catalog::{Example, gallery, single};

const SOURCE: &str = include_str!("dates.rs");

pub fn all() -> Vec<Example> {
    vec![
        single!("calendar", "thumb", "A calendar", calendar_thumb).height(300),
        single!("calendar", "default", "A calendar", calendar_default).height(340),
        single!("calendar", "multiple", "Several days", calendar_multiple).height(340),
        single!(
            "calendar",
            "range",
            "A range over two months",
            calendar_range
        )
        .height(340),
        single!(
            "calendar",
            "limits",
            "Days that can't be chosen",
            calendar_limits
        )
        .height(340),
        single!("calendar", "weeks", "Week numbers", calendar_weeks).height(340),
        single!(
            "calendar",
            "dropdowns",
            "A date of birth",
            calendar_dropdowns
        )
        .height(340),
        gallery!("calendar", "locales", "Languages", calendar_locales).height(340),
        gallery!("calendar", "states", "States", calendar_states).height(340),
        single!("date-picker", "thumb", "A date picker", date_picker_thumb).height(120),
        single!(
            "date-picker",
            "default",
            "A date picker",
            date_picker_default
        )
        .start()
        .height(420),
        single!(
            "date-picker",
            "options",
            "A value, Today and Clear",
            date_picker_options
        )
        .start()
        .height(420),
        // Tall enough for the calendar to open under each field, whole.
        gallery!("date-picker", "states", "States", date_picker_states)
            .start()
            .height(440),
        single!("date-picker", "button", "One button", date_picker_button)
            .start()
            .height(420),
        gallery!("date-picker", "locales", "Languages", date_picker_locales)
            .start()
            .height(440),
        single!("date-picker", "birth", "A date of birth", date_picker_birth)
            .start()
            .height(440),
        single!(
            "date-range-picker",
            "thumb",
            "A range picker",
            date_range_picker_thumb
        )
        .height(120),
        single!(
            "date-range-picker",
            "default",
            "A range picker",
            date_range_picker_default
        )
        .start()
        .height(460),
        single!(
            "date-range-picker",
            "presets",
            "Presets",
            date_range_picker_presets
        )
        .start()
        .height(460),
        single!(
            "date-range-picker",
            "confirm",
            "Apply and Cancel",
            date_range_picker_confirm
        )
        .start()
        .height(500),
        single!(
            "date-range-picker",
            "limits",
            "Limits",
            date_range_picker_limits
        )
        .start()
        .height(460),
        single!(
            "date-range-picker",
            "report",
            "A report's window in the address",
            date_range_picker_report
        )
        .start()
        .height(460),
        single!("time-picker", "thumb", "A time picker", time_picker_thumb).height(120),
        single!(
            "time-picker",
            "default",
            "A time picker",
            time_picker_default
        )
        .start()
        .height(380),
        // Tall enough for the list of times to open under each field, whole.
        gallery!(
            "time-picker",
            "clocks",
            "12 and 24 hours",
            time_picker_clocks
        )
        .start()
        .height(380),
        single!("time-picker", "rules", "Office hours", time_picker_rules)
            .start()
            .height(380),
        gallery!("time-picker", "states", "States", time_picker_states)
            .start()
            .height(380),
        single!(
            "time-picker",
            "zone",
            "The clock it counts on",
            time_picker_zone
        )
        .start()
        .height(380),
    ]
}

fn calendar_thumb() -> Calendar {
    Calendar::new("thumb")
        .today(NaiveDate::from_ymd_opt(2026, 11, 3).unwrap_or_default())
        .selected(NaiveDate::from_ymd_opt(2026, 11, 12).unwrap_or_default())
        .size(Size::Sm)
}

fn calendar_default() -> Calendar {
    let today = NaiveDate::from_ymd_opt(2026, 11, 3).unwrap_or_default();
    Calendar::new("due")
        .name("due")
        .today(today)
        .selected(NaiveDate::from_ymd_opt(2026, 11, 12).unwrap_or_default())
        .with_today()
}

fn calendar_multiple() -> Calendar {
    let day = |d| NaiveDate::from_ymd_opt(2026, 11, d).unwrap_or_default();
    Calendar::new("days")
        .name("days")
        .today(day(3))
        .mode(CalendarMode::Multiple)
        .selected_days([day(5), day(12), day(19)])
        .max_count(4)
}

fn calendar_range() -> Calendar {
    let day = |d| NaiveDate::from_ymd_opt(2026, 11, d).unwrap_or_default();
    Calendar::new("window")
        .range_names("from", "to")
        .today(day(3))
        .mode(CalendarMode::Range)
        .months(2)
        .selected_range(DateRange::new(day(9), day(17)))
}

fn calendar_limits() -> Calendar {
    let day = |m, d| NaiveDate::from_ymd_opt(2026, m, d).unwrap_or_default();
    Calendar::new("visit")
        .name("visit")
        .today(day(11, 3))
        .rules(
            DateRules::new()
                .min(day(11, 2))
                .max(day(12, 31))
                .disable_weekdays([Weekday::Sat, Weekday::Sun])
                .disable_dates([day(11, 11)])
                .disable_range(day(12, 24), day(12, 26))
                .disable_if(|date| date.day() == 13),
        )
}

fn calendar_weeks() -> Calendar {
    Calendar::new("weeks")
        .today(NaiveDate::from_ymd_opt(2026, 11, 3).unwrap_or_default())
        .week_start(Weekday::Mon)
        .week_numbers()
        .fixed_weeks()
}

fn calendar_dropdowns() -> Calendar {
    let today = NaiveDate::from_ymd_opt(2026, 11, 3).unwrap_or_default();
    Calendar::new("born")
        .name("born")
        .today(today)
        .month(NaiveDate::from_ymd_opt(1990, 5, 1).unwrap_or_default())
        .caption(CaptionLayout::Dropdowns)
        .max(today)
}

fn calendar_locales() -> impl IntoIterator<Item = (&'static str, Calendar)> {
    let today = NaiveDate::from_ymd_opt(2026, 11, 3).unwrap_or_default();
    [
        (
            "de",
            Calendar::new("de")
                .today(today)
                .locale(DateLocale::of("de"))
                .week_numbers(),
        ),
        (
            "ja",
            Calendar::new("ja")
                .today(today)
                .locale(DateLocale::of("ja")),
        ),
        (
            "ar",
            Calendar::new("ar")
                .today(today)
                .locale(DateLocale::of("ar")),
        ),
    ]
}

fn calendar_states() -> impl IntoIterator<Item = (&'static str, Calendar)> {
    let today = NaiveDate::from_ymd_opt(2026, 11, 3).unwrap_or_default();
    let chosen = NaiveDate::from_ymd_opt(2026, 11, 12).unwrap_or_default();
    [
        (
            "Read only",
            Calendar::new("ro")
                .today(today)
                .selected(chosen)
                .read_only(),
        ),
        (
            "Disabled",
            Calendar::new("off")
                .today(today)
                .selected(chosen)
                .disabled(true),
        ),
    ]
}

fn date_picker_thumb() -> DatePicker {
    DatePicker::new("thumb")
        .today(NaiveDate::from_ymd_opt(2026, 11, 3).unwrap_or_default())
        .value(NaiveDate::from_ymd_opt(2026, 11, 12).unwrap_or_default())
        .label("Date")
}

fn date_picker_default() -> Field {
    Field::new("due", "Due date")
        .help("Type it as 11/30/2026, or pick it.")
        .control(
            DatePicker::new("due").today(NaiveDate::from_ymd_opt(2026, 11, 3).unwrap_or_default()),
        )
}

fn date_picker_options() -> Field {
    Field::new("follow-up", "Follow up").control(
        DatePicker::new("follow_up")
            .today(NaiveDate::from_ymd_opt(2026, 11, 3).unwrap_or_default())
            .value(NaiveDate::from_ymd_opt(2026, 11, 12).unwrap_or_default())
            .clearable()
            .with_today(),
    )
}

fn date_picker_states() -> impl IntoIterator<Item = (&'static str, Field)> {
    let today = NaiveDate::from_ymd_opt(2026, 11, 3).unwrap_or_default();
    let chosen = NaiveDate::from_ymd_opt(2026, 11, 12).unwrap_or_default();
    [
        (
            "Invalid",
            Field::new("late", "Late date")
                .error("Choose a weekday.")
                .control(
                    DatePicker::new("late")
                        .today(today)
                        .value_text("garbage")
                        .invalid(true),
                ),
        ),
        (
            "Disabled",
            Field::new("off", "Disabled").control(
                DatePicker::new("off")
                    .today(today)
                    .value(chosen)
                    .disabled(true),
            ),
        ),
        (
            "Read only",
            Field::new("ro", "Read only")
                .control(DatePicker::new("ro").today(today).value(chosen).read_only()),
        ),
    ]
}

fn date_picker_button() -> DatePicker {
    DatePicker::new("when")
        .today(NaiveDate::from_ymd_opt(2026, 11, 3).unwrap_or_default())
        .style(PickerStyle::Button)
        .placeholder("Pick a day")
        .clearable()
        .label("When")
}

fn date_picker_locales() -> impl IntoIterator<Item = (&'static str, DatePicker)> {
    let today = NaiveDate::from_ymd_opt(2026, 11, 3).unwrap_or_default();
    let chosen = NaiveDate::from_ymd_opt(2026, 11, 12).unwrap_or_default();
    [
        (
            "de",
            DatePicker::new("de")
                .today(today)
                .locale(DateLocale::of("de"))
                .value(chosen)
                .label("Datum"),
        ),
        (
            "ja",
            DatePicker::new("ja")
                .today(today)
                .locale(DateLocale::of("ja"))
                .value(chosen)
                .label("日付"),
        ),
        (
            "ar",
            DatePicker::new("ar")
                .today(today)
                .locale(DateLocale::of("ar"))
                .value(chosen)
                .label("التاريخ"),
        ),
    ]
}

fn date_picker_birth() -> Field {
    let today = NaiveDate::from_ymd_opt(2026, 11, 3).unwrap_or_default();
    Field::new("born", "Date of birth").control(
        DatePicker::new("born")
            .today(today)
            .caption(CaptionLayout::Dropdowns)
            .max(today)
            .value(NaiveDate::from_ymd_opt(1990, 5, 17).unwrap_or_default()),
    )
}

fn date_range_picker_thumb() -> DateRangePicker {
    let day = |d| NaiveDate::from_ymd_opt(2026, 11, d).unwrap_or_default();
    DateRangePicker::new("from", "to")
        .today(day(3))
        .title("Created")
        .value(DateRange::new(day(1), day(3)))
}

fn date_range_picker_default() -> DateRangePicker {
    DateRangePicker::new("from", "to")
        .today(NaiveDate::from_ymd_opt(2026, 11, 3).unwrap_or_default())
        .week_start(Weekday::Mon)
        .title("Created")
}

fn date_range_picker_presets() -> DateRangePicker {
    DateRangePicker::new("from", "to")
        .today(NaiveDate::from_ymd_opt(2026, 11, 3).unwrap_or_default())
        .presets([
            DatePreset::Today,
            DatePreset::Last7Days,
            DatePreset::ThisMonth,
            DatePreset::LastMonth,
            DatePreset::ThisQuarter,
            DatePreset::ThisYear,
        ])
        .preset_label(DatePreset::Last7Days, "The last week")
        .title("Closed")
}

fn date_range_picker_confirm() -> DateRangePicker {
    let day = |m, d| NaiveDate::from_ymd_opt(2026, m, d).unwrap_or_default();
    DateRangePicker::new("from", "to")
        .today(day(11, 3))
        .value(DateRange::new(day(10, 27), day(11, 3)))
        .confirm()
        .title("Window")
}

fn date_range_picker_limits() -> DateRangePicker {
    let day = |m, d| NaiveDate::from_ymd_opt(2026, m, d).unwrap_or_default();
    // The limits are one value: the same one holds the request on the server.
    let limits = RangeRules::new(DateRules::new().min(day(10, 15)).max(day(11, 20)))
        .min_days(2)
        .max_days(10);
    DateRangePicker::new("from", "to")
        .today(day(11, 3))
        .limits(limits)
        .presets([
            DatePreset::Today,
            DatePreset::Last7Days,
            DatePreset::AllTime,
        ])
        .title("Booking")
}

fn date_range_picker_report() -> Node {
    let today = NaiveDate::from_ymd_opt(2026, 11, 3).unwrap_or_default();
    // The picker lives in a GET form, so the window is in the address, and the form
    // fetches its region again when the window changes.
    el("form")
        .attr("method", "get")
        .attr("action", "/reports")
        .attr("data-ui-target", "report")
        .children((
            DateRangePicker::new("from", "to")
                .today(today)
                .title("Created")
                .value(DateRange::new(today - chrono::Duration::days(6), today)),
            Button::primary("Apply").submit(),
        ))
        .into()
}

fn time_picker_thumb() -> TimePicker {
    TimePicker::new("thumb")
        .value(NaiveTime::from_hms_opt(14, 30, 0).unwrap_or_default())
        .label("Time")
}

fn time_picker_default() -> Field {
    Field::new("starts", "Starts at").control(
        TimePicker::new("starts").value(NaiveTime::from_hms_opt(14, 30, 0).unwrap_or_default()),
    )
}

fn time_picker_clocks() -> impl IntoIterator<Item = (&'static str, TimePicker)> {
    let at = NaiveTime::from_hms_opt(15, 5, 0).unwrap_or_default();
    [
        ("en", TimePicker::new("en").value(at).label("Time")),
        (
            "de",
            TimePicker::new("de")
                .locale(DateLocale::of("de"))
                .value(at)
                .label("Uhrzeit"),
        ),
        (
            "en, 24 hour",
            TimePicker::new("en24")
                .clock(Clock::H24)
                .value(at)
                .label("Time"),
        ),
    ]
}

fn time_picker_rules() -> Field {
    let at = |h| NaiveTime::from_hms_opt(h, 0, 0).unwrap_or_default();
    Field::new("visit-time", "Visit").control(
        TimePicker::new("visit").rules(
            TimeRules::new()
                .min(at(9))
                .max(at(17))
                .unavailable(
                    at(12),
                    NaiveTime::from_hms_opt(12, 59, 0).unwrap_or_default(),
                )
                .step_minutes(15),
        ),
    )
}

fn time_picker_states() -> impl IntoIterator<Item = (&'static str, TimePicker)> {
    let at = NaiveTime::from_hms_opt(8, 0, 0).unwrap_or_default();
    [
        (
            "Invalid",
            TimePicker::new("bad")
                .value_text("soon")
                .invalid(true)
                .label("Time"),
        ),
        (
            "Disabled",
            TimePicker::new("off")
                .value(at)
                .disabled(true)
                .label("Time"),
        ),
        (
            "Clearable",
            TimePicker::new("clear").value(at).clearable().label("Time"),
        ),
    ]
}

fn time_picker_zone() -> TimePicker {
    TimePicker::new("meeting")
        .value(NaiveTime::from_hms_opt(9, 0, 0).unwrap_or_default())
        .zone_note("Times are in Asia/Kolkata (IST).")
        .label("Meeting")
}
