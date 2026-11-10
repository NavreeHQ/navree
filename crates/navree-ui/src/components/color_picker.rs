//! ColorPicker: the browser's own colour field, with a hex field and swatches beside it.

use super::{only_with_script, root_methods};
use crate::node::{Element, Node, Uses, el, into_node};

/// A colour field. The colour is the browser's own `<input type="color">`, which posts
/// `#rrggbb` and has the platform's picker, its keys and its screen reader support. With a
/// script, a hex field and a row of swatches sit beside it and move in step with it;
/// without one, the colour field alone is drawn, and it posts all the same.
///
/// ```
/// use navree_ui::{ColorPicker, render};
///
/// let picker = ColorPicker::new("accent")
///     .value("#D97706")
///     .swatches(["#ef4444", "#d97706", "#16a34a", "#2563eb"]);
/// let html = render(&picker.into());
/// assert!(html.contains(r##"type="color" name="accent" value="#d97706""##));
/// assert_eq!(ColorPicker::parse("ABC"), Some("#aabbcc".to_owned()));
/// assert_eq!(ColorPicker::parse("red"), None);
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct ColorPicker {
    el: Element,
    native: Element,
    value: String,
    swatches: Vec<(String, Option<String>)>,
    hex_label: String,
    swatches_label: String,
    disabled: bool,
}

impl ColorPicker {
    /// A colour field posting as `name`, black until [`value`](Self::value) says otherwise.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            el: el("div")
                .slot("color-picker")
                .flag("data-ui-color", true)
                .uses(Uses::PARTS.with(Uses::PARTS_STYLES)),
            native: el("input")
                .slot("color-native")
                .attr("type", "color")
                .attr("name", name.into()),
            value: "#000000".to_owned(),
            swatches: Vec::new(),
            hex_label: "Hex value".to_owned(),
            swatches_label: "Swatches".to_owned(),
            disabled: false,
        }
    }

    /// Reads a colour the way the field posts it: `#rrggbb`, `#rgb` or either without
    /// the `#`, in any case, as lowercase `#rrggbb`. Anything else, such as a colour
    /// name or an alpha value, is `None`. A server reads what a form sent with this.
    pub fn parse(text: &str) -> Option<String> {
        let text = text.trim();
        let digits = text.strip_prefix('#').unwrap_or(text);
        if !matches!(digits.len(), 3 | 6) || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let mut out = String::with_capacity(7);
        out.push('#');
        if digits.len() == 3 {
            for c in digits.chars() {
                let c = c.to_ascii_lowercase();
                out.push(c);
                out.push(c);
            }
        } else {
            out.push_str(&digits.to_ascii_lowercase());
        }
        Some(out)
    }

    /// Sets the colour. Text that is not a colour (see [`parse`](Self::parse)) is ignored.
    pub fn value(mut self, value: impl AsRef<str>) -> Self {
        if let Some(value) = Self::parse(value.as_ref()) {
            self.value = value;
        }
        self
    }

    /// Adds swatches to choose from, named by their hex code. Text that is not a colour
    /// is left out.
    pub fn swatches<S: AsRef<str>>(mut self, colors: impl IntoIterator<Item = S>) -> Self {
        for color in colors {
            if let Some(color) = Self::parse(color.as_ref()) {
                self.swatches.push((color, None));
            }
        }
        self
    }

    /// Adds one swatch with a name a reader hears (`Saffron`) in place of its hex code.
    /// Text that is not a colour is left out.
    pub fn swatch(mut self, color: impl AsRef<str>, name: impl Into<String>) -> Self {
        if let Some(color) = Self::parse(color.as_ref()) {
            self.swatches.push((color, Some(name.into())));
        }
        self
    }

    /// Names the hex field and the group of swatches for assistive technology
    /// (`Hex value` and `Swatches` by default).
    pub fn labels(mut self, hex: impl Into<String>, swatches: impl Into<String>) -> Self {
        self.hex_label = hex.into();
        self.swatches_label = swatches.into();
        self
    }

    /// Disables it.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Marks it invalid.
    pub fn invalid(mut self, invalid: bool) -> Self {
        if invalid {
            self.native = self.native.attr("aria-invalid", "true");
        }
        self
    }

    fn into_node(self) -> Node {
        let value = self.value;
        let native = self
            .native
            .attr("value", value.clone())
            .flag("disabled", self.disabled);
        let hex = el("input")
            .slot("color-hex")
            .attr("type", "text")
            .attr("value", value.clone())
            .attr("maxlength", 7_i64)
            .attr("size", 8_i64)
            .attr("autocomplete", "off")
            .attr("autocapitalize", "off")
            .attr("spellcheck", "false")
            .attr("aria-label", self.hex_label)
            .flag("disabled", self.disabled);
        let swatches = (!self.swatches.is_empty()).then(|| {
            el("div")
                .slot("color-swatches")
                .attr("role", "group")
                .attr("aria-label", self.swatches_label)
                .children(
                    self.swatches
                        .into_iter()
                        .map(|(color, name)| {
                            el("button")
                                .slot("color-swatch")
                                .attr("type", "button")
                                .attr("data-ui-color-value", color.clone())
                                .attr("aria-label", name.unwrap_or_else(|| color.clone()))
                                .attr(
                                    "aria-pressed",
                                    if color == value { "true" } else { "false" },
                                )
                                .flag("disabled", self.disabled)
                                .child(
                                    el("svg")
                                        .attr("viewBox", "0 0 1 1")
                                        .attr("aria-hidden", "true")
                                        .attr("focusable", "false")
                                        .child(
                                            el("rect")
                                                .attr("width", "1")
                                                .attr("height", "1")
                                                .attr("fill", color),
                                        ),
                                )
                        })
                        .collect::<Vec<_>>(),
                )
        });
        self.el
            .children((native, only_with_script((hex, swatches))))
            .into()
    }
}

root_methods!(ColorPicker);
into_node!(ColorPicker);
