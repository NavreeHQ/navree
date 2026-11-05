//! Markdown and code blocks, drawn on the server through the toolkit's one escaping path.
//!
//! [`Markdown`] reads CommonMark (with tables, strikethrough and task lists) and builds
//! the same [`Node`] tree every component builds, so the text of a document can never
//! become markup. What the safety rests on:
//!
//! - **No raw HTML.** An HTML block or tag in the text is shown as the text it is.
//! - **Links are checked.** Only `http`, `https`, `mailto`, `tel`, a path, a query or a
//!   fragment make a link; anything else keeps its words and loses the link. A link to
//!   another site carries `rel="noopener noreferrer"` and, in untrusted text, `nofollow`
//!   and its host, so a link that says one thing and goes to another can be seen.
//! - **Images are a decision.** Untrusted text never draws one: a picture in text from a
//!   model is a way to send what it knows to another site. Trusted text may draw images
//!   from this site.
//! - **Nothing grows without a bound.** The text is cut at [`MAX_BYTES`], nesting stops
//!   at a fixed depth, and a code block past 64 KB is not coloured.
//!
//! [`CodeBlock`] draws code with its colours already in the markup: no script runs in
//! the browser to colour it.

mod highlight;

use std::borrow::Cow;

use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::cmd::Cmd;
use crate::components::{Button, ButtonVariant, Size};
use crate::html::LocalHref;
use crate::icons;
use crate::node::{Element, Node, Uses, el, into_node};
use crate::render::render;

/// The most Markdown text that is read. A longer text is cut at a character boundary, and
/// a line says so.
pub const MAX_BYTES: usize = 256 * 1024;

/// How deep lists, quotes and tables nest before the rest is flattened.
const MAX_DEPTH: usize = 24;

/// The longest code the copy button carries. A longer block has no button.
const MAX_COPY: usize = 16 * 1024;

/// How far the text is trusted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MarkdownTrust {
    /// Text from a model, a visitor or anyone you do not control. No image is drawn, and a
    /// link to another site is marked with its host and given `nofollow`.
    #[default]
    Untrusted,
    /// Text your own authors wrote. Images from this site are drawn, and links to other
    /// sites are not marked beyond `noopener noreferrer`.
    Trusted,
}

/// The words Markdown and code blocks write, for translation.
#[derive(Clone, Debug)]
pub struct MarkdownLabels {
    /// Read by assistive technology after a link that opens a new tab.
    pub new_tab: Cow<'static, str>,
    /// Where a picture would be, in untrusted text.
    pub image_removed: Cow<'static, str>,
    /// Names a table that scrolls sideways.
    pub table: Cow<'static, str>,
    /// The name of a task list item that is done.
    pub task_done: Cow<'static, str>,
    /// The name of a task list item that is not done.
    pub task_open: Cow<'static, str>,
    /// The copy button of a code block.
    pub copy_code: Cow<'static, str>,
    /// The line after text that was cut.
    pub cut: Cow<'static, str>,
}

impl Default for MarkdownLabels {
    fn default() -> Self {
        Self {
            new_tab: Cow::Borrowed(" (opens in a new tab)"),
            image_removed: Cow::Borrowed("Image not shown"),
            table: Cow::Borrowed("Table"),
            task_done: Cow::Borrowed("Done"),
            task_open: Cow::Borrowed("Not done"),
            copy_code: Cow::Borrowed("Copy code"),
            cut: Cow::Borrowed("The rest of this text is not shown."),
        }
    }
}

/// Text written in Markdown, drawn as HTML that cannot hold anything the text did not say.
///
/// ```
/// use navree_ui::{Markdown, render};
///
/// let html = render(&Markdown::new("**Hello** <script>alert(1)</script>").into());
/// assert!(html.contains("<strong>Hello</strong>"));
/// assert!(!html.contains("<script>"));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Markdown {
    text: String,
    trust: MarkdownTrust,
    first_heading: u8,
    new_tab: bool,
    copy: bool,
    max_bytes: usize,
    labels: MarkdownLabels,
}

impl Markdown {
    /// Markdown from a source you do not control. Use [`trusted`](Self::trusted) for text
    /// your own authors wrote.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            trust: MarkdownTrust::Untrusted,
            first_heading: 1,
            new_tab: false,
            copy: false,
            max_bytes: MAX_BYTES,
            labels: MarkdownLabels::default(),
        }
    }

    /// Markdown your own authors wrote: images from this site are drawn.
    pub fn trusted(text: impl Into<String>) -> Self {
        Self::new(text).trust(MarkdownTrust::Trusted)
    }

    /// Sets how far the text is trusted.
    pub fn trust(mut self, trust: MarkdownTrust) -> Self {
        self.trust = trust;
        self
    }

    /// Sets which heading level `#` becomes, 1 to 6 (1 by default). Deeper headings follow:
    /// at 3, `#` is an `h3` and `##` an `h4`. None goes past `h6`. A chat message in a page
    /// that has its own `h1` and `h2` starts at 3.
    pub fn heading_level(mut self, level: u8) -> Self {
        self.first_heading = level.clamp(1, 6);
        self
    }

    /// Opens links to other sites in a new tab.
    pub fn links_in_new_tab(mut self) -> Self {
        self.new_tab = true;
        self
    }

    /// Adds a copy button to every code block (short ones only). It needs the core runtime.
    pub fn copy_code(mut self) -> Self {
        self.copy = true;
        self
    }

    /// Sets the most bytes of text that is read (never more than [`MAX_BYTES`]).
    pub fn max_bytes(mut self, bytes: usize) -> Self {
        self.max_bytes = bytes.min(MAX_BYTES);
        self
    }

    /// Sets the words.
    pub fn labels(mut self, labels: MarkdownLabels) -> Self {
        self.labels = labels;
        self
    }

    /// The text drawn as a fragment of HTML with no element around it. Use it where the
    /// HTML goes somewhere that is not a toolkit page, such as an e-mail or an API reply.
    #[must_use]
    pub fn to_fragment_html(&self) -> String {
        render(&Node::list(self.content()))
    }

    /// The text and the element that holds it, as HTML.
    #[must_use]
    pub fn to_html(&self) -> String {
        render(&self.clone().into_node())
    }

    fn content(&self) -> Vec<Node> {
        let (text, cut) = self.limited();
        let mut options = Options::empty();
        options.insert(Options::ENABLE_TABLES);
        options.insert(Options::ENABLE_STRIKETHROUGH);
        options.insert(Options::ENABLE_TASKLISTS);
        let mut writer = Writer {
            md: self,
            stack: vec![el("div")],
            overflow: 0,
            code: None,
            aligns: Vec::new(),
            column: 0,
            in_head: false,
            images: Vec::new(),
        };
        for event in Parser::new_ext(text, options) {
            writer.event(event);
        }
        if cut {
            writer.open(el("p").slot("markdown-cut"));
            writer.text(&self.labels.cut);
            writer.close();
        }
        writer.finish()
    }

    /// The text to read, and whether some was left out.
    fn limited(&self) -> (&str, bool) {
        if self.text.len() <= self.max_bytes {
            return (&self.text, false);
        }
        let mut end = self.max_bytes;
        while end > 0 && !self.text.is_char_boundary(end) {
            end -= 1;
        }
        (&self.text[..end], true)
    }

    fn into_node(self) -> Node {
        let untrusted = self.trust == MarkdownTrust::Untrusted;
        el("div")
            .slot("markdown")
            .flag("data-untrusted", untrusted)
            .uses(Uses::MARKDOWN)
            .child(self.content())
            .into()
    }
}

into_node!(Markdown);

/// Code, drawn with its colours in the markup.
///
/// The language picks the colours: `rust`, `ts`, `tsx`, `js`, `json`, `toml`, `yaml`,
/// `sh`, `sql`, `html`, `css`, `diff`, `http`, `python`, `go` and `env`. Any other name
/// leaves the code plain, and a name is shown as the block's caption.
///
/// ```
/// use navree_ui::{CodeBlock, render};
///
/// let html = render(&CodeBlock::new("rust", "let total = 42;").into());
/// assert!(html.contains(r#"<span data-t="k">let</span>"#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct CodeBlock {
    language: String,
    code: String,
    copy: bool,
    labels: MarkdownLabels,
}

impl CodeBlock {
    /// A block of `code` in `language` (an empty name is plain code).
    pub fn new(language: impl AsRef<str>, code: impl Into<String>) -> Self {
        Self {
            language: clean_language(language.as_ref()),
            code: code.into(),
            copy: false,
            labels: MarkdownLabels::default(),
        }
    }

    /// Adds a button that copies the code (short blocks only). It needs the core runtime.
    pub fn copy_button(mut self) -> Self {
        self.copy = true;
        self
    }

    /// Sets the words.
    pub fn labels(mut self, labels: MarkdownLabels) -> Self {
        self.labels = labels;
        self
    }

    fn into_node(self) -> Node {
        code_block(&self.language, &self.code, self.copy, &self.labels)
    }
}

into_node!(CodeBlock);

/// The first word of a fence's info string, held to the characters a language name has.
fn clean_language(info: &str) -> String {
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

fn code_block(language: &str, code: &str, copy: bool, labels: &MarkdownLabels) -> Node {
    let shown = code.strip_suffix('\n').unwrap_or(code);
    let inside: Vec<Node> = match highlight::highlight(language, shown) {
        Some(nodes) => nodes,
        None => vec![Node::text(shown.to_owned())],
    };
    let button = (copy && shown.len() <= MAX_COPY).then(|| {
        Button::icon_only(icons::COPY, labels.copy_code.to_string())
            .variant(ButtonVariant::Ghost)
            .size(Size::Sm)
            .on_click(Cmd::copy(shown.to_owned()))
    });
    let bar = (!language.is_empty() || button.is_some()).then(|| {
        el("div").slot("code-block-bar").children((
            el("span")
                .slot("code-block-language")
                .child(language.to_owned()),
            button,
        ))
    });
    el("div")
        .slot("code-block")
        .uses(Uses::MARKDOWN)
        .children((
            bar,
            el("pre")
                .attr("tabindex", 0_i64)
                .child(el("code").child(inside)),
        ))
        .into()
}

/// The tag of a heading, `#` being `first`.
fn heading_tag(first: u8, level: HeadingLevel) -> &'static str {
    let depth = match level {
        HeadingLevel::H1 => 0,
        HeadingLevel::H2 => 1,
        HeadingLevel::H3 => 2,
        HeadingLevel::H4 => 3,
        HeadingLevel::H5 => 4,
        HeadingLevel::H6 => 5,
    };
    match (first.clamp(1, 6) + depth).min(6) {
        1 => "h1",
        2 => "h2",
        3 => "h3",
        4 => "h4",
        5 => "h5",
        _ => "h6",
    }
}

/// Whether a link's address goes to another site: it names a host, with a scheme or with
/// `//`, `/\` or `\\` (which browsers read as `//`). A `mailto:` or `tel:` link does not.
fn is_external(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("mailto:") || lower.starts_with("tel:") {
        return false;
    }
    LocalHref::parse(url).is_err()
}

/// The host a link goes to, for the reader to see: no scheme, no user name, no path.
fn host_of(url: &str) -> String {
    let after = match url.split_once("://") {
        Some((_, rest)) => rest,
        None => url.trim_start_matches(['/', '\\']),
    };
    let authority = after
        .split(['/', '\\', '?', '#'])
        .next()
        .unwrap_or_default();
    let host = authority.rsplit('@').next().unwrap_or_default();
    host.chars()
        .take(80)
        .collect::<String>()
        .to_ascii_lowercase()
}

/// Turns the parser's events into elements.
struct Writer<'a> {
    md: &'a Markdown,
    /// The open elements, the root first.
    stack: Vec<Element>,
    /// Containers past the depth limit, whose content joins the element below them.
    overflow: usize,
    /// The language and text of a code block being read.
    code: Option<(String, String)>,
    aligns: Vec<Alignment>,
    column: usize,
    in_head: bool,
    /// The address of each open image, when it may be drawn.
    images: Vec<Option<String>>,
}

impl Writer<'_> {
    fn open(&mut self, element: Element) {
        if self.stack.len() >= MAX_DEPTH {
            self.overflow += 1;
        } else {
            self.stack.push(element);
        }
    }

    fn close(&mut self) {
        self.close_with(Node::Element);
    }

    fn close_with(&mut self, finish: impl FnOnce(Element) -> Node) {
        if self.overflow > 0 {
            self.overflow -= 1;
            return;
        }
        if self.stack.len() <= 1 {
            return;
        }
        if let Some(done) = self.stack.pop() {
            let node = finish(done);
            self.add(node);
        }
    }

    fn add(&mut self, node: Node) {
        if let Some(top) = self.stack.pop() {
            self.stack.push(top.child(node));
        }
    }

    fn text(&mut self, text: &str) {
        self.add(Node::text(text.to_owned()));
    }

    fn finish(mut self) -> Vec<Node> {
        self.overflow = 0;
        while self.stack.len() > 1 {
            self.close();
        }
        self.stack
            .pop()
            .map(|root| root.children)
            .unwrap_or_default()
    }

    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(end) => self.end(end),
            Event::Text(text) => match &mut self.code {
                Some((_, buffer)) => buffer.push_str(&text),
                None => self.text(&text),
            },
            Event::Code(text) => {
                self.add(el("code").slot("code").child(text.into_string()).into());
            }
            // Raw HTML is text: it shows as the characters it is.
            Event::Html(text) | Event::InlineHtml(text) => match &mut self.code {
                Some((_, buffer)) => buffer.push_str(&text),
                None => self.text(&text),
            },
            Event::SoftBreak => self.text("\n"),
            Event::HardBreak => self.add(el("br").into()),
            Event::Rule => self.add(el("hr").into()),
            Event::TaskListMarker(done) => {
                let label = if done {
                    self.md.labels.task_done.clone()
                } else {
                    self.md.labels.task_open.clone()
                };
                self.add(
                    el("input")
                        .attr("type", "checkbox")
                        .attr("aria-label", label)
                        .flag("checked", done)
                        .flag("disabled", true)
                        .into(),
                );
            }
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => self.open(el("p")),
            Tag::Heading { level, .. } => {
                let tag = heading_tag(self.md.first_heading, level);
                self.open(el(tag));
            }
            Tag::BlockQuote(_) => self.open(el("blockquote")),
            Tag::CodeBlock(kind) => {
                let language = match kind {
                    CodeBlockKind::Fenced(info) => clean_language(&info),
                    CodeBlockKind::Indented => String::new(),
                };
                self.code = Some((language, String::new()));
            }
            Tag::HtmlBlock => self.open(el("p").slot("markdown-raw")),
            Tag::List(Some(first)) if first != 1 => {
                let first = i64::try_from(first).unwrap_or(1);
                self.open(el("ol").attr("start", first));
            }
            Tag::List(Some(_)) => self.open(el("ol")),
            Tag::List(None) => self.open(el("ul")),
            Tag::Item => self.open(el("li")),
            Tag::Table(aligns) => {
                self.aligns = aligns;
                self.open(
                    el("div")
                        .slot("markdown-table")
                        .attr("role", "group")
                        .attr("aria-label", self.md.labels.table.clone())
                        .attr("tabindex", 0_i64),
                );
                self.open(el("table"));
            }
            Tag::TableHead => {
                self.in_head = true;
                self.column = 0;
                self.open(el("thead"));
                self.open(el("tr"));
            }
            Tag::TableRow => {
                self.column = 0;
                self.open(el("tr"));
            }
            Tag::TableCell => {
                let mut cell = el(if self.in_head { "th" } else { "td" });
                if self.in_head {
                    cell = cell.attr("scope", "col");
                }
                let align = match self.aligns.get(self.column) {
                    Some(Alignment::Left) => Some("start"),
                    Some(Alignment::Center) => Some("center"),
                    Some(Alignment::Right) => Some("end"),
                    _ => None,
                };
                self.column += 1;
                self.open(cell.attr_opt("data-cell-align", align));
            }
            Tag::Emphasis => self.open(el("em")),
            Tag::Strong => self.open(el("strong")),
            Tag::Strikethrough => self.open(el("del")),
            Tag::Link {
                dest_url, title, ..
            } => {
                let link = self.link(&dest_url, &title);
                self.open(link);
            }
            Tag::Image {
                dest_url, title, ..
            } => {
                let src = if self.md.trust == MarkdownTrust::Trusted {
                    LocalHref::parse(dest_url.trim())
                        .ok()
                        .map(|href| href.as_str().to_owned())
                } else {
                    None
                };
                self.images.push(src);
                let mut holder = el("span");
                if !title.is_empty() {
                    holder = holder.attr("title", title.to_string());
                }
                self.open(holder);
            }
            _ => self.open(el("div")),
        }
    }

    fn end(&mut self, end: TagEnd) {
        match end {
            TagEnd::CodeBlock => {
                if let Some((language, code)) = self.code.take() {
                    let node = code_block(&language, &code, self.md.copy, &self.md.labels);
                    self.add(node);
                }
            }
            TagEnd::TableHead => {
                self.close();
                self.close();
                self.in_head = false;
            }
            TagEnd::Table => {
                self.close();
                self.close();
            }
            TagEnd::Link => {
                let note = self.md.labels.new_tab.clone();
                self.close_with(move |link| {
                    if link.tag() == "a" && link.get("target").is_some() {
                        link.child(el("span").flag("data-ui-sr", true).child(note.to_string()))
                            .into()
                    } else {
                        link.into()
                    }
                });
            }
            TagEnd::Image => {
                let src = self.images.pop().flatten();
                let removed = self.md.labels.image_removed.clone();
                self.close_with(move |holder| {
                    let title = holder
                        .get("title")
                        .and_then(|v| v.as_text())
                        .map(Cow::into_owned);
                    let label: Node = holder.into();
                    let alt = label.text_content();
                    match src {
                        Some(src) => el("img")
                            .attr("src", src)
                            .attr("alt", alt)
                            .attr_opt("title", title)
                            .attr("loading", "lazy")
                            .attr("decoding", "async")
                            .into(),
                        None if alt.is_empty() => el("span")
                            .slot("markdown-image")
                            .child(removed.to_string())
                            .into(),
                        None => el("span")
                            .slot("markdown-image")
                            .attr("title", removed.to_string())
                            .child(alt)
                            .into(),
                    }
                });
            }
            _ => self.close(),
        }
    }

    /// The element for a link: an `a` when the address is one a link may have, and a plain
    /// `span` when it is not, so the words stay and the link goes.
    fn link(&self, dest: &str, title: &str) -> Element {
        let dest = dest.trim();
        let Ok(href) = crate::html::Href::parse(dest) else {
            return el("span");
        };
        if dest.is_empty() {
            return el("span");
        }
        let external = is_external(dest);
        let untrusted = self.md.trust == MarkdownTrust::Untrusted;
        let mut link = el("a").href(href);
        if !title.is_empty() {
            link = link.attr("title", title.to_owned());
        }
        if external {
            link = link
                .attr(
                    "rel",
                    if untrusted {
                        "noopener noreferrer nofollow"
                    } else {
                        "noopener noreferrer"
                    },
                )
                .flag("data-external", true);
            if untrusted {
                let host = host_of(dest);
                if !host.is_empty() {
                    link = link.attr("data-host", host);
                }
            }
            if self.md.new_tab {
                link = link.attr("target", "_blank");
            }
        }
        link
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn html(text: &str) -> String {
        Markdown::new(text).to_fragment_html()
    }

    fn trusted(text: &str) -> String {
        Markdown::trusted(text).to_fragment_html()
    }

    #[test]
    fn the_blocks_of_commonmark_are_drawn() {
        let out = html(
            "# Title\n\nA paragraph with *em*, **strong**, ~~gone~~ and `code`.\n\n> quoted\n\n---\n\n1. one\n2. two\n\n- a\n- b\n",
        );
        assert!(out.contains("<h1>Title</h1>"), "{out}");
        assert!(out.contains("<em>em</em>"), "{out}");
        assert!(out.contains("<strong>strong</strong>"), "{out}");
        assert!(out.contains("<del>gone</del>"), "{out}");
        assert!(
            out.contains(r#"<code data-slot="code">code</code>"#),
            "{out}"
        );
        assert!(out.contains("<blockquote>"), "{out}");
        assert!(out.contains("<hr>"), "{out}");
        assert!(out.contains("<ol><li>one</li><li>two</li></ol>"), "{out}");
        assert!(out.contains("<ul><li>a</li><li>b</li></ul>"), "{out}");
    }

    #[test]
    fn an_ordered_list_keeps_its_first_number() {
        let out = html("3. c\n4. d\n");
        assert!(out.contains(r#"<ol start="3">"#), "{out}");
    }

    #[test]
    fn headings_start_where_they_are_told_and_stop_at_six() {
        let out = Markdown::new("# a\n\n## b\n\n###### c")
            .heading_level(3)
            .to_fragment_html();
        assert!(out.contains("<h3>a</h3>"), "{out}");
        assert!(out.contains("<h4>b</h4>"), "{out}");
        assert!(out.contains("<h6>c</h6>"), "{out}");
        let out = Markdown::new("# a").heading_level(9).to_fragment_html();
        assert!(out.contains("<h6>a</h6>"), "{out}");
    }

    #[test]
    fn raw_html_is_text() {
        let out = html(
            "hi <script>alert(1)</script> <img src=x onerror=alert(1)>\n\n<div onclick=\"x()\">block</div>\n",
        );
        assert!(!out.contains("<script"), "{out}");
        assert!(!out.contains("<img"), "{out}");
        assert!(!out.contains("<div onclick"), "{out}");
        assert!(
            out.contains("&lt;script&gt;alert(1)&lt;/script&gt;"),
            "{out}"
        );
        assert!(out.contains("&lt;div onclick="), "{out}");
    }

    #[test]
    fn a_link_with_a_bad_scheme_keeps_its_words_and_loses_the_link() {
        for bad in [
            "javascript:alert(1)",
            "JaVaScRiPt:alert(1)",
            "data:text/html;base64,PHNjcmlwdD4=",
            "vbscript:x",
            "file:///etc/passwd",
            "java\tscript:alert(1)",
            "&#106;avascript:alert(1)",
        ] {
            let out = html(&format!("[click me]({bad})"));
            assert!(!out.contains("<a "), "{bad}: {out}");
            assert!(out.contains("click me"), "{bad}: {out}");
            assert!(!out.contains("javascript:"), "{bad}: {out}");
        }
    }

    #[test]
    fn a_link_to_another_site_is_marked_and_shows_its_host() {
        let out = html("[bank](https://secure-login.example.com/path?x=1)");
        assert!(
            out.contains(r#"href="https://secure-login.example.com/path?x=1""#),
            "{out}"
        );
        assert!(
            out.contains(r#"rel="noopener noreferrer nofollow""#),
            "{out}"
        );
        assert!(out.contains("data-external"), "{out}");
        assert!(
            out.contains(r#"data-host="secure-login.example.com""#),
            "{out}"
        );
    }

    #[test]
    fn the_host_shown_is_the_one_the_browser_goes_to() {
        for (url, host) in [
            ("https://google.com@evil.example/x", "evil.example"),
            ("https://user:pass@evil.example:8443/x", "evil.example:8443"),
            ("//evil.example/x", "evil.example"),
            ("HTTPS://EVIL.example", "evil.example"),
            ("https://evil.example?x=1", "evil.example"),
            ("https://evil.example#frag", "evil.example"),
        ] {
            let out = html(&format!("[x]({url})"));
            assert!(
                out.contains(&format!(r#"data-host="{host}""#)),
                "{url}: {out}"
            );
            assert!(out.contains("data-external"), "{url}: {out}");
        }
        // Backslashes are slashes to a browser, so these leave the site as well.
        for url in ["/\\evil.example/x", "\\\\evil.example/x"] {
            let out = html(&format!("[x](<{url}>)"));
            assert!(out.contains("data-external"), "{url}: {out}");
        }
    }

    #[test]
    fn links_on_this_site_and_mail_links_are_not_external() {
        for url in [
            "/deals/7",
            "../up",
            "#section",
            "?page=2",
            "mailto:gurpreet@example.com",
            "tel:+919876543210",
        ] {
            let out = html(&format!("[x]({url})"));
            assert!(out.contains(&format!(r#"href="{url}""#)), "{url}: {out}");
            assert!(!out.contains("data-external"), "{url}: {out}");
            assert!(!out.contains("rel="), "{url}: {out}");
        }
    }

    #[test]
    fn trusted_links_carry_noopener_but_not_the_host() {
        let out = trusted("[docs](https://navree.com/docs)");
        assert!(out.contains(r#"rel="noopener noreferrer""#), "{out}");
        assert!(
            !out.contains("nofollow") && !out.contains("data-host"),
            "{out}"
        );
    }

    #[test]
    fn a_link_may_open_a_new_tab_and_says_so() {
        let out = Markdown::new("[x](https://example.com)")
            .links_in_new_tab()
            .to_fragment_html();
        assert!(out.contains(r#"target="_blank""#), "{out}");
        assert!(out.contains(r#"<span data-ui-sr>"#), "{out}");
        let local = Markdown::new("[x](/here)")
            .links_in_new_tab()
            .to_fragment_html();
        assert!(!local.contains("target="), "{local}");
    }

    #[test]
    fn an_empty_link_is_only_words() {
        let out = html("[nothing]()");
        assert!(!out.contains("<a "), "{out}");
        assert!(out.contains("nothing"), "{out}");
    }

    #[test]
    fn untrusted_text_never_draws_an_image() {
        for text in [
            "![secret](https://attacker.example/steal?data=abc)",
            "![x](/local.png)",
            "![x](data:image/png;base64,AAAA)",
            "![](https://attacker.example/pixel.gif)",
            "[![x](https://attacker.example/a.png)](https://example.com)",
        ] {
            let out = html(text);
            assert!(!out.contains("<img"), "{text}: {out}");
            assert!(!out.contains("attacker.example/steal"), "{text}: {out}");
            assert!(!out.contains("pixel.gif"), "{text}: {out}");
        }
        let out = html("![chart of sales](https://attacker.example/x.png)");
        assert!(out.contains("chart of sales"), "{out}");
        assert!(out.contains("Image not shown"), "{out}");
    }

    #[test]
    fn trusted_text_draws_images_from_this_site_only() {
        let out = trusted("![Ludhiana store](/media/store.png \"The store\")");
        assert!(out.contains(r#"<img src="/media/store.png" alt="Ludhiana store" title="The store" loading="lazy" decoding="async">"#), "{out}");
        for far in [
            "https://other.example/a.png",
            "//other.example/a.png",
            "data:image/png;base64,AAAA",
            "javascript:x",
        ] {
            let out = trusted(&format!("![x]({far})"));
            assert!(!out.contains("<img"), "{far}: {out}");
        }
    }

    #[test]
    fn code_is_coloured_by_its_language_and_escaped() {
        let out = html("```rust\nfn main() { println!(\"<b>\"); }\n```\n");
        assert!(out.contains(r#"data-slot="code-block""#), "{out}");
        assert!(out.contains(r#"<span data-t="k">fn</span>"#), "{out}");
        assert!(out.contains("&lt;b&gt;"), "{out}");
        assert!(!out.contains("<b>"), "{out}");
        assert!(
            out.contains(r#"data-slot="code-block-language">rust<"#),
            "{out}"
        );
    }

    #[test]
    fn code_in_a_language_the_readers_do_not_know_is_plain() {
        let out = html("```klingon\nqapla'\n```\n\n    indented\n");
        assert!(!out.contains("data-t="), "{out}");
        assert!(out.contains("qapla&#39;"), "{out}");
        assert!(out.contains("indented"), "{out}");
    }

    #[test]
    fn a_hostile_language_name_cannot_leave_its_attribute_or_its_caption() {
        let out = html("```\"><script>alert(1)</script>\ncode\n```\n");
        assert!(!out.contains("<script"), "{out}");
        let out = CodeBlock::new("rust\" onmouseover=\"x", "1").into_html_for_test();
        assert!(!out.contains("onmouseover"), "{out}");
    }

    impl CodeBlock {
        fn into_html_for_test(self) -> String {
            render(&self.into_node())
        }
    }

    #[test]
    fn tables_scroll_in_a_labelled_group_and_align_their_cells() {
        let out = html("| Name | Amount |\n|:--|--:|\n| Harleen Kaur | ₹1,00,000 |\n");
        assert!(
            out.contains(
                r#"data-slot="markdown-table" role="group" aria-label="Table" tabindex="0""#
            ),
            "{out}"
        );
        assert!(
            out.contains(r#"<th scope="col" data-cell-align="start">Name</th>"#),
            "{out}"
        );
        assert!(
            out.contains(r#"<td data-cell-align="end">₹1,00,000</td>"#),
            "{out}"
        );
        assert!(!out.contains("style="), "{out}");
    }

    #[test]
    fn task_list_items_are_named_checkboxes_that_cannot_be_changed() {
        let out = html("- [x] done\n- [ ] open\n");
        assert!(
            out.contains(r#"aria-label="Done" checked disabled"#),
            "{out}"
        );
        assert!(out.contains(r#"aria-label="Not done" disabled"#), "{out}");
    }

    #[test]
    fn the_words_can_be_translated() {
        let labels = MarkdownLabels {
            image_removed: "ਤਸਵੀਰ ਨਹੀਂ ਦਿਖਾਈ".into(),
            table: "ਸਾਰਣੀ".into(),
            ..MarkdownLabels::default()
        };
        let out = Markdown::new("![](https://a.example/x.png)\n\n| a |\n|---|\n| b |\n")
            .labels(labels)
            .to_fragment_html();
        assert!(out.contains("ਤਸਵੀਰ ਨਹੀਂ ਦਿਖਾਈ"), "{out}");
        assert!(out.contains(r#"aria-label="ਸਾਰਣੀ""#), "{out}");
    }

    #[test]
    fn a_text_past_the_limit_is_cut_at_a_character_and_says_so() {
        let text = "ਪੰਜਾਬ ".repeat(100);
        let out = Markdown::new(text).max_bytes(101).to_fragment_html();
        assert!(out.contains(r#"data-slot="markdown-cut""#), "{out}");
        let whole = Markdown::new("short").to_fragment_html();
        assert!(!whole.contains("markdown-cut"), "{whole}");
    }

    #[test]
    fn nesting_stops_at_a_depth_and_the_rest_is_flattened() {
        let deep = format!("{}deep", "> ".repeat(500));
        let out = html(&deep);
        assert!(out.contains("deep"), "{out}");
        assert!(out.matches("<blockquote>").count() <= MAX_DEPTH, "{out}");
        assert_eq!(
            out.matches("<blockquote>").count(),
            out.matches("</blockquote>").count()
        );
        let lists = format!("{}x", "- ".repeat(300));
        let out = html(&lists);
        assert!(out.contains('x'), "{out}");
        assert!(out.matches("<ul>").count() <= MAX_DEPTH, "{out}");
    }

    #[test]
    fn the_element_marks_untrusted_text_and_asks_for_its_stylesheet() {
        let node: Node = Markdown::new("x").into();
        assert!(crate::render::measure(&node).uses.has(Uses::MARKDOWN));
        let out = render(&node);
        assert!(
            out.starts_with(r#"<div data-slot="markdown" data-untrusted>"#),
            "{out}"
        );
        let node: Node = Markdown::trusted("x").into();
        assert!(!render(&node).contains("data-untrusted"));
    }

    #[test]
    fn a_copy_button_carries_the_code_and_a_long_block_has_none() {
        let out = Markdown::new("```sh\ncargo build\n```\n")
            .copy_code()
            .to_html();
        assert!(out.contains("Copy code"), "{out}");
        assert!(out.contains("cargo build"), "{out}");
        let long = format!("```sh\n{}\n```\n", "x".repeat(MAX_COPY + 1));
        let out = Markdown::new(long).copy_code().to_html();
        assert!(!out.contains("Copy code"), "{}", out.len());
    }

    #[test]
    fn hostile_text_neither_panics_nor_stalls() {
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
        for text in nasty {
            let out = Markdown::new(text).to_html();
            assert!(out.starts_with("<div"), "{}", out.len());
        }
    }
}
