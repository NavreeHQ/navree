//! `SplitPane`: panes side by side, or one at a time on a narrow screen.
//!
//! A split of two panes is a primary pane and its detail. A split of more is
//! the first pane and, as its detail, a split of the others: a list, then a
//! conversation with its customer beside it. Each is a `pane` that says the part
//! it plays and whether it is the one a narrow screen shows. The page does not name
//! the parts; the split does by where a pane stands: the first is the list a reader
//! chooses from, the last of three or more is the inspector, any other is the
//! detail of what was chosen. What a browser adds to arrange the panes (their
//! sizes, the handles between them, which collapse below a width, the buttons that
//! move between them) is a window's way; a system's own navigation moves between
//! panes, and its split view sizes them.

use super::super::lower::{Cx, N, Parts, Read, attr, elements, fail, has, hooks, id, slot};
use crate::node::Element;
use navree_native::node as n;

/// The hooks the root of a split carries that this reader answers for.
pub(crate) const HOOKS: &[&str] = &["data-ui-split", "data-ui-split-save"];

/// `SplitPane`.
pub(crate) fn split(cx: &mut Cx, element: &Element) -> Read {
    let mut panes: Vec<n::Pane> = Vec::new();
    for part in elements(element)? {
        match slot(part).as_deref() {
            Some("split-pane") => panes.push(pane(cx, part)?),
            // The bar a person drags to size the panes.
            Some("split-handle") => hooks(part, &[])?,
            _ => return fail("something that is not a pane in a split"),
        }
    }
    if has(element, "data-ui-split-save") {
        cx.dropped.push("split-sizes".to_owned());
    }
    if attr(element, "data-orientation").as_deref() == Some("vertical") {
        // Panes always one over another are not a primary and its detail.
        return Ok(vec![N::Stack(n::Stack {
            id: id(element),
            children: panes.into_iter().map(N::Pane).collect(),
            ..n::Stack::default()
        })]);
    }
    if panes.len() < 2 {
        return fail("a split of fewer than two panes");
    }
    let last = panes.len() - 1;
    for (at, pane) in panes.iter_mut().enumerate() {
        pane.role = match at {
            0 => n::PaneRole::List,
            at if at == last && last >= 2 => n::PaneRole::Inspector,
            _ => n::PaneRole::Detail,
        };
    }
    let mut from_the_last = panes.into_iter().rev();
    let Some(last) = from_the_last.next() else {
        return fail("a split with no panes");
    };
    let mut detail = N::Pane(last);
    for primary in from_the_last {
        detail = N::Split(n::Split {
            id: None,
            primary: Box::new(N::Pane(primary)),
            detail: Box::new(detail),
        });
    }
    // The split has the id the page gave it; the inner ones have none.
    if let N::Split(outer) = &mut detail {
        outer.id = id(element);
    }
    Ok(vec![detail])
}

/// One pane: its header, what it holds, its footer.
fn pane(cx: &mut Cx, section: &Element) -> Read<n::Pane> {
    hooks(section, &[])?;
    let mut parts = Parts::of(section);
    let header = parts.slot("split-pane-header");
    let body = parts.slot("split-pane-body");
    let footer = parts.slot("split-pane-footer");
    parts.done()?;
    let Some(body) = body else {
        return fail("a pane with no body");
    };
    hooks(body, &[])?;
    let mut inside = Parts::of(body);
    let content = inside.slot("split-pane-content");
    inside.done()?;
    let Some(content) = content else {
        return fail("a pane with no content");
    };
    let mut children = Vec::new();
    if let Some(header) = header {
        hooks(header, &[])?;
        children.extend(cx.of(header));
    }
    hooks(content, &[])?;
    children.extend(cx.of(content));
    if let Some(footer) = footer {
        hooks(footer, &[])?;
        children.extend(cx.of(footer));
    }
    Ok(n::Pane {
        id: id(section),
        title: attr(section, "aria-label").map(std::borrow::Cow::into_owned),
        current: has(section, "data-split-current"),
        children,
        ..n::Pane::default()
    })
}

#[cfg(test)]
mod tests {
    use super::super::testing::{misses, read, tamper};
    use super::*;
    use crate::node::Node;
    use crate::{Button, Pane, SplitPane, Text, icons};

    fn pane_of(node: &N) -> &n::Pane {
        let N::Pane(pane) = node else {
            panic!("not a pane: {node:?}");
        };
        pane
    }

    #[test]
    fn two_panes_are_a_primary_and_its_detail() {
        let N::Split(split) = read(
            SplitPane::new("mail")
                .pane(
                    Pane::new("list", "Conversations")
                        .size(30)
                        .body(Text::new("rows")),
                )
                .pane(
                    Pane::fill("thread", "Conversation")
                        .header(Text::new("Refund for CS-20481"))
                        .body(Text::new("messages"))
                        .footer(Text::new("composer")),
                )
                .current("thread"),
        ) else {
            panic!("not a split");
        };
        assert_eq!(split.id.as_deref(), Some("mail"));
        let list = pane_of(&split.primary);
        assert_eq!(list.id.as_deref(), Some("list"));
        assert_eq!(list.role, n::PaneRole::List);
        assert_eq!(list.title.as_deref(), Some("Conversations"));
        assert!(!list.current);
        assert_eq!(list.children, [N::text("rows")]);
        let thread = pane_of(&split.detail);
        assert_eq!(thread.id.as_deref(), Some("thread"));
        // The pane a narrow screen shows.
        assert_eq!(thread.role, n::PaneRole::Detail);
        assert!(thread.current);
        assert_eq!(
            thread.children,
            [
                N::text("Refund for CS-20481"),
                N::text("messages"),
                N::text("composer")
            ]
        );
    }

    #[test]
    fn more_panes_are_the_first_and_a_split_of_the_rest() {
        let N::Split(split) = read(
            SplitPane::new("mail")
                .pane(Pane::new("list", "Conversations").body(Text::new("rows")))
                .pane(Pane::fill("thread", "Conversation").body(Text::new("messages")))
                .pane(
                    Pane::new("customer", "Customer")
                        .collapsible()
                        .body(Text::new("who")),
                ),
        ) else {
            panic!("not a split");
        };
        assert_eq!(pane_of(&split.primary).id.as_deref(), Some("list"));
        let N::Split(rest) = &*split.detail else {
            panic!("not a split of the rest");
        };
        assert_eq!(rest.id, None);
        assert_eq!(pane_of(&split.primary).role, n::PaneRole::List);
        assert_eq!(pane_of(&rest.primary).id.as_deref(), Some("thread"));
        assert_eq!(pane_of(&rest.primary).role, n::PaneRole::Detail);
        assert_eq!(pane_of(&rest.detail).id.as_deref(), Some("customer"));
        assert_eq!(pane_of(&rest.detail).role, n::PaneRole::Inspector);
    }

    #[test]
    fn what_a_browser_adds_to_arrange_the_panes_is_left_out_and_said() {
        let node: Node = SplitPane::new("mail")
            .save_to("/panes")
            .pane(Pane::new("list", "List").body(Text::new("rows")))
            .pane(
                Pane::fill("thread", "Thread")
                    .header(SplitPane::stacked_only(
                        Button::ghost("Back").icon(icons::ARROW_LEFT).href("/inbox"),
                    ))
                    .body(Text::new("messages")),
            )
            .current("thread")
            .into();
        let lowered = crate::native::lower(&node);
        assert!(lowered.is_native(), "{:?}", lowered.misses);
        let N::Split(split) = &lowered.node else {
            panic!("not a split");
        };
        // The way back to the list, shown only while stacked, is not in the pane.
        assert_eq!(pane_of(&split.detail).children, [N::text("messages")]);
        let mut dropped = lowered.dropped.clone();
        dropped.sort();
        assert_eq!(dropped, ["split-only", "split-sizes"]);
    }

    #[test]
    fn the_button_that_moves_between_panes_is_the_clients_navigation() {
        let node: Node = SplitPane::new("mail")
            .pane(Pane::new("list", "List").body(Text::new("rows")))
            .pane(
                Pane::fill("thread", "Thread")
                    .header(SplitPane::toggle(
                        Button::ghost("Customer").href("/inbox/1?pane=customer"),
                        "customer",
                    ))
                    .body(Text::new("messages")),
            )
            .into();
        let lowered = crate::native::lower(&node);
        assert!(lowered.is_native(), "{:?}", lowered.misses);
        let N::Split(split) = &lowered.node else {
            panic!("not a split");
        };
        assert_eq!(pane_of(&split.detail).children, [N::text("messages")]);
    }

    #[test]
    fn panes_one_over_another_are_a_stack() {
        let N::Stack(stack) = read(
            SplitPane::new("tools")
                .vertical()
                .pane(Pane::new("top", "Top").body(Text::new("a")))
                .pane(Pane::fill("bottom", "Bottom").body(Text::new("b"))),
        ) else {
            panic!("not a stack");
        };
        assert_eq!(stack.id.as_deref(), Some("tools"));
        assert_eq!(stack.children.len(), 2);
        assert!(stack.children.iter().all(|pane| matches!(pane, N::Pane(_))));
    }

    #[test]
    fn a_hook_no_reader_knows_inside_a_split_is_still_a_miss() {
        for name in ["split", "split-pane", "split-pane-body", "split-handle"] {
            let node: Node = SplitPane::new("mail")
                .pane(Pane::new("list", "List").body(Text::new("rows")))
                .pane(Pane::fill("thread", "Thread").body(Text::new("messages")))
                .into();
            assert_eq!(
                misses(tamper(node, name, "data-ui-wizard")),
                ["hook:data-ui-wizard"],
                "{name}"
            );
        }
    }
}
