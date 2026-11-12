//! The blocks a section is made of: logos, features, numbers, testimonials and the band
//! that asks the reader to act.

use std::borrow::Cow;

use super::hero::Backdrop;
use super::{container, eyebrow};
use crate::components::{icon, root_methods};
use crate::icons::{self, Icon};
use crate::node::{Element, IntoNodes, Node, Uses, el, into_node};

/// A row of the names or marks of those who use the product.
///
/// ```
/// use navree_ui::marketing::LogoCloud;
/// use navree_ui::render;
///
/// let cloud = LogoCloud::new()
///     .title("Used by clinics and teams across India")
///     .wordmark("Sutlej Clinics")
///     .wordmark("Doaba Fitness");
/// assert!(render(&cloud.into()).contains("Doaba Fitness"));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct LogoCloud {
    el: Element,
    title: Option<Node>,
    logos: Vec<Node>,
}

impl LogoCloud {
    /// An empty row.
    pub fn new() -> Self {
        Self {
            el: el("div").slot("logo-cloud").uses(Uses::MARKETING),
            title: None,
            logos: Vec::new(),
        }
    }

    /// The line above the logos.
    pub fn title(mut self, title: impl Into<Node>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Adds a logo: a [`Picture`](crate::Picture) with its alt text, or an `svg` with a
    /// label.
    pub fn logo(mut self, logo: impl Into<Node>) -> Self {
        self.logos.push(logo.into());
        self
    }

    /// Adds a name set as a wordmark, for a logo you have no file of.
    pub fn wordmark(mut self, name: impl Into<Cow<'static, str>>) -> Self {
        self.logos
            .push(el("span").slot("logo-wordmark").child(name.into()).into());
        self
    }

    fn into_node(self) -> Node {
        self.el
            .children((
                self.title
                    .map(|title| el("p").slot("logo-cloud-title").child(title)),
                el("ul").slot("logo-cloud-list").child(
                    self.logos
                        .into_iter()
                        .map(|logo| el("li").child(logo))
                        .collect::<Vec<_>>(),
                ),
            ))
            .into()
    }
}

impl Default for LogoCloud {
    fn default() -> Self {
        Self::new()
    }
}

/// One feature in a [`FeatureGrid`]: an icon, a title (`h3`), a line or two, and a
/// picture when it has one.
#[derive(Clone, Debug)]
#[must_use]
pub struct Feature {
    el: Element,
    title: Node,
    text: Option<Node>,
    glyph: Option<Icon>,
    media: Option<Node>,
    link: Option<Node>,
}

impl Feature {
    /// A feature titled `title`, described by `text`.
    pub fn new(title: impl Into<Node>, text: impl Into<Node>) -> Self {
        Self {
            el: el("li").slot("feature"),
            title: Node::list(title),
            text: Some(text.into()),
            glyph: None,
            media: None,
            link: None,
        }
    }

    /// Its icon.
    pub fn icon(mut self, glyph: Icon) -> Self {
        self.glyph = Some(glyph);
        self
    }

    /// A picture under the words: a chart, a screenshot, a part of the product.
    pub fn media(mut self, media: impl Into<Node>) -> Self {
        self.media = Some(media.into());
        self
    }

    /// A link under the words: "Read about sleep".
    pub fn link(mut self, label: impl Into<Node>, href: impl Into<crate::html::Href>) -> Self {
        self.link = Some(
            el("a")
                .slot("feature-link")
                .href(href)
                .children((label.into(), icon(icons::ARROW_RIGHT)))
                .into(),
        );
        self
    }

    /// Takes two columns of the grid, from 64 rem up.
    pub fn wide(mut self) -> Self {
        self.el = self.el.attr("data-span", "2");
        self
    }

    fn into_node(self) -> Node {
        self.el
            .flag("data-media", self.media.is_some())
            .children((
                self.glyph.map(|glyph| {
                    el("span")
                        .slot("feature-icon")
                        .attr("aria-hidden", "true")
                        .child(icon(glyph))
                }),
                el("h3").slot("feature-title").child(self.title),
                self.text
                    .map(|text| el("p").slot("feature-text").child(text)),
                self.link,
                self.media
                    .map(|media| el("div").slot("feature-media").child(media)),
            ))
            .into()
    }
}

/// Features side by side in cards: two, three or four columns on a wide screen, one on a
/// phone. A [`wide`](Feature::wide) feature with a picture makes it a bento grid.
///
/// ```
/// use navree_ui::marketing::{Feature, FeatureGrid};
/// use navree_ui::{icons, render};
///
/// let grid = FeatureGrid::new()
///     .cols(3)
///     .feature(Feature::new("Sleep stages", "Deep, REM and light sleep, night by night.").icon(icons::BED))
///     .feature(Feature::new("Resting heart", "Your baseline and what moves it.").icon(icons::HEART));
/// assert!(render(&grid.into()).contains(r#"data-cols="3""#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct FeatureGrid {
    el: Element,
    features: Vec<Node>,
}

impl FeatureGrid {
    /// An empty grid of three columns.
    pub fn new() -> Self {
        Self {
            el: el("ul").slot("feature-grid").uses(Uses::MARKETING),
            features: Vec::new(),
        }
    }

    /// Two, three or four columns on a wide screen.
    pub fn cols(mut self, columns: u8) -> Self {
        let value = match columns.clamp(2, 4) {
            2 => "2",
            4 => "4",
            _ => "3",
        };
        self.el = self.el.attr("data-cols", value);
        self
    }

    /// Adds a feature.
    pub fn feature(mut self, feature: Feature) -> Self {
        self.features.push(feature.into());
        self
    }

    /// Lays the features out without cards, on the section's own background.
    pub fn plain(mut self) -> Self {
        self.el = self.el.attr("data-variant", "plain");
        self
    }

    fn into_node(self) -> Node {
        self.el.child(self.features).into()
    }
}

impl Default for FeatureGrid {
    fn default() -> Self {
        Self::new()
    }
}

/// One feature told at length, beside its picture: a small line, a title (`h3`), a
/// paragraph, points with ticks and a link.
#[derive(Clone, Debug)]
#[must_use]
pub struct FeatureRow {
    el: Element,
    title: Node,
    eyebrow: Option<Node>,
    text: Vec<Node>,
    points: Vec<Node>,
    action: Option<Node>,
    media: Option<Node>,
}

impl FeatureRow {
    /// A row titled `title`.
    pub fn new(title: impl Into<Node>) -> Self {
        Self {
            el: el("li").slot("feature-row"),
            title: Node::list(title),
            eyebrow: None,
            text: Vec::new(),
            points: Vec::new(),
            action: None,
            media: None,
        }
    }

    /// The small line above the title.
    pub fn eyebrow(mut self, text: impl Into<Node>) -> Self {
        self.eyebrow = Some(text.into());
        self
    }

    /// Adds a paragraph.
    pub fn text(mut self, text: impl Into<Node>) -> Self {
        self.text.push(text.into());
        self
    }

    /// Adds a point, with a tick.
    pub fn point(mut self, text: impl Into<Node>) -> Self {
        self.points.push(text.into());
        self
    }

    /// A link or button under the words.
    pub fn action(mut self, action: impl Into<Node>) -> Self {
        self.action = Some(action.into());
        self
    }

    /// The picture beside the words.
    pub fn media(mut self, media: impl Into<Node>) -> Self {
        self.media = Some(media.into());
        self
    }

    fn into_node(self) -> Node {
        let points = (!self.points.is_empty()).then(|| {
            el("ul").slot("feature-points").child(
                self.points
                    .into_iter()
                    .map(|point| el("li").children((icon(icons::CHECK), point)))
                    .collect::<Vec<_>>(),
            )
        });
        self.el
            .children((
                el("div").slot("feature-row-copy").children((
                    self.eyebrow.map(eyebrow),
                    el("h3").slot("feature-row-title").child(self.title),
                    self.text
                        .into_iter()
                        .map(|text| el("p").slot("feature-row-text").child(text))
                        .collect::<Vec<_>>(),
                    points,
                    self.action
                        .map(|action| el("div").slot("feature-row-action").child(action)),
                )),
                self.media
                    .map(|media| el("div").slot("feature-row-media").child(media)),
            ))
            .into()
    }
}

/// Features told one after another, each beside its picture, the picture on the right,
/// then the left, in turn. On a phone each picture is under its words.
#[derive(Clone, Debug)]
#[must_use]
pub struct FeatureRows {
    el: Element,
    rows: Vec<Node>,
}

impl FeatureRows {
    /// No rows yet.
    pub fn new() -> Self {
        Self {
            el: el("ul").slot("feature-rows").uses(Uses::MARKETING),
            rows: Vec::new(),
        }
    }

    /// Adds a row.
    pub fn row(mut self, row: FeatureRow) -> Self {
        self.rows.push(row.into());
        self
    }

    /// Starts with the picture on the left.
    pub fn media_first(mut self) -> Self {
        self.el = self.el.flag("data-media-first", true);
        self
    }

    fn into_node(self) -> Node {
        self.el.child(self.rows).into()
    }
}

impl Default for FeatureRows {
    fn default() -> Self {
        Self::new()
    }
}

/// A band of a few large numbers, each with what it counts.
///
/// ```
/// use navree_ui::marketing::StatBand;
/// use navree_ui::render;
///
/// let band = StatBand::new()
///     .stat("1.2 lakh", "nights read last month")
///     .stat("20+", "measures from one phone");
/// let html = render(&band.into());
/// assert!(html.contains("<dt data-slot=\"stat-band-label\">nights read last month</dt>"));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct StatBand {
    el: Element,
    stats: Vec<(Node, Node, Option<Node>)>,
}

impl StatBand {
    /// No numbers yet.
    pub fn new() -> Self {
        Self {
            el: el("dl").slot("stat-band").uses(Uses::MARKETING),
            stats: Vec::new(),
        }
    }

    /// Adds `value`, labelled `label`.
    pub fn stat(mut self, value: impl Into<Node>, label: impl Into<Node>) -> Self {
        self.stats.push((value.into(), label.into(), None));
        self
    }

    /// Adds `value`, labelled `label`, with a line under it saying where it comes from.
    pub fn stat_with(
        mut self,
        value: impl Into<Node>,
        label: impl Into<Node>,
        detail: impl Into<Node>,
    ) -> Self {
        self.stats
            .push((value.into(), label.into(), Some(detail.into())));
        self
    }

    fn into_node(self) -> Node {
        self.el
            .child(
                self.stats
                    .into_iter()
                    .map(|(value, label, detail)| {
                        el("div").slot("stat-band-item").children((
                            el("dt").slot("stat-band-label").child(label),
                            el("dd").slot("stat-band-value").child(value),
                            detail.map(|detail| el("dd").slot("stat-band-detail").child(detail)),
                        ))
                    })
                    .collect::<Vec<_>>(),
            )
            .into()
    }
}

impl Default for StatBand {
    fn default() -> Self {
        Self::new()
    }
}

/// What one person said about the product, with their name and who they are.
#[derive(Clone, Debug)]
#[must_use]
pub struct Testimonial {
    el: Element,
    quote: Node,
    author: Option<Node>,
    role: Option<Node>,
    avatar: Option<Node>,
    rating: Option<u8>,
    rating_label: Cow<'static, str>,
}

impl Testimonial {
    /// The words `quote`, without quotation marks: the card draws them.
    pub fn new(quote: impl Into<Node>) -> Self {
        Self {
            el: el("li").slot("testimonial"),
            quote: quote.into(),
            author: None,
            role: None,
            avatar: None,
            rating: None,
            rating_label: Cow::Borrowed("out of 5"),
        }
    }

    /// Who said it.
    pub fn author(mut self, name: impl Into<Node>) -> Self {
        self.author = Some(name.into());
        self
    }

    /// Who they are: a role and a place.
    pub fn role(mut self, role: impl Into<Node>) -> Self {
        self.role = Some(role.into());
        self
    }

    /// Their picture: an [`Avatar`](crate::Avatar).
    pub fn avatar(mut self, avatar: impl Into<Node>) -> Self {
        self.avatar = Some(avatar.into());
        self
    }

    /// The stars they gave, out of five, and the words after the number for assistive
    /// technology (`out of 5`).
    pub fn rating(mut self, stars: u8) -> Self {
        self.rating = Some(stars.min(5));
        self
    }

    /// The words after the number of stars, in the page's language.
    pub fn rating_label(mut self, label: impl Into<Cow<'static, str>>) -> Self {
        self.rating_label = label.into();
        self
    }

    /// A larger card, two columns wide on a wide screen.
    pub fn featured(mut self) -> Self {
        self.el = self.el.flag("data-featured", true);
        self
    }

    fn into_node(self) -> Node {
        let stars = self.rating.map(|stars| {
            el("p").slot("testimonial-rating").children((
                el("span").attr("aria-hidden", "true").child(
                    (0..5)
                        .map(|at| {
                            el("span")
                                .slot("testimonial-star")
                                .flag("data-on", at < stars)
                                .child(icon(icons::STAR))
                        })
                        .collect::<Vec<_>>(),
                ),
                el("span")
                    .flag("data-ui-sr", true)
                    .child(format!("{stars} {}", self.rating_label)),
            ))
        });
        self.el
            .child(
                el("figure").slot("testimonial-card").children((
                    stars,
                    el("blockquote")
                        .slot("testimonial-quote")
                        .child(el("p").child(self.quote)),
                    el("figcaption").slot("testimonial-author").children((
                        self.avatar,
                        el("span").slot("testimonial-who").children((
                            self.author
                                .map(|name| el("span").slot("testimonial-name").child(name)),
                            self.role
                                .map(|role| el("span").slot("testimonial-role").child(role)),
                        )),
                    )),
                )),
            )
            .into()
    }
}

/// Testimonials in columns that fill from the top, as many as the width allows.
///
/// ```
/// use navree_ui::marketing::{Testimonial, Testimonials};
/// use navree_ui::render;
///
/// let wall = Testimonials::new().item(
///     Testimonial::new("I finally know why Mondays feel heavy: Sunday nights.")
///         .author("Simran Kaur Gill")
///         .role("Teacher, Ludhiana"),
/// );
/// assert!(render(&wall.into()).contains("<blockquote"));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Testimonials {
    el: Element,
    items: Vec<Node>,
}

impl Testimonials {
    /// No testimonials yet.
    pub fn new() -> Self {
        Self {
            el: el("ul").slot("testimonials").uses(Uses::MARKETING),
            items: Vec::new(),
        }
    }

    /// Adds one.
    pub fn item(mut self, testimonial: Testimonial) -> Self {
        self.items.push(testimonial.into());
        self
    }

    fn into_node(self) -> Node {
        self.el.child(self.items).into()
    }
}

impl Default for Testimonials {
    fn default() -> Self {
        Self::new()
    }
}

/// A band near the end of a page that asks the reader to act: a title (`h2`), a line and
/// the buttons, centred over soft light from the accent.
///
/// ```
/// use navree_ui::marketing::CtaBand;
/// use navree_ui::{Button, render};
///
/// let band = CtaBand::new("cta", "Your data is already there.")
///     .lead("Start reading it today.")
///     .action(Button::primary("Build my dashboard").href("/signup"));
/// assert!(render(&band.into()).contains(r#"aria-labelledby="cta-title""#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct CtaBand {
    el: Element,
    id: String,
    title: Node,
    lead: Option<Node>,
    actions: Vec<Node>,
    note: Option<Node>,
    backdrop: Backdrop,
}

impl CtaBand {
    /// A band whose `id` links name, titled `title`.
    pub fn new(id: impl Into<String>, title: impl IntoNodes) -> Self {
        Self {
            el: el("section").slot("cta-band").uses(Uses::MARKETING),
            id: id.into(),
            title: Node::list(title),
            lead: None,
            actions: Vec::new(),
            note: None,
            backdrop: Backdrop::Glow,
        }
    }

    /// The line under the title.
    pub fn lead(mut self, lead: impl Into<Node>) -> Self {
        self.lead = Some(lead.into());
        self
    }

    /// Adds a button, the primary one first.
    pub fn action(mut self, action: impl IntoNodes) -> Self {
        action.push_into(&mut self.actions);
        self
    }

    /// A small line under the buttons: "No card needed".
    pub fn note(mut self, note: impl Into<Node>) -> Self {
        self.note = Some(note.into());
        self
    }

    /// What is drawn behind it.
    pub fn backdrop(mut self, backdrop: Backdrop) -> Self {
        self.backdrop = backdrop;
        self
    }

    fn into_node(self) -> Node {
        let title = format!("{}-title", self.id);
        let backdrop = self.backdrop.value();
        self.el
            .id(self.id)
            .attr("aria-labelledby", title.clone())
            .child(
                container().child(
                    el("div")
                        .slot("cta-band-panel")
                        .attr_opt("data-backdrop", backdrop)
                        .children((
                            el("h2").slot("cta-band-title").id(title).child(self.title),
                            self.lead
                                .map(|lead| el("p").slot("cta-band-lead").child(lead)),
                            (!self.actions.is_empty())
                                .then(|| el("div").slot("cta-band-actions").child(self.actions)),
                            self.note
                                .map(|note| el("p").slot("cta-band-note").child(note)),
                        )),
                ),
            )
            .into()
    }
}

root_methods!(LogoCloud, FeatureGrid, FeatureRows, StatBand, Testimonials);
into_node!(
    LogoCloud,
    Feature,
    FeatureGrid,
    FeatureRow,
    FeatureRows,
    StatBand,
    Testimonial,
    Testimonials,
    CtaBand
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render;

    #[test]
    fn a_feature_has_its_title_as_a_third_level_heading() {
        let html = render(
            &FeatureGrid::new()
                .cols(9)
                .feature(Feature::new("Sleep", "Stages").icon(icons::BED).wide())
                .into(),
        );
        assert!(html.contains(r#"data-cols="4""#), "{html}");
        assert!(
            html.contains(r#"<h3 data-slot="feature-title">Sleep</h3>"#),
            "{html}"
        );
        assert!(html.contains(r#"data-span="2""#), "{html}");
        assert!(html.contains(r#"<span data-slot="feature-icon" aria-hidden="true">"#));
    }

    #[test]
    fn a_rating_is_said_in_words() {
        let html = render(
            &Testimonial::new("Good")
                .rating(9)
                .rating_label("में से 5")
                .into(),
        );
        assert_eq!(html.matches("data-on").count(), 5, "{html}");
        assert!(html.contains("5 में से 5"), "{html}");
    }

    #[test]
    fn a_stat_names_its_value_before_it() {
        let html = render(
            &StatBand::new()
                .stat_with("82", "Score", "Last night")
                .into(),
        );
        let label = html.find("stat-band-label").unwrap();
        let value = html.find("stat-band-value").unwrap();
        assert!(label < value, "{html}");
        assert!(html.contains("stat-band-detail"), "{html}");
    }

    #[test]
    fn feature_rows_carry_ticks_and_pictures() {
        let html = render(
            &FeatureRows::new()
                .media_first()
                .row(
                    FeatureRow::new("Heart")
                        .point("Resting rate")
                        .media(el("div").child("chart")),
                )
                .into(),
        );
        assert!(html.contains("data-media-first"), "{html}");
        assert!(html.contains("feature-points"), "{html}");
        assert!(html.find("feature-row-copy").unwrap() < html.find("feature-row-media").unwrap());
    }
}
