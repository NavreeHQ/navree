//! The overlays. Each rests on what the browser already has: `<dialog>` for
//! dialogs, sheets and drawers, `popover` for light-dismiss layers,
//! `<details>` for accordion and collapsible, and `command`/`commandfor` to
//! open them. Only placing a popover and the hover delay of a tooltip take
//! script, in a small module.

use super::{Button, ButtonVariant, Size, icon, root_methods};
use crate::icons;
use crate::node::{Element, IntoNodes, Node, Uses, el, into_node};

/// The header of a dialog-like layer: its title, its description and a close
/// button. Every layer has a title, so it always has an accessible name.
fn dialog_parts(
    id: &str,
    title: Node,
    description: Option<Node>,
    close: bool,
) -> (Element, Option<Element>, Option<Element>) {
    let title = el("h2")
        .slot("dialog-title")
        .attr("id", format!("{id}-title"))
        .child(title);
    let description = description.map(|text| {
        el("p")
            .slot("dialog-description")
            .attr("id", format!("{id}-description"))
            .child(text)
    });
    let close = close.then(|| {
        el("button")
            .slot("dialog-close")
            .attr("type", "button")
            .attr("command", "close")
            .attr("commandfor", id.to_owned())
            .attr("aria-label", "Close")
            .child(icon(icons::X))
    });
    (title, description, close)
}

/// A modal dialog: the browser's `<dialog>`, which brings the focus trap, Esc,
/// focus return and the inert page behind it.
///
/// ```
/// use navree_ui::{Button, Dialog, render};
///
/// let dialog = Dialog::new("new-deal", "New deal").body("…").footer(Button::primary("Create"));
/// let open = Button::outline("New deal").opens_dialog("new-deal");
/// assert!(render(&open.into()).contains(r#"command="show-modal" commandfor="new-deal""#));
/// assert!(render(&dialog.into()).contains(r#"aria-labelledby="new-deal-title""#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Dialog {
    el: Element,
    dialog_id: String,
    title: Node,
    description: Option<Node>,
    body: Vec<Node>,
    footer: Vec<Node>,
}

impl Dialog {
    /// A dialog with the id `id`. The title is required.
    pub fn new(id: impl Into<String>, title: impl Into<Node>) -> Self {
        Self::with_slot("dialog", id.into(), title.into())
    }

    fn with_slot(slot: &'static str, id: String, title: Node) -> Self {
        Self {
            el: el("dialog")
                .slot(slot)
                .attr("id", id.clone())
                .attr("aria-labelledby", format!("{id}-title"))
                .attr("closedby", "any"),
            dialog_id: id,
            title,
            description: None,
            body: Vec::new(),
            footer: Vec::new(),
        }
    }

    /// Sets the description.
    pub fn description(mut self, description: impl Into<Node>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Adds to the body.
    pub fn body(mut self, body: impl IntoNodes) -> Self {
        body.push_into(&mut self.body);
        self
    }

    /// Adds to the footer.
    pub fn footer(mut self, footer: impl IntoNodes) -> Self {
        footer.push_into(&mut self.footer);
        self
    }

    fn into_node(self) -> Node {
        let described = self
            .description
            .is_some()
            .then(|| format!("{}-description", self.dialog_id));
        let (title, description, close) =
            dialog_parts(&self.dialog_id, self.title, self.description, true);
        self.el
            .attr_opt("aria-describedby", described)
            .children((
                el("div")
                    .slot("dialog-header")
                    .children((title, description)),
                el("div").slot("dialog-body").child(self.body),
                (!self.footer.is_empty())
                    .then(|| el("div").slot("dialog-footer").child(self.footer)),
                // Last, so the first thing to focus is the dialog's own content.
                close,
            ))
            .into()
    }
}

/// A dialog that must be answered: `alertdialog`, no close button, and a click
/// outside doesn't dismiss it. Esc still cancels, as APG asks.
#[derive(Clone, Debug)]
#[must_use]
pub struct AlertDialog {
    el: Element,
    dialog_id: String,
    title: Node,
    description: Node,
    cancel: Node,
    action: Vec<Node>,
}

impl AlertDialog {
    /// An alert dialog with the id `id`. The title and description are required.
    pub fn new(
        id: impl Into<String>,
        title: impl Into<Node>,
        description: impl Into<Node>,
    ) -> Self {
        let id = id.into();
        Self {
            el: el("dialog")
                .slot("alert-dialog")
                .attr("id", id.clone())
                .attr("role", "alertdialog")
                .attr("aria-labelledby", format!("{id}-title"))
                .attr("aria-describedby", format!("{id}-description"))
                .attr("closedby", "closerequest"),
            title: title.into(),
            description: description.into(),
            cancel: Node::from("Cancel"),
            action: Vec::new(),
            dialog_id: id,
        }
    }

    /// Sets the cancel button's text.
    pub fn cancel(mut self, label: impl Into<Node>) -> Self {
        self.cancel = label.into();
        self
    }

    /// Adds the answer: usually a form with a destructive button.
    pub fn action(mut self, action: impl IntoNodes) -> Self {
        action.push_into(&mut self.action);
        self
    }

    fn into_node(self) -> Node {
        let id = self.dialog_id;
        let (title, description, _) = dialog_parts(&id, self.title, Some(self.description), false);
        // The first button of a dialog takes focus: for an alert, that's Cancel.
        let cancel = Button::new(self.cancel)
            .variant(ButtonVariant::Outline)
            .closes_dialog(id);
        self.el
            .children((
                el("div")
                    .slot("dialog-header")
                    .children((title, description)),
                el("div")
                    .slot("dialog-footer")
                    .children((cancel, self.action)),
            ))
            .into()
    }
}

/// Which edge a [`Sheet`] comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum SheetSide {
    /// The right: the default.
    #[default]
    Right,
    /// The left.
    Left,
    /// The top.
    Top,
    /// The bottom.
    Bottom,
}

impl SheetSide {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Right => "right",
            Self::Left => "left",
            Self::Top => "top",
            Self::Bottom => "bottom",
        }
    }
}

/// How wide a [`Sheet`] on the left or right is. A sheet is never wider than the
/// screen less 3 rem, so on a phone every width is the same.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum SheetWidth {
    /// 24 rem: a form or a list of filters. The default.
    #[default]
    Md,
    /// 40 rem: a record's details, with room for cards side by side and tabs.
    Lg,
    /// 60% of the screen and never less than `Lg`: a record with a table in it,
    /// such as an order and its lines, while the page behind stays in view.
    Xl,
}

impl SheetWidth {
    const fn value(self) -> Option<&'static str> {
        match self {
            Self::Md => None,
            Self::Lg => Some("lg"),
            Self::Xl => Some("xl"),
        }
    }
}

/// A dialog that slides in from an edge.
///
/// ```
/// use navree_ui::{Sheet, SheetWidth, render};
///
/// let sheet = Sheet::new("person", "Maya Patel").width(SheetWidth::Lg);
/// assert!(render(&sheet.into()).contains(r#"data-width="lg""#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct Sheet {
    inner: Dialog,
    side: SheetSide,
    width: SheetWidth,
}

impl Sheet {
    /// A sheet with the id `id`. The title is required.
    pub fn new(id: impl Into<String>, title: impl Into<Node>) -> Self {
        Self {
            inner: Dialog::with_slot("sheet", id.into(), title.into()),
            side: SheetSide::Right,
            width: SheetWidth::Md,
        }
    }

    /// Sets the edge.
    pub fn side(mut self, side: SheetSide) -> Self {
        self.side = side;
        self
    }

    /// Sets the width of a sheet on the left or right. A sheet at the top or
    /// bottom is as wide as the screen.
    pub fn width(mut self, width: SheetWidth) -> Self {
        self.width = width;
        self
    }

    /// Sets the description.
    pub fn description(mut self, description: impl Into<Node>) -> Self {
        self.inner = self.inner.description(description);
        self
    }

    /// Adds to the body.
    pub fn body(mut self, body: impl IntoNodes) -> Self {
        self.inner = self.inner.body(body);
        self
    }

    /// Adds to the footer.
    pub fn footer(mut self, footer: impl IntoNodes) -> Self {
        self.inner = self.inner.footer(footer);
        self
    }

    fn into_node(mut self) -> Node {
        self.inner.el = self
            .inner
            .el
            .attr("data-side", self.side.as_str())
            .attr_opt("data-width", self.width.value());
        self.inner.into_node()
    }
}

/// A dialog that rises from the bottom, for phones.
#[derive(Clone, Debug)]
#[must_use]
pub struct Drawer {
    inner: Dialog,
}

impl Drawer {
    /// A drawer with the id `id`. The title is required.
    pub fn new(id: impl Into<String>, title: impl Into<Node>) -> Self {
        Self {
            inner: Dialog::with_slot("drawer", id.into(), title.into()),
        }
    }

    /// Sets the description.
    pub fn description(mut self, description: impl Into<Node>) -> Self {
        self.inner = self.inner.description(description);
        self
    }

    /// Adds to the body.
    pub fn body(mut self, body: impl IntoNodes) -> Self {
        self.inner = self.inner.body(body);
        self
    }

    /// Adds to the footer.
    pub fn footer(mut self, footer: impl IntoNodes) -> Self {
        self.inner = self.inner.footer(footer);
        self
    }

    fn into_node(self) -> Node {
        self.inner.into_node()
    }
}

/// Which side of what opens it a popup appears on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Side {
    /// Above.
    Top,
    /// Below: the default.
    #[default]
    Bottom,
    /// To the left.
    Left,
    /// To the right.
    Right,
}

impl Side {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Bottom => "bottom",
            Self::Left => "left",
            Self::Right => "right",
        }
    }
}

/// Where a popup lines up along the side it opens on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum PopupAlign {
    /// With the start of what opens it.
    Start,
    /// Centred: the default.
    #[default]
    Center,
    /// With the end.
    End,
}

impl PopupAlign {
    pub(crate) const fn as_str(self) -> Option<&'static str> {
        match self {
            Self::Start => Some("start"),
            Self::Center => None,
            Self::End => Some("end"),
        }
    }
}

/// A popup with a click-outside and Esc dismissal, from the browser's
/// `popover`, opened by a button.
#[derive(Clone, Debug)]
#[must_use]
pub struct Popover {
    el: Element,
    pop_id: String,
    trigger: Node,
    variant: ButtonVariant,
    size: Size,
    side: Side,
    align: PopupAlign,
    label: Option<String>,
    body: Vec<Node>,
}

impl Popover {
    /// A popover with the id `id`, opened by a button showing `trigger`.
    pub fn new(id: impl Into<String>, trigger: impl Into<Node>) -> Self {
        Self {
            el: el("div").slot("popover-content"),
            pop_id: id.into(),
            trigger: trigger.into(),
            variant: ButtonVariant::Outline,
            size: Size::Md,
            side: Side::Bottom,
            align: PopupAlign::Center,
            label: None,
            body: Vec::new(),
        }
    }

    /// Sets the button's look.
    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Sets the button's size.
    pub fn size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }

    /// Sets the side it opens on.
    pub fn side(mut self, side: Side) -> Self {
        self.side = side;
        self
    }

    /// Sets how it lines up along that side.
    pub fn align(mut self, align: PopupAlign) -> Self {
        self.align = align;
        self
    }

    /// Names the popup for assistive technology, when the button's text doesn't.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Adds to the popup.
    pub fn body(mut self, body: impl IntoNodes) -> Self {
        body.push_into(&mut self.body);
        self
    }

    fn into_node(self) -> Node {
        let trigger = Button::new(self.trigger)
            .variant(self.variant)
            .size(self.size)
            .attr("popovertarget", self.pop_id.clone())
            .attr("aria-haspopup", "dialog");
        let popup = self
            .el
            .attr("id", self.pop_id)
            .attr("popover", "auto")
            .attr("role", "dialog")
            .attr("tabindex", -1_i64)
            .flag("autofocus", true)
            .attr_opt("aria-label", self.label)
            .flag("data-ui-place", true)
            .attr("data-side", self.side.as_str())
            .attr_opt("data-align", self.align.as_str())
            .uses(Uses::OVERLAY)
            .child(self.body);
        Node::list((trigger, popup))
    }
}

/// A popup on hover or focus with more about a link. Sighted mouse users get
/// it; the same content must be reachable another way.
#[derive(Clone, Debug)]
#[must_use]
pub struct HoverCard {
    el: Element,
    card_id: String,
    trigger: Node,
    body: Vec<Node>,
}

impl HoverCard {
    /// A card with the id `id` for `trigger`, usually a link.
    pub fn new(id: impl Into<String>, trigger: impl Into<Node>) -> Self {
        Self {
            el: el("div").slot("hover-card-content"),
            card_id: id.into(),
            trigger: trigger.into(),
            body: Vec::new(),
        }
    }

    /// Adds to the card.
    pub fn body(mut self, body: impl IntoNodes) -> Self {
        body.push_into(&mut self.body);
        self
    }

    fn into_node(self) -> Node {
        let trigger = el("span")
            .slot("hover-card-trigger")
            .attr("data-ui-hover", self.card_id.clone())
            .child(self.trigger);
        let card = self
            .el
            .attr("id", self.card_id)
            .attr("popover", "manual")
            .flag("data-ui-place", true)
            .uses(Uses::OVERLAY)
            .child(self.body);
        Node::list((trigger, card))
    }
}

/// A short description on hover and on focus, with the `tooltip` role.
#[derive(Clone, Debug)]
#[must_use]
pub struct Tooltip {
    el: Element,
    tip_id: String,
    trigger: Node,
    text: Node,
    side: Side,
}

impl Tooltip {
    /// A tooltip with the id `id` saying `text` about `trigger`, a focusable element.
    pub fn new(id: impl Into<String>, trigger: impl Into<Node>, text: impl Into<Node>) -> Self {
        Self {
            el: el("div").slot("tooltip-content"),
            tip_id: id.into(),
            trigger: trigger.into(),
            text: text.into(),
            side: Side::Top,
        }
    }

    /// Sets the side.
    pub fn side(mut self, side: Side) -> Self {
        self.side = side;
        self
    }

    fn into_node(self) -> Node {
        let mut inner = self.trigger;
        if let Node::Element(element) = &mut inner {
            element.attrs.push(crate::node::Attr {
                name: "aria-describedby",
                value: crate::node::AttrValue::Text(self.tip_id.clone().into()),
            });
        }
        let wrap = el("span")
            .slot("tooltip-trigger")
            .attr("data-ui-hover", self.tip_id.clone())
            .child(inner);
        let tip = self
            .el
            .attr("id", self.tip_id)
            .attr("role", "tooltip")
            .attr("popover", "manual")
            .flag("data-ui-place", true)
            .attr("data-side", self.side.as_str())
            .uses(Uses::OVERLAY)
            .child(self.text);
        Node::list((wrap, tip))
    }
}

/// Sections that open and close: `<details>`, whose keyboard is the browser's.
/// With [`exclusive`](Self::exclusive), one open at a time through `name`.
#[derive(Clone, Debug)]
#[must_use]
pub struct Accordion {
    el: Element,
    name: Option<String>,
    items: Vec<(Node, Node, bool)>,
}

impl Accordion {
    /// An accordion.
    pub fn new() -> Self {
        Self {
            el: el("div").slot("accordion"),
            name: None,
            items: Vec::new(),
        }
    }

    /// Allows one section open at a time, in the group `name`.
    pub fn exclusive(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Adds a section.
    pub fn item(mut self, title: impl Into<Node>, body: impl Into<Node>) -> Self {
        self.items.push((title.into(), body.into(), false));
        self
    }

    /// Adds a section that starts open.
    pub fn open_item(mut self, title: impl Into<Node>, body: impl Into<Node>) -> Self {
        self.items.push((title.into(), body.into(), true));
        self
    }

    fn into_node(self) -> Node {
        let name = self.name;
        let items: Vec<Element> = self
            .items
            .into_iter()
            .map(|(title, body, open)| {
                el("details")
                    .slot("accordion-item")
                    .attr_opt("name", name.clone())
                    .flag("open", open)
                    .children((
                        el("summary")
                            .slot("accordion-trigger")
                            .children((title, icon(icons::CHEVRON_DOWN))),
                        el("div").slot("accordion-content").child(body),
                    ))
            })
            .collect();
        self.el.child(items).into()
    }
}

impl Default for Accordion {
    fn default() -> Self {
        Self::new()
    }
}

/// One section that opens and closes.
#[derive(Clone, Debug)]
#[must_use]
pub struct Collapsible {
    el: Element,
    title: Node,
    body: Vec<Node>,
}

impl Collapsible {
    /// A section titled `title`.
    pub fn new(title: impl Into<Node>) -> Self {
        Self {
            el: el("details").slot("collapsible"),
            title: title.into(),
            body: Vec::new(),
        }
    }

    /// Starts open.
    pub fn open(mut self, open: bool) -> Self {
        self.el = self.el.flag("open", open);
        self
    }

    /// Adds to the section.
    pub fn body(mut self, body: impl IntoNodes) -> Self {
        body.push_into(&mut self.body);
        self
    }

    fn into_node(self) -> Node {
        self.el
            .children((
                el("summary")
                    .slot("collapsible-trigger")
                    .children((self.title, icon(icons::CHEVRON_DOWN))),
                el("div").slot("collapsible-content").child(self.body),
            ))
            .into()
    }
}

/// A scrolling region with thin scrollbars, reachable by keyboard.
#[derive(Clone, Debug)]
#[must_use]
pub struct ScrollArea {
    el: Element,
}

impl ScrollArea {
    /// An area named `label` for assistive technology.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            el: el("div")
                .slot("scroll-area")
                .attr("role", "region")
                .attr("aria-label", label.into())
                .attr("tabindex", 0_i64),
        }
    }

    /// Adds content.
    pub fn children(mut self, children: impl IntoNodes) -> Self {
        self.el = self.el.child(children);
        self
    }

    fn into_node(self) -> Node {
        self.el.into()
    }
}

root_methods!(
    Dialog,
    AlertDialog,
    Popover,
    HoverCard,
    Tooltip,
    Accordion,
    Collapsible,
    ScrollArea
);
into_node!(
    Dialog,
    AlertDialog,
    Sheet,
    Drawer,
    Popover,
    HoverCard,
    Tooltip,
    Accordion,
    Collapsible,
    ScrollArea
);
