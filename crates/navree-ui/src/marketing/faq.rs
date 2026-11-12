//! [`Faq`]: questions that open to their answers, and the same questions as structured
//! data.

use crate::components::icon;
use crate::icons;
use crate::node::{Element, Node, Uses, el, into_node};
use crate::seo::FaqPage;

/// Questions and their answers. Each question is a heading (`h3`) in a `<details>`, so it
/// opens with no script and the browser gives it its keyboard and its open or closed
/// state; [`exclusive`](Self::exclusive) keeps one open at a time.
///
/// Answers are plain text, so the page and [`faq_page`](Self::faq_page)'s structured data
/// say the same words, as answer engines check. A blank line starts a new paragraph.
///
/// ```
/// use navree_ui::marketing::Faq;
/// use navree_ui::render;
///
/// let faq = Faq::new("faq")
///     .item("Do I need a smartwatch?", "No. Kesar reads the steps, sleep and heart rate your phone already records.")
///     .exclusive();
/// let html = render(&faq.clone().into());
/// assert!(html.contains(r#"<summary data-slot="faq-question"><h3 data-slot="faq-title">Do I need a smartwatch?</h3>"#));
/// let data = faq.faq_page("https://kesar.example/");
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Faq {
    el: Element,
    id: String,
    items: Vec<(String, String)>,
    exclusive: bool,
    open_first: bool,
}

impl Faq {
    /// Questions named `id`: with [`exclusive`](Self::exclusive), the group's name.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            el: el("div").slot("faq").uses(Uses::MARKETING),
            id: id.into(),
            items: Vec::new(),
            exclusive: false,
            open_first: false,
        }
    }

    /// Adds `question`, answered by `answer`, in plain text.
    pub fn item(mut self, question: impl Into<String>, answer: impl Into<String>) -> Self {
        self.items.push((question.into(), answer.into()));
        self
    }

    /// Keeps one answer open at a time: opening one closes the one before.
    pub fn exclusive(mut self) -> Self {
        self.exclusive = true;
        self
    }

    /// Opens the first answer.
    pub fn open_first(mut self) -> Self {
        self.open_first = true;
        self
    }

    /// The questions and answers.
    pub fn items(&self) -> &[(String, String)] {
        &self.items
    }

    /// The same questions as `FAQPage` structured data, for the page at `url`: add it to
    /// the page's [`JsonLd`](crate::seo::JsonLd).
    pub fn faq_page(&self, url: impl Into<String>) -> FaqPage {
        self.items
            .iter()
            .fold(FaqPage::new(url), |page, (question, answer)| {
                page.question(question.clone(), answer.clone())
            })
    }

    fn into_node(self) -> Node {
        let name = self.exclusive.then(|| self.id.clone());
        let open_first = self.open_first;
        let items: Vec<Element> = self
            .items
            .into_iter()
            .enumerate()
            .map(|(at, (question, answer))| {
                let paragraphs: Vec<Element> = answer
                    .split("\n\n")
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(|text| el("p").child(text.to_owned()))
                    .collect();
                el("details")
                    .slot("faq-item")
                    .attr_opt("name", name.clone())
                    .flag("open", open_first && at == 0)
                    .children((
                        el("summary").slot("faq-question").children((
                            el("h3").slot("faq-title").child(question),
                            el("span")
                                .slot("faq-sign")
                                .attr("aria-hidden", "true")
                                .child(icon(icons::PLUS)),
                        )),
                        el("div").slot("faq-answer").child(paragraphs),
                    ))
            })
            .collect();
        self.el.id(self.id).child(items).into()
    }
}

into_node!(Faq);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render;
    use crate::seo::Thing;

    fn faq() -> Faq {
        Faq::new("faq")
            .item(
                "Is it free?",
                "Yes, for one person.\n\nFamily plans cost ₹499 a month.",
            )
            .item(
                "<b>Is it safe?</b>",
                "Your data stays in India & is never sold.",
            )
    }

    #[test]
    fn the_page_and_the_data_say_the_same_words() {
        let html = render(&faq().open_first().into());
        assert!(
            html.contains("<p>Yes, for one person.</p><p>Family plans cost ₹499 a month.</p>"),
            "{html}"
        );
        assert!(html.contains("&lt;b&gt;Is it safe?&lt;/b&gt;"), "{html}");
        assert_eq!(html.matches(" open").count(), 1, "{html}");
        let data = Thing::from(faq().faq_page("https://kesar.example/")).to_value();
        assert_eq!(data["mainEntity"][1]["name"], "<b>Is it safe?</b>");
        assert_eq!(
            data["mainEntity"][0]["acceptedAnswer"]["text"],
            "Yes, for one person.\n\nFamily plans cost ₹499 a month."
        );
    }

    #[test]
    fn an_exclusive_group_shares_one_name() {
        let html = render(&faq().exclusive().into());
        assert_eq!(html.matches(r#"name="faq""#).count(), 2, "{html}");
        let free = render(&faq().into());
        assert!(!free.contains("name="), "{free}");
    }
}
