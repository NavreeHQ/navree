//! Slider: one value or a range, on native range inputs.
//!
//! A native `<input type="range">` is already a slider in every sense that
//! WAI-ARIA's pattern asks: its role, its value, the arrow, Home, End and Page keys,
//! a field that posts, and it works with no script. A range is two of them. The
//! `slider` module adds what a native input lacks: the filled part, ticks, the value
//! as text, a larger step, a gap between thumbs and a click on the track. What the
//! server knows (the filled part, the ticks) is drawn in SVG whose percentages are
//! attributes, since the content security policy forbids `style=""`.
//!
//! The server holds a posted value to the grid the browser draws: [`SliderSpec`]
//! is that grid, and `check` and `read` turn text into a number or an error.

use std::borrow::Cow;

use super::number::{Decimal, DecimalError};
use crate::node::{AttrValue, Element, Node, Uses, el, into_node};

/// The most ticks one slider draws.
const MAX_TICKS: i128 = 101;
/// The most marks (labelled or not) one slider takes.
const MAX_MARKS: usize = 100;
/// The most names for steps one slider takes.
const MAX_NAMES: usize = 1000;

/// Why a posted value is not one the slider allows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SliderError {
    /// Nothing was posted.
    Missing,
    /// The same field came twice.
    Duplicate,
    /// Not a plain number.
    NotANumber(DecimalError),
    /// Below the minimum.
    TooLow,
    /// Above the maximum.
    TooHigh,
    /// Between two steps.
    OffStep,
    /// A range whose lower value is above its upper.
    LowAboveHigh,
    /// A range whose thumbs are closer than the gap allows.
    TooClose,
}

impl std::fmt::Display for SliderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Missing => "Choose a value.",
            Self::Duplicate => "Choose one value.",
            Self::NotANumber(_) => "That is not a number.",
            Self::TooLow => "That is below the lowest value.",
            Self::TooHigh => "That is above the highest value.",
            Self::OffStep => "That is between two steps.",
            Self::LowAboveHigh => "The lower value can't be above the upper one.",
            Self::TooClose => "The two values are too close together.",
        })
    }
}

impl std::error::Error for SliderError {}

/// The words a slider uses. English by default. `{min}`, `{max}`, `{step}` and
/// `{gap}` are filled in.
#[derive(Clone, Debug)]
#[allow(missing_docs)]
pub struct SliderLabels {
    pub minimum: Cow<'static, str>,
    pub maximum: Cow<'static, str>,
    pub range_to: Cow<'static, str>,
    pub missing: Cow<'static, str>,
    pub duplicate: Cow<'static, str>,
    pub not_a_number: Cow<'static, str>,
    pub too_low: Cow<'static, str>,
    pub too_high: Cow<'static, str>,
    pub off_step: Cow<'static, str>,
    pub low_above_high: Cow<'static, str>,
    pub too_close: Cow<'static, str>,
}

impl Default for SliderLabels {
    fn default() -> Self {
        Self {
            minimum: "minimum".into(),
            maximum: "maximum".into(),
            range_to: "to".into(),
            missing: "Choose a value.".into(),
            duplicate: "Choose one value.".into(),
            not_a_number: "Enter a number.".into(),
            too_low: "Choose {min} or more.".into(),
            too_high: "Choose {max} or less.".into(),
            off_step: "Choose a value in steps of {step}.".into(),
            low_above_high: "The lower value can't be above the upper one.".into(),
            too_close: "The two values must be at least {gap} apart.".into(),
        }
    }
}

/// The grid of values a slider allows: the minimum, the maximum and the step. The
/// browser draws it and the server holds a post to it.
///
/// ```
/// use navree_ui::flow::SliderSpec;
///
/// let grid = SliderSpec::new(0, 100).step(5);
/// assert!(grid.check("35").is_ok());
/// assert!(grid.check("36").is_err());
/// assert!(grid.check("105").is_err());
/// assert!(grid.check("1e2").is_err());
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SliderSpec {
    min: Decimal,
    max: Decimal,
    step: Option<Decimal>,
    gap: u32,
}

impl SliderSpec {
    /// A grid from `min` to `max` in steps of 1. A maximum below the minimum
    /// swaps them.
    pub fn new(min: impl Into<Decimal>, max: impl Into<Decimal>) -> Self {
        let (min, max) = (min.into(), max.into());
        let (min, max) = if max < min { (max, min) } else { (min, max) };
        Self {
            min,
            max,
            step: Some(Decimal::ONE),
            gap: 0,
        }
    }

    /// Sets the step. A step of zero or less is 1, as in HTML.
    pub fn step(mut self, step: impl Into<Decimal>) -> Self {
        let step = step.into();
        self.step = Some(if step > Decimal::ZERO { step } else { Decimal::ONE });
        self
    }

    /// Allows any value between the minimum and the maximum.
    pub fn any_step(mut self) -> Self {
        self.step = None;
        self
    }

    /// The fewest steps between the two values of a range.
    pub fn min_gap(mut self, steps: u32) -> Self {
        self.gap = steps;
        self
    }

    /// The minimum.
    pub fn minimum(&self) -> Decimal {
        self.min
    }

    /// The maximum.
    pub fn maximum(&self) -> Decimal {
        self.max
    }

    /// The step, or `None` for any value.
    pub fn step_size(&self) -> Option<Decimal> {
        self.step
    }

    /// The greatest value the grid holds: the maximum, or the last step below it.
    pub fn top(&self) -> Decimal {
        match self.step {
            Some(step) => {
                let span = self.max.units() - self.min.units();
                let count = span / step.units();
                Decimal::from_units(self.min.units() + count * step.units())
            }
            None => self.max,
        }
    }

    /// The nearest value on the grid to `value`.
    pub fn snap(&self, value: Decimal) -> Decimal {
        let held = value.max(self.min).min(self.max);
        match self.step {
            Some(step) => held.snapped(self.min, step).min(self.top()),
            None => held,
        }
    }

    /// Whether `value` is on the grid.
    pub fn allows(&self, value: Decimal) -> bool {
        value >= self.min
            && value <= self.max
            && self.step.is_none_or(|step| {
                (value.units() - self.min.units()).rem_euclid(step.units()) == 0
            })
    }

    /// Reads one value from text and holds it to the grid.
    pub fn check(&self, text: &str) -> Result<Decimal, SliderError> {
        if text.is_empty() {
            return Err(SliderError::Missing);
        }
        let value: Decimal = text.parse().map_err(SliderError::NotANumber)?;
        if value < self.min {
            return Err(SliderError::TooLow);
        }
        if value > self.max {
            return Err(SliderError::TooHigh);
        }
        if !self.allows(value) {
            return Err(SliderError::OffStep);
        }
        Ok(value)
    }

    /// Reads the two values of a range and holds them to the grid and the gap.
    pub fn check_range(&self, low: &str, high: &str) -> Result<(Decimal, Decimal), SliderError> {
        let (low, high) = (self.check(low)?, self.check(high)?);
        if low > high {
            return Err(SliderError::LowAboveHigh);
        }
        if let Some(step) = self.step
            && (high.units() - low.units()) < i128::from(self.gap) * step.units()
        {
            return Err(SliderError::TooClose);
        }
        Ok((low, high))
    }

    /// The value posted as `name` in a form's pairs. Two of the same name are an
    /// error, not a choice.
    pub fn read(&self, pairs: &[(String, String)], name: &str) -> Result<Decimal, SliderError> {
        self.check(one(pairs, name)?)
    }

    /// The range posted as `{name}_min` and `{name}_max`.
    pub fn read_range(
        &self,
        pairs: &[(String, String)],
        name: &str,
    ) -> Result<(Decimal, Decimal), SliderError> {
        let low = one(pairs, &format!("{name}_min"))?;
        let high = one(pairs, &format!("{name}_max"))?;
        self.check_range(low, high)
    }

    /// The error in words, with the grid's numbers in them.
    pub fn message(&self, error: SliderError, labels: &SliderLabels) -> String {
        let words = match error {
            SliderError::Missing => &labels.missing,
            SliderError::Duplicate => &labels.duplicate,
            SliderError::NotANumber(_) => &labels.not_a_number,
            SliderError::TooLow => &labels.too_low,
            SliderError::TooHigh => &labels.too_high,
            SliderError::OffStep => &labels.off_step,
            SliderError::LowAboveHigh => &labels.low_above_high,
            SliderError::TooClose => &labels.too_close,
        };
        let step = self.step.unwrap_or(Decimal::ONE);
        let gap = step.times(i64::from(self.gap)).unwrap_or(step);
        words
            .replace("{min}", &self.min.to_string())
            .replace("{max}", &self.max.to_string())
            .replace("{step}", &step.to_string())
            .replace("{gap}", &gap.to_string())
    }

    /// Where `value` is along the track, from 0 to 100.
    fn percent(&self, value: Decimal) -> f64 {
        let span = self.max.units() - self.min.units();
        if span <= 0 {
            return 0.0;
        }
        let offset = value.units() - self.min.units();
        (offset as f64 / span as f64 * 100.0).clamp(0.0, 100.0)
    }

    /// The decimals a value is written with.
    fn decimals(&self) -> u32 {
        self.step
            .map_or(0, Decimal::decimals)
            .max(self.min.decimals())
    }
}

/// The only pair named `name`.
fn one<'a>(pairs: &'a [(String, String)], name: &str) -> Result<&'a str, SliderError> {
    let mut found = pairs.iter().filter(|(key, _)| key == name);
    let first = found.next().ok_or(SliderError::Missing)?;
    if found.next().is_some() {
        return Err(SliderError::Duplicate);
    }
    Ok(&first.1)
}

/// A percentage as an attribute value: at most three decimals, no trailing zeros.
fn pct(value: f64) -> String {
    let text = format!("{:.3}", value.clamp(0.0, 100.0));
    let text = text.trim_end_matches('0').trim_end_matches('.');
    format!("{}%", if text.is_empty() { "0" } else { text })
}

/// A slider, single or a range.
///
/// ```
/// use navree_ui::{Slider, render};
///
/// let volume = Slider::new("volume").label("Volume").max(100).step(5).value(40);
/// let html = render(&volume.into());
/// assert!(html.contains(r#"type="range""#));
/// assert!(html.contains(r#"name="volume""#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Slider {
    el: Element,
    name: String,
    range: bool,
    spec: SliderSpec,
    low: Decimal,
    high: Decimal,
    id: Option<String>,
    label: Option<String>,
    labelled_by: Option<String>,
    marks: Vec<(Decimal, Option<String>)>,
    ticks_every: Option<u32>,
    names: Vec<String>,
    prefix: Option<String>,
    suffix: Option<String>,
    show_value: bool,
    big_step: Option<Decimal>,
    disabled: bool,
    readonly: bool,
    invalid: bool,
    labels: SliderLabels,
}

impl Slider {
    fn with(name: String, range: bool) -> Self {
        let spec = SliderSpec::new(0, 100);
        Self {
            el: el("div").slot("slider"),
            name,
            range,
            spec,
            low: spec.minimum(),
            high: if range { spec.maximum() } else { spec.minimum() },
            id: None,
            label: None,
            labelled_by: None,
            marks: Vec::new(),
            ticks_every: None,
            names: Vec::new(),
            prefix: None,
            suffix: None,
            show_value: true,
            big_step: None,
            disabled: false,
            readonly: false,
            invalid: false,
            labels: SliderLabels::default(),
        }
    }

    /// A slider with one thumb, posting its value as `name`.
    pub fn new(name: impl Into<String>) -> Self {
        Self::with(name.into(), false)
    }

    /// A slider with two thumbs, posting `{name}_min` and `{name}_max`.
    pub fn range(name: impl Into<String>) -> Self {
        Self::with(name.into(), true)
    }

    /// Sets the whole grid at once.
    pub fn spec(mut self, spec: SliderSpec) -> Self {
        self.spec = spec;
        self
    }

    /// Sets the minimum.
    pub fn min(mut self, min: impl Into<Decimal>) -> Self {
        let max = self.spec.maximum();
        self.spec = self.rebuilt(min.into(), max);
        self
    }

    /// Sets the maximum.
    pub fn max(mut self, max: impl Into<Decimal>) -> Self {
        let min = self.spec.minimum();
        self.spec = self.rebuilt(min, max.into());
        self
    }

    fn rebuilt(&self, min: Decimal, max: Decimal) -> SliderSpec {
        let mut spec = SliderSpec::new(min, max);
        spec.step = self.spec.step;
        spec.gap = self.spec.gap;
        spec
    }

    /// Sets the step. Zero or less is 1.
    pub fn step(mut self, step: impl Into<Decimal>) -> Self {
        self.spec = self.spec.step(step);
        self
    }

    /// Allows any value between the minimum and the maximum.
    pub fn any_step(mut self) -> Self {
        self.spec = self.spec.any_step();
        self
    }

    /// Sets the value of a single slider. A value off the grid or outside it is
    /// moved to the nearest one the grid holds.
    pub fn value(mut self, value: impl Into<Decimal>) -> Self {
        self.low = value.into();
        self
    }

    /// Sets the values of a range.
    pub fn values(mut self, low: impl Into<Decimal>, high: impl Into<Decimal>) -> Self {
        self.low = low.into();
        self.high = high.into();
        self
    }

    /// The fewest steps between the two thumbs of a range.
    pub fn min_gap(mut self, steps: u32) -> Self {
        self.spec = self.spec.min_gap(steps);
        self
    }

    /// Names the slider for assistive technology, when no visible label does.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Says which element's text names the slider.
    pub fn labelled_by(mut self, id: impl Into<String>) -> Self {
        self.labelled_by = Some(id.into());
        self
    }

    /// Sets the thumb's `id` (the first thumb's, with `-min` and `-max` for a
    /// range), so a `<label for>` finds it. Without one the name makes it.
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Draws a mark at `value`, with a label under it when `label` is not empty.
    /// A value outside the track is left out.
    pub fn mark(mut self, value: impl Into<Decimal>, label: impl Into<String>) -> Self {
        if self.marks.len() < MAX_MARKS {
            let label = label.into();
            self.marks
                .push((value.into(), (!label.is_empty()).then_some(label)));
        }
        self
    }

    /// Draws a tick at every `steps`th step, starting at the minimum. No more than
    /// 101 are drawn: a finer grid gets fewer.
    pub fn ticks_every(mut self, steps: u32) -> Self {
        self.ticks_every = Some(steps.max(1));
        self
    }

    /// Names the steps, from the first. The name is what assistive technology
    /// hears for that step, it is written in place of the number, and with up to
    /// twelve names each one is drawn under its step.
    pub fn names<S: Into<String>>(mut self, names: impl IntoIterator<Item = S>) -> Self {
        self.names = names.into_iter().take(MAX_NAMES).map(Into::into).collect();
        self
    }

    /// Text before the value: `$`.
    pub fn prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }

    /// Text after the value: `%` or ` min`.
    pub fn suffix(mut self, suffix: impl Into<String>) -> Self {
        self.suffix = Some(suffix.into());
        self
    }

    /// Leaves out the value written beside the slider.
    pub fn hide_value(mut self) -> Self {
        self.show_value = false;
        self
    }

    /// What Page Up, Page Down and Shift with the arrows move by. Without one it
    /// is a tenth of the track, and at least a step.
    pub fn big_step(mut self, big: impl Into<Decimal>) -> Self {
        self.big_step = Some(big.into());
        self
    }

    /// Disables it: it posts nothing.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Shows the value and keeps it still, but posts it: a disabled slider with
    /// the value in hidden fields.
    pub fn readonly(mut self) -> Self {
        self.readonly = true;
        self
    }

    /// Marks it as holding a bad value.
    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    /// Sets the words.
    pub fn labels(mut self, labels: SliderLabels) -> Self {
        self.labels = labels;
        self
    }

    /// Sets an attribute on the root element.
    pub fn attr(mut self, name: &'static str, value: impl Into<AttrValue>) -> Self {
        self.el = self.el.attr(name, value);
        self
    }

    /// The value as it is written: the step's name, or the number with its
    /// prefix and suffix.
    fn text_of(&self, value: Decimal) -> String {
        if let Some(name) = self.name_of(value) {
            return name.to_owned();
        }
        format!(
            "{}{}{}",
            self.prefix.as_deref().unwrap_or(""),
            value.fixed(self.spec.decimals()),
            self.suffix.as_deref().unwrap_or("")
        )
    }

    fn name_of(&self, value: Decimal) -> Option<&str> {
        let step = self.spec.step?;
        if self.names.is_empty() {
            return None;
        }
        let offset = value.units() - self.spec.minimum().units();
        if offset < 0 || offset % step.units() != 0 {
            return None;
        }
        let at = usize::try_from(offset / step.units()).ok()?;
        self.names.get(at).map(String::as_str)
    }

    fn has_text(&self) -> bool {
        !self.names.is_empty() || self.prefix.is_some() || self.suffix.is_some()
    }

    fn into_node(self) -> Node {
        let spec = self.spec;
        let low = spec.snap(self.low);
        let high = if self.range {
            spec.snap(self.high).max(low)
        } else {
            low
        };
        let base = self.id.clone().unwrap_or_else(|| {
            let safe: String = self
                .name
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
                .collect();
            format!("slider-{safe}")
        });
        let ids: Vec<String> = if self.range {
            vec![format!("{base}-min"), format!("{base}-max")]
        } else {
            vec![base.clone()]
        };
        let step_text = spec
            .step_size()
            .map_or_else(|| "any".to_owned(), |step| step.to_string());
        let off = self.disabled || self.readonly;

        let thumb = |at: usize, value: Decimal| {
            let (name, id, kind) = if self.range {
                let name = format!("{}_{}", self.name, if at == 0 { "min" } else { "max" });
                (name, ids[at].clone(), if at == 0 { "min" } else { "max" })
            } else {
                (self.name.clone(), ids[0].clone(), "value")
            };
            let word = if at == 0 {
                &self.labels.minimum
            } else {
                &self.labels.maximum
            };
            let mut input = el("input")
                .slot("slider-input")
                .attr("type", "range")
                .attr("id", id.clone())
                .attr("min", spec.minimum().to_string())
                .attr("max", spec.maximum().to_string())
                .attr("step", step_text.clone())
                .attr("value", value.to_string())
                .attr("data-thumb", kind)
                .flag("disabled", off)
                .attr_opt("aria-invalid", self.invalid.then_some("true"));
            if !off {
                input = input.attr("name", name.clone());
            }
            // The name of a thumb: the group's label, and for a range which end it is.
            input = match (&self.labelled_by, &self.label, self.range) {
                (Some(by), _, true) => input
                    .attr("aria-labelledby", format!("{by} {id}"))
                    .attr("aria-label", word.to_string()),
                (Some(by), _, false) => input.attr("aria-labelledby", by.clone()),
                (None, Some(label), true) => {
                    input.attr("aria-label", format!("{label} {word}"))
                }
                (None, Some(label), false) => input.attr("aria-label", label.clone()),
                (None, None, true) => input.attr("aria-label", word.to_string()),
                (None, None, false) => input,
            };
            if self.has_text() {
                input = input.attr("aria-valuetext", self.text_of(value));
            }
            (input, name, value)
        };

        let thumbs: Vec<(Element, String, Decimal)> = if self.range {
            vec![thumb(0, low), thumb(1, high)]
        } else {
            vec![thumb(0, low)]
        };
        // A read-only slider posts what it shows through hidden fields.
        let hidden: Vec<Element> = if self.readonly && !self.disabled {
            thumbs
                .iter()
                .map(|(_, name, value)| {
                    el("input")
                        .attr("type", "hidden")
                        .attr("name", name.clone())
                        .attr("value", value.to_string())
                })
                .collect()
        } else {
            Vec::new()
        };

        let (fill_from, fill_to) = if self.range {
            (spec.percent(low), spec.percent(high))
        } else {
            (0.0, spec.percent(low))
        };
        let graphic = el("svg")
            .slot("slider-graphic")
            .attr("aria-hidden", "true")
            .attr("focusable", "false")
            .attr("height", 24_i64)
            .children((
                el("rect")
                    .slot("slider-track")
                    .attr("x", "0")
                    .attr("y", 10_i64)
                    .attr("width", "100%")
                    .attr("height", 4_i64)
                    .attr("rx", 2_i64),
                el("rect")
                    .slot("slider-fill")
                    .attr("x", pct(fill_from))
                    .attr("y", 10_i64)
                    .attr("width", pct(fill_to - fill_from))
                    .attr("height", 4_i64),
            ));

        let marks = self.marks_node();
        let output = self.show_value.then(|| {
            let text = if self.range {
                format!(
                    "{} {} {}",
                    self.text_of(low),
                    self.labels.range_to,
                    self.text_of(high)
                )
            } else {
                self.text_of(low)
            };
            el("output")
                .slot("slider-value")
                .attr("for", ids.join(" "))
                .child(text)
        });
        let names_json = (!self.names.is_empty())
            .then(|| serde_json::to_string(&self.names).unwrap_or_default());
        let big = self.big_step.filter(|big| *big > Decimal::ZERO);

        self.el
            .attr("data-ui-slider", "")
            .flag("data-range", self.range)
            .flag("data-disabled", self.disabled)
            .flag("data-readonly", self.readonly)
            .flag("data-invalid", self.invalid)
            .flag("data-labelled", marks.1)
            .attr("data-decimals", i64::from(spec.decimals()))
            .attr_opt("data-gap", (spec.gap > 0).then(|| i64::from(spec.gap)))
            .attr_opt("data-big", big.map(|big| big.to_string()))
            .attr_opt("data-prefix", self.prefix.clone())
            .attr_opt("data-suffix", self.suffix.clone())
            .attr_opt("data-names", names_json)
            .attr("data-to", self.labels.range_to.to_string())
            .uses(Uses::SLIDER)
            .children((
                el("div")
                    .slot("slider-control")
                    .children((graphic, thumbs.into_iter().map(|(input, ..)| input).collect::<Vec<_>>())),
                marks.0,
                output,
                hidden,
            ))
            .into()
    }

    /// The ticks and labels under the track, and whether any has a label.
    fn marks_node(&self) -> (Option<Element>, bool) {
        let spec = self.spec;
        let mut marks: Vec<(f64, Option<String>)> = Vec::new();
        // Given marks first; a mark off the track is left out.
        for (value, label) in &self.marks {
            if *value >= spec.minimum() && *value <= spec.maximum() {
                marks.push((spec.percent(*value), label.clone()));
            }
        }
        if let (Some(every), Some(step)) = (self.ticks_every, spec.step_size()) {
            let count = (spec.maximum().units() - spec.minimum().units()) / step.units();
            // Thin the ticks until no more than the most are drawn.
            let mut every = i128::from(every);
            while count / every + 1 > MAX_TICKS {
                every *= 2;
            }
            let mut at = 0;
            while at <= count {
                let value = Decimal::from_units(spec.minimum().units() + at * step.units());
                marks.push((spec.percent(value), None));
                at += every;
            }
        }
        // Names of steps are drawn when there are few enough to read.
        if !self.names.is_empty() && self.names.len() <= 12 {
            if let Some(step) = spec.step_size() {
                for (at, name) in self.names.iter().enumerate() {
                    let units = spec.minimum().units() + at as i128 * step.units();
                    if units <= spec.maximum().units() {
                        marks.push((spec.percent(Decimal::from_units(units)), Some(name.clone())));
                    }
                }
            }
        }
        if marks.is_empty() {
            return (None, false);
        }
        let labelled = marks.iter().any(|(_, label)| label.is_some());
        // Drawn twice: once from the left and once from the right, and the stylesheet shows the one
        // that fits the page's direction. Nothing in the picture is mirrored by a transform, which
        // browsers do not agree on for text in SVG.
        let group = |flip: bool| -> Element {
            let at_of = |at: f64| if flip { 100.0 - at } else { at };
            let lines: Vec<Element> = marks
                .iter()
                .map(|(at, _)| {
                    el("line")
                        .attr("x1", pct(at_of(*at)))
                        .attr("x2", pct(at_of(*at)))
                        .attr("y1", 0_i64)
                        .attr("y2", 6_i64)
                })
                .collect();
            let texts: Vec<Element> = marks
                .iter()
                .filter_map(|(at, label)| {
                    let label = label.as_ref()?;
                    Some(
                        el("text")
                            .attr("x", pct(at_of(*at)))
                            .attr("y", 20_i64)
                            .attr("text-anchor", "middle")
                            .child(label.clone()),
                    )
                })
                .collect();
            el("g")
                .attr("data-dir", if flip { "rtl" } else { "ltr" })
                .children((
                    el("g").slot("slider-ticks").children(lines),
                    el("g").slot("slider-mark-labels").children(texts),
                ))
        };
        let label_count = marks.iter().filter(|(_, label)| label.is_some()).count();
        let marks = el("svg")
            .slot("slider-marks")
            .attr("aria-hidden", "true")
            .attr("focusable", "false")
            .attr("height", if labelled { 26_i64 } else { 8_i64 })
            .attr("data-labels", i64::try_from(label_count).unwrap_or(0))
            .children((group(false), group(true)));
        (Some(marks), labelled)
    }
}

into_node!(Slider);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{measure, render};

    fn html(slider: Slider) -> String {
        render(&slider.into())
    }

    #[test]
    fn a_single_slider_is_one_native_range_that_posts() {
        let out = html(Slider::new("volume").label("Volume").step(5).value(40));
        assert_eq!(out.matches(r#"type="range""#).count(), 1, "{out}");
        assert!(out.contains(r#"name="volume""#), "{out}");
        assert!(out.contains(r#"min="0""#), "{out}");
        assert!(out.contains(r#"max="100""#), "{out}");
        assert!(out.contains(r#"step="5""#), "{out}");
        assert!(out.contains(r#"value="40""#), "{out}");
        assert!(out.contains(r#"aria-label="Volume""#), "{out}");
        assert!(out.contains("<output"), "{out}");
    }

    #[test]
    fn a_range_is_two_native_ranges_with_names_for_each_end() {
        let out = html(Slider::range("price").label("Price").values(20, 80).max(100));
        assert_eq!(out.matches(r#"type="range""#).count(), 2, "{out}");
        assert!(out.contains(r#"name="price_min""#), "{out}");
        assert!(out.contains(r#"name="price_max""#), "{out}");
        assert!(out.contains(r#"aria-label="Price minimum""#), "{out}");
        assert!(out.contains(r#"aria-label="Price maximum""#), "{out}");
        assert!(out.contains("20 to 80"), "{out}");
    }

    #[test]
    fn the_filled_part_is_drawn_in_attributes_not_style() {
        let out = html(Slider::new("v").value(40));
        assert!(!out.contains("style="), "{out}");
        assert!(out.contains(r#"data-slot="slider-fill" x="0%" y="10" width="40%""#), "{out}");
        let range = html(Slider::range("r").values(25, 75));
        assert!(range.contains(r#"x="25%" y="10" width="50%""#), "{range}");
    }

    #[test]
    fn a_value_off_the_grid_or_outside_is_moved_to_the_grid() {
        assert!(html(Slider::new("v").step(5).value(38)).contains(r#"value="40""#));
        assert!(html(Slider::new("v").step(5).value(500)).contains(r#"value="100""#));
        assert!(html(Slider::new("v").step(5).value(-9)).contains(r#"value="0""#));
        // A maximum the steps don't reach: the last step is the top.
        assert!(html(Slider::new("v").max(10).step(3).value(10)).contains(r#"value="9""#));
        // Two thumbs never cross.
        let crossed = html(Slider::range("r").values(80, 20));
        assert!(crossed.contains(r#"value="80""#), "{crossed}");
        assert!(!crossed.contains(r#"value="20""#), "{crossed}");
    }

    #[test]
    fn decimal_steps_are_exact() {
        let out = html(Slider::new("v").max(1).step(0.1_f64).value(0.3_f64));
        assert!(out.contains(r#"step="0.1""#), "{out}");
        assert!(out.contains(r#"value="0.3""#), "{out}");
        assert!(out.contains(r#"data-decimals="1""#), "{out}");
        assert!(out.contains(">0.3</output>"), "{out}");
    }

    #[test]
    fn a_degenerate_grid_still_draws() {
        // The minimum equals the maximum; the step is larger than the track; a swapped pair.
        for slider in [
            Slider::new("v").min(5).max(5),
            Slider::new("v").max(3).step(100),
            Slider::new("v").min(10).max(0),
            Slider::range("r").min(7).max(7).values(7, 7),
        ] {
            let out = html(slider);
            assert!(out.contains(r#"type="range""#), "{out}");
            assert!(!out.contains("NaN") && !out.contains("inf"), "{out}");
        }
    }

    #[test]
    fn extreme_numbers_from_the_app_do_not_panic() {
        for (min, max, step) in [
            (f64::MIN, f64::MAX, f64::MIN_POSITIVE),
            (f64::NAN, f64::INFINITY, f64::NAN),
            (f64::NEG_INFINITY, f64::INFINITY, 0.0),
            (-1e300, 1e300, 1e-300),
        ] {
            let out = html(Slider::new("v").min(min).max(max).step(step).ticks_every(1).value(f64::NAN));
            assert!(out.contains("<input"), "{out}");
        }
        let out = html(Slider::range("r").min(i64::MIN).max(u64::MAX).step(1).ticks_every(1));
        assert!(out.matches("<line").count() <= 202, "{out}");
    }

    #[test]
    fn ticks_marks_and_names_are_drawn_where_they_belong() {
        let out = html(Slider::new("v").ticks_every(25));
        // Five ticks, drawn from each side.
        assert_eq!(out.matches("<line").count(), 10, "{out}");
        assert!(out.contains(r#"x1="50%""#), "{out}");
        assert!(out.contains(r#"data-dir="rtl""#), "{out}");
        let marked = html(Slider::new("v").mark(0, "0").mark(50, "Half").mark(500, "Off"));
        assert_eq!(marked.matches(">Half</text>").count(), 2, "{marked}");
        assert!(!marked.contains("Off"), "a mark off the track is left out: {marked}");
        assert!(marked.contains("data-labelled"), "{marked}");
        let named = html(Slider::new("v").max(2).names(["Low", "Mid", "High"]).value(1));
        assert!(named.contains(r#"aria-valuetext="Mid""#), "{named}");
        assert_eq!(named.matches(">High</text>").count(), 2, "{named}");
        assert!(named.contains(">Mid</output>"), "{named}");
    }

    #[test]
    fn a_fine_grid_gets_at_most_101_ticks() {
        let out = html(Slider::new("v").max(1_000_000).ticks_every(1));
        assert!(out.matches("<line").count() <= 202, "{}", out.matches("<line").count());
    }

    #[test]
    fn a_prefix_and_suffix_make_the_value_text() {
        let out = html(Slider::new("v").prefix("$").suffix(" a month").value(40));
        assert!(out.contains(r#"aria-valuetext="$40 a month""#), "{out}");
        assert!(out.contains(">$40 a month</output>"), "{out}");
        assert!(out.contains(r#"data-prefix="$""#), "{out}");
    }

    #[test]
    fn disabled_posts_nothing_and_read_only_posts_through_hidden_fields() {
        let off = html(Slider::new("v").value(3).disabled(true));
        assert!(off.contains("disabled"), "{off}");
        assert!(!off.contains("name="), "{off}");
        let still = html(Slider::range("r").values(1, 9).readonly());
        assert!(still.contains("disabled"), "{still}");
        assert!(still.contains(r#"type="hidden" name="r_min" value="1""#), "{still}");
        assert!(still.contains(r#"type="hidden" name="r_max" value="9""#), "{still}");
    }

    #[test]
    fn a_visible_label_names_the_thumbs() {
        let out = html(Slider::range("p").labelled_by("price-label"));
        assert!(out.contains(r#"aria-labelledby="price-label slider-p-min""#), "{out}");
        let single = html(Slider::new("p").labelled_by("price-label"));
        assert!(single.contains(r#"aria-labelledby="price-label""#), "{single}");
    }

    #[test]
    fn hostile_names_and_labels_are_text() {
        let out = html(
            Slider::range("a\"><script>")
                .label("<b>")
                .prefix("\"><img src=x>")
                .names(["<i>"])
                .mark(1, "</text><script>"),
        );
        assert!(!out.contains("<script>"), "{out}");
        assert!(!out.contains("<img"), "{out}");
        assert!(!out.contains("<b>"), "{out}");
        assert!(out.contains(r#"id="slider-a___script_-min""#), "{out}");
    }

    #[test]
    fn it_asks_for_the_slider_module_and_nothing_else() {
        let node: Node = Slider::new("v").into();
        assert_eq!(measure(&node).uses, Uses::SLIDER);
    }

    #[test]
    fn the_grid_holds_a_post_to_its_steps() {
        let grid = SliderSpec::new(0, 100).step(5);
        assert_eq!(grid.check("35"), Ok(Decimal::from(35)));
        assert_eq!(grid.check("36"), Err(SliderError::OffStep));
        assert_eq!(grid.check("-5"), Err(SliderError::TooLow));
        assert_eq!(grid.check("105"), Err(SliderError::TooHigh));
        assert_eq!(grid.check(""), Err(SliderError::Missing));
        for text in ["1e2", "0x10", "NaN", "10 ", " 10", "5px", "1,5", "--5", "ten"] {
            assert!(matches!(grid.check(text), Err(SliderError::NotANumber(_))), "{text}");
        }
        let tenths = SliderSpec::new(0, 1).step(0.1_f64);
        assert!(tenths.check("0.3").is_ok());
        assert!(tenths.check("0.35").is_err());
        assert!(tenths.check("0.30").is_ok());
        // From a minimum that isn't zero, the grid starts there.
        let offset = SliderSpec::new(3, 20).step(5);
        assert!(offset.check("8").is_ok());
        assert!(offset.check("10").is_err());
        assert!(offset.check("18").is_ok());
        assert!(offset.check("23").is_err());
        // Any step allows any value inside.
        assert!(SliderSpec::new(0, 1).any_step().check("0.123456789").is_ok());
        assert!(SliderSpec::new(0, 1).any_step().check("1.000000001").is_err());
    }

    #[test]
    fn a_range_is_held_to_its_order_and_its_gap() {
        let grid = SliderSpec::new(0, 100).step(10).min_gap(2);
        assert_eq!(grid.check_range("20", "60"), Ok((Decimal::from(20), Decimal::from(60))));
        assert_eq!(grid.check_range("60", "20"), Err(SliderError::LowAboveHigh));
        assert_eq!(grid.check_range("20", "30"), Err(SliderError::TooClose));
        assert_eq!(grid.check_range("20", "40").is_ok(), true);
        assert_eq!(grid.check_range("20", ""), Err(SliderError::Missing));
        assert_eq!(grid.check_range("21", "60"), Err(SliderError::OffStep));
    }

    #[test]
    fn reading_pairs_finds_the_field_and_refuses_doubles() {
        let grid = SliderSpec::new(0, 10);
        let pair = |k: &str, v: &str| (k.to_owned(), v.to_owned());
        assert_eq!(grid.read(&[pair("v", "3")], "v"), Ok(Decimal::from(3)));
        assert_eq!(grid.read(&[], "v"), Err(SliderError::Missing));
        assert_eq!(grid.read(&[pair("v", "3"), pair("v", "4")], "v"), Err(SliderError::Duplicate));
        assert_eq!(
            grid.read_range(&[pair("r_min", "2"), pair("r_max", "8")], "r"),
            Ok((Decimal::from(2), Decimal::from(8)))
        );
        assert_eq!(grid.read_range(&[pair("r_min", "2")], "r"), Err(SliderError::Missing));
        assert_eq!(
            grid.read_range(&[pair("r_min", "2"), pair("r_max", "8"), pair("r_max", "9")], "r"),
            Err(SliderError::Duplicate)
        );
    }

    #[test]
    fn errors_say_what_the_grid_is() {
        let grid = SliderSpec::new(0, 100).step(5).min_gap(2);
        let labels = SliderLabels::default();
        assert_eq!(grid.message(SliderError::TooLow, &labels), "Choose 0 or more.");
        assert_eq!(grid.message(SliderError::TooHigh, &labels), "Choose 100 or less.");
        assert_eq!(grid.message(SliderError::OffStep, &labels), "Choose a value in steps of 5.");
        assert_eq!(grid.message(SliderError::TooClose, &labels), "The two values must be at least 10 apart.");
    }

    /// A small generator, so the properties run the same everywhere.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
    }

    #[test]
    fn no_text_makes_a_value_the_grid_does_not_allow() {
        let alphabet: Vec<char> = "0123456789.-+eE \u{0}x".chars().collect();
        let mut rng = Rng(7);
        for _ in 0..100_000 {
            let min = Decimal::from_f64((rng.next() % 2000) as f64 / 10.0 - 100.0);
            let max = Decimal::from_f64((rng.next() % 2000) as f64 / 10.0 - 100.0);
            let step = Decimal::from_f64((1 + rng.next() % 500) as f64 / 100.0);
            let grid = SliderSpec::new(min, max).step(step);
            let len = (rng.next() % 12) as usize;
            let text: String = (0..len)
                .map(|_| alphabet[(rng.next() % alphabet.len() as u64) as usize])
                .collect();
            if let Ok(value) = grid.check(&text) {
                assert!(grid.allows(value), "{text:?} on {grid:?}");
                assert!(value <= grid.top(), "{text:?} on {grid:?}");
            }
            let snapped = grid.snap(Decimal::from_f64(rng.next() as f64 / 1e12 - 5000.0));
            assert!(grid.allows(snapped), "{snapped} on {grid:?}");
        }
    }
}
