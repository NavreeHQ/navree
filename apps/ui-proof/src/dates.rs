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
    DateRangePicker, DateRules, PickerStyle, TimePicker, TimeRules,
};
use navree::ui::form::{FormView, Rejected};
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

fn pickers() -> Element {
    let dp = |name: &'static str, label: &'static str| {
        DatePicker::new(name)
            .id(format!("dp-{name}"))
            .today(today())
            .label(label)
    };
    let field = |id: &'static str, label: &'static str, control: DatePicker| {
        Field::new(id, label).control(control)
    };
    section(
        "Date pickers",
        Grid::cols(3).gap(4).children((
            dp("basic", "Basic date"),
            dp("value", "Date with a value")
                .value(day(2026, 11, 12))
                .clearable()
                .with_today(),
            field(
                "due",
                "Due date",
                DatePicker::new("due")
                    .id("dp-due")
                    .today(today())
                    .required(),
            )
            .help("Type it as 11/30/2026, or pick it."),
            Field::new("late", "Late date")
                .error("Choose a weekday in the next three months.")
                .control(
                    DatePicker::new("late")
                        .id("dp-late")
                        .today(today())
                        .value_text("garbage")
                        .invalid(true),
                ),
            dp("disabled", "Disabled")
                .value(day(2026, 11, 12))
                .disabled(true),
            dp("readonly", "Read only")
                .value(day(2026, 11, 12))
                .read_only(),
            dp("button", "Button style")
                .style(PickerStyle::Button)
                .clearable()
                .placeholder("Pick a day"),
            dp("de", "Deutsch")
                .locale(DateLocale::of("de"))
                .value(day(2026, 11, 12)),
            dp("ar", "Arabic")
                .locale(DateLocale::of("ar"))
                .value(day(2026, 11, 12)),
            dp("ja", "Japanese")
                .locale(DateLocale::of("ja"))
                .value(day(2026, 11, 12)),
            dp("birth", "Date of birth")
                .caption(CaptionLayout::Dropdowns)
                .max(today())
                .value(day(1990, 5, 17)),
            dp("rules", "Weekdays in the window").rules(
                DateRules::new()
                    .min(day(2026, 11, 1))
                    .max(day(2026, 12, 31))
                    .disable_weekdays([Weekday::Sat, Weekday::Sun]),
            ),
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

fn times() -> Element {
    let tp = |name: &'static str, label: &'static str| {
        TimePicker::new(name).id(format!("tp-{name}")).label(label)
    };
    section(
        "Time pickers",
        Grid::cols(3).gap(4).children((
            tp("twelve", "Twelve hour").value(time(14, 30)),
            tp("de", "Twenty-four hour")
                .locale(DateLocale::of("de"))
                .value(time(14, 30)),
            tp("seconds", "With seconds")
                .seconds()
                .value(NaiveTime::from_hms_opt(14, 30, 15).unwrap_or(NaiveTime::MIN)),
            tp("hours", "Office hours").rules(
                TimeRules::new()
                    .min(time(9, 0))
                    .max(time(17, 0))
                    .unavailable(time(12, 0), time(12, 59))
                    .step_minutes(15),
            ),
            tp("strict", "Strict quarter hours")
                .rules(TimeRules::new().step_minutes(15).strict_step()),
            tp("zone", "In a zone")
                .value(time(9, 0))
                .zone_note("Times are in America/New_York (UTC-5 until 8 March)."),
            tp("clear", "Clearable").value(time(8, 0)).clearable(),
            tp("invalid", "Invalid").value_text("soon").invalid(true),
            tp("disabled", "Disabled").value(time(8, 0)).disabled(true),
            tp("ja", "Japanese")
                .locale(DateLocale::of("ja"))
                .value(time(15, 5)),
            tp("ar", "Arabic")
                .locale(DateLocale::of("ar"))
                .value(time(15, 30)),
            tp("fine", "Every minute").rules(TimeRules::new().step_minutes(1)),
        )),
    )
}

/// `GET /dates`.
pub fn page(shell: &AppShell) -> AppPage {
    shell
        .page("Dates")
        .body(Stack::gap(8).children((calendars(), pickers(), ranges(), times())))
}

/// A booking: dates and a time, checked by the server.
#[derive(Debug, Deserialize, Validate, Form)]
pub struct Booking {
    /// Start date
    #[validate(custom(open_day))]
    #[form(widget = date_picker, help = "A weekday in the next three months.")]
    pub start: NaiveDate,
    /// End date
    #[validate(skip)]
    #[form(widget = date_picker)]
    pub end: Option<NaiveDate>,
    /// Arrives at
    #[validate(custom(open_hours))]
    #[form(widget = time_picker, help = "Between 9:00 and 17:00, not at lunch.")]
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

fn open_day(day: &NaiveDate) -> Result<(), String> {
    booking_rules()
        .check(*day)
        .map_err(|error| error.message(&DateLocale::english()))
}

fn office_hours() -> TimeRules {
    TimeRules::new()
        .min(time(9, 0))
        .max(time(17, 0))
        .unavailable(time(12, 0), time(12, 59))
}

fn open_hours(at: &NaiveTime) -> Result<(), String> {
    office_hours()
        .check(*at)
        .map_err(|error| error.message(&DateLocale::english()))
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
        Ok(Submit(booking)) => View::new("Booked")
            .body(
                el("main").slot("page").children((
                    Heading::h1("Booked"),
                    el("dl").attr("data-saved", "").children((
                        el("dt").child("start"),
                        el("dd")
                            .attr("data-field", "start")
                            .child(booking.start.to_string()),
                        el("dt").child("end"),
                        el("dd")
                            .attr("data-field", "end")
                            .child(booking.end.map(|d| d.to_string()).unwrap_or_default()),
                        el("dt").child("arrives"),
                        el("dd")
                            .attr("data-field", "arrives")
                            .child(booking.arrives.to_string()),
                    )),
                )),
            )
            .into_response(),
        Err(rejected) => rejected.respond(|form| booking_page(shell, form)),
    }
}

/// `GET /dates/report`: the window is in the address.
pub fn report(shell: &AppShell, params: &HashMap<String, String>) -> AppPage {
    let from = params.get("from").map(String::as_str).unwrap_or_default();
    let to = params.get("to").map(String::as_str).unwrap_or_default();
    let range = DateRange::parse(from, to);
    let zone: Tz = "America/New_York".parse().unwrap_or(Tz::UTC);
    let facts: Vec<Element> = match range {
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
    View::new("Month").body(el("main").slot("page").child(calendar))
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
                "!/dates/report",
                |shell: AppShell, Query(params): Query<HashMap<String, String>>| async move {
                    report(&shell, &params)
                },
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
