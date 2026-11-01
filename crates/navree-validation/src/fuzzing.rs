//! Entry points for the fuzz targets in `fuzz/` (feature `fuzzing`): the
//! HTML sanitizer's output, and the tree it checked. Not part of the API.

use crate::HtmlPolicy;
use crate::html::clean::{Checked, as_text};
use crate::html::tree::{Event, Limits, Tree, escape_text};

/// One step of a walk over a parsed fragment, in document order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    /// An element: its namespace URL, local name and attributes
    /// (`(namespace URL, local name, value)`, in order).
    Open {
        namespace: String,
        name: String,
        attributes: Vec<(String, String, String)>,
    },
    /// The end of the last open element.
    Close,
    /// Adjacent text, joined.
    Text(String),
    Comment,
    /// A doctype or a processing instruction.
    Other,
}

fn nodes(events: Vec<Event<'_>>) -> Vec<Node> {
    events
        .into_iter()
        .map(|event| match event {
            Event::Open(name, attrs) => Node::Open {
                namespace: name.ns.to_string(),
                name: name.local.to_string(),
                attributes: attrs
                    .iter()
                    .map(|attr| {
                        (
                            attr.name.ns.to_string(),
                            attr.name.local.to_string(),
                            attr.value.to_string(),
                        )
                    })
                    .collect(),
            },
            Event::Close => Node::Close,
            Event::Text(text) => Node::Text(text),
            Event::Comment => Node::Comment,
            Event::Other => Node::Other,
        })
        .collect()
}

/// `html` parsed as the content of a `<div>`, the way the sanitizer (and a
/// browser) parses it.
pub fn parse(html: &str) -> Vec<Node> {
    let limits = Limits {
        max_input_bytes: usize::MAX,
        max_nodes: usize::MAX,
        max_depth: usize::MAX,
    };
    Tree::parse(html, limits)
        .map(|tree| nodes(tree.events(|_, _| false)))
        .unwrap_or_default()
}

/// `html` sanitized by `policy`, and the walk over what the sanitizer
/// checked: the tree it kept, or the text alone when it kept only text.
pub fn sanitize(policy: &HtmlPolicy, html: &str) -> (String, Vec<Node>) {
    // Over a limit, `HtmlPolicy::clean` keeps the input as text.
    let (out, checked) = policy.sanitizer().sanitize(html).unwrap_or_else(|_| {
        let text = as_text(html);
        (escape_text(&text), Checked::Text(text))
    });
    let checked = match checked {
        Checked::Tree(tree) => nodes(tree.events(|_, _| false)),
        Checked::Text(text) if text.is_empty() => Vec::new(),
        Checked::Text(text) => vec![Node::Text(text)],
    };
    (out, checked)
}
