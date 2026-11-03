//! [`TimePicker`]: a time of day, typed or chosen from a list.

use chrono::{NaiveTime, Timelike};

use super::locale::{Clock, DateLocale};
use super::picker::id_from;
use super::time::TimeRules;
use crate::components::{Button, ButtonVariant, Size};
use crate::icons;
use crate::node::{AttrValue, Element, Node, Uses, el, into_node};

/// What the picker was given as its value.
#[derive(Clone, Debug)]
enum Given {
    Time(NaiveTime),
    Text(String),
}

/// A time of day: hours and minutes, and seconds if asked.
///
/// **A time here is a time on a wall.** It has no date and no zone, so `09:30` is
/// whatever 09:30 means where the person is. To store *when* something happens,
/// combine it with a date and the zone it was meant in, and keep the UTC instant:
/// [`zoned::resolve`](super::zoned::resolve) does that, and tells you about the
/// times that clock changes make impossible (`02:30` on the night clocks go forward)
/// or doubled (`01:30` when they go back). [`zone_note`](Self::zone_note) writes the
/// zone under the picker so a person knows which clock they are choosing on.
///
/// It posts `HH:MM`, or `HH:MM:SS` with [`seconds`](Self::seconds). The list offers
/// the times of [`TimeRules`] at its step; a person may type any other time unless
/// the rules are strict.
///
/// ```
/// use navree_ui::dates::{DateLocale, TimePicker};
/// use navree_ui::render;
/// use chrono::NaiveTime;
///
/// let t = NaiveTime::from_hms_opt(14, 30, 0).unwrap();
/// let html = render(&TimePicker::new("starts").value(t).locale(DateLocale::of("en")).into());
/// assert!(html.contains(r#"type="time" name="starts" value="14:30""#));
/// assert!(html.contains(r#"value="2:30 PM""#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct TimePicker {
    name: String,
    id: Option<String>,
    value: Option<Given>,
    rules: TimeRules,
    locale: DateLocale,
    clock: Option<Clock>,
    seconds: bool,
    placeholder: Option<String>,
    clearable: bool,
    required: bool,
    disabled: bool,
    read_only: bool,
    invalid: bool,
    zone_note: Option<String>,
    label: Option<String>,
    el: Element,
}

impl TimePicker {
    /// A picker that posts as `name`.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            id: None,
            value: None,
            rules: TimeRules::new(),
            locale: DateLocale::english(),
            clock: None,
            seconds: false,
            placeholder: None,
            clearable: false,
            required: false,
            disabled: false,
            read_only: false,
            invalid: false,
            zone_note: None,
            label: None,
            el: el("div").slot("time-picker"),
        }
    }

    /// Sets the id of the picker's own element.
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Sets the time.
    pub fn value(mut self, time: NaiveTime) -> Self {
        self.value = Some(Given::Time(time));
        self
    }

    /// Sets the time, or leaves the picker empty.
    pub fn value_opt(mut self, time: Option<NaiveTime>) -> Self {
        self.value = time.map(Given::Time);
        self
    }

    /// Sets the value from text a form sent back: `14:30` or `14:30:00` shows as
    /// that time; anything else shows as typed.
    pub fn value_text(mut self, text: &str) -> Self {
        let text = text.trim();
        self.value = if text.is_empty() {
            None
        } else {
            Some(
                NaiveTime::parse_from_str(text, "%H:%M:%S")
                    .or_else(|_| NaiveTime::parse_from_str(text, "%H:%M"))
                    .map_or_else(|_| Given::Text(text.to_owned()), Given::Time),
            )
        };
        self
    }

    /// Which times can be chosen, and the step of the list.
    pub fn rules(mut self, rules: TimeRules) -> Self {
        self.rules = rules;
        self
    }

    /// The minutes between the times of the list: a number that divides a day.
    pub fn step_minutes(mut self, minutes: u32) -> Self {
        self.rules = self.rules.step_minutes(minutes);
        self
    }

    /// The earliest time.
    pub fn min(mut self, time: NaiveTime) -> Self {
        self.rules = self.rules.min(time);
        self
    }

    /// The latest time.
    pub fn max(mut self, time: NaiveTime) -> Self {
        self.rules = self.rules.max(time);
        self
    }

    /// Names and writes times after a language and region.
    pub fn locale(mut self, locale: DateLocale) -> Self {
        self.locale = locale;
        self
    }

    /// Counts hours on a 12 or 24 hour clock, when the locale's isn't wanted.
    pub fn clock(mut self, clock: Clock) -> Self {
        self.clock = Some(clock);
        self
    }

    /// Adds seconds.
    pub fn seconds(mut self) -> Self {
        self.seconds = true;
        self
    }

    /// The hint in an empty box.
    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = Some(text.into());
        self
    }

    /// Adds a button that empties the picker.
    pub fn clearable(mut self) -> Self {
        self.clearable = true;
        self
    }

    /// Requires a time.
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// Turns the picker off: nothing can be chosen and nothing is posted.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Shows the time without letting it change. It still posts.
    pub fn read_only(mut self) -> Self {
        self.read_only = true;
        self
    }

    /// Marks it invalid.
    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    /// Writes a note under the picker about the clock it counts on, such as
    /// `Times are in Europe/London (UTC+0)`.
    pub fn zone_note(mut self, text: impl Into<String>) -> Self {
        self.zone_note = Some(text.into());
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
            .unwrap_or_else(|| format!("tp-{}", id_from(&self.name)));
        let pop_id = format!("{root_id}-pop");
        let list_id = format!("{root_id}-list");
        let mut locale = self.locale.clone();
        if let Some(clock) = self.clock {
            locale = locale.with_clock(clock);
        }
        let labels = locale.labels.clone();
        let (native_value, text, raw) = match &self.value {
            Some(Given::Time(time)) => (
                Some(if self.seconds || time.second() != 0 {
                    time.format("%H:%M:%S").to_string()
                } else {
                    time.format("%H:%M").to_string()
                }),
                locale.format_time(*time, self.seconds),
                false,
            ),
            Some(Given::Text(text)) => (None, text.clone(), true),
            None => (None, String::new(), false),
        };
        let has_value = native_value.is_some() || raw;
        // The step on the native control would make a typed 10:07 a mismatch; it is
        // set only when the rules are strict or seconds are asked for.
        let native_step = if self.seconds {
            Some(1)
        } else if self.rules.client().get("strict").is_some() {
            Some(i64::from(self.rules.step()) * 60)
        } else {
            None
        };
        let rules = self.rules.client();
        let native = el("input")
            .slot("input")
            .attr("type", "time")
            .attr("name", self.name.clone())
            .attr_opt("value", native_value)
            .attr_opt(
                "min",
                rules["min"]
                    .as_str()
                    .map(|text| text.chars().take(5).collect::<String>()),
            )
            .attr_opt(
                "max",
                rules["max"]
                    .as_str()
                    .map(|text| text.chars().take(5).collect::<String>()),
            )
            .attr_opt("step", native_step)
            .attr_opt("aria-label", self.label.clone())
            .attr_opt("aria-invalid", self.invalid.then_some("true"))
            .attr("data-ui-native", "")
            .flag("required", self.required)
            .flag("disabled", self.disabled)
            .flag("readonly", self.read_only);
        let example = locale.example_time();
        let placeholder = self.placeholder.clone().unwrap_or(example);
        let button = |glyph, label: &str, act: &'static str| -> Element {
            let mut button = Button::icon_only(glyph, label.to_owned())
                .variant(ButtonVariant::Ghost)
                .size(Size::Sm)
                .attr("data-ui-act", act)
                .disabled(self.disabled || self.read_only);
            if act == "open" {
                button = button
                    .attr("popovertarget", pop_id.clone())
                    .attr("aria-haspopup", "listbox");
            }
            let node: Node = button.into();
            node.into_element().unwrap_or_else(|| el("span"))
        };
        let control = el("div").slot("input-group").children((
            el("input")
                .slot("input")
                .attr("type", "text")
                .attr("data-ui-text", "")
                .attr("autocomplete", "off")
                .attr("placeholder", placeholder)
                .attr("value", text)
                .attr_opt("aria-label", self.label.clone())
                .attr_opt("aria-invalid", (self.invalid || raw).then_some("true"))
                .flag("required", self.required)
                .flag("disabled", self.disabled)
                .flag("readonly", self.read_only),
            el("div").slot("input-group-addon").children((
                self.clearable
                    .then(|| button(icons::X, &labels.clear, "clear")),
                button(icons::CLOCK, &labels.choose_time, "open"),
            )),
        ));
        let align = if locale.rtl { "start" } else { "end" };
        let popup = el("div")
            .slot("popover-content")
            .id(pop_id.clone())
            .attr("popover", "auto")
            .attr("role", "dialog")
            .attr("tabindex", -1_i64)
            .attr("aria-label", labels.choose_time.to_string())
            .flag("data-ui-place", true)
            .attr("data-side", "bottom")
            .attr("data-align", align)
            .attr("data-variant", "times")
            .uses(Uses::OVERLAY)
            .child(
                el("ul")
                    .slot("time-list")
                    .id(list_id)
                    .attr("role", "listbox")
                    .attr("tabindex", 0_i64)
                    .attr("aria-label", labels.choose_time.to_string()),
            );
        let note = self
            .zone_note
            .clone()
            .map(|text| el("p").slot("field-description").child(text));
        self.el
            .id(root_id)
            .attr("data-ui-dates", "time")
            .attr("data-ui-loc", locale.client().to_string())
            .attr("data-ui-trules", rules.to_string())
            .attr_opt("dir", locale.rtl.then_some("rtl"))
            .attr_opt("lang", Some(locale.tag.to_string()))
            .flag("data-seconds", self.seconds)
            .flag("data-has-value", has_value)
            .flag("data-invalid", self.invalid || raw)
            .flag("data-disabled", self.disabled)
            .uses(Uses::DATES)
            .children((
                native,
                el("div").slot("time-picker-ui").children((control, popup)),
                note,
            ))
            .into()
    }
}

into_node!(TimePicker);
