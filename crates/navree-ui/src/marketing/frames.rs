//! [`BrowserFrame`] and [`PhoneFrame`]: a screenshot, or a live part of the product, shown
//! in the window or the phone it runs in.

use std::borrow::Cow;

use crate::components::root_methods;
use crate::node::{Element, Node, Uses, el, into_node};
use crate::theme::Mode;

/// The `data-mode` that draws a frame and what it holds in `mode`: a frame in the page's
/// own mode says nothing.
fn mode_value(mode: Option<Mode>) -> Option<&'static str> {
    mode.map(Mode::id)
}

/// A browser window around a screenshot or a live part of the product: a bar with its
/// three buttons and an address, and the view under it.
///
/// The frame follows the page's light or dark mode; [`mode`](Self::mode) fixes it, so a
/// dark screenshot sits in a dark window on a light page. What it holds is drawn in the
/// same mode: components inside a dark frame are dark.
///
/// ```
/// use navree_ui::marketing::BrowserFrame;
/// use navree_ui::theme::Mode;
/// use navree_ui::{Picture, render};
///
/// let frame = BrowserFrame::new(Picture::new("/img/today.png", "Today: a score of 82", 1440, 900))
///     .address("app.kesar.example/today")
///     .mode(Mode::Dark);
/// let html = render(&frame.into());
/// assert!(html.contains(r#"data-mode="dark""#));
/// assert!(html.contains("app.kesar.example/today"));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct BrowserFrame {
    el: Element,
    content: Node,
    address: Option<Cow<'static, str>>,
    mode: Option<Mode>,
    caption: Option<Node>,
}

impl BrowserFrame {
    /// A window showing `content`.
    pub fn new(content: impl Into<Node>) -> Self {
        Self {
            el: el("figure").slot("browser-frame").uses(Uses::MARKETING),
            content: content.into(),
            address: None,
            mode: None,
            caption: None,
        }
    }

    /// The address in its bar, without `https://`.
    pub fn address(mut self, address: impl Into<Cow<'static, str>>) -> Self {
        self.address = Some(address.into());
        self
    }

    /// Draws it, and what it holds, light or dark whatever the page's mode.
    pub fn mode(mut self, mode: Mode) -> Self {
        self.mode = Some(mode);
        self
    }

    /// A line under it, saying what it shows.
    pub fn caption(mut self, caption: impl Into<Node>) -> Self {
        self.caption = Some(caption.into());
        self
    }

    fn into_node(self) -> Node {
        self.el
            .attr_opt("data-mode", mode_value(self.mode))
            .children((
                el("div")
                    .slot("browser-bar")
                    .attr("aria-hidden", "true")
                    .children((
                        el("span").slot("browser-dots").children((
                            el("span"),
                            el("span"),
                            el("span"),
                        )),
                        self.address
                            .map(|address| el("span").slot("browser-address").child(address)),
                    )),
                el("div").slot("browser-view").child(self.content),
                self.caption
                    .map(|caption| el("figcaption").slot("frame-caption").child(caption)),
            ))
            .into()
    }
}

/// A phone around a screenshot or a live part of the product: its body, the camera at the
/// top of the screen, and the screen at a phone's shape. Like [`BrowserFrame`] it follows
/// the page's mode unless [`mode`](Self::mode) fixes it.
///
/// ```
/// use navree_ui::marketing::PhoneFrame;
/// use navree_ui::{Picture, render};
///
/// let phone = PhoneFrame::new(Picture::new("/img/sleep.png", "Last night: 7 h 36 min", 390, 844));
/// assert!(render(&phone.into()).contains(r#"data-slot="phone-screen""#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct PhoneFrame {
    el: Element,
    content: Node,
    mode: Option<Mode>,
    caption: Option<Node>,
}

impl PhoneFrame {
    /// A phone showing `content`.
    pub fn new(content: impl Into<Node>) -> Self {
        Self {
            el: el("figure").slot("phone-frame").uses(Uses::MARKETING),
            content: content.into(),
            mode: None,
            caption: None,
        }
    }

    /// Draws it, and what it holds, light or dark whatever the page's mode.
    pub fn mode(mut self, mode: Mode) -> Self {
        self.mode = Some(mode);
        self
    }

    /// A line under it, saying what it shows.
    pub fn caption(mut self, caption: impl Into<Node>) -> Self {
        self.caption = Some(caption.into());
        self
    }

    fn into_node(self) -> Node {
        self.el
            .attr_opt("data-mode", mode_value(self.mode))
            .children((
                el("div")
                    .slot("phone-body")
                    .child(el("div").slot("phone-screen").children((
                        el("span").slot("phone-camera").attr("aria-hidden", "true"),
                        el("div").slot("phone-view").child(self.content),
                    ))),
                self.caption
                    .map(|caption| el("figcaption").slot("frame-caption").child(caption)),
            ))
            .into()
    }
}

root_methods!(BrowserFrame, PhoneFrame);
into_node!(BrowserFrame, PhoneFrame);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render;

    #[test]
    fn a_frame_in_the_pages_mode_says_nothing_and_a_fixed_one_says_its_mode() {
        let plain = render(&BrowserFrame::new("x").into());
        assert!(!plain.contains("data-mode"), "{plain}");
        let light = render(&PhoneFrame::new("x").mode(Mode::Light).into());
        assert!(light.contains(r#"data-mode="light""#), "{light}");
    }

    #[test]
    fn the_bar_is_hidden_from_assistive_technology_and_the_caption_is_not() {
        let html = render(
            &BrowserFrame::new("x")
                .address("<kesar>")
                .caption("Today")
                .into(),
        );
        assert!(
            html.contains(r#"data-slot="browser-bar" aria-hidden="true""#),
            "{html}"
        );
        assert!(html.contains("&lt;kesar&gt;"), "{html}");
        assert!(html.contains("<figcaption data-slot=\"frame-caption\">Today</figcaption>"));
    }
}
