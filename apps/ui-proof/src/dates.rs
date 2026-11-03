//! Calendars, date pickers, range pickers and time pickers in their states, a form
//! with server checks, and a report whose window lives in the URL.
//!
//! Today is 3 November 2026 on every page here, so a screenshot or a test never
//! depends on the clock.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{Datelike, NaiveDate, NaiveTime, Weekday};
use chrono_tz::Tz;
use http::Method;
use navree::http::extract::Query;
use navree::http::response::{IntoResponse, Response};
use navree::http::{Controller, RouteDef};
use navree::kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree::ui::dates::{
    Calendar, CalendarMode, CaptionLayout, DateLocale, DatePicker, DatePreset, DateRange,
    DateRangePicker, DateRangeQuery, DateRules, PickerStyle, RangeRules, TimePicker, TimeRules,
};
use navree::ui::form::{FormView, Rejected};

use crate::deals::DealRow;
use navree::ui::*;
use serde::Deserialize;

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, 3).unwrap_or(NaiveDate::MIN)
}

fn day(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap_or(NaiveDate::MIN)
}

fn time(h: u32, m: u32) -> NaiveTime {
    NaiveTime::from_hms_opt(h, m, 0).unwrap_or(NaiveTime::MIN)
}

fn section(title: &'static str, content: impl IntoNodes) -> Element {
    el("section")
        .attr("data-gallery", title)
        .children((Heading::h2(title), Stack::gap(3).children(content)))
}

fn rules() -> DateRules {
    DateRules::new()
        .min(day(2026, 10, 15))
        .max(day(2027, 2, 10))
        .disable_weekdays([Weekday::Sat, Weekday::Sun])
        .disable_dates([day(2026, 11, 11)])
        .disable_range(day(2026, 12, 24), day(2026, 12, 26))
        .disable_if(|d| d.day() == 13)
}

fn calendars() -> Element {
    let cal = |id: &'static str| Calendar::new(id).today(today());
    section(
        "Calendars",
        (
            Row::gap(4).wrap().children((
                cal("cal-single")
                    .name("cal_single")
                    .selected(day(2026, 11, 12))
                    .with_today()
                    .label("Single date"),
                cal("cal-multi")
                    .mode(CalendarMode::Multiple)
                    .name("cal_multi")
                    .selected_days([day(2026, 11, 3), day(2026, 11, 10)])
                    .max_count(3)
                    .label("Several dates, at most three"),
                cal("cal-range")
                    .mode(CalendarMode::Range)
                    .range_names("cal_from", "cal_to")
                    .selected_range(DateRange::new(day(2026, 11, 9), day(2026, 11, 12)))
                    .months(2)
                    .label("Date range"),
                cal("cal-limits")
                    .mode(CalendarMode::Range)
                    .range_names("lim_from", "lim_to")
                    .min_days(2)
                    .max_days(5)
                    .label("Range of two to five days"),
                cal("cal-rules")
                    .name("cal_rules")
                    .rules(rules())
                    .label("With rules"),
            )),
            Row::gap(4).wrap().children((
                cal("cal-weeks")
                    .name("cal_weeks")
                    .month(day(2026, 2, 1))
                    .week_numbers()
                    .fixed_weeks()
                    .week_start(Weekday::Mon)
                    .label("Week numbers"),
                cal("cal-dob")
                    .name("cal_dob")
                    .month(day(1990, 5, 1))
                    .caption(CaptionLayout::Dropdowns)
                    .max(today())
                    .label("Date of birth"),
                cal("cal-ar")
                    .name("cal_ar")
                    .locale(DateLocale::of("ar"))
                    .selected(day(2026, 11, 12)),
                cal("cal-de")
                    .name("cal_de")
                    .locale(DateLocale::of("de"))
                    .selected(day(2026, 11, 12))
                    .week_numbers(),
                cal("cal-sm")
                    .size(Size::Sm)
                    .name("cal_sm")
                    .selected(day(2026, 11, 12)),
                cal("cal-lg")
                    .size(Size::Lg)
                    .name("cal_lg")
                    .selected(day(2026, 11, 12)),
                cal("cal-ro").read_only().selected(day(2026, 11, 12)),
                cal("cal-off")
                    .disabled(true)
                    .name("cal_off")
                    .selected(day(2026, 11, 12)),
            )),
        ),
    )
}

fn dfield(
    name: &'static str,
    label: &'static str,
    build: impl FnOnce(DatePicker) -> DatePicker,
) -> Field {
    Field::new(format!("fld-{name}"), label).control(build(
        DatePicker::new(name)
            .id(format!("dp-{name}"))
            .today(today()),
    ))
}

fn pickers() -> Element {
    section(
        "Date pickers",
        Grid::cols(3).gap(4).children((
            dfield("basic", "Basic date", |p| p),
            dfield("value", "Date with a value", |p| {
                p.value(day(2026, 11, 12)).clearable().with_today()
            }),
            dfield("due", "Due date", DatePicker::required)
                .help("Type it as 11/30/2026, or pick it."),
            Field::new("fld-late", "Late date")
                .error("Choose a weekday in the next three months.")
                .control(
                    DatePicker::new("late")
                        .id("dp-late")
                        .today(today())
                        .value_text("garbage")
                        .invalid(true),
                ),
            dfield("disabled", "Disabled", |p| {
                p.value(day(2026, 11, 12)).disabled(true)
            }),
            dfield("readonly", "Read only", |p| {
                p.value(day(2026, 11, 12)).read_only()
            }),
            dfield("button", "Button style", |p| {
                p.style(PickerStyle::Button)
                    .clearable()
                    .placeholder("Pick a day")
            }),
            dfield("de", "Deutsch", |p| {
                p.locale(DateLocale::of("de")).value(day(2026, 11, 12))
            }),
            dfield("ar", "Arabic", |p| {
                p.locale(DateLocale::of("ar")).value(day(2026, 11, 12))
            }),
            dfield("ja", "Japanese", |p| {
                p.locale(DateLocale::of("ja")).value(day(2026, 11, 12))
            }),
            dfield("birth", "Date of birth", |p| {
                p.caption(CaptionLayout::Dropdowns)
                    .max(today())
                    .value(day(1990, 5, 17))
            }),
            dfield("rules", "Weekdays in the window", |p| {
                p.rules(
                    DateRules::new()
                        .min(day(2026, 11, 1))
                        .max(day(2026, 12, 31))
                        .disable_weekdays([Weekday::Sat, Weekday::Sun]),
                )
            }),
        )),
    )
}

fn ranges() -> Element {
    let dr = |from: &'static str, to: &'static str| {
        DateRangePicker::new(from, to)
            .id(format!("dr-{from}"))
            .today(today())
            .week_start(Weekday::Mon)
    };
    section(
        "Range pickers",
        Row::gap(4).wrap().children((
            dr("a_from", "a_to").title("Created"),
            dr("b_from", "b_to")
                .title("Created")
                .value(DateRange::new(day(2026, 11, 1), day(2026, 11, 3))),
            dr("c_from", "c_to")
                .confirm()
                .value(DateRange::new(day(2026, 10, 27), day(2026, 11, 3)))
                .title("Window"),
            dr("d_from", "d_to")
                .min(day(2026, 10, 15))
                .max(day(2026, 11, 20))
                .min_days(2)
                .max_days(10)
                .presets([
                    DatePreset::Today,
                    DatePreset::Last7Days,
                    DatePreset::AllTime,
                ])
                .title("Limited"),
            dr("e_from", "e_to")
                .locale(DateLocale::of("de"))
                .value(DateRange::new(day(2026, 11, 1), day(2026, 11, 3)))
                .preset_label(DatePreset::Today, "Heute")
                .title("Zeitraum"),
            dr("f_from", "f_to")
                .locale(DateLocale::of("ar"))
                .value(DateRange::new(day(2026, 11, 1), day(2026, 11, 3))),
        )),
    )
}

fn tfield(
    name: &'static str,
    label: &'static str,
    build: impl FnOnce(TimePicker) -> TimePicker,
) -> Field {
    Field::new(format!("fld-{name}"), label)
        .control(build(TimePicker::new(name).id(format!("tp-{name}"))))
}

fn times() -> Element {
    section(
        "Time pickers",
        Grid::cols(3).gap(4).children((
            tfield("twelve", "Twelve hour", |p| p.value(time(14, 30))),
            tfield("h24", "Twenty-four hour", |p| {
                p.locale(DateLocale::of("de")).value(time(14, 30))
            }),
            tfield("seconds", "With seconds", |p| {
                p.seconds()
                    .value(NaiveTime::from_hms_opt(14, 30, 15).unwrap_or(NaiveTime::MIN))
            }),
            tfield("hours", "Office hours", |p| {
                p.rules(
                    TimeRules::new()
                        .min(time(9, 0))
                        .max(time(17, 0))
                        .unavailable(time(12, 0), time(12, 59))
                        .step_minutes(15),
                )
            }),
            tfield("strict", "Strict quarter hours", |p| {
                p.rules(TimeRules::new().step_minutes(15).strict_step())
            }),
            tfield("zone", "In a zone", |p| {
                p.value(time(9, 0))
                    .zone_note("Times are in America/New_York (UTC-5 until 8 March).")
            }),
            tfield("clear", "Clearable", |p| p.value(time(8, 0)).clearable()),
            Field::new("fld-invalid", "Invalid")
                .error("Enter a time like 2:30 PM.")
                .control(
                    TimePicker::new("invalid")
                        .id("tp-invalid")
                        .value_text("soon")
                        .invalid(true),
                ),
            tfield("tdisabled", "Disabled", |p| {
                p.value(time(8, 0)).disabled(true)
            }),
            tfield("tja", "Japanese", |p| {
                p.locale(DateLocale::of("ja")).value(time(15, 5))
            }),
            tfield("tar", "Arabic", |p| {
                p.locale(DateLocale::of("ar")).value(time(15, 30))
            }),
            tfield("fine", "Every minute", |p| {
                p.rules(TimeRules::new().step_minutes(1))
            }),
        )),
    )
}

/// `GET /dates`.
pub fn page(shell: &AppShell) -> AppPage {
    shell
        .page("Dates")
        .body(Stack::gap(8).children((calendars(), pickers(), ranges(), times())))
}

/// A booking: dates and a time. The rules are named once, on the fields: the pickers draw
/// them for the browser and the server holds the posted values to them, with no rule of
/// the type's own.
#[derive(Debug, Deserialize, Validate, Form)]
pub struct Booking {
    /// Start date
    #[validate(skip)]
    #[form(
        widget = date_picker,
        date_rules = booking_rules,
        help = "A weekday in the next three months."
    )]
    pub start: NaiveDate,
    /// End date
    #[validate(skip)]
    #[form(widget = date_picker)]
    pub end: Option<NaiveDate>,
    /// Arrives at
    #[validate(skip)]
    #[form(
        widget = time_picker,
        time_rules = office_hours,
        help = "Between 9:00 and 17:00, not at lunch."
    )]
    pub arrives: NaiveTime,
    /// Note
    #[validate(length(max = 40))]
    pub note: String,
}

fn booking_rules() -> DateRules {
    DateRules::new()
        .min(day(2026, 11, 3))
        .max(day(2027, 2, 3))
        .disable_weekdays([Weekday::Sat, Weekday::Sun])
}

fn office_hours() -> TimeRules {
    TimeRules::new()
        .min(time(9, 0))
        .max(time(17, 0))
        .unavailable(time(12, 0), time(12, 59))
}

fn booking_page(shell: &AppShell, form: FormView<Booking>) -> AppPage {
    shell.page("Booking").body(
        Card::titled("New booking").body(
            form.post("/dates/form")
                .submit("Book")
                .dates(DateLocale::english(), today()),
        ),
    )
}

/// `GET /dates/form`.
pub fn form_page(shell: &AppShell) -> AppPage {
    booking_page(shell, FormView::new())
}

/// `POST /dates/form`.
pub fn form_store(
    shell: &AppShell,
    submit: Result<Submit<Booking>, Rejected<Booking>>,
) -> Response {
    match submit {
        Ok(Submit(booking)) => Reply::redirect(format!(
            "/dates/booked?start={}&end={}&arrives={}",
            booking.start,
            booking.end.map(|d| d.to_string()).unwrap_or_default(),
            booking.arrives.format("%H:%M:%S"),
        ))
        .toast(ToastTone::Success, "Booked")
        .into_response(),
        Err(rejected) => rejected.respond(|form| booking_page(shell, form)),
    }
}

/// `GET /dates/booked`: what the booking saved.
pub fn booked(shell: &AppShell, params: &HashMap<String, String>) -> AppPage {
    let get = |key: &str| params.get(key).cloned().unwrap_or_default();
    shell
        .page("Booked")
        .body(el("dl").attr("data-saved", "").children((
            el("dt").child("start"),
            el("dd").attr("data-field", "start").child(get("start")),
            el("dt").child("end"),
            el("dd").attr("data-field", "end").child(get("end")),
            el("dt").child("arrives"),
            el("dd").attr("data-field", "arrives").child(get("arrives")),
        )))
}

/// The windows a report takes, written once: the picker draws them for the browser, and the
/// server holds the address to the same value.
fn window_limits() -> RangeRules {
    RangeRules::new(DateRules::new().min(day(2020, 1, 1)).max(day(2027, 12, 31))).max_days(400)
}

/// `GET /dates/report`: the window is in the address, and a window the limits refuse is
/// said to be refused, not drawn.
pub fn report(shell: &AppShell, query: &DateRangeQuery) -> AppPage {
    let limits = window_limits();
    let (range, refused) = match query.checked(&limits) {
        Ok(range) => (range, None),
        Err(error) => (None, Some(error.message(&DateLocale::english()))),
    };
    let zone: Tz = "America/New_York".parse().unwrap_or(Tz::UTC);
    let mut facts: Vec<Element> = match range {
        Some(range) => {
            let (start, end) = range.utc_bounds(&zone);
            vec![
                el("dt").child("Window"),
                el("dd")
                    .attr("data-report", "window")
                    .child(format!("{} to {}", range.from, range.to)),
                el("dt").child("Days"),
                el("dd")
                    .attr("data-report", "days")
                    .child(range.day_count().to_string()),
                el("dt").child("Query"),
                el("dd").attr("data-report", "query").child(format!(
                    "created_at >= {} AND created_at < {}",
                    start.format("%Y-%m-%dT%H:%M:%SZ"),
                    end.format("%Y-%m-%dT%H:%M:%SZ")
                )),
            ]
        }
        None => vec![
            el("dt").child("Window"),
            el("dd").attr("data-report", "window").child("none"),
        ],
    };
    if let Some(message) = refused {
        facts.push(el("dt").child("Refused"));
        facts.push(el("dd").attr("data-report", "refused").child(message));
    }
    shell.page("Report").body(
        Region::new("report").children(
            el("form")
                .attr("id", "report-form")
                .attr("method", "get")
                .attr("action", "/dates/report")
                .attr("data-ui-target", "report")
                .children((
                    DateRangePicker::new("from", "to")
                        .id("report-range")
                        .today(today())
                        .week_start(Weekday::Mon)
                        .title("Created")
                        .limits(limits)
                        .value_opt(range),
                    Button::primary("Apply").submit(),
                    el("dl").attr("data-report", "").children(facts),
                )),
        ),
    )
}

/// `GET /dates/month`: one calendar, set by the query, for comparing with the
/// browser's own drawing of the same month.
pub fn month(params: &HashMap<String, String>) -> View {
    let get = |key: &str| params.get(key).map(String::as_str);
    let when = get("month")
        .and_then(|m| NaiveDate::parse_from_str(&format!("{m}-01"), "%Y-%m-%d").ok())
        .unwrap_or(day(2026, 11, 1));
    let mut calendar = Calendar::new("probe")
        .today(today())
        .month(when)
        .locale(DateLocale::of(get("loc").unwrap_or("en")))
        .months(get("n").and_then(|n| n.parse().ok()).unwrap_or(1))
        .selected(day(2026, 11, 12));
    if let Some(start) = get("ws").and_then(|n| n.parse::<u8>().ok()) {
        calendar = calendar.week_start(match start {
            1 => Weekday::Mon,
            2 => Weekday::Tue,
            3 => Weekday::Wed,
            4 => Weekday::Thu,
            5 => Weekday::Fri,
            6 => Weekday::Sat,
            _ => Weekday::Sun,
        });
    }
    if get("wn").is_some() {
        calendar = calendar.week_numbers();
    }
    if get("fixed").is_some() {
        calendar = calendar.fixed_weeks();
    }
    if get("dd").is_some() {
        calendar = calendar.caption(CaptionLayout::Dropdowns);
    }
    if get("rules").is_some() {
        calendar = calendar.rules(rules());
    }
    View::new("Month").body(
        el("main")
            .slot("page")
            .children((calendar, Button::new("After").attr("id", "after"))),
    )
}

/// `GET /dates/probe`: a date picker and a time picker in the locale the query names,
/// for typing at in any language.
pub fn probe(params: &HashMap<String, String>) -> View {
    let locale = DateLocale::of(params.get("loc").map_or("en", String::as_str));
    let rules = DateRules::new()
        .min(day(2026, 10, 1))
        .max(day(2027, 12, 31));
    View::new("Probe").body(
        el("main")
            .slot("page")
            .attr("dir", if locale.rtl { "rtl" } else { "ltr" })
            .child(
                el("form")
                    .attr("id", "probe")
                    .attr("method", "get")
                    .attr("action", "/dates/probe")
                    .children((
                        DatePicker::new("d")
                            .id("probe-date")
                            .label("Date")
                            .locale(locale.clone())
                            .today(today())
                            .rules(rules),
                        TimePicker::new("t")
                            .id("probe-time")
                            .label("Time")
                            .locale(locale),
                        Button::primary("Send").submit(),
                    )),
            ),
    )
}

/// An id as a page might build one from a request: it closes the attribute, then adds a tag
/// that sends the page to another site and a field that would post a role.
const HOSTILE: &str = "x\"><meta http-equiv=refresh content=\"0;url=https://example.com/\"><input name=role value=admin><a id=planted href=//evil.example/>x</a>";

/// `GET /dates/hostile`: every kind of picker and calendar with a hostile id, for the
/// browser to draw months and lists from.
pub fn hostile() -> View {
    let cal = |suffix: &str| Calendar::new(format!("{HOSTILE}{suffix}")).today(today());
    View::new("Hostile ids").body(
        el("main").slot("page").child(
            el("form")
                .attr("id", "hostile")
                .attr("method", "get")
                .attr("action", "/dates/hostile")
                .children((
                    cal("-one").name("cal_one").selected(day(2026, 11, 12)),
                    cal("-two").name("cal_two").months(2),
                    cal("-dd").name("cal_dd").caption(CaptionLayout::Dropdowns),
                    cal("-range")
                        .mode(CalendarMode::Range)
                        .range_names("r_from", "r_to"),
                    DatePicker::new("due")
                        .id(HOSTILE)
                        .today(today())
                        .value(day(2026, 11, 12)),
                    DateRangePicker::new("from", "to")
                        .id(format!("{HOSTILE}-r"))
                        .today(today()),
                    TimePicker::new("at")
                        .id(format!("{HOSTILE}-t"))
                        .value(time(14, 30)),
                    Button::primary("Send").submit(),
                )),
        ),
    )
}

/// What the server says of a posted date or time under rules: `ok` or its own words.
fn said_of(name: &str, params: &HashMap<String, String>) -> Option<(String, String)> {
    let text = params.get(name)?;
    let english = DateLocale::english();
    let verdict = match name {
        "due" => match text.parse::<NaiveDate>() {
            Ok(day) => stale_dates().check(day).map_or_else(
                |e| format!("refused: {}", e.message(&english)),
                |()| "ok".to_owned(),
            ),
            Err(_) => "refused: not a date".to_owned(),
        },
        _ => match NaiveTime::parse_from_str(text, "%H:%M")
            .or_else(|_| NaiveTime::parse_from_str(text, "%H:%M:%S"))
        {
            Ok(at) => (match name {
                "at" => stale_time(),
                "quarter" => quarter_time(),
                _ => stepped_time(),
            })
            .check(at)
            .map_or_else(
                |e| format!("refused: {}", e.message(&english)),
                |()| "ok".to_owned(),
            ),
            Err(_) => "refused: not a time".to_owned(),
        },
    };
    Some((format!("{name}={text}"), verdict))
}

fn stale_dates() -> DateRules {
    DateRules::new().min(day(2026, 11, 3))
}

fn stale_time() -> TimeRules {
    TimeRules::new().min(time(9, 0))
}

/// Quarter hours with no least time: a stored 9:07 is off the step, and a native field that
/// counted the step from its own value would take 9:22 and refuse 9:15.
fn quarter_time() -> TimeRules {
    TimeRules::new().step_minutes(15).strict_step()
}

fn stepped_time() -> TimeRules {
    TimeRules::new()
        .min(time(9, 10))
        .step_minutes(15)
        .strict_step()
}

/// `GET /dates/stale`: values stored before their rules changed (a date before the earliest
/// allowed, a time before the opening), and a time picker whose least time is not on its
/// step. A form with these must say so at the box, not stop with no word said; and what it
/// posts is checked by the server under the same rules, shown below it.
pub fn stale(params: &HashMap<String, String>) -> View {
    let results: Vec<Element> = ["due", "at", "stepped", "quarter"]
        .iter()
        .filter_map(|name| said_of(name, params))
        .map(|(posted, verdict)| {
            el("li")
                .attr("data-result", "")
                .child(format!("{posted} {verdict}"))
        })
        .collect();
    View::new("Stored values").body(
        el("main").slot("page").children((
            el("form")
                .attr("id", "stale")
                .attr("method", "get")
                .attr("action", "/dates/stale")
                .children((
                    Field::new("fld-due", "Due").control(
                        DatePicker::new("due")
                            .id("dp-stale-due")
                            .today(today())
                            .rules(stale_dates())
                            .value(day(2026, 10, 1)),
                    ),
                    Field::new("fld-at", "Opens at").control(
                        TimePicker::new("at")
                            .id("tp-stale-at")
                            .rules(stale_time())
                            .value(time(8, 0)),
                    ),
                    Field::new("fld-stepped", "Quarter hours from 9:10").control(
                        TimePicker::new("stepped")
                            .id("tp-stale-step")
                            .rules(stepped_time())
                            .value(time(9, 15)),
                    ),
                    Field::new("fld-quarter", "Quarter hours, any start").control(
                        TimePicker::new("quarter")
                            .id("tp-stale-quarter")
                            .rules(quarter_time())
                            .value(time(9, 7)),
                    ),
                    Button::primary("Send").submit(),
                )),
            el("ul").attr("id", "results").children(results),
        )),
    )
}

/// A window a page owns: two parameters in the table's address, and a range picker in
/// its toolbar. The table keeps them in every link and doesn't query them.
#[derive(Default, Deserialize)]
pub struct Window {
    pub from: Option<String>,
    pub to: Option<String>,
}

impl Window {
    fn range(&self) -> Option<DateRange> {
        DateRangeQuery {
            from: self.from.clone(),
            to: self.to.clone(),
        }
        .range_within(&window_limits())
    }
}

impl navree::ui::table::FilterBar for Window {
    fn params(&self) -> Vec<(&'static str, String)> {
        match self.range() {
            Some(range) => vec![
                ("from", range.from.to_string()),
                ("to", range.to.to_string()),
            ],
            None => Vec::new(),
        }
    }

    fn controls(&self) -> Vec<Node> {
        vec![
            DateRangePicker::new("from", "to")
                .id("table-window")
                .today(today())
                .week_start(Weekday::Mon)
                .title("Updated")
                .limits(window_limits())
                .value_opt(self.range())
                .into(),
        ]
    }
}

static ROWS: std::sync::LazyLock<Vec<DealRow>> =
    std::sync::LazyLock::new(|| crate::deals::rows(1000));

/// `GET /dates/table`: the deals updated in the window, paged in the address.
pub fn table(shell: &AppShell, query: &TableQuery<DealRow, Window>) -> AppPage {
    let range = query.filter().range();
    let mut found: Vec<&DealRow> = ROWS
        .iter()
        .filter(|row| range.is_none_or(|range| range.contains(row.updated_at)))
        .collect();
    if let Some((column, direction)) = query.sort() {
        match column {
            "title" => found.sort_by(|a, b| a.title.cmp(&b.title)),
            "amount" => found.sort_by_key(|row| row.amount),
            _ => found.sort_by_key(|row| row.updated_at),
        }
        if direction == navree::ui::table::Direction::Desc {
            found.reverse();
        }
    }
    let total = found.len() as u64;
    let skip = usize::try_from(query.offset()).unwrap_or(usize::MAX);
    let take = query.per_page() as usize;
    let page: Vec<DealRow> = found.into_iter().skip(skip).take(take).cloned().collect();
    shell.page("Deals by window").body(
        DataTable::new("window-deals", query, &page)
            .total(total)
            .caption("Deals updated in the window"),
    )
}

pub struct DatePages;

impl Injectable for DatePages {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for DatePages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "!/dates", |shell: AppShell| async move {
                page(&shell)
            })
            .public(),
            RouteDef::on(Method::GET, "!/dates/form", |shell: AppShell| async move {
                form_page(&shell)
            })
            .public(),
            RouteDef::on(
                Method::POST,
                "!/dates/form",
                |shell: AppShell, submit: Result<Submit<Booking>, Rejected<Booking>>| async move {
                    form_store(&shell, submit)
                },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "!/dates/table",
                |shell: AppShell, query: TableQuery<DealRow, Window>| async move {
                    table(&shell, &query)
                },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "!/dates/booked",
                |shell: AppShell, Query(params): Query<HashMap<String, String>>| async move {
                    booked(&shell, &params)
                },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "!/dates/report",
                |shell: AppShell, Query(query): Query<DateRangeQuery>| async move {
                    report(&shell, &query)
                },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "!/dates/probe",
                |Query(params): Query<HashMap<String, String>>| async move { probe(&params) },
            )
            .public(),
            RouteDef::on(Method::GET, "!/dates/hostile", || async move { hostile() }).public(),
            RouteDef::on(
                Method::GET,
                "!/dates/stale",
                |Query(params): Query<HashMap<String, String>>| async move { stale(&params) },
            )
            .public(),
            RouteDef::on(
                Method::GET,
                "!/dates/month",
                |Query(params): Query<HashMap<String, String>>| async move { month(&params) },
            )
            .public(),
        ]
    }
}
