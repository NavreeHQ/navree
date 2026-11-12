//! `Accordion` and `Collapsible`: parts that open and close under their
//! labels.
//!
//! Both are the page's own `<details>`, which opens and closes with no script,
//! so each section is a disclosure of the vocabulary. An accordion that lets
//! one section open at a time (`name` on its sections) is read as sections
//! that open on their own: the other part of that rule is the browser's.

use super::super::lower::{Cx, N, Parts, Read, elements, fail, has, hooks, id, slot, text_of};
use crate::node::Element;
use navree_native::node as n;

/// `Accordion`: its sections, one over another.
pub(crate) fn accordion(cx: &mut Cx, element: &Element) -> Read {
    let mut sections = Vec::new();
    for section in elements(element)? {
        if slot(section).as_deref() != Some("accordion-item") {
            return fail("something that is not a section in an accordion");
        }
        if has(section, "name") {
            cx.dropped.push("accordion-exclusive".to_owned());
        }
        sections.push(N::Disclosure(disclosure(
            cx,
            section,
            "accordion-trigger",
            "accordion-content",
        )?));
    }
    Ok(vec![N::Stack(n::Stack {
        id: id(element),
        children: sections,
        ..n::Stack::default()
    })])
}

/// `Collapsible`: one section.
pub(crate) fn collapsible(cx: &mut Cx, element: &Element) -> Read {
    disclosure(cx, element, "collapsible-trigger", "collapsible-content")
        .map(|section| vec![N::Disclosure(section)])
}

/// A `<details>` with the trigger and the content that components give it.
fn disclosure(
    cx: &mut Cx,
    element: &Element,
    trigger: &str,
    content: &str,
) -> Read<n::Disclosure> {
    if element.tag() != "details" {
        return fail(format!("a section that is a <{}>", element.tag()));
    }
    let mut parts = Parts::of(element);
    let label = parts.slot(trigger);
    let body = parts.slot(content);
    parts.done()?;
    let (Some(label), Some(body)) = (label, body) else {
        return fail("a section with no trigger or no content");
    };
    hooks(element, &[])?;
    hooks(label, &[])?;
    hooks(body, &[])?;
    Ok(n::Disclosure {
        id: id(element),
        label: text_of(label),
        open: has(element, "open"),
        children: cx.of(body),
    })
}

#[cfg(test)]
mod tests {
    use super::super::testing::{misses, read, tamper};
    use super::*;
    use crate::node::Node;
    use crate::{Accordion, Card, Collapsible, Text};

    fn section(node: &N) -> &n::Disclosure {
        let N::Disclosure(section) = node else {
            panic!("not a section: {node:?}");
        };
        section
    }

    #[test]
    fn an_accordion_is_sections_that_open_under_their_labels() {
        let N::Stack(stack) = read(
            Accordion::new()
                .item("How long does delivery take?", Text::new("Three to five days."))
                .open_item(
                    "Can I return it?",
                    Card::titled("Returns").body(Text::new("Within 30 days.")),
                ),
        ) else {
            panic!("not a stack");
        };
        let [first, second] = stack.children.as_slice() else {
            panic!("not two sections");
        };
        assert_eq!(section(first).label, "How long does delivery take?");
        assert!(!section(first).open);
        assert_eq!(section(first).children, [N::text("Three to five days.")]);
        assert_eq!(section(second).label, "Can I return it?");
        assert!(section(second).open);
        assert!(matches!(section(second).children.as_slice(), [N::Section(_)]));
    }

    #[test]
    fn one_at_a_time_is_the_browsers_rule_and_is_said() {
        let node: Node = Accordion::new()
            .exclusive("faq")
            .item("a", Text::new("b"))
            .into();
        let lowered = crate::native::lower(&node);
        assert!(lowered.is_native(), "{:?}", lowered.misses);
        assert_eq!(lowered.dropped, ["accordion-exclusive"]);
    }

    #[test]
    fn a_collapsible_is_one_section() {
        let node = read(Collapsible::new("More details").open(true).body(Text::new("Hidden")));
        let section = section(&node);
        assert_eq!(section.label, "More details");
        assert!(section.open);
        assert_eq!(section.children, [N::text("Hidden")]);
    }

    #[test]
    fn a_hook_no_reader_knows_inside_either_is_still_a_miss() {
        let accordion: Node = Accordion::new().item("a", Text::new("b")).into();
        for name in ["accordion", "accordion-item", "accordion-content"] {
            assert_eq!(
                misses(tamper(accordion.clone(), name, "data-ui-wizard")),
                ["hook:data-ui-wizard"],
                "{name}"
            );
        }
        let collapsible: Node = Collapsible::new("More").body(Text::new("b")).into();
        assert_eq!(
            misses(tamper(collapsible, "collapsible", "data-ui-wizard")),
            ["hook:data-ui-wizard"]
        );
    }
}
