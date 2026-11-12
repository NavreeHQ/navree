//! [`Picture`]: a responsive image, in the formats a browser takes, that never moves the
//! page as it loads.

use super::root_methods;
use crate::html::Href;
use crate::node::{Element, Node, el, into_node};
use crate::theme::Mode;

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
/// `'self'` and `data:`. An [`AppImage`](crate::AppImage) the toolkit serves is one:
/// `Picture::new(&MARK, ..)`.
///
/// A screenshot of the product has a light and a dark version: [`dark`](Self::dark) gives
/// the dark one, which a reader whose device is dark gets (`prefers-color-scheme`), and
/// [`for_mode`](Self::for_mode) settles it by the page's own mode when the reader chose
/// light or dark on the site, so only the one they see is fetched:
///
/// ```
/// use navree_ui::theme::Mode;
/// use navree_ui::{Picture, render};
///
/// let shot = || {
///     Picture::new("/img/today-light.png", "Today: a score of 82", 1440, 900)
///         .dark(Picture::new("/img/today-dark.png", "", 1440, 900).webp([(1440, "/img/today-dark.webp")]))
/// };
/// let system = render(&shot().into());
/// assert!(system.contains(r#"<source media="(prefers-color-scheme: dark)" type="image/webp" srcset="/img/today-dark.webp 1440w""#));
/// assert!(system.contains(r#"<img src="/img/today-light.png""#));
/// let dark = render(&shot().for_mode(Mode::Dark).into());
/// assert!(dark.contains(r#"<img src="/img/today-dark.png""#) && !dark.contains("today-light"));
/// ```
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
    /// The dark version's files: its original, its sizes, its AVIF and its WebP.
    dark: Option<Box<DarkFiles>>,
    mode: Option<Mode>,
}

/// The files of a [`Picture`]'s dark version.
#[derive(Clone, Debug)]
struct DarkFiles {
    src: String,
    srcset: Vec<(u32, String)>,
    avif: Vec<(u32, String)>,
    webp: Vec<(u32, String)>,
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
            dark: None,
            mode: None,
        }
    }

    /// The image as it looks in dark mode: `dark`'s file, its sizes and its formats (its
    /// alt text and size are this picture's, which it must share). A device in dark mode
    /// gets it, through `prefers-color-scheme`, unless [`for_mode`](Self::for_mode) says
    /// which mode the page is in.
    pub fn dark(mut self, dark: Picture) -> Self {
        self.dark = Some(Box::new(DarkFiles {
            src: dark.src,
            srcset: dark.srcset,
            avif: dark.avif,
            webp: dark.webp,
        }));
        self
    }

    /// The mode the page is drawn in (its [`Appearance`](crate::theme::Appearance)'s): light
    /// or dark draws that version alone, so the reader's own choice on the site wins over
    /// the device's; [`Mode::System`] leaves it to the device. Without a
    /// [`dark`](Self::dark) version it changes nothing.
    pub fn for_mode(mut self, mode: Mode) -> Self {
        self.mode = Some(mode);
        self
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

    fn into_node(mut self) -> Node {
        // A page in light or dark draws that version alone.
        let mut dark = self.dark.take();
        match (self.mode, dark.take()) {
            (Some(Mode::Dark), Some(files)) => {
                let files = *files;
                self.src = files.src;
                self.srcset = files.srcset;
                self.avif = files.avif;
                self.webp = files.webp;
            }
            (Some(Mode::Light), Some(_)) => {}
            (_, files) => dark = files,
        }
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
        // The dark version first: a source in the media query, for each format and then the
        // original, which a browser takes before any source without one.
        let dark_sources: Vec<Element> = dark
            .as_deref()
            .map(|files| {
                let media = |kind: Option<&'static str>, srcset: String, sized: bool| {
                    el("source")
                        .attr("media", "(prefers-color-scheme: dark)")
                        .attr_opt("type", kind)
                        .attr("srcset", srcset)
                        .attr_opt("sizes", sized.then(|| sizes.clone()))
                        .attr("width", i64::from(self.width))
                        .attr("height", i64::from(self.height))
                };
                let original = if files.srcset.is_empty() {
                    candidates_of([(1, files.src.clone())])
                        .first()
                        .map(|_| media(None, files.src.clone(), false))
                } else {
                    Some(media(None, srcset_text(&files.srcset), true))
                };
                [
                    (!files.avif.is_empty())
                        .then(|| media(Some("image/avif"), srcset_text(&files.avif), true)),
                    (!files.webp.is_empty())
                        .then(|| media(Some("image/webp"), srcset_text(&files.webp), true)),
                    original,
                ]
                .into_iter()
                .flatten()
                .collect()
            })
            .unwrap_or_default();
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
                dark_sources,
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

    // A screenshot in both modes: the device's mode picks it, unless the page says its own.
    #[test]
    fn a_dark_version_follows_the_device_or_the_page() {
        let shot = || {
            Picture::new("/l.png", "Today", 10, 10)
                .webp([(10, "/l.webp")])
                .dark(
                    Picture::new("/d.png", "ignored", 99, 99)
                        .avif([(10, "/d.avif")])
                        .webp([(10, "/d.webp")]),
                )
        };
        let system = render(&shot().into());
        assert_eq!(
            system,
            concat!(
                r#"<picture data-slot="picture">"#,
                r#"<source media="(prefers-color-scheme: dark)" type="image/avif" srcset="/d.avif 10w" sizes="auto, 100vw" width="10" height="10">"#,
                r#"<source media="(prefers-color-scheme: dark)" type="image/webp" srcset="/d.webp 10w" sizes="auto, 100vw" width="10" height="10">"#,
                r#"<source media="(prefers-color-scheme: dark)" srcset="/d.png" width="10" height="10">"#,
                r#"<source type="image/webp" srcset="/l.webp 10w" sizes="auto, 100vw" width="10" height="10">"#,
                r#"<img src="/l.png" alt="Today" width="10" height="10" sizes="auto, 100vw" loading="lazy" decoding="async">"#,
                r#"</picture>"#
            )
        );
        assert_eq!(render(&shot().for_mode(Mode::System).into()), system);
        let light = render(&shot().for_mode(Mode::Light).into());
        assert!(
            !light.contains("/d.") && light.contains(r#"<img src="/l.png""#),
            "{light}"
        );
        let dark = render(&shot().for_mode(Mode::Dark).into());
        assert!(!dark.contains("/l.") && !dark.contains("media="), "{dark}");
        assert!(
            dark.contains(r#"<img src="/d.png" alt="Today" width="10" height="10""#),
            "{dark}"
        );
        // A dark file that isn't an address an image may hold is dropped.
        let bad = render(
            &Picture::new("/l.png", "x", 1, 1)
                .dark(Picture::new("javascript:alert(1)", "", 1, 1))
                .into(),
        );
        assert!(!bad.contains("javascript"), "{bad}");
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
