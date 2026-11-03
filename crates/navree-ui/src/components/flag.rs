//! A country's flag, inline.

use std::borrow::Cow;

use super::root_methods;
use crate::flags::CountryCode;
use crate::html::SafeHtml;
use crate::node::{Element, Node, el, into_node};

/// The shape of a [`Flag`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum FlagShape {
    /// Four by three: how a flag is drawn. The default.
    #[default]
    Rect,
    /// Square, from the artwork drawn for a square.
    Square,
    /// Square art cropped to a circle.
    Round,
}

/// How wide a [`Flag`] is. A rectangle is a quarter shorter than it is wide; a
/// square or round flag is as tall as it is wide.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum FlagSize {
    /// 20 px wide.
    Xs,
    /// 24 px wide: the default.
    #[default]
    Sm,
    /// 32 px wide.
    Md,
    /// 40 px wide.
    Lg,
    /// 48 px wide.
    Xl,
}

impl FlagSize {
    const fn as_str(self) -> Option<&'static str> {
        match self {
            Self::Xs => Some("xs"),
            Self::Sm => None,
            Self::Md => Some("md"),
            Self::Lg => Some("lg"),
            Self::Xl => Some("xl"),
        }
    }
}

/// What a flag the toolkit does not know draws: a pale box with a cross, in the
/// text colour. One for each shape.
const UNKNOWN_RECT: &str = r##"<rect width="640" height="480" fill="currentColor" fill-opacity=".08"/><path d="M0 0l640 480M640 0L0 480" stroke="currentColor" stroke-opacity=".3" stroke-width="12" fill="none"/>"##;
const UNKNOWN_SQUARE: &str = r##"<rect width="512" height="512" fill="currentColor" fill-opacity=".08"/><path d="M0 0l512 512M512 0L0 512" stroke="currentColor" stroke-opacity=".3" stroke-width="12" fill="none"/>"##;

/// A flag, drawn inline from the code of a country, a region or a body.
///
/// ```
/// use navree_ui::{Flag, FlagShape, FlagSize, flag, flags, render};
///
/// // A constant or the macro: only this flag goes into the binary.
/// let html = render(&Flag::new(flags::AE).size(FlagSize::Md).into());
/// assert!(html.contains(r#"aria-label="United Arab Emirates""#));
/// let same = render(&flag!("AE").size(FlagSize::Md).into());
/// assert_eq!(html, same);
///
/// // A code from data: any case, and an unknown one draws a neutral box.
/// let from_data = Flag::country("gb-sct").shape(FlagShape::Round);
/// let unknown = Flag::country("??");
/// assert!(render(&unknown.into()).contains(r#"data-state="unknown""#));
/// # let _ = from_data;
/// ```
///
/// By default a flag is an image with the country's name, so a flag alone in a
/// table cell still says which country it is. Beside the country's name it
/// would be read twice: say [`decorative`](Self::decorative).
#[derive(Clone, Debug)]
#[must_use]
pub struct Flag {
    el: Element,
    code: Option<CountryCode>,
    shape: FlagShape,
    label: Option<Cow<'static, str>>,
    decorative: bool,
    scope: Option<&'static str>,
}

impl Flag {
    /// The flag of `code`: `Flag::new(flags::AE)`.
    pub fn new(code: CountryCode) -> Self {
        Self {
            el: el("svg").slot("flag"),
            code: Some(code),
            shape: FlagShape::Rect,
            label: None,
            decorative: false,
            scope: None,
        }
    }

    /// The flag of a code that comes from data, such as `"ae"` or `"GB-SCT"`. A
    /// code no flag has draws a neutral placeholder and never panics.
    ///
    /// This puts every flag into the binary, since the code could be any of
    /// them. When the code is in the source, use [`Flag::new`] with a constant
    /// from [`flags`](crate::flags), or [`flag!`](crate::flag), which also
    /// checks it while the app compiles.
    pub fn country(code: impl AsRef<str>) -> Self {
        let mut flag = Self::new_unknown();
        flag.code = CountryCode::parse(code.as_ref().trim());
        flag
    }

    fn new_unknown() -> Self {
        Self {
            el: el("svg").slot("flag"),
            code: None,
            shape: FlagShape::Rect,
            label: None,
            decorative: false,
            scope: None,
        }
    }

    /// The code this draws, or `None` for the placeholder.
    pub fn code(&self) -> Option<CountryCode> {
        self.code
    }

    /// Sets the shape.
    pub fn shape(mut self, shape: FlagShape) -> Self {
        self.shape = shape;
        self
    }

    /// Shorthand for [`FlagShape::Round`].
    pub fn round(self) -> Self {
        self.shape(FlagShape::Round)
    }

    /// Sets the width.
    pub fn size(mut self, size: FlagSize) -> Self {
        self.el = match size.as_str() {
            Some(name) => self.el.attr("data-size", name),
            None => self.el,
        };
        self
    }

    /// Replaces the accessible name, which is the country's name.
    pub fn label(mut self, label: impl Into<Cow<'static, str>>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Hides the flag from assistive technology, for when the country's name is
    /// written beside it.
    pub fn decorative(mut self) -> Self {
        self.decorative = true;
        self
    }

    /// Gives the ids inside the flag's artwork a prefix, so that two copies on one page
    /// never share one. Only Anguilla, Belarus and Nicaragua need it: they cut a shape
    /// out of their drawing with a clip, and browsers do not draw a clip that is
    /// defined inside hidden content. Use it on the flags of a closed menu, a hidden
    /// tab or a closed dialog that sit before the same flags in view. A scope is
    /// letters, digits, `-` and `_`; anything else is ignored.
    pub fn scope(mut self, scope: &'static str) -> Self {
        let valid = !scope.is_empty()
            && scope
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        self.scope = valid.then_some(scope);
        self
    }

    fn into_node(self) -> Node {
        let square = self.shape != FlagShape::Rect;
        let (art, view_box) = match (self.code, square) {
            (Some(code), false) => (code.rect_shapes(), "0 0 640 480"),
            (Some(code), true) => (code.square_shapes(), "0 0 512 512"),
            (None, false) => (UNKNOWN_RECT, "0 0 640 480"),
            (None, true) => (UNKNOWN_SQUARE, "0 0 512 512"),
        };
        let mut root = self.el.attr("viewBox", view_box).attr("focusable", "false");
        root = match self.shape {
            FlagShape::Rect => root,
            FlagShape::Square => root.attr("data-shape", "square"),
            FlagShape::Round => root.attr("data-shape", "round"),
        };
        if self.code.is_none() {
            root = root.attr("data-state", "unknown");
        }
        root = if self.decorative {
            root.attr("aria-hidden", "true")
        } else {
            let name: Cow<'static, str> = match (self.label, self.code) {
                (Some(label), _) => label,
                (None, Some(code)) => Cow::Borrowed(code.name()),
                (None, None) => Cow::Borrowed("Unknown country"),
            };
            root.attr("role", "img").attr("aria-label", name)
        };
        let art = match self.scope {
            Some(scope) if art.contains(" id=\"") => {
                Node::Raw(SafeHtml::trusted(scoped(art, scope)))
            }
            _ => Node::Static(art),
        };
        root.child(art).into()
    }
}

/// The artwork with every id, and every reference to one, prefixed with `scope-`.
fn scoped(art: &str, scope: &str) -> String {
    art.replace(" id=\"", &format!(" id=\"{scope}-"))
        .replace("href=\"#", &format!("href=\"#{scope}-"))
        .replace("url(#", &format!("url(#{scope}-"))
}

impl From<CountryCode> for Flag {
    fn from(code: CountryCode) -> Self {
        Self::new(code)
    }
}

root_methods!(Flag);
into_node!(Flag);
