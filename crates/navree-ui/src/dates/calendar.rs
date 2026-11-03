//! [`Calendar`]: a month grid to choose days from.

use std::collections::BTreeSet;

use chrono::{Datelike, NaiveDate, Utc, Weekday};

use super::grid::{self, add_months, month_grid, week_number};
use super::locale::DateLocale;
use super::range::DateRange;
use super::rules::DateRules;
use crate::components::{Button, ButtonVariant, Size};
use crate::icons::{self, Icon};
use crate::node::{Element, Node, Uses, el, into_node};

/// How a calendar takes choices.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum CalendarMode {
    /// One day.
    #[default]
    Single,
    /// Any number of days, posted as one comma-separated field.
    Multiple,
    /// A run of days: a click starts it and a second click ends it.
    Range,
}

impl CalendarMode {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Single => "single",
            Self::Multiple => "multiple",
            Self::Range => "range",
        }
    }
}

/// How the month's heading is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum CaptionLayout {
    /// The month and year as text.
    #[default]
    Label,
    /// A list of months and a list of years: the way to a date of birth in a few
    /// clicks. Only a calendar of one month has them.
    Dropdowns,
}

/// A month as `(year, month)`.
type Month = (i32, u32);

/// A calendar: one or more months, with the days that can be chosen.
///
/// The server draws the first month; the browser module draws the others as a
/// person moves, with no request. Selecting writes ISO dates (`2026-11-03`) into
/// hidden fields, and every one is checked again on the server.
///
/// ```
/// use navree_ui::dates::Calendar;
/// use navree_ui::render;
/// use chrono::NaiveDate;
///
/// let today = NaiveDate::from_ymd_opt(2026, 11, 3).unwrap();
/// let html = render(&Calendar::new("due").name("due").today(today).selected(today).into());
/// assert!(html.contains(r#"aria-selected="true""#));
/// assert!(html.contains(r#"name="due" value="2026-11-03""#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Calendar {
    pub(super) id: String,
    pub(super) mode: CalendarMode,
    pub(super) view: Option<NaiveDate>,
    pub(super) selected: Vec<NaiveDate>,
    pub(super) range: Option<DateRange>,
    pub(super) names: Option<(String, Option<String>)>,
    pub(super) months: u8,
    pub(super) rules: DateRules,
    pub(super) locale: DateLocale,
    pub(super) today: NaiveDate,
    pub(super) week_start: Option<Weekday>,
    pub(super) week_numbers: bool,
    pub(super) fixed_weeks: bool,
    pub(super) outside_days: bool,
    pub(super) caption: CaptionLayout,
    pub(super) navigation: bool,
    pub(super) read_only: bool,
    pub(super) disabled: bool,
    pub(super) required: bool,
    pub(super) size: Size,
    pub(super) min_days: Option<u32>,
    pub(super) max_days: Option<u32>,
    pub(super) max_count: Option<u32>,
    pub(super) with_today: bool,
    pub(super) label: Option<String>,
    pub(super) embedded: bool,
    el: Element,
}

impl Calendar {
    /// A calendar with this id, showing the month of today.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            mode: CalendarMode::Single,
            view: None,
            selected: Vec::new(),
            range: None,
            names: None,
            months: 1,
            rules: DateRules::new(),
            locale: DateLocale::english(),
            today: Utc::now().date_naive(),
            week_start: None,
            week_numbers: false,
            fixed_weeks: false,
            outside_days: true,
            caption: CaptionLayout::Label,
            navigation: true,
            read_only: false,
            disabled: false,
            required: false,
            size: Size::Md,
            min_days: None,
            max_days: None,
            max_count: None,
            with_today: false,
            label: None,
            embedded: false,
            el: el("div").slot("calendar"),
        }
    }

    /// Sets how it takes choices.
    pub fn mode(mut self, mode: CalendarMode) -> Self {
        self.mode = mode;
        self
    }

    /// Names the field that posts the choice: one ISO date, or the dates of
    /// [`CalendarMode::Multiple`] joined by commas. For a range, the first day.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        let to = self.names.take().and_then(|(_, to)| to);
        self.names = Some((name.into(), to));
        self
    }

    /// Names the two fields that post a range: its first and its last day.
    pub fn range_names(mut self, from: impl Into<String>, to: impl Into<String>) -> Self {
        self.names = Some((from.into(), Some(to.into())));
        self
    }

    /// The chosen day.
    pub fn selected(mut self, day: NaiveDate) -> Self {
        self.selected = vec![day];
        self
    }

    /// The chosen days, for [`CalendarMode::Multiple`].
    pub fn selected_days(mut self, days: impl IntoIterator<Item = NaiveDate>) -> Self {
        self.selected = days.into_iter().collect();
        self
    }

    /// The chosen range, for [`CalendarMode::Range`].
    pub fn selected_range(mut self, range: DateRange) -> Self {
        self.range = Some(range);
        self
    }

    /// The month to open on, given as any day in it. By default the month of the
    /// choice, else the month of today.
    pub fn month(mut self, day: NaiveDate) -> Self {
        self.view = Some(day);
        self
    }

    /// How many months to show side by side, 1 to 3.
    pub fn months(mut self, months: u8) -> Self {
        self.months = months.clamp(1, 3);
        self
    }

    /// Which days can't be chosen.
    pub fn rules(mut self, rules: DateRules) -> Self {
        self.rules = rules;
        self
    }

    /// The earliest day that can be chosen.
    pub fn min(mut self, day: NaiveDate) -> Self {
        self.rules = self.rules.min(day);
        self
    }

    /// The latest day that can be chosen.
    pub fn max(mut self, day: NaiveDate) -> Self {
        self.rules = self.rules.max(day);
        self
    }

    /// Names and orders the days after a language and region.
    pub fn locale(mut self, locale: DateLocale) -> Self {
        self.locale = locale;
        self
    }

    /// Today, as the page counts it. The calendar marks it and the browser module
    /// never asks the clock, so a page is the same wherever it is read. Without it,
    /// today is the day in UTC.
    pub fn today(mut self, today: NaiveDate) -> Self {
        self.today = today;
        self
    }

    /// The first day of the week, when the locale's isn't wanted.
    pub fn week_start(mut self, day: Weekday) -> Self {
        self.week_start = Some(day);
        self
    }

    /// Shows the ISO-8601 week number of each row.
    pub fn week_numbers(mut self) -> Self {
        self.week_numbers = true;
        self
    }

    /// Always draws six rows, so the calendar doesn't change height from month to
    /// month.
    pub fn fixed_weeks(mut self) -> Self {
        self.fixed_weeks = true;
        self
    }

    /// Leaves out the days of the neighbouring months that fill the first and last
    /// rows.
    pub fn hide_outside_days(mut self) -> Self {
        self.outside_days = false;
        self
    }

    /// Sets how the month's heading is drawn.
    pub fn caption(mut self, caption: CaptionLayout) -> Self {
        self.caption = caption;
        self
    }

    /// Turns the previous and next buttons off.
    pub fn navigation(mut self, navigation: bool) -> Self {
        self.navigation = navigation;
        self
    }

    /// Draws the calendar to look at only: no buttons, no tab stops.
    pub fn read_only(mut self) -> Self {
        self.read_only = true;
        self
    }

    /// Turns the calendar off: nothing can be chosen and nothing is posted.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Stops a second click on the chosen day from unchoosing it.
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// Sets the size of the day cells.
    pub fn size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }

    /// The days that can't be chosen and the least and most days a range may hold, as one
    /// [`RangeRules`](super::RangeRules) (see [`DateRangePicker::limits`](super::DateRangePicker::limits)).
    pub fn limits(mut self, limits: super::RangeRules) -> Self {
        self.rules = limits.rules().clone();
        self.min_days = limits.least_days();
        self.max_days = limits.most_days();
        self
    }

    /// The fewest days a range may hold.
    pub fn min_days(mut self, days: u32) -> Self {
        self.min_days = Some(days.max(1));
        self
    }

    /// The most days a range may hold.
    pub fn max_days(mut self, days: u32) -> Self {
        self.max_days = Some(days.max(1));
        self
    }

    /// The most days [`CalendarMode::Multiple`] may hold.
    pub fn max_count(mut self, count: u32) -> Self {
        self.max_count = Some(count.max(1));
        self
    }

    /// Adds a button under the grid that goes to this month, or chooses today.
    pub fn with_today(mut self) -> Self {
        self.with_today = true;
        self
    }

    /// Names the calendar for assistive technology.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Draws the calendar inside a picker: the picker holds the value.
    pub(super) fn embedded(mut self) -> Self {
        self.embedded = true;
        self
    }

    /// The first day of the week.
    pub(super) fn first_day(&self) -> Weekday {
        self.week_start.unwrap_or(self.locale.first_day)
    }

    /// The days chosen, in order.
    fn chosen(&self) -> BTreeSet<NaiveDate> {
        match self.mode {
            CalendarMode::Single => self.selected.iter().copied().take(1).collect(),
            CalendarMode::Multiple => self.selected.iter().copied().collect(),
            CalendarMode::Range => BTreeSet::new(),
        }
    }

    /// The month shown first: the view, else the choice, else today; held inside
    /// the limits.
    pub(super) fn first_month(&self) -> (i32, u32) {
        let anchor = self
            .view
            .or_else(|| self.chosen().first().copied())
            .or_else(|| self.range.map(|range| range.from))
            .unwrap_or(self.today);
        let mut anchor = grid::clamp(anchor);
        if self.view.is_none() {
            if let Some(min) = self.rules.earliest()
                && anchor < min
            {
                anchor = min;
            }
            if let Some(max) = self.rules.latest()
                && anchor > max
            {
                anchor = max;
            }
        }
        // Keep room for the months after it.
        let span = i32::from(self.months) - 1;
        let (year, month) = (anchor.year(), anchor.month());
        let last = add_months(year, month, span);
        match last {
            Some(_) => (year, month),
            None => add_months(year, month, -span).unwrap_or((year, month)),
        }
    }

    /// The months on show.
    fn visible(&self) -> Vec<(i32, u32)> {
        let (year, month) = self.first_month();
        (0..i32::from(self.months))
            .filter_map(|n| add_months(year, month, n))
            .collect()
    }

    /// The day that holds the tab stop: the first chosen day on show, else today,
    /// else the first day that can be chosen.
    fn focus_day(&self, visible: &[(i32, u32)]) -> Option<NaiveDate> {
        let shown = |day: NaiveDate| visible.contains(&(day.year(), day.month()));
        let chosen: Vec<NaiveDate> = match self.mode {
            CalendarMode::Range => self.range.iter().flat_map(|r| [r.from, r.to]).collect(),
            _ => self.chosen().into_iter().collect(),
        };
        chosen
            .into_iter()
            .find(|day| shown(*day) && self.rules.allows(*day))
            .or_else(|| (shown(self.today) && self.rules.allows(self.today)).then_some(self.today))
            .or_else(|| {
                let (year, month) = *visible.first()?;
                (1..=grid::days_in_month(year, month))
                    .filter_map(|day| NaiveDate::from_ymd_opt(year, month, day))
                    .find(|day| self.rules.allows(*day))
                    .or_else(|| NaiveDate::from_ymd_opt(year, month, 1))
            })
    }

    /// The attribute `data-ui-rules`: limits, weekdays and runs, for the days the
    /// browser can reach.
    fn rules_json(&self, year: i32, month: u32) -> String {
        let window = self.rules.window(year, month);
        self.rules.client(window, self.rules.has_rule()).to_string()
    }

    /// The root element with its data, and the months inside it.
    pub(super) fn build(self) -> Element {
        let visible = self.visible();
        let (year, month) = visible.first().copied().unwrap_or((self.today.year(), 1));
        let focus = self.focus_day(&visible);
        let chosen = self.chosen();
        let first_day = self.first_day();
        let editable = !self.read_only && !self.disabled;
        let months_id = format!("{}-months", self.id);
        // Inside a picker the months are the browser's to draw when the popover opens;
        // the server sends the shell and nothing a page would never show.
        let blocks: Vec<Element> = if self.embedded {
            Vec::new()
        } else {
            visible
                .iter()
                .map(|(y, m)| self.month_block(*y, *m, &chosen, focus, first_day, editable))
                .collect()
        };
        let nav = (self.navigation && editable).then(|| self.nav(&visible));
        let mut root = self
            .el
            .clone()
            .id(self.id.clone())
            .attr("role", "group")
            .attr(
                "aria-label",
                self.label
                    .clone()
                    .unwrap_or_else(|| self.locale.labels.choose_date.to_string()),
            )
            .attr("data-ui-dates", "calendar")
            .attr("data-mode", self.mode.as_str())
            .attr("data-month", format!("{year:04}-{month:02}"))
            .attr("data-today", self.today.to_string())
            .attr(
                "data-week-start",
                i64::from(first_day.num_days_from_sunday()),
            )
            .attr("data-months", i64::from(self.months))
            .attr("data-ui-loc", self.locale.client().to_string())
            .attr("data-ui-rules", self.rules_json(year, month))
            .attr("lang", self.locale.tag.to_string())
            .attr_opt("dir", self.locale.rtl.then_some("rtl"))
            .attr_opt("data-size", self.size.as_str())
            .flag("data-week-numbers", self.week_numbers)
            .flag("data-fixed-weeks", self.fixed_weeks)
            .flag("data-no-outside", !self.outside_days)
            .flag("data-readonly", self.read_only)
            .flag("data-disabled", self.disabled)
            .flag("data-required", self.required)
            .flag("data-no-nav", !self.navigation)
            .flag("data-embedded", self.embedded)
            .attr_opt(
                "data-caption",
                (self.caption == CaptionLayout::Dropdowns && self.months == 1)
                    .then_some("dropdowns"),
            )
            .attr_opt("data-min-days", self.min_days.map(i64::from))
            .attr_opt("data-max-days", self.max_days.map(i64::from))
            .attr_opt("data-max-count", self.max_count.map(i64::from))
            .uses(Uses::DATES);
        if !self.embedded {
            root = root.child(self.value_fields(&chosen));
        }
        root = root.child(nav);
        root = root.child(
            el("div")
                .slot("calendar-months")
                .id(months_id)
                .flag("data-ui-keep", true)
                .child(blocks),
        );
        if self.with_today && !self.read_only {
            root = root.child(
                el("div").slot("calendar-foot").child(
                    Button::new(self.locale.labels.today.to_string())
                        .variant(ButtonVariant::Ghost)
                        .size(Size::Sm)
                        .attr("data-ui-cal", "today")
                        .attr("aria-label", self.locale.labels.today.to_string()),
                ),
            );
        }
        root
    }

    /// The hidden fields that post the choice.
    fn value_fields(&self, chosen: &BTreeSet<NaiveDate>) -> Vec<Element> {
        let field = |name: Option<&str>, which: &'static str, value: String| {
            el("input")
                .attr("type", "hidden")
                .attr_opt("name", name.map(str::to_owned))
                .attr("value", value)
                .attr("data-ui-val", which)
                .flag("disabled", self.disabled)
        };
        let (name, to_name) = match &self.names {
            Some((name, to)) => (Some(name.as_str()), to.as_deref()),
            None => (None, None),
        };
        match self.mode {
            CalendarMode::Single | CalendarMode::Multiple => {
                let value = chosen
                    .iter()
                    .map(NaiveDate::to_string)
                    .collect::<Vec<_>>()
                    .join(",");
                vec![field(name, "main", value)]
            }
            CalendarMode::Range => {
                let from = self.range.map(|r| r.from.to_string()).unwrap_or_default();
                let to = self.range.map(|r| r.to.to_string()).unwrap_or_default();
                vec![field(name, "from", from), field(to_name, "to", to)]
            }
        }
    }

    fn month_block(
        &self,
        year: i32,
        month: u32,
        chosen: &BTreeSet<NaiveDate>,
        focus: Option<NaiveDate>,
        first_day: Weekday,
        editable: bool,
    ) -> Element {
        let labels = &self.locale.labels;
        let caption_id = format!("{}-cap-{year:04}-{month:02}", self.id);
        let caption_text = self.locale.format_caption(year, month);
        let dropdowns = self.caption == CaptionLayout::Dropdowns && self.months == 1;
        let caption = if dropdowns {
            self.dropdown_caption(&caption_id, year, month)
        } else {
            el("div")
                .slot("calendar-caption")
                .id(caption_id.clone())
                .attr("aria-live", "polite")
                .child(caption_text)
        };
        let weekday_names: Vec<Element> = (0..7u32)
            .map(|offset| {
                let day = weekday_at(first_day, offset);
                el("th")
                    .slot("calendar-weekday")
                    .attr("scope", "col")
                    .attr("abbr", self.locale.weekday_name(day).to_owned())
                    .child(self.locale.weekday_short(day).to_owned())
            })
            .collect();
        let head_row = el("tr").children((
            self.week_numbers.then(|| {
                el("th")
                    .slot("calendar-weeknum")
                    .attr("scope", "col")
                    .child(labels.week.to_string())
            }),
            weekday_names,
        ));
        let grid = month_grid(year, month, first_day, self.fixed_weeks);
        let rows: Vec<Element> = grid
            .rows
            .iter()
            .map(|row| {
                el("tr").children((
                    self.week_numbers.then(|| {
                        el("th")
                            .slot("calendar-weeknum")
                            .attr("scope", "row")
                            .child(i64::from(week_number(row)))
                    }),
                    row.iter()
                        .map(|day| self.cell(*day, (year, month), chosen, focus, editable))
                        .collect::<Vec<_>>(),
                ))
            })
            .collect();
        el("div")
            .slot("calendar-month")
            .attr("data-month", format!("{year:04}-{month:02}"))
            .children((
                el("div").slot("calendar-head").child(caption),
                el("table")
                    .slot("calendar-grid")
                    .attr("role", "grid")
                    .attr("aria-labelledby", caption_id)
                    .children((el("thead").child(head_row), el("tbody").child(rows))),
            ))
    }

    /// The previous and next buttons, on or off by how far navigation may go.
    fn nav(&self, visible: &[(i32, u32)]) -> Element {
        let labels = &self.locale.labels;
        let (min_month, max_month) = self.nav_months();
        let first = visible.first().copied().unwrap_or((self.today.year(), 1));
        let last = visible.last().copied().unwrap_or(first);
        let before =
            min_month.is_none_or(|min| first > min) && add_months(first.0, first.1, -1).is_some();
        let after =
            max_month.is_none_or(|max| last < max) && add_months(last.0, last.1, 1).is_some();
        let button = |which: &'static str, glyph: Icon, label: &str, enabled: bool| -> Element {
            let node: Node = Button::icon_only(glyph, label.to_owned())
                .variant(ButtonVariant::Ghost)
                .size(Size::Sm)
                .attr("data-ui-cal", which)
                .disabled(!enabled)
                .into();
            node.into_element().unwrap_or_else(|| el("span"))
        };
        el("div").slot("calendar-nav").children((
            button("prev", icons::CHEVRON_LEFT, &labels.prev_month, before),
            button("next", icons::CHEVRON_RIGHT, &labels.next_month, after),
        ))
    }

    /// The first and last months navigation may reach, from the limits and, with a
    /// rule function, from the window the browser was told about.
    pub(super) fn nav_months(&self) -> (Option<Month>, Option<Month>) {
        let (year, month) = self.first_month();
        let (start, end) = self.rules.window(year, month);
        let limited = self.rules.has_rule();
        let first = if limited {
            Some((start.year(), start.month()))
        } else {
            self.rules.earliest().map(|d| (d.year(), d.month()))
        };
        let last = if limited {
            Some((end.year(), end.month()))
        } else {
            self.rules.latest().map(|d| (d.year(), d.month()))
        };
        (first, last)
    }

    fn dropdown_caption(&self, caption_id: &str, year: i32, month: u32) -> Element {
        let labels = &self.locale.labels;
        let low = self
            .rules
            .earliest()
            .map_or(year - 100, |d| d.year())
            .max(grid::FIRST_YEAR);
        let high = self
            .rules
            .latest()
            .map_or(year + 100, |d| d.year())
            .min(grid::LAST_YEAR);
        let (low, high) = if high - low > 200 {
            ((year - 100).max(low), (year + 100).min(high))
        } else {
            (low, high)
        };
        let months: Vec<Element> = (1..=12)
            .map(|m| {
                el("option")
                    .attr("value", i64::from(m))
                    .flag("selected", m == month)
                    .child(self.locale.month_name(m).to_owned())
            })
            .collect();
        let years: Vec<Element> = (low..=high)
            .map(|y| {
                el("option")
                    .attr("value", i64::from(y))
                    .flag("selected", y == year)
                    .child(i64::from(y))
            })
            .collect();
        let select = |which: &'static str, label: &str, options: Vec<Element>| {
            el("select")
                .slot("native-select")
                .attr("data-size", "sm")
                .attr("data-ui-cal", which)
                .attr("aria-label", label.to_owned())
                .child(options)
        };
        el("div")
            .slot("calendar-caption")
            .id(caption_id.to_owned())
            .attr("data-layout", "dropdowns")
            .children((
                select("month", &labels.month, months),
                select("year", &labels.year, years),
            ))
    }

    fn cell(
        &self,
        day: NaiveDate,
        shown: (i32, u32),
        chosen: &BTreeSet<NaiveDate>,
        focus: Option<NaiveDate>,
        editable: bool,
    ) -> Element {
        let outside = (day.year(), day.month()) != shown;
        if outside {
            let cell = el("td").attr("role", "gridcell");
            return if self.outside_days {
                cell.child(
                    el("span")
                        .slot("calendar-day")
                        .attr("data-outside", "")
                        .attr("aria-hidden", "true")
                        .child(i64::from(day.day())),
                )
            } else {
                cell
            };
        }
        let in_range = self.range.is_some_and(|range| range.contains(day));
        let selected = match self.mode {
            CalendarMode::Range => in_range,
            _ => chosen.contains(&day),
        };
        let disabled = !self.rules.allows(day);
        let start = self.range.is_some_and(|r| r.from == day);
        let end = self.range.is_some_and(|r| r.to == day);
        let button = el("button")
            .slot("calendar-day")
            .attr("type", "button")
            .attr("data-date", day.to_string())
            .attr(
                "tabindex",
                if focus == Some(day) && editable {
                    0_i64
                } else {
                    -1_i64
                },
            )
            .attr("aria-label", self.locale.format_long(day))
            .attr_opt("aria-current", (day == self.today).then_some("date"))
            .attr_opt("aria-disabled", (disabled || !editable).then_some("true"))
            .flag("data-today", day == self.today)
            .flag("data-disabled", disabled)
            .flag(
                "data-selected",
                selected && self.mode != CalendarMode::Range,
            )
            .flag(
                "data-range-start",
                self.mode == CalendarMode::Range && start,
            )
            .flag("data-range-end", self.mode == CalendarMode::Range && end)
            .flag(
                "data-range-middle",
                self.mode == CalendarMode::Range && in_range && !start && !end,
            )
            .child(i64::from(day.day()));
        let cell = el("td").attr("role", "gridcell");
        let cell = if matches!(
            self.mode,
            CalendarMode::Range | CalendarMode::Single | CalendarMode::Multiple
        ) && !self.read_only
        {
            cell.attr("aria-selected", if selected { "true" } else { "false" })
        } else {
            cell
        };
        cell.child(button)
    }

    fn into_node(self) -> Node {
        self.build().into()
    }
}

/// The weekday `offset` days after `start`.
pub(super) fn weekday_at(start: Weekday, offset: u32) -> Weekday {
    let mut day = start;
    for _ in 0..offset {
        day = day.succ();
    }
    day
}

impl Calendar {
    /// Sets an attribute on the root element.
    pub fn attr(mut self, name: &'static str, value: impl Into<crate::node::AttrValue>) -> Self {
        self.el = self.el.attr(name, value);
        self
    }
}

into_node!(Calendar);
