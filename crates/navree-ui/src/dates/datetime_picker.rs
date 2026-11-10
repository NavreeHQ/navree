//! [`DateTimePicker`]: a date and a time of day in one control.

use chrono::{NaiveDate, NaiveDateTime, Timelike, Utc, Weekday};

use super::locale::{Clock, DateLocale};
use super::picker::{DatePicker, id_from};
use super::rules::DateRules;
use super::time::TimeRules;
use super::time_picker::TimePicker;
use crate::node::{AttrValue, Element, Node, Uses, el, into_node};

/// What the picker was given as its value.
#[derive(Clone, Debug)]
enum Given {
    At(NaiveDateTime),
    Text(String),
}

/// A date and a time without a leap second or a fraction: the picker takes and posts
/// whole seconds.
fn whole_seconds(at: NaiveDateTime) -> NaiveDateTime {
    at.with_nanosecond(0).unwrap_or(at)
}

/// A date and a time of day together: a date box with its calendar and a time box with its
/// list, side by side in one control that posts one value.
///
/// It posts `name` as a date and a time on a wall clock, `2026-11-12T14:30` (with `:SS`
/// with [`seconds`](Self::seconds)), the form a browser's own date-and-time field posts,
/// and nothing until both halves are filled. With JavaScript off it is that field. Read it
/// back with [`DateTimePicker::read`], and check each half with the rules that drew it:
/// [`DateRules::check`] for the day and [`TimeRules::check`] for the time.
///
/// Like a [`TimePicker`], it has no zone: to store *when* something happens, resolve the
/// date and time in the zone they were meant in ([`zoned::resolve`](super::zoned::resolve)),
/// and say which zone that is with [`zone_note`](Self::zone_note).
///
/// ```
/// use navree_ui::dates::{DateLocale, DateTimePicker};
/// use navree_ui::render;
/// use chrono::NaiveDate;
///
/// let at = NaiveDate::from_ymd_opt(2026, 11, 12).unwrap().and_hms_opt(14, 30, 0).unwrap();
/// let html = render(&DateTimePicker::new("starts").value(at).locale(DateLocale::of("en-IN")).into());
/// assert!(html.contains(r#"type="datetime-local" name="starts" value="2026-11-12T14:30""#));
/// assert!(html.contains(r#"value="12/11/2026""#));
/// assert_eq!(DateTimePicker::read("2026-11-12T14:30"), Some(at));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct DateTimePicker {
    name: String,
    id: Option<String>,
    value: Option<Given>,
    date_rules: DateRules,
    time_rules: TimeRules,
    locale: DateLocale,
    today: NaiveDate,
    week_start: Option<Weekday>,
    clock: Option<Clock>,
    seconds: bool,
    clearable: bool,
    required: bool,
    disabled: bool,
    read_only: bool,
    invalid: bool,
    zone_note: Option<String>,
    label: Option<String>,
    el: Element,
}

impl DateTimePicker {
    /// A picker that posts as `name`.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            id: None,
            value: None,
            date_rules: DateRules::new(),
            time_rules: TimeRules::new(),
            locale: DateLocale::english(),
            today: Utc::now().date_naive(),
            week_start: None,
            clock: None,
            seconds: false,
            clearable: false,
            required: false,
            disabled: false,
            read_only: false,
            invalid: false,
            zone_note: None,
            label: None,
            el: el("div").slot("date-time-picker"),
        }
    }

    /// Reads what the picker posts: a date and a time, with or without seconds. Anything
    /// else is `None`.
    pub fn read(text: &str) -> Option<NaiveDateTime> {
        let text = text.trim();
        NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S")
            .or_else(|_| NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M"))
            .ok()
            .filter(|at| at.nanosecond() == 0)
    }

    /// Sets the id of the picker's own element. The control a label points at is the
    /// native field inside it, which a [`Field`](crate::Field) gives its id.
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Sets the date and time.
    pub fn value(mut self, at: NaiveDateTime) -> Self {
        self.value = Some(Given::At(whole_seconds(at)));
        self
    }

    /// Sets the date and time, or leaves the picker empty.
    pub fn value_opt(mut self, at: Option<NaiveDateTime>) -> Self {
        self.value = at.map(|at| Given::At(whole_seconds(at)));
        self
    }

    /// Sets the value from text a form sent back: a date and time as the picker posts it
    /// shows as that; anything else shows as typed, in the date box, so a person sees what
    /// they sent.
    pub fn value_text(mut self, text: &str) -> Self {
        let text = text.trim();
        self.value = if text.is_empty() {
            None
        } else {
            Some(Self::read(text).map_or_else(|| Given::Text(text.to_owned()), Given::At))
        };
        self
    }

    /// Which days can't be chosen.
    pub fn date_rules(mut self, rules: DateRules) -> Self {
        self.date_rules = rules;
        self
    }

    /// Which times of day can be chosen, and the step of the list.
    pub fn time_rules(mut self, rules: TimeRules) -> Self {
        self.time_rules = rules;
        self
    }

    /// The minutes between the times of the list: a number that divides a day.
    pub fn step_minutes(mut self, minutes: u32) -> Self {
        self.time_rules = self.time_rules.step_minutes(minutes);
        self
    }

    /// Names and writes dates and times after a language and region.
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

    /// Adds a button to each half that empties it.
    pub fn clearable(mut self) -> Self {
        self.clearable = true;
        self
    }

    /// Requires a date and a time.
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// Turns the picker off: nothing can be chosen and nothing is posted.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Shows the date and time without letting them change. They still post.
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
    /// `Times are in Asia/Kolkata (UTC+5:30)`.
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
            .unwrap_or_else(|| format!("dtp-{}", id_from(&self.name)));
        let labels = self.locale.labels.clone();
        let (at, raw) = match &self.value {
            Some(Given::At(at)) => (Some(*at), None),
            Some(Given::Text(text)) => (None, Some(text.clone())),
            None => (None, None),
        };
        let with_seconds = self.seconds || at.is_some_and(|at| at.second() != 0);
        // The joint field takes what the server takes and never less: the first and last
        // day, and the time's step (whole seconds when seconds are in play).
        let step = self.time_rules.native_limits(with_seconds).step;
        let native = el("input")
            .slot("input")
            .attr("type", "datetime-local")
            .attr("name", self.name.clone())
            .attr_opt(
                "value",
                at.map(|at| {
                    at.format(if with_seconds {
                        "%Y-%m-%dT%H:%M:%S"
                    } else {
                        "%Y-%m-%dT%H:%M"
                    })
                    .to_string()
                }),
            )
            .attr_opt(
                "min",
                self.date_rules.earliest().map(|day| format!("{day}T00:00")),
            )
            .attr_opt(
                "max",
                self.date_rules
                    .latest()
                    .map(|day| format!("{day}T23:59:59")),
            )
            .attr_opt("step", step.map(i64::from))
            .attr_opt("aria-label", self.label.clone())
            .attr_opt("aria-invalid", self.invalid.then_some("true"))
            .attr("data-ui-joint", "")
            .attr("autocomplete", "off")
            .flag("required", self.required)
            .flag("disabled", self.disabled)
            .flag("readonly", self.read_only);

        let invalid = self.invalid || raw.is_some();
        let mut date = DatePicker::new(self.name.clone())
            .part()
            .id(format!("{root_id}-date"))
            .locale(self.locale.clone())
            .today(self.today)
            .rules(self.date_rules.clone())
            .disabled(self.disabled)
            .invalid(invalid);
        date = match &raw {
            Some(text) => date.value_text(text),
            None => date.value_opt(at.map(|at| at.date())),
        };
        let mut time = TimePicker::new(self.name.clone())
            .part()
            .id(format!("{root_id}-time"))
            .locale(self.locale.clone())
            .rules(self.time_rules.clone())
            .value_opt(at.map(|at| at.time()))
            .disabled(self.disabled)
            .invalid(self.invalid)
            // The label of the whole control names the date box; the time box says what it is.
            .label(match &self.label {
                Some(label) => format!("{label}, {}", labels.pick_time),
                None => labels.pick_time.to_string(),
            });
        if let Some(label) = &self.label {
            date = date.label(label.clone());
        }
        if let Some(day) = self.week_start {
            date = date.week_start(day);
        }
        if let Some(clock) = self.clock {
            time = time.clock(clock);
        }
        if self.seconds {
            time = time.seconds();
        }
        if self.clearable {
            date = date.clearable();
            time = time.clearable();
        }
        if self.read_only {
            date = date.read_only();
            time = time.read_only();
        }
        let note = self
            .zone_note
            .clone()
            .map(|text| el("p").slot("field-description").child(text));
        self.el
            .id(root_id)
            .attr("role", "group")
            .attr("data-ui-dates", "datetime")
            .attr_opt("dir", self.locale.rtl.then_some("rtl"))
            .flag("data-invalid", invalid)
            .flag("data-disabled", self.disabled)
            .uses(Uses::TIME)
            .children((
                native,
                el("div").slot("date-time-picker-ui").children((date, time)),
                note,
            ))
            .into()
    }
}

into_node!(DateTimePicker);
