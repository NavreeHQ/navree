//! The blocks of a public website, drawn on the server: a page with its skip link and
//! landmarks ([`SitePage`]), a header whose menu works with no script ([`SiteHeader`]),
//! a hero, sections and their headings, logos, features, numbers, browser and phone
//! frames, testimonials, pricing with a monthly or yearly switch, questions and answers,
//! a call to action, a sign-up form, legal pages with their contents, and a footer with a
//! theme toggle.
//!
//! Every block is composable: each turns into a [`Node`] and takes nodes, so a block can
//! hold any component and any block can stand in a page of your own. None needs a
//! script: the menu and the questions are `<details>`, the pricing switch is a pair of
//! radio buttons, and the theme toggle is a form. A page of them links one stylesheet of
//! its own, `marketing`, after the toolkit's, and loads no script unless a part of it does
//! (a form, a chart's tooltip).
//!
//! Colours, corners and fonts come from the theme's tokens, so a site is right in light
//! and dark and in a theme of its own; draw it with
//! [`Appearance::for_reader`](crate::theme::Appearance::for_reader) so the reader's choice
//! from the [`ThemeToggle`] holds. Its head comes from [`View::seo`](crate::View::seo).
//!
//! ```
//! use navree_ui::marketing::*;
//! use navree_ui::{Button, View};
//!
//! let page = SitePage::new()
//!     .header(SiteHeader::new(Brand::new("Kesar", "/")).link("Pricing", "/pricing/"))
//!     .section(Hero::new("Know your body, calmly.").action(Button::primary("Start free").href("/signup")))
//!     .footer(SiteFooter::new(Brand::new("Kesar", "/")).legal("© 2026 Kesar Health"));
//! let html = View::new("Kesar").body(page).to_html();
//! assert!(html.contains(r##"<a data-slot="skip-link" href="#main">"##));
//! assert!(!html.contains("<script"));
//! ```

mod blocks;
mod faq;
mod footer;
mod frames;
mod header;
mod hero;
mod legal;
mod newsletter;
mod pricing;

pub use blocks::{
    CtaBand, Feature, FeatureGrid, FeatureRow, FeatureRows, LogoCloud, StatBand, Testimonial,
    Testimonials,
};
pub use faq::Faq;
pub use footer::{FooterColumn, SiteFooter, ThemeToggle};
pub use frames::{BrowserFrame, PhoneFrame};
pub use header::SiteHeader;
pub use hero::{Backdrop, Hero};
pub use legal::{LegalPage, LegalSection};
pub use newsletter::Newsletter;
pub use pricing::{Plan, Pricing};

use std::borrow::Cow;

use crate::components::icon;
use crate::html::Href;
use crate::icons::Icon;
use crate::node::{AttrValue, Element, IntoNodes, Node, Uses, el, into_node};

/// What a site's name and mark are: the link home in its header and footer.
///
/// ```
/// use navree_ui::marketing::Brand;
/// use navree_ui::{icons, render};
///
/// let html = render(&Brand::new("Kesar", "/").icon(icons::SUN).into());
/// assert!(html.contains(r#"<a data-slot="site-brand" href="/">"#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Brand {
    name: Cow<'static, str>,
    href: Href,
    mark: Option<Node>,
}

impl Brand {
    /// The site `name`, linking to `href`, its home page.
    pub fn new(name: impl Into<Cow<'static, str>>, href: impl Into<Href>) -> Self {
        Self {
            name: name.into(),
            href: href.into(),
            mark: None,
        }
    }

    /// Its mark: a [`Picture`](crate::Picture), an `svg` of your own, any node. It is
    /// hidden from assistive technology, since the name says the same.
    pub fn mark(mut self, mark: impl Into<Node>) -> Self {
        self.mark = Some(mark.into());
        self
    }

    /// Its mark as an icon, in the accent colour.
    pub fn icon(self, glyph: Icon) -> Self {
        self.mark(icon(glyph))
    }

    fn into_node(self) -> Node {
        el("a")
            .slot("site-brand")
            .href(self.href)
            .children((
                self.mark.map(|mark| {
                    el("span")
                        .slot("site-brand-mark")
                        .attr("aria-hidden", "true")
                        .child(mark)
                }),
                el("span").slot("site-brand-name").child(self.name),
            ))
            .into()
    }
}

/// A whole public page: a link that skips to the content, then the header, the `<main>`
/// landmark with the page's sections, and the footer. The skip link is the first thing a
/// keyboard reaches and shows itself when it has focus.
///
/// ```
/// use navree_ui::marketing::{Section, SitePage};
/// use navree_ui::render;
///
/// let html = render(&SitePage::new().section(Section::new("features")).into());
/// assert!(html.contains(r#"<main data-slot="site-main" id="main" tabindex="-1">"#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct SitePage {
    el: Element,
    skip: Cow<'static, str>,
    header: Option<Node>,
    sections: Vec<Node>,
    footer: Option<Node>,
}

impl SitePage {
    /// An empty page.
    pub fn new() -> Self {
        Self {
            el: el("div").slot("site").uses(Uses::MARKETING),
            skip: Cow::Borrowed("Skip to content"),
            header: None,
            sections: Vec::new(),
            footer: None,
        }
    }

    /// The skip link's words, in the page's language.
    pub fn skip_label(mut self, label: impl Into<Cow<'static, str>>) -> Self {
        self.skip = label.into();
        self
    }

    /// The header: a [`SiteHeader`] or one of your own.
    pub fn header(mut self, header: impl Into<Node>) -> Self {
        self.header = Some(header.into());
        self
    }

    /// Adds a part of the page's content: a [`Hero`], a [`Section`], a band.
    pub fn section(mut self, section: impl IntoNodes) -> Self {
        section.push_into(&mut self.sections);
        self
    }

    /// The footer: a [`SiteFooter`] or one of your own.
    pub fn footer(mut self, footer: impl Into<Node>) -> Self {
        self.footer = Some(footer.into());
        self
    }

    fn into_node(self) -> Node {
        self.el
            .children((
                el("a")
                    .slot("skip-link")
                    .attr("href", "#main")
                    .child(self.skip),
                self.header,
                el("main")
                    .slot("site-main")
                    .id("main")
                    .attr("tabindex", -1_i64)
                    .child(self.sections),
                self.footer,
            ))
            .into()
    }
}

impl Default for SitePage {
    fn default() -> Self {
        Self::new()
    }
}

/// How a section heading sits over what follows it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum HeadingAlign {
    /// From the start, the lead under the title. The default.
    #[default]
    Start,
    /// Centred.
    Center,
    /// The title in the wider part and the lead beside it, from 64 rem up: for a section
    /// whose content runs the full width under it.
    Split,
}

/// A section's heading: a small line above it, the title (`h2`) and a lead paragraph.
///
/// ```
/// use navree_ui::marketing::{Section, SectionHeading};
/// use navree_ui::render;
///
/// let html = render(&Section::new("features").heading(
///     SectionHeading::new("Five lenses on one body").eyebrow("What it measures").lead("All from the phone you already carry."),
/// ).into());
/// assert!(html.contains(r#"aria-labelledby="features-title""#));
/// assert!(html.contains(r#"<h2 data-slot="section-title" id="features-title">"#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct SectionHeading {
    title: Node,
    eyebrow: Option<Node>,
    lead: Option<Node>,
    align: HeadingAlign,
    action: Option<Node>,
}

impl SectionHeading {
    /// A heading titled `title`.
    pub fn new(title: impl IntoNodes) -> Self {
        Self {
            title: Node::list(title),
            eyebrow: None,
            lead: None,
            align: HeadingAlign::Start,
            action: None,
        }
    }

    /// The small line above the title, saying what the section is about.
    pub fn eyebrow(mut self, eyebrow: impl Into<Node>) -> Self {
        self.eyebrow = Some(eyebrow.into());
        self
    }

    /// The paragraph under the title.
    pub fn lead(mut self, lead: impl Into<Node>) -> Self {
        self.lead = Some(lead.into());
        self
    }

    /// A link or button beside the lead: "See every feature".
    pub fn action(mut self, action: impl Into<Node>) -> Self {
        self.action = Some(action.into());
        self
    }

    /// Centres it.
    pub fn center(mut self) -> Self {
        self.align = HeadingAlign::Center;
        self
    }

    /// Puts the lead beside the title on a wide screen.
    pub fn split(mut self) -> Self {
        self.align = HeadingAlign::Split;
        self
    }

    /// The heading, its title named `id`.
    pub(crate) fn render(self, id: Option<String>, level: &'static str) -> Element {
        let align = match self.align {
            HeadingAlign::Start => None,
            HeadingAlign::Center => Some("center"),
            HeadingAlign::Split => Some("split"),
        };
        el("div")
            .slot("section-heading")
            .attr_opt("data-align", align)
            .children((
                el("div").slot("section-heading-main").children((
                    self.eyebrow.map(eyebrow),
                    el(level)
                        .slot("section-title")
                        .attr_opt("id", id)
                        .child(self.title),
                )),
                (self.lead.is_some() || self.action.is_some()).then(|| {
                    el("div").slot("section-heading-aside").children((
                        self.lead
                            .map(|lead| el("p").slot("section-lead").child(lead)),
                        self.action
                            .map(|action| el("div").slot("section-action").child(action)),
                    ))
                }),
            ))
    }
}

/// The background of a [`Section`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum SectionTone {
    /// The page's own. The default.
    #[default]
    Plain,
    /// A step raised from the page, to set a section apart.
    Muted,
}

/// A part of a page: its heading, its content, in the page's width, with the space above
/// and below that every section shares. Its `id` is what a link to it names
/// (`/#pricing`), and its heading's title names the section for assistive technology.
#[derive(Clone, Debug)]
#[must_use]
pub struct Section {
    el: Element,
    id: String,
    heading: Option<SectionHeading>,
    content: Vec<Node>,
    narrow: bool,
}

impl Section {
    /// A section that links name with `#{id}`.
    pub fn new(id: impl Into<String>) -> Self {
        let id = id.into();
        Self {
            el: el("section").slot("site-section").uses(Uses::MARKETING),
            id,
            heading: None,
            content: Vec::new(),
            narrow: false,
        }
    }

    /// Its heading.
    pub fn heading(mut self, heading: SectionHeading) -> Self {
        self.heading = Some(heading);
        self
    }

    /// Adds to its content.
    pub fn child(mut self, content: impl IntoNodes) -> Self {
        content.push_into(&mut self.content);
        self
    }

    /// Sets its background.
    pub fn tone(mut self, tone: SectionTone) -> Self {
        if tone == SectionTone::Muted {
            self.el = self.el.attr("data-tone", "muted");
        }
        self
    }

    /// Draws it with less space above and below: for a row of logos or a band of numbers
    /// between two full sections.
    pub fn compact(mut self) -> Self {
        self.el = self.el.attr("data-size", "compact");
        self
    }

    /// Keeps its content to the width of a column of text, for prose and questions.
    pub fn narrow(mut self) -> Self {
        self.narrow = true;
        self
    }

    /// Sets an attribute on the section.
    pub fn attr(mut self, name: &'static str, value: impl Into<AttrValue>) -> Self {
        self.el = self.el.attr(name, value);
        self
    }

    fn into_node(self) -> Node {
        let title = format!("{}-title", self.id);
        let labelled = self.heading.is_some();
        self.el
            .id(self.id)
            .attr_opt("aria-labelledby", labelled.then(|| title.clone()))
            .child(
                container().flag("data-narrow", self.narrow).children((
                    self.heading
                        .map(|heading| heading.render(Some(title), "h2")),
                    (!self.content.is_empty())
                        .then(|| el("div").slot("section-body").child(self.content)),
                )),
            )
            .into()
    }
}

/// Words in a heading set in the accent colour: the half of a headline that carries it.
///
/// ```
/// use navree_ui::marketing::{Hero, accent};
///
/// let hero = Hero::new(("Know your body ", accent("like never before.")));
/// ```
pub fn accent(text: impl Into<Node>) -> Node {
    el("span").slot("accent").child(text.into()).into()
}

/// The small line above a heading.
pub(crate) fn eyebrow(text: Node) -> Element {
    el("p").slot("eyebrow").child(text)
}

/// The page's width with its gutters, as every block lays itself out.
pub(crate) fn container() -> Element {
    el("div").slot("site-container")
}

/// An id made from `text`: lower case letters, digits and dashes.
pub(crate) fn slug(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

into_node!(Brand, SitePage, Section);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render;

    #[test]
    fn a_page_has_its_landmarks_in_order() {
        let html = render(
            &SitePage::new()
                .skip_label("सामग्री पर जाएँ")
                .header(el("header").child("h"))
                .section(Section::new("a"))
                .footer(el("footer").child("f"))
                .into(),
        );
        let skip = html.find("skip-link").unwrap();
        let header = html.find("<header").unwrap();
        let main = html.find("<main").unwrap();
        let footer = html.find("<footer").unwrap();
        assert!(skip < header && header < main && main < footer, "{html}");
        assert!(html.contains("सामग्री पर जाएँ"), "{html}");
    }

    #[test]
    fn a_section_without_a_heading_names_nothing() {
        let html = render(&Section::new("plain").child("x").into());
        assert!(!html.contains("aria-labelledby"), "{html}");
        assert!(html.contains(r#"id="plain""#), "{html}");
    }

    #[test]
    fn slugs_are_plain() {
        assert_eq!(slug("1. What we collect"), "1-what-we-collect");
        assert_eq!(slug("  Data & you!  "), "data-you");
        assert_eq!(slug("पंजाबी"), "");
    }
}
