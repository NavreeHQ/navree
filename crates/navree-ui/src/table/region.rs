//! [`Region`]: a named part of a page that updates by itself.

use std::time::Duration;

use crate::components::{Skeleton, SkeletonShape};
use crate::html::Href;
use crate::node::{AttrValue, Element, IntoNodes, Node, Uses, el};

/// A named part of a page that updates by itself: fetched after paint, polled,
/// or refetched when an op invalidates it. Its answer morphs in.
///
/// ```
/// use navree_ui::{Region, render};
///
/// let html = render(&Region::new("kpis").src("/fragments/kpis").lazy().into());
/// assert!(html.contains(r#"data-ui-src="/fragments/kpis""#));
/// assert!(html.contains("data-ui-lazy"));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Region {
    el: Element,
    placeholder: bool,
}

impl Region {
    /// A region named `id`.
    pub fn new(id: impl Into<AttrValue>) -> Self {
        Self {
            el: el("div")
                .slot("region")
                .attr("id", id)
                .flag("data-ui-region", true)
                .uses(Uses::CORE),
            placeholder: false,
        }
    }

    /// Sets where its content comes from: a page holding an element with this
    /// id, or a fragment, which is a page with no script of its own. A page with
    /// the runtime that lacks this id, or a redirect, is somewhere else: the user
    /// goes there, as when a session has ended.
    pub fn src(mut self, src: impl Into<Href>) -> Self {
        self.el = self.el.attr("data-ui-src", src.into());
        self
    }

    /// Fetches it after paint, showing a skeleton until then.
    pub fn lazy(mut self) -> Self {
        self.el = self.el.flag("data-ui-lazy", true).attr("aria-busy", "true");
        self.placeholder = true;
        self
    }

    /// Refetches it this often, while the page is visible.
    pub fn poll(mut self, every: Duration) -> Self {
        self.el = self.el.attr(
            "data-ui-poll",
            i64::try_from(every.as_millis()).unwrap_or(i64::MAX),
        );
        self
    }

    /// Makes it a feed: each poll asks the source for what is newer than a cursor
    /// and adds it at the start of the element `into` inside the region, instead
    /// of fetching the whole region again.
    ///
    /// The source is the region's [`src`](Self::src) with `after=<cursor>` added.
    /// It answers the new rows as HTML, nothing when there are none, and the
    /// newest cursor in the `X-UI-Cursor` header. `empty`, if given, names an
    /// element removed once the first row arrives (an "nothing yet" message).
    pub fn feed(
        mut self,
        into: impl Into<AttrValue>,
        after: impl Into<AttrValue>,
        empty: Option<&str>,
    ) -> Self {
        self.el = self
            .el
            .attr("data-ui-feed", into)
            .attr("data-ui-after", after)
            .attr_opt("data-ui-feed-empty", empty.map(str::to_owned));
        self
    }

    /// Trims a feed to its newest `most` rows as new ones arrive, so a page left
    /// open for days doesn't grow without end. The default is 200, which `0` also
    /// means (a feed with no limit is not offered); the most is 5,000. The row the
    /// reader is in is never taken out.
    pub fn feed_limit(mut self, most: u32) -> Self {
        if most > 0 {
            self.el = self.el.attr("data-ui-feed-max", most.min(5000));
        }
        self
    }

    /// Announces new feed rows to assistive technology: `many` with `{n}` filled in,
    /// or `one` when a single row came. Without this a feed adds rows silently.
    pub fn announce_new(mut self, many: impl Into<String>, one: impl Into<String>) -> Self {
        self.el = self
            .el
            .attr("data-ui-feed-say", many.into())
            .attr("data-ui-feed-say-one", one.into());
        self
    }

    /// Refetches the region whenever this server-sent event stream says something,
    /// or, with a feed, asks the source for what is new. Needs [`src`](Self::src).
    ///
    /// An event is a nudge: its payload is never read, so a stream can't put markup,
    /// a URL or a command into the page. One connection serves every region on the
    /// same stream. The connection closes while the page is hidden and opens again,
    /// with one fetch to catch up, when it shows. A connection that drops reconnects
    /// after 1 s, 2 s, 4 s and so on up to 30 s. The stream must be on this site.
    /// While it runs, the region carries `data-live` as `connecting`, `on`, `retry`,
    /// `paused` or `closed`, which a live dot reads. A stream that answers `204`,
    /// `401` or `403` is not tried again (`closed`). A `retry:` line in the stream
    /// is not read: the schedule above is the same in every browser.
    ///
    /// A region has one fetch of its own in flight at a time. Events that come
    /// within 300 ms of the first cost one fetch; events that come while that fetch
    /// is out cost one more after it lands, however many there were.
    ///
    /// ```
    /// use navree_ui::{Region, render};
    ///
    /// let html = render(&Region::new("deals").src("/fragments/deals").live_from("/events").into());
    /// assert!(html.contains(r#"data-ui-live-src="/events""#));
    /// ```
    pub fn live_from(mut self, url: impl Into<Href>) -> Self {
        self.el = self.el.attr("data-ui-live-src", url.into());
        self
    }

    /// Listens for the stream's events called `name` instead of its unnamed ones
    /// (`event: name` on the wire), so one stream can feed several regions. A name
    /// of letters, digits and `_ . : -`, at most 64 long; any other is ignored.
    /// `open` and `error` are the connection's own events, so they are ignored too.
    pub fn live_event(mut self, name: &'static str) -> Self {
        if valid_event_name(name) {
            self.el = self.el.attr("data-ui-live-event", name);
        }
        self
    }

    /// Adds its content.
    pub fn children(mut self, children: impl IntoNodes) -> Self {
        self.el = self.el.child(children);
        self.placeholder = false;
        self
    }

    fn into_node(self) -> Node {
        if self.placeholder {
            return self
                .el
                .children((
                    Skeleton::new(SkeletonShape::Line),
                    Skeleton::new(SkeletonShape::Short),
                ))
                .into();
        }
        self.el.into()
    }
}

/// Whether `name` can be an event name the runtime listens for. `open` and
/// `error` can't: a browser reports the connection opening or failing under those
/// names, and a stream's own events of the same name would be taken for them.
pub(crate) fn valid_event_name(name: &str) -> bool {
    !matches!(name, "open" | "error")
        && (1..=64).contains(&name.len())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '-'))
}

impl From<Region> for Node {
    fn from(region: Region) -> Self {
        region.into_node()
    }
}
