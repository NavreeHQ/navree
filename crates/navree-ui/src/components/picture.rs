//! [`Picture`]: a responsive image, in the formats a browser takes, that never moves the
//! page as it loads.

use super::root_methods;
use crate::html::Href;
use crate::node::{Element, Node, el, into_node};

/// A responsive image: the sizes it comes in (`srcset` and `sizes`), AVIF and WebP for
/// the browsers that read them with the original as the fallback (`<picture>`), its width
/// and height so the page keeps its room while it loads, and lazy loading below the fold.
///
/// Alt text is required: [`new`](Self::new) takes it, and an image that only decorates
/// says so with [`decorative`](Self::decorative), which writes an empty `alt`.
///
/// ```
/// use navree_ui::{Picture, render};
///
/// let hero = Picture::new("/img/today-1600.png", "Today's score: a ring at 82, with sleep, heart and steps under it", 1600, 1000)
///     .srcset([(800, "/img/today-800.png"), (1600, "/img/today-1600.png")])
///     .avif([(800, "/img/today-800.avif"), (1600, "/img/today-1600.avif")])
///     .webp([(800, "/img/today-800.webp"), (1600, "/img/today-1600.webp")])
///     .sizes("(min-width: 64rem) 40rem, 100vw")
///     .priority();
/// let html = render(&hero.into());
/// assert!(html.contains(r#"<source type="image/avif" srcset="/img/today-800.avif 800w, /img/today-1600.avif 1600w""#));
/// assert!(html.contains(r#"width="1600" height="1000""#));
/// assert!(html.contains(r#"loading="eager" fetchpriority="high""#));
/// ```
///
/// Images come from the page's own site: the content security policy's `img-src` is
/// `'self'` and `data:`.
#[derive(Clone, Debug)]
#[must_use]
pub struct Picture {
    el: Element,
    src: String,
    alt: String,
    width: u32,
    height: u32,
    srcset: Vec<(u32, String)>,
    avif: Vec<(u32, String)>,
    webp: Vec<(u32, String)>,
    sizes: Option<String>,
    priority: bool,
}

impl Picture {
    /// The image at `src`, `width` by `height` pixels as drawn at its largest, showing
    /// what `alt` says. Write what the image tells a reader who can't see it, not what it
    /// is a picture of.
    pub fn new(src: impl Into<String>, alt: impl Into<String>, width: u32, height: u32) -> Self {
        Self {
            el: el("picture").slot("picture"),
            src: src.into(),
            alt: alt.into(),
            width,
            height,
            srcset: Vec::new(),
            avif: Vec::new(),
            webp: Vec::new(),
            sizes: None,
            priority: false,
        }
    }

    /// An image that only decorates, which assistive technology passes over: its `alt`
    /// is empty.
    pub fn decorative(src: impl Into<String>, width: u32, height: u32) -> Self {
        Self::new(src, "", width, height)
    }

    /// The original format at other widths: each a width in pixels and its file.
    pub fn srcset<S: Into<String>>(
        mut self,
        candidates: impl IntoIterator<Item = (u32, S)>,
    ) -> Self {
        self.srcset = candidates_of(candidates);
        self
    }

    /// The image as AVIF, at the widths given, for the browsers that read it.
    pub fn avif<S: Into<String>>(mut self, candidates: impl IntoIterator<Item = (u32, S)>) -> Self {
        self.avif = candidates_of(candidates);
        self
    }

    /// The image as WebP, at the widths given, for the browsers that read it and not AVIF.
    pub fn webp<S: Into<String>>(mut self, candidates: impl IntoIterator<Item = (u32, S)>) -> Self {
        self.webp = candidates_of(candidates);
        self
    }

    /// How wide the image is drawn at each screen width, so the browser picks the
    /// smallest file that is sharp: `(min-width: 64rem) 40rem, 100vw`. Without it a lazy
    /// image is measured where it lands (`auto`), and an eager one is taken as the
    /// screen's width.
    pub fn sizes(mut self, sizes: impl Into<String>) -> Self {
        self.sizes = Some(sizes.into());
        self
    }

    /// Loads the image at once and first: for the one large image at the top of a page,
    /// its hero or its main screenshot. Every other image loads lazily, as it nears the
    /// screen.
    pub fn priority(mut self) -> Self {
        self.priority = true;
        self
    }

    /// Draws the image as wide as its container, at its own shape.
    pub fn fill(mut self) -> Self {
        self.el = self.el.flag("data-fill", true);
        self
    }

    fn into_node(self) -> Node {
        let sizes = self.sizes.clone().unwrap_or_else(|| {
            if self.priority {
                "100vw".to_owned()
            } else {
                "auto, 100vw".to_owned()
            }
        });
        let source = |kind: &'static str, set: &[(u32, String)]| {
            (!set.is_empty()).then(|| {
                el("source")
                    .attr("type", kind)
                    .attr("srcset", srcset_text(set))
                    .attr("sizes", sizes.clone())
                    .attr("width", i64::from(self.width))
                    .attr("height", i64::from(self.height))
            })
        };
        let any_set = !(self.srcset.is_empty() && self.avif.is_empty() && self.webp.is_empty());
        let img = el("img")
            .attr("src", self.src.clone())
            .attr("alt", self.alt.clone())
            .attr("width", i64::from(self.width))
            .attr("height", i64::from(self.height))
            .attr_opt(
                "srcset",
                (!self.srcset.is_empty()).then(|| srcset_text(&self.srcset)),
            )
            .attr_opt("sizes", any_set.then_some(sizes.clone()))
            .attr("loading", if self.priority { "eager" } else { "lazy" })
            .attr_opt("fetchpriority", self.priority.then_some("high"))
            .attr("decoding", if self.priority { "auto" } else { "async" });
        self.el
            .children((
                source("image/avif", &self.avif),
                source("image/webp", &self.webp),
                img,
            ))
            .into()
    }
}

/// The candidates whose files are safe in a `srcset`: a URL a link may hold, with no
/// space or comma, which would split it, and a width above zero.
fn candidates_of<S: Into<String>>(
    candidates: impl IntoIterator<Item = (u32, S)>,
) -> Vec<(u32, String)> {
    let mut out: Vec<(u32, String)> = candidates
        .into_iter()
        .map(|(width, url)| (width, url.into()))
        .filter(|(width, url)| {
            *width > 0
                && !url.is_empty()
                && !url.contains([' ', ',', '\t', '\n', '\r'])
                && Href::parse(url).is_ok_and(|href| href.as_str() == url)
        })
        .collect();
    out.sort_by_key(|(width, _)| *width);
    out.dedup_by_key(|(width, _)| *width);
    out
}

/// `a.png 480w, b.png 960w`.
fn srcset_text(set: &[(u32, String)]) -> String {
    set.iter()
        .map(|(width, url)| format!("{url} {width}w"))
        .collect::<Vec<_>>()
        .join(", ")
}

root_methods!(Picture);
into_node!(Picture);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render;

    #[test]
    fn a_lazy_image_keeps_its_room_and_is_measured_where_it_lands() {
        let html = render(&Picture::new("/a.png", "A chart", 800, 450).into());
        assert_eq!(
            html,
            r#"<picture data-slot="picture"><img src="/a.png" alt="A chart" width="800" height="450" loading="lazy" decoding="async"></picture>"#
        );
        let html = render(
            &Picture::new("/a.png", "A chart", 800, 450)
                .srcset([(800, "/a.png"), (400, "/a-400.png")])
                .into(),
        );
        assert!(
            html.contains(
                r#"srcset="/a-400.png 400w, /a.png 800w" sizes="auto, 100vw" loading="lazy""#
            ),
            "{html}"
        );
    }

    #[test]
    fn the_modern_formats_come_first_and_the_original_last() {
        let html = render(
            &Picture::new("/a.jpg", "A", 10, 10)
                .avif([(10, "/a.avif")])
                .webp([(10, "/a.webp")])
                .sizes("50vw")
                .into(),
        );
        let avif = html.find("image/avif").unwrap();
        let webp = html.find("image/webp").unwrap();
        let img = html.find("<img").unwrap();
        assert!(avif < webp && webp < img, "{html}");
        assert_eq!(html.matches(r#"sizes="50vw""#).count(), 3, "{html}");
    }

    #[test]
    fn a_hero_loads_first() {
        let html = render(&Picture::new("/h.png", "Hero", 1600, 900).priority().into());
        assert!(
            html.contains(r#"loading="eager" fetchpriority="high" decoding="auto""#),
            "{html}"
        );
    }

    #[test]
    fn a_decorative_image_has_an_empty_alt() {
        let html = render(&Picture::decorative("/d.png", 4, 4).into());
        assert!(html.contains(r#"alt="""#), "{html}");
    }

    #[test]
    fn hostile_files_and_text_are_dropped_or_escaped() {
        let html = render(
            &Picture::new("javascript:alert(1)", "\"><script>", 1, 1)
                .srcset([
                    (1, "/ok.png"),
                    (2, "javascript:alert(1)"),
                    (3, "/a.png 9000w, /evil.png"),
                    (0, "/zero.png"),
                    (4, "/b\".png"),
                ])
                .sizes("\"><x>")
                .into(),
        );
        assert!(!html.contains("javascript:"), "{html}");
        assert!(!html.contains("evil"), "{html}");
        assert!(!html.contains("zero"), "{html}");
        assert!(!html.contains("<script>"), "{html}");
        assert!(!html.contains("<x>"), "{html}");
        assert!(
            html.contains(r#"srcset="/ok.png 1w, /b&quot;.png 4w""#),
            "{html}"
        );
    }
}
