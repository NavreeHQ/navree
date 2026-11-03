//! One definition for the browser and the server: a range picker's limits, a form's rules,
//! and the limits of the native fields that stand in with JavaScript off. What a page says
//! once is what both hold a person to, so the two can't differ.

use chrono::{Datelike, NaiveDate, NaiveTime, Weekday};
use navree_ui::dates::{
    Calendar, CalendarMode, CaptionLayout, DateError, DateLocale, DatePicker, DateRange,
    DateRangePicker, DateRangeQuery, DateRules, RangeRules, TimePicker, TimeRules,
};
use navree_ui::form::decode;
use navree_ui::{Form, FormView, Node, Validate, render};
use serde::Deserialize;
use serde_json::{Value, json};

fn day(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

fn t(h: u32, m: u32, s: u32) -> NaiveTime {
    NaiveTime::from_hms_opt(h, m, s).unwrap()
}

fn html(node: impl Into<Node>) -> String {
    render(&node.into())
}

/// The value of attribute `name` on the first element whose opening tag holds `marker`.
fn attr_of(html: &str, marker: &str, name: &str) -> Option<String> {
    let at = html.find(marker)?;
    let start = html[..at].rfind('<')?;
    let end = start + html[start..].find('>')?;
    let tag = &html[start..end];
    let key = format!(" {name}=\"");
    let from = tag.find(&key)? + key.len();
    Some(tag[from..from + tag[from..].find('"')?].to_owned())
}

fn json_attr(html: &str, marker: &str, name: &str) -> Value {
    let text = attr_of(html, marker, name).unwrap_or_default();
    let text = text
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&");
    serde_json::from_str(&text).unwrap_or(Value::Null)
}

fn open_tag<'a>(html: &'a str, marker: &str) -> &'a str {
    let at = html
        .find(marker)
        .unwrap_or_else(|| panic!("{marker} is not in {html}"));
    let start = html[..at].rfind('<').unwrap();
    &html[start..start + html[start..].find('>').unwrap() + 1]
}

#[test]
fn a_range_picker_and_the_server_read_take_the_same_limits() {
    let limits = RangeRules::new(
        DateRules::new()
            .min(day(2026, 11, 2))
            .disable_weekdays([Weekday::Sat, Weekday::Sun]),
    )
    .min_days(2)
    .max_days(10);
    assert_eq!(
        (limits.least_days(), limits.most_days()),
        (Some(2), Some(10))
    );
    // What the browser is given.
    let page = html(
        DateRangePicker::new("from", "to")
            .today(day(2026, 11, 3))
            .limits(limits.clone()),
    );
    assert_eq!(
        attr_of(&page, "data-ui-dates=\"range\"", "data-min-days").as_deref(),
        Some("2")
    );
    assert_eq!(
        attr_of(&page, "data-ui-dates=\"range\"", "data-max-days").as_deref(),
        Some("10")
    );
    let rules = json_attr(&page, "data-ui-dates=\"calendar\"", "data-ui-rules");
    assert_eq!(rules["wd"], json!([0, 6]));
    assert_eq!(rules["min"], json!("2026-11-02"));
    // What the server holds a request to: the same value.
    let query = |from: &str, to: &str| DateRangeQuery {
        from: Some(from.into()),
        to: Some(to.into()),
    };
    assert_eq!(
        query("2026-11-02", "2026-11-04").checked(&limits),
        Ok(DateRange::parse("2026-11-02", "2026-11-04"))
    );
    assert_eq!(
        query("2026-11-02", "2026-11-02").checked(&limits),
        Err(DateError::RangeTooShort(2))
    );
    assert_eq!(
        query("2026-11-02", "2026-11-20").checked(&limits),
        Err(DateError::RangeTooLong(10))
    );
    assert_eq!(
        query("2026-11-06", "2026-11-09").checked(&limits),
        Err(DateError::RangeBlocked)
    );
    assert_eq!(
        query("2026-11-02", "2026-11-07").checked(&limits),
        Err(DateError::Weekday(Weekday::Sat))
    );
    assert_eq!(
        query("2026-10-30", "2026-11-04").checked(&limits),
        Err(DateError::TooEarly(day(2026, 11, 2)))
    );
    // No range is no error. The lenient read gives no range for one the limits refuse.
    assert_eq!(DateRangeQuery::default().checked(&limits), Ok(None));
    assert_eq!(query("soon", "2026-11-04").checked(&limits), Ok(None));
    assert_eq!(
        query("2026-11-02", "2026-11-02").range_within(&limits),
        None
    );
    assert!(
        query("2026-11-02", "2026-11-04")
            .range_within(&limits)
            .is_some()
    );
    // A calendar takes them as one too.
    let calendar = html(
        Calendar::new("c")
            .today(day(2026, 11, 3))
            .mode(CalendarMode::Range)
            .limits(limits),
    );
    assert_eq!(
        attr_of(&calendar, "data-ui-dates=\"calendar\"", "data-max-days").as_deref(),
        Some("10")
    );
}

fn office_days() -> DateRules {
    DateRules::new()
        .min(day(2026, 11, 3))
        .max(day(2027, 2, 3))
        .disable_weekdays([Weekday::Sat, Weekday::Sun])
}

fn office_hours() -> TimeRules {
    TimeRules::new()
        .min(t(9, 0, 0))
        .max(t(17, 0, 0))
        .unavailable(t(12, 0, 0), t(12, 59, 0))
        .step_minutes(15)
        .strict_step()
}

fn open_day(day: &NaiveDate) -> Result<(), String> {
    office_days()
        .check(*day)
        .map_err(|error| error.message(&DateLocale::english()))
}

fn open_hours(at: &NaiveTime) -> Result<(), String> {
    office_hours()
        .check(*at)
        .map_err(|error| error.message(&DateLocale::english()))
}

#[derive(Debug, Deserialize, Validate, Form)]
struct Visit {
    /// Day
    #[validate(custom(open_day))]
    #[form(widget = date_picker, date_rules = office_days)]
    day: NaiveDate,
    /// At
    #[validate(custom(open_hours))]
    #[form(widget = time_picker, time_rules = office_hours)]
    at: NaiveTime,
}

fn posted(day: &str, at: &str) -> Vec<(String, String)> {
    vec![
        ("day".to_owned(), day.to_owned()),
        ("at".to_owned(), at.to_owned()),
    ]
}

#[test]
fn a_form_gives_its_pickers_the_rules_the_server_holds_it_to() {
    let page = html(FormView::<Visit>::new().dates(DateLocale::english(), day(2026, 11, 3)));
    // The calendar in the date picker carries the rules, not an empty set.
    let rules = json_attr(&page, "data-ui-dates=\"calendar\"", "data-ui-rules");
    assert_eq!(rules["wd"], json!([0, 6]), "{rules}");
    assert_eq!(rules["min"], json!("2026-11-03"));
    assert_eq!(rules["max"], json!("2027-02-03"));
    assert!(open_tag(&page, "type=\"date\"").contains(" min=\"2026-11-03\""));
    // The time picker too, and its native field is told what the server takes.
    let times = json_attr(&page, "data-ui-dates=\"time\"", "data-ui-trules");
    assert_eq!(times["min"], json!("09:00:00"));
    assert_eq!(times["u"], json!([[720, 779]]));
    assert_eq!(times["strict"], json!(1));
    let native = open_tag(&page, "type=\"time\"");
    assert!(
        native.contains(" step=\"900\"") && native.contains(" min=\"09:00\""),
        "{native}"
    );
    // The browser's reading of the days is the server's, day by day, across four months.
    let min = rules["min"].as_str().unwrap().to_owned();
    let max = rules["max"].as_str().unwrap().to_owned();
    let weekdays: Vec<u64> = rules["wd"]
        .as_array()
        .unwrap()
        .iter()
        .map(|weekday| weekday.as_u64().unwrap())
        .collect();
    let mut checked = 0;
    let mut date = day(2026, 10, 20);
    while date < day(2027, 3, 1) {
        let browser_allows = date.to_string() >= min
            && date.to_string() <= max
            && !weekdays.contains(&u64::from(date.weekday().num_days_from_sunday()));
        let server_accepts = decode::<Visit>(&posted(&date.to_string(), "10:00")).is_ok();
        assert_eq!(browser_allows, server_accepts, "{date}");
        checked += 1;
        date = date.succ_opt().unwrap();
    }
    assert!(checked > 130);
    // The times, minute by minute through a day: the server's check is the rules' own.
    for minute in 0..1440 {
        let at = format!("{:02}:{:02}", minute / 60, minute % 60);
        let server_accepts = decode::<Visit>(&posted("2026-11-09", &at)).is_ok();
        assert_eq!(
            server_accepts,
            office_hours().allows(t(minute / 60, minute % 60, 0)),
            "{at}"
        );
    }
    // A leap second is no time of day the server takes, though chrono reads it.
    assert!(decode::<Visit>(&posted("2026-11-09", "16:00:60")).is_err());
}

#[test]
fn the_native_time_field_is_told_what_the_server_takes_and_never_less() {
    let tag = |picker: TimePicker| open_tag(&html(picker), "type=\"time\"").to_owned();
    // A strict step counts from midnight on the server. The least time is put on it, so the
    // native step, which counts from the least, is the same.
    let strict = tag(TimePicker::new("t").rules(
        TimeRules::new()
            .min(t(9, 10, 0))
            .step_minutes(15)
            .strict_step(),
    ));
    assert!(
        strict.contains(" min=\"09:15\"") && strict.contains(" step=\"900\""),
        "{strict}"
    );
    // Not strict: no step, and the least as it is.
    let loose = tag(TimePicker::new("t").rules(TimeRules::new().min(t(9, 10, 0)).max(t(17, 0, 0))));
    assert!(
        loose.contains(" min=\"09:10\"")
            && loose.contains(" max=\"17:00\"")
            && !loose.contains(" step="),
        "{loose}"
    );
    // A value with seconds, or a limit with seconds, needs a step of a second.
    let seconds = tag(TimePicker::new("t").value(t(9, 0, 30)));
    assert!(
        seconds.contains(" step=\"1\"") && seconds.contains("value=\"09:00:30\""),
        "{seconds}"
    );
    let limits =
        tag(TimePicker::new("t").rules(TimeRules::new().min(t(9, 0, 30)).max(t(17, 0, 30))));
    assert!(
        limits.contains(" min=\"09:00:30\"")
            && limits.contains(" max=\"17:00:30\"")
            && limits.contains(" step=\"1\""),
        "{limits}"
    );
    // A least time past the last step of the day leaves nothing to count from: no least.
    let late = tag(TimePicker::new("t").rules(
        TimeRules::new()
            .min(t(23, 50, 0))
            .step_minutes(30)
            .strict_step(),
    ));
    assert!(!late.contains(" min="), "{late}");
}

#[test]
fn a_hostile_id_is_text_in_every_component_that_takes_one() {
    let nasty = "x\"><meta http-equiv=refresh content=\"0;url=https://example.com/\"><input name=role value=admin><a id=planted href=//evil.example/>x</a>";
    let today = day(2026, 11, 3);
    let pages = [
        html(Calendar::new(nasty).today(today).months(2)),
        html(
            Calendar::new(nasty)
                .today(today)
                .caption(CaptionLayout::Dropdowns),
        ),
        html(DatePicker::new("d").id(nasty).today(today)),
        html(DateRangePicker::new("a", "b").id(nasty).today(today)),
        html(TimePicker::new("t").id(nasty)),
    ];
    for page in pages {
        assert!(!page.contains("<meta"), "{page}");
        assert!(!page.contains("<input name=role"), "{page}");
        assert!(!page.contains("<a id=planted"), "{page}");
    }
}
