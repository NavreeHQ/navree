//! [`Timeline`]: events down a rail, grouped by day, each with a time, a coloured
//! dot, a title and the facts under it.

use std::borrow::Cow;

use super::clock::{Wall, offset_label};
use super::tone::{DisplayLabels, Meta, Tone, Zone, hidden_notice, meta_list};
use super::{Empty, Skeleton, SkeletonShape, icon};
use crate::icons::Icon;
use crate::node::{Element, IntoNodes, Node, el, into_node};

/// Where an event stands on the rail.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum TimelineState {
    /// It happened: a filled dot.
    #[default]
    Done,
    /// It is happening now: a ring, and the event is the current step.
    Current,
    /// It has not happened: an empty ring.
    Upcoming,
    /// It happened but is set aside, such as work a machine did: a dashed ring and
    /// quieter text.
    Muted,
}

impl TimelineState {
    const fn value(self) -> Option<&'static str> {
        match self {
            Self::Done => None,
            Self::Current => Some("current"),
            Self::Upcoming => Some("upcoming"),
            Self::Muted => Some("muted"),
        }
    }
}

/// One thing that happened.
///
/// ```
/// use navree_ui::{Meta, Tone, TimelineEvent, icons};
///
/// let event = TimelineEvent::new("Opened the email")
///     .at(1_793_592_667)
///     .tone(Tone::Info)
///     .detail("Second open")
///     .meta(Meta::icon(icons::MONITOR, "Safari on macOS"));
/// # let _ = event;
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct TimelineEvent {
    title: Node,
    detail: Option<Node>,
    at: Option<i64>,
    time_text: Option<Cow<'static, str>>,
    tone: Tone,
    glyph: Option<Icon>,
    state: TimelineState,
    gap: Gap,
    meta: Vec<Meta>,
    filtered: bool,
    id: Option<String>,
}

#[derive(Clone, Debug, Default)]
enum Gap {
    /// Worked out from the times.
    #[default]
    Computed,
    /// Written by the app.
    Given(String),
    /// None.
    Off,
}

impl TimelineEvent {
    /// An event called `title`.
    pub fn new(title: impl Into<Node>) -> Self {
        Self {
            title: title.into(),
            detail: None,
            at: None,
            time_text: None,
            tone: Tone::Neutral,
            glyph: None,
            state: TimelineState::Done,
            gap: Gap::Computed,
            meta: Vec::new(),
            filtered: false,
            id: None,
        }
    }

    /// Says when it happened, in unix seconds. Events with a time are grouped by
    /// day and get a gap chip.
    pub fn at(mut self, unix_seconds: i64) -> Self {
        self.at = Some(unix_seconds);
        self
    }

    /// Prints this in the time column instead of the clock time: "just now", or the
    /// only time there is for an event with no moment.
    pub fn time_text(mut self, text: impl Into<Cow<'static, str>>) -> Self {
        self.time_text = Some(text.into());
        self
    }

    /// Adds a line under the title.
    pub fn detail(mut self, detail: impl Into<Node>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// Colours the dot.
    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }

    /// Puts an icon in a larger round indicator instead of a dot.
    pub fn icon(mut self, glyph: Icon) -> Self {
        self.glyph = Some(glyph);
        self
    }

    /// Sets where it stands.
    pub fn state(mut self, state: TimelineState) -> Self {
        self.state = state;
        self
    }

    /// The step happening now.
    pub fn current(self) -> Self {
        self.state(TimelineState::Current)
    }

    /// A step that has not happened.
    pub fn upcoming(self) -> Self {
        self.state(TimelineState::Upcoming)
    }

    /// An event set aside, such as work a machine did.
    pub fn muted(self) -> Self {
        self.state(TimelineState::Muted)
    }

    /// Writes the gap chip by hand (`+2 min after the reminder`) instead of working
    /// it out.
    pub fn gap(mut self, text: impl Into<String>) -> Self {
        self.gap = Gap::Given(text.into());
        self
    }

    /// Leaves the gap chip out.
    pub fn no_gap(mut self) -> Self {
        self.gap = Gap::Off;
        self
    }

    /// Adds a fact under it: a device, a place.
    pub fn meta(mut self, meta: Meta) -> Self {
        self.meta.push(meta);
        self
    }

    /// Adds several facts under it.
    pub fn metas(mut self, metas: impl IntoIterator<Item = Meta>) -> Self {
        self.meta.extend(metas);
        self
    }

    /// Leaves it out until the reader ticks the notice that says some events were
    /// hidden. It is still in the page, so this needs no request and no script.
    pub fn filtered(mut self) -> Self {
        self.filtered = true;
        self
    }

    /// Sets its `id`.
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }
}

#[derive(Clone, Debug)]
enum Entry {
    Event(Box<TimelineEvent>),
    Day(Node, Vec<TimelineEvent>),
    Block(Node),
}

/// Events on a rail.
///
/// Give each event a time in unix seconds and the timeline groups them by day in
/// its time zone, with a heading like "Monday, November 2, 2026", and works out
/// how long each event came after the one before. Give it no times and write them
/// with [`time_text`](TimelineEvent::time_text) and [`day`](Self::day) instead.
/// Events keep the order you give them.
///
/// ```
/// use navree_ui::{Timeline, TimelineEvent, Tone, render};
///
/// let timeline = Timeline::new("steps", "What happened")
///     .time_zone("Asia/Kolkata", 19_800)
///     .event(TimelineEvent::new("Sent").at(1_793_592_667).tone(Tone::Success))
///     .event(TimelineEvent::new("Opened").at(1_793_592_727).tone(Tone::Info));
/// let html = render(&timeline.into());
/// assert!(html.contains("Monday, November 2, 2026"));
/// assert!(html.contains("+1 min"));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Timeline {
    el: Element,
    id: String,
    label: Node,
    zone: Option<Zone>,
    entries: Vec<Entry>,
    notes: Vec<Node>,
    newest_first: bool,
    heading: u8,
    labels: DisplayLabels,
    loading: bool,
    empty: Option<Empty>,
}

impl Timeline {
    /// A timeline with this `id` (the notice and the day headings build on it),
    /// named `label` for assistive technology.
    pub fn new(id: impl Into<String>, label: impl Into<Node>) -> Self {
        let id = id.into();
        Self {
            el: el("div").slot("timeline").id(id.clone()),
            id,
            label: label.into(),
            zone: None,
            entries: Vec::new(),
            notes: Vec::new(),
            newest_first: false,
            heading: 3,
            labels: DisplayLabels::default(),
            loading: false,
            empty: None,
        }
    }

    /// Sets the zone the times are shown in, by its name and its offset from UTC in
    /// seconds, and writes "Times in Asia/Kolkata (GMT+05:30)" above the events.
    pub fn time_zone(mut self, name: impl Into<String>, offset_seconds: i32) -> Self {
        self.zone = Some(Zone {
            name: name.into(),
            offset: offset_seconds,
        });
        self
    }

    /// Adds an event.
    pub fn event(mut self, event: TimelineEvent) -> Self {
        self.entries.push(Entry::Event(Box::new(event)));
        self
    }

    /// Adds events.
    pub fn events(mut self, events: impl IntoIterator<Item = TimelineEvent>) -> Self {
        self.entries.extend(
            events
                .into_iter()
                .map(|event| Entry::Event(Box::new(event))),
        );
        self
    }

    /// Adds events under a heading you write, for times that are not unix seconds.
    pub fn day(
        mut self,
        heading: impl Into<Node>,
        events: impl IntoIterator<Item = TimelineEvent>,
    ) -> Self {
        self.entries
            .push(Entry::Day(heading.into(), events.into_iter().collect()));
        self
    }

    /// Adds a full-width item with no dot: a callout, a progress bar.
    pub fn block(mut self, content: impl IntoNodes) -> Self {
        self.entries.push(Entry::Block(Node::list(content)));
        self
    }

    /// Adds a dashed note after the events: "Nothing else happened."
    pub fn note(mut self, note: impl Into<Node>) -> Self {
        self.notes.push(note.into());
        self
    }

    /// Says the events run from the newest to the oldest, so a gap is measured to
    /// the event below, not the one above.
    pub fn newest_first(mut self) -> Self {
        self.newest_first = true;
        self
    }

    /// Sets the level of the day headings. The default is 3.
    pub fn heading(mut self, level: u8) -> Self {
        self.heading = level.clamp(1, 6);
        self
    }

    /// Replaces the words.
    pub fn labels(mut self, labels: DisplayLabels) -> Self {
        self.labels = labels;
        self
    }

    /// Shows that events are on their way.
    pub fn loading(mut self) -> Self {
        self.loading = true;
        self
    }

    /// Sets what shows when there are no events.
    pub fn empty(mut self, empty: Empty) -> Self {
        self.empty = Some(empty);
        self
    }

    /// The gap of each event, in the order they were given: seconds to the event
    /// before it in time, for events that have a time.
    fn gaps(&self, flat: &[&TimelineEvent]) -> Vec<Option<u64>> {
        let times: Vec<Option<i64>> = flat.iter().map(|event| event.at).collect();
        (0..flat.len())
            .map(|at| {
                let now = times[at]?;
                let before = if self.newest_first {
                    times[at + 1..].iter().flatten().next()
                } else {
                    times[..at].iter().rev().flatten().next()
                }?;
                Some(now.abs_diff(*before))
            })
            .collect()
    }

    fn into_node(self) -> Node {
        let zone = self.zone.clone();
        let offset = zone.as_ref().map_or(0, |zone| zone.offset);
        let flat: Vec<&TimelineEvent> = self
            .entries
            .iter()
            .flat_map(|entry| match entry {
                Entry::Event(event) => vec![&**event],
                Entry::Day(_, events) => events.iter().collect(),
                Entry::Block(_) => Vec::new(),
            })
            .collect();
        let hidden = flat.iter().filter(|event| event.filtered).count();
        let gaps = self.gaps(&flat);
        let mut next = 0;
        let mut sections: Vec<Node> = Vec::new();
        let mut open: Option<Open> = None;
        let mut days = 0;
        let finish = |open: &mut Option<Open>, sections: &mut Vec<Node>| {
            if let Some(group) = open.take() {
                sections.push(group.finish(&self.id, self.heading));
            }
        };
        for entry in &self.entries {
            match entry {
                Entry::Event(event) => {
                    let wall = event.at.map(|at| Wall::at(at, offset));
                    // A new day closes the section; an event with no time stays in it.
                    if let (Some(group), Some(wall)) = (open.as_ref(), wall)
                        && group.key.is_some_and(|key| key != wall.day_key())
                    {
                        finish(&mut open, &mut sections);
                    }
                    if open.is_none() {
                        days += 1;
                        open = Some(Open::new(
                            days,
                            wall.map(|wall| (wall.day_key(), wall.day_label())),
                        ));
                    }
                    if let (Some(group), Some(wall)) = (open.as_mut(), wall)
                        && group.key.is_none()
                    {
                        group.key = Some(wall.day_key());
                        group.label = Some(Node::text(wall.day_label()));
                    }
                    let item = event_item(event, wall, gaps[next], &self.labels);
                    next += 1;
                    if let Some(group) = open.as_mut() {
                        group.items.push(item);
                    }
                }
                Entry::Day(heading, events) => {
                    finish(&mut open, &mut sections);
                    days += 1;
                    let mut group = Open::new(days, None);
                    group.label = Some(heading.clone());
                    for event in events {
                        let wall = event.at.map(|at| Wall::at(at, offset));
                        group
                            .items
                            .push(event_item(event, wall, gaps[next], &self.labels));
                        next += 1;
                    }
                    sections.push(group.finish(&self.id, self.heading));
                }
                Entry::Block(content) => {
                    if open.is_none() {
                        days += 1;
                        open = Some(Open::new(days, None));
                    }
                    if let Some(group) = open.as_mut() {
                        group.items.push(
                            el("li")
                                .slot("timeline-block")
                                .child(content.clone())
                                .into(),
                        );
                    }
                }
            }
        }
        finish(&mut open, &mut sections);

        let note = zone.map(|zone| {
            el("p")
                .slot("timeline-zone")
                .child(self.labels.times_in.replace(
                    "{zone}",
                    &format!("{} ({})", zone.name, offset_label(zone.offset)),
                ))
        });
        let body: Vec<Node> = if sections.is_empty() {
            self.empty_node().into_iter().collect()
        } else {
            sections
        };
        let loading = self.loading.then(|| {
            el("div")
                .slot("timeline-loading")
                .attr("role", "status")
                .children((
                    Skeleton::new(SkeletonShape::Line),
                    Skeleton::new(SkeletonShape::Short),
                    el("span")
                        .attr("data-ui-sr", "")
                        .child(self.labels.loading.to_string()),
                ))
        });
        self.el
            .attr("role", "group")
            .attr("aria-label", self.label.text_content())
            .attr_opt("aria-busy", self.loading.then_some("true"))
            .children((
                note,
                hidden_notice(&self.id, hidden, &self.labels),
                body,
                loading,
                self.notes
                    .into_iter()
                    .map(|note| el("p").slot("timeline-note").child(note))
                    .collect::<Vec<_>>(),
            ))
            .into()
    }

    fn empty_node(&self) -> Option<Node> {
        if self.loading {
            return None;
        }
        Some(
            self.empty
                .clone()
                .unwrap_or_else(|| {
                    Empty::new(self.labels.timeline_empty.to_string()).icon(crate::icons::CLOCK)
                })
                .into(),
        )
    }
}

/// A day's section while it is being filled.
struct Open {
    number: usize,
    key: Option<(i64, u8, u8)>,
    label: Option<Node>,
    items: Vec<Node>,
}

impl Open {
    fn new(number: usize, day: Option<((i64, u8, u8), String)>) -> Self {
        Self {
            number,
            key: day.as_ref().map(|(key, _)| *key),
            label: day.map(|(_, label)| Node::text(label)),
            items: Vec::new(),
        }
    }

    fn finish(self, id: &str, level: u8) -> Node {
        let heading_id = format!("{id}-day-{}", self.number);
        let heading = self.label.map(|label| {
            let tag = match level {
                1 => "h1",
                2 => "h2",
                3 => "h3",
                4 => "h4",
                5 => "h5",
                _ => "h6",
            };
            el(tag)
                .slot("timeline-day-title")
                .id(heading_id.clone())
                .child(label)
        });
        // A group, not a landmark: a page with two timelines on the same day would
        // otherwise hold two regions of one name.
        let section = el("div").slot("timeline-day").attr("role", "group");
        let section = if heading.is_some() {
            section.attr("aria-labelledby", heading_id)
        } else {
            section
        };
        section
            .children((heading, el("ol").slot("timeline-list").child(self.items)))
            .into()
    }
}

/// One `<li>`.
fn event_item(
    event: &TimelineEvent,
    wall: Option<Wall>,
    gap: Option<u64>,
    words: &DisplayLabels,
) -> Node {
    let time: Option<Node> = match (&event.time_text, wall) {
        (Some(text), Some(wall)) => Some(
            el("time")
                .slot("timeline-time")
                .attr("datetime", wall.iso())
                .attr("dir", "ltr")
                .child(text.to_string())
                .into(),
        ),
        (Some(text), None) => Some(
            el("span")
                .slot("timeline-time")
                .attr("dir", "ltr")
                .child(text.to_string())
                .into(),
        ),
        (None, Some(wall)) => Some(
            el("time")
                .slot("timeline-time")
                .attr("datetime", wall.iso())
                .attr("dir", "ltr")
                .child(wall.clock())
                .into(),
        ),
        (None, None) => None,
    };
    let chip = match &event.gap {
        Gap::Off => None,
        Gap::Given(text) => Some(text.clone()),
        Gap::Computed => gap.map(|seconds| words.gap(seconds)),
    }
    .map(|text| {
        let title = words
            .gap_after
            .replace("{gap}", text.trim_start_matches('+'));
        el("span")
            .slot("timeline-gap")
            .attr("title", title)
            .child(text)
    });
    let dot = el("span")
        .slot("timeline-dot")
        .attr("aria-hidden", "true")
        .flag("data-icon", event.glyph.is_some())
        .child(event.glyph.map(icon));
    let upcoming = (event.state == TimelineState::Upcoming).then(|| {
        el("span")
            .attr("data-ui-sr", "")
            .child(format!("{}: ", words.upcoming))
    });
    let item: Element = el("li")
        .slot("timeline-event")
        .attr_opt("id", event.id.clone())
        .attr_opt("data-tone", event.tone.value())
        .attr_opt("data-state", event.state.value())
        .attr_opt(
            "aria-current",
            (event.state == TimelineState::Current).then_some("step"),
        )
        .flag("data-filtered", event.filtered)
        .flag("data-untimed", time.is_none());
    item.children((
        time,
        dot,
        el("div").slot("timeline-body").children((
            el("div").slot("timeline-head").children((
                el("span")
                    .slot("timeline-title")
                    .children((upcoming, el("bdi").child(event.title.clone()))),
                chip,
            )),
            event.detail.clone().map(|detail| {
                el("p")
                    .slot("timeline-detail")
                    .child(el("bdi").child(detail))
            }),
            meta_list(event.meta.clone()),
        )),
    ))
    .into()
}

into_node!(Timeline);
