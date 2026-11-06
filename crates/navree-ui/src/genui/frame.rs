//! [`SurfaceFrame`]: where a component the model chose is drawn.
//!
//! The frame is a region of its own, named after what it shows, with a line that says
//! the assistant chose it. What the component's code drew goes in its body after
//! everything that could act is taken out of it:
//!
//! - **Nothing that posts or takes input**: forms, buttons, fields, dialogs and
//!   templates go, with what is inside them. A component's buttons are
//!   [`ActionButton`]s, which the frame draws itself below the body, each signed by the
//!   host for this reader, this conversation and this component.
//! - **Nothing the runtime would run**: commands, hotkeys, live sources, fetches and
//!   invoker commands go; only a chart's own behaviour stays.
//! - **Nothing of the page's**: an element a conversation's parts drew (an approval
//!   card, a composer, a message) goes whole, so a component can't stand in for one; and
//!   every id, and every reference to one, is put under the frame's own prefix, so
//!   nothing in the body can be mistaken for, or point at, a part of the page.
//! - **No raw markup**, which can't be read to check it.
//!
//! The body is walked to a fixed depth and a fixed number of elements; past them the
//! rest is left out.

use crate::components::{AlertDialog, Button, Card, LazyItem, Text};
use crate::html::LocalHref;
use crate::icons;
use crate::node::{Attr, AttrValue, Element, Node, el};

use super::catalog::ActionButton;
use super::chat::{hidden, id_part, post_form};

/// How deep a component's tree is walked; deeper parts are left out.
const MAX_DEPTH: usize = 48;

/// How many elements of a component's tree are kept; the rest are left out.
const MAX_ELEMENTS: usize = 4000;

/// Elements a component's body never keeps, with what is inside them.
const DROPPED: [&str; 16] = [
    "form", "button", "textarea", "select", "option", "optgroup", "datalist", "dialog",
    "template", "slot", "fieldset", "legend", "menu", "frame", "frameset", "portal",
];

/// Attributes that point at an element by its id, and are put under the prefix.
const ID_REFERENCES: [&str; 12] = [
    "for",
    "aria-labelledby",
    "aria-describedby",
    "aria-controls",
    "aria-owns",
    "aria-activedescendant",
    "aria-details",
    "aria-errormessage",
    "aria-flowto",
    "headers",
    "list",
    "data-keys",
];

/// Attributes a component's body never keeps: what acts, opens, submits, fetches or takes
/// the keyboard.
fn dropped_attr(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    (name.starts_with("data-ui-") && name != "data-ui-chart")
        || name.starts_with("data-genui")
        || matches!(
            name.as_str(),
            "command"
                | "commandfor"
                | "popover"
                | "popovertarget"
                | "popovertargetaction"
                | "form"
                | "formaction"
                | "formmethod"
                | "formtarget"
                | "formenctype"
                | "formnovalidate"
                | "autofocus"
                | "accesskey"
                | "contenteditable"
                | "target"
                | "ping"
                | "download"
                | "name"
        )
}

/// Whether an element is one of a conversation's own parts, or the page's.
fn is_chrome(element: &Element) -> bool {
    element.attrs.iter().any(|attr| attr.name == "data-genui")
}

/// Whether an `input` may stay: a checkbox with no name, which a chart's stylesheet
/// turns into a toggle and which posts nothing.
fn harmless_input(element: &Element) -> bool {
    element.get("type").and_then(AttrValue::as_text).as_deref() == Some("checkbox")
        && element.get("name").is_none()
}

/// The walk's count of elements kept.
struct Walk<'a> {
    prefix: &'a str,
    kept: usize,
}

impl Walk<'_> {
    fn node(&mut self, mut node: Node, depth: usize) -> Node {
        // A node frees its children itself, so they are taken out of it, never moved.
        if let Node::Element(element) = &mut node {
            let element = std::mem::replace(element, el("div"));
            return self.element(element, depth);
        }
        if let Node::Fragment(nodes) = &mut node {
            let nodes = std::mem::take(nodes);
            return Node::Fragment(
                nodes
                    .into_iter()
                    .map(|node| self.node(node, depth))
                    .filter(|node| !matches!(node, Node::Empty))
                    .collect(),
            );
        }
        if matches!(node, Node::Raw(_)) {
            return Node::Empty;
        }
        node
    }

    fn element(&mut self, mut element: Element, depth: usize) -> Node {
        if depth > MAX_DEPTH || self.kept >= MAX_ELEMENTS {
            return Node::Empty;
        }
        let tag = element.tag.to_ascii_lowercase();
        if DROPPED.contains(&tag.as_str())
            || (tag == "input" && !harmless_input(&element))
            || is_chrome(&element)
        {
            return Node::Empty;
        }
        self.kept += 1;
        let attrs = std::mem::take(&mut element.attrs);
        element.attrs = attrs
            .into_iter()
            .filter(|attr| !dropped_attr(attr.name))
            .map(|attr| self.attr(attr))
            .collect();
        let children = std::mem::take(&mut element.children);
        element.children = children
            .into_iter()
            .map(|child| self.node(child, depth + 1))
            .filter(|node| !matches!(node, Node::Empty))
            .collect();
        Node::Element(element)
    }

    fn attr(&self, attr: Attr) -> Attr {
        let Some(text) = attr.value.as_text().map(|text| text.into_owned()) else {
            return attr;
        };
        let value = if attr.name == "id" {
            format!("{}-{}", self.prefix, text)
        } else if ID_REFERENCES.contains(&attr.name) {
            text.split_whitespace()
                .map(|id| format!("{}-{}", self.prefix, id))
                .collect::<Vec<_>>()
                .join(" ")
        } else if attr.name == "href" && text.starts_with('#') && text.len() > 1 {
            format!("#{}-{}", self.prefix, &text[1..])
        } else {
            return attr;
        };
        Attr {
            name: attr.name,
            value: AttrValue::Text(value.into_boxed_str()),
        }
    }
}

/// Takes out of `body` everything a component may not draw (see the module's docs),
/// with its ids put under `prefix`.
pub fn defuse(body: Node, prefix: &str) -> Node {
    let mut walk = Walk {
        prefix: &id_part(prefix),
        kept: 0,
    };
    walk.node(body, 0)
}

/// What the frame says around a component.
#[derive(Clone, Debug)]
pub struct FrameLabels {
    /// The line under the title: who chose it.
    pub chosen: String,
    /// In place of a component the reader may no longer see.
    pub unavailable: String,
    /// In place of a component that failed to draw.
    pub failed: String,
    /// What is read out when a component arrives, before its title.
    pub arrived: String,
    /// The cancel button of an action's question.
    pub cancel: String,
}

impl Default for FrameLabels {
    fn default() -> Self {
        Self {
            chosen: "Shown by the assistant from your data".to_owned(),
            unavailable: "You can't see this any more.".to_owned(),
            failed: "This couldn't be shown.".to_owned(),
            arrived: "The assistant showed".to_owned(),
            cancel: "Cancel".to_owned(),
        }
    }
}

/// How a frame stands.
#[derive(Clone, Debug)]
enum Shown {
    Body(Node),
    Unavailable,
    Failed,
}

/// Where a component the model chose is drawn, as an item of the conversation. See the
/// module's docs for what is taken out of what it drew.
///
/// ```
/// use navree_ui::genui::{ApprovalCard, SurfaceFrame};
/// use navree_ui::{el, render};
/// use serde_json::json;
///
/// // A component that tries to draw an approval card and a form of its own.
/// let body = el("div").children((
///     ApprovalCard::new("c1", "Refund", json!({})).routes("/a", "/r"),
///     el("form").attr("action", "/orders/1/refund"),
///     el("p").id("total").child("₹1,250"),
/// ));
/// let html = render(&SurfaceFrame::new("s1", "Refunds", body.into()).into_item("0001").into());
/// assert!(!html.contains("approval"));
/// assert!(!html.contains("<form"));
/// assert!(html.contains(r#"id="s1-total""#));
/// ```
#[derive(Clone, Debug)]
#[must_use]
pub struct SurfaceFrame {
    id: String,
    title: String,
    shown: Shown,
    buttons: Vec<Node>,
    labels: FrameLabels,
}

impl SurfaceFrame {
    /// The component `id` (its item's key), called `title`, that drew `body`.
    pub fn new(id: impl Into<String>, title: impl Into<String>, body: Node) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            shown: Shown::Body(body),
            buttons: Vec::new(),
            labels: FrameLabels::default(),
        }
    }

    /// A component the reader may no longer see: its place, and nothing it would show.
    pub fn unavailable(id: impl Into<String>) -> Self {
        Self {
            shown: Shown::Unavailable,
            ..Self::new(id, String::new(), Node::Empty)
        }
    }

    /// A component that could not be drawn.
    pub fn failed(id: impl Into<String>) -> Self {
        Self {
            shown: Shown::Failed,
            ..Self::new(id, String::new(), Node::Empty)
        }
    }

    /// Replaces the words.
    pub fn labels(mut self, labels: FrameLabels) -> Self {
        self.labels = labels;
        self
    }

    /// Draws `button` below the body, posting to `url` (which the host signed). A
    /// destructive one opens the page's own question first, and its post says it was
    /// answered (`confirmed=1`); with no script the server asks the question instead.
    pub fn button(mut self, button: &ActionButton, url: LocalHref, csrf: Option<&str>) -> Self {
        let at = self.buttons.len();
        let frame = id_part(&self.id);
        let node: Node = match button.question() {
            None => post_form(&url, csrf, Button::outline(button.label().to_owned()).submit())
                .into(),
            Some(confirm) => {
                let dialog = format!("{frame}-ask-{at}");
                Node::list((
                    Button::destructive(button.label().to_owned()).opens_dialog(dialog.clone()),
                    AlertDialog::new(dialog, confirm.title, confirm.body)
                        .cancel(self.labels.cancel.clone())
                        .action(post_form(
                            &url,
                            csrf,
                            (
                                hidden("confirmed", "1"),
                                Button::destructive(confirm.yes).submit(),
                            ),
                        )),
                ))
            }
        };
        self.buttons.push(node);
        self
    }

    /// The frame as an item of the conversation, at `cursor`: read out when it arrives,
    /// with its title.
    pub fn into_item(self, cursor: impl AsRef<str>) -> LazyItem {
        let key = self.id.clone();
        let say = match &self.shown {
            Shown::Body(_) => format!("{}: {}", self.labels.arrived, self.title),
            Shown::Unavailable => self.labels.unavailable.clone(),
            Shown::Failed => self.labels.failed.clone(),
        };
        LazyItem::new(key, cursor, self.into_node()).announce(say)
    }

    fn into_node(self) -> Node {
        let frame = id_part(&self.id);
        let root = |card: Card, label: String| -> Node {
            card.id(frame.clone())
                .attr("data-genui", "surface")
                .attr("role", "region")
                .attr("aria-label", label)
                .into()
        };
        match self.shown {
            Shown::Unavailable => root(
                Card::new().body(Text::muted(self.labels.unavailable.clone())),
                self.labels.unavailable,
            ),
            Shown::Failed => root(
                Card::new().body(Text::muted(self.labels.failed.clone())),
                self.labels.failed,
            ),
            Shown::Body(body) => {
                let body = defuse(body, &frame);
                let label = format!("{}: {}", self.labels.chosen, self.title);
                let mut card = Card::new()
                    .heading(3)
                    .title(self.title)
                    .description(
                        el("span").children((
                            crate::components::icon(icons::SPARKLES),
                            " ",
                            self.labels.chosen,
                        )),
                    )
                    .body(el("div").attr("data-genui", "surface-body").child(body));
                if !self.buttons.is_empty() {
                    card = card.footer(
                        el("div")
                            .attr("data-genui", "surface-actions")
                            .child(self.buttons),
                    );
                }
                root(card, label)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::Cmd;
    use crate::render;

    #[test]
    fn what_could_act_is_taken_out() {
        let body: Node = el("div")
            .children((
                el("button").child("Approve"),
                el("input").attr("type", "text").attr("name", "amount"),
                el("input").attr("type", "checkbox").slot("chart-toggle").id("t1"),
                el("label").attr("for", "t1").child("UPI"),
                el("a").href("/orders/1").on_click(Cmd::open("approval-call_1")).child("Open"),
                el("a").href("#t1").child("Jump"),
                el("div").attr("data-ui-src", "/admin").attr("popovertarget", "x"),
                el("p").attr("aria-describedby", "t1 t2").child("₹1,250"),
                Node::Raw(crate::SafeHtml::trusted("<form action=/x></form>")),
            ))
            .into();
        let html = render(&defuse(body, "s9"));
        assert!(!html.contains("<button"), "{html}");
        assert!(!html.contains(r#"name="amount""#), "{html}");
        assert!(html.contains(r#"id="s9-t1""#), "{html}");
        assert!(html.contains(r#"for="s9-t1""#), "{html}");
        assert!(html.contains(r##"href="#s9-t1""##), "{html}");
        assert!(html.contains(r#"aria-describedby="s9-t1 s9-t2""#), "{html}");
        assert!(!html.contains("data-ui-click"), "{html}");
        assert!(!html.contains("data-ui-src"), "{html}");
        assert!(!html.contains("popovertarget"), "{html}");
        assert!(!html.contains("<form"), "{html}");
    }

    #[test]
    fn deep_and_wide_trees_are_cut() {
        let mut node: Node = el("span").child("leaf").into();
        for _ in 0..200 {
            node = el("div").child(node).into();
        }
        let html = render(&defuse(node, "s"));
        assert!(!html.contains("leaf"), "a tree past the depth loses what is below");
        let wide: Node = el("ul")
            .children((0..10_000).map(|n| el("li").child(n as i64)).collect::<Vec<_>>())
            .into();
        let html = render(&defuse(wide, "s"));
        assert!(html.matches("<li>").count() < MAX_ELEMENTS, "a wide tree is cut");
    }

    #[test]
    fn a_destructive_button_asks_in_a_dialog_the_page_draws() {
        crate::props! {
            /// An order.
            pub struct Order {
                /// Its key.
                pub order: crate::genui::Key,
            }
        }
        struct Refund;
        impl crate::genui::Action for Refund {
            type Input = Order;
            const NAME: &'static str = "refund";
            const DESTRUCTIVE: bool = true;
        }
        let button = ActionButton::new::<Refund>(
            "Refund",
            &Order {
                order: crate::genui::Key::new("1042").unwrap(),
            },
        )
        .confirm("Refund order CS-1042?", "₹1,250 goes back to Harpreet Kaur.", "Refund ₹1,250");
        let html = render(
            &SurfaceFrame::new("s1", "Order CS-1042", el("p").child("Paid").into())
                .button(&button, LocalHref::from("/assistant/act?sig=1"), Some("tok"))
                .into_item("0001")
                .into(),
        );
        assert!(html.contains(r#"commandfor="s1-ask-0""#), "{html}");
        assert!(html.contains(r#"role="alertdialog""#), "{html}");
        assert!(html.contains(r#"name="confirmed" value="1""#), "{html}");
        assert!(html.contains("Refund order CS-1042?"), "{html}");
        assert!(html.contains(r#"data-genui="surface-actions""#), "{html}");
    }
}
