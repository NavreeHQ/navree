//! The blocks an app's dashboard is made of past the basic components: the info tip on a
//! "?" chip, chart-shaped loading skeletons, a paced breathing guide, a floating dock, the
//! day pager, a field with a value to copy, and the page of a sign-in or sign-up form. Their
//! styles are one sheet, [`Uses::APP_BLOCKS`], and none of them has a script of its own.

use std::borrow::Cow;

use super::overlays::{PopupAlign, Side};
use super::{ButtonVariant, Popover, Size, Tone, icon};
use crate::html::Href;
use crate::icons::{self, Icon};
use crate::node::{Element, IntoNodes, Node, Uses, el};

// ---------------------------------------------------------------------------------------
// InfoTip
// ---------------------------------------------------------------------------------------

/// Which mark an [`InfoTip`] wears.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum InfoMark {
    /// A "?" in a circle: the default, for "how do I read this".
    #[default]
    Question,
    /// An "i" in a circle: for "more about this".
    Info,
}

/// A small "?" or (i) chip that opens a note about the thing beside it: how to read a
/// chart, what a score is made of. It is a button that opens a popup (the browser's
/// `popover`), so a click, Enter or Space opens it, Esc and a click outside close it, and
/// it works for a keyboard and a screen reader as well as a mouse. The note can hold any
/// content: a title, text, a short list, a legend.
///
/// The chip is named "About" and what it is about, which a screen reader says in place of
/// the bare mark.
///
/// ```
/// use navree_ui::{InfoTip, Text, render};
///
/// let tip = InfoTip::new("autonomic-tip", "Autonomic balance")
///     .title("How to read this chart")
///     .body(Text::new(
///         "The balance compares your resting and your active nervous system over the night.",
///     ));
/// let html = render(&tip.into());
/// assert!(html.contains(r#"aria-label="About Autonomic balance""#));
/// assert!(html.contains(r#"popover="auto""#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct InfoTip {
    id: String,
    about: String,
    word: Cow<'static, str>,
    title: Option<String>,
    body: Vec<Node>,
    mark: InfoMark,
    side: Side,
    align: PopupAlign,
}

impl InfoTip {
    /// A tip with the id `id` about `about`, the name of what it explains.
    pub fn new(id: impl Into<String>, about: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            about: about.into(),
            word: Cow::Borrowed("About"),
            title: None,
            body: Vec::new(),
            mark: InfoMark::Question,
            side: Side::Bottom,
            align: PopupAlign::Center,
        }
    }

    /// The word before what it is about in the chip's name, "About" by default: write it
    /// in the page's language.
    pub fn word(mut self, word: impl Into<Cow<'static, str>>) -> Self {
        self.word = word.into();
        self
    }

    /// A heading for the note.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Adds to the note.
    pub fn body(mut self, body: impl IntoNodes) -> Self {
        body.push_into(&mut self.body);
        self
    }

    /// Which mark the chip wears.
    pub fn mark(mut self, mark: InfoMark) -> Self {
        self.mark = mark;
        self
    }

    /// The side it opens on.
    pub fn side(mut self, side: Side) -> Self {
        self.side = side;
        self
    }

    /// How it lines up along that side.
    pub fn align(mut self, align: PopupAlign) -> Self {
        self.align = align;
        self
    }

    fn into_node(self) -> Node {
        let glyph = match self.mark {
            InfoMark::Question => icons::CIRCLE_QUESTION_MARK,
            InfoMark::Info => icons::INFO,
        };
        let name = format!("{} {}", self.word, self.about);
        let title_id = format!("{}-title", self.id);
        let trigger = el("button")
            .slot("info-tip")
            .attr("type", "button")
            .attr("popovertarget", self.id.clone())
            .attr("aria-haspopup", "dialog")
            .attr("aria-label", name.clone())
            .uses(Uses::APP_BLOCKS)
            .child(icon(glyph));
        let labelled = self.title.is_some();
        let note = el("div")
            .slot("info-tip-content")
            .attr("id", self.id)
            .attr("popover", "auto")
            .attr("role", "dialog")
            .attr("tabindex", -1_i64)
            .flag("autofocus", true)
            .flag("data-ui-place", true)
            .attr("data-side", self.side.as_str())
            .attr_opt("data-align", self.align.as_str())
            .attr_opt("aria-labelledby", labelled.then(|| title_id.clone()))
            .attr_opt("aria-label", (!labelled).then_some(name))
            .uses(Uses::OVERLAY.with(Uses::APP_BLOCKS))
            .children((
                self.title.map(|title| {
                    el("p")
                        .slot("info-tip-title")
                        .attr("id", title_id)
                        .child(title)
                }),
                self.body,
            ));
        Node::list((trigger, note))
    }
}

crate::node::into_node!(InfoTip);

// ---------------------------------------------------------------------------------------
// ChartSkeleton
// ---------------------------------------------------------------------------------------

/// The deterministic heights a bar skeleton draws, in percent.
const SKELETON_BARS: [u32; 12] = [45, 65, 35, 75, 55, 40, 70, 50, 60, 45, 55, 65];

/// The shapes a [`ChartSkeleton`] takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SkeletonKind {
    Ring(u32),
    Gauge(u32),
    Line(u32),
    Bars(u32, u32),
    HeatMap(u32),
    Matrix(u32),
    Meter,
}

impl SkeletonKind {
    const fn name(self) -> &'static str {
        match self {
            Self::Ring(_) => "ring",
            Self::Gauge(_) => "gauge",
            Self::Line(_) => "line",
            Self::Bars(..) => "bars",
            Self::HeatMap(_) => "heat-map",
            Self::Matrix(_) => "matrix",
            Self::Meter => "meter",
        }
    }
}

/// A placeholder shaped like the chart that is coming, so a section that loads late doesn't
/// jump: a [`ring`](Self::ring), a [`gauge`](Self::gauge), a [`line`](Self::line), a row of
/// [`bars`](Self::bars), a calendar [`heat_map`](Self::heat_map), a [`matrix`](Self::matrix)
/// or a [`meter`](Self::meter). It pulses, and holds still when the reader asks for less
/// motion. For a screen reader it is one status that says what is loading; the shapes
/// are hidden. A chart's own `loading()` state keeps the chart's height; use a skeleton
/// where the chart isn't drawn yet at all, such as the first paint of a lazy [`Region`].
///
/// [`Region`]: crate::Region
///
/// ```
/// use navree_ui::{ChartSkeleton, render};
///
/// let ring = ChartSkeleton::ring(120).label("Loading recovery");
/// let year = ChartSkeleton::heat_map(26).label("Loading activity");
/// let html = render(&ring.into());
/// assert!(html.contains(r#"role="status""#) && html.contains("Loading recovery"));
/// # let _ = render(&year.into());
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct ChartSkeleton {
    kind: SkeletonKind,
    label: Cow<'static, str>,
}

impl ChartSkeleton {
    fn new(kind: SkeletonKind) -> Self {
        Self {
            kind,
            label: Cow::Borrowed("Loading"),
        }
    }

    /// A ring, `size` pixels across (48 to 320).
    pub fn ring(size: u32) -> Self {
        Self::new(SkeletonKind::Ring(size.clamp(48, 320)))
    }

    /// An open arc, `size` pixels across (48 to 320).
    pub fn gauge(size: u32) -> Self {
        Self::new(SkeletonKind::Gauge(size.clamp(48, 320)))
    }

    /// A line over faint gridlines, `height` pixels tall (48 to 480), the width of its place.
    pub fn line(height: u32) -> Self {
        Self::new(SkeletonKind::Line(height.clamp(48, 480)))
    }

    /// `count` bars (1 to 48), `height` pixels tall (48 to 480).
    pub fn bars(count: u32, height: u32) -> Self {
        Self::new(SkeletonKind::Bars(
            count.clamp(1, 48),
            height.clamp(48, 480),
        ))
    }

    /// A calendar of `weeks` columns (4 to 53) of seven days.
    pub fn heat_map(weeks: u32) -> Self {
        Self::new(SkeletonKind::HeatMap(weeks.clamp(4, 53)))
    }

    /// The lower triangle of a square matrix of `size` measures (2 to 12).
    pub fn matrix(size: u32) -> Self {
        Self::new(SkeletonKind::Matrix(size.clamp(2, 12)))
    }

    /// One bar split in parts, with a legend under it.
    pub fn meter() -> Self {
        Self::new(SkeletonKind::Meter)
    }

    /// What a screen reader hears: "Loading" by default, better "Loading sleep stages".
    pub fn label(mut self, label: impl Into<Cow<'static, str>>) -> Self {
        self.label = label.into();
        self
    }

    fn into_node(self) -> Node {
        let (view, attrs, shapes) = self.draw();
        let mut svg = el("svg")
            .attr("viewBox", view)
            .attr("aria-hidden", "true")
            .attr("focusable", "false");
        for (name, value) in attrs {
            svg = svg.attr(name, value);
        }
        el("div")
            .slot("chart-skeleton")
            .attr("data-kind", self.kind.name())
            .attr("role", "status")
            .uses(Uses::APP_BLOCKS)
            .children((
                el("span").attr("data-ui-sr", "").child(self.label),
                svg.child(shapes),
            ))
            .into()
    }

    /// The view box, the size attributes of the drawing and its shapes.
    #[allow(clippy::type_complexity)]
    fn draw(&self) -> (String, Vec<(&'static str, String)>, Vec<Element>) {
        let shape = |tag: &'static str, attrs: &[(&'static str, String)]| {
            attrs.iter().fold(el(tag), |element, (name, value)| {
                element.attr(name, value.clone())
            })
        };
        match self.kind {
            SkeletonKind::Ring(size) => (
                "0 0 100 100".into(),
                vec![("width", size.to_string()), ("height", size.to_string())],
                vec![
                    shape(
                        "circle",
                        &[
                            ("data-part", "track".into()),
                            ("cx", "50".into()),
                            ("cy", "50".into()),
                            ("r", "42".into()),
                        ],
                    ),
                    shape(
                        "circle",
                        &[
                            ("data-part", "arc".into()),
                            ("cx", "50".into()),
                            ("cy", "50".into()),
                            ("r", "42".into()),
                            ("pathLength", "100".into()),
                            ("stroke-dasharray", "28 72".into()),
                            ("transform", "rotate(-90 50 50)".into()),
                        ],
                    ),
                ],
            ),
            SkeletonKind::Gauge(size) => (
                "0 0 100 100".into(),
                vec![("width", size.to_string()), ("height", size.to_string())],
                vec![
                    shape(
                        "path",
                        &[
                            ("data-part", "track".into()),
                            ("d", "M 21.7 78.3 A 40 40 0 1 1 78.3 78.3".into()),
                        ],
                    ),
                    shape(
                        "path",
                        &[
                            ("data-part", "arc".into()),
                            ("d", "M 21.7 78.3 A 40 40 0 0 1 30 24".into()),
                        ],
                    ),
                ],
            ),
            SkeletonKind::Line(height) => {
                let mut shapes: Vec<Element> = (1..=3)
                    .map(|row| {
                        shape(
                            "line",
                            &[
                                ("data-part", "grid".into()),
                                ("x1", "0".into()),
                                ("x2", "300".into()),
                                ("y1", (row * 25).to_string()),
                                ("y2", (row * 25).to_string()),
                            ],
                        )
                    })
                    .collect();
                shapes.push(shape(
                    "path",
                    &[
                        ("data-part", "wash".into()),
                        (
                            "d",
                            "M0 70 C 30 40, 50 80, 85 55 S 140 30, 175 52 S 240 82, 300 38 V 100 H 0 Z"
                                .into(),
                        ),
                    ],
                ));
                shapes.push(shape(
                    "path",
                    &[
                        ("data-part", "line".into()),
                        (
                            "d",
                            "M0 70 C 30 40, 50 80, 85 55 S 140 30, 175 52 S 240 82, 300 38".into(),
                        ),
                        ("vector-effect", "non-scaling-stroke".into()),
                    ],
                ));
                (
                    "0 0 300 100".into(),
                    vec![
                        ("width", "100%".into()),
                        ("height", height.to_string()),
                        ("preserveAspectRatio", "none".into()),
                    ],
                    shapes,
                )
            }
            SkeletonKind::Bars(count, height) => {
                let step = 100.0 / f64::from(count);
                let shapes = (0..count)
                    .map(|at| {
                        let tall = f64::from(SKELETON_BARS[at as usize % SKELETON_BARS.len()]);
                        shape(
                            "rect",
                            &[
                                ("x", format!("{:.2}", f64::from(at) * step + step * 0.15)),
                                ("y", format!("{:.2}", 100.0 - tall)),
                                ("width", format!("{:.2}", step * 0.7)),
                                ("height", format!("{tall:.2}")),
                                ("rx", "1.2".into()),
                            ],
                        )
                    })
                    .collect();
                (
                    "0 0 100 100".into(),
                    vec![
                        ("width", "100%".into()),
                        ("height", height.to_string()),
                        ("preserveAspectRatio", "none".into()),
                    ],
                    shapes,
                )
            }
            SkeletonKind::HeatMap(weeks) => {
                let mut shapes = Vec::new();
                for week in 0..weeks {
                    for day in 0..7 {
                        shapes.push(shape(
                            "rect",
                            &[
                                ("x", (week * 14).to_string()),
                                ("y", (day * 14).to_string()),
                                ("width", "11".into()),
                                ("height", "11".into()),
                                ("rx", "2".into()),
                            ],
                        ));
                    }
                }
                (
                    format!("0 0 {} 98", weeks * 14 - 3),
                    vec![("width", "100%".into())],
                    shapes,
                )
            }
            SkeletonKind::Matrix(size) => {
                let cell = 100.0 / f64::from(size);
                let mut shapes = Vec::new();
                for row in 0..size {
                    for column in 0..=row {
                        shapes.push(shape(
                            "rect",
                            &[
                                (
                                    "x",
                                    format!("{:.2}", f64::from(column) * cell + cell * 0.06),
                                ),
                                ("y", format!("{:.2}", f64::from(row) * cell + cell * 0.06)),
                                ("width", format!("{:.2}", cell * 0.88)),
                                ("height", format!("{:.2}", cell * 0.88)),
                                ("rx", "1.5".into()),
                            ],
                        ));
                    }
                }
                ("0 0 100 100".into(), vec![("width", "100%".into())], shapes)
            }
            SkeletonKind::Meter => {
                let mut shapes = vec![shape(
                    "rect",
                    &[
                        ("data-part", "track".into()),
                        ("x", "0".into()),
                        ("y", "0".into()),
                        ("width", "300".into()),
                        ("height", "20".into()),
                        ("rx", "4".into()),
                    ],
                )];
                for (at, x) in [0, 90, 170, 240].into_iter().enumerate() {
                    shapes.push(shape(
                        "rect",
                        &[
                            ("x", x.to_string()),
                            ("y", "34".into()),
                            ("width", (50 + at * 6).to_string()),
                            ("height", "8".into()),
                            ("rx", "2".into()),
                        ],
                    ));
                }
                ("0 0 300 44".into(), vec![("width", "100%".into())], shapes)
            }
        }
    }
}

crate::node::into_node!(ChartSkeleton);

// ---------------------------------------------------------------------------------------
// BreathingGuide
// ---------------------------------------------------------------------------------------

/// The words a [`BreathingGuide`] writes.
#[derive(Clone, Debug)]
pub struct BreathingLabels {
    /// The switch that runs the guide.
    pub run: Cow<'static, str>,
    /// Shown while it isn't running.
    pub ready: Cow<'static, str>,
    /// The first phase.
    pub breathe_in: Cow<'static, str>,
    /// The second phase.
    pub hold: Cow<'static, str>,
    /// The third phase.
    pub breathe_out: Cow<'static, str>,
    /// The unit after a phase's length: "s".
    pub seconds: Cow<'static, str>,
}

impl Default for BreathingLabels {
    fn default() -> Self {
        Self {
            run: Cow::Borrowed("Guide"),
            ready: Cow::Borrowed("Ready"),
            breathe_in: Cow::Borrowed("Breathe in"),
            hold: Cow::Borrowed("Hold"),
            breathe_out: Cow::Borrowed("Breathe out"),
            seconds: Cow::Borrowed("s"),
        }
    }
}

/// A paced breathing guide for the 4-7-8 pattern: a circle that grows over four seconds
/// as you breathe in, rests for seven while you hold, and shrinks over eight as you breathe
/// out, with the phase named in its middle and the pattern written under it.
///
/// It runs in the browser from CSS alone, so it needs no script and works with JavaScript
/// off. It starts still, with a switch to run and pause it, and a reader who asks for less
/// motion gets no animation at all: the circle holds still, the switch is gone, and the three
/// steps stay written under it, which is the whole guide in words. The pattern is also
/// written in text for a screen reader, which can't follow a circle.
///
/// ```
/// use navree_ui::{BreathingGuide, render};
///
/// let guide = BreathingGuide::new("relax-guide", "4-7-8 breathing").size(220);
/// let html = render(&guide.into());
/// assert!(html.contains(r#"role="switch""#));
/// assert!(html.contains("Breathe in") && html.contains("Hold") && html.contains("Breathe out"));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct BreathingGuide {
    id: String,
    title: Cow<'static, str>,
    size: u32,
    tone: Tone,
    labels: BreathingLabels,
}

impl BreathingGuide {
    /// A guide with the id `id`, called `title`.
    pub fn new(id: impl Into<String>, title: impl Into<Cow<'static, str>>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            size: 200,
            tone: Tone::Neutral,
            labels: BreathingLabels::default(),
        }
    }

    /// The circle's size at its largest, in pixels: 120, 160, 200 (the default), 240, 280 or
    /// 320. Another number takes the nearest.
    pub fn size(mut self, px: u32) -> Self {
        self.size = (px.clamp(120, 320) + 20) / 40 * 40;
        self
    }

    /// Colours the circle (the accent by default).
    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }

    /// The words it writes.
    pub fn labels(mut self, labels: BreathingLabels) -> Self {
        self.labels = labels;
        self
    }

    fn into_node(self) -> Node {
        let words = &self.labels;
        let step = |phase: &'static str, name: &Cow<'static, str>, seconds: u32| {
            el("li").attr("data-phase", phase).children((
                el("b").child(seconds.to_string()),
                el("span").child(name.clone()),
                el("span").attr("data-ui-sr", "").child(format!(
                    "{seconds} {}",
                    if words.seconds == "s" {
                        "seconds"
                    } else {
                        &words.seconds
                    }
                )),
            ))
        };
        let run_id = format!("{}-run", self.id);
        el("section")
            .slot("breathing")
            .attr("id", self.id.clone())
            .attr("aria-label", self.title)
            .attr("data-size", self.size.to_string())
            .attr_opt("data-tone", self.tone.value())
            .uses(Uses::APP_BLOCKS)
            .children((
                el("div")
                    .slot("breathing-stage")
                    .attr("aria-hidden", "true")
                    .children((
                        el("span").slot("breathing-track"),
                        el("span").slot("breathing-orb"),
                        el("span").slot("breathing-label").children((
                            el("span")
                                .attr("data-phase", "idle")
                                .child(words.ready.clone()),
                            el("span")
                                .attr("data-phase", "in")
                                .child(words.breathe_in.clone()),
                            el("span")
                                .attr("data-phase", "hold")
                                .child(words.hold.clone()),
                            el("span")
                                .attr("data-phase", "out")
                                .child(words.breathe_out.clone()),
                        )),
                    )),
                el("div").slot("breathing-run").children((
                    el("input")
                        .slot("switch")
                        .attr("type", "checkbox")
                        .attr("role", "switch")
                        .attr("id", run_id.clone()),
                    el("label").attr("for", run_id).child(words.run.clone()),
                )),
                el("ol").slot("breathing-steps").children((
                    step("in", &words.breathe_in, 4),
                    step("hold", &words.hold, 7),
                    step("out", &words.breathe_out, 8),
                )),
            ))
            .into()
    }
}

crate::node::into_node!(BreathingGuide);

// ---------------------------------------------------------------------------------------
// Dock
// ---------------------------------------------------------------------------------------

/// One destination in a [`Dock`].
#[derive(Clone, Debug)]
#[must_use]
pub struct DockItem {
    label: Cow<'static, str>,
    href: Href,
    glyph: Option<Icon>,
    tone: Tone,
    current: bool,
    prominent: bool,
}

impl DockItem {
    /// A destination called `label` at `href`. Its label is the link's name, shown as a
    /// tooltip and read by a screen reader.
    pub fn new(label: impl Into<Cow<'static, str>>, href: impl Into<Href>) -> Self {
        Self {
            label: label.into(),
            href: href.into(),
            glyph: None,
            tone: Tone::Neutral,
            current: false,
            prominent: false,
        }
    }

    /// Its icon.
    pub fn icon(mut self, glyph: Icon) -> Self {
        self.glyph = Some(glyph);
        self
    }

    /// The colour its icon takes when it is current.
    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }

    /// Marks it as the page the reader is on.
    pub fn current(mut self) -> Self {
        self.current = true;
        self
    }

    /// Draws it larger, between the others: the home destination.
    pub fn prominent(mut self) -> Self {
        self.prominent = true;
        self
    }
}

/// A floating row of icon links, pinned bottom centre: the main places of an app on a phone,
/// or beside a content column on a wide screen. The current one is marked with
/// `aria-current="page"` and lit in its tone. Each icon is named by its label for a screen
/// reader and shown as a tooltip; the page keeps room for it with a bottom padding of its own.
///
/// ```
/// use navree_ui::{Dock, DockItem, Tone, icons, render};
///
/// let dock = Dock::new("Sections")
///     .item(DockItem::new("Sleep", "/sleep").icon(icons::MOON).tone(Tone::named("violet")))
///     .item(DockItem::new("Home", "/").icon(icons::HOUSE).prominent().current())
///     .item(DockItem::new("Fitness", "/fitness").icon(icons::ACTIVITY).tone(Tone::Success));
/// let html = render(&dock.into());
/// assert!(html.contains(r#"aria-current="page""#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Dock {
    label: Cow<'static, str>,
    items: Vec<DockItem>,
    below: Option<crate::charts::Screen>,
}

impl Dock {
    /// A dock whose navigation is called `label` to a screen reader. It shows at every
    /// width until [`below`](Self::below) says otherwise.
    pub fn new(label: impl Into<Cow<'static, str>>) -> Self {
        Self {
            label: label.into(),
            items: Vec::new(),
            below: None,
        }
    }

    /// Shows the dock only below `screen`: on phones, when the app's top bar holds the same
    /// places from [`Screen::Md`](crate::charts::Screen::Md) up. From that width on it is
    /// not drawn at all, so it is neither seen nor read twice.
    ///
    /// ```
    /// use navree_ui::charts::Screen;
    /// use navree_ui::{Dock, DockItem, render};
    ///
    /// let dock = Dock::new("Sections").below(Screen::Md).item(DockItem::new("Home", "/"));
    /// assert!(render(&dock.into()).contains(r#"data-below="md""#));
    /// ```
    pub fn below(mut self, screen: crate::charts::Screen) -> Self {
        self.below = Some(screen);
        self
    }

    /// Adds a destination.
    pub fn item(mut self, item: DockItem) -> Self {
        self.items.push(item);
        self
    }

    fn into_node(self) -> Node {
        let below = self.below.map(|screen| match screen {
            crate::charts::Screen::Sm => "sm",
            crate::charts::Screen::Md => "md",
            crate::charts::Screen::Lg => "lg",
        });
        el("nav")
            .slot("dock")
            .attr("aria-label", self.label)
            .attr_opt("data-below", below)
            .uses(Uses::APP_BLOCKS)
            .children(
                self.items
                    .into_iter()
                    .map(|item| {
                        el("a")
                            .slot("dock-item")
                            .attr("href", item.href)
                            .attr("title", item.label.clone())
                            .attr_opt("aria-current", item.current.then_some("page"))
                            .attr_opt("data-tone", item.tone.value())
                            .flag("data-prominent", item.prominent)
                            .children((
                                item.glyph.map(icon),
                                el("span").attr("data-ui-sr", "").child(item.label),
                            ))
                    })
                    .collect::<Vec<_>>(),
            )
            .into()
    }
}

crate::node::into_node!(Dock);

// ---------------------------------------------------------------------------------------
// CopyField
// ---------------------------------------------------------------------------------------

/// A value to take elsewhere (an invite link, a key, a code to type on another device), in
/// a read-only field with a Copy button beside it.
///
/// The button copies the value and turns to "Copied" for a moment, and a screen reader hears
/// "Copied". Focusing the field selects the whole value, so a keyboard copies it with one
/// shortcut. With JavaScript off the button is not drawn (it could do nothing) and the field
/// stays: read-only, selectable and copied by hand.
///
/// ```
/// use navree_ui::{CopyField, render};
///
/// let html = render(
///     &CopyField::new("invite", "Invite link", "https://kesar.example/join/8k2m")
///         .help("Anyone with the link can ask to join.")
///         .into(),
/// );
/// assert!(html.contains(r#"readonly"#));
/// assert!(html.contains(r#"value="https://kesar.example/join/8k2m""#));
/// assert!(html.contains("data-script-only"));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct CopyField {
    id: Cow<'static, str>,
    label: Cow<'static, str>,
    value: String,
    help: Option<Cow<'static, str>>,
    copy: Cow<'static, str>,
    copied: Cow<'static, str>,
}

impl CopyField {
    /// The field `id`, called `label`, holding `value`.
    pub fn new(
        id: impl Into<Cow<'static, str>>,
        label: impl Into<Cow<'static, str>>,
        value: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            value: value.into(),
            help: None,
            copy: Cow::Borrowed("Copy"),
            copied: Cow::Borrowed("Copied"),
        }
    }

    /// A line under the field: what the value is for.
    pub fn help(mut self, text: impl Into<Cow<'static, str>>) -> Self {
        self.help = Some(text.into());
        self
    }

    /// The button's words, in the page's language: "Copy" and "Copied" by default.
    pub fn labels(
        mut self,
        copy: impl Into<Cow<'static, str>>,
        copied: impl Into<Cow<'static, str>>,
    ) -> Self {
        self.copy = copy.into();
        self.copied = copied.into();
        self
    }

    fn into_node(self) -> Node {
        let help_id = format!("{}-help", self.id);
        let button = el("button")
            .slot("copy-field-button")
            .attr("type", "button")
            .flag("data-script-only", true)
            .attr("aria-describedby", self.id.clone())
            .on_click_one(crate::cmd::Cmd::copy(self.value.clone()))
            .uses(Uses::CORE)
            .children((
                icon(icons::COPY),
                el("span").slot("copy-field-copy").child(self.copy),
                el("span").slot("copy-field-copied").child(self.copied),
            ));
        el("div")
            .slot("copy-field")
            .uses(Uses::APP_BLOCKS)
            .children((
                el("label")
                    .slot("label")
                    .attr("for", self.id.clone())
                    .child(self.label),
                el("div").slot("copy-field-row").children((
                    el("input")
                        .slot("input")
                        .attr("id", self.id.clone())
                        .attr("type", "text")
                        .attr("value", self.value)
                        .flag("readonly", true)
                        .attr("spellcheck", "false")
                        .attr("autocomplete", "off")
                        .flag("data-ui-select", true)
                        .attr_opt(
                            "aria-describedby",
                            self.help.is_some().then(|| help_id.clone()),
                        ),
                    button,
                )),
                self.help.map(|help| {
                    el("p")
                        .slot("field-description")
                        .attr("id", help_id)
                        .child(help)
                }),
            ))
            .into()
    }
}

crate::node::into_node!(CopyField);

// ---------------------------------------------------------------------------------------
// AuthLayout
// ---------------------------------------------------------------------------------------

/// The page of one form a guest fills in (signing in, signing up, a new password): the form
/// centred in the window, a panel beside it on wide screens for the app's own picture and
/// words, and a corner at the top for a theme menu. navree-auth's `AuthPages` draws its
/// pages in it.
///
/// The panel shows from 960 px up; on a phone the form stands alone, first. The main part is
/// the page's `<main>`, so a skip link and a screen reader's landmarks find the form.
///
/// ```
/// use navree_ui::{AuthLayout, el, render};
///
/// let page = AuthLayout::new(el("form").child("…"))
///     .aside(el("p").child("Sleep, heart and steps, scored every day."))
///     .corner(el("span").child("menu"));
/// let html = render(&page.into());
/// assert!(html.contains(r#"<div data-slot="auth-layout" data-aside>"#));
/// assert!(html.contains(r#"<main data-slot="auth-main" id="main">"#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct AuthLayout {
    body: Node,
    aside: Option<Node>,
    aside_label: Cow<'static, str>,
    corner: Option<Node>,
}

impl AuthLayout {
    /// The page around `body`: the heading and the form's card.
    pub fn new(body: impl Into<Node>) -> Self {
        Self {
            body: body.into(),
            aside: None,
            aside_label: Cow::Borrowed("About"),
            corner: None,
        }
    }

    /// The panel beside the form on wide screens: the app's mark, a line about it, a picture.
    pub fn aside(mut self, aside: impl Into<Node>) -> Self {
        self.aside = Some(aside.into());
        self
    }

    /// What the panel is called to a screen reader ("About" by default): "About Kesar".
    pub fn aside_label(mut self, label: impl Into<Cow<'static, str>>) -> Self {
        self.aside_label = label.into();
        self
    }

    /// What stands in the top corner, over the form: a theme menu.
    pub fn corner(mut self, corner: impl Into<Node>) -> Self {
        self.corner = Some(corner.into());
        self
    }

    fn into_node(self) -> Node {
        let has_aside = self.aside.is_some();
        el("div")
            .slot("auth-layout")
            .flag("data-aside", has_aside)
            .uses(Uses::APP_BLOCKS)
            .children((
                self.aside.map(|aside| {
                    el("aside")
                        .slot("auth-aside")
                        .attr("aria-label", self.aside_label)
                        .child(aside)
                }),
                el("main").slot("auth-main").attr("id", "main").children((
                    self.corner
                        .map(|corner| el("div").slot("auth-corner").child(corner)),
                    el("div").slot("auth-body").child(self.body),
                )),
            ))
            .into()
    }
}

crate::node::into_node!(AuthLayout);

// ---------------------------------------------------------------------------------------
// DayPager
// ---------------------------------------------------------------------------------------

/// Steps through days: a link to the day before, the day shown (which can open a calendar),
/// and a link to the day after. A day with nothing to step to shows the arrow dimmed and
/// unlinked, so the pager never changes width. Both arrows are links, so stepping works with
/// JavaScript off and a step is a page of its own in the history.
///
/// The middle takes a small graphic before the date, such as the day's three scores as
/// [`Rings`](crate::charts::Rings) in their `inline` form, and a picker (any content, a
/// [`Calendar`](crate::dates::Calendar)) in a popup behind the date.
///
/// ```
/// use navree_ui::charts::{RingValue, Rings};
/// use navree_ui::{DayPager, render};
///
/// let scores = Rings::new("Scores for today")
///     .ring(RingValue::new("Movement", 35))
///     .ring(RingValue::new("Recovery", 72))
///     .ring(RingValue::new("Sleep", 78))
///     .size(28)
///     .inline();
/// let pager = DayPager::new("day", "Wed, 11 Nov")
///     .previous("/?date=2026-11-10")
///     .scores(scores);
/// let html = render(&pager.into());
/// assert!(html.contains("Previous day") && html.contains(r#"aria-disabled="true""#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct DayPager {
    id: String,
    day: Node,
    previous: Option<Href>,
    next: Option<Href>,
    scores: Option<Node>,
    picker: Vec<Node>,
    words: [Cow<'static, str>; 3],
}

impl DayPager {
    /// A pager with the id `id` showing `day`, written as you like it ("Today", "Wed, 11 Nov").
    pub fn new(id: impl Into<String>, day: impl Into<Node>) -> Self {
        Self {
            id: id.into(),
            day: day.into(),
            previous: None,
            next: None,
            scores: None,
            picker: Vec::new(),
            words: [
                Cow::Borrowed("Previous day"),
                Cow::Borrowed("Next day"),
                Cow::Borrowed("Choose a day"),
            ],
        }
    }

    /// Where the day before is.
    pub fn previous(mut self, href: impl Into<Href>) -> Self {
        self.previous = Some(href.into());
        self
    }

    /// Where the day after is.
    pub fn next(mut self, href: impl Into<Href>) -> Self {
        self.next = Some(href.into());
        self
    }

    /// A small graphic before the date: the day's scores.
    pub fn scores(mut self, scores: impl Into<Node>) -> Self {
        self.scores = Some(scores.into());
        self
    }

    /// Opens `picker` in a popup when the date is pressed.
    pub fn picker(mut self, picker: impl IntoNodes) -> Self {
        picker.push_into(&mut self.picker);
        self
    }

    /// The words: the arrows' names and the picker's.
    pub fn words(
        mut self,
        previous: impl Into<Cow<'static, str>>,
        next: impl Into<Cow<'static, str>>,
        choose: impl Into<Cow<'static, str>>,
    ) -> Self {
        self.words = [previous.into(), next.into(), choose.into()];
        self
    }

    fn into_node(self) -> Node {
        let [previous_word, next_word, choose_word] = self.words;
        let arrow =
            |slot: &'static str, href: Option<Href>, word: Cow<'static, str>, glyph: Icon| {
                match href {
                    Some(href) => el("a")
                        .slot(slot)
                        .attr("href", href)
                        .attr("aria-label", word)
                        .child(icon(glyph)),
                    None => el("span")
                        .slot(slot)
                        .attr("role", "link")
                        .attr("aria-label", word)
                        .attr("aria-disabled", "true")
                        .child(icon(glyph)),
                }
            };
        let day = el("span").slot("day-pager-day").child(self.day);
        let middle: Node = if self.picker.is_empty() {
            el("span")
                .slot("day-pager-face")
                .children((self.scores, day))
                .into()
        } else {
            Popover::new(
                format!("{}-picker", self.id),
                Node::list((self.scores, day, icon(icons::CALENDAR))),
            )
            .variant(ButtonVariant::Ghost)
            .size(Size::Sm)
            .label(choose_word)
            .body(self.picker)
            .into()
        };
        el("div")
            .slot("day-pager")
            .attr("id", self.id)
            .attr("role", "group")
            .uses(Uses::APP_BLOCKS)
            .children((
                arrow(
                    "day-pager-previous",
                    self.previous,
                    previous_word,
                    icons::CHEVRON_LEFT,
                ),
                middle,
                arrow("day-pager-next", self.next, next_word, icons::CHEVRON_RIGHT),
            ))
            .into()
    }
}

crate::node::into_node!(DayPager);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render;

    fn html(node: impl Into<Node>) -> String {
        render(&node.into())
    }

    #[test]
    fn a_plain_stat_is_the_markup_it_always_was_and_loads_no_extra_styles() {
        use super::super::{Stat, Trend};
        let out = html(Stat::new("Steps", "9,412").change(Trend::new(4.5)));
        assert!(
            out.starts_with(r#"<div data-slot="stat"><span data-slot="stat-label">Steps</span><span data-slot="stat-value">9,412</span><span data-slot="stat-trend"><span data-slot="trend""#),
            "{out}"
        );
        let uses = crate::measure(&Stat::new("Steps", "9,412").trend("+4%", true).into()).uses;
        assert!(!uses.has(Uses::APP_BLOCKS));
    }

    #[test]
    fn a_tile_carries_an_icon_a_unit_a_status_a_caption_an_aside_and_a_link() {
        use super::super::{Badge, Stat, Trend};
        let out = html(
            Stat::new("Resting heart rate", "58")
                .icon(icons::HEART_PULSE)
                .tone(Tone::Danger)
                .unit("bpm")
                .status(Badge::new("Athletic").tone(Tone::Success))
                .change(Trend::new(-3.3).up_is_bad())
                .caption("vs last week")
                .aside("RING")
                .chart("SPARK")
                .href("/heart"),
        );
        let at = |needle: &str| {
            out.find(needle)
                .unwrap_or_else(|| panic!("{needle} in {out}"))
        };
        // The head: the icon, the label as the link's name, the status.
        assert!(
            out.contains(r#"<span data-slot="stat-icon" data-tone="danger" aria-hidden="true">"#),
            "{out}"
        );
        assert!(out.contains(r#"<span data-slot="stat-label"><a data-slot="stat-link" href="/heart">Resting heart rate</a></span>"#), "{out}");
        // The figure and its unit read as "58 bpm".
        assert!(
            out.contains(r#"58 <span data-slot="stat-unit">bpm</span></span>"#),
            "{out}"
        );
        assert!(
            out.contains(r#"<span data-slot="stat-caption">vs last week</span>"#),
            "{out}"
        );
        // The order: head, figure with its change, the aside beside it, the chart under.
        assert!(at("stat-icon") < at("stat-label") && at("stat-label") < at("stat-status"));
        assert!(at("stat-status") < at("stat-figure") && at("stat-figure") < at("stat-value"));
        assert!(at("stat-trend") < at("stat-aside") && at("RING") < at("stat-chart"));
        let uses = crate::measure(&Stat::new("x", "1").icon(icons::HEART).into()).uses;
        assert!(uses.has(Uses::APP_BLOCKS));
        // Only the words, never markup, from a hostile label.
        let hostile = html(Stat::new("<img src=x onerror=1>", "1").icon(icons::HEART));
        assert!(!hostile.contains("<img"), "{hostile}");
    }

    #[test]
    fn a_fit_grid_takes_the_nearest_column_width_and_no_column_count() {
        use super::super::Grid;
        for (asked, drawn) in [
            (0, 8),
            (9, 8),
            (11, 10),
            (13, 12),
            (18, 16),
            (19, 20),
            (200, 24),
        ] {
            let out = html(Grid::fit(asked));
            assert!(
                out.contains(&format!(r#"data-fit="{drawn}""#)),
                "{asked}: {out}"
            );
            assert!(!out.contains("data-cols"), "{out}");
        }
    }

    #[test]
    fn an_info_tip_is_a_button_that_opens_a_popover_named_by_what_it_explains() {
        let out = html(
            InfoTip::new("tip", "Poincaré plot")
                .title("How to read this chart")
                .body("The comet's width is the short-term variability."),
        );
        assert!(
            out.contains(r#"<button data-slot="info-tip" type="button" popovertarget="tip" aria-haspopup="dialog" aria-label="About Poincaré plot">"#),
            "{out}"
        );
        assert!(
            out.contains(r#"id="tip" popover="auto" role="dialog" tabindex="-1" autofocus data-ui-place data-side="bottom""#),
            "{out}"
        );
        assert!(out.contains(r#"aria-labelledby="tip-title""#), "{out}");
        assert!(
            out.contains(
                r#"<p data-slot="info-tip-title" id="tip-title">How to read this chart</p>"#
            ),
            "{out}"
        );
        // Without a title the note is named like the chip.
        let bare = html(InfoTip::new("t", "Brain age").body("x"));
        assert!(
            bare.contains(r#"role="dialog" tabindex="-1""#)
                && bare.contains(r#"aria-label="About Brain age""#)
        );
        assert!(!bare.contains("aria-labelledby"), "{bare}");
        // The note's words are the page's.
        let hindi = html(InfoTip::new("t", "नींद").word("के बारे में"));
        assert!(hindi.contains(r#"aria-label="के बारे में नींद""#), "{hindi}");
        // Hostile text stays text.
        let hostile = html(InfoTip::new("t", "\"><script>x</script>").title("<img src=x>"));
        assert!(
            !hostile.contains("<script>") && !hostile.contains("<img"),
            "{hostile}"
        );
    }

    #[test]
    fn an_info_tip_loads_the_overlay_and_the_app_block_styles() {
        let uses = crate::measure(&InfoTip::new("t", "x").into()).uses;
        assert!(uses.has(Uses::OVERLAY) && uses.has(Uses::APP_BLOCKS));
        assert!(!uses.has(Uses::CORE), "no script of its own");
    }

    #[test]
    fn a_skeleton_is_one_status_with_the_shapes_hidden() {
        for (skeleton, kind) in [
            (ChartSkeleton::ring(120), "ring"),
            (ChartSkeleton::gauge(120), "gauge"),
            (ChartSkeleton::line(160), "line"),
            (ChartSkeleton::bars(12, 120), "bars"),
            (ChartSkeleton::heat_map(26), "heat-map"),
            (ChartSkeleton::matrix(5), "matrix"),
            (ChartSkeleton::meter(), "meter"),
        ] {
            let out = html(skeleton.label("Loading heart rate"));
            assert!(
                out.starts_with(&format!(
                    r#"<div data-slot="chart-skeleton" data-kind="{kind}" role="status">"#
                )),
                "{out}"
            );
            assert!(
                out.contains(r#"<span data-ui-sr="">Loading heart rate</span>"#),
                "{out}"
            );
            assert!(out.contains(r#"aria-hidden="true""#), "{out}");
            assert!(!out.contains(" style="), "{out}");
        }
    }

    #[test]
    fn a_skeleton_draws_the_cells_it_is_asked_for_and_clamps_the_rest() {
        let heat = html(ChartSkeleton::heat_map(26));
        assert_eq!(heat.matches("<rect").count(), 26 * 7);
        assert!(heat.contains(r#"viewBox="0 0 361 98""#), "{heat}");
        let matrix = html(ChartSkeleton::matrix(5));
        assert_eq!(matrix.matches("<rect").count(), 15, "the lower triangle");
        let bars = html(ChartSkeleton::bars(12, 120));
        assert_eq!(bars.matches("<rect").count(), 12);
        // Past the ends of a range the drawing stays a sane size.
        assert_eq!(
            html(ChartSkeleton::heat_map(100_000))
                .matches("<rect")
                .count(),
            53 * 7
        );
        assert_eq!(
            html(ChartSkeleton::bars(u32::MAX, 0))
                .matches("<rect")
                .count(),
            48
        );
        assert!(html(ChartSkeleton::ring(0)).contains(r#"width="48""#));
    }

    #[test]
    fn a_breathing_guide_names_its_phases_and_runs_from_a_switch() {
        let out = html(BreathingGuide::new("g", "4-7-8 breathing").size(10_000));
        assert!(
            out.contains(r#"aria-label="4-7-8 breathing""#) && out.contains(r#"data-size="320""#),
            "{out}"
        );
        assert!(
            out.contains(r#"<input data-slot="switch" type="checkbox" role="switch" id="g-run"><label for="g-run">Guide</label>"#),
            "{out}"
        );
        assert!(!out.contains("name="), "it posts nothing: {out}");
        for (asked, drawn) in [
            (0, 120),
            (139, 120),
            (140, 160),
            (200, 200),
            (219, 200),
            (221, 240),
            (9999, 320),
        ] {
            let out = html(BreathingGuide::new("g", "x").size(asked));
            assert!(
                out.contains(&format!(r#"data-size="{drawn}""#)),
                "{asked}: {out}"
            );
        }
        for phase in ["Breathe in", "Hold", "Breathe out", "Ready"] {
            assert!(out.contains(phase), "{phase} in {out}");
        }
        // The pattern written for everyone, and for a screen reader with units.
        assert!(
            out.contains("<b>4</b>") && out.contains("<b>7</b>") && out.contains("<b>8</b>"),
            "{out}"
        );
        assert!(
            out.contains(">4 seconds<") && out.contains(">8 seconds<"),
            "{out}"
        );
        // The stage is a picture a screen reader skips.
        assert!(
            out.contains(r#"data-slot="breathing-stage" aria-hidden="true""#),
            "{out}"
        );
        let uses = crate::measure(&BreathingGuide::new("g", "x").into()).uses;
        assert!(uses.has(Uses::APP_BLOCKS) && !uses.has(Uses::CORE));
    }

    #[test]
    fn a_dock_marks_the_current_page_and_names_each_icon() {
        let out = html(
            Dock::new("Sections")
                .item(DockItem::new("Sleep", "/sleep").icon(icons::MOON))
                .item(
                    DockItem::new("Home", "/")
                        .icon(icons::HOUSE)
                        .prominent()
                        .current(),
                ),
        );
        assert!(
            out.starts_with(r#"<nav data-slot="dock" aria-label="Sections">"#),
            "{out}"
        );
        assert_eq!(out.matches(r#"aria-current="page""#).count(), 1, "{out}");
        assert!(
            out.contains(r#"title="Sleep""#) && out.contains(r#"<span data-ui-sr="">Sleep</span>"#),
            "{out}"
        );
        assert!(out.contains("data-prominent"), "{out}");
    }

    #[test]
    fn a_copy_field_copies_with_a_script_and_is_selectable_without_one() {
        let out = html(
            CopyField::new("invite", "Invite <link>", "https://x.example/?a=1&b=\"2\"")
                .help("Share it once.")
                .labels("Copy link", "Link copied"),
        );
        assert!(
            out.contains(r#"<label data-slot="label" for="invite">Invite &lt;link&gt;</label>"#),
            "{out}"
        );
        assert!(
            out.contains(r#"value="https://x.example/?a=1&amp;b=&quot;2&quot;" readonly"#),
            "{out}"
        );
        assert!(
            out.contains(r#"data-ui-select aria-describedby="invite-help""#),
            "{out}"
        );
        assert!(
            out.contains(r#"<p data-slot="field-description" id="invite-help">Share it once.</p>"#),
            "{out}"
        );
        // The button is drawn for a page with a script only, copies the value as data, and
        // names the field it copies.
        assert!(
            out.contains(r#"type="button" data-script-only aria-describedby="invite""#),
            "{out}"
        );
        assert!(
            out.contains(
                r#"[[&quot;copy&quot;,&quot;https://x.example/?a=1&amp;b=\&quot;2\&quot;&quot;]]"#
            ),
            "{out}"
        );
        assert!(
            out.contains("Copy link</span>") && out.contains("Link copied</span>"),
            "{out}"
        );
        let uses = crate::render::measure(&CopyField::new("k", "Key", "v").into()).uses;
        assert!(uses.has(Uses::CORE) && uses.has(Uses::APP_BLOCKS));
    }

    #[test]
    fn an_auth_layout_puts_the_form_in_main_and_the_panel_beside_it() {
        let plain = html(AuthLayout::new("form"));
        assert_eq!(
            plain,
            r#"<div data-slot="auth-layout"><main data-slot="auth-main" id="main"><div data-slot="auth-body">form</div></main></div>"#
        );
        let full = html(
            AuthLayout::new("form")
                .aside("<about>")
                .aside_label("About Kesar")
                .corner("menu"),
        );
        assert!(
            full.starts_with(r#"<div data-slot="auth-layout" data-aside><aside data-slot="auth-aside" aria-label="About Kesar">&lt;about&gt;</aside>"#),
            "{full}"
        );
        assert!(
            full.contains(r#"<div data-slot="auth-corner">menu</div>"#),
            "{full}"
        );
    }

    #[test]
    fn a_dock_can_keep_to_phones() {
        let every = html(Dock::new("Sections").item(DockItem::new("Home", "/")));
        assert!(!every.contains("data-below"), "{every}");
        for (screen, id) in [
            (crate::charts::Screen::Sm, "sm"),
            (crate::charts::Screen::Md, "md"),
            (crate::charts::Screen::Lg, "lg"),
        ] {
            let out = html(Dock::new("Sections").below(screen));
            assert!(
                out.starts_with(&format!(
                    r#"<nav data-slot="dock" aria-label="Sections" data-below="{id}">"#
                )),
                "{out}"
            );
        }
        // The stylesheet hides it from each width on.
        let css = include_str!("../../styles/extra/app-blocks.css");
        for rule in [
            "@media (min-width:640px){[data-slot=dock][data-below=sm]{display:none}}",
            "@media (min-width:768px){[data-slot=dock][data-below=md]{display:none}}",
            "@media (min-width:1024px){[data-slot=dock][data-below=lg]{display:none}}",
        ] {
            assert!(css.contains(rule), "{rule}");
        }
    }

    #[test]
    fn a_day_pager_keeps_its_width_when_there_is_no_day_to_step_to() {
        let out = html(DayPager::new("d", "Today").previous("/?date=2026-11-11"));
        assert!(out.contains(r#"<a data-slot="day-pager-previous" href="/?date=2026-11-11" aria-label="Previous day">"#), "{out}");
        assert!(
            out.contains(
                r#"<span data-slot="day-pager-next" role="link" aria-label="Next day" aria-disabled="true">"#
            ),
            "{out}"
        );
        assert!(
            out.contains(r#"<span data-slot="day-pager-day">Today</span>"#),
            "{out}"
        );
        assert!(!out.contains("popover"), "no picker, no popup: {out}");
        let with = html(DayPager::new("d", "Today").picker("A calendar"));
        assert!(
            with.contains(r#"popovertarget="d-picker""#) && with.contains("A calendar"),
            "{with}"
        );
    }
}
