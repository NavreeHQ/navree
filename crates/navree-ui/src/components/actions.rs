//! Button, ButtonGroup, Toggle.

use super::{Size, icon, root_methods};
use crate::cmd::Commands;
use crate::html::Href;
use crate::icons::Icon;
use crate::node::{Element, IntoNodes, Node, el, into_node};

/// How a button looks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum ButtonVariant {
    /// The one main action: green.
    Primary,
    /// A filled, quiet action: the default.
    #[default]
    Secondary,
    /// A line around it.
    Outline,
    /// No fill until hovered.
    Ghost,
    /// Something that destroys.
    Destructive,
    /// Looks like a link.
    Link,
}

/// What a `<button>` does in a form.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum ButtonKind {
    /// Nothing by itself: the default.
    #[default]
    Button,
    /// Submits the form.
    Submit,
    /// Resets the form.
    Reset,
}

/// A button, or a link that looks like one when it has an [`href`](Self::href).
///
/// ```
/// use navree_ui::{Button, icons, render};
///
/// let html = render(&Button::primary("Edit").href("/deals/1/edit").into());
/// assert_eq!(html, r#"<a data-slot="button" data-variant="primary" href="/deals/1/edit">Edit</a>"#);
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Button {
    label: Option<Node>,
    aria_label: Option<Node>,
    variant: ButtonVariant,
    size: Size,
    start: Option<Icon>,
    end: Option<Icon>,
    href: Option<Href>,
    kind: ButtonKind,
    disabled: bool,
    block: bool,
    pending: bool,
    el: Element,
}

impl Button {
    /// A button with the default look.
    pub fn new(label: impl Into<Node>) -> Self {
        Self {
            label: Some(label.into()),
            aria_label: None,
            variant: ButtonVariant::Secondary,
            size: Size::Md,
            start: None,
            end: None,
            href: None,
            kind: ButtonKind::Button,
            disabled: false,
            block: false,
            pending: false,
            el: el("button"),
        }
    }

    /// The one main action.
    pub fn primary(label: impl Into<Node>) -> Self {
        Self::new(label).variant(ButtonVariant::Primary)
    }

    /// A button with a line around it.
    pub fn outline(label: impl Into<Node>) -> Self {
        Self::new(label).variant(ButtonVariant::Outline)
    }

    /// A button with no fill until hovered.
    pub fn ghost(label: impl Into<Node>) -> Self {
        Self::new(label).variant(ButtonVariant::Ghost)
    }

    /// A button that destroys something.
    pub fn destructive(label: impl Into<Node>) -> Self {
        Self::new(label).variant(ButtonVariant::Destructive)
    }

    /// A button with an icon and no text. It must say what it does, so the
    /// accessible label is required.
    pub fn icon_only(glyph: Icon, label: impl Into<String>) -> Self {
        let mut button = Self::new(Node::Empty);
        button.label = None;
        button.start = Some(glyph);
        button.aria_label = Some(Node::Text(label.into().into()));
        button.el = button.el.flag("data-icon-only", true);
        button
    }

    /// Sets the look.
    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Sets the size.
    pub fn size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }

    /// Adds an icon before the label.
    pub fn icon(mut self, glyph: Icon) -> Self {
        self.start = Some(glyph);
        self
    }

    /// Adds an icon after the label.
    pub fn icon_end(mut self, glyph: Icon) -> Self {
        self.end = Some(glyph);
        self
    }

    /// Makes this a link.
    pub fn href(mut self, href: impl Into<Href>) -> Self {
        self.href = Some(href.into());
        self
    }

    /// Says this link may be fetched ahead of a click (see
    /// [`Element::prefetch`](crate::Element::prefetch)). Only for a link whose `GET`
    /// changes nothing.
    pub fn prefetch(mut self) -> Self {
        self.el = self.el.prefetch();
        self
    }

    /// Says this link is fetched as soon as the page is quiet (see
    /// [`Element::prefetch_eager`](crate::Element::prefetch_eager)).
    pub fn prefetch_eager(mut self) -> Self {
        self.el = self.el.prefetch_eager();
        self
    }

    /// Makes this submit its form.
    pub fn submit(mut self) -> Self {
        self.kind = ButtonKind::Submit;
        self
    }

    /// Sets what it does in a form.
    pub fn kind(mut self, kind: ButtonKind) -> Self {
        self.kind = kind;
        self
    }

    /// Disables it.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Fills the width.
    pub fn block(mut self) -> Self {
        self.block = true;
        self
    }

    /// Shows the pending state from the first render.
    pub fn pending(mut self, pending: bool) -> Self {
        self.pending = pending;
        self
    }

    /// Opens the `<dialog>` with this id, natively, with no script.
    pub fn opens_dialog(mut self, dialog: impl Into<String>) -> Self {
        self.el = self
            .el
            .attr("command", "show-modal")
            .attr("commandfor", dialog.into());
        self
    }

    /// Closes the `<dialog>` with this id, natively, with no script.
    pub fn closes_dialog(mut self, dialog: impl Into<String>) -> Self {
        self.el = self
            .el
            .attr("command", "close")
            .attr("commandfor", dialog.into());
        self
    }

    /// Runs commands in the browser when clicked.
    pub fn on_click(mut self, commands: impl Into<Commands>) -> Self {
        self.el = self.el.on_click(commands);
        self
    }

    /// Sets `form`, `name` and the like, on the button itself.
    pub fn named(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.el = self
            .el
            .attr("name", name.into())
            .attr("value", value.into());
        self
    }

    fn into_node(self) -> Node {
        let mut root = self.el;
        root.tag = if self.href.is_some() { "a" } else { "button" };
        let mut attrs = Vec::with_capacity(root.attrs.len() + 4);
        attrs.push(slot_attr());
        attrs.append(&mut root.attrs);
        root.attrs = attrs;
        root = match self.variant {
            ButtonVariant::Secondary => root,
            ButtonVariant::Primary => root.attr("data-variant", "primary"),
            ButtonVariant::Outline => root.attr("data-variant", "outline"),
            ButtonVariant::Ghost => root.attr("data-variant", "ghost"),
            ButtonVariant::Destructive => root.attr("data-variant", "destructive"),
            ButtonVariant::Link => root.attr("data-variant", "link"),
        };
        root = root
            .attr_opt("data-size", self.size.as_str())
            .flag("data-block", self.block)
            .flag("data-pending", self.pending);
        if let Some(Node::Text(label)) = &self.aria_label {
            root = root.attr("aria-label", label.clone());
        }
        root = match self.href {
            Some(href) => {
                let root = root.href(href);
                if self.disabled {
                    root.attr("aria-disabled", "true").attr("tabindex", -1_i64)
                } else {
                    root
                }
            }
            None => {
                let kind = match self.kind {
                    ButtonKind::Button => "button",
                    ButtonKind::Submit => "submit",
                    ButtonKind::Reset => "reset",
                };
                root.attr("type", kind).flag("disabled", self.disabled)
            }
        };
        root.children((self.start.map(icon), self.label, self.end.map(icon)))
            .into()
    }
}

fn slot_attr() -> crate::node::Attr {
    crate::node::Attr {
        name: "data-slot",
        value: crate::node::AttrValue::Static("button"),
    }
}

/// Buttons joined together. It needs a label, because a group is a landmark
/// for assistive technology.
#[derive(Clone, Debug)]
#[must_use]
pub struct ButtonGroup {
    el: Element,
}

impl ButtonGroup {
    /// A group labelled `label`.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            el: el("div")
                .slot("button-group")
                .attr("role", "group")
                .attr("aria-label", label.into()),
        }
    }

    /// Stacks the buttons.
    pub fn vertical(mut self) -> Self {
        self.el = self.el.attr("data-orientation", "vertical");
        self
    }

    /// Adds buttons.
    pub fn children(mut self, children: impl IntoNodes) -> Self {
        self.el = self.el.child(children);
        self
    }

    fn into_node(self) -> Node {
        self.el.into()
    }
}

/// A two-state button. It is a checkbox underneath, so it posts with its form,
/// keeps its state with no script and has the keyboard of a checkbox.
#[derive(Clone, Debug)]
#[must_use]
pub struct Toggle {
    input: Element,
    label: Node,
    start: Option<Icon>,
    size: Size,
    el: Element,
}

impl Toggle {
    /// A toggle posting as `name`.
    pub fn new(name: impl Into<String>, label: impl Into<Node>) -> Self {
        Self {
            input: el("input")
                .attr("type", "checkbox")
                .attr("name", name.into()),
            label: label.into(),
            start: None,
            size: Size::Md,
            el: el("label").slot("toggle"),
        }
    }

    /// Starts on.
    pub fn pressed(mut self, pressed: bool) -> Self {
        self.input = self.input.flag("checked", pressed);
        self
    }

    /// Disables it.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.input = self.input.flag("disabled", disabled);
        self
    }

    /// Adds an icon.
    pub fn icon(mut self, glyph: Icon) -> Self {
        self.start = Some(glyph);
        self
    }

    /// Sets the size.
    pub fn size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }

    fn into_node(self) -> Node {
        self.el
            .attr_opt("data-size", self.size.as_str())
            .children((self.input, self.start.map(icon), self.label))
            .into()
    }
}

root_methods!(Button, ButtonGroup, Toggle);
into_node!(Button, ButtonGroup, Toggle);
