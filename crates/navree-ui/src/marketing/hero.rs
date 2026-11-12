//! [`Hero`]: the top of a page, with its one `h1`.

use super::{container, eyebrow};
use crate::components::icon;
use crate::icons::Icon;
use crate::node::{Element, IntoNodes, Node, Uses, el, into_node};

/// What is drawn behind a [`Hero`] or a band: soft light from the theme's accent, a fine
/// grid, or nothing. Each is drawn by the stylesheet from the theme's colours, needs no
/// image and holds still for a reader who asks for less motion.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Backdrop {
    /// The page's own background.
    None,
    /// Light from the accent, from above. The default.
    #[default]
    Glow,
    /// A fine grid fading out from the middle, with the glow over it.
    Grid,
}

impl Backdrop {
    pub(super) const fn value(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Glow => Some("glow"),
            Self::Grid => Some("grid"),
        }
    }
}

/// The top of a page: a small line, the headline (the page's `h1`), a lead, the calls to
/// action, a row of short notes, and a picture beside them or under them: a screenshot in
/// a [`BrowserFrame`](super::BrowserFrame), a [`PhoneFrame`](super::PhoneFrame), a chart,
/// any node.
///
/// ```
/// use navree_ui::marketing::{Hero, accent};
/// use navree_ui::{Button, icons, render};
///
/// let hero = Hero::new(("Know your body ", accent("like never before.")))
///     .eyebrow("Health, read calmly")
///     .lead("Sleep, heart and steps from the phone you already carry, in one score a day.")
///     .action(Button::primary("Start free").href("/signup"))
///     .action(Button::outline("See the app").href("/#app"))
///     .note(icons::LOCK, "Your data stays yours");
/// let html = render(&hero.into());
/// assert!(html.contains(r#"<h1 data-slot="hero-title" id="hero-title">"#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Hero {
    el: Element,
    title: Node,
    eyebrow: Option<Node>,
    lead: Option<Node>,
    actions: Vec<Node>,
    notes: Vec<(Icon, Node)>,
    media: Option<Node>,
    centered: bool,
    backdrop: Backdrop,
}

impl Hero {
    /// A hero whose headline is `title`.
    pub fn new(title: impl IntoNodes) -> Self {
        Self {
            el: el("section").slot("hero").uses(Uses::MARKETING),
            title: Node::list(title),
            eyebrow: None,
            lead: None,
            actions: Vec::new(),
            notes: Vec::new(),
            media: None,
            centered: false,
            backdrop: Backdrop::Glow,
        }
    }

    /// The small line above the headline.
    pub fn eyebrow(mut self, text: impl Into<Node>) -> Self {
        self.eyebrow = Some(text.into());
        self
    }

    /// The paragraph under the headline: what the product does, for whom.
    pub fn lead(mut self, text: impl Into<Node>) -> Self {
        self.lead = Some(text.into());
        self
    }

    /// Adds a call to action: a [`Button`](crate::Button) with an `href`, the primary one
    /// first.
    pub fn action(mut self, action: impl IntoNodes) -> Self {
        action.push_into(&mut self.actions);
        self
    }

    /// Adds a short note under the actions, with an icon: "No card needed".
    pub fn note(mut self, glyph: Icon, text: impl Into<Node>) -> Self {
        self.notes.push((glyph, text.into()));
        self
    }

    /// The picture: beside the words from 64 rem up, under them on a narrower screen, or
    /// under them always when the hero is [`centered`](Self::centered).
    pub fn media(mut self, media: impl Into<Node>) -> Self {
        self.media = Some(media.into());
        self
    }

    /// Centres the words, with the picture under them.
    pub fn centered(mut self) -> Self {
        self.centered = true;
        self
    }

    /// What is drawn behind it.
    pub fn backdrop(mut self, backdrop: Backdrop) -> Self {
        self.backdrop = backdrop;
        self
    }

    fn into_node(self) -> Node {
        let layout = if self.centered || self.media.is_none() {
            "center"
        } else {
            "split"
        };
        let notes = (!self.notes.is_empty()).then(|| {
            el("ul").slot("hero-notes").child(
                self.notes
                    .into_iter()
                    .map(|(glyph, text)| el("li").children((icon(glyph), text)))
                    .collect::<Vec<_>>(),
            )
        });
        self.el
            .attr("aria-labelledby", "hero-title")
            .attr("data-layout", layout)
            .attr_opt("data-backdrop", self.backdrop.value())
            .child(
                container().children((
                    el("div").slot("hero-copy").children((
                        self.eyebrow.map(eyebrow),
                        el("h1")
                            .slot("hero-title")
                            .id("hero-title")
                            .child(self.title),
                        self.lead.map(|lead| el("p").slot("hero-lead").child(lead)),
                        (!self.actions.is_empty())
                            .then(|| el("div").slot("hero-actions").child(self.actions)),
                        notes,
                    )),
                    self.media
                        .map(|media| el("div").slot("hero-media").child(media)),
                )),
            )
            .into()
    }
}

into_node!(Hero);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Button, icons, render};

    #[test]
    fn a_hero_without_a_picture_is_centred_and_has_one_h1() {
        let html = render(
            &Hero::new("Calm numbers")
                .action(Button::primary("Start").href("/s"))
                .into(),
        );
        assert!(html.contains(r#"data-layout="center""#), "{html}");
        assert!(html.contains(r#"data-backdrop="glow""#), "{html}");
        assert_eq!(html.matches("<h1").count(), 1);
    }

    #[test]
    fn a_hero_with_a_picture_is_split_and_the_words_come_first() {
        let html = render(
            &Hero::new("Calm numbers")
                .note(icons::LOCK, "Private")
                .media(el("img").attr("src", "/a.png").attr("alt", "A"))
                .backdrop(Backdrop::None)
                .into(),
        );
        assert!(html.contains(r#"data-layout="split""#), "{html}");
        assert!(!html.contains("data-backdrop"), "{html}");
        assert!(html.find("hero-copy").unwrap() < html.find("hero-media").unwrap());
        assert!(html.contains("<li><svg"), "{html}");
    }
}
