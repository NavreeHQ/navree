//! The components that show things: no behaviour, no script.

use super::{Size, icon, root_methods};
use crate::html::Href;
use crate::icons::{self, Icon};
use crate::node::{AttrValue, Element, IntoNodes, Node, el, into_node};

/// What an [`Alert`] means.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum AlertTone {
    /// Plain information.
    #[default]
    Neutral,
    /// It worked.
    Success,
    /// Something needs attention.
    Warning,
    /// It failed.
    Danger,
}

/// A message inside the page.
#[derive(Clone, Debug)]
#[must_use]
pub struct Alert {
    el: Element,
    start: Option<Icon>,
    title: Option<Node>,
    body: Vec<Node>,
}

impl Alert {
    /// An alert meaning `tone`.
    pub fn new(tone: AlertTone) -> Self {
        let root = el("div").slot("alert");
        let (root, glyph) = match tone {
            AlertTone::Neutral => (root.attr("role", "status"), icons::INFO),
            AlertTone::Success => (root.attr("role", "status").attr("data-tone", "success"), icons::CIRCLE_CHECK),
            AlertTone::Warning => (root.attr("role", "alert").attr("data-tone", "warning"), icons::TRIANGLE_ALERT),
            AlertTone::Danger => (root.attr("role", "alert").attr("data-tone", "danger"), icons::CIRCLE_ALERT),
        };
        Self { el: root, start: Some(glyph), title: None, body: Vec::new() }
    }

    /// Replaces the icon.
    pub fn icon(mut self, glyph: Icon) -> Self {
        self.start = Some(glyph);
        self
    }

    /// Sets the title.
    pub fn title(mut self, title: impl Into<Node>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Adds to the description.
    pub fn body(mut self, body: impl IntoNodes) -> Self {
        body.push_into(&mut self.body);
        self
    }

    fn into_node(self) -> Node {
        self.el
            .children((
                self.start.map(icon),
                self.title.map(|title| el("div").slot("alert-title").child(title)),
                (!self.body.is_empty()).then(|| el("div").slot("alert-description").child(self.body)),
            ))
            .into()
    }
}

/// A shape for [`AspectRatio`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Ratio {
    /// 1:1.
    Square,
    /// 16:9: the default.
    #[default]
    Video,
    /// 4:3.
    Photo,
    /// 21:9.
    Wide,
}

/// Content held to a shape. The ratios are a closed set because the content
/// security policy allows no `style` attribute.
#[derive(Clone, Debug)]
#[must_use]
pub struct AspectRatio {
    el: Element,
}

impl AspectRatio {
    /// Content in `ratio`.
    pub fn new(ratio: Ratio, content: impl IntoNodes) -> Self {
        let name = match ratio {
            Ratio::Square => "square",
            Ratio::Video => "video",
            Ratio::Photo => "photo",
            Ratio::Wide => "wide",
        };
        Self { el: el("div").slot("aspect-ratio").attr("data-ratio", name).child(content) }
    }

    fn into_node(self) -> Node {
        self.el.into()
    }
}

/// A file shown as a small tile.
#[derive(Clone, Debug)]
#[must_use]
pub struct Attachment {
    el: Element,
    name: Node,
    meta: Option<Node>,
    glyph: Icon,
    href: Option<Href>,
    action: Option<Node>,
}

impl Attachment {
    /// A file called `name`.
    pub fn new(name: impl Into<Node>) -> Self {
        Self { el: el("div").slot("attachment"), name: name.into(), meta: None, glyph: icons::FILE, href: None, action: None }
    }

    /// Adds what's known about it (`2.4 MB`).
    pub fn meta(mut self, meta: impl Into<Node>) -> Self {
        self.meta = Some(meta.into());
        self
    }

    /// Replaces the icon.
    pub fn icon(mut self, glyph: Icon) -> Self {
        self.glyph = glyph;
        self
    }

    /// Links the name to the file.
    pub fn href(mut self, href: impl Into<Href>) -> Self {
        self.href = Some(href.into());
        self
    }

    /// Adds a button: remove, retry.
    pub fn action(mut self, action: impl Into<Node>) -> Self {
        self.action = Some(action.into());
        self
    }

    fn into_node(self) -> Node {
        let name: Node = match self.href {
            Some(href) => el("a").slot("attachment-title").href(href).child(self.name).into(),
            None => el("span").slot("attachment-title").child(self.name).into(),
        };
        self.el
            .children((
                el("span").slot("attachment-media").child(icon(self.glyph)),
                el("div").slot("attachment-content").children((name, self.meta.map(|meta| el("p").slot("attachment-description").child(meta)))),
                self.action,
            ))
            .into()
    }
}

/// A picture of a person: an image, or a neutral icon when there is none.
/// There is no way to put initials in it.
#[derive(Clone, Debug)]
#[must_use]
pub struct Avatar {
    el: Element,
    inner: Node,
    size: Size,
}

impl Avatar {
    /// An image, described by `alt`.
    pub fn image(src: impl Into<Href>, alt: impl Into<String>) -> Self {
        let image = el("img").attr("src", src.into()).attr("alt", alt.into()).attr("loading", "lazy").attr("width", 32_i64).attr("height", 32_i64);
        Self { el: el("span").slot("avatar"), inner: image.into(), size: Size::Md }
    }

    /// No image: the person icon, labelled `label`.
    pub fn placeholder(label: impl Into<String>) -> Self {
        Self { el: el("span").slot("avatar").attr("role", "img").attr("aria-label", label.into()), inner: icon(icons::USER_ROUND), size: Size::Md }
    }

    /// Sets the size.
    pub fn size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }

    fn into_node(self) -> Node {
        self.el.attr_opt("data-size", self.size.as_str()).child(self.inner).into()
    }
}

/// How a [`Badge`] looks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum BadgeVariant {
    /// Filled, neutral: the default.
    #[default]
    Neutral,
    /// A line around it.
    Outline,
    /// Good news.
    Success,
    /// Needs attention.
    Warning,
    /// Bad news.
    Danger,
}

/// A short label beside something.
#[derive(Clone, Debug)]
#[must_use]
pub struct Badge {
    el: Element,
    text: Node,
    start: Option<Icon>,
}

impl Badge {
    /// A badge saying `text`.
    pub fn new(text: impl Into<Node>) -> Self {
        Self { el: el("span").slot("badge"), text: text.into(), start: None }
    }

    /// Sets the look.
    pub fn variant(mut self, variant: BadgeVariant) -> Self {
        self.el = match variant {
            BadgeVariant::Neutral => self.el,
            BadgeVariant::Outline => self.el.attr("data-variant", "outline"),
            BadgeVariant::Success => self.el.attr("data-variant", "success"),
            BadgeVariant::Warning => self.el.attr("data-variant", "warning"),
            BadgeVariant::Danger => self.el.attr("data-variant", "danger"),
        };
        self
    }

    /// Adds an icon.
    pub fn icon(mut self, glyph: Icon) -> Self {
        self.start = Some(glyph);
        self
    }

    fn into_node(self) -> Node {
        self.el.children((self.start.map(icon), self.text)).into()
    }
}

/// Where this page sits: a list of links, ending in the current page.
#[derive(Clone, Debug)]
#[must_use]
pub struct Breadcrumb {
    el: Element,
    label: &'static str,
    items: Vec<Element>,
}

impl Breadcrumb {
    /// An empty trail.
    pub fn new() -> Self {
        Self { el: el("nav").slot("breadcrumb"), label: "Breadcrumb", items: Vec::new() }
    }

    /// Names it, when a page has more than one: landmarks must differ.
    pub fn label(mut self, label: &'static str) -> Self {
        self.label = label;
        self
    }

    /// Adds a link.
    pub fn link(mut self, label: impl Into<Node>, href: impl Into<Href>) -> Self {
        self.items.push(el("li").child(el("a").href(href).child(label.into())));
        self
    }

    /// Adds the current page.
    pub fn current(mut self, label: impl Into<Node>) -> Self {
        self.items.push(el("li").child(el("span").attr("aria-current", "page").child(label.into())));
        self
    }

    fn into_node(self) -> Node {
        self.el.attr("aria-label", self.label).child(el("ol").child(self.items)).into()
    }
}

impl Default for Breadcrumb {
    fn default() -> Self {
        Self::new()
    }
}

/// Which way a chat bubble leans.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum BubbleSide {
    /// Someone else's: the default.
    #[default]
    Start,
    /// Yours.
    End,
}

/// A chat bubble.
#[derive(Clone, Debug)]
#[must_use]
pub struct Bubble {
    el: Element,
}

impl Bubble {
    /// A bubble holding `content`.
    pub fn new(side: BubbleSide, content: impl IntoNodes) -> Self {
        let root = el("div").slot("bubble");
        let root = if side == BubbleSide::End { root.attr("data-align", "end") } else { root };
        Self { el: root.child(el("div").slot("bubble-content").child(content)) }
    }

    fn into_node(self) -> Node {
        self.el.into()
    }
}

/// A surface for related content.
#[derive(Clone, Debug)]
#[must_use]
pub struct Card {
    el: Element,
    level: &'static str,
    title: Option<Node>,
    description: Option<Node>,
    action: Option<Node>,
    body: Vec<Node>,
    footer: Vec<Node>,
}

impl Card {
    /// An empty card.
    pub fn new() -> Self {
        Self { el: el("div").slot("card"), level: "h2", title: None, description: None, action: None, body: Vec::new(), footer: Vec::new() }
    }

    /// Sets the heading level of its title (2 by default, under a page's `h1`).
    pub fn heading(mut self, level: u8) -> Self {
        self.level = match level {
            0..=2 => "h2",
            3 => "h3",
            4 => "h4",
            _ => "h5",
        };
        self
    }

    /// A card with a title.
    pub fn titled(title: impl Into<Node>) -> Self {
        Self::new().title(title)
    }

    /// Sets the title.
    pub fn title(mut self, title: impl Into<Node>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Sets the description.
    pub fn description(mut self, description: impl Into<Node>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Adds something to the header's end.
    pub fn action(mut self, action: impl Into<Node>) -> Self {
        self.action = Some(action.into());
        self
    }

    /// Adds to the body.
    pub fn body(mut self, body: impl IntoNodes) -> Self {
        body.push_into(&mut self.body);
        self
    }

    /// Adds to the footer.
    pub fn footer(mut self, footer: impl IntoNodes) -> Self {
        footer.push_into(&mut self.footer);
        self
    }

    fn into_node(self) -> Node {
        let header = (self.title.is_some() || self.description.is_some() || self.action.is_some()).then(|| {
            el("div").slot("card-header").children((
                self.title.map(|title| el(self.level).slot("card-title").child(title)),
                self.description.map(|text| el("p").slot("card-description").child(text)),
                self.action.map(|action| el("div").slot("card-action").child(action)),
            ))
        });
        self.el
            .children((
                header,
                (!self.body.is_empty()).then(|| el("div").slot("card-content").child(self.body)),
                (!self.footer.is_empty()).then(|| el("div").slot("card-footer").child(self.footer)),
            ))
            .into()
    }
}

impl Default for Card {
    fn default() -> Self {
        Self::new()
    }
}

/// Sets the reading direction of what's inside.
#[derive(Clone, Debug)]
#[must_use]
pub struct Direction {
    el: Element,
}

impl Direction {
    /// Right to left.
    pub fn rtl(content: impl IntoNodes) -> Self {
        Self { el: el("div").slot("direction").attr("dir", "rtl").child(content) }
    }

    /// Left to right.
    pub fn ltr(content: impl IntoNodes) -> Self {
        Self { el: el("div").slot("direction").attr("dir", "ltr").child(content) }
    }

    fn into_node(self) -> Node {
        self.el.into()
    }
}

/// What a list shows when it has nothing in it.
#[derive(Clone, Debug)]
#[must_use]
pub struct Empty {
    el: Element,
    glyph: Option<Icon>,
    title: Node,
    description: Option<Node>,
    actions: Vec<Node>,
}

impl Empty {
    /// A message saying `title`.
    pub fn new(title: impl Into<Node>) -> Self {
        Self { el: el("div").slot("empty"), glyph: None, title: title.into(), description: None, actions: Vec::new() }
    }

    /// Adds an icon.
    pub fn icon(mut self, glyph: Icon) -> Self {
        self.glyph = Some(glyph);
        self
    }

    /// Sets the description.
    pub fn description(mut self, description: impl Into<Node>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Adds what to do next.
    pub fn actions(mut self, actions: impl IntoNodes) -> Self {
        actions.push_into(&mut self.actions);
        self
    }

    fn into_node(self) -> Node {
        self.el
            .children((
                self.glyph.map(|glyph| el("div").slot("empty-media").child(icon(glyph))),
                el("p").slot("empty-title").child(self.title),
                self.description.map(|text| el("p").slot("empty-description").child(text)),
                (!self.actions.is_empty()).then(|| el("div").slot("empty-content").child(self.actions)),
            ))
            .into()
    }
}

/// How an [`Item`] looks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum ItemVariant {
    /// No border: the default.
    #[default]
    Plain,
    /// A line around it.
    Outline,
    /// Filled.
    Muted,
}

/// A row with a title, a description, and things at either end.
#[derive(Clone, Debug)]
#[must_use]
pub struct Item {
    el: Element,
    media: Option<Node>,
    title: Node,
    description: Option<Node>,
    actions: Vec<Node>,
    href: Option<Href>,
}

impl Item {
    /// A row titled `title`.
    pub fn new(title: impl Into<Node>) -> Self {
        Self { el: el("div").slot("item"), media: None, title: title.into(), description: None, actions: Vec::new(), href: None }
    }

    /// Sets the look.
    pub fn variant(mut self, variant: ItemVariant) -> Self {
        self.el = match variant {
            ItemVariant::Plain => self.el,
            ItemVariant::Outline => self.el.attr("data-variant", "outline"),
            ItemVariant::Muted => self.el.attr("data-variant", "muted"),
        };
        self
    }

    /// Adds an icon, or anything else, at the start.
    pub fn media(mut self, media: impl Into<Node>) -> Self {
        self.media = Some(media.into());
        self
    }

    /// Sets the description.
    pub fn description(mut self, description: impl Into<Node>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Adds to the end.
    pub fn actions(mut self, actions: impl IntoNodes) -> Self {
        actions.push_into(&mut self.actions);
        self
    }

    /// Makes the whole row a link.
    pub fn href(mut self, href: impl Into<Href>) -> Self {
        self.href = Some(href.into());
        self
    }

    fn into_node(self) -> Node {
        let mut root = self.el;
        if let Some(href) = self.href {
            root.tag = "a";
            root = root.href(href);
        }
        root.children((
            self.media.map(|media| el("div").slot("item-media").child(media)),
            el("div").slot("item-content").children((
                el("div").slot("item-title").child(self.title),
                self.description.map(|text| el("p").slot("item-description").child(text)),
            )),
            (!self.actions.is_empty()).then(|| el("div").slot("item-actions").child(self.actions)),
        ))
        .into()
    }
}

/// A key on the keyboard.
#[derive(Clone, Debug)]
#[must_use]
pub struct Kbd {
    el: Element,
}

impl Kbd {
    /// The key `key`.
    pub fn new(key: impl Into<Node>) -> Self {
        Self { el: el("kbd").slot("kbd").child(key.into()) }
    }

    fn into_node(self) -> Node {
        self.el.into()
    }
}

/// How a [`Marker`] looks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum MarkerVariant {
    /// Just the text: the default.
    #[default]
    Plain,
    /// Lines on both sides.
    Separator,
    /// A line below.
    Border,
}

/// A small note in a stream: a date between messages, "joined the chat".
#[derive(Clone, Debug)]
#[must_use]
pub struct Marker {
    el: Element,
}

impl Marker {
    /// A marker saying `text`.
    pub fn new(variant: MarkerVariant, text: impl Into<Node>) -> Self {
        let root = el("div").slot("marker");
        let root = match variant {
            MarkerVariant::Plain => root,
            MarkerVariant::Separator => root.attr("data-variant", "separator"),
            MarkerVariant::Border => root.attr("data-variant", "border"),
        };
        Self { el: root.child(el("span").slot("marker-content").child(text.into())) }
    }

    fn into_node(self) -> Node {
        self.el.into()
    }
}

/// A message in a conversation: who, when, and bubbles.
#[derive(Clone, Debug)]
#[must_use]
pub struct Message {
    el: Element,
    avatar: Option<Node>,
    header: Option<Node>,
    content: Vec<Node>,
    footer: Option<Node>,
}

impl Message {
    /// An empty message.
    pub fn new(side: BubbleSide) -> Self {
        let root = el("div").slot("message");
        let root = if side == BubbleSide::End { root.attr("data-align", "end") } else { root };
        Self { el: root, avatar: None, header: None, content: Vec::new(), footer: None }
    }

    /// Sets the avatar.
    pub fn avatar(mut self, avatar: impl Into<Node>) -> Self {
        self.avatar = Some(avatar.into());
        self
    }

    /// Sets what goes above: usually who wrote it.
    pub fn header(mut self, header: impl Into<Node>) -> Self {
        self.header = Some(header.into());
        self
    }

    /// Sets what goes below: usually when.
    pub fn footer(mut self, footer: impl Into<Node>) -> Self {
        self.footer = Some(footer.into());
        self
    }

    /// Adds bubbles and attachments.
    pub fn content(mut self, content: impl IntoNodes) -> Self {
        content.push_into(&mut self.content);
        self
    }

    fn into_node(self) -> Node {
        self.el
            .children((
                self.avatar.map(|avatar| el("div").slot("message-avatar").child(avatar)),
                el("div").slot("message-content").children((
                    self.header.map(|header| el("div").slot("message-header").child(header)),
                    self.content,
                    self.footer.map(|footer| el("div").slot("message-footer").child(footer)),
                )),
            ))
            .into()
    }
}

/// Links between the pages of a list.
#[derive(Clone, Debug)]
#[must_use]
pub struct Pagination {
    el: Element,
}

impl Pagination {
    /// Page `page` of `pages`; `href` gives each page's link.
    pub fn new(page: u32, pages: u32, href: impl Fn(u32) -> String) -> Self {
        let pages = pages.max(1);
        let page = page.clamp(1, pages);
        let mut items = Vec::with_capacity(9);
        let edge = |target: u32, enabled: bool, glyph: Icon, label: &'static str| {
            let link = el("a").slot("pagination-link").attr("aria-label", label).child(icon(glyph));
            let link = if enabled { link.href(href(target)) } else { link.attr("role", "link").attr("aria-disabled", "true") };
            el("li").child(link)
        };
        items.push(edge(page.saturating_sub(1), page > 1, icons::CHEVRON_LEFT, "Previous page"));
        let mut shown = 0;
        for number in 1..=pages {
            let near = number == 1 || number == pages || number.abs_diff(page) <= 1;
            if !near {
                if number > shown + 1 && shown != 0 {
                    continue;
                }
                continue;
            }
            if shown != 0 && number > shown + 1 {
                items.push(el("li").child(el("span").slot("pagination-ellipsis").attr("aria-hidden", "true").child("…")));
            }
            shown = number;
            let link = el("a").slot("pagination-link").href(href(number)).attr("aria-label", format!("Page {number}"));
            let link = if number == page { link.attr("aria-current", "page") } else { link };
            items.push(el("li").child(link.child(i64::from(number))));
        }
        items.push(edge(page + 1, page < pages, icons::CHEVRON_RIGHT, "Next page"));
        Self { el: el("nav").slot("pagination").attr("aria-label", "Pagination").child(el("ul").child(items)) }
    }

    fn into_node(self) -> Node {
        self.el.into()
    }
}

/// How far along something is: the browser's `<progress>`.
#[derive(Clone, Debug)]
#[must_use]
pub struct Progress {
    el: Element,
}

impl Progress {
    /// `value` out of `max`, named `label`.
    pub fn new(value: u32, max: u32, label: impl Into<String>) -> Self {
        Self { el: el("progress").slot("progress").attr("value", value.min(max)).attr("max", max.max(1)).attr("aria-label", label.into()) }
    }

    /// Going, with no known end.
    pub fn indeterminate(label: impl Into<String>) -> Self {
        Self { el: el("progress").slot("progress").attr("aria-label", label.into()) }
    }

    fn into_node(self) -> Node {
        self.el.into()
    }
}

/// A line between things.
#[derive(Clone, Debug)]
#[must_use]
pub struct Separator {
    el: Element,
}

impl Separator {
    /// A horizontal line.
    pub fn horizontal() -> Self {
        Self { el: el("hr").slot("separator") }
    }

    /// A vertical line, in a row.
    pub fn vertical() -> Self {
        Self { el: el("div").slot("separator").attr("role", "separator").attr("aria-orientation", "vertical") }
    }

    fn into_node(self) -> Node {
        self.el.into()
    }
}

/// The shape of a [`Skeleton`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum SkeletonShape {
    /// A line of text: the default.
    #[default]
    Line,
    /// A short line.
    Short,
    /// A block filling its space.
    Block,
}

/// A placeholder while content loads.
#[derive(Clone, Debug)]
#[must_use]
pub struct Skeleton {
    el: Element,
}

impl Skeleton {
    /// A placeholder of `shape`.
    pub fn new(shape: SkeletonShape) -> Self {
        let root = el("div").slot("skeleton").attr("aria-hidden", "true");
        let root = match shape {
            SkeletonShape::Line => root,
            SkeletonShape::Short => root.attr("data-shape", "short"),
            SkeletonShape::Block => root.attr("data-shape", "block"),
        };
        Self { el: root }
    }

    fn into_node(self) -> Node {
        self.el.into()
    }
}

/// A sign that something is happening, named for assistive technology.
#[derive(Clone, Debug)]
#[must_use]
pub struct Spinner {
    el: Element,
}

impl Spinner {
    /// A spinner saying `label` to assistive technology.
    pub fn new(label: impl Into<String>) -> Self {
        Self { el: el("span").slot("spinner").attr("role", "status").attr("aria-label", label.into()).child(icon(icons::LOADER_CIRCLE)) }
    }

    fn into_node(self) -> Node {
        self.el.into()
    }
}

/// A header cell of a [`Table`].
#[derive(Clone, Debug)]
#[must_use]
pub struct TableHead {
    el: Element,
}

impl TableHead {
    /// A column headed `label`.
    pub fn new(label: impl IntoNodes) -> Self {
        Self { el: el("th").slot("table-head").attr("scope", "col").child(label) }
    }

    /// Aligns it right, for numbers.
    pub fn numeric(mut self) -> Self {
        self.el = self.el.flag("data-numeric", true);
        self
    }

    /// Hides it on phones.
    pub fn hide_on_mobile(mut self) -> Self {
        self.el = self.el.flag("data-hide-mobile", true);
        self
    }

    /// Sets how it's sorted now.
    pub fn sort(mut self, ascending: bool) -> Self {
        self.el = self.el.attr("aria-sort", if ascending { "ascending" } else { "descending" });
        self
    }

    fn into_node(self) -> Node {
        self.el.into()
    }
}

/// A cell of a [`Table`].
#[derive(Clone, Debug)]
#[must_use]
pub struct TableCell {
    el: Element,
}

impl TableCell {
    /// A cell holding `content`.
    pub fn new(content: impl IntoNodes) -> Self {
        Self { el: el("td").child(content) }
    }

    /// Aligns it right in tabular figures, for numbers.
    pub fn numeric(mut self) -> Self {
        self.el = self.el.flag("data-numeric", true);
        self
    }

    /// Hides it on phones.
    pub fn hide_on_mobile(mut self) -> Self {
        self.el = self.el.flag("data-hide-mobile", true);
        self
    }

    fn into_node(self) -> Node {
        self.el.into()
    }
}

/// A row of a [`Table`].
#[derive(Clone, Debug)]
#[must_use]
pub struct TableRowEl {
    el: Element,
}

impl TableRowEl {
    /// A row of cells.
    pub fn new(cells: impl IntoNodes) -> Self {
        Self { el: el("tr").child(cells) }
    }

    /// Gives it a stable key, so a morph keeps the row.
    pub fn key(mut self, key: impl Into<AttrValue>) -> Self {
        self.el = self.el.attr("data-key", key);
        self
    }

    /// Marks it selected.
    pub fn selected(mut self, selected: bool) -> Self {
        self.el = self.el.flag("data-selected", selected);
        self
    }

    fn into_node(self) -> Node {
        self.el.into()
    }
}

/// A table of data. The wrapper scrolls on narrow screens.
#[derive(Clone, Debug)]
#[must_use]
pub struct Table {
    el: Element,
    caption: Option<Node>,
    head: Vec<Node>,
    rows: Vec<Node>,
}

impl Table {
    /// An empty table.
    pub fn new() -> Self {
        Self { el: el("table").slot("table"), caption: None, head: Vec::new(), rows: Vec::new() }
    }

    /// Sets the caption.
    pub fn caption(mut self, caption: impl Into<Node>) -> Self {
        self.caption = Some(caption.into());
        self
    }

    /// Adds header cells.
    pub fn head(mut self, head: impl IntoNodes) -> Self {
        head.push_into(&mut self.head);
        self
    }

    /// Adds rows.
    pub fn rows(mut self, rows: impl IntoNodes) -> Self {
        rows.push_into(&mut self.rows);
        self
    }

    fn into_node(self) -> Node {
        let table = self.el.children((
            self.caption.map(|caption| el("caption").child(caption)),
            el("thead").child(el("tr").child(self.head)),
            el("tbody").child(self.rows),
        ));
        el("div").slot("table-container").child(table).into()
    }
}

impl Default for Table {
    fn default() -> Self {
        Self::new()
    }
}

/// A heading. The level is the document's, the look follows it.
#[derive(Clone, Debug)]
#[must_use]
pub struct Heading {
    el: Element,
}

impl Heading {
    /// A page title: `<h1>`.
    pub fn h1(text: impl Into<Node>) -> Self {
        Self { el: el("h1").slot("heading").child(text.into()) }
    }

    /// `<h2>`.
    pub fn h2(text: impl Into<Node>) -> Self {
        Self { el: el("h2").slot("heading").child(text.into()) }
    }

    /// `<h3>`.
    pub fn h3(text: impl Into<Node>) -> Self {
        Self { el: el("h3").slot("heading").child(text.into()) }
    }

    /// `<h4>`.
    pub fn h4(text: impl Into<Node>) -> Self {
        Self { el: el("h4").slot("heading").child(text.into()) }
    }

    fn into_node(self) -> Node {
        self.el.into()
    }
}

/// How a run of [`Text`] looks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum TextVariant {
    /// Body text: the default.
    #[default]
    Body,
    /// Secondary, 13px.
    Muted,
    /// An introduction, 16px.
    Lead,
    /// Mono.
    Code,
}

/// A paragraph, or inline text when [`inline`](Self::inline).
#[derive(Clone, Debug)]
#[must_use]
pub struct Text {
    el: Element,
}

impl Text {
    /// A paragraph.
    pub fn new(text: impl IntoNodes) -> Self {
        Self { el: el("p").slot("text").child(text) }
    }

    /// Secondary text.
    pub fn muted(text: impl IntoNodes) -> Self {
        Self::new(text).variant(TextVariant::Muted)
    }

    /// Inline code.
    pub fn code(text: impl Into<Node>) -> Self {
        Self { el: el("code").slot("code").child(text.into()) }
    }

    /// Sets the look.
    pub fn variant(mut self, variant: TextVariant) -> Self {
        self.el = match variant {
            TextVariant::Body => self.el,
            TextVariant::Muted => self.el.attr("data-variant", "muted"),
            TextVariant::Lead => self.el.attr("data-variant", "lead"),
            TextVariant::Code => self.el.attr("data-variant", "code"),
        };
        self
    }

    /// Writes it inline: `<span>`.
    pub fn inline(mut self) -> Self {
        self.el.tag = "span";
        self
    }

    fn into_node(self) -> Node {
        self.el.into()
    }
}

/// One figure and what it is.
#[derive(Clone, Debug)]
#[must_use]
pub struct Stat {
    el: Element,
    label: Node,
    value: Node,
    trend: Option<(Node, bool)>,
}

impl Stat {
    /// The figure `value`, called `label`.
    pub fn new(label: impl Into<Node>, value: impl Into<Node>) -> Self {
        Self { el: el("div").slot("stat"), label: label.into(), value: value.into(), trend: None }
    }

    /// Adds how it's moving; `good` colours it.
    pub fn trend(mut self, text: impl Into<Node>, good: bool) -> Self {
        self.trend = Some((text.into(), good));
        self
    }

    fn into_node(self) -> Node {
        self.el
            .children((
                el("span").slot("stat-label").child(self.label),
                el("span").slot("stat-value").child(self.value),
                self.trend.map(|(text, good)| el("span").slot("stat-trend").attr("data-tone", if good { "up" } else { "down" }).child(text)),
            ))
            .into()
    }
}

/// A small bar chart, drawn on the server as SVG: no script and no style
/// attribute. The description is what assistive technology gets.
#[derive(Clone, Debug)]
#[must_use]
pub struct Bars {
    el: Element,
}

impl Bars {
    /// `values` as bars, described by `description`.
    pub fn new(description: impl Into<String>, values: &[u32]) -> Self {
        let top = values.iter().copied().max().unwrap_or(0).max(1);
        let bars: Vec<Element> = values
            .iter()
            .enumerate()
            .map(|(at, value)| {
                let height = i64::from(value.saturating_mul(100) / top);
                el("rect").attr("x", i64::try_from(at * 10).unwrap_or(0) + 1).attr("y", 100 - height).attr("width", 8_i64).attr("height", height)
            })
            .collect();
        let width = i64::try_from(values.len() * 10).unwrap_or(0);
        Self {
            el: el("svg")
                .slot("bars")
                .attr("viewBox", format!("0 0 {width} 100"))
                .attr("preserveAspectRatio", "none")
                .attr("role", "img")
                .attr("aria-label", description.into())
                .child(bars),
        }
    }

    fn into_node(self) -> Node {
        self.el.into()
    }
}

root_methods!(Stat, Bars);
into_node!(Stat, Bars);

root_methods!(
    Alert, AspectRatio, Attachment, Avatar, Badge, Breadcrumb, Bubble, Card, Direction, Empty, Item, Kbd, Marker, Message, Pagination,
    Progress, Separator, Skeleton, Spinner, Table, TableHead, TableCell, TableRowEl, Heading, Text,
);
into_node!(
    Alert, AspectRatio, Attachment, Avatar, Badge, Breadcrumb, Bubble, Card, Direction, Empty, Item, Kbd, Marker, Message, Pagination,
    Progress, Separator, Skeleton, Spinner, Table, TableHead, TableCell, TableRowEl, Heading, Text,
);
