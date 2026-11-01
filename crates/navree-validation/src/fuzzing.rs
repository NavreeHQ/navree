//! Entry points for the fuzz targets in `fuzz/` (feature `fuzzing`): the
//! HTML sanitizer's output, and the tree it checked. Not part of the API.

use crate::HtmlPolicy;
use crate::html::clean::Checked;
use crate::html::tree::{Event, Tree};

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
    nodes(Tree::parse(html).events(|_, _| false))
}

/// `html` sanitized by `policy`, and the walk over what the sanitizer
/// checked: the tree it kept, or the text alone when it kept only text.
pub fn sanitize(policy: &HtmlPolicy, html: &str) -> (String, Vec<Node>) {
    let (out, checked) = policy.sanitizer().sanitize(html);
    let checked = match checked {
        Checked::Tree(tree) => nodes(tree.events(|_, _| false)),
        Checked::Text(text) if text.is_empty() => Vec::new(),
        Checked::Text(text) => vec![Node::Text(text)],
    };
    (out, checked)
}
