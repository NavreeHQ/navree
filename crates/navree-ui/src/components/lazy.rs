//! LazyList: a long list that loads as it is read. The page draws one page of items;
//! the `lazy` module fetches the next page when the reader nears either end, by a
//! cursor the server chose, never by an offset. Older items prepended above keep the
//! reader's place. A list too long for the page drops what is far off screen and
//! fetches it again when the reader comes back. With a live stream it merges what
//! changed: new items in their place, changed ones moved, gone ones taken out, never
//! twice. With JavaScript off each end is a link to the page that holds the next part.
//!
//! The server side is three small things: [`LazyQuery`] reads what the runtime asks,
//! [`LazyPage`] answers a page of items, and [`LazyPage::changes`] answers what changed
//! since a revision.
//!
//! **Cursors compare as text.** The runtime places a live item by comparing cursors as
//! strings, so make them sort: pad numbers (`format!("{at:012}{id:010}")`).
//!
//! **Revisions are taken where they commit.** A live list asks for what changed since the
//! newest revision it has seen, so a change must never become visible after a reader has
//! seen a later one. Take the revision inside the transaction that writes it, from
//! something only one writer holds at a time: a counter row the transaction updates
//! (`UPDATE counter SET rev = rev + 1 RETURNING rev`), or on SQLite, which has one writer,
//! a row id the database assigns. A number taken from a counter in memory before the write
//! is not safe: a later number can commit first, and the reader moves past the earlier one.
//!
//! A merge changes an item the list has where it stands, node by node, so the focus, a
//! caret and the scroll stay where the reader left them.

use http::HeaderValue;
use http::header::{CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE};
use http::request::Parts;
use navree_http::extract::{FromRequestParts, urlencoded};
use navree_http::response::{IntoResponse, Response};

use crate::html::Href;
use crate::node::{Element, IntoNodes, Node, Uses, el, into_node};
use crate::render::render_into;
use crate::table::region::valid_event_name;
use crate::view::PAGE_CSP;

/// The longest cursor or revision read or drawn, in bytes.
pub const MAX_CURSOR: usize = 200;

/// Which way the cursors run down the list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum LazyOrder {
    /// The first item has the greatest cursor: newest first, an inbox.
    #[default]
    Descending,
    /// The first item has the least cursor: oldest first, a conversation.
    Ascending,
}

impl LazyOrder {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Descending => "desc",
            Self::Ascending => "asc",
        }
    }
}

/// Which end of the list a live change arrives at, when it is new.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum LazyEnd {
    /// The top: the newest conversation of an inbox.
    #[default]
    Start,
    /// The bottom: the newest message of a conversation.
    End,
}

/// One item of a [`LazyList`]: its key (the record it shows, unique in the list), its
/// cursor (where it sorts), and what it draws.
#[derive(Clone, Debug)]
#[must_use]
pub struct LazyItem {
    el: Element,
}

/// A cursor, key or revision kept to printable ASCII and [`MAX_CURSOR`] bytes.
fn clip(text: &str) -> String {
    text.chars()
        .filter(|ch| ch.is_ascii_graphic())
        .take(MAX_CURSOR)
        .collect()
}

impl LazyItem {
    /// An item for the record `key`, sorting at `cursor`, drawing `content`.
    pub fn new(key: impl AsRef<str>, cursor: impl AsRef<str>, content: impl IntoNodes) -> Self {
        Self {
            el: el("li")
                .attr("data-key", clip(key.as_ref()))
                .attr("data-cursor", clip(cursor.as_ref()))
                .child(content),
        }
    }

    /// Sets an attribute on the item.
    pub fn attr(mut self, name: &'static str, value: impl Into<crate::node::AttrValue>) -> Self {
        self.el = self.el.attr(name, value);
        self
    }

    /// Sets the item's `id`, for links and tests to find it.
    pub fn id(mut self, id: impl Into<crate::node::AttrValue>) -> Self {
        self.el = self.el.id(id);
        self
    }

    /// What a list that [announces new items](LazyList::announce_new) says when this
    /// item arrives on its own: `New message from Harleen Kaur`. Without it the list's
    /// own words are said.
    pub fn announce(mut self, text: impl Into<String>) -> Self {
        self.el = self.el.attr("data-ui-say", text.into());
        self
    }

    /// Arrives without being announced: something the reader did themselves, such as
    /// their own reply.
    pub fn quiet(mut self) -> Self {
        self.el = self.el.flag("data-ui-quiet", true);
        self
    }

    fn into_node(self) -> Node {
        self.el.into()
    }
}

/// What the ends of a list say.
#[derive(Clone, Debug)]
pub struct LazyLabels {
    /// The link at the start: `Load newer`, `Load earlier messages`.
    pub start: String,
    /// The link at the end: `Load more`.
    pub end: String,
    /// What is announced after a page arrives, with `{n}`: `{n} more loaded`.
    pub loaded: String,
    /// What the link says while its page is on its way.
    pub loading: String,
    /// What the link says when its page didn't come.
    pub failed: String,
}

impl Default for LazyLabels {
    fn default() -> Self {
        Self {
            start: "Load newer".to_owned(),
            end: "Load more".to_owned(),
            loaded: "{n} more loaded".to_owned(),
            loading: "Loading…".to_owned(),
            failed: "That didn't load. Try again".to_owned(),
        }
    }
}

/// A list that loads as it is read, in pages, by cursor. See the module's docs.
///
/// ```
/// use navree_ui::{LazyItem, LazyList, render};
///
/// let list = LazyList::new("inbox-list", "Conversations", "/inbox/rows")
///     .items([LazyItem::new("7", "0001793793600", "Card declined")])
///     .more_after("/inbox?before=0001793793600")
///     .max(300);
/// let html = render(&list.into());
/// assert!(html.contains(r#"<li data-key="7" data-cursor="0001793793600">Card declined</li>"#));
/// assert!(html.contains(r#"href="/inbox?before=0001793793600""#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct LazyList {
    id: String,
    label: String,
    src: Href,
    order: LazyOrder,
    items: Vec<Node>,
    before: Option<Href>,
    after: Option<Href>,
    max: u32,
    since: Option<String>,
    /// The stream, its event names, where new items go, and whether it catches up.
    live: Option<(Href, Option<String>, LazyEnd, bool)>,
    empty: Option<Node>,
    labels: LazyLabels,
    gap: u8,
    view: Option<String>,
    current: Option<String>,
    announce: Option<(String, String)>,
    seen: Option<Href>,
}

impl LazyList {
    /// A list named `id` in the page and `label` for assistive technology, whose pages
    /// come from `src`. The runtime asks `src` with `dir=start` or `dir=end` and the
    /// `cursor` of the item at that end, or with `dir=live` and `since`; read it with
    /// [`LazyQuery`].
    pub fn new(id: impl Into<String>, label: impl Into<String>, src: impl Into<Href>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            src: src.into(),
            order: LazyOrder::Descending,
            items: Vec::new(),
            before: None,
            after: None,
            max: 300,
            since: None,
            live: None,
            empty: None,
            labels: LazyLabels::default(),
            gap: 1,
            view: None,
            current: None,
            announce: None,
            seen: None,
        }
    }

    /// Announces items that arrive live, politely and once they stop coming (a second
    /// after the last, five at most after the first): `many` with `{n}` filled in, or for
    /// one item its own words ([`LazyItem::announce`]), else `one`. Items that are only
    /// changed, or [quiet](LazyItem::quiet), are not announced. Without this a live list
    /// changes silently.
    pub fn announce_new(mut self, many: impl Into<String>, one: impl Into<String>) -> Self {
        self.announce = Some((many.into(), one.into()));
        self
    }

    /// Tells the server, with a `POST` to `url`, that the reader has seen what the list
    /// shows: once when the page shows it, and again when new items arrive, only while
    /// the page is in sight, one at a time. For marking a conversation read. With no
    /// script nothing is posted: offer a form for it.
    pub fn seen_by(mut self, url: impl Into<Href>) -> Self {
        self.seen = Some(url.into());
        self
    }

    /// Names what the list shows: its filters, its search. A page drawn again for the
    /// same view keeps the items the reader loaded and where they scrolled, so opening an
    /// item deep in the list doesn't take the reader back to the top; a page for another
    /// view draws its own first page. The source by default.
    pub fn view(mut self, view: impl Into<String>) -> Self {
        self.view = Some(view.into());
        self
    }

    /// The item the page is about: its first link says `aria-current="page"` and is
    /// drawn so, and the mark moves when a page for another item keeps the list.
    pub fn current(mut self, key: impl AsRef<str>) -> Self {
        self.current = Some(clip(key.as_ref()));
        self
    }

    /// Sets the space between items, as [`Stack::gap`](crate::Stack::gap) does: 1 by default.
    pub fn gap(mut self, gap: u8) -> Self {
        self.gap = gap;
        self
    }

    /// Sets which way the cursors run: [`LazyOrder::Descending`] by default.
    pub fn order(mut self, order: LazyOrder) -> Self {
        self.order = order;
        self
    }

    /// Adds the first page's items.
    pub fn items(mut self, items: impl IntoIterator<Item = LazyItem>) -> Self {
        self.items.extend(items.into_iter().map(Node::from));
        self
    }

    /// Says there is more before the first item, and where a page with no script finds
    /// it. Leave it out when the list starts at its start.
    pub fn more_before(mut self, page: impl Into<Href>) -> Self {
        self.before = Some(page.into());
        self
    }

    /// Says there is more after the last item, and where a page with no script finds it.
    pub fn more_after(mut self, page: impl Into<Href>) -> Self {
        self.after = Some(page.into());
        self
    }

    /// Keeps at most `most` items in the page (300 by default, at least 50): past that,
    /// items far off screen at the other end are dropped, and fetched again when the
    /// reader scrolls back.
    pub fn max(mut self, most: u32) -> Self {
        self.max = most.clamp(50, 10_000);
        self
    }

    /// The revision the items are at: a live fetch asks for what changed since.
    pub fn since(mut self, revision: impl AsRef<str>) -> Self {
        self.since = Some(clip(revision.as_ref()));
        self
    }

    /// Merges what changed whenever this server-sent event stream speaks (its event
    /// `event`, or its unnamed ones): `src` with `dir=live&since=`. New items arrive at
    /// `new_at`'s end or in their place, changed ones move, gone ones go. The stream is
    /// a nudge: its payload is never read.
    pub fn live_from(
        mut self,
        url: impl Into<Href>,
        event: Option<&'static str>,
        new_at: LazyEnd,
    ) -> Self {
        let event = event
            .filter(|name| valid_event_name(name))
            .map(str::to_owned);
        self.live = Some((url.into(), event, new_at, false));
        self
    }

    /// Merges what changed whenever its stream says one of its names: the app's
    /// live hub ([`LiveStream::on`](crate::LiveStream::on)) or any stream on this
    /// site. See [`live_from`](Self::live_from).
    pub fn live_on(mut self, live: impl Into<super::Live>, new_at: LazyEnd) -> Self {
        let live = live.into();
        self.live = Some((
            live.src().clone(),
            live.event_attr(),
            new_at,
            live.catches_up(),
        ));
        self
    }

    /// What shows when the list has nothing in it.
    pub fn empty(mut self, empty: impl Into<Node>) -> Self {
        self.empty = Some(empty.into());
        self
    }

    /// Replaces the words at the ends and in announcements.
    pub fn labels(mut self, labels: LazyLabels) -> Self {
        self.labels = labels;
        self
    }

    /// The link at one end: for a page with no script, and the place the runtime watches.
    fn end_link(dir: &'static str, href: Option<Href>, label: &str) -> Element {
        let done = href.is_none();
        el("a")
            .slot("lazy-more")
            .attr("data-dir", dir)
            .attr("href", href.unwrap_or_else(|| Href::from("#")))
            .flag("hidden", done)
            .child(label.to_owned())
    }

    fn into_node(self) -> Node {
        let view = self
            .view
            .clone()
            .unwrap_or_else(|| self.src.as_str().to_owned());
        let items_id = format!(
            "{}-{}",
            self.id,
            crate::assets::content_hash(view.as_bytes())
        );
        let mut root = el("div")
            .slot("lazy-list")
            .attr("id", self.id.clone())
            .flag("data-ui-lazy-list", true)
            .attr("data-ui-src", self.src)
            .attr("data-ui-order", self.order.as_str())
            .attr("data-ui-max", self.max)
            .attr_opt("data-ui-current", self.current)
            .attr_opt("data-ui-seen", self.seen)
            .attr("data-l-loaded", self.labels.loaded.clone())
            .attr("data-l-loading", self.labels.loading.clone())
            .attr("data-l-failed", self.labels.failed.clone())
            .uses(Uses::LAZY);
        if let Some((many, one)) = self.announce {
            root = root.attr("data-l-new", many).attr("data-l-new-one", one);
        }
        if let Some((url, event, new_at, catch_up)) = self.live {
            root = root
                .attr("data-ui-live-src", url)
                .attr_opt("data-ui-live-event", event)
                .flag("data-ui-live-catch-up", catch_up)
                .attr(
                    "data-ui-live-at",
                    if new_at == LazyEnd::End {
                        "end"
                    } else {
                        "start"
                    },
                )
                .uses(Uses::LIVE);
        }
        let empty = self.items.is_empty();
        let (start_done, end_done) = (self.before.is_none(), self.after.is_none());
        root.children((
            Self::end_link("start", self.before, &self.labels.start),
            // The items of one view are the reader's: a page drawn again for that view
            // leaves them as they are, and the module keeps them.
            el("ul")
                .slot("lazy-items")
                .attr("id", items_id)
                .flag("data-ui-keep", true)
                .flag("data-ui-start-done", start_done)
                .flag("data-ui-end-done", end_done)
                .attr_opt("data-ui-since", self.since)
                .attr("data-gap", crate::components::layout::gap_name(self.gap))
                .attr("role", "list")
                .attr("aria-label", self.label)
                .child(self.items),
            empty.then(|| {
                self.empty
                    .map(|node| el("div").slot("lazy-empty").child(node))
            }),
            Self::end_link("end", self.after, &self.labels.end),
            el("p")
                .slot("lazy-status")
                .attr("role", "status")
                .attr("aria-live", "polite"),
        ))
        .into()
    }
}

/// Which part of a list the runtime asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LazyAsk {
    /// The first page: no cursor.
    First,
    /// What comes before the item with this cursor.
    Before(String),
    /// What comes after the item with this cursor.
    After(String),
    /// What changed since this revision.
    Since(String),
}

/// What the runtime asks of a list's source: `?dir=end&cursor=...`, `?dir=start&cursor=...`
/// or `?dir=live&since=...`. It never fails: anything else is [`LazyAsk::First`]. Cursors
/// are clipped to printable ASCII and [`MAX_CURSOR`] bytes; they are the app's own words,
/// so the handler reads them as it wrote them and binds them in its query.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LazyQuery {
    ask: LazyAsk,
    window: Option<(String, String)>,
}

impl LazyQuery {
    /// Reads a query string.
    pub fn parse(query: &str) -> Self {
        let mut dir = None;
        let mut cursor = None;
        let mut since = None;
        let mut from = None;
        let mut to = None;
        for (count, (name, value)) in urlencoded::parse(query.as_bytes()).enumerate() {
            if count > 64 {
                break;
            }
            match name.as_ref() {
                "dir" => dir = Some(value.into_owned()),
                "cursor" => cursor = Some(clip(&value)),
                "since" => since = Some(clip(&value)),
                "from" => from = Some(clip(&value)),
                "to" => to = Some(clip(&value)),
                _ => {}
            }
        }
        let ask = match (dir.as_deref(), cursor, since) {
            (Some("start"), Some(cursor), _) if !cursor.is_empty() => LazyAsk::Before(cursor),
            (Some("end"), Some(cursor), _) if !cursor.is_empty() => LazyAsk::After(cursor),
            (Some("live"), _, Some(since)) if !since.is_empty() => LazyAsk::Since(since),
            _ => LazyAsk::First,
        };
        let window = match (&ask, from, to) {
            (LazyAsk::Since(_), Some(from), Some(to)) if !from.is_empty() && !to.is_empty() => {
                Some((from, to))
            }
            _ => None,
        };
        Self { ask, window }
    }

    /// For a live ask, the cursors of the first and last items the page holds: the
    /// window it has loaded, in which the server may draw again what changed with no
    /// write of its own, such as a clock counting down.
    pub fn window(&self) -> Option<(&str, &str)> {
        self.window
            .as_ref()
            .map(|(from, to)| (from.as_str(), to.as_str()))
    }

    /// What was asked.
    pub fn ask(&self) -> &LazyAsk {
        &self.ask
    }
}

impl FromRequestParts for LazyQuery {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        Ok(Self::parse(parts.uri.query().unwrap_or_default()))
    }
}

/// An answer to a [`LazyQuery`]: items as markup, and whether there is more that way,
/// or, for a live ask, the new revision and the keys that left the list.
///
/// It is `text/html` that is never stored, under the page's content security policy. It
/// is not a page: the runtime reads its items into the list.
#[derive(Clone, Debug, Default)]
#[must_use]
pub struct LazyPage {
    items: Vec<Node>,
    more: bool,
    since: Option<String>,
    gone: Vec<String>,
}

impl LazyPage {
    /// A page of items, in the list's order, and whether more lie beyond them.
    pub fn new(items: impl IntoIterator<Item = LazyItem>, more: bool) -> Self {
        Self {
            items: items.into_iter().map(Node::from).collect(),
            more,
            since: None,
            gone: Vec::new(),
        }
    }

    /// What changed since a live ask's revision: the items as they are now, the
    /// revision they are at, and the keys of items that left the list. An answer that
    /// holds only part of what changed says so with [`more`](Self::more), and the
    /// runtime asks again from the revision it gave.
    pub fn changes(
        items: impl IntoIterator<Item = LazyItem>,
        since: impl AsRef<str>,
        gone: impl IntoIterator<Item = String>,
    ) -> Self {
        Self {
            items: items.into_iter().map(Node::from).collect(),
            more: false,
            since: Some(clip(since.as_ref())),
            gone: gone.into_iter().map(|key| clip(&key)).collect(),
        }
    }

    /// Says whether more lies beyond these items: for a page, more in the list that way;
    /// for [changes](Self::changes), more changes after the revision this answer gives.
    pub fn more(mut self, more: bool) -> Self {
        self.more = more;
        self
    }

    /// The items as markup.
    pub fn to_html(&self) -> String {
        let mut out = String::with_capacity(self.items.len() * 512);
        for item in &self.items {
            render_into(&mut out, item);
        }
        out
    }
}

impl IntoResponse for LazyPage {
    fn into_response(self) -> Response {
        // A native client gets the rows as parts, to add to the list it named.
        #[cfg(feature = "native-preview")]
        if let Some(response) = crate::native::swap::lazy_page(
            &self.items,
            self.more,
            self.since.as_deref(),
            &self.gone,
        ) {
            return response;
        }
        let html = self.to_html();
        let mut response = (
            [
                (
                    CONTENT_TYPE,
                    HeaderValue::from_static("text/html; charset=utf-8"),
                ),
                (CACHE_CONTROL, HeaderValue::from_static("no-store")),
                (CONTENT_SECURITY_POLICY, HeaderValue::from_static(PAGE_CSP)),
            ],
            html,
        )
            .into_response();
        let headers = response.headers_mut();
        headers.insert(
            "x-ui-more",
            HeaderValue::from_static(if self.more { "1" } else { "0" }),
        );
        if let Some(since) = self.since
            && let Ok(value) = HeaderValue::from_str(&since)
        {
            headers.insert("x-ui-since", value);
        }
        if !self.gone.is_empty()
            && let Ok(value) = HeaderValue::from_str(
                &self
                    .gone
                    .iter()
                    .map(|key| gone_key(key))
                    .collect::<Vec<_>>()
                    .join(","),
            )
        {
            headers.insert("x-ui-gone", value);
        }
        response
    }
}

/// A key as `x-ui-gone` carries it: `%` and `,` written as `%25` and `%2C`, so a comma in
/// a key is no separator. The runtime reads each one back with `decodeURIComponent`.
fn gone_key(key: &str) -> String {
    key.replace('%', "%25").replace(',', "%2C")
}

into_node!(LazyList, LazyItem);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render;

    fn list() -> LazyList {
        LazyList::new("l", "Things", "/things/rows")
            .items(
                (1..=3)
                    .map(|n| LazyItem::new(n.to_string(), format!("{n:04}"), format!("Thing {n}"))),
            )
            .more_after("/things?before=0001")
    }

    #[test]
    fn the_first_page_is_drawn_with_a_link_to_the_next() {
        let html = render(&list().into());
        assert!(html.contains(r#"<div data-slot="lazy-list" id="l" data-ui-lazy-list data-ui-src="/things/rows" data-ui-order="desc" data-ui-max="300" data-l-loaded"#), "{html}");
        assert!(
            html.contains(r#"data-ui-keep data-ui-start-done data-gap="1""#),
            "the start is done and the end is not: {html}"
        );
        assert!(
            html.contains(
                r##"<a data-slot="lazy-more" data-dir="start" href="#" hidden>Load newer</a>"##
            ),
            "{html}"
        );
        assert!(html.contains(r#"<a data-slot="lazy-more" data-dir="end" href="/things?before=0001">Load more</a>"#), "{html}");
        assert!(html.contains(r#"role="list" aria-label="Things"><li data-key="1" data-cursor="0001">Thing 1</li>"#), "{html}");
        assert!(
            html.contains(r#"<p data-slot="lazy-status" role="status" aria-live="polite"></p>"#)
        );
        assert!(crate::render::measure(&list().into()).uses.has(Uses::LAZY));
        assert!(!crate::render::measure(&list().into()).uses.has(Uses::LIVE));
    }

    #[test]
    fn a_live_list_loads_the_live_module_too() {
        let node: Node = list()
            .since("42")
            .live_from("/events", Some("rows"), LazyEnd::End)
            .order(LazyOrder::Ascending)
            .into();
        let uses = crate::render::measure(&node).uses;
        assert!(uses.has(Uses::LAZY) && uses.has(Uses::LIVE));
        let html = render(&node);
        assert!(html.contains(r#"data-ui-since="42""#));
        assert!(
            html.contains(
                r#"data-ui-live-src="/events" data-ui-live-event="rows" data-ui-live-at="end""#
            ),
            "{html}"
        );
        assert!(html.contains(r#"data-ui-order="asc""#));
        // A name an event can't have is left out: the list follows unnamed events.
        let html = render(
            &list()
                .live_from("/events", Some("open"), LazyEnd::Start)
                .into(),
        );
        assert!(!html.contains("data-ui-live-event"));
    }

    #[test]
    fn an_empty_list_says_so_and_has_no_links() {
        let html = render(
            &LazyList::new("l", "Things", "/rows")
                .empty(crate::components::Empty::new("Nothing yet"))
                .into(),
        );
        assert!(html.contains("lazy-empty"));
        assert!(html.contains("data-ui-keep data-ui-start-done data-ui-end-done"));
        assert_eq!(html.matches(" hidden>").count(), 2);
    }

    #[test]
    fn cursors_and_keys_are_clipped_and_escaped() {
        let html = render(&LazyItem::new("a\"b<c>", "x y\u{0}z\u{e9}", "t").into());
        assert!(html.contains(r#"data-key="a&quot;b&lt;c&gt;""#), "{html}");
        assert!(html.contains(r#"data-cursor="xyz""#), "{html}");
        let long = "9".repeat(5000);
        let html = render(&LazyItem::new("k", &long, "t").into());
        assert!(html.contains(&format!("data-cursor=\"{}\"", "9".repeat(MAX_CURSOR))));
        assert_eq!(
            LazyQuery::parse(&format!("dir=end&cursor={long}")).ask(),
            &LazyAsk::After("9".repeat(MAX_CURSOR))
        );
    }

    #[test]
    fn a_query_reads_what_was_asked_and_never_fails() {
        assert_eq!(LazyQuery::parse("").ask(), &LazyAsk::First);
        assert_eq!(
            LazyQuery::parse("dir=end&cursor=0042").ask(),
            &LazyAsk::After("0042".into())
        );
        assert_eq!(
            LazyQuery::parse("cursor=0042&dir=start").ask(),
            &LazyAsk::Before("0042".into())
        );
        assert_eq!(
            LazyQuery::parse("dir=live&since=17").ask(),
            &LazyAsk::Since("17".into())
        );
        for odd in [
            "dir=end",
            "dir=end&cursor=",
            "dir=sideways&cursor=1",
            "dir=live",
            "dir=live&cursor=3",
            "%ff%fe=1",
            "dir=start&cursor=%00%01",
        ] {
            assert_eq!(LazyQuery::parse(odd).ask(), &LazyAsk::First, "{odd}");
        }
        let live = LazyQuery::parse("dir=live&since=9&from=0099&to=0042");
        assert_eq!(live.window(), Some(("0099", "0042")));
        assert_eq!(
            LazyQuery::parse("dir=end&cursor=1&from=1&to=2").window(),
            None
        );
        assert_eq!(LazyQuery::parse("dir=live&since=9&from=1").window(), None);
        let many = "a=1&".repeat(500) + "dir=end&cursor=1";
        assert_eq!(
            LazyQuery::parse(&many).ask(),
            &LazyAsk::First,
            "a query past 64 pairs reads no further"
        );
    }

    #[test]
    fn a_page_answers_items_and_says_whether_there_is_more() {
        let response = LazyPage::new([LazyItem::new("1", "1", "One")], true).into_response();
        assert_eq!(response.headers()["x-ui-more"], "1");
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert!(
            response.headers()["content-type"]
                .to_str()
                .unwrap()
                .starts_with("text/html")
        );
        let changes = LazyPage::changes(
            [LazyItem::new("2", "2", "Two")],
            "99",
            ["5".to_owned(), "7\"".to_owned()],
        );
        assert_eq!(
            changes.to_html(),
            r#"<li data-key="2" data-cursor="2">Two</li>"#
        );
        let response = changes.into_response();
        assert_eq!(response.headers()["x-ui-since"], "99");
        assert_eq!(response.headers()["x-ui-gone"], "5,7\"");
        assert_eq!(response.headers()["x-ui-more"], "0");
    }

    #[test]
    fn a_gone_key_with_a_comma_stays_one_key_and_a_long_answer_says_there_is_more() {
        let response = LazyPage::changes(
            [LazyItem::new("1", "1", "One")],
            "7",
            ["a,b".to_owned(), "50%".to_owned(), "9".to_owned()],
        )
        .more(true)
        .into_response();
        assert_eq!(response.headers()["x-ui-gone"], "a%2Cb,50%25,9");
        assert_eq!(response.headers()["x-ui-more"], "1");
        assert_eq!(
            gone_key("%2C"),
            "%252C",
            "a key that looks encoded reads back as itself"
        );
    }

    #[test]
    fn a_list_can_announce_what_arrives_and_say_what_was_seen() {
        let html = render(
            &list()
                .announce_new("{n} new things", "A new thing")
                .seen_by("/things/seen")
                .items([
                    LazyItem::new("8", "0008", "Eight").announce("Eight came"),
                    LazyItem::new("9", "0009", "Nine").quiet(),
                ])
                .into(),
        );
        assert!(html.contains(r#"data-ui-seen="/things/seen""#), "{html}");
        assert!(
            html.contains(r#"data-l-new="{n} new things" data-l-new-one="A new thing""#),
            "{html}"
        );
        assert!(
            html.contains(r#"<li data-key="8" data-cursor="0008" data-ui-say="Eight came">"#),
            "{html}"
        );
        assert!(
            html.contains(r#"<li data-key="9" data-cursor="0009" data-ui-quiet>"#),
            "{html}"
        );
        let quiet = render(&list().into());
        assert!(!quiet.contains("data-l-new") && !quiet.contains("data-ui-seen"));
    }

    #[test]
    fn one_view_keeps_its_items_and_another_draws_its_own() {
        let id = |list: LazyList| {
            let html = render(&list.into());
            let marker = "data-slot=\"lazy-items\" id=\"";
            let at = html.find(marker).unwrap() + marker.len();
            let end = at + html[at..].find('"').unwrap();
            html[at..end].to_owned()
        };
        let mine = id(list().view("mine"));
        assert_eq!(
            mine,
            id(list().view("mine").current("2")),
            "the same view, another item open"
        );
        assert_ne!(mine, id(list().view("all")));
        let html = render(&list().view("mine").current("2").into());
        assert!(html.contains(r#"data-ui-current="2""#));
    }

    #[test]
    fn the_cap_has_a_floor_and_a_ceiling() {
        assert!(render(&list().max(3).into()).contains(r#"data-ui-max="50""#));
        assert!(render(&list().max(u32::MAX).into()).contains(r#"data-ui-max="10000""#));
    }
}
