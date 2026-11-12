//! [`LegalPage`]: a privacy policy, terms or any long page of rules, with its contents.

use std::borrow::Cow;

use super::{container, eyebrow, slug};
use crate::node::{Element, IntoNodes, Node, Uses, el, into_node};

/// One numbered part of a [`LegalPage`]: its heading (`h2`), which the contents link to,
/// and its text.
#[derive(Clone, Debug)]
#[must_use]
pub struct LegalSection {
    id: String,
    title: Cow<'static, str>,
    body: Vec<Node>,
}

impl LegalSection {
    /// A part titled `title`, which links name with `#{id}`.
    pub fn new(id: impl Into<String>, title: impl Into<Cow<'static, str>>) -> Self {
        let title = title.into();
        let mut id: String = id.into();
        if id.is_empty() {
            id = slug(&title);
        }
        Self {
            id,
            title,
            body: Vec::new(),
        }
    }

    /// Adds a paragraph of plain text.
    pub fn text(mut self, text: impl Into<Cow<'static, str>>) -> Self {
        self.body.push(el("p").child(text.into()).into());
        self
    }

    /// Adds a list of points.
    pub fn points<T: Into<Node>>(mut self, points: impl IntoIterator<Item = T>) -> Self {
        self.body.push(
            el("ul")
                .child(
                    points
                        .into_iter()
                        .map(|point| el("li").child(point.into()))
                        .collect::<Vec<_>>(),
                )
                .into(),
        );
        self
    }

    /// Adds any content: a table, a [`Markdown`](crate::Markdown) block, a callout.
    pub fn body(mut self, body: impl IntoNodes) -> Self {
        body.push_into(&mut self.body);
        self
    }
}

/// A long page of rules: a privacy policy, terms of service, a refund policy. Its title
/// (`h1`), a short introduction and when it last changed, then its parts, with a list of
/// them that links to each: beside the text and held in view on a wide screen, above it
/// on a phone. Lines are kept to a length that reads well.
///
/// ```
/// use navree_ui::marketing::{LegalPage, LegalSection};
/// use navree_ui::render;
///
/// let page = LegalPage::new("Privacy policy")
///     .updated("Last updated 5 September 2026")
///     .section(LegalSection::new("collect", "1. What we collect").text("Your steps, sleep and heart rate, from your phone."))
///     .section(LegalSection::new("", "2. What we never do").text("We never sell your data."));
/// let html = render(&page.into());
/// assert!(html.contains(r##"<a href="#2-what-we-never-do">2. What we never do</a>"##));
/// assert!(html.contains(r#"<h2 data-slot="legal-title" id="collect">1. What we collect</h2>"#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct LegalPage {
    el: Element,
    title: Node,
    eyebrow: Option<Node>,
    intro: Option<Node>,
    updated: Option<Node>,
    sections: Vec<LegalSection>,
    contents_label: Cow<'static, str>,
}

impl LegalPage {
    /// A page titled `title`.
    pub fn new(title: impl IntoNodes) -> Self {
        Self {
            el: el("article").slot("legal").uses(Uses::MARKETING),
            title: Node::list(title),
            eyebrow: None,
            intro: None,
            updated: None,
            sections: Vec::new(),
            contents_label: Cow::Borrowed("On this page"),
        }
    }

    /// The small line above the title: "Legal".
    pub fn eyebrow(mut self, text: impl Into<Node>) -> Self {
        self.eyebrow = Some(text.into());
        self
    }

    /// A paragraph under the title saying what the page covers.
    pub fn intro(mut self, text: impl Into<Node>) -> Self {
        self.intro = Some(text.into());
        self
    }

    /// When it last changed, as the reader reads a date: "Last updated 5 September 2026".
    pub fn updated(mut self, text: impl Into<Node>) -> Self {
        self.updated = Some(text.into());
        self
    }

    /// Adds a part.
    pub fn section(mut self, section: LegalSection) -> Self {
        self.sections.push(section);
        self
    }

    /// The contents' name, in the page's language: "On this page".
    pub fn contents_label(mut self, label: impl Into<Cow<'static, str>>) -> Self {
        self.contents_label = label.into();
        self
    }

    fn into_node(self) -> Node {
        let contents = (!self.sections.is_empty()).then(|| {
            el("nav")
                .slot("legal-contents")
                .attr("aria-labelledby", "legal-contents-title")
                .children((
                    el("p")
                        .slot("legal-contents-title")
                        .id("legal-contents-title")
                        .child(self.contents_label),
                    el("ol").child(
                        self.sections
                            .iter()
                            .map(|section| {
                                el("li").child(
                                    el("a")
                                        .attr("href", format!("#{}", section.id))
                                        .child(section.title.clone()),
                                )
                            })
                            .collect::<Vec<_>>(),
                    ),
                ))
        });
        let parts: Vec<Element> = self
            .sections
            .into_iter()
            .map(|section| {
                el("section")
                    .slot("legal-section")
                    .attr("aria-labelledby", section.id.clone())
                    .children((
                        el("h2")
                            .slot("legal-title")
                            .id(section.id)
                            .child(section.title),
                        section.body,
                    ))
            })
            .collect();
        self.el
            .child(
                container().children((
                    el("header").slot("legal-head").children((
                        self.eyebrow.map(eyebrow),
                        el("h1").slot("legal-heading").child(self.title),
                        self.intro
                            .map(|intro| el("p").slot("legal-intro").child(intro)),
                        self.updated
                            .map(|updated| el("p").slot("legal-updated").child(updated)),
                    )),
                    el("div")
                        .slot("legal-layout")
                        .children((contents, el("div").slot("legal-body").child(parts))),
                )),
            )
            .into()
    }
}

into_node!(LegalPage);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render;

    #[test]
    fn the_contents_link_every_part_in_order() {
        let html = render(
            &LegalPage::new("Terms")
                .section(LegalSection::new("a", "One").points(["x", "y"]))
                .section(LegalSection::new("b", "Two").text("t"))
                .into(),
        );
        let first = html.find(r##"href="#a""##).unwrap();
        let second = html.find(r##"href="#b""##).unwrap();
        assert!(first < second, "{html}");
        assert!(html.contains("<ul><li>x</li><li>y</li></ul>"), "{html}");
        assert_eq!(html.matches("<h1").count(), 1);
        assert_eq!(html.matches("<h2").count(), 2);
    }

    #[test]
    fn a_page_with_no_parts_has_no_contents() {
        let html = render(&LegalPage::new("Contact").into());
        assert!(!html.contains("<nav"), "{html}");
    }
}
