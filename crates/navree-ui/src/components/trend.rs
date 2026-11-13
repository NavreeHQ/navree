//! [`Trend`]: which way something moved and by how much.

use std::borrow::Cow;

use super::{Tone, icon};
use crate::charts::{IntoValue, NumberFormat, NumberLocale};
use crate::icons;
use crate::node::{Node, Uses, el};

/// Which way something moved and by how much: an arrow, the change with its sign, and
/// the colour of what it means. Whether a rise is good news depends on the figure: more
/// steps are good, a higher resting heart rate is not, so the colour follows
/// [`up_is_bad`](Self::up_is_bad) and never the direction alone. The words "Up", "Down"
/// or "No change" are read by a screen reader before the figure.
///
/// ```
/// use navree_ui::{Stat, Tone, Trend, render};
///
/// let steps = Trend::new(12.4);
/// let resting = Trend::new(3.0).up_is_bad();
/// let html = render(&steps.into());
/// assert!(html.contains("+12.4%") && html.contains(r#"data-tone="success""#));
/// let html = render(&resting.into());
/// assert!(html.contains(r#"data-tone="danger""#));
///
/// // Beside a figure.
/// let stat = Stat::new("Steps", "9,412").change(Trend::new(-4.5));
/// # let _ = stat;
/// ```
///
/// A change that rounds to nothing is "No change" with a dash, and a missing change is
/// a dash with no colour.
#[derive(Clone, Debug)]
#[must_use]
pub struct Trend {
    change: Option<f64>,
    up_is_good: bool,
    tone: Option<Tone>,
    decimals: u8,
    suffix: Cow<'static, str>,
    numbers: NumberLocale,
    words: [Cow<'static, str>; 3],
}

impl Trend {
    /// A change of `change`, written with its sign and a percent sign: `12.4` is
    /// `+12.4%`. Use [`suffix`](Self::suffix) for another unit.
    pub fn new(change: impl IntoValue) -> Self {
        Self {
            change: change.into_value().filter(|v| v.is_finite()),
            up_is_good: true,
            tone: None,
            decimals: 1,
            suffix: Cow::Borrowed("%"),
            numbers: NumberLocale::english(),
            words: [
                Cow::Borrowed("Up"),
                Cow::Borrowed("Down"),
                Cow::Borrowed("No change"),
            ],
        }
    }

    /// A rise is bad news (resting heart rate, stress, debt): it is coloured as a
    /// warning, and a fall as good.
    pub fn up_is_bad(mut self) -> Self {
        self.up_is_good = false;
        self
    }

    /// Colours the trend with `tone` whichever way it moved: [`Tone::Neutral`] shows the
    /// direction with no judgement.
    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = Some(tone);
        self
    }

    /// The most decimals written, 0 to 3 (1 by default).
    pub fn decimals(mut self, places: u8) -> Self {
        self.decimals = places.min(3);
        self
    }

    /// What follows the number: `%` by default, ` bpm`, ` min`.
    pub fn suffix(mut self, suffix: impl Into<Cow<'static, str>>) -> Self {
        self.suffix = suffix.into();
        self
    }

    /// Writes the number for a language: `de`, `en-IN`.
    pub fn locale(mut self, tag: &str) -> Self {
        self.numbers = NumberLocale::of(tag);
        self
    }

    /// The words a screen reader hears before the figure.
    pub fn words(
        mut self,
        up: impl Into<Cow<'static, str>>,
        down: impl Into<Cow<'static, str>>,
        none: impl Into<Cow<'static, str>>,
    ) -> Self {
        self.words = [up.into(), down.into(), none.into()];
        self
    }

    fn into_node(self) -> Node {
        let root = el("span").slot("trend").uses(Uses::CHART_LINEAR);
        let Some(change) = self.change else {
            return root
                .children((
                    "–",
                    el("span")
                        .attr("data-ui-sr", "")
                        .child(self.words[2].clone()),
                ))
                .into();
        };
        let format = NumberFormat::decimals(self.decimals);
        let shown = format.format(change.abs(), &self.numbers);
        // A change that writes as zero is no change, whichever way it was going.
        let flat = shown.bytes().all(|b| !(b'1'..=b'9').contains(&b));
        let (direction, glyph, word) = if flat {
            ("flat", icons::MINUS, &self.words[2])
        } else if change > 0.0 {
            ("up", icons::TRENDING_UP, &self.words[0])
        } else {
            ("down", icons::TRENDING_DOWN, &self.words[1])
        };
        let tone = self.tone.unwrap_or(match (direction, self.up_is_good) {
            ("up", true) | ("down", false) => Tone::Success,
            ("up", false) | ("down", true) => Tone::Danger,
            _ => Tone::Neutral,
        });
        let sign = match direction {
            "up" => "+",
            "down" => "-",
            _ => "",
        };
        let text = if flat {
            format!("{shown}{}", self.suffix)
        } else {
            format!("{sign}{shown}{}", self.suffix)
        };
        root.attr("data-direction", direction)
            .attr_opt("data-tone", tone.value())
            .children((
                icon(glyph),
                el("span").attr("data-ui-sr", "").child(word.clone()),
                el("span").child(text),
            ))
            .into()
    }
}

crate::node::into_node!(Trend);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render;

    fn html(trend: Trend) -> String {
        render(&trend.into())
    }

    #[test]
    fn a_trend_is_an_arrow_a_signed_share_and_the_word_for_a_screen_reader() {
        let out = html(Trend::new(12.4));
        assert!(
            out.starts_with(r#"<span data-slot="trend" data-direction="up" data-tone="success">"#),
            "{out}"
        );
        assert!(
            out.contains(r#"<span data-ui-sr="">Up</span><span>+12.4%</span>"#),
            "{out}"
        );
        let out = html(Trend::new(-3.25).decimals(2));
        assert!(
            out.contains(r#"data-direction="down""#) && out.contains("-3.25%"),
            "{out}"
        );
        assert!(out.contains(r#"data-tone="danger""#), "{out}");
    }

    #[test]
    fn whether_a_rise_is_good_decides_the_colour_and_never_the_arrow() {
        let rise = html(Trend::new(3).up_is_bad());
        assert!(rise.contains(r#"data-direction="up""#) && rise.contains(r#"data-tone="danger""#));
        let fall = html(Trend::new(-3).up_is_bad());
        assert!(
            fall.contains(r#"data-direction="down""#) && fall.contains(r#"data-tone="success""#)
        );
        let plain = html(Trend::new(3).tone(Tone::Neutral));
        assert!(!plain.contains("data-tone"), "{plain}");
        let forced = html(Trend::new(-3).tone(Tone::named("cyan")));
        assert!(forced.contains(r#"data-tone="cyan""#), "{forced}");
    }

    #[test]
    fn a_change_that_rounds_to_nothing_is_no_change() {
        for change in [0.0, 0.04, -0.04] {
            let out = html(Trend::new(change));
            assert!(out.contains(r#"data-direction="flat""#), "{out}");
            assert!(out.contains("No change") && out.contains("0%"), "{out}");
            assert!(
                !out.contains("data-tone") && !out.contains("+0") && !out.contains("-0"),
                "{out}"
            );
        }
        // With more decimals the same change shows.
        assert!(html(Trend::new(0.04).decimals(2)).contains("+0.04%"));
    }

    #[test]
    fn a_missing_change_is_a_dash() {
        for missing in [f64::NAN, f64::INFINITY] {
            let out = html(Trend::new(missing));
            assert!(
                out.contains("–") && !out.contains("data-tone") && !out.contains("svg"),
                "{out}"
            );
        }
        assert!(html(Trend::new(None::<f64>)).contains("–"));
    }

    #[test]
    fn numbers_group_by_the_language_and_units_follow_the_number() {
        let out = html(
            Trend::new(123_456.0)
                .suffix(" steps")
                .decimals(0)
                .locale("en-IN"),
        );
        assert!(out.contains("+1,23,456 steps"), "{out}");
        let out = html(Trend::new(-1.5).locale("de"));
        assert!(out.contains("-1,5%"), "{out}");
        let out = html(Trend::new(2).words("Gestiegen", "Gesunken", "Gleich"));
        assert!(out.contains("Gestiegen"), "{out}");
    }

    #[test]
    fn a_trend_links_the_linear_sheet_and_no_script() {
        let node: Node = Trend::new(1).into();
        let Node::Element(el) = &node else {
            panic!("an element")
        };
        assert!(el.uses.has(Uses::CHART_LINEAR));
        assert!(!el.uses.meets(Uses::CHARTS));
    }
}
