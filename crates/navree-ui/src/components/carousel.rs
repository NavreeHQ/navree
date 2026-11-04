//! Carousel: slides in a row (or a column) that scroll and snap, with buttons, slide
//! pickers and rotation that stops for the reader. It follows the carousel pattern of
//! the WAI-ARIA Authoring Practices.
//!
//! The track is the browser's own scroller, so touch, the wheel and the keys scroll it
//! with no script, and each slide snaps into place. With no script the pickers, when
//! there are any, are links to the slides; the previous and next buttons wait for the
//! `carousel` module, which makes them work, keeps the pickers in step with the track,
//! and rotates the slides when asked to.

use std::time::Duration;

use super::{Button, ButtonVariant, Size, icon};
use crate::icons::{self, Icon};
use crate::node::{Node, Uses, el, into_node};

/// The fastest rotation: a slide stays at least this long, in milliseconds.
const FASTEST: u64 = 2_000;
/// The slowest rotation, in milliseconds.
const SLOWEST: u64 = 60_000;

/// The words a carousel says to assistive technology. `{n}` stands for a slide's
/// number and `{total}` for how many there are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CarouselLabels {
    /// The previous button.
    pub previous: String,
    /// The next button.
    pub next: String,
    /// The rotation button while the slides rotate.
    pub pause: String,
    /// The rotation button once it has been stopped.
    pub play: String,
    /// The name of the track that holds the slides.
    pub slides: String,
    /// The name of a slide that has no picker: "1 of 4".
    pub slide_of: String,
    /// The name of a picker that has no slide title to use: "Slide 1".
    pub picker: String,
    /// The name of the row of pickers.
    pub pickers: String,
}

impl Default for CarouselLabels {
    fn default() -> Self {
        Self {
            previous: "Previous slide".to_owned(),
            next: "Next slide".to_owned(),
            pause: "Pause automatic rotation".to_owned(),
            play: "Start automatic rotation".to_owned(),
            slides: "Slides".to_owned(),
            slide_of: "{n} of {total}".to_owned(),
            picker: "Slide {n}".to_owned(),
            pickers: "Choose a slide".to_owned(),
        }
    }
}

/// How tall a vertical carousel is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum CarouselHeight {
    /// 12rem.
    Short,
    /// 18rem: the default.
    #[default]
    Medium,
    /// 26rem.
    Tall,
}

impl CarouselHeight {
    const fn as_str(self) -> Option<&'static str> {
        match self {
            Self::Short => Some("short"),
            Self::Medium => None,
            Self::Tall => Some("tall"),
        }
    }
}

/// One slide of a [`Carousel`].
#[derive(Clone, Debug)]
#[must_use]
pub struct CarouselSlide {
    content: Node,
    title: Option<String>,
}

impl CarouselSlide {
    /// A slide holding `content`.
    pub fn new(content: impl Into<Node>) -> Self {
        Self {
            content: content.into(),
            title: None,
        }
    }

    /// Names the slide's picker, in place of "Slide 2".
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
}

/// Slides that scroll and snap.
///
/// ```
/// use navree_ui::{Carousel, CarouselSlide, Heading, render};
///
/// let offers = Carousel::new("offers", "Offers from Ludhiana")
///     .slide(CarouselSlide::new(Heading::h3("Phulkari dupatta")))
///     .slide(CarouselSlide::new(Heading::h3("Basmati rice, 5 kg")))
///     .pickers();
/// let html = render(&offers.into());
/// assert!(html.contains(r#"aria-roledescription="carousel""#));
/// assert!(html.contains(r#"role="tablist""#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Carousel {
    id: String,
    label: String,
    slides: Vec<CarouselSlide>,
    per_view: u8,
    vertical: bool,
    height: CarouselHeight,
    pickers: bool,
    looped: bool,
    autoplay: Option<Duration>,
    labels: CarouselLabels,
}

impl Carousel {
    /// A carousel named `label` for assistive technology. `id` ties its parts
    /// together and must be unique on the page.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            slides: Vec::new(),
            per_view: 1,
            vertical: false,
            height: CarouselHeight::Medium,
            pickers: false,
            looped: false,
            autoplay: None,
            labels: CarouselLabels::default(),
        }
    }

    /// Adds a slide.
    pub fn slide(mut self, slide: CarouselSlide) -> Self {
        self.slides.push(slide);
        self
    }

    /// Adds slides.
    pub fn slides(mut self, slides: impl IntoIterator<Item = CarouselSlide>) -> Self {
        self.slides.extend(slides);
        self
    }

    /// Shows `count` slides at once, 1 to 4. A narrow screen shows fewer.
    pub fn per_view(mut self, count: u8) -> Self {
        self.per_view = count.clamp(1, 4);
        self
    }

    /// Runs the slides from top to bottom instead of side by side.
    pub fn vertical(mut self) -> Self {
        self.vertical = true;
        self
    }

    /// Sets how tall a vertical carousel is.
    pub fn height(mut self, height: CarouselHeight) -> Self {
        self.height = height;
        self
    }

    /// Draws a picker for each slide under the track: tabs that go to their slide. They
    /// are links with no script. They need one slide at a time, so with
    /// [`per_view`](Self::per_view) above 1 there are none.
    pub fn pickers(mut self) -> Self {
        self.pickers = true;
        self
    }

    /// Makes the buttons go round: next on the last slide goes to the first.
    pub fn loop_around(mut self) -> Self {
        self.looped = true;
        self
    }

    /// Moves to the next slide every `every`, held between 2 and 60 seconds, once the
    /// `carousel` module has loaded. A pause button comes with it, and rotation also
    /// stops while the pointer or focus is on the carousel, and for a reader who asks
    /// their device for less motion. It goes round at the end.
    pub fn autoplay(mut self, every: Duration) -> Self {
        let millis = u64::try_from(every.as_millis()).unwrap_or(u64::MAX);
        self.autoplay = Some(Duration::from_millis(millis.clamp(FASTEST, SLOWEST)));
        self
    }

    /// Sets the words, for another language.
    pub fn labels(mut self, labels: CarouselLabels) -> Self {
        self.labels = labels;
        self
    }

    fn into_node(self) -> Node {
        let total = self.slides.len();
        if total == 0 {
            return Node::Empty;
        }
        let id = self.id;
        let labels = self.labels;
        let per_view = self.per_view.clamp(1, 4);
        let pickers = self.pickers && per_view == 1 && total > 1;
        let track_id = format!("{id}-track");
        let say = |template: &str, n: usize| {
            template
                .replace("{n}", &n.to_string())
                .replace("{total}", &total.to_string())
        };
        let titles: Vec<Option<String>> = self.slides.iter().map(|slide| slide.title.clone()).collect();

        let slides: Vec<Node> = self
            .slides
            .into_iter()
            .enumerate()
            .map(|(at, slide)| {
                let n = at + 1;
                let base = el("div")
                    .slot("carousel-slide")
                    .attr("id", format!("{id}-slide-{n}"))
                    .attr("aria-roledescription", "slide");
                let base = if pickers {
                    base.attr("role", "tabpanel")
                        .attr("aria-labelledby", format!("{id}-tab-{n}"))
                } else {
                    base.attr("role", "group")
                        .attr("aria-label", say(&labels.slide_of, n))
                };
                Node::from(base.child(slide.content))
            })
            .collect();

        let (before, after): (Icon, Icon) = if self.vertical {
            (icons::CHEVRON_UP, icons::CHEVRON_DOWN)
        } else {
            (icons::CHEVRON_LEFT, icons::CHEVRON_RIGHT)
        };
        let step = |glyph: Icon, label: &str, which: &'static str| -> Node {
            Button::icon_only(glyph, label.to_owned())
                .variant(ButtonVariant::Outline)
                .size(Size::Sm)
                .attr("data-carousel", which)
                .attr("aria-controls", track_id.clone())
                .into()
        };
        let previous = step(before, &labels.previous, "prev");
        let next = step(after, &labels.next, "next");

        let rotation = self.autoplay.map(|_| {
            el("button")
                .slot("button")
                .attr("data-variant", "outline")
                .attr("data-size", "sm")
                .flag("data-icon-only", true)
                .attr("data-carousel", "rotation")
                .attr("type", "button")
                .attr("aria-label", labels.pause.clone())
                .attr("data-pause", labels.pause.clone())
                .attr("data-play", labels.play.clone())
                .child((icon(icons::PAUSE), icon(icons::PLAY)))
        });

        let track = el("div")
            .slot("carousel-track")
            .attr("id", track_id.clone())
            .attr("role", "group")
            .attr("aria-label", labels.slides.clone())
            .attr("tabindex", 0_i64)
            .attr(
                "aria-live",
                if self.autoplay.is_some() {
                    "off"
                } else {
                    "polite"
                },
            )
            .children(slides);

        let tabs = pickers.then(|| {
            let items: Vec<Node> = titles
                .iter()
                .enumerate()
                .map(|(at, title)| {
                    let n = at + 1;
                    let name = title
                        .clone()
                        .unwrap_or_else(|| say(&labels.picker, n));
                    Node::from(
                        el("a")
                            .slot("carousel-picker")
                            .attr("role", "tab")
                            .attr("id", format!("{id}-tab-{n}"))
                            .attr("href", format!("#{id}-slide-{n}"))
                            .attr("aria-controls", format!("{id}-slide-{n}"))
                            .attr("aria-selected", if at == 0 { "true" } else { "false" })
                            .attr("aria-label", name),
                    )
                })
                .collect();
            el("div")
                .slot("carousel-pickers")
                .attr("role", "tablist")
                .attr("aria-label", labels.pickers.clone())
                .children(items)
        });

        el("section")
            .slot("carousel")
            .attr("id", id.clone())
            .attr("role", "region")
            .attr("aria-roledescription", "carousel")
            .attr("aria-label", self.label)
            .flag("data-ui-carousel", true)
            // A rotating carousel goes round at the end, so its buttons do too.
            .flag("data-loop", self.looped || self.autoplay.is_some())
            .attr_opt(
                "data-orientation",
                self.vertical.then_some("vertical"),
            )
            .attr_opt(
                "data-height",
                if self.vertical {
                    self.height.as_str()
                } else {
                    None
                },
            )
            .attr_opt(
                "data-per-view",
                (per_view > 1).then(|| i64::from(per_view)),
            )
            .attr_opt(
                "data-autoplay",
                self.autoplay
                    .map(|every| i64::try_from(every.as_millis()).unwrap_or(i64::MAX)),
            )
            .uses(Uses::CAROUSEL)
            .children((
                el("div")
                    .slot("carousel-viewport")
                    .children((rotation, previous, next, track)),
                tabs,
            ))
            .into()
    }
}

into_node!(Carousel);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::render;

    fn two() -> Carousel {
        Carousel::new("c", "Offers")
            .slide(CarouselSlide::new("Phulkari"))
            .slide(CarouselSlide::new("Jutti"))
    }

    #[test]
    fn the_slides_are_numbered_from_one() {
        let out = render(&two().into());
        assert!(out.contains(r#"id="c-slide-1""#) && out.contains(r#"id="c-slide-2""#));
        assert!(out.contains(r#"aria-label="2 of 2""#), "{out}");
    }

    #[test]
    fn rotation_is_held_to_its_limits() {
        for (asked, held) in [(0_u64, 2_000), (5_000, 5_000), (9_999_999, 60_000)] {
            let out = render(&two().autoplay(Duration::from_millis(asked)).into());
            assert!(out.contains(&format!(r#"data-autoplay="{held}""#)), "{out}");
        }
    }

    #[test]
    fn a_picker_needs_more_than_one_slide_and_one_in_view() {
        assert!(render(&two().pickers().into()).contains(r#"role="tab""#));
        assert!(!render(&two().pickers().per_view(2).into()).contains(r#"role="tab""#));
    }
}
