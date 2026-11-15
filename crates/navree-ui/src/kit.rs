//! Checking a component of your own against the toolkit's rules, in a plain test with no
//! browser.
//!
//! A component is a struct that turns into one [`Node`], through the same two macros the
//! toolkit's own components use ([`component_methods!`](crate::component_methods) and
//! [`component_node!`](crate::component_node)). What it draws has to keep a few rules, so the
//! page, the stylesheet that comes with it and the browser's runtime can rely on them:
//!
//! - raw markup is only an icon's shapes inside `svg`, cleaned stored HTML directly inside an
//!   element marked `data-ui-stored`, or the text of a toolkit part (an element whose slot has
//!   no colon): a component's own parts are elements;
//! - the root of an extension has a slot of its namespace, a colon and a word, so its
//!   stylesheet reaches it;
//! - the files an element names are files of an extension the toolkit serves, and its props are
//!   JSON of at most 64 KB.
//!
//! [`problems`] says what breaks them, [`assert_component`] fails a test with that, and
//! [`stored_twin`] draws the same node as stored content, for a browser test that checks the
//! component does not act on its own twin.

use crate::extension::{EXTENSION_PATH, is_script_address, is_sheet_address, markup_values};
use crate::node::{AttrValue, Element, Node, Uses, el};

/// The deepest a walk goes, as the renderer's: what is below is left out.
const MAX_DEPTH: u32 = 512;

/// The most props an element carries, in bytes.
const MOST_PROPS: usize = 64 * 1024;

/// What a walk knows about the node it is at: what is above it.
#[derive(Clone, Copy, Default)]
struct Above<'a> {
    /// The path of the element above, from the node the walk began with
    /// (`div[geo:map] > div[geo:map-body]`); empty at the top.
    path: &'a str,
    /// The tag of the element above.
    tag: Option<&'a str>,
    /// Whether the element above carries `data-ui-stored`.
    stored_parent: bool,
    /// The slot of the nearest element above that has one.
    slot: Option<&'a str>,
    /// Whether an element above names an extension's files.
    inside: bool,
    /// Whether any element above carries `data-ui-stored`.
    stored: bool,
}

/// What breaks a component's rules, one line each; empty when it keeps them: raw markup only as
/// an icon's shapes inside `svg`, cleaned stored HTML directly inside an element marked
/// `data-ui-stored`, or text under a toolkit part (a slot with no colon); the root of an
/// extension with a slot of its namespace, a colon and a word; the files an element names (in
/// attributes, and in raw text inside an extension's root) being served files of an extension;
/// props that are JSON of at most 64 KB; and no element that is both an island and an
/// extension. Each line names where it is: the elements from the node given down, each by its
/// tag and its slot in brackets, joined by `>`.
///
/// ```
/// use navree_ui::{Node, SafeHtml, el, kit};
///
/// let card = el("div")
///     .slot("geo:card")
///     .child(Node::Raw(SafeHtml::trusted("<b>Ludhiana</b>")));
/// assert_eq!(
///     kit::problems(&card.into()),
///     ["raw markup at div[geo:card]: only an icon's shapes, cleaned stored HTML, or a toolkit part's own text is written as it is"]
/// );
/// ```
pub fn problems(node: &Node) -> Vec<String> {
    let mut found = Vec::new();
    walk(node, &Above::default(), 0, &mut found);
    found
}

/// Fails the test with every line of [`problems`], when there are any.
///
/// ```
/// use navree_ui::{el, kit};
///
/// kit::assert_component(&el("div").slot("geo:card").child("Ludhiana").into());
/// ```
#[track_caller]
pub fn assert_component(node: &Node) {
    let found = problems(node);
    assert!(found.is_empty(), "{}", found.join("\n"));
}

/// `node` again inside an element marked `data-ui-stored`: how stored content, such as a message
/// a reader wrote, would carry it. A browser test draws the twin beside the node and checks that
/// the runtime acts on the node and on nothing in the twin.
///
/// ```
/// use navree_ui::{el, kit, render};
///
/// let twin = kit::stored_twin(&el("p").child("Meera Sharma").into());
/// assert_eq!(render(&twin), "<div data-ui-stored><p>Meera Sharma</p></div>");
/// ```
pub fn stored_twin(node: &Node) -> Node {
    el("div")
        .flag("data-ui-stored", true)
        .child(node.clone())
        .into()
}

/// One step of [`problems`].
fn walk(node: &Node, above: &Above<'_>, depth: u32, found: &mut Vec<String>) {
    if depth > MAX_DEPTH {
        return;
    }
    match node {
        Node::Element(element) => element_problems(element, above, depth, found),
        Node::Fragment(nodes) => {
            for child in nodes {
                walk(child, above, depth + 1, found);
            }
        }
        Node::Raw(markup) => raw_problems(markup.as_str(), above, found),
        Node::Static(markup) => raw_problems(markup, above, found),
        Node::Empty | Node::Text(_) | Node::Int(_) => {}
    }
}

/// Where `above` is, for a line: its path, or the top of the node given.
fn at(path: &str) -> &str {
    if path.is_empty() {
        "the top of the node"
    } else {
        path
    }
}

/// The line for an address that is not a served file.
fn not_served(path: &str, value: &str, attribute: &str) -> String {
    format!(
        "{} names {value} in {attribute}, which is not a served extension file",
        at(path)
    )
}

/// The text of attribute `name`, when it has one.
fn text<'a>(element: &'a Element, name: &str) -> Option<&'a str> {
    match element.get(name)? {
        AttrValue::Static(text) => Some(*text),
        AttrValue::Text(text) => Some(text),
        AttrValue::Flag | AttrValue::Int(_) => None,
    }
}

/// The namespace an extension file's address names, when it is the address of a served file.
fn namespace_of(address: &str) -> Option<&str> {
    if !is_script_address(address) && !is_sheet_address(address) {
        return None;
    }
    let file = address.strip_prefix(EXTENSION_PATH)?.strip_prefix('/')?;
    file.split_once('.').map(|(namespace, _)| namespace)
}

/// Whether `slot` is `namespace`, a colon and a word of 1 to 64 lower-case letters, digits and
/// `-`.
fn is_extension_slot(slot: &str, namespace: &str) -> bool {
    slot.strip_prefix(namespace)
        .and_then(|rest| rest.strip_prefix(':'))
        .is_some_and(|word| {
            !word.is_empty()
                && word.len() <= 64
                && word
                    .bytes()
                    .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'-'))
        })
}

/// What raw markup under `above` breaks: markup the rules do not take, and an address in it
/// that is not a served file.
fn raw_problems(markup: &str, above: &Above<'_>, found: &mut Vec<String>) {
    let taken = above.tag == Some("svg")
        || above.stored_parent
        || above.slot.is_some_and(|slot| !slot.contains(':'));
    if !taken {
        found.push(format!(
            "raw markup at {}: only an icon's shapes, cleaned stored HTML, or a toolkit part's own text is written as it is",
            at(above.path)
        ));
    }
    // The page's head reads the addresses written in raw text inside an extension's root, as it
    // reads the attributes of its elements.
    if above.inside && !above.stored {
        for (attribute, value) in markup_values(markup) {
            let served = if attribute == "data-ui-ext-sheet" {
                is_sheet_address(value)
            } else {
                is_script_address(value)
            };
            if !served {
                found.push(not_served(above.path, value, attribute));
            }
        }
    }
}

/// What `element` breaks, and then what is below it.
fn element_problems(element: &Element, above: &Above<'_>, depth: u32, found: &mut Vec<String>) {
    let slot = text(element, "data-slot");
    let segment = match slot {
        Some(slot) => format!("{}[{slot}]", element.tag()),
        None => element.tag().to_owned(),
    };
    let path = if above.path.is_empty() {
        segment
    } else {
        format!("{} > {segment}", above.path)
    };
    let script = text(element, "data-ui-ext");
    let sheet = text(element, "data-ui-ext-sheet");

    // The root of an extension: a slot of its namespace, which the stylesheet is scoped to.
    if script.is_some() || sheet.is_some() {
        let namespace = script
            .and_then(namespace_of)
            .or_else(|| sheet.and_then(namespace_of));
        match (slot, namespace) {
            (None, _) => found.push(format!("the extension root at {path} has no data-slot")),
            (Some(slot), Some(namespace)) if !is_extension_slot(slot, namespace) => {
                found.push(format!(
                    "the extension root at {path} has data-slot=\"{slot}\", which is not {namespace}:<word>"
                ));
            }
            _ => {}
        }
    }
    if let Some(value) = script
        && !is_script_address(value)
    {
        found.push(not_served(&path, value, "data-ui-ext"));
    }
    if let Some(value) = sheet
        && !is_sheet_address(value)
    {
        found.push(not_served(&path, value, "data-ui-ext-sheet"));
    }
    if let Some(props) = text(element, "data-ui-props") {
        if serde_json::from_str::<serde::de::IgnoredAny>(props).is_err() {
            found.push(format!("{path}: data-ui-props is not JSON"));
        }
        if props.len() > MOST_PROPS {
            found.push(format!(
                "{path}: data-ui-props is {} KB; keep props under 64 KB and fetch the rest from a route",
                props.len().div_ceil(1024)
            ));
        }
    }
    if element.get("data-ui-island").is_some() && element.get("data-ui-ext").is_some() {
        found.push(format!("{path} is both an island and an extension"));
    }

    let stored_here = element.get("data-ui-stored").is_some();
    let names_files = script.is_some()
        || sheet.is_some()
        || element
            .uses
            .meets(Uses::EXTENSIONS.with(Uses::EXTENSION_SHEETS));
    let below = Above {
        path: &path,
        tag: Some(element.tag()),
        stored_parent: stored_here,
        slot: slot.or(above.slot),
        inside: above.inside || names_files,
        stored: above.stored || stored_here,
    };
    for child in element.child_nodes() {
        walk(child, &below, depth + 1, found);
    }
}
