//! [`Region`]: a named part of a page that updates by itself.

use std::time::Duration;

use crate::components::{Button, Live, Size, Skeleton, SkeletonShape};
use crate::html::{Href, LocalHref};
use crate::icons;
use crate::node::{AttrValue, Element, IntoNodes, Node, Uses, el};
use crate::pages::{State, StateKind, StateLabels};

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
    failed: Option<Node>,
    /// Whether `failed` shows only when nothing could be reached at all.
    offline: bool,
    /// Whether the address given to [`src`](Self::src) was refused: the region then has no
    /// source, and nothing that needs one.
    refused: bool,
}

/// What a region's source fetches with: what [`Region::src`] and a lazy part's source
/// write. An address that is not on this site, or `#` (which is what a refused address
/// becomes), fetches nothing the region can draw, so the region gets no source. An address
/// refused here is named in the log.
pub(crate) fn region_source(src: &Href) -> Option<Href> {
    let Ok(local) = LocalHref::parse(src.as_str()) else {
        say_refused(src.as_str());
        return None;
    };
    let local = local.into_href();
    (local.as_str() != "#").then_some(local)
}

/// How many refused sources the log names, each once: a page that builds a source from what
/// a request gave can fill neither the log nor the memory with them.
const REFUSED_SAID: usize = 32;

/// Says in the log, once for each address, that a source was refused. The part is still
/// drawn, only never fetched, which is easy to miss: a whole address of the site's own
/// (`https://example.com/fragments/kpis`) is refused like any other, and is written as a path.
fn say_refused(src: &str) {
    static SAID: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    let src = src
        .char_indices()
        .nth(200)
        .map_or(src, |(at, _)| &src[..at]);
    {
        let Ok(mut said) = SAID.lock() else {
            return;
        };
        if said.len() >= REFUSED_SAID || said.iter().any(|known| known == src) {
            return;
        }
        said.push(src.to_owned());
    }
    tracing::warn!(
        source = src,
        "a region, a lazy tab or a lazy sidebar group was given a source that is not an address on this site (a path, a query or a fragment): it is drawn once and never fetched"
    );
}

/// The attributes a region has only with a source: they are left out with it.
const SOURCED: [&str; 13] = [
    "data-ui-src",
    "data-ui-lazy",
    "aria-busy",
    "data-ui-poll",
    "data-ui-feed",
    "data-ui-after",
    "data-ui-feed-empty",
    "data-ui-feed-max",
    "data-ui-feed-say",
    "data-ui-feed-say-one",
    "data-ui-live-src",
    "data-ui-live-event",
    "data-ui-live-catch-up",
];

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
            failed: None,
            offline: false,
            refused: false,
        }
    }

    /// Sets where its content comes from: a page holding an element with this
    /// id, or a fragment: a [`View`](crate::View) whose body is what goes inside, which
    /// may hold parts that need the runtime (a lazy tab, another region), since the
    /// toolkit marks a view answered to a region's fetch as its fragment. A page with
    /// the runtime that lacks this id and isn't marked so (one a proxy serves), or a
    /// redirect, is somewhere else: the user goes there, as when a session has ended.
    ///
    /// The source is on this site. An address that is not a [`LocalHref`], or that is `#`,
    /// gives the region no source at all: it is drawn once, as it is, and never fetched, so
    /// what goes with a source (fetching after paint, a poll, a feed, a stream) is left out
    /// with it. A region never names another site, nor fetches the page it stands on. A
    /// whole address of the site's own (`https://example.com/fragments/kpis`) is refused like
    /// any other: write the path. A refused address is named in the log as a warning, once.
    ///
    /// ```
    /// use std::time::Duration;
    ///
    /// use navree_ui::{Region, render};
    ///
    /// let html = render(
    ///     &Region::new("kpis")
    ///         .src("https://other.example/kpis")
    ///         .poll(Duration::from_secs(10))
    ///         .into(),
    /// );
    /// assert!(html.contains(r#"id="kpis""#));
    /// assert!(!html.contains("data-ui-src"), "{html}");
    /// assert!(!html.contains("data-ui-poll"), "{html}");
    /// ```
    pub fn src(mut self, src: impl Into<Href>) -> Self {
        match region_source(&src.into()) {
            Some(local) => {
                self.el = self.el.attr("data-ui-src", local);
                self.refused = false;
            }
            None => self.refused = true,
        }
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
        self.el = self
            .el
            .attr("data-ui-live-src", url.into())
            .uses(Uses::LIVE);
        self
    }

    /// Refetches the region, or with a feed asks for what is new, whenever its
    /// stream says one of its names: the app's live hub
    /// ([`LiveStream::on`](crate::LiveStream::on)) or any stream on this site.
    /// Everything [`live_from`](Self::live_from) says holds; use one or the
    /// other. With the hub, a stream that comes back after it dropped is told
    /// only what it missed, so the region fetches only if it missed something.
    ///
    /// ```
    /// use navree_ui::{LiveStream, Region, render};
    ///
    /// let stream = LiveStream::topics(["conversation.42", "clock"]);
    /// let heading = Region::new("heading")
    ///     .src("/conversations/42/heading")
    ///     .live_on(stream.on("conversation.42").event("clock"));
    /// let html = render(&heading.into());
    /// assert!(html.contains(r#"data-ui-live-event="conversation.42 clock""#));
    /// ```
    pub fn live_on(mut self, live: impl Into<Live>) -> Self {
        let live = live.into();
        self.el = self
            .el
            .attr("data-ui-live-src", live.src().clone())
            .attr_opt("data-ui-live-event", live.event_attr())
            .flag("data-ui-live-catch-up", live.catches_up())
            .uses(Uses::LIVE);
        self
    }

    /// Listens for the stream's events called `name` instead of its unnamed ones
    /// (`event: name` on the wire), so one stream can feed several regions. A name
    /// of letters, digits and `_ . : -`, at most 64 long; any other is ignored.
    /// `open` and `error` are the connection's own events, so they are ignored too.
    /// [`live_on`](Self::live_on) takes names made at run time, and several.
    pub fn live_event(mut self, name: &'static str) -> Self {
        if valid_event_name(name) {
            self.el = self.el.attr("data-ui-live-event", name);
        }
        self
    }

    /// Says `state` when a fetch of the region fails in a way the server did not draw: the
    /// connection failed, the request ran out of time, or something between answered in its own
    /// words (a proxy's page, an answer that is not HTML). What the region had stays, and the
    /// state appears beside it. An error from your app is drawn by the server as a state and
    /// shown by the runtime in place of the region (see [`ErrorPage`](crate::ErrorPage)),
    /// whether or not you call this.
    ///
    /// The state shows when the runtime marks the region failed, and goes when the next fetch
    /// starts. It needs no script of its own, and its button can be a `data-ui-refresh` button.
    pub fn when_failed(mut self, state: impl Into<Node>) -> Self {
        self.failed = Some(state.into());
        self
    }

    /// [`when_failed`](Self::when_failed) with "You're offline" and a Try again button that
    /// fetches the region again, shown only when the browser could not reach the server at all
    /// (the connection failed). A request that ran out of time, or a server that answered with
    /// an error, is not being offline: use [`when_failed`](Self::when_failed) with your own words
    /// for those.
    ///
    /// ```
    /// use navree_ui::{Region, render};
    ///
    /// let html = render(&Region::new("kpis").src("/fragments/kpis").lazy().when_offline().into());
    /// assert!(html.contains(r#"data-slot="region-failed""#));
    /// ```
    pub fn when_offline(mut self) -> Self {
        let labels = StateLabels::default();
        let state = State::of(StateKind::Offline).labels(&labels).actions(
            Button::outline(labels.try_again.to_string())
                .size(Size::Sm)
                .icon(icons::REFRESH_CW)
                .attr("data-ui-refresh", ""),
        );
        self = self.when_failed(state);
        self.offline = true;
        self
    }

    /// Adds its content.
    pub fn children(mut self, children: impl IntoNodes) -> Self {
        self.el = self.el.child(children);
        self.placeholder = false;
        self
    }

    fn into_node(mut self) -> Node {
        if self.refused {
            // No source: drawn once, as it is, and never fetched.
            self.el.attrs.retain(|attr| !SOURCED.contains(&attr.name));
            self.placeholder = false;
        }
        let offline = self.offline;
        let failed = self.failed.map(|state| {
            el("div")
                .slot("region-failed")
                .attr("role", "alert")
                .attr_opt("data-when", offline.then_some("offline"))
                .child(state)
        });
        if self.placeholder {
            return self
                .el
                .children((
                    Skeleton::new(SkeletonShape::Line),
                    Skeleton::new(SkeletonShape::Short),
                    failed,
                ))
                .into();
        }
        self.el.child(failed).into()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render;

    #[test]
    fn a_region_whose_source_is_refused_is_drawn_once_and_never_fetched() {
        for src in [
            "https://other.example/kpis",
            "//other.example/kpis",
            "/\\other.example/kpis",
            "javascript:x",
            "#",
        ] {
            // Whichever order the source and what goes with it are given in.
            let before = Region::new("kpis")
                .lazy()
                .poll(Duration::from_secs(10))
                .feed("rows", "0", Some("none"))
                .live_from("/events")
                .src(src);
            let after = Region::new("kpis")
                .src(src)
                .lazy()
                .poll(Duration::from_secs(10))
                .feed_limit(50)
                .live_event("kpis");
            for region in [before, after] {
                let html = render(&region.into());
                assert!(html.contains(r#"id="kpis""#), "{src}: {html}");
                for name in SOURCED {
                    assert!(!html.contains(name), "{src}: {name} in {html}");
                }
                assert!(!html.contains("data-slot=\"skeleton\""), "{src}: {html}");
            }
        }
        // An address on this site is the source, and what goes with it stays.
        let html = render(
            &Region::new("kpis")
                .src("/fragments/kpis")
                .poll(Duration::from_secs(10))
                .into(),
        );
        assert!(html.contains(r#"data-ui-src="/fragments/kpis""#), "{html}");
        assert!(html.contains(r#"data-ui-poll="10000""#), "{html}");
    }
}
