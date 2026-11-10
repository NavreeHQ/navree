//! What the display components share: tones, meta items, a time zone and the
//! words they print.

use std::borrow::Cow;

use super::icon;
use crate::cmd::Cmd;
use crate::icons::{self, Icon};
use crate::node::{Node, el};

/// What a dot, an icon or a badge means by its colour.
///
/// The colour never carries the meaning alone: write what it means in the text
/// beside it. A tone is a `data-tone` attribute and the stylesheet gives it a
/// colour, so no component writes a `style` attribute.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Tone {
    /// No colour: the default.
    #[default]
    Neutral,
    /// Something to know.
    Info,
    /// Good news.
    Success,
    /// Needs attention.
    Warning,
    /// Bad news.
    Danger,
    /// A colour the stylesheet names: `violet`, `cyan` and `rose` ship, and an
    /// app adds its own as `[data-tone=brand]{--tone:#7c5cff}`. A name that is not
    /// lowercase letters, digits and hyphens, at most 32 long, is neutral.
    Named(&'static str),
}

impl Tone {
    /// The colour called `name` in the stylesheet.
    pub const fn named(name: &'static str) -> Self {
        Self::Named(name)
    }

    /// The `data-tone` value, or `None` for no colour.
    pub(crate) fn value(self) -> Option<&'static str> {
        match self {
            Self::Neutral => None,
            Self::Info => Some("info"),
            Self::Success => Some("success"),
            Self::Warning => Some("warning"),
            Self::Danger => Some("danger"),
            Self::Named(name) => valid_name(name).then_some(name),
        }
    }
}

/// Whether `name` is safe as a tone: `[a-z][a-z0-9-]{0,31}`.
fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    name.len() <= 32
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// One small fact under a title: an icon or a flag and some text, such as a
/// device or a place.
///
/// The start slot takes any node, so a flag from any set fits beside the text.
/// It is hidden from assistive technology: what it shows is in the text.
///
/// ```
/// use navree_ui::{Meta, icons};
///
/// let device = Meta::icon(icons::MONITOR, "Safari on macOS");
/// let place = Meta::flag("🇮🇳", "Mumbai");
/// # let _ = (device, place);
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Meta {
    start: Option<Node>,
    text: Node,
    ltr: bool,
}

impl Meta {
    /// Some text.
    pub fn new(text: impl Into<Node>) -> Self {
        Self {
            start: None,
            text: text.into(),
            ltr: false,
        }
    }

    /// An icon, then some text.
    pub fn icon(glyph: Icon, text: impl Into<Node>) -> Self {
        Self::new(text).start(icon(glyph))
    }

    /// A flag (or any small picture), then some text.
    pub fn flag(flag: impl Into<Node>, text: impl Into<Node>) -> Self {
        Self::new(text).start(flag)
    }

    /// Puts any node before the text.
    pub fn start(mut self, start: impl Into<Node>) -> Self {
        self.start = Some(start.into());
        self
    }

    /// Reads the text left to right whatever the page's direction: an email
    /// address, an IP address.
    pub fn ltr(mut self) -> Self {
        self.ltr = true;
        self
    }

    /// Facts in a row that wraps: under a card's title, beside a record's name.
    /// Nothing at all when there are none.
    ///
    /// ```
    /// use navree_ui::{Meta, icons, render};
    ///
    /// let facts = Meta::list([Meta::icon(icons::LIST_CHECKS, "2/3"), Meta::new("Due today")]);
    /// assert!(render(&facts).contains(r#"<ul data-slot="meta">"#));
    /// ```
    pub fn list(items: impl IntoIterator<Item = Meta>) -> Node {
        meta_list(items.into_iter().collect()).unwrap_or(Node::Empty)
    }

    pub(crate) fn into_item(self) -> Node {
        el("li")
            .slot("meta-item")
            .children((
                self.start.map(|start| {
                    el("span")
                        .slot("meta-start")
                        .attr("aria-hidden", "true")
                        .child(start)
                }),
                el("span")
                    .slot("meta-text")
                    .attr_opt("dir", self.ltr.then_some("ltr"))
                    .child(el("bdi").child(self.text)),
            ))
            .into()
    }
}

/// A list of meta items, or nothing when there are none.
pub(crate) fn meta_list(items: Vec<Meta>) -> Option<Node> {
    if items.is_empty() {
        return None;
    }
    Some(
        el("ul")
            .slot("meta")
            .children(items.into_iter().map(Meta::into_item).collect::<Vec<_>>())
            .into(),
    )
}

/// The notice over a list that has rows left out: "3 items were hidden. Show them".
/// Ticking its box shows the rows with no script, and a dismiss button (shown when
/// the runtime is there) removes the notice. It is in the page even with nothing
/// left out, hidden, when `always` says the list is live: a live feed that brings a
/// filtered row can then say so, as the runtime writes the count into it from the
/// rows marked `filtered`. A list that isn't live has no notice when it has no rows
/// left out, so a page of them still ships no script.
pub(crate) fn hidden_notice(
    id: &str,
    count: usize,
    words: &DisplayLabels,
    always: bool,
) -> Option<Node> {
    if count == 0 && !always {
        return None;
    }
    let notice = format!("{id}-hidden");
    let text = format!("{id}-hidden-text");
    Some(
        el("div")
            .slot("hidden-notice")
            .id(notice.clone())
            .flag("hidden", count == 0)
            .children((
                el("p")
                    .slot("hidden-notice-text")
                    .id(text.clone())
                    .attr("data-ui-one", words.hidden_one.to_string())
                    .attr("data-ui-many", words.hidden_many.to_string())
                    .child(words.hidden(count)),
                el("label").slot("hidden-notice-toggle").children((
                    el("input")
                        .slot("reveal")
                        .attr("type", "checkbox")
                        .attr("aria-describedby", text),
                    el("span")
                        .attr("data-state", "off")
                        .child(words.show_hidden.to_string()),
                    el("span")
                        .attr("data-state", "on")
                        .child(words.hide_hidden.to_string()),
                )),
                el("button")
                    .slot("hidden-notice-dismiss")
                    .attr("type", "button")
                    .attr("aria-label", words.dismiss.to_string())
                    .on_click(vec![
                        Cmd::set_attr(notice.clone(), "data-dismissed", ""),
                        Cmd::hide(notice),
                    ])
                    .child(icon(icons::X)),
            ))
            .into(),
    )
}

/// A time zone: a name for the note and a fixed offset for the clock.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Zone {
    pub(crate) name: String,
    pub(crate) offset: i32,
}

/// The words the timeline, the feed and the outcome chips print. English by
/// default; pass your own for another language. `{n}`, `{zone}` and `{gap}` are
/// filled in.
#[derive(Clone, Debug)]
#[allow(missing_docs)]
pub struct DisplayLabels {
    pub hidden_one: Cow<'static, str>,
    pub hidden_many: Cow<'static, str>,
    pub show_hidden: Cow<'static, str>,
    pub hide_hidden: Cow<'static, str>,
    pub dismiss: Cow<'static, str>,
    pub connecting: Cow<'static, str>,
    pub live: Cow<'static, str>,
    pub reconnecting: Cow<'static, str>,
    pub paused: Cow<'static, str>,
    pub closed: Cow<'static, str>,
    pub times_in: Cow<'static, str>,
    pub timeline_empty: Cow<'static, str>,
    pub feed_empty: Cow<'static, str>,
    pub failed: Cow<'static, str>,
    pub retry: Cow<'static, str>,
    pub apply: Cow<'static, str>,
    pub loading: Cow<'static, str>,
    pub upcoming: Cow<'static, str>,
    pub gap_under_a_second: Cow<'static, str>,
    pub gap_after: Cow<'static, str>,
    pub second: Cow<'static, str>,
    pub minute: Cow<'static, str>,
    pub hour: Cow<'static, str>,
    pub day: Cow<'static, str>,
    pub outcome_yes: Cow<'static, str>,
    pub outcome_no: Cow<'static, str>,
    pub new_one: Cow<'static, str>,
    pub new_many: Cow<'static, str>,
}

impl Default for DisplayLabels {
    fn default() -> Self {
        Self {
            hidden_one: "1 item was hidden.".into(),
            hidden_many: "{n} items were hidden.".into(),
            show_hidden: "Show them".into(),
            hide_hidden: "Hide them".into(),
            dismiss: "Dismiss".into(),
            connecting: "Connecting".into(),
            live: "Live".into(),
            reconnecting: "Reconnecting".into(),
            paused: "Paused".into(),
            closed: "Not live".into(),
            times_in: "Times in {zone}".into(),
            timeline_empty: "No events yet".into(),
            feed_empty: "Nothing yet".into(),
            failed: "This couldn't load.".into(),
            retry: "Try again".into(),
            apply: "Apply".into(),
            loading: "Loading".into(),
            upcoming: "Upcoming".into(),
            gap_under_a_second: "under 1 s".into(),
            gap_after: "{gap} after the previous step".into(),
            second: "s".into(),
            minute: "min".into(),
            hour: "h".into(),
            day: "d".into(),
            outcome_yes: "yes".into(),
            outcome_no: "no".into(),
            new_one: "1 new item".into(),
            new_many: "{n} new items".into(),
        }
    }
}

impl DisplayLabels {
    /// The hidden-items sentence for `count`.
    pub(crate) fn hidden(&self, count: usize) -> String {
        if count == 1 {
            self.hidden_one.to_string()
        } else {
            self.hidden_many.replace("{n}", &count.to_string())
        }
    }

    /// `+1 min`, `+2 h 5 min`, `under 1 s`: the time between two events.
    pub(crate) fn gap(&self, seconds: u64) -> String {
        if seconds == 0 {
            return self.gap_under_a_second.to_string();
        }
        let unit = |amount: u64, name: &str| format!("{amount} {name}");
        let two = |big: (u64, &str), small: (u64, &str)| {
            if small.0 == 0 {
                format!("+{}", unit(big.0, big.1))
            } else {
                format!("+{} {}", unit(big.0, big.1), unit(small.0, small.1))
            }
        };
        match seconds {
            1..=59 => format!("+{}", unit(seconds, &self.second)),
            60..=3599 => two((seconds / 60, &self.minute), (seconds % 60, &self.second)),
            3600..=86_399 => two(
                (seconds / 3600, &self.hour),
                (seconds % 3600 / 60, &self.minute),
            ),
            _ => two(
                (seconds / 86_400, &self.day),
                (seconds % 86_400 / 3600, &self.hour),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gaps_read_in_the_two_largest_units() {
        let words = DisplayLabels::default();
        assert_eq!(words.gap(0), "under 1 s");
        assert_eq!(words.gap(1), "+1 s");
        assert_eq!(words.gap(59), "+59 s");
        assert_eq!(words.gap(60), "+1 min");
        assert_eq!(words.gap(90), "+1 min 30 s");
        assert_eq!(words.gap(3599), "+59 min 59 s");
        assert_eq!(words.gap(3600), "+1 h");
        assert_eq!(words.gap(7500), "+2 h 5 min");
        assert_eq!(words.gap(86_400), "+1 d");
        assert_eq!(words.gap(90_000), "+1 d 1 h");
        assert!(words.gap(u64::MAX).starts_with('+'));
    }

    #[test]
    fn a_tone_name_is_checked() {
        assert_eq!(Tone::named("brand").value(), Some("brand"));
        assert_eq!(Tone::named("brand-2").value(), Some("brand-2"));
        for bad in [
            "",
            "Brand",
            "x;y",
            "9lives",
            "a b",
            "a\"b",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ] {
            assert_eq!(Tone::Named(bad).value(), None, "{bad:?}");
        }
        assert_eq!(Tone::Neutral.value(), None);
        assert_eq!(Tone::Danger.value(), Some("danger"));
    }
}
