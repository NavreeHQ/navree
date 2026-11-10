//! [`TestView`]: drive a page in a plain test, with no browser. A view is a
//! value, so reading it, filling it and submitting it needs nothing else.
//! What only a browser can prove lives in the real-browser suite.

use serde::de::DeserializeOwned;

use navree_validation::Validate;

use crate::form::{FieldErrors, Form, Language, decode, decode_for};
use crate::node::{AttrValue, Element, Node};
use crate::view::View;

/// What clicking something does.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Clicked {
    /// A link, to this URL.
    Link(String),
    /// A submit button.
    Submit,
    /// Browser-side commands, as the JSON the runtime reads.
    Commands(String),
    /// Opens or closes the dialog or popover with this id, natively.
    Target(String),
    /// Nothing the toolkit knows about.
    Nothing,
}

/// A page under test.
///
/// ```
/// use navree_ui::{Button, TestView, View};
/// use navree_ui::test_view::Clicked;
///
/// let page = TestView::of(&View::new("x").body(Button::primary("Edit").href("/edit")));
/// assert!(page.sees("Edit"));
/// assert_eq!(page.click("Edit"), Clicked::Link("/edit".to_owned()));
/// ```
#[derive(Clone, Debug)]
pub struct TestView {
    body: Node,
    filled: Vec<(String, String)>,
}

impl TestView {
    /// The page `view`.
    pub fn of(view: &View) -> Self {
        Self {
            body: view.body_node().clone(),
            filled: Vec::new(),
        }
    }

    /// A part of a page.
    pub fn of_node(node: impl Into<Node>) -> Self {
        Self {
            body: node.into(),
            filled: Vec::new(),
        }
    }

    /// Whether a user can read `text` on the page.
    pub fn sees(&self, text: &str) -> bool {
        self.body.text_content().contains(text)
    }

    /// Finds the first element that matches.
    fn find<'a>(node: &'a Node, test: &impl Fn(&Element) -> bool) -> Option<&'a Element> {
        match node {
            Node::Element(element) => {
                if test(element) {
                    return Some(element);
                }
                element
                    .child_nodes()
                    .iter()
                    .find_map(|child| Self::find(child, test))
            }
            Node::Fragment(nodes) => nodes.iter().find_map(|child| Self::find(child, test)),
            _ => None,
        }
    }

    fn text_of(element: &Element, name: &str) -> Option<String> {
        element
            .get(name)
            .and_then(AttrValue::as_text)
            .map(|text| text.into_owned())
    }

    /// What clicking the link or button named `name` does. The name is its
    /// text or its `aria-label`.
    pub fn click(&self, name: &str) -> Clicked {
        let found = Self::find(&self.body, &|element| {
            matches!(element.tag(), "a" | "button")
                && (Node::Element(element.clone()).text_content().trim() == name
                    || Self::text_of(element, "aria-label").as_deref() == Some(name))
        });
        let Some(element) = found else {
            return Clicked::Nothing;
        };
        if let Some(commands) = Self::text_of(element, "data-ui-click") {
            return Clicked::Commands(commands);
        }
        if let Some(target) =
            Self::text_of(element, "commandfor").or_else(|| Self::text_of(element, "popovertarget"))
        {
            return Clicked::Target(target);
        }
        if let Some(href) = Self::text_of(element, "href") {
            return Clicked::Link(href);
        }
        if Self::text_of(element, "type").as_deref() == Some("submit") {
            return Clicked::Submit;
        }
        Clicked::Nothing
    }

    /// Whether the control posting as `name` shows an error, or any field does
    /// when no name is given.
    pub fn shows_error(&self, name: Option<&str>) -> bool {
        match name {
            Some(name) => Self::find(&self.body, &|element| {
                element.get("data-invalid").is_some()
                    && element.child_nodes().iter().any(|node| {
                        Self::find(node, &|inner| {
                            Self::text_of(inner, "name").as_deref() == Some(name)
                        })
                        .is_some()
                    })
            })
            .is_some(),
            None => {
                Self::find(&self.body, &|element| element.get("data-invalid").is_some()).is_some()
            }
        }
    }

    /// Types `value` into the control posting as `name`.
    #[must_use]
    pub fn fill(mut self, name: &str, value: &str) -> Self {
        self.filled.retain(|(field, _)| field != name);
        self.filled.push((name.to_owned(), value.to_owned()));
        self
    }

    /// What the page's form would post: each control's value, then what was typed.
    pub fn form_values(&self) -> Vec<(String, String)> {
        fn walk(node: &Node, out: &mut Vec<(String, String)>) {
            match node {
                Node::Element(element) => {
                    let name = TestView::text_of(element, "name");
                    match (element.tag(), name) {
                        ("input", Some(name)) => {
                            let kind = TestView::text_of(element, "type").unwrap_or_default();
                            let on = element.get("checked").is_some();
                            if !matches!(kind.as_str(), "checkbox" | "radio") || on {
                                out.push((
                                    name,
                                    TestView::text_of(element, "value").unwrap_or_default(),
                                ));
                            }
                        }
                        ("textarea", Some(name)) => {
                            out.push((name, Node::Element(element.clone()).text_content()))
                        }
                        ("select", Some(name)) => {
                            let options = element.child_nodes();
                            let chosen = options.iter().find_map(|node| {
                                TestView::find(node, &|inner| {
                                    inner.tag() == "option" && inner.get("selected").is_some()
                                })
                            });
                            let first = options.iter().find_map(|node| {
                                TestView::find(node, &|inner| inner.tag() == "option")
                            });
                            let selected = chosen
                                .or(first)
                                .and_then(|option| TestView::text_of(option, "value"));
                            out.push((name, selected.unwrap_or_default()));
                        }
                        _ => element
                            .child_nodes()
                            .iter()
                            .for_each(|child| walk(child, out)),
                    }
                }
                Node::Fragment(nodes) => nodes.iter().for_each(|child| walk(child, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        walk(&self.body, &mut out);
        out.retain(|(name, _)| !self.filled.iter().any(|(filled, _)| filled == name));
        out.extend(self.filled.iter().cloned());
        out
    }

    /// Submits the page's form as `T`, through the same decoding as `Submit`.
    pub fn submit_as<T>(&self) -> Result<T, FieldErrors>
    where
        T: Form + DeserializeOwned + Validate,
    {
        decode::<T>(&self.form_values())
    }

    /// [`submit_as`](Self::submit_as) in the reader's language: the messages of the app
    /// and the reader's date, time and slider words, as `Submit` says them.
    pub fn submit_as_for<T>(&self, language: &Language) -> Result<T, FieldErrors>
    where
        T: Form + DeserializeOwned + Validate,
    {
        decode_for::<T>(&self.form_values(), language)
    }
}
