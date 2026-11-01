//! One pass from [`Node`] to one `String`: a walk to size the buffer and find
//! what the page must load, then a walk to write. No templates are parsed and
//! nothing is allocated per node.

use std::fmt::Write as _;

use crate::html::escape_into;
use crate::node::{AttrValue, Element, Node, Uses};

/// The deepest tree written. A tree this deep is a bug in whoever built it:
/// what's below is dropped rather than risk the stack.
const MAX_DEPTH: u32 = 512;

/// What a walk of the tree found.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Measure {
    /// About how many bytes the markup needs, before escaping.
    pub bytes: usize,
    /// What the page must load.
    pub uses: Uses,
}

/// Walks `node`.
pub fn measure(node: &Node) -> Measure {
    let mut found = Measure::default();
    measure_into(node, &mut found, 0);
    found
}

fn measure_into(node: &Node, found: &mut Measure, depth: u32) {
    if depth > MAX_DEPTH {
        return;
    }
    match node {
        Node::Empty => {}
        Node::Text(text) => found.bytes += text.len(),
        Node::Int(_) => found.bytes += 20,
        Node::Raw(html) => found.bytes += html.as_str().len(),
        Node::Static(html) => found.bytes += html.len(),
        Node::Element(element) => {
            found.uses = found.uses.with(element.uses);
            found.bytes += 2 * element.tag.len() + 5;
            for attr in &element.attrs {
                found.bytes += attr.name.len()
                    + 4
                    + match &attr.value {
                        AttrValue::Flag => 0,
                        AttrValue::Static(text) => text.len(),
                        AttrValue::Text(text) => text.len(),
                        AttrValue::Int(_) => 20,
                    };
            }
            for child in &element.children {
                measure_into(child, found, depth + 1);
            }
        }
        Node::Fragment(nodes) => {
            for child in nodes {
                measure_into(child, found, depth + 1);
            }
        }
    }
}

/// Elements with no closing tag.
#[inline]
fn is_void(tag: &str) -> bool {
    matches!(
        tag,
        "input" | "img" | "br" | "hr" | "meta" | "link" | "area" | "base" | "col" | "embed" | "source" | "track" | "wbr"
    )
}

/// `node` as HTML.
///
/// ```
/// use navree_ui::{el, render};
///
/// let node = el("a").href("/x").attr("title", "a\"b".to_owned()).child("<hi>").into();
/// assert_eq!(render(&node), r#"<a href="/x" title="a&quot;b">&lt;hi&gt;</a>"#);
/// ```
pub fn render(node: &Node) -> String {
    let mut out = String::with_capacity(measure(node).bytes);
    render_into(&mut out, node);
    out
}

/// Appends `node` to `out` as HTML.
pub fn render_into(out: &mut String, node: &Node) {
    write_node(out, node, 0);
}

fn write_node(out: &mut String, node: &Node, depth: u32) {
    if depth > MAX_DEPTH {
        return;
    }
    match node {
        Node::Empty => {}
        Node::Text(text) => escape_into(out, text),
        Node::Int(value) => {
            let _ = write!(out, "{value}");
        }
        Node::Raw(html) => out.push_str(html.as_str()),
        Node::Static(html) => out.push_str(html),
        Node::Element(element) => write_element(out, element, depth),
        Node::Fragment(nodes) => {
            for child in nodes {
                write_node(out, child, depth + 1);
            }
        }
    }
}

fn write_element(out: &mut String, element: &Element, depth: u32) {
    out.push('<');
    out.push_str(element.tag);
    for attr in &element.attrs {
        out.push(' ');
        out.push_str(attr.name);
        match &attr.value {
            AttrValue::Flag => {}
            AttrValue::Static(text) => {
                out.push_str("=\"");
                escape_into(out, text);
                out.push('"');
            }
            AttrValue::Text(text) => {
                out.push_str("=\"");
                escape_into(out, text);
                out.push('"');
            }
            AttrValue::Int(value) => {
                let _ = write!(out, "=\"{value}\"");
            }
        }
    }
    out.push('>');
    if is_void(element.tag) {
        return;
    }
    for child in &element.children {
        write_node(out, child, depth + 1);
    }
    out.push_str("</");
    out.push_str(element.tag);
    out.push('>');
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::el;
    use crate::html::SafeHtml;

    #[test]
    fn writes_elements_attributes_and_text() {
        let node: Node = el("div")
            .id("a")
            .flag("hidden", true)
            .flag("inert", false)
            .attr("tabindex", 0_i64)
            .children(("<x>", el("br"), el("span").child(5_i64)))
            .into();
        assert_eq!(render(&node), r#"<div id="a" hidden tabindex="0">&lt;x&gt;<br><span>5</span></div>"#);
    }

    #[test]
    fn attribute_values_cannot_break_out() {
        let evil = "\"><script>alert(1)</script>";
        let node: Node = el("a").attr("title", evil.to_owned()).child(evil.to_owned()).into();
        let html = render(&node);
        assert!(!html.contains("<script"), "{html}");
        assert_eq!(html.matches('"').count(), 2);
    }

    #[test]
    fn raw_markup_is_the_only_way_past_escaping() {
        assert_eq!(render(&Node::Raw(SafeHtml::trusted("<b>x</b>"))), "<b>x</b>");
        assert_eq!(render(&Node::text("<b>x</b>")), "&lt;b&gt;x&lt;/b&gt;");
    }

    #[test]
    fn the_size_estimate_covers_ordinary_markup() {
        let node: Node = el("table")
            .children(
                (0..100)
                    .map(|row| {
                        el("tr").children((0..5).map(|cell| el("td").slot("cell").child(row * cell)).collect::<Vec<_>>())
                    })
                    .collect::<Vec<_>>(),
            )
            .into();
        let html = render(&node);
        let estimate = measure(&node).bytes;
        assert!(estimate >= html.len(), "{estimate} < {}", html.len());
        assert!(estimate < html.len() * 2, "{estimate} vs {}", html.len());
    }

    #[test]
    fn a_tree_too_deep_is_cut_not_a_crash() {
        let mut node = Node::text("bottom");
        for _ in 0..2000 {
            node = el("div").child(node).into();
        }
        assert!(!render(&node).contains("bottom"));
    }

    #[test]
    fn measure_finds_what_a_page_uses() {
        let node: Node = el("div").child(el("p").uses(Uses::SIDEBAR)).into();
        assert_eq!(measure(&node).uses, Uses::SIDEBAR);
    }
}
