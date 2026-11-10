//! [`ActivityFeed`] and [`ActivityItem`]: what people did lately, newest first,
//! one row each, with a live dot and a notice for rows left out.

use std::borrow::Cow;

use super::clock::{Wall, offset_label};
use super::tone::{DisplayLabels, Meta, Tone, Zone, hidden_notice, meta_list};
use super::{
    Alert, AlertTone, Badge, BadgeVariant, Button, Empty, NativeSelect, Skeleton, SkeletonShape,
    icon,
};
use crate::html::Href;
use crate::icons::{self, Icon};
use crate::node::{Element, Node, Uses, el, into_node};
use crate::table::Region;

/// One row of a feed: who did what.
///
/// ```
/// use navree_ui::{ActivityItem, Meta, Tone, icons};
///
/// let row = ActivityItem::new("Manpreet Gill", "replied to")
///     .object("Ticket 2041")
///     .tone(Tone::Info)
///     .icon(icons::MESSAGE_SQUARE)
///     .at(1_793_592_667)
///     .meta(Meta::icon(icons::CLOCK, "First reply in 12 min"));
/// # let _ = row;
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct ActivityItem {
    actor: Node,
    verb: Node,
    object: Option<Node>,
    tag: Option<Node>,
    tone: Tone,
    glyph: Icon,
    at: Option<i64>,
    time_text: Option<Cow<'static, str>>,
    meta: Vec<Meta>,
    href: Option<Href>,
    muted: bool,
    filtered: bool,
    id: Option<String>,
}

impl ActivityItem {
    /// `actor` did `verb`: "Manpreet Gill" "replied to".
    pub fn new(actor: impl Into<Node>, verb: impl Into<Node>) -> Self {
        Self {
            actor: actor.into(),
            verb: verb.into(),
            object: None,
            tag: None,
            tone: Tone::Neutral,
            glyph: icons::ACTIVITY,
            at: None,
            time_text: None,
            meta: Vec::new(),
            href: None,
            muted: false,
            filtered: false,
            id: None,
        }
    }

    /// What it was done to, after the verb.
    pub fn object(mut self, object: impl Into<Node>) -> Self {
        self.object = Some(object.into());
        self
    }

    /// Adds a small tag at the end of the line.
    pub fn tag(mut self, tag: impl Into<Node>) -> Self {
        self.tag = Some(tag.into());
        self
    }

    /// Colours the icon and the verb.
    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }

    /// Sets the icon in the round marker.
    pub fn icon(mut self, glyph: Icon) -> Self {
        self.glyph = glyph;
        self
    }

    /// Says when, in unix seconds. The feed prints the clock time in its zone.
    pub fn at(mut self, unix_seconds: i64) -> Self {
        self.at = Some(unix_seconds);
        self
    }

    /// Prints this instead of the clock time: "2 min ago".
    pub fn time_text(mut self, text: impl Into<Cow<'static, str>>) -> Self {
        self.time_text = Some(text.into());
        self
    }

    /// Adds a fact on the meta line: a browser, a place, an email.
    pub fn meta(mut self, meta: Meta) -> Self {
        self.meta.push(meta);
        self
    }

    /// Adds several facts.
    pub fn metas(mut self, metas: impl IntoIterator<Item = Meta>) -> Self {
        self.meta.extend(metas);
        self
    }

    /// Makes the actor a link and the whole row a click target. Point it at a page
    /// or at a page that opens a [`Sheet`](crate::Sheet).
    pub fn href(mut self, href: impl Into<Href>) -> Self {
        self.href = Some(href.into());
        self
    }

    /// Quieter, for work a machine did.
    pub fn muted(mut self) -> Self {
        self.muted = true;
        self
    }

    /// Leaves it out until the reader ticks the notice that says some rows were
    /// hidden. It is still in the page.
    pub fn filtered(mut self) -> Self {
        self.filtered = true;
        self
    }

    /// Sets its `id`. A row whose id is already on the page is not added again
    /// when a live feed brings it a second time.
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    fn into_row(self, zone: &Zone) -> Node {
        let wall = self.at.map(|at| Wall::at(at, zone.offset));
        let linked = self.href.is_some();
        let time: Option<Element> = match (self.time_text, wall) {
            (Some(text), Some(wall)) => Some(
                el("time")
                    .slot("activity-time")
                    .attr("datetime", wall.iso())
                    .child(text.to_string()),
            ),
            (Some(text), None) => Some(el("span").slot("activity-time").child(text.to_string())),
            (None, Some(wall)) => Some(
                el("time")
                    .slot("activity-time")
                    .attr("datetime", wall.iso())
                    .child(wall.clock()),
            ),
            (None, None) => None,
        };
        let actor = match self.href {
            Some(href) => el("a")
                .slot("activity-actor")
                .href(href)
                .child(el("bdi").child(self.actor)),
            None => el("span")
                .slot("activity-actor")
                .child(el("bdi").child(self.actor)),
        };
        el("li")
            .slot("activity-item")
            .attr_opt("id", self.id)
            .attr_opt("data-tone", self.tone.value())
            .attr_opt("data-state", self.muted.then_some("muted"))
            .flag("data-filtered", self.filtered)
            .flag("data-link", linked)
            .children((
                el("span")
                    .slot("activity-icon")
                    .attr("aria-hidden", "true")
                    .child(icon(self.glyph)),
                el("div").slot("activity-body").children((
                    el("p").slot("activity-line").children((
                        actor,
                        " ",
                        el("span").slot("activity-verb").child(self.verb),
                        self.object.map(|object| {
                            (
                                " ",
                                el("span")
                                    .slot("activity-object")
                                    .child(el("bdi").child(object)),
                            )
                        }),
                        self.tag.map(|tag| {
                            el("span")
                                .slot("activity-tag")
                                .child(Badge::new(tag).variant(BadgeVariant::Outline))
                        }),
                    )),
                    meta_list(self.meta),
                )),
                time,
            ))
            .into()
    }
}

/// How tall a feed grows before it scrolls inside itself.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum FeedHeight {
    /// As tall as its rows.
    #[default]
    Auto,
    /// About 16 rem.
    Sm,
    /// About 27 rem.
    Md,
    /// About 40 rem.
    Lg,
}

impl FeedHeight {
    const fn value(self) -> Option<&'static str> {
        match self {
            Self::Auto => None,
            Self::Sm => Some("sm"),
            Self::Md => Some("md"),
            Self::Lg => Some("lg"),
        }
    }
}

#[derive(Clone, Debug, Default)]
enum Status {
    #[default]
    Ready,
    Loading,
    Failed(String, Option<Href>),
}

/// What people did lately, newest first.
///
/// A header holds the title, an optional live dot, a summary line and a slot for
/// any control, such as a period select. Rows left out by a filter are marked
/// [`filtered`](ActivityItem::filtered) and a notice offers them back. The feed
/// is plain markup: put it in a [`Region`] with [`in_region`](Self::in_region) and
/// it takes new rows as a server-sent event stream announces them.
///
/// ```
/// use navree_ui::{ActivityFeed, ActivityItem, Tone, render};
///
/// let feed = ActivityFeed::new("activity", "Recent activity")
///     .item(ActivityItem::new("Manpreet Gill", "replied to").tone(Tone::Info));
/// assert!(render(&feed.into()).contains("Manpreet Gill"));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct ActivityFeed {
    el: Element,
    id: String,
    title: Node,
    live: bool,
    summary: Option<Node>,
    period: Option<Node>,
    items: Vec<ActivityItem>,
    zone: Zone,
    zone_note: bool,
    height: FeedHeight,
    heading: u8,
    labels: DisplayLabels,
    status: Status,
    empty: Option<Empty>,
}

impl ActivityFeed {
    /// A feed with this `id`, called `title`. The id builds the ids of the list,
    /// the notice and the empty message.
    pub fn new(id: impl Into<String>, title: impl Into<Node>) -> Self {
        let id = id.into();
        Self {
            // A group, not a landmark region: a page may hold several feeds.
            el: el("div")
                .slot("activity-feed")
                .attr("role", "group")
                .id(id.clone()),
            id,
            title: title.into(),
            live: false,
            summary: None,
            period: None,
            items: Vec::new(),
            zone: Zone {
                name: "UTC".to_owned(),
                offset: 0,
            },
            zone_note: false,
            height: FeedHeight::Auto,
            heading: 3,
            labels: DisplayLabels::default(),
            status: Status::Ready,
            empty: None,
        }
    }

    /// Shows the live dot. It says "Connecting" in grey until a stream is open, then
    /// "Live" in green; "Reconnecting" in amber while it comes back; "Paused" while
    /// the page is hidden; and "Not live" when the stream refused for good. Without
    /// the runtime it is not shown.
    pub fn live(mut self) -> Self {
        self.live = true;
        self
    }

    /// Writes one line under the title: "10:30 to 11:30 · 14 events".
    pub fn summary(mut self, summary: impl Into<Node>) -> Self {
        self.summary = Some(summary.into());
        self
    }

    /// Puts any control at the end of the header: a period select, a switch.
    pub fn period(mut self, control: impl Into<Node>) -> Self {
        self.period = Some(control.into());
        self
    }

    /// Puts a period select at the end of the header. Choosing an option asks
    /// `action` for the page again with the choice in the query (`?window=15`) and
    /// morphs the region `region_id` in place; with JavaScript off, an Apply button
    /// does the same as a plain form. Give the region's source the same choice, so
    /// a live feed keeps to the period. `select` needs a name and an `aria-label`.
    pub fn period_select(
        mut self,
        action: impl Into<Href>,
        region_id: &str,
        select: NativeSelect,
    ) -> Self {
        let apply = Button::outline(self.labels.apply.to_string())
            .size(super::Size::Sm)
            .submit();
        self.period = Some(
            el("form")
                .attr("method", "get")
                .attr("action", action.into())
                .attr("autocomplete", "off")
                .attr("data-ui-target", region_id.to_owned())
                .uses(Uses::CORE)
                .children((select, el("noscript").child(apply)))
                .into(),
        );
        self
    }

    /// Adds a row.
    pub fn item(mut self, item: ActivityItem) -> Self {
        self.items.push(item);
        self
    }

    /// Adds rows.
    pub fn items(mut self, items: impl IntoIterator<Item = ActivityItem>) -> Self {
        self.items.extend(items);
        self
    }

    /// Sets the zone the clock times are shown in, by name and offset from UTC in
    /// seconds, and writes "Times in Asia/Kolkata (GMT+05:30)" under the title.
    pub fn time_zone(mut self, name: impl Into<String>, offset_seconds: i32) -> Self {
        self.zone = Zone {
            name: name.into(),
            offset: offset_seconds,
        };
        self.zone_note = true;
        self
    }

    /// Sets how tall the list grows before it scrolls.
    pub fn height(mut self, height: FeedHeight) -> Self {
        self.height = height;
        self
    }

    /// Sets the level of the title. The default is 3.
    pub fn heading(mut self, level: u8) -> Self {
        self.heading = level.clamp(1, 6);
        self
    }

    /// Replaces the words.
    pub fn labels(mut self, labels: DisplayLabels) -> Self {
        self.labels = labels;
        self
    }

    /// Shows placeholder rows: the first load is on its way.
    pub fn loading(mut self) -> Self {
        self.status = Status::Loading;
        self
    }

    /// Says the rows couldn't be loaded, keeping the header so the reader can
    /// still change the period.
    pub fn failed(mut self, message: impl Into<String>) -> Self {
        self.status = Status::Failed(message.into(), None);
        self
    }

    /// Adds a "Try again" link to a failed feed.
    pub fn retry(mut self, href: impl Into<Href>) -> Self {
        if let Status::Failed(message, _) = std::mem::take(&mut self.status) {
            self.status = Status::Failed(message, Some(href.into()));
        }
        self
    }

    /// Sets what shows when there are no rows.
    pub fn empty(mut self, empty: Empty) -> Self {
        self.empty = Some(empty);
        self
    }

    /// Just the rows, with no header and no list: what the source of a live feed
    /// answers with, to be added at the top of the feed's list. `offset_seconds` is
    /// the feed's own time zone offset, so a clock time reads the same in a row
    /// that arrives later.
    ///
    /// ```
    /// use navree_ui::{ActivityFeed, ActivityItem, render};
    ///
    /// let rows = ActivityFeed::rows([ActivityItem::new("Manpreet Gill", "replied to").id("event-9")], 19_800);
    /// assert!(render(&rows).starts_with("<li"));
    /// ```
    pub fn rows(items: impl IntoIterator<Item = ActivityItem>, offset_seconds: i32) -> Node {
        let zone = Zone {
            name: String::new(),
            offset: offset_seconds,
        };
        Node::list(
            items
                .into_iter()
                .map(|item| item.into_row(&zone))
                .collect::<Vec<_>>(),
        )
    }

    /// The id of the list that rows are added to: a [`Region`]'s feed target.
    pub fn list_id(&self) -> String {
        format!("{}-list", self.id)
    }

    /// The id of the empty message, which a feed removes when the first row comes.
    pub fn empty_id(&self) -> String {
        format!("{}-empty", self.id)
    }

    /// Wraps the feed in a [`Region`] that fetches new rows from `src` (with
    /// `after=<cursor>`) whenever `stream` speaks, and adds them at the top:
    /// a stream's address, or names on the app's live hub
    /// ([`LiveStream::on`](crate::LiveStream::on)). `cursor` is the newest row's
    /// cursor. The feed shows its live dot.
    ///
    /// ```
    /// use navree_ui::{ActivityFeed, render};
    ///
    /// let region = ActivityFeed::new("activity", "Recent activity")
    ///     .in_region("activity-region", "/fragments/activity", "/events", "0");
    /// let html = render(&region.into());
    /// assert!(html.contains(r#"data-ui-live-src="/events""#));
    /// ```
    pub fn in_region(
        self,
        region_id: &str,
        src: impl Into<Href>,
        stream: impl Into<super::Live>,
        cursor: impl Into<String>,
    ) -> Region {
        let list = self.list_id();
        let empty = self.empty_id();
        let cursor: String = cursor.into();
        let one = self.labels.new_one.to_string();
        let many = self.labels.new_many.to_string();
        Region::new(region_id.to_owned())
            .src(src)
            .live_on(stream)
            .feed(list, cursor, Some(&empty))
            .announce_new(many, one)
            .children(self.live())
    }

    fn into_node(self) -> Node {
        let hidden = self.items.iter().filter(|item| item.filtered).count();
        let title_id = format!("{}-title", self.id);
        let list_id = self.list_id();
        let tag = match self.heading {
            1 => "h1",
            2 => "h2",
            3 => "h3",
            4 => "h4",
            5 => "h5",
            _ => "h6",
        };
        let live = self.live.then(|| {
            el("span")
                .slot("live-state")
                .attr("role", "status")
                .children((
                    el("span").slot("live-dot").attr("aria-hidden", "true"),
                    el("span")
                        .attr("data-for", "connecting")
                        .child(self.labels.connecting.to_string()),
                    el("span")
                        .attr("data-for", "on")
                        .child(self.labels.live.to_string()),
                    el("span")
                        .attr("data-for", "retry")
                        .child(self.labels.reconnecting.to_string()),
                    el("span")
                        .attr("data-for", "paused")
                        .child(self.labels.paused.to_string()),
                    el("span")
                        .attr("data-for", "closed")
                        .child(self.labels.closed.to_string()),
                ))
        });
        let note = self.zone_note.then(|| {
            el("p")
                .slot("activity-zone")
                .child(self.labels.times_in.replace(
                    "{zone}",
                    &format!("{} ({})", self.zone.name, offset_label(self.zone.offset)),
                ))
        });
        let header = el("header").slot("activity-header").children((
            el("div").slot("activity-heading").children((
                el(tag)
                    .slot("activity-title")
                    .id(title_id.clone())
                    .child(self.title),
                live,
            )),
            self.summary
                .map(|summary| el("p").slot("activity-summary").child(summary)),
            note,
            self.period
                .map(|control| el("div").slot("activity-period").child(control)),
        ));
        let busy = matches!(self.status, Status::Loading);
        let rows: Vec<Node> = self
            .items
            .into_iter()
            .map(|item| item.into_row(&self.zone))
            .collect();
        let none = rows.is_empty();
        let below: Option<Node> = match self.status {
            Status::Loading => Some(
                el("div")
                    .slot("activity-loading")
                    .attr("role", "status")
                    .children((
                        loading_row(),
                        loading_row(),
                        loading_row(),
                        el("span")
                            .attr("data-ui-sr", "")
                            .child(self.labels.loading.to_string()),
                    ))
                    .into(),
            ),
            Status::Failed(message, retry) => Some(
                el("div")
                    .slot("activity-failed")
                    .child(
                        Alert::new(AlertTone::Danger)
                            .title(if message.is_empty() {
                                self.labels.failed.to_string()
                            } else {
                                message
                            })
                            .body(retry.map(|href| {
                                Button::outline(self.labels.retry.to_string())
                                    .size(super::Size::Sm)
                                    .href(href)
                            })),
                    )
                    .into(),
            ),
            Status::Ready if none => Some(
                el("div")
                    .slot("activity-empty")
                    .id(format!("{}-empty", self.id))
                    .child(self.empty.unwrap_or_else(|| {
                        Empty::new(self.labels.feed_empty.to_string()).icon(icons::ACTIVITY)
                    }))
                    .into(),
            ),
            Status::Ready => None,
        };
        self.el
            .attr("aria-labelledby", title_id.clone())
            .attr_opt("aria-busy", busy.then_some("true"))
            .children((
                header,
                hidden_notice(&self.id, hidden, &self.labels, self.live),
                el("ol")
                    .slot("activity-list")
                    .id(list_id)
                    .attr_opt("data-height", self.height.value())
                    // A list that scrolls inside itself is reached by keyboard, and named.
                    .attr_opt("tabindex", self.height.value().map(|_| "0"))
                    .attr_opt(
                        "aria-labelledby",
                        self.height.value().map(|_| title_id.clone()),
                    )
                    .child(rows),
                below,
            ))
            .into()
    }
}

/// A placeholder row: a circle and two lines.
fn loading_row() -> Element {
    el("div").slot("activity-loading-row").children((
        el("span").slot("activity-icon").attr("aria-hidden", "true"),
        el("div").slot("activity-body").children((
            Skeleton::new(SkeletonShape::Line),
            Skeleton::new(SkeletonShape::Short),
        )),
    ))
}

into_node!(ActivityFeed);
