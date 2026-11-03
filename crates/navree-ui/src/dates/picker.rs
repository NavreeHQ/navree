//! [`DatePicker`] and [`DateRangePicker`]: a calendar in a popover, with typed entry
//! and a native fallback.
//!
//! Both draw a native `<input type="date">` first. It is the control with
//! JavaScript off, and the one field that posts with it on: the browser module hides
//! it and keeps its value, so what is posted is always an ISO date the server can
//! parse and check. The text box, the button and the calendar are drawn too, and
//! hidden by CSS where scripting is off.

use chrono::{NaiveDate, Utc, Weekday};

use super::calendar::{Calendar, CalendarMode, CaptionLayout};
use super::locale::DateLocale;
use super::range::{DatePreset, DateRange};
use super::rules::DateRules;
use crate::components::{Button, ButtonVariant, Size};
use crate::icons;
use crate::node::{AttrValue, Element, Node, Uses, el, into_node};

/// A safe id from a field name: letters, digits, `-` and `_` only.
pub(super) fn id_from(name: &str) -> String {
    let id: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if id.is_empty() {
        "field".to_owned()
    } else {
        id
    }
}

/// What a picker's control looks like.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum PickerStyle {
    /// A text box you can type in, with a button for the calendar.
    #[default]
    Input,
    /// One button that shows the date, for toolbars and filter rows. No typing.
    Button,
}

impl PickerStyle {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Button => "button",
        }
    }
}

/// What the picker was given as its value.
#[derive(Clone, Debug)]
enum Given {
    Date(NaiveDate),
    Text(String),
}

fn button_element(button: Button) -> Element {
    let node: Node = button.into();
    node.into_element().unwrap_or_else(|| el("span"))
}

/// A date field with a calendar.
///
/// It posts `name` as an ISO date (`2026-11-03`), or nothing when empty. A value
/// that comes back is read with `NaiveDate`, and checked with the same
/// [`DateRules`] that drew the calendar.
///
/// ```
/// use navree_ui::dates::{DateLocale, DatePicker};
/// use navree_ui::render;
/// use chrono::NaiveDate;
///
/// let day = NaiveDate::from_ymd_opt(2026, 11, 3).unwrap();
/// let html = render(&DatePicker::new("due").value(day).locale(DateLocale::of("de")).today(day).into());
/// assert!(html.contains(r#"type="date" name="due" value="2026-11-03""#));
/// assert!(html.contains(r#"value="03.11.2026""#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct DatePicker {
    name: String,
    id: Option<String>,
    value: Option<Given>,
    rules: DateRules,
    locale: DateLocale,
    today: NaiveDate,
    week_start: Option<Weekday>,
    style: PickerStyle,
    placeholder: Option<String>,
    clearable: bool,
    required: bool,
    disabled: bool,
    read_only: bool,
    invalid: bool,
    with_today: bool,
    caption: CaptionLayout,
    months: u8,
    week_numbers: bool,
    fixed_weeks: bool,
    label: Option<String>,
    el: Element,
}

impl DatePicker {
    /// A picker that posts as `name`.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            id: None,
            value: None,
            rules: DateRules::new(),
            locale: DateLocale::english(),
            today: Utc::now().date_naive(),
            week_start: None,
            style: PickerStyle::Input,
            placeholder: None,
            clearable: false,
            required: false,
            disabled: false,
            read_only: false,
            invalid: false,
            with_today: false,
            caption: CaptionLayout::Label,
            months: 1,
            week_numbers: false,
            fixed_weeks: false,
            label: None,
            el: el("div").slot("date-picker"),
        }
    }

    /// Sets the id of the picker's own element. The control a label points at is
    /// the native input inside it, which a [`Field`](crate::Field) gives its id.
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Sets the date.
    pub fn value(mut self, day: NaiveDate) -> Self {
        self.value = Some(Given::Date(day));
        self
    }

    /// Sets the date, or leaves the picker empty.
    pub fn value_opt(mut self, day: Option<NaiveDate>) -> Self {
        self.value = day.map(Given::Date);
        self
    }

    /// Sets the value from text a form sent back: an ISO date shows as that date;
    /// anything else shows as typed, so a person sees what they sent.
    pub fn value_text(mut self, text: &str) -> Self {
        let text = text.trim();
        self.value = if text.is_empty() {
            None
        } else {
            Some(match text.parse::<NaiveDate>() {
                Ok(day) => Given::Date(day),
                Err(_) => Given::Text(text.to_owned()),
            })
        };
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

    /// Today, as the page counts it. Without it, today is the day in UTC.
    pub fn today(mut self, today: NaiveDate) -> Self {
        self.today = today;
        self
    }

    /// The first day of the week, when the locale's isn't wanted.
    pub fn week_start(mut self, day: Weekday) -> Self {
        self.week_start = Some(day);
        self
    }

    /// Sets the control: a text box, or one button.
    pub fn style(mut self, style: PickerStyle) -> Self {
        self.style = style;
        self
    }

    /// The hint in an empty box, or on an empty button.
    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = Some(text.into());
        self
    }

    /// Adds a button that empties the picker.
    pub fn clearable(mut self) -> Self {
        self.clearable = true;
        self
    }

    /// Requires a date.
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// Turns the picker off: nothing can be chosen and nothing is posted.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Shows the date without letting it change. It still posts.
    pub fn read_only(mut self) -> Self {
        self.read_only = true;
        self
    }

    /// Marks it invalid.
    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    /// Adds a Today button under the calendar.
    pub fn with_today(mut self) -> Self {
        self.with_today = true;
        self
    }

    /// Draws month and year lists in the calendar's heading.
    pub fn caption(mut self, caption: CaptionLayout) -> Self {
        self.caption = caption;
        self
    }

    /// How many months the calendar shows, 1 or 2.
    pub fn months(mut self, months: u8) -> Self {
        self.months = months.clamp(1, 2);
        self
    }

    /// Shows week numbers in the calendar.
    pub fn week_numbers(mut self) -> Self {
        self.week_numbers = true;
        self
    }

    /// Draws six rows in every month.
    pub fn fixed_weeks(mut self) -> Self {
        self.fixed_weeks = true;
        self
    }

    /// Names the picker for assistive technology, when no label points at it.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets an attribute on the root element.
    pub fn attr(mut self, name: &'static str, value: impl Into<AttrValue>) -> Self {
        self.el = self.el.attr(name, value);
        self
    }

    fn into_node(self) -> Node {
        let root_id = self
            .id
            .clone()
            .unwrap_or_else(|| format!("dp-{}", id_from(&self.name)));
        let pop_id = format!("{root_id}-pop");
        let labels = self.locale.labels.clone();
        let (iso, text, raw) = match &self.value {
            Some(Given::Date(day)) => (Some(day.to_string()), self.locale.format_date(*day), false),
            Some(Given::Text(text)) => (None, text.clone(), true),
            None => (None, String::new(), false),
        };
        let has_value = iso.is_some() || raw;
        let native = el("input")
            .slot("input")
            .attr("type", "date")
            .attr("name", self.name.clone())
            .attr_opt("value", iso.clone())
            .attr_opt("min", self.rules.earliest().map(|d| d.to_string()))
            .attr_opt("max", self.rules.latest().map(|d| d.to_string()))
            .attr_opt("aria-label", self.label.clone())
            .attr_opt("aria-invalid", self.invalid.then_some("true"))
            .attr("data-ui-native", "")
            .flag("required", self.required)
            .flag("disabled", self.disabled)
            .flag("readonly", self.read_only);
        let mut calendar = Calendar::new(format!("{root_id}-cal"))
            .mode(CalendarMode::Single)
            .locale(self.locale.clone())
            .today(self.today)
            .rules(self.rules.clone())
            .caption(self.caption)
            .months(self.months)
            .label(labels.choose_date.to_string())
            .embedded();
        if let Some(Given::Date(day)) = &self.value {
            calendar = calendar.selected(*day);
        }
        if let Some(start) = self.week_start {
            calendar = calendar.week_start(start);
        }
        if self.week_numbers {
            calendar = calendar.week_numbers();
        }
        if self.fixed_weeks {
            calendar = calendar.fixed_weeks();
        }
        let open_button = |label: &str| {
            button_element(
                Button::icon_only(icons::CALENDAR, label.to_owned())
                    .variant(ButtonVariant::Ghost)
                    .size(Size::Sm)
                    .attr("popovertarget", pop_id.clone())
                    .attr("aria-haspopup", "dialog")
                    .attr("data-ui-act", "open")
                    .disabled(self.disabled || self.read_only),
            )
        };
        let clear = self.clearable.then(|| {
            button_element(
                Button::icon_only(icons::X, labels.clear.to_string())
                    .variant(ButtonVariant::Ghost)
                    .size(Size::Sm)
                    .attr("data-ui-act", "clear")
                    .disabled(self.disabled || self.read_only),
            )
        });
        let placeholder = self
            .placeholder
            .clone()
            .unwrap_or_else(|| self.locale.placeholder.to_string());
        let control = match self.style {
            PickerStyle::Input => el("div").slot("input-group").children((
                el("input")
                    .slot("input")
                    .attr("type", "text")
                    .attr("data-ui-text", "")
                    .attr("autocomplete", "off")
                    .attr("placeholder", placeholder)
                    .attr("value", text.clone())
                    .attr_opt("aria-label", self.label.clone())
                    .attr_opt("aria-invalid", (self.invalid || raw).then_some("true"))
                    .flag("required", self.required)
                    .flag("disabled", self.disabled)
                    .flag("readonly", self.read_only),
                el("div")
                    .slot("input-group-addon")
                    .children((clear, open_button(&labels.choose_date))),
            )),
            PickerStyle::Button => {
                let placeholder_text = placeholder.clone();
                let shown = if has_value { text.clone() } else { placeholder };
                el("div").slot("date-picker-buttons").children((
                    button_element(
                        Button::new(
                            el("span")
                                .attr("data-ui-label", "")
                                .attr("data-empty", placeholder_text.clone())
                                .child(shown),
                        )
                        .variant(ButtonVariant::Outline)
                        .icon(icons::CALENDAR)
                        .attr("popovertarget", pop_id.clone())
                        .attr("aria-haspopup", "dialog")
                        .attr("data-ui-act", "open")
                        .disabled(self.disabled || self.read_only),
                    ),
                    clear,
                ))
            }
        };
        let foot = (self.with_today || self.clearable).then(|| {
            el("div").slot("date-picker-foot").children((
                self.with_today.then(|| {
                    button_element(
                        Button::new(labels.today.to_string())
                            .variant(ButtonVariant::Ghost)
                            .size(Size::Sm)
                            .attr("data-ui-act", "today"),
                    )
                }),
                self.clearable.then(|| {
                    button_element(
                        Button::new(labels.clear.to_string())
                            .variant(ButtonVariant::Ghost)
                            .size(Size::Sm)
                            .attr("data-ui-act", "clear"),
                    )
                }),
            ))
        });
        let align = match (self.style, self.locale.rtl) {
            (PickerStyle::Input, false) => "end",
            (PickerStyle::Input, true) | (PickerStyle::Button, false) => "start",
            (PickerStyle::Button, true) => "end",
        };
        let popup = el("div")
            .slot("popover-content")
            .id(pop_id.clone())
            .attr("popover", "auto")
            .attr("role", "dialog")
            .attr("tabindex", -1_i64)
            .attr("aria-label", labels.choose_date.to_string())
            .flag("data-ui-place", true)
            .attr("data-side", "bottom")
            .attr("data-align", align)
            .attr("data-variant", "calendar")
            .uses(Uses::OVERLAY)
            .children((calendar.build(), foot));
        self.el
            .id(root_id)
            .attr("data-ui-dates", "picker")
            .attr("data-style", self.style.as_str())
            .attr("data-today", self.today.to_string())
            .attr_opt("dir", self.locale.rtl.then_some("rtl"))
            .flag("data-has-value", has_value)
            .flag("data-invalid", self.invalid || raw)
            .flag("data-disabled", self.disabled)
            .uses(Uses::DATES)
            .children((
                native,
                el("div").slot("date-picker-ui").children((control, popup)),
            ))
            .into()
    }
}

into_node!(DatePicker);

/// A range of days from two dates: one button, and a popover with presets beside a
/// calendar of two months.
///
/// It posts two ISO dates, `from` and `to`, both days included (see
/// [`DateRange`]). With JavaScript off it shows two native date fields. Put it in a
/// GET form to keep a report's window in the URL: the form's inputs are the query.
///
/// ```
/// use navree_ui::dates::{DatePreset, DateRange, DateRangePicker};
/// use navree_ui::render;
/// use chrono::NaiveDate;
///
/// let day = |d| NaiveDate::from_ymd_opt(2026, 11, d).unwrap();
/// let html = render(
///     &DateRangePicker::new("from", "to")
///         .today(day(3))
///         .value(DateRange::new(day(1), day(3)))
///         .presets([DatePreset::Today, DatePreset::Last7Days])
///         .into(),
/// );
/// assert!(html.contains(r#"name="from" value="2026-11-01""#));
/// assert!(html.contains("Last 7 days"));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct DateRangePicker {
    from_name: String,
    to_name: String,
    id: Option<String>,
    value: Option<DateRange>,
    rules: DateRules,
    locale: DateLocale,
    today: NaiveDate,
    week_start: Option<Weekday>,
    presets: Vec<DatePreset>,
    preset_labels: Vec<(DatePreset, String)>,
    custom: bool,
    months: u8,
    min_days: Option<u32>,
    max_days: Option<u32>,
    placeholder: Option<String>,
    title: Option<String>,
    clearable: bool,
    confirm: bool,
    required: bool,
    disabled: bool,
    invalid: bool,
    week_numbers: bool,
    el: Element,
}

impl DateRangePicker {
    /// A picker that posts its first day as `from_name` and its last as `to_name`.
    pub fn new(from_name: impl Into<String>, to_name: impl Into<String>) -> Self {
        Self {
            from_name: from_name.into(),
            to_name: to_name.into(),
            id: None,
            value: None,
            rules: DateRules::new(),
            locale: DateLocale::english(),
            today: Utc::now().date_naive(),
            week_start: None,
            presets: vec![
                DatePreset::Today,
                DatePreset::Yesterday,
                DatePreset::Last7Days,
                DatePreset::Last30Days,
                DatePreset::ThisMonth,
                DatePreset::LastMonth,
            ],
            preset_labels: Vec::new(),
            custom: true,
            months: 2,
            min_days: None,
            max_days: None,
            placeholder: None,
            title: None,
            clearable: true,
            confirm: false,
            required: false,
            disabled: false,
            invalid: false,
            week_numbers: false,
            el: el("div").slot("date-range-picker"),
        }
    }

    /// Sets the id of the picker's own element.
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Sets the range.
    pub fn value(mut self, range: DateRange) -> Self {
        self.value = Some(range);
        self
    }

    /// Sets the range, or leaves the picker empty.
    pub fn value_opt(mut self, range: Option<DateRange>) -> Self {
        self.value = range;
        self
    }

    /// Which days can't be chosen. A range may not hold a day that can't be chosen.
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

    /// Today, as the page counts it: in the zone of the workspace or the person that
    /// the page's days are counted in. Every preset is counted from it.
    pub fn today(mut self, today: NaiveDate) -> Self {
        self.today = today;
        self
    }

    /// The first day of the week, for the calendar and for the week presets.
    pub fn week_start(mut self, day: Weekday) -> Self {
        self.week_start = Some(day);
        self
    }

    /// The presets to offer, in order. An empty list shows none.
    pub fn presets(mut self, presets: impl IntoIterator<Item = DatePreset>) -> Self {
        self.presets = presets.into_iter().collect();
        self
    }

    /// Words for one preset's button, in place of the English ones.
    pub fn preset_label(mut self, preset: DatePreset, text: impl Into<String>) -> Self {
        self.preset_labels.push((preset, text.into()));
        self
    }

    /// Leaves out the "Custom range" entry that sends a person to the calendar.
    pub fn without_custom(mut self) -> Self {
        self.custom = false;
        self
    }

    /// How many months the calendar shows, 1 or 2. Below 640 pixels it shows one.
    pub fn months(mut self, months: u8) -> Self {
        self.months = months.clamp(1, 2);
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

    /// The words on the button while no range is chosen.
    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = Some(text.into());
        self
    }

    /// A title shown on the button before the range, such as "Created".
    pub fn title(mut self, text: impl Into<String>) -> Self {
        self.title = Some(text.into());
        self
    }

    /// Leaves out the button that empties the picker.
    pub fn not_clearable(mut self) -> Self {
        self.clearable = false;
        self
    }

    /// Holds a choice until Apply is pressed. Cancel, Escape and a click outside
    /// drop it.
    pub fn confirm(mut self) -> Self {
        self.confirm = true;
        self
    }

    /// Requires a range. The check is the server's: a hidden field can't show the
    /// browser's own message.
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// Turns the picker off: nothing can be chosen and nothing is posted.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Marks it invalid.
    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    /// Shows week numbers in the calendar.
    pub fn week_numbers(mut self) -> Self {
        self.week_numbers = true;
        self
    }

    /// Sets an attribute on the root element.
    pub fn attr(mut self, name: &'static str, value: impl Into<AttrValue>) -> Self {
        self.el = self.el.attr(name, value);
        self
    }

    /// Whether a preset's range may be chosen under this picker's limits.
    fn preset_range(&self, preset: DatePreset) -> Option<DateRange> {
        let start = self.week_start.unwrap_or(self.locale.first_day);
        let range = preset.range(self.today, start, self.rules.earliest())?;
        if let Some(min) = self.rules.earliest()
            && range.from < min
        {
            return None;
        }
        if let Some(max) = self.rules.latest()
            && range.to > max
        {
            return None;
        }
        if self
            .min_days
            .is_some_and(|days| range.day_count() < i64::from(days))
            || self
                .max_days
                .is_some_and(|days| range.day_count() > i64::from(days))
        {
            return None;
        }
        Some(range)
    }

    fn into_node(self) -> Node {
        let root_id = self
            .id
            .clone()
            .unwrap_or_else(|| format!("dr-{}", id_from(&self.from_name)));
        let pop_id = format!("{root_id}-pop");
        let labels = self.locale.labels.clone();
        let start = self.week_start.unwrap_or(self.locale.first_day);
        let date_text = |day: NaiveDate| day.to_string();
        let native = |name: &str, which: &'static str, label: &str, value: Option<NaiveDate>| {
            el("label").slot("range-native-field").children((
                el("span").child(label.to_owned()),
                el("input")
                    .slot("input")
                    .attr("type", "date")
                    .attr("name", name.to_owned())
                    .attr_opt("value", value.map(date_text))
                    .attr_opt("min", self.rules.earliest().map(|d| d.to_string()))
                    .attr_opt("max", self.rules.latest().map(|d| d.to_string()))
                    .attr("data-ui-native", which)
                    .flag("required", self.required)
                    .flag("disabled", self.disabled),
            ))
        };
        let natives = el("div").slot("range-native").children((
            native(
                &self.from_name,
                "from",
                &labels.from,
                self.value.map(|r| r.from),
            ),
            native(&self.to_name, "to", &labels.to, self.value.map(|r| r.to)),
        ));
        let mut calendar = Calendar::new(format!("{root_id}-cal"))
            .mode(CalendarMode::Range)
            .locale(self.locale.clone())
            .today(self.today)
            .rules(self.rules.clone())
            .months(self.months)
            .label(labels.pick_range.to_string())
            .embedded();
        if let Some(range) = self.value {
            calendar = calendar.selected_range(range);
        }
        if let Some(days) = self.min_days {
            calendar = calendar.min_days(days);
        }
        if let Some(days) = self.max_days {
            calendar = calendar.max_days(days);
        }
        if let Some(first) = self.week_start {
            calendar = calendar.week_start(first);
        }
        if self.week_numbers {
            calendar = calendar.week_numbers();
        }
        // The presets: each carries its range, so choosing one needs no clock.
        let mut matched = false;
        let mut items: Vec<Element> = Vec::new();
        for preset in &self.presets {
            let range = self.preset_range(*preset);
            let active = range.is_some() && range == self.value;
            matched |= active;
            let text = self
                .preset_labels
                .iter()
                .find(|(p, _)| p == preset)
                .map_or_else(|| preset.label().to_owned(), |(_, text)| text.clone());
            let button = Button::new(text)
                .variant(ButtonVariant::Ghost)
                .size(Size::Sm)
                .attr("data-ui-preset", preset.key())
                .disabled(range.is_none() || self.disabled);
            let mut element = button_element(button);
            if let Some(range) = range {
                element = element
                    .attr("data-from", range.from.to_string())
                    .attr("data-to", range.to.to_string());
            }
            if active {
                element = element
                    .attr("aria-pressed", "true")
                    .flag("data-active", true);
            } else {
                element = element.attr("aria-pressed", "false");
            }
            items.push(element);
        }
        if self.custom && !self.presets.is_empty() {
            let active = self.value.is_some() && !matched;
            let button = Button::new("Custom range")
                .variant(ButtonVariant::Ghost)
                .size(Size::Sm)
                .attr("data-ui-preset", "custom")
                .disabled(self.disabled);
            items.push(
                button_element(button)
                    .attr("aria-pressed", if active { "true" } else { "false" })
                    .flag("data-active", active),
            );
        }
        let presets = (!items.is_empty()).then(|| {
            el("div")
                .slot("range-presets")
                .attr("role", "group")
                .attr("aria-label", "Presets")
                .child(items)
        });
        let placeholder = self
            .placeholder
            .clone()
            .unwrap_or_else(|| labels.pick_range.to_string());
        let shown = self.value.map_or_else(
            || placeholder.clone(),
            |range| self.locale.format_range(&range, self.today),
        );
        let full = self.value.map(|range| {
            format!(
                "{} – {}",
                self.locale.format_date(range.from),
                self.locale.format_date(range.to)
            )
        });
        let trigger = button_element(
            Button::new(
                el("span").slot("range-label").children((
                    self.title
                        .clone()
                        .map(|title| el("span").slot("range-title").child(title)),
                    el("span")
                        .attr("data-ui-label", "")
                        .attr("data-empty", placeholder.clone())
                        .child(shown),
                )),
            )
            .variant(ButtonVariant::Outline)
            .icon(icons::CALENDAR)
            .attr("popovertarget", pop_id.clone())
            .attr("aria-haspopup", "dialog")
            .attr("data-ui-act", "open")
            .disabled(self.disabled),
        )
        .attr_opt("title", full);
        let clear = self.clearable.then(|| {
            button_element(
                Button::icon_only(icons::X, labels.clear.to_string())
                    .variant(ButtonVariant::Ghost)
                    .size(Size::Sm)
                    .attr("data-ui-act", "clear")
                    .disabled(self.disabled),
            )
        });
        let foot = el("div").slot("range-foot").children((
            self.clearable.then(|| {
                button_element(
                    Button::new(labels.clear.to_string())
                        .variant(ButtonVariant::Ghost)
                        .size(Size::Sm)
                        .attr("data-ui-act", "clear"),
                )
            }),
            self.confirm.then(|| {
                el("div").slot("range-foot-actions").children((
                    button_element(
                        Button::new(labels.cancel.to_string())
                            .variant(ButtonVariant::Outline)
                            .size(Size::Sm)
                            .attr("data-ui-act", "cancel"),
                    ),
                    button_element(
                        Button::new(labels.apply.to_string())
                            .variant(ButtonVariant::Primary)
                            .size(Size::Sm)
                            .attr("data-ui-act", "apply"),
                    ),
                ))
            }),
        ));
        let align = if self.locale.rtl { "end" } else { "start" };
        let popup = el("div")
            .slot("popover-content")
            .id(pop_id.clone())
            .attr("popover", "auto")
            .attr("role", "dialog")
            .attr("tabindex", -1_i64)
            .attr("aria-label", labels.pick_range.to_string())
            .flag("data-ui-place", true)
            .attr("data-side", "bottom")
            .attr("data-align", align)
            .attr("data-variant", "range")
            .uses(Uses::OVERLAY)
            .child(
                el("div")
                    .slot("range-body")
                    .children((presets, calendar.build())),
            )
            .child((self.clearable || self.confirm).then_some(foot));
        self.el
            .id(root_id)
            .attr("data-ui-dates", "range")
            .attr("data-today", self.today.to_string())
            .attr("data-week-start", i64::from(start.num_days_from_sunday()))
            .attr_opt("data-min-days", self.min_days.map(i64::from))
            .attr_opt("data-max-days", self.max_days.map(i64::from))
            .attr_opt("dir", self.locale.rtl.then_some("rtl"))
            .flag("data-has-value", self.value.is_some())
            .flag("data-invalid", self.invalid)
            .flag("data-disabled", self.disabled)
            .flag("data-confirm", self.confirm)
            .uses(Uses::DATES)
            .children((
                natives,
                el("div").slot("date-range-ui").children((
                    el("div")
                        .slot("date-range-buttons")
                        .children((trigger, clear)),
                    popup,
                )),
            ))
            .into()
    }
}

into_node!(DateRangePicker);
