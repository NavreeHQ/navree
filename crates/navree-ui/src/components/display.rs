//! The components that show things: no behaviour, no script.

use super::tone::Tone;
use super::trend::Trend;
use super::{Size, icon, root_methods};
use crate::cmd::Cmd;
use crate::html::Href;
use crate::icons::{self, Icon};
use crate::node::{AttrValue, Element, IntoNodes, Node, Uses, el, into_node};

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
    actions: Vec<Node>,
    close: Option<(String, String)>,
}

impl Alert {
    /// An alert meaning `tone`.
    pub fn new(tone: AlertTone) -> Self {
        let root = el("div").slot("alert");
        let (root, glyph) = match tone {
            AlertTone::Neutral => (root.attr("role", "status"), icons::INFO),
            AlertTone::Success => (
                root.attr("role", "status").attr("data-tone", "success"),
                icons::CIRCLE_CHECK,
            ),
            AlertTone::Warning => (
                root.attr("role", "alert").attr("data-tone", "warning"),
                icons::TRIANGLE_ALERT,
            ),
            AlertTone::Danger => (
                root.attr("role", "alert").attr("data-tone", "danger"),
                icons::CIRCLE_ALERT,
            ),
        };
        Self {
            el: root,
            start: Some(glyph),
            title: None,
            body: Vec::new(),
            actions: Vec::new(),
            close: None,
        }
    }

    /// Replaces the icon.
    pub fn icon(mut self, glyph: Icon) -> Self {
        self.start = Some(glyph);
        self
    }

    /// Adds a close button, named `label` for assistive technology, that hides the alert
    /// in the browser with no request. The alert takes the id `id`, which the button's
    /// command names. With no script the button is not drawn and the alert stays.
    pub fn dismissible(mut self, id: impl Into<String>, label: impl Into<String>) -> Self {
        let id = id.into();
        self.el = self.el.id(id.clone()).uses(Uses::PARTS_STYLES);
        self.close = Some((id, label.into()));
        self
    }

    /// Adds buttons or links under the description: what the reader can do about it.
    pub fn actions(mut self, actions: impl IntoNodes) -> Self {
        actions.push_into(&mut self.actions);
        self.el = self.el.uses(Uses::PARTS_STYLES);
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
        let close = self.close.map(|(id, label)| {
            super::only_with_script(
                super::Button::icon_only(icons::X, label)
                    .variant(super::ButtonVariant::Ghost)
                    .size(Size::Xs)
                    .on_click(Cmd::hide(id)),
            )
        });
        self.el
            .flag("data-closable", close.is_some())
            .children((
                self.start.map(icon),
                self.title
                    .map(|title| el("div").slot("alert-title").child(title)),
                (!self.body.is_empty())
                    .then(|| el("div").slot("alert-description").child(self.body)),
                (!self.actions.is_empty())
                    .then(|| el("div").slot("alert-actions").child(self.actions)),
                close.map(|button| el("div").slot("alert-close").child(button)),
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
        Self {
            el: el("div")
                .slot("aspect-ratio")
                .attr("data-ratio", name)
                .child(content),
        }
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
        Self {
            el: el("div").slot("attachment"),
            name: name.into(),
            meta: None,
            glyph: icons::FILE,
            href: None,
            action: None,
        }
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
            Some(href) => el("a")
                .slot("attachment-title")
                .href(href)
                .child(self.name)
                .into(),
            None => el("span").slot("attachment-title").child(self.name).into(),
        };
        self.el
            .children((
                el("span").slot("attachment-media").child(icon(self.glyph)),
                el("div").slot("attachment-content").children((
                    name,
                    self.meta
                        .map(|meta| el("p").slot("attachment-description").child(meta)),
                )),
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
        let image = el("img")
            .attr("src", src.into())
            .attr("alt", alt.into())
            .attr("loading", "lazy")
            .attr("width", 32_i64)
            .attr("height", 32_i64);
        Self {
            el: el("span").slot("avatar"),
            inner: image.into(),
            size: Size::Md,
        }
    }

    /// No image: the person icon, labelled `label`.
    pub fn placeholder(label: impl Into<String>) -> Self {
        Self {
            el: el("span")
                .slot("avatar")
                .attr("role", "img")
                .attr("aria-label", label.into()),
            inner: icon(icons::USER_ROUND),
            size: Size::Md,
        }
    }

    /// Sets the size.
    pub fn size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }

    /// Shows the person icon when the picture does not load. It needs the parts module,
    /// so a page with one loads a small script; without it a picture that fails shows
    /// the browser's own broken-image mark, as before.
    pub fn fallback(mut self) -> Self {
        self.el = self
            .el
            .flag("data-ui-fallback", true)
            .uses(Uses::PARTS.with(Uses::PARTS_STYLES));
        self
    }

    fn into_node(self) -> Node {
        self.el
            .attr_opt("data-size", self.size.as_str())
            .child(self.inner)
            .into()
    }
}

/// Avatars side by side, each overlapping the one before, with a `+N` tile for the
/// people left out when there are more than `max`.
///
/// ```
/// use navree_ui::{Avatar, AvatarGroup, render};
///
/// let group = AvatarGroup::new("Reviewers").max(2).children((
///     Avatar::placeholder("Gurpreet Kaur"),
///     Avatar::placeholder("Harjot Singh"),
///     Avatar::placeholder("Simran Arora"),
/// ));
/// let html = render(&group.into());
/// assert!(html.contains(r#"role="group""#));
/// assert!(html.contains("+1"));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct AvatarGroup {
    el: Element,
    people: Vec<Node>,
    max: usize,
    total: Option<usize>,
    size: Size,
}

impl AvatarGroup {
    /// A group named `label` (`Reviewers`), which assistive technology reads before
    /// the people.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            el: el("div")
                .slot("avatar-group")
                .attr("role", "group")
                .attr("aria-label", label.into())
                .uses(Uses::PARTS_STYLES),
            people: Vec::new(),
            max: 5,
            total: None,
            size: Size::Md,
        }
    }

    /// Adds avatars. Each should be named, as [`Avatar::placeholder`] and
    /// [`Avatar::image`] require, so the group can be read.
    pub fn children(mut self, avatars: impl IntoNodes) -> Self {
        avatars.push_into(&mut self.people);
        self
    }

    /// Shows at most `max` avatars (1 or more; 5 by default) and `+N` for the rest.
    pub fn max(mut self, max: usize) -> Self {
        self.max = max.max(1);
        self
    }

    /// Says how many people there are in all, when the avatars given are only the first
    /// of a longer list: the tile then counts the rest from `total`.
    pub fn total(mut self, total: usize) -> Self {
        self.total = Some(total);
        self
    }

    /// Sets the size of the `+N` tile; give each avatar the same size.
    pub fn size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }

    fn into_node(self) -> Node {
        let given = self.people.len();
        let total = self.total.map_or(given, |total| total.max(given));
        let shown = self.max.min(given);
        let left = total.saturating_sub(shown);
        let mut people = self.people;
        people.truncate(shown);
        let more = (left > 0).then(|| {
            el("span")
                .slot("avatar")
                .attr("data-count", "")
                .attr_opt("data-size", self.size.as_str())
                .attr("role", "img")
                .attr("aria-label", format!("{left} more"))
                .child(format!("+{left}"))
        });
        self.el.children((people, more)).into()
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
///
/// ```
/// use navree_ui::{Badge, Tone, render};
///
/// let html = render(&Badge::new("Delivered").tone(Tone::Success).dot().into());
/// assert!(html.contains(r#"data-tone="success""#));
/// assert!(html.contains("badge-dot"));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Badge {
    el: Element,
    text: Node,
    start: Option<Icon>,
    end: Option<Icon>,
    dot: bool,
    outcome: Option<(bool, String, String)>,
}

impl Badge {
    /// A badge saying `text`.
    pub fn new(text: impl Into<Node>) -> Self {
        Self {
            el: el("span").slot("badge"),
            text: text.into(),
            start: None,
            end: None,
            dot: false,
            outcome: None,
        }
    }

    /// A chip for something that did or didn't happen: "Opened", "Clicked". When
    /// it did, it carries a check and its tone; when it didn't, a dash and a quiet,
    /// dashed look. A screen reader hears "yes" or "no" after the label (change
    /// the words with [`outcome_words`](Self::outcome_words)).
    ///
    /// ```
    /// use navree_ui::{Badge, Tone, render};
    ///
    /// let paid = render(&Badge::outcome("Paid", true).tone(Tone::Success).into());
    /// let shipped = render(&Badge::outcome("Shipped", false).into());
    /// assert!(paid.contains("yes"));
    /// assert!(shipped.contains("no"));
    /// ```
    pub fn outcome(text: impl Into<Node>, reached: bool) -> Self {
        let mut badge = Self::new(text);
        badge.el = badge
            .el
            .attr("data-variant", "outcome")
            .attr("data-state", if reached { "reached" } else { "missed" });
        badge.start = Some(if reached { icons::CHECK } else { icons::MINUS });
        badge.outcome = Some((reached, "yes".to_owned(), "no".to_owned()));
        badge
    }

    /// Sets the words a screen reader hears after an outcome chip.
    pub fn outcome_words(mut self, yes: impl Into<String>, no: impl Into<String>) -> Self {
        if let Some((reached, ..)) = self.outcome {
            self.outcome = Some((reached, yes.into(), no.into()));
        }
        self
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

    /// Colours it. A [`Tone::Named`] colour comes from the stylesheet.
    pub fn tone(mut self, tone: Tone) -> Self {
        self.el = self.el.attr_opt("data-tone", tone.value());
        self
    }

    /// Adds a small dot before the text, in the badge's colour.
    pub fn dot(mut self) -> Self {
        self.dot = true;
        self
    }

    /// Adds an icon before the text.
    pub fn icon(mut self, glyph: Icon) -> Self {
        self.start = Some(glyph);
        self
    }

    /// Adds an icon after the text.
    pub fn icon_end(mut self, glyph: Icon) -> Self {
        self.end = Some(glyph);
        self
    }

    fn into_node(self) -> Node {
        let outcome = self.outcome.map(|(reached, yes, no)| {
            el("span")
                .attr("data-ui-sr", "")
                .child(if reached { yes } else { no })
        });
        self.el
            .children((
                self.dot
                    .then(|| el("span").slot("badge-dot").attr("aria-hidden", "true")),
                self.start.map(icon),
                self.text,
                outcome,
                self.end.map(icon),
            ))
            .into()
    }
}

/// Where this page sits: a list of links, ending in the current page.
#[derive(Clone, Debug)]
#[must_use]
pub struct Breadcrumb {
    el: Element,
    label: &'static str,
    items: Vec<(Node, Option<Href>)>,
    prefetch: bool,
    max: usize,
    hidden_label: &'static str,
}

impl Breadcrumb {
    /// An empty trail.
    pub fn new() -> Self {
        Self {
            el: el("nav").slot("breadcrumb"),
            label: "Breadcrumb",
            items: Vec::new(),
            prefetch: false,
            max: usize::MAX,
            hidden_label: "Show hidden pages",
        }
    }

    /// Keeps a long trail short: with more than `max` items (3 or more), the first and
    /// the last `max - 2` show, and the ones between fold into an ellipsis that opens a
    /// list of them. It opens and closes natively, so it needs no script.
    pub fn max_items(mut self, max: usize) -> Self {
        self.max = max.max(3);
        self.el = self.el.uses(Uses::PARTS_STYLES);
        self
    }

    /// Names the ellipsis for assistive technology (`Show hidden pages` by default).
    pub fn hidden_label(mut self, label: &'static str) -> Self {
        self.hidden_label = label;
        self
    }

    /// Names it, when a page has more than one: landmarks must differ.
    pub fn label(mut self, label: &'static str) -> Self {
        self.label = label;
        self
    }

    /// Adds a link.
    pub fn link(mut self, label: impl Into<Node>, href: impl Into<Href>) -> Self {
        self.items.push((label.into(), Some(href.into())));
        self
    }

    /// Adds the current page.
    pub fn current(mut self, label: impl Into<Node>) -> Self {
        self.items.push((label.into(), None));
        self
    }

    /// Fetches the trail's links when the pointer rests on one or it takes focus, so
    /// the page is often there before the click. Off unless asked, as for every link.
    pub fn prefetch(mut self) -> Self {
        self.prefetch = true;
        self
    }

    fn into_node(self) -> Node {
        let prefetch = self.prefetch;
        let item = |(label, href): (Node, Option<Href>)| {
            el("li").child(match href {
                Some(href) => el("a")
                    .href(href)
                    .flag("data-ui-prefetch", prefetch)
                    .child(label),
                None => el("span").attr("aria-current", "page").child(label),
            })
        };
        let count = self.items.len();
        let mut items: Vec<Element> = Vec::with_capacity(count.min(self.max) + 1);
        if count > self.max {
            // The first one, an ellipsis holding the ones between, then the last few.
            let tail = self.max - 2;
            let mut rest = self.items.into_iter();
            if let Some(first) = rest.next() {
                items.push(item(first));
            }
            let folded: Vec<Element> = rest.by_ref().take(count - 1 - tail).map(item).collect();
            items.push(
                el("li").slot("breadcrumb-ellipsis").child(
                    el("details").children((
                        el("summary")
                            .attr("aria-label", self.hidden_label)
                            .child(icon(icons::ELLIPSIS)),
                        el("ol").slot("breadcrumb-hidden").child(folded),
                    )),
                ),
            );
            items.extend(rest.map(item));
        } else {
            items.extend(self.items.into_iter().map(item));
        }
        self.el
            .attr("aria-label", self.label)
            .child(el("ol").child(items))
            .into()
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
        let root = if side == BubbleSide::End {
            root.attr("data-align", "end")
        } else {
            root
        };
        Self {
            el: root.child(el("div").slot("bubble-content").child(content)),
        }
    }

    /// Draws it as a note that only the team sees, set apart from what was said in the
    /// conversation: a dashed line and an amber tint, never colour alone, so say it in
    /// the message's header too.
    pub fn note(mut self) -> Self {
        self.el = self.el.attr("data-variant", "note");
        self
    }

    fn into_node(self) -> Node {
        self.el.into()
    }
}

/// How a [`Card`] is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum CardVariant {
    /// A line around a surface: the default.
    #[default]
    Default,
    /// A filled surface with no line: a quiet group.
    Muted,
    /// No line and no fill: only the card's spacing, for content that stands alone.
    Ghost,
    /// A dashed line: a place to add something.
    Dashed,
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
        Self {
            el: el("div").slot("card"),
            level: "h2",
            title: None,
            description: None,
            action: None,
            body: Vec::new(),
            footer: Vec::new(),
        }
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

    /// Sets how the card is drawn. The default needs nothing more; the others link the
    /// parts stylesheet.
    pub fn variant(mut self, variant: CardVariant) -> Self {
        let name = match variant {
            CardVariant::Default => None,
            CardVariant::Muted => Some("muted"),
            CardVariant::Ghost => Some("ghost"),
            CardVariant::Dashed => Some("dashed"),
        };
        if name.is_some() {
            self.el = self.el.uses(Uses::PARTS_STYLES);
        }
        self.el = self.el.attr_opt("data-variant", name);
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
        let header = (self.title.is_some() || self.description.is_some() || self.action.is_some())
            .then(|| {
                el("div").slot("card-header").children((
                    self.title
                        .map(|title| el(self.level).slot("card-title").child(title)),
                    self.description
                        .map(|text| el("p").slot("card-description").child(text)),
                    self.action
                        .map(|action| el("div").slot("card-action").child(action)),
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
        Self {
            el: el("div")
                .slot("direction")
                .attr("dir", "rtl")
                .child(content),
        }
    }

    /// Left to right.
    pub fn ltr(content: impl IntoNodes) -> Self {
        Self {
            el: el("div")
                .slot("direction")
                .attr("dir", "ltr")
                .child(content),
        }
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
        Self {
            el: el("div").slot("empty"),
            glyph: None,
            title: title.into(),
            description: None,
            actions: Vec::new(),
        }
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
                self.glyph
                    .map(|glyph| el("div").slot("empty-media").child(icon(glyph))),
                el("p").slot("empty-title").child(self.title),
                self.description
                    .map(|text| el("p").slot("empty-description").child(text)),
                (!self.actions.is_empty())
                    .then(|| el("div").slot("empty-content").child(self.actions)),
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
    aside: Option<Node>,
    description: Option<Node>,
    footer: Vec<Node>,
    actions: Vec<Node>,
    href: Option<Href>,
    current: bool,
}

impl Item {
    /// A row titled `title`.
    pub fn new(title: impl Into<Node>) -> Self {
        Self {
            el: el("div").slot("item"),
            media: None,
            title: title.into(),
            aside: None,
            description: None,
            footer: Vec::new(),
            actions: Vec::new(),
            href: None,
            current: false,
        }
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

    /// Adds something small at the end of the title's line: a time, a timer.
    pub fn aside(mut self, aside: impl Into<Node>) -> Self {
        self.aside = Some(aside.into());
        self
    }

    /// Adds a line under the description: badges, people.
    pub fn footer(mut self, footer: impl IntoNodes) -> Self {
        footer.push_into(&mut self.footer);
        self
    }

    /// Marks it as the one open now: `aria-current="page"` on a link, and drawn so.
    pub fn current(mut self, current: bool) -> Self {
        self.current = current;
        self
    }

    /// Marks it as new to the reader: its title stronger. Say so in words too, with a
    /// count or a label, since weight alone tells a screen reader nothing.
    pub fn unread(mut self, unread: bool) -> Self {
        self.el = self.el.flag("data-unread", unread);
        self
    }

    /// Keeps the title and the description to one line each, cut with an ellipsis: for a
    /// long list read at a glance. The whole text stays in the page for assistive
    /// technology and find in page.
    pub fn truncate(mut self) -> Self {
        self.el = self.el.flag("data-truncate", true);
        self
    }

    fn into_node(self) -> Node {
        let mut root = self.el;
        if let Some(href) = self.href {
            root.tag = "a";
            root = root.href(href);
            if self.current {
                root = root.attr("aria-current", "page");
            }
        }
        root = root.flag("data-current", self.current);
        let title = el("div").slot("item-title").child(self.title);
        let head: Node = match self.aside {
            Some(aside) => el("div")
                .slot("item-head")
                .children((title, el("span").slot("item-aside").child(aside)))
                .into(),
            None => title.into(),
        };
        root.children((
            self.media
                .map(|media| el("div").slot("item-media").child(media)),
            el("div").slot("item-content").children((
                head,
                self.description
                    .map(|text| el("p").slot("item-description").child(text)),
                (!self.footer.is_empty()).then(|| el("div").slot("item-footer").child(self.footer)),
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
        Self {
            el: el("kbd").slot("kbd").child(key.into()),
        }
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
        Self {
            el: root.child(el("span").slot("marker-content").child(text.into())),
        }
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
        let root = if side == BubbleSide::End {
            root.attr("data-align", "end")
        } else {
            root
        };
        Self {
            el: root,
            avatar: None,
            header: None,
            content: Vec::new(),
            footer: None,
        }
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
                self.avatar
                    .map(|avatar| el("div").slot("message-avatar").child(avatar)),
                el("div").slot("message-content").children((
                    self.header
                        .map(|header| el("div").slot("message-header").child(header)),
                    self.content,
                    self.footer
                        .map(|footer| el("div").slot("message-footer").child(footer)),
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
            let link = el("a")
                .slot("pagination-link")
                .attr("aria-label", label)
                .child(icon(glyph));
            let link = if enabled {
                link.href(href(target))
            } else {
                link.attr("role", "link").attr("aria-disabled", "true")
            };
            el("li").child(link)
        };
        items.push(edge(
            page.saturating_sub(1),
            page > 1,
            icons::CHEVRON_LEFT,
            "Previous page",
        ));
        // Only the pages shown are ever looked at, however many there are.
        let near = [
            Some(1),
            page.checked_sub(1),
            Some(page),
            page.checked_add(1),
            Some(pages),
        ];
        let mut shown = 0;
        for number in near
            .into_iter()
            .flatten()
            .filter(|number| (1..=pages).contains(number))
        {
            if number <= shown {
                continue;
            }
            if shown != 0 && number > shown + 1 {
                items.push(
                    el("li").child(
                        el("span")
                            .slot("pagination-ellipsis")
                            .attr("aria-hidden", "true")
                            .child("…"),
                    ),
                );
            }
            shown = number;
            let link = el("a")
                .slot("pagination-link")
                .href(href(number))
                .attr("aria-label", format!("Page {number}"));
            let link = if number == page {
                link.attr("aria-current", "page")
            } else {
                link
            };
            items.push(el("li").child(link.child(i64::from(number))));
        }
        items.push(edge(
            page.saturating_add(1),
            page < pages,
            icons::CHEVRON_RIGHT,
            "Next page",
        ));
        Self {
            el: el("nav")
                .slot("pagination")
                .attr("aria-label", "Pagination")
                .child(el("ul").child(items)),
        }
    }

    fn into_node(self) -> Node {
        self.el.into()
    }
}

/// How far along something is: the browser's `<progress>`.
///
/// Painted, it is drawn as a meter instead: an SVG bar with a rounded track, its fill
/// in a gradient, a theme colour or the colour of its value's band, a goal marker, a
/// glow, and a fill that grows in. It keeps the progress bar's role and values for
/// assistive technology, and reads the same with no script.
///
/// ```
/// use navree_ui::charts::{ColorScale, Gradient, Ink, Motion};
/// use navree_ui::{Progress, Tone};
///
/// let plain = Progress::new(3, 8, "Profile complete");
/// let sleep = Progress::new(402, 480, "Sleep, 6 h 42 min of 8 h")
///     .fill(Gradient::horizontal().stop(0.0, Ink::slot(7)).stop_with_opacity(1.0, Ink::slot(7), 0.6))
///     .goal(420)
///     .motion(Motion::new().delay(400));
/// let recovery = Progress::new(68, 100, "Recovery 68%").colors(
///     ColorScale::new(Ink::tone(Tone::Danger))
///         .from(50, Ink::tone(Tone::Warning))
///         .from(75, Ink::tone(Tone::Success)),
/// );
/// # let _ = (plain, sleep, recovery);
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Progress {
    el: Element,
    percent: Option<u32>,
    show: bool,
    value: u32,
    max: u32,
    meter: Meter,
}

/// What a painted progress bar draws past the plain one.
#[derive(Clone, Debug, Default)]
struct Meter {
    fill: Option<crate::charts::Gradient>,
    ink: Option<crate::charts::Ink>,
    scale: Option<crate::charts::ColorScale>,
    goal: Option<u32>,
    glow: bool,
    motion: crate::charts::Motion,
    height: Option<u32>,
}

impl Meter {
    fn is_on(&self) -> bool {
        self.fill.is_some()
            || self.ink.is_some()
            || self.scale.is_some()
            || self.goal.is_some()
            || self.glow
            || self.motion.is_on()
            || self.height.is_some()
    }
}

impl Progress {
    /// `value` out of `max`, named `label`.
    pub fn new(value: u32, max: u32, label: impl Into<String>) -> Self {
        let percent = u64::from(value.min(max)) * 100 / u64::from(max.max(1));
        Self {
            el: el("progress")
                .slot("progress")
                .attr("value", value.min(max))
                .attr("max", max.max(1))
                .attr("aria-label", label.into()),
            percent: Some(u32::try_from(percent).unwrap_or(100)),
            show: false,
            value: value.min(max),
            max: max.max(1),
            meter: Meter::default(),
        }
    }

    /// Going, with no known end.
    pub fn indeterminate(label: impl Into<String>) -> Self {
        Self {
            el: el("progress")
                .slot("progress")
                .attr("aria-label", label.into()),
            percent: None,
            show: false,
            value: 0,
            max: 1,
            meter: Meter::default(),
        }
    }

    /// Fills the bar with `gradient`, which spans the filled part. [`Ink::own`] in its
    /// stops is the bar's colour.
    ///
    /// [`Ink::own`]: crate::charts::Ink::own
    pub fn fill(mut self, gradient: crate::charts::Gradient) -> Self {
        self.meter.fill = Some(gradient);
        self
    }

    /// Colours the bar with any [`Ink`](crate::charts::Ink) of the theme.
    pub fn ink(mut self, ink: impl Into<crate::charts::Ink>) -> Self {
        self.meter.ink = Some(ink.into());
        self
    }

    /// Colours the bar by the band its share (0 to 100) falls in: red when low, green
    /// when good. Say what the band means in the label as well.
    pub fn colors(mut self, scale: crate::charts::ColorScale) -> Self {
        self.meter.scale = Some(scale);
        self
    }

    /// Marks `goal` (out of the same `max`) on the track with a short upright line. Say
    /// the goal in the label as well.
    pub fn goal(mut self, goal: u32) -> Self {
        self.meter.goal = Some(goal.min(self.max));
        self
    }

    /// Puts a soft glow of its own colour around the fill.
    pub fn glow(mut self) -> Self {
        self.meter.glow = true;
        self
    }

    /// How the fill comes in: it grows from the start. Off by default; always off for a
    /// reader who asks for less motion.
    pub fn motion(mut self, motion: crate::charts::Motion) -> Self {
        self.meter.motion = motion;
        self
    }

    /// The bar's thickness in pixels, 2 to 24 (6 by default).
    pub fn height(mut self, px: u32) -> Self {
        self.meter.height = Some(px.clamp(2, 24));
        self
    }

    /// The meter: the progress bar's root as a `div` with the progressbar role, and an
    /// SVG track, fill and goal marker.
    fn meter_node(self) -> Element {
        use crate::charts::motion::Effect;
        use crate::charts::paint::{Defs, glow};
        let meter = self.meter;
        let h = meter.height.unwrap_or(6);
        let share = f64::from(self.value) / f64::from(self.max) * 100.0;
        let mut root = self.el;
        root.tag = "div";
        root.attrs
            .retain(|a| !matches!(a.name, "value" | "max" | "data-slot"));
        let root = root
            .slot("meter")
            .attr("role", "progressbar")
            .attr("aria-valuemin", 0_i64)
            .attr("aria-valuemax", i64::from(self.max))
            .attr("aria-valuenow", i64::from(self.value))
            .uses(Uses::CHART_PAINT);
        let ink = match (&meter.scale, meter.ink) {
            (Some(scale), _) => Some(scale.ink_of(share)),
            (None, ink) => ink,
        };
        let root = match ink {
            Some(ink) => {
                // A colour asked for replaces the tone's.
                let mut root = root;
                root.attrs.retain(|a| a.name != "data-tone");
                ink.on(root)
            }
            None => root,
        };
        let id = root
            .attrs
            .iter()
            .find(|a| a.name == "id")
            .and_then(|a| a.value.as_text().map(|t| t.into_owned()))
            .unwrap_or_else(|| format!("meter-{:x}", fnv(&root)));
        let mut defs = Defs::new(&id);
        let color = ink.unwrap_or(crate::charts::Ink::muted()).color();
        let radius = f64::from(h) / 2.0;
        let fill = (self.value > 0).then(|| {
            let rect = el("rect")
                .slot("meter-fill")
                .attr("width", pct(share))
                .attr("height", i64::from(h))
                .attr("rx", num_text(radius));
            let rect = match &meter.fill {
                Some(g) => rect.attr("fill", defs.gradient(g, color, None)),
                None => rect,
            };
            let rect = if meter.glow { glow(rect) } else { rect };
            meter.motion.apply(rect, Effect::GrowRight, 0)
        });
        let goal = meter.goal.map(|goal| {
            let at = pct(f64::from(goal) / f64::from(self.max) * 100.0);
            el("line")
                .slot("meter-goal")
                .attr("x1", at.clone())
                .attr("x2", at)
                .attr("y1", -3_i64)
                .attr("y2", i64::from(h) + 3)
        });
        root.child(
            el("svg")
                .attr("aria-hidden", "true")
                .attr("focusable", "false")
                .attr("width", "100%")
                .attr("height", i64::from(h))
                .children((
                    defs.node(),
                    el("rect")
                        .slot("meter-track")
                        .attr("width", "100%")
                        .attr("height", i64::from(h))
                        .attr("rx", num_text(radius)),
                    fill,
                    goal,
                )),
        )
    }

    /// Colours the bar by what it means. The colour never carries the meaning alone:
    /// the label says it.
    pub fn tone(mut self, tone: Tone) -> Self {
        if let Some(name) = tone.value() {
            self.el = self.el.attr("data-tone", name).uses(Uses::PARTS_STYLES);
        }
        self
    }

    /// Writes the percentage beside the bar. A screen reader gets it from the bar
    /// itself, so the text is hidden from it. An indeterminate bar has none to write.
    pub fn show_value(mut self) -> Self {
        self.show = true;
        self.el = self.el.uses(Uses::PARTS_STYLES);
        self
    }

    fn into_node(self) -> Node {
        let (show, percent) = (self.show, self.percent);
        let bar = if percent.is_some() && self.meter.is_on() {
            self.meter_node()
        } else {
            self.el
        };
        match (show, percent) {
            (true, Some(percent)) => el("div")
                .slot("progress-row")
                .children((
                    bar,
                    el("span")
                        .slot("progress-value")
                        .attr("aria-hidden", "true")
                        .child(format!("{percent}%")),
                ))
                .into(),
            _ => bar.into(),
        }
    }
}

/// A share as an SVG length: `62.5%`.
fn pct(share: f64) -> String {
    let mut out = num_text(share.clamp(0.0, 100.0));
    out.push('%');
    out
}

/// A number with at most two decimals and no trailing zeros.
fn num_text(value: f64) -> String {
    let out = format!("{value:.2}");
    let out = out.trim_end_matches('0').trim_end_matches('.');
    if out.is_empty() || out == "-" {
        "0".to_owned()
    } else {
        out.to_owned()
    }
}

/// A short hash of an element's attributes: an id for a meter that has none, the same on
/// every render of the same meter.
fn fnv(element: &Element) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for attr in &element.attrs {
        let value = attr.value.as_text().unwrap_or_default();
        for byte in attr.name.bytes().chain(value.bytes()) {
            hash ^= u32::from(byte);
            hash = hash.wrapping_mul(0x0100_0193);
        }
    }
    hash & 0x00ff_ffff
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
        Self {
            el: el("hr").slot("separator"),
        }
    }

    /// A vertical line, in a row.
    pub fn vertical() -> Self {
        Self {
            el: el("div")
                .slot("separator")
                .attr("role", "separator")
                .attr("aria-orientation", "vertical"),
        }
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
        Self {
            el: el("span")
                .slot("spinner")
                .attr("role", "status")
                .attr("aria-label", label.into())
                .child(icon(icons::LOADER_CIRCLE)),
        }
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
        Self {
            el: el("th")
                .slot("table-head")
                .attr("scope", "col")
                .child(label),
        }
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
        self.el = self.el.attr(
            "aria-sort",
            if ascending { "ascending" } else { "descending" },
        );
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
        Self {
            el: el("td").child(content),
        }
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
        Self {
            el: el("tr").child(cells),
        }
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

/// A table of data. The wrapper scrolls on narrow screens, and a keyboard can
/// reach it to scroll it: it is a tab stop. Named with [`Table::label`], it is
/// also a region a screen reader lists (two regions of one page need two names).
#[derive(Clone, Debug)]
#[must_use]
pub struct Table {
    el: Element,
    label: Option<String>,
    caption: Option<Node>,
    head: Vec<Node>,
    rows: Vec<Node>,
    body_id: Option<AttrValue>,
}

impl Table {
    /// An empty table.
    pub fn new() -> Self {
        Self {
            el: el("table").slot("table"),
            label: None,
            caption: None,
            head: Vec::new(),
            rows: Vec::new(),
            body_id: None,
        }
    }

    /// Names the scrolling box around the table, which makes it a region a
    /// screen reader lists.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Gives the body an id, so [`Region::feed`](crate::Region::feed) can add rows to it.
    pub fn body_id(mut self, id: impl Into<AttrValue>) -> Self {
        self.body_id = Some(id.into());
        self
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
            el("tbody").attr_opt("id", self.body_id).child(self.rows),
        ));
        let named = self.label.is_some();
        el("div")
            .slot("table-container")
            .attr("tabindex", 0_i64)
            .attr_opt("role", named.then_some("region"))
            .attr_opt("aria-label", self.label)
            .child(table)
            .into()
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
        Self {
            el: el("h1").slot("heading").child(text.into()),
        }
    }

    /// `<h2>`.
    pub fn h2(text: impl Into<Node>) -> Self {
        Self {
            el: el("h2").slot("heading").child(text.into()),
        }
    }

    /// `<h3>`.
    pub fn h3(text: impl Into<Node>) -> Self {
        Self {
            el: el("h3").slot("heading").child(text.into()),
        }
    }

    /// `<h4>`.
    pub fn h4(text: impl Into<Node>) -> Self {
        Self {
            el: el("h4").slot("heading").child(text.into()),
        }
    }

    /// `<h5>`. It links the parts stylesheet, which sizes it.
    pub fn h5(text: impl Into<Node>) -> Self {
        Self {
            el: el("h5")
                .slot("heading")
                .uses(Uses::PARTS_STYLES)
                .child(text.into()),
        }
    }

    /// `<h6>`. It links the parts stylesheet, which sizes it.
    pub fn h6(text: impl Into<Node>) -> Self {
        Self {
            el: el("h6")
                .slot("heading")
                .uses(Uses::PARTS_STYLES)
                .child(text.into()),
        }
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
        Self {
            el: el("p").slot("text").child(text),
        }
    }

    /// Secondary text.
    pub fn muted(text: impl IntoNodes) -> Self {
        Self::new(text).variant(TextVariant::Muted)
    }

    /// Inline code.
    pub fn code(text: impl Into<Node>) -> Self {
        Self {
            el: el("code").slot("code").child(text.into()),
        }
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
    change: Option<Node>,
    chart: Option<Node>,
}

impl Stat {
    /// The figure `value`, called `label`.
    pub fn new(label: impl Into<Node>, value: impl Into<Node>) -> Self {
        Self {
            el: el("div").slot("stat"),
            label: label.into(),
            value: value.into(),
            trend: None,
            change: None,
            chart: None,
        }
    }

    /// Puts a small chart under the figure, such as a
    /// [`Sparkline`](crate::charts::Sparkline) of its last periods.
    pub fn chart(mut self, chart: impl Into<Node>) -> Self {
        self.chart = Some(chart.into());
        self
    }

    /// Adds how it's moving; `good` colours it.
    pub fn trend(mut self, text: impl Into<Node>, good: bool) -> Self {
        self.trend = Some((text.into(), good));
        self
    }

    /// Adds how it's moving as a [`Trend`]: an arrow, the change with its sign, and a
    /// colour for what it means. It replaces [`trend`](Self::trend).
    pub fn change(mut self, trend: Trend) -> Self {
        self.change = Some(trend.into());
        self
    }

    fn into_node(self) -> Node {
        self.el
            .children((
                el("span").slot("stat-label").child(self.label),
                el("span").slot("stat-value").child(self.value),
                self.change
                    .map(|change| el("span").slot("stat-trend").child(change))
                    .or_else(|| {
                        self.trend.map(|(text, good)| {
                            el("span")
                                .slot("stat-trend")
                                .attr("data-tone", if good { "up" } else { "down" })
                                .child(text)
                        })
                    }),
                self.chart
                    .map(|chart| el("div").slot("stat-chart").child(chart)),
            ))
            .into()
    }
}

/// A small bar chart, drawn on the server as SVG: no script and no style
/// attribute. The description is what assistive technology gets.
///
/// Painted, its bars are rounded columns in a theme colour, a gradient or the colour of
/// each value's band, and can grow in. For a table twin and a summary, use a
/// [`Sparkline`](crate::charts::Sparkline) with [`columns`](crate::charts::Sparkline::columns).
///
/// ```
/// use navree_ui::charts::{ColorScale, Gradient, Ink, Motion};
/// use navree_ui::{Bars, Tone};
///
/// let hrv = Bars::new("Heart rate variability, 14 days", &[48, 52, 45, 61, 58, 44, 50, 63, 57, 49, 55, 60, 62, 59])
///     .colors(ColorScale::new(Ink::tone(Tone::Warning)).from(50, Ink::tone(Tone::Success)))
///     .pill()
///     .motion(Motion::new());
/// # let _ = hrv;
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Bars {
    el: Element,
    values: Vec<u32>,
    ink: Option<crate::charts::Ink>,
    scale: Option<crate::charts::ColorScale>,
    fill: Option<crate::charts::Gradient>,
    pill: bool,
    motion: crate::charts::Motion,
}

impl Bars {
    /// `values` as bars, described by `description`.
    pub fn new(description: impl Into<String>, values: &[u32]) -> Self {
        Self {
            el: el("svg")
                .slot("bars")
                .attr("role", "img")
                .attr("aria-label", description.into()),
            values: values.to_vec(),
            ink: None,
            scale: None,
            fill: None,
            pill: false,
            motion: crate::charts::Motion::none(),
        }
    }

    /// Colours the bars with any [`Ink`](crate::charts::Ink) of the theme.
    pub fn ink(mut self, ink: impl Into<crate::charts::Ink>) -> Self {
        self.ink = Some(ink.into());
        self
    }

    /// Colours each bar by the band its value falls in. Say what the colours mean beside
    /// the bars.
    pub fn colors(mut self, scale: crate::charts::ColorScale) -> Self {
        self.scale = Some(scale);
        self
    }

    /// Fills each bar with `gradient`; [`Ink::own`](crate::charts::Ink::own) in its
    /// stops is the bar's colour.
    pub fn fill(mut self, gradient: crate::charts::Gradient) -> Self {
        self.fill = Some(gradient);
        self
    }

    /// Rounds both ends of each bar into a pill.
    pub fn pill(mut self) -> Self {
        self.pill = true;
        self
    }

    /// How the bars come in: they grow from the baseline, one after another. Off by
    /// default; always off for a reader who asks for less motion.
    pub fn motion(mut self, motion: crate::charts::Motion) -> Self {
        self.motion = motion;
        self
    }

    fn painted(&self) -> bool {
        self.ink.is_some()
            || self.scale.is_some()
            || self.fill.is_some()
            || self.pill
            || self.motion.is_on()
    }

    fn into_node(self) -> Node {
        let top = self.values.iter().copied().max().unwrap_or(0).max(1);
        if !self.painted() {
            let bars: Vec<Element> = self
                .values
                .iter()
                .enumerate()
                .map(|(at, value)| {
                    let height = i64::from(value.saturating_mul(100) / top);
                    el("rect")
                        .attr("x", i64::try_from(at * 10).unwrap_or(0) + 1)
                        .attr("y", 100 - height)
                        .attr("width", 8_i64)
                        .attr("height", height)
                })
                .collect();
            let width = i64::try_from(self.values.len() * 10).unwrap_or(0);
            return self
                .el
                .attr("viewBox", format!("0 0 {width} 100"))
                .attr("preserveAspectRatio", "none")
                .child(bars)
                .into();
        }
        // Painted: x and width in percent, y and height in percent of the box, so the
        // rounded ends keep their shape at any size.
        use crate::charts::motion::Effect;
        use crate::charts::paint::Defs;
        let count = self.values.len().max(1);
        let band = 100.0 / count as f64;
        let id = format!("bars-{:x}", fnv(&self.el) ^ self.values.len() as u32);
        let mut defs = Defs::new(&id);
        let mut painted: Vec<(crate::charts::Ink, String)> = Vec::new();
        let base = self.ink.unwrap_or(crate::charts::Ink::muted());
        let clip = if self.pill {
            "inset(0 max(1px, 15%) round 99px)"
        } else {
            "inset(0 max(1px, 10%) round 2px 2px 0 0)"
        };
        let bars: Vec<Node> = self
            .values
            .iter()
            .enumerate()
            .map(|(at, value)| {
                let share = f64::from(*value) / f64::from(top) * 100.0;
                // A bar of nothing keeps a sliver, so the row keeps its rhythm.
                let share = if *value == 0 { 2.0 } else { share.max(4.0) };
                let ink = match &self.scale {
                    Some(scale) => scale.ink_of(f64::from(*value)),
                    None => base,
                };
                let rect = ink.on(el("rect")
                    .attr("x", pct(band * at as f64))
                    .attr("width", pct(band))
                    .attr("y", pct(100.0 - share))
                    .attr("height", pct(share))
                    .attr("clip-path", clip));
                let rect = match &self.fill {
                    Some(g) => {
                        let url = match painted.iter().find(|(i, _)| *i == ink) {
                            Some((_, url)) => url.clone(),
                            None => {
                                let url = defs.gradient(g, ink.color(), None);
                                painted.push((ink, url.clone()));
                                url
                            }
                        };
                        rect.attr("fill", url)
                    }
                    None => rect,
                };
                self.motion.apply(rect, Effect::GrowUp, at).into()
            })
            .collect();
        self.el
            .attr("data-painted", "")
            .uses(Uses::CHART_PAINT)
            .children((defs.node(), bars))
            .into()
    }
}

root_methods!(Stat, Bars);
into_node!(Stat, Bars);

root_methods!(
    Alert,
    AspectRatio,
    Attachment,
    Avatar,
    AvatarGroup,
    Badge,
    Breadcrumb,
    Bubble,
    Card,
    Direction,
    Empty,
    Item,
    Kbd,
    Marker,
    Message,
    Pagination,
    Progress,
    Separator,
    Skeleton,
    Spinner,
    Table,
    TableHead,
    TableCell,
    TableRowEl,
    Heading,
    Text,
);
into_node!(
    Alert,
    AspectRatio,
    Attachment,
    Avatar,
    AvatarGroup,
    Badge,
    Breadcrumb,
    Bubble,
    Card,
    Direction,
    Empty,
    Item,
    Kbd,
    Marker,
    Message,
    Pagination,
    Progress,
    Separator,
    Skeleton,
    Spinner,
    Table,
    TableHead,
    TableCell,
    TableRowEl,
    Heading,
    Text,
);
