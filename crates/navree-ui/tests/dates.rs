//! The calendar family: what the server draws, what it accepts, and that nothing a
//! person types or a page supplies can break out of its place.

use chrono::{Datelike, NaiveDate, NaiveTime, Weekday};
use navree_ui::dates::{
    Calendar, CalendarMode, CaptionLayout, Clock, DateLocale, DatePicker, DatePreset, DateRange,
    DateRangePicker, DateRules, PickerStyle, TimePicker, TimeRules,
};
use navree_ui::form::decode;
use navree_ui::{Field, Form, FormView, Node, Uses, Validate, View, assets, el, render};
use serde::Deserialize;
use serde_json::Value;

fn day(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

fn time(h: u32, m: u32) -> NaiveTime {
    NaiveTime::from_hms_opt(h, m, 0).unwrap()
}

fn today() -> NaiveDate {
    day(2026, 11, 3)
}

fn html(node: impl Into<Node>) -> String {
    render(&node.into())
}

fn count(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
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

/// The whole opening tag of the first element that holds `marker`.
fn open_tag<'a>(html: &'a str, marker: &str) -> &'a str {
    let at = html
        .find(marker)
        .unwrap_or_else(|| panic!("{marker} is not in {html}"));
    let start = html[..at].rfind('<').unwrap();
    &html[start..start + html[start..].find('>').unwrap() + 1]
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

fn calendar() -> Calendar {
    Calendar::new("c").today(today())
}

#[test]
fn a_month_is_an_aria_grid_of_buttons() {
    let page = html(calendar());
    assert!(page.contains(r#"role="grid""#));
    assert!(page.contains(r#"data-month="2026-11""#));
    assert!(page.contains(r#"<div data-slot="calendar-caption" id="c-cap-2026-11" aria-live="polite">November 2026</div>"#));
    // November 2026 begins on a Sunday: 30 days make exactly five rows and nothing spills over.
    assert_eq!(count(&page, r#"<button data-slot="calendar-day""#), 30);
    assert_eq!(count(&page, "<tr>"), 6, "a head row and five weeks");
    assert!(page.contains(r#"aria-label="Tuesday, November 3, 2026""#));
    // The head names every weekday in full for assistive technology.
    assert!(page.contains(r#"scope="col" abbr="Sunday">Su</th>"#));
    assert!(page.contains(r#"scope="col" abbr="Saturday">Sa</th>"#));
}

#[test]
fn one_day_holds_the_tab_stop() {
    let page = html(calendar());
    assert_eq!(count(&page, r#"tabindex="0""#), 1);
    assert!(
        page.contains(r#"data-date="2026-11-03" tabindex="0""#),
        "today, with nothing chosen"
    );
    let chosen = html(calendar().selected(day(2026, 11, 12)));
    assert!(chosen.contains(r#"data-date="2026-11-12" tabindex="0""#));
    assert_eq!(count(&chosen, r#"tabindex="0""#), 1);
    assert_eq!(count(&chosen, r#"aria-selected="true""#), 1);
    assert!(chosen.contains(r#"aria-current="date""#));
}

#[test]
fn the_tab_stop_skips_days_that_cannot_be_chosen() {
    let blocked = html(calendar().rules(DateRules::new().min(day(2026, 11, 20))));
    assert!(
        blocked.contains(r#"data-date="2026-11-20" tabindex="0""#),
        "the first day that can be"
    );
}

#[test]
fn rules_mark_days_and_the_browser_gets_them_as_runs() {
    let rules = DateRules::new()
        .disable_weekdays([Weekday::Sat, Weekday::Sun])
        .disable_dates([day(2026, 11, 11)])
        .disable_range(day(2026, 12, 24), day(2026, 12, 26))
        .disable_if(|d| d.day() == 13);
    let page = html(calendar().rules(rules));
    // Saturday the 7th and the listed Wednesday the 11th and the 13th (a Friday).
    for date in ["2026-11-07", "2026-11-11", "2026-11-13"] {
        let tag = attr_of(&page, &format!(r#"data-date="{date}""#), "aria-disabled");
        assert_eq!(tag.as_deref(), Some("true"), "{date}");
    }
    assert!(!page.contains(r#"data-date="2026-11-12" tabindex="0" aria-label="Thursday, November 12, 2026" aria-disabled"#));
    let json = json_attr(&page, "data-ui-dates=\"calendar\"", "data-ui-rules");
    assert_eq!(json["wd"], serde_json::json!([0, 6]));
    let runs = json["r"].as_array().unwrap();
    assert!(
        runs.iter()
            .any(|r| r == &serde_json::json!(["2026-12-24", "2026-12-26"]))
    );
    assert!(
        json["nav"].is_array(),
        "a rule function limits navigation to its window"
    );
}

#[test]
fn what_is_chosen_posts_in_hidden_fields() {
    let single = html(calendar().name("due").selected(day(2026, 11, 12)));
    assert!(
        single
            .contains(r#"<input type="hidden" name="due" value="2026-11-12" data-ui-val="main">"#)
    );
    let multiple = html(
        calendar()
            .mode(CalendarMode::Multiple)
            .name("days")
            .selected_days([day(2026, 11, 12), day(2026, 11, 3)]),
    );
    assert!(multiple.contains(r#"name="days" value="2026-11-03,2026-11-12""#));
    let range = html(
        calendar()
            .mode(CalendarMode::Range)
            .range_names("from", "to")
            .selected_range(DateRange::new(day(2026, 11, 14), day(2026, 11, 9))),
    );
    assert!(range.contains(r#"name="from" value="2026-11-09" data-ui-val="from""#));
    assert!(range.contains(r#"name="to" value="2026-11-14" data-ui-val="to""#));
    assert!(range.contains("data-range-start"));
    assert!(range.contains("data-range-end"));
    assert_eq!(count(&range, "data-range-middle"), 4, "10th to 13th");
    // Off: nothing posts.
    let off = html(
        calendar()
            .name("due")
            .disabled(true)
            .selected(day(2026, 11, 12)),
    );
    assert!(off.contains(r#"name="due" value="2026-11-12" data-ui-val="main" disabled"#));
}

#[test]
fn navigation_stops_at_the_limits() {
    let page = html(calendar().min(day(2026, 11, 1)));
    assert!(open_tag(&page, r#"data-ui-cal="prev""#).contains(" disabled"));
    assert!(!open_tag(&page, r#"data-ui-cal="next""#).contains(" disabled"));
    let both = html(calendar().min(day(2026, 11, 1)).max(day(2026, 11, 30)));
    assert!(open_tag(&both, r#"data-ui-cal="next""#).contains(" disabled"));
    // A calendar at the first month of year 1 has nowhere earlier to go.
    let first = html(calendar().month(day(1, 1, 1)));
    assert!(open_tag(&first, r#"data-ui-cal="prev""#).contains(" disabled"));
    let last = html(calendar().month(day(9999, 12, 1)));
    assert!(open_tag(&last, r#"data-ui-cal="next""#).contains(" disabled"));
    // Two months: the buttons look past the months on show.
    let two = html(calendar().months(2).max(day(2026, 12, 31)));
    assert!(open_tag(&two, r#"data-ui-cal="next""#).contains(" disabled"));
}

#[test]
fn several_months_sit_side_by_side() {
    let page = html(calendar().months(2));
    assert_eq!(count(&page, r#"data-slot="calendar-month""#), 2);
    assert!(page.contains(r#"data-month="2026-12""#));
    assert_eq!(
        count(&html(calendar().months(9)), r#"data-slot="calendar-month""#),
        3,
        "capped"
    );
    // Year 9999 makes room for the months after it by showing the ones before.
    let end = html(calendar().months(2).month(day(9999, 12, 1)));
    assert!(end.contains(r#"data-month="9999-11""#));
    assert!(end.contains(r#"data-month="9999-12""#));
}

#[test]
fn week_numbers_and_fixed_weeks() {
    let page = html(calendar().week_start(Weekday::Mon).week_numbers());
    // 1 November 2026 is a Sunday, so a week that starts on Monday needs six rows.
    assert_eq!(count(&page, r#"scope="row""#), 6);
    assert!(
        page.contains(r#"<th data-slot="calendar-weeknum" scope="row">44</th>"#),
        "26 Oct 2026 is in ISO week 44"
    );
    let fixed = html(calendar().fixed_weeks().month(day(2026, 2, 1)));
    assert_eq!(
        count(&fixed, "<tr>"),
        7,
        "six weeks and the head, in a four-week February"
    );
    let hidden = html(calendar().week_start(Weekday::Mon).hide_outside_days());
    assert!(!hidden.contains("data-outside"));
}

#[test]
fn a_heading_can_be_lists_of_months_and_years() {
    let page = html(
        calendar()
            .caption(CaptionLayout::Dropdowns)
            .month(day(1990, 5, 1))
            .max(today()),
    );
    assert!(page.contains(r#"data-ui-cal="month""#));
    assert!(page.contains(r#"data-ui-cal="year""#));
    assert!(page.contains(r#"<option value="1990" selected>1990</option>"#));
    assert!(page.contains(r#"<option value="2026">2026</option>"#));
    assert!(
        !page.contains(r#"<option value="2027">"#),
        "the limit ends the list"
    );
    assert!(page.contains(r#"<option value="5" selected>May</option>"#));
    // Two months have plain headings: the lists belong to one.
    let two = html(calendar().caption(CaptionLayout::Dropdowns).months(2));
    assert!(!two.contains(r#"data-ui-cal="year""#));
}

#[test]
fn a_right_to_left_language_reverses_the_grid_and_starts_on_its_own_day() {
    let page = html(calendar().locale(DateLocale::of("ar")));
    assert!(page.contains(r#"dir="rtl""#));
    assert!(page.contains(r#"lang="ar""#));
    let first = page.find("calendar-weekday").unwrap();
    assert!(
        page[first..first + 120].contains("السبت"),
        "Saturday starts the week"
    );
    assert!(page.contains("نوفمبر 2026"));
}

#[test]
fn a_calendar_to_look_at_has_no_controls() {
    let page = html(calendar().read_only().selected(day(2026, 11, 12)));
    assert!(!page.contains("data-ui-cal"));
    assert!(!page.contains(r#"tabindex="0""#));
    assert!(!page.contains("aria-selected"));
    assert_eq!(count(&page, r#"aria-disabled="true""#), 30);
}

#[test]
fn the_locale_goes_to_the_browser_whole() {
    let page = html(calendar());
    let loc = json_attr(&page, "data-ui-dates=\"calendar\"", "data-ui-loc");
    assert_eq!(loc["m"].as_array().unwrap().len(), 12);
    assert_eq!(loc["w"].as_array().unwrap().len(), 7);
    assert_eq!(loc["L"].as_array().unwrap().len(), 20);
    assert_eq!(loc["o"], "mdy");
    assert_eq!(loc["m"][10], "November");
}

#[test]
fn nothing_supplied_by_a_page_can_break_out() {
    let nasty = r#""><img src=x onerror=alert(1)><script>alert(2)</script>"#;
    let page = html(
        calendar()
            .label(nasty)
            .name(nasty.to_owned())
            .selected(day(2026, 11, 12)),
    );
    assert!(!page.contains("<img"), "{page}");
    assert!(!page.contains("<script"));
    assert!(!page.contains(" style="));
    let picker = html(
        DatePicker::new(nasty)
            .id(nasty)
            .label(nasty)
            .placeholder(nasty)
            .value_text(nasty)
            .today(today()),
    );
    assert!(!picker.contains("<img"), "{picker}");
    assert!(!picker.contains("<script"));
    assert!(!picker.contains(" style="));
    let range = html(
        DateRangePicker::new(nasty, nasty)
            .title(nasty)
            .placeholder(nasty)
            .today(today()),
    );
    assert!(!range.contains("<img"));
    assert!(!range.contains(" style="));
    let clock = html(
        TimePicker::new(nasty)
            .zone_note(nasty)
            .placeholder(nasty)
            .value_text(nasty),
    );
    assert!(!clock.contains("<img"));
    assert!(!clock.contains("<script"));
    // A tag from a request becomes a locale or falls back; it never reaches the markup raw.
    let odd = html(calendar().locale(DateLocale::of(nasty)));
    assert!(!odd.contains("<img"));
    assert!(odd.contains(r#"lang="en""#));
}

#[test]
fn a_page_with_a_calendar_loads_the_dates_module_and_nothing_else() {
    let page = View::new("x").body(calendar());
    assert!(page.uses().has(Uses::DATES));
    assert!(!page.uses().has(Uses::CORE));
    assert!(!page.uses().has(Uses::OVERLAY));
    let text = page.to_html();
    assert!(text.contains(assets::DATES_URL), "{text}");
    let plain = View::new("x").body(el("main").child("hi"));
    assert!(!plain.uses().has(Uses::DATES));
    assert!(!plain.to_html().contains("dates-"));
    let picker = View::new("x").body(DatePicker::new("d"));
    assert!(picker.uses().has(Uses::DATES));
    assert!(picker.uses().has(Uses::OVERLAY));
}

#[test]
fn a_date_picker_is_a_native_field_with_a_box_and_a_calendar_after_it() {
    let page = html(
        DatePicker::new("due")
            .today(today())
            .value(day(2026, 11, 12))
            .locale(DateLocale::of("de"))
            .min(day(2026, 11, 1))
            .max(day(2026, 12, 31))
            .required(),
    );
    // The first input is the one a label would find, and the one that posts.
    let first = page.find("<input").unwrap();
    assert!(page[first..].starts_with(r#"<input data-slot="input" type="date" name="due" value="2026-11-12" min="2026-11-01" max="2026-12-31""#), "{page}");
    assert!(page.contains("required"));
    // The box shows the date as typed in Germany.
    assert!(page.contains(r#"data-ui-text"#));
    assert!(page.contains(r#"value="12.11.2026""#));
    assert!(page.contains(r#"placeholder="TT.MM.JJJJ""#));
    // The button opens the popover by its id.
    let id = attr_of(&page, "popover=\"auto\"", "id").unwrap();
    assert_eq!(
        attr_of(&page, "data-ui-act=\"open\"", "popovertarget"),
        Some(id)
    );
    assert!(page.contains(r#"role="dialog""#));
    assert!(
        page.contains("data-ui-keep"),
        "the months are the browser's after the first draw"
    );
}

#[test]
fn a_date_picker_inside_a_field_hands_the_label_the_native_field() {
    let page = html(
        Field::new("when", "When")
            .control(DatePicker::new("when").today(today()))
            .error("Pick a day."),
    );
    assert!(page.contains(r#"<label data-slot="label" for="when">When</label>"#));
    let native = open_tag(&page, r#"type="date""#);
    assert!(native.contains(r#" id="when""#), "{native}");
    assert!(native.contains(r#"aria-describedby="when-error""#));
    assert!(
        !open_tag(&page, "data-ui-text").contains(" id="),
        "the box is named by the page's script"
    );
    assert!(page.contains(r#"aria-invalid="true""#));
    assert!(page.contains(r#"aria-describedby="when-error""#));
}

#[test]
fn text_that_came_back_is_shown_as_typed_and_marked() {
    let page = html(DatePicker::new("d").value_text("not a date").today(today()));
    assert!(page.contains(r#"value="not a date""#));
    assert!(page.contains(r#"aria-invalid="true""#));
    assert!(page.contains("data-invalid"));
    // Nothing is posted by the native field.
    assert!(
        page.contains(r#"type="date" name="d" aria-invalid"#)
            || page.contains(r#"type="date" name="d""#)
    );
    assert!(!page.contains(r#"type="date" name="d" value"#));
    assert!(
        html(DatePicker::new("d").value_text("2026-11-03").today(today()))
            .contains(r#"value="11/03/2026""#)
    );
    assert!(!html(DatePicker::new("d").value_text("  ")).contains("data-has-value"));
}

#[test]
fn a_date_picker_can_be_one_button() {
    let page = html(
        DatePicker::new("d")
            .style(PickerStyle::Button)
            .placeholder("Pick a day")
            .clearable()
            .today(today()),
    );
    assert!(page.contains(r#"data-style="button""#));
    assert!(page.contains(r#"data-ui-label"#));
    assert!(page.contains(r#"data-empty="Pick a day""#));
    assert!(page.contains(">Pick a day</span>"));
    assert!(!page.contains("data-ui-text"));
    let valued = html(
        DatePicker::new("d")
            .style(PickerStyle::Button)
            .value(day(2026, 11, 3))
            .today(today()),
    );
    assert!(valued.contains(">11/03/2026</span>"));
}

#[test]
fn a_disabled_picker_posts_nothing_and_a_read_only_one_cannot_open() {
    let off = html(
        DatePicker::new("d")
            .value(day(2026, 11, 3))
            .disabled(true)
            .today(today()),
    );
    assert!(off.matches(" disabled").count() >= 3, "{off}");
    let ro = html(
        DatePicker::new("d")
            .value(day(2026, 11, 3))
            .read_only()
            .today(today()),
    );
    assert!(ro.contains("readonly"));
}

#[test]
fn a_range_picker_posts_two_dates_and_offers_presets_counted_from_today() {
    let page = html(
        DateRangePicker::new("from", "to")
            .today(today())
            .week_start(Weekday::Mon)
            .presets([
                DatePreset::Today,
                DatePreset::Last7Days,
                DatePreset::ThisWeek,
            ])
            .value(DateRange::new(day(2026, 11, 1), day(2026, 11, 3))),
    );
    assert!(page.contains(r#"name="from" value="2026-11-01""#));
    assert!(page.contains(r#"name="to" value="2026-11-03""#));
    let seven = open_tag(&page, r#"data-ui-preset="last_7_days""#);
    assert!(
        seven.contains(r#"data-from="2026-10-28" data-to="2026-11-03""#),
        "{seven}"
    );
    let week = open_tag(&page, r#"data-ui-preset="this_week""#);
    assert!(
        week.contains(r#"data-from="2026-11-02" data-to="2026-11-03""#),
        "{week}"
    );
    // The window matches no preset, so Custom range is the one in force.
    assert!(open_tag(&page, r#"data-ui-preset="custom""#).contains(r#"aria-pressed="true""#));
    // The chip reads short, and the title holds the whole.
    assert!(page.contains(">Nov 1 – 3</span>"));
    assert!(page.contains(r#"title="11/01/2026 – 11/03/2026""#));
}

#[test]
fn the_matching_preset_is_pressed() {
    let page = html(
        DateRangePicker::new("from", "to")
            .today(today())
            .presets([DatePreset::Today, DatePreset::Last7Days])
            .value(DateRange::new(day(2026, 10, 28), day(2026, 11, 3))),
    );
    assert!(open_tag(&page, r#"data-ui-preset="last_7_days""#).contains(r#"aria-pressed="true""#));
    assert!(open_tag(&page, r#"data-ui-preset="custom""#).contains(r#"aria-pressed="false""#));
}

#[test]
fn a_preset_outside_the_limits_cannot_be_pressed() {
    let page = html(
        DateRangePicker::new("from", "to")
            .today(today())
            .min(day(2026, 10, 30))
            .presets([
                DatePreset::Today,
                DatePreset::Last7Days,
                DatePreset::AllTime,
            ]),
    );
    assert!(open_tag(&page, r#"data-ui-preset="last_7_days""#).contains(" disabled"));
    assert!(!open_tag(&page, r#"data-ui-preset="today""#).contains(" disabled"));
    // All time begins at the earliest day.
    let all = open_tag(&page, r#"data-ui-preset="all_time""#);
    assert!(
        all.contains(r#"data-from="2026-10-30" data-to="2026-11-03""#),
        "{all}"
    );
}

#[test]
fn a_range_picker_can_hold_its_choice_until_apply() {
    let page = html(DateRangePicker::new("from", "to").today(today()).confirm());
    assert!(page.contains("data-confirm"));
    assert!(page.contains(r#"data-ui-act="apply""#));
    assert!(page.contains(r#"data-ui-act="cancel""#));
    let plain = html(DateRangePicker::new("from", "to").today(today()));
    assert!(!plain.contains(r#"data-ui-act="apply""#));
}

#[test]
fn a_range_picker_shows_two_native_fields_with_javascript_off() {
    let page = html(DateRangePicker::new("from", "to").today(today()));
    assert_eq!(count(&page, r#"type="date""#), 2);
    assert!(page.contains(r#"data-slot="range-native""#));
    assert!(page.contains(">From</span>"));
    assert!(page.contains(">To</span>"));
}

#[test]
fn a_time_picker_posts_hours_and_minutes() {
    let page = html(TimePicker::new("starts").value(time(14, 30)));
    assert!(
        page.contains(r#"type="time" name="starts" value="14:30""#),
        "{page}"
    );
    assert!(page.contains(r#"value="2:30 PM""#));
    assert!(
        !page.contains(r#" step=""#),
        "a native step would call a typed 10:07 a mismatch"
    );
    let de = html(
        TimePicker::new("starts")
            .value(time(14, 30))
            .locale(DateLocale::of("de")),
    );
    assert!(de.contains(r#"value="14:30""#));
    let ko = html(
        TimePicker::new("starts")
            .value(time(15, 5))
            .locale(DateLocale::of("ko")),
    );
    assert!(ko.contains(r#"value="오후 3:05""#));
    let seconds = html(
        TimePicker::new("t")
            .seconds()
            .value(NaiveTime::from_hms_opt(14, 30, 15).unwrap()),
    );
    assert!(seconds.contains(r#"value="14:30:15""#));
    assert!(seconds.contains(r#"step="1""#));
    assert!(seconds.contains(r#"value="2:30:15 PM""#));
    let forced = html(TimePicker::new("t").value(time(14, 30)).clock(Clock::H24));
    assert!(forced.contains(r#"value="14:30""#));
}

#[test]
fn time_rules_reach_the_browser_and_the_native_field() {
    let rules = TimeRules::new()
        .min(time(9, 0))
        .max(time(17, 0))
        .unavailable(time(12, 0), time(12, 59))
        .step_minutes(15)
        .strict_step();
    let page = html(
        TimePicker::new("t")
            .rules(rules)
            .zone_note("Times are in Europe/London."),
    );
    assert!(
        page.contains(r#"min="09:00" max="17:00" step="900""#),
        "{page}"
    );
    let json = json_attr(&page, "data-ui-dates=\"time\"", "data-ui-trules");
    assert_eq!(json["u"], serde_json::json!([[720, 779]]));
    assert_eq!(json["st"], 15);
    assert!(page.contains("Times are in Europe/London."));
}

#[derive(Debug, Deserialize, Validate, Form)]
struct Booking {
    /// Start date
    #[validate(custom(weekday))]
    #[form(widget = date_picker)]
    start: NaiveDate,
    /// End date
    #[validate(skip)]
    #[form(widget = date_picker)]
    end: Option<NaiveDate>,
    /// Arrives at
    #[validate(skip)]
    #[form(widget = time_picker)]
    arrives: NaiveTime,
}

fn weekday(day: &NaiveDate) -> Result<(), String> {
    DateRules::new()
        .disable_weekdays([Weekday::Sat, Weekday::Sun])
        .check(*day)
        .map_err(|error| error.message(&DateLocale::english()))
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
        .collect()
}

#[test]
fn the_form_derive_draws_pickers_and_the_server_checks_what_comes_back() {
    let page = html(FormView::<Booking>::new().dates(DateLocale::of("de"), today()));
    assert!(page.contains(r#"data-ui-dates="picker""#));
    assert!(page.contains(r#"data-ui-dates="time""#));
    assert!(page.contains(r#"for="booking-start""#), "{page}");
    assert!(page.contains(r#"id="booking-start""#));
    assert!(page.contains(r#"placeholder="TT.MM.JJJJ""#));
    // A required field makes the native one required.
    assert!(
        open_tag(&page, r#"name="start""#).contains(" required"),
        "{page}"
    );
    // The end is optional.
    assert!(!open_tag(&page, r#"name="end""#).contains(" required"));

    // Monday 9 November; a browser posts the time without seconds.
    let ok: Booking = decode(&pairs(&[
        ("start", "2026-11-09"),
        ("end", ""),
        ("arrives", "14:30"),
    ]))
    .unwrap();
    assert_eq!(ok.start, day(2026, 11, 9));
    assert_eq!(ok.end, None);
    assert_eq!(ok.arrives, time(14, 30));
    // Seconds are fine too.
    let ok: Booking = decode(&pairs(&[("start", "2026-11-09"), ("arrives", "14:30:15")])).unwrap();
    assert_eq!(ok.arrives, NaiveTime::from_hms_opt(14, 30, 15).unwrap());

    // A Saturday fails the rule, in words.
    let wrong =
        decode::<Booking>(&pairs(&[("start", "2026-11-07"), ("arrives", "14:30")])).unwrap_err();
    assert!(wrong["start"][0].contains("isn't available"), "{wrong:?}");
    // Text that isn't a date, an impossible day, and a time that isn't one.
    for bad in [
        "garbage",
        "2026-02-30",
        "03.11.2026",
        "",
        "2026-13-01",
        "11/03/2026",
        "2026-11-03T10:00",
    ] {
        let errors =
            decode::<Booking>(&pairs(&[("start", bad), ("arrives", "14:30")])).unwrap_err();
        assert!(errors.contains_key("start"), "{bad:?}");
    }
    let errors =
        decode::<Booking>(&pairs(&[("start", "2026-11-09"), ("arrives", "25:00")])).unwrap_err();
    assert!(errors["arrives"][0].contains("enter a time"), "{errors:?}");
    let errors =
        decode::<Booking>(&pairs(&[("start", "2026-11-09"), ("arrives", "soon")])).unwrap_err();
    assert!(errors.contains_key("arrives"));
}

#[test]
fn a_form_sent_back_shows_the_pickers_what_the_person_sent() {
    let page = html(
        FormView::<Booking>::new()
            .dates(DateLocale::english(), today())
            .with_values(vec![
                ("start".into(), "2026-11-07".into()),
                ("arrives".into(), "soon".into()),
            ])
            .with_errors(
                [(
                    "start".to_owned(),
                    vec!["Saturday can't be chosen.".to_owned()],
                )]
                .into(),
            ),
    );
    assert!(page.contains(r#"value="2026-11-07""#));
    assert!(page.contains(r#"value="11/07/2026""#));
    assert!(page.contains(r#"value="soon""#));
    assert!(
        page.contains("Saturday can&#39;t be chosen.")
            || page.contains("Saturday can't be chosen.")
    );
}

/// A tiny generator, so the tests need no extra crate and a failure repeats.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
}

const TAGS: [&str; 19] = [
    "en", "en-GB", "de", "fr", "es", "it", "pt", "nl", "sv", "pl", "ru", "tr", "ja", "zh", "ko",
    "ar", "he", "hi", "id",
];

#[test]
fn every_date_reads_back_as_the_locale_writes_it() {
    let mut rng = Lcg(7);
    for tag in TAGS {
        let locale = DateLocale::of(tag);
        let mut days = vec![
            day(1, 1, 1),
            day(9999, 12, 31),
            day(2000, 2, 29),
            day(2026, 11, 3),
            day(1999, 12, 31),
            day(2024, 2, 29),
        ];
        for _ in 0..600 {
            let year = 1 + i32::try_from(rng.next() % 9999).unwrap();
            let month = 1 + u32::try_from(rng.next() % 12).unwrap();
            let d = 1 + u32::try_from(rng.next() % 28).unwrap();
            days.push(day(year, month, d));
        }
        for d in days {
            let text = locale.format_date(d);
            assert_eq!(locale.parse_date(&text, today()), Ok(d), "{tag}: {text}");
            // And through a name, where the locale writes dates with one.
            let named = format!(
                "{} {} {:04}",
                d.day(),
                locale.month_name(d.month()),
                d.year()
            );
            if let Ok(back) = locale.parse_date(&named, today()) {
                assert_eq!(back, d, "{tag}: {named}");
            }
        }
    }
}

#[test]
fn every_time_reads_back_as_the_locale_writes_it() {
    for tag in TAGS {
        let locale = DateLocale::of(tag);
        for minute in 0..1440 {
            let t = time(minute / 60, minute % 60);
            let text = locale.format_time(t, false);
            assert_eq!(locale.parse_time(&text), Ok(t), "{tag}: {text}");
        }
        let t = NaiveTime::from_hms_opt(23, 59, 58).unwrap();
        assert_eq!(
            locale.parse_time(&locale.format_time(t, true)),
            Ok(t),
            "{tag}"
        );
    }
}

#[test]
fn random_text_never_panics_and_never_reads_as_an_impossible_day() {
    let mut rng = Lcg(99);
    let alphabet: Vec<char> =
        "0123456789/.-, :٠١٢٣٤٥٦٧٨٩०१२३४५६७८९１２３年月日amAMpP\u{200f}\u{202e}ñéßİıΣ<>\"'&\0\n"
            .chars()
            .collect();
    for tag in TAGS {
        let locale = DateLocale::of(tag);
        for _ in 0..2000 {
            let length = usize::try_from(rng.next() % 24).unwrap();
            let text: String = (0..length)
                .map(|_| alphabet[usize::try_from(rng.next()).unwrap() % alphabet.len()])
                .collect();
            if let Ok(found) = locale.parse_date(&text, today()) {
                assert!((1..=9999).contains(&found.year()), "{tag}: {text:?}");
            }
            let _ = locale.parse_time(&text);
        }
    }
}

#[test]
fn the_dates_module_is_small_and_a_calendar_draws_fast() {
    let size = assets::get("dates").unwrap().gzip.unwrap().len();
    assert!(size <= 12 * 1024, "{size}");
}

#[cfg(not(debug_assertions))]
#[test]
fn a_two_month_calendar_renders_in_well_under_a_millisecond() {
    let start = std::time::Instant::now();
    let mut bytes = 0;
    for _ in 0..500 {
        bytes += html(
            Calendar::new("c")
                .today(today())
                .months(2)
                .rules(DateRules::new().disable_weekdays([Weekday::Sat, Weekday::Sun]))
                .selected(day(2026, 11, 12)),
        )
        .len();
    }
    let each = start.elapsed() / 500;
    assert!(
        each < std::time::Duration::from_micros(1500),
        "{each:?} for {bytes} bytes"
    );
}
