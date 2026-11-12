//! `Sortable`: lists whose items move, alone or between lists, as a board.
//!
//! A sortable is read as its lists: each a titled section of rows, one row an
//! item with what the page shows for it. Every item that can move carries the
//! form that posts its move, in the page, for a person with no script: which
//! list, and which place in it. That form is read as it is, under the item
//! (`Move`), so an item moves on a client that has no drag. Dragging, with its
//! handle, its keys and its announcements, is a pointer's and a script's; what a
//! native drag would need from the vocabulary is written in the notes.

use std::borrow::Cow;

use navree_native::node as n;

use super::super::lower::{
    Control, Cx, N, Parts, Read, attr, elements, fail, flat, has, hooks, hidden_in, id, slot,
    text_of,
};
use crate::node::{Element, Node};

/// The hooks the root of a sortable carries that this reader answers for.
pub(crate) const HOOKS: &[&str] = &["data-ui-sortable"];

/// `Sortable`.
pub(crate) fn sortable(cx: &mut Cx, element: &Element) -> Read {
    let mut parts = Parts::of(element);
    // The help for the keys, and what is announced as items move.
    let _help = parts.slot("sortable-help");
    let _status = parts.slot("sortable-status");
    let lists = parts.slot("sortable-lists");
    parts.done()?;
    let Some(lists) = lists else {
        return fail("a sortable with no lists");
    };
    hooks(lists, &[])?;
    let mut sections = Vec::new();
    for list in elements(lists)? {
        if slot(list).as_deref() != Some("sortable-list") {
            return fail("something that is not a list in a sortable");
        }
        sections.push(list_of(cx, list)?);
    }
    let board = attr(element, "data-layout").as_deref() == Some("board");
    Ok(vec![if board {
        N::Grid(n::Grid {
            id: id(element),
            children: sections,
            ..n::Grid::default()
        })
    } else {
        N::Stack(n::Stack {
            id: id(element),
            children: sections,
            ..n::Stack::default()
        })
    }])
}

/// One list: its title and count, its items, what stands under them.
fn list_of(cx: &mut Cx, list: &Element) -> Read<N> {
    hooks(list, &[])?;
    let mut parts = Parts::of(list);
    let header = parts.slot("sortable-list-header");
    let items = parts.slot("sortable-items");
    let empty = parts.slot("sortable-empty");
    let more = parts.slot("sortable-more");
    let footer = parts.slot("sortable-list-footer");
    parts.done()?;
    let (Some(header), Some(items)) = (header, items) else {
        return fail("a list with no header or no items");
    };
    let mut section = n::Section {
        id: id(list),
        ..n::Section::default()
    };
    heading(cx, header, &mut section)?;
    hooks(items, &[])?;
    let mut rows = Vec::new();
    for item in elements(items)? {
        if slot(item).as_deref() != Some("sortable-item") {
            return fail("something that is not an item in a list");
        }
        rows.push(row_of(cx, item)?);
    }
    let none = rows.is_empty();
    let mut list_node = n::List {
        id: id(items),
        children: rows,
        ..n::List::default()
    };
    let mut after = Vec::new();
    if let Some(more) = more {
        match more_of(more)? {
            More::Next(href) => list_node.more = Some(href),
            More::Note(note) => after.push(N::Text(n::Text {
                value: note,
                style: n::TextStyle::Caption,
                muted: true,
                ..n::Text::default()
            })),
        }
    }
    section.children.push(N::List(list_node));
    if none && let Some(empty) = empty {
        section.children.push(N::Text(n::Text {
            value: text_of(empty),
            muted: true,
            ..n::Text::default()
        }));
    }
    section.children.extend(after);
    if let Some(footer) = footer {
        section.footer = cx.of(footer);
    }
    Ok(N::Section(section))
}

/// A list's title, how many it holds, and what the page puts beside the title.
fn heading(cx: &mut Cx, header: &Element, section: &mut n::Section) -> Read<()> {
    let mut parts = Parts::of(header);
    let count = parts.slot("sortable-count");
    let actions = parts.slot("sortable-list-actions");
    let title = parts.slot("sortable-list-title");
    parts.done()?;
    let Some(title) = title else {
        return fail("a list with no title");
    };
    section.title = Some(text_of(title));
    // The number that is shown; the words for a screen reader say it again.
    section.subtitle = count.and_then(shown_count);
    if let Some(actions) = actions {
        section.actions = cx.of(actions);
    }
    Ok(())
}

/// The number a count shows: the span the page marks as the count.
fn shown_count(count: &Element) -> Option<String> {
    flat(count.child_nodes()).into_iter().find_map(|node| match node {
        Node::Element(span) if has(span, "data-count") => Some(text_of(span)),
        _ => None,
    })
}

/// Where the rest of a list is.
enum More {
    /// The address of the next items.
    Next(String),
    /// Words that say the list goes on.
    Note(String),
}

/// A list that holds only its start: a form that asks for more, or a note.
fn more_of(more: &Element) -> Read<More> {
    if more.tag() != "form" {
        return Ok(More::Note(text_of(more)));
    }
    hooks(more, &["data-ui-form", "data-ui-autoload"])?;
    let action = attr(more, "action").map(Cow::into_owned).unwrap_or_default();
    let mut fields = Vec::new();
    hidden_in(more.child_nodes(), &mut fields);
    let query: Vec<(&str, &str)> = fields
        .iter()
        .map(|field| (field.name.as_str(), field.value.as_str()))
        .collect();
    let joiner = if action.contains('?') { '&' } else { '?' };
    Ok(More::Next(format!(
        "{action}{joiner}{}",
        crate::form::encode(&query)
    )))
}

/// One item: what the page shows for it, and the form that moves it.
fn row_of(cx: &mut Cx, item: &Element) -> Read<N> {
    hooks(item, &[])?;
    let mut parts = Parts::of(item);
    // The grip a pointer takes hold of.
    let handle = parts.slot("sortable-handle");
    let body = parts.slot("sortable-body");
    let moves = parts.slot("sortable-move");
    parts.done()?;
    let Some(body) = body else {
        return fail("an item with no body");
    };
    if let Some(handle) = handle {
        hooks(handle, &[])?;
        cx.dropped.push("sortable-handle".to_owned());
    }
    let name = attr(item, "data-name")
        .map(Cow::into_owned)
        .unwrap_or_default();
    if name.is_empty() {
        return fail("an item with no name");
    }
    hooks(body, &[])?;
    let mut row = n::Row {
        id: id(item),
        title: name.clone(),
        ..n::Row::default()
    };
    // The body of an item that has none of its own is its name, said again.
    if !is_only_the_name(body, &name) {
        row.detail = cx.of(body);
    }
    if let Some(locked) = attr(item, "data-locked") {
        let why = locked.trim();
        if !why.is_empty() {
            row.detail.push(N::Text(n::Text {
                value: why.to_owned(),
                style: n::TextStyle::Caption,
                muted: true,
                ..n::Text::default()
            }));
        }
    }
    if let Some(moves) = moves {
        row.detail.push(move_of(cx, moves)?);
    }
    Ok(N::Row(row))
}

/// Whether a body is the one span an item with no body of its own gets.
fn is_only_the_name(body: &Element, name: &str) -> bool {
    match flat(body.child_nodes()).as_slice() {
        [Node::Element(span)] => {
            span.tag() == "span"
                && span.attrs().is_empty()
                && text_of(span) == name
                && span.child_nodes().iter().all(|node| matches!(node, Node::Text(_)))
        }
        _ => false,
    }
}

/// The form that moves an item, under its summary.
fn move_of(cx: &mut Cx, moves: &Element) -> Read<N> {
    if moves.tag() != "details" {
        return fail("a move that is not a <details>");
    }
    hooks(moves, &[])?;
    let mut parts = Parts::of(moves);
    let summary = parts.slot("sortable-move-summary");
    let form = parts.slot("sortable-form");
    parts.done()?;
    let (Some(summary), Some(form)) = (summary, form) else {
        return fail("a move with no summary or no form");
    };
    hooks(summary, &[])?;
    hooks(form, &["data-ui-form", "data-ui-move", "data-ui-deadline"])?;
    let mut hidden = Vec::new();
    hidden_in(form.child_nodes(), &mut hidden);
    // The labels name the fields beside them, by the field's id.
    let mut words: Vec<(String, String)> = Vec::new();
    let mut children = Vec::new();
    for part in elements(form)? {
        match (part.tag(), slot(part).as_deref()) {
            ("input", _) if attr(part, "type").as_deref() == Some("hidden") => {}
            ("label", Some("sortable-move-field")) => {
                let Some(target) = attr(part, "for") else {
                    return fail("a label of a move that names nothing");
                };
                words.push((target.into_owned(), text_of(part)));
            }
            ("select" | "input", _) => {
                let control = cx.as_form(|cx| cx.control(part))?;
                let Control::Field(mut field) = control else {
                    return fail("a part of a move that is not a field");
                };
                if let Some(name) = id(part)
                    && let Some((_, label)) = words.iter().find(|(target, _)| *target == name)
                {
                    field.label = Some(label.clone());
                }
                children.push(N::Field(*field));
            }
            ("button", _) => {
                let mut read = Vec::new();
                cx.as_form(|cx| cx.element(part, &mut read));
                children.extend(read);
            }
            _ => return fail("a part of a move no reader here knows"),
        }
    }
    Ok(N::Disclosure(n::Disclosure {
        id: id(moves),
        label: text_of(summary),
        open: false,
        children: vec![N::Form(n::Form {
            id: id(form),
            method: n::Method::Post,
            action: attr(form, "action").map(Cow::into_owned).unwrap_or_default(),
            hidden,
            children,
        })],
    }))
}

#[cfg(test)]
mod tests {
    use navree_native::Action;
    use navree_native::input as i;

    use super::super::testing::{misses, read, tamper};
    use super::*;
    use crate::{SortItem, SortList, Sortable, Text};

    fn board() -> Sortable {
        Sortable::new("board", "Cards")
            .post("/cards/move")
            .csrf("tok")
            .list(
                SortList::new("todo", "To do")
                    .item(SortItem::new("c1", "Fix login"))
                    .item(
                        SortItem::new("c2", "Add search").body(Text::new("Search the whole board")),
                    )
                    .item(SortItem::new("c3", "Pay the vendor").locked("Waiting on review")),
            )
            .list(SortList::new("done", "Done"))
    }

    fn sections(node: &N) -> &[N] {
        match node {
            N::Stack(stack) => &stack.children,
            N::Grid(grid) => &grid.children,
            other => panic!("not lists: {other:?}"),
        }
    }

    fn rows(section: &N) -> &[N] {
        let N::Section(section) = section else {
            panic!("not a section");
        };
        let Some(N::List(list)) = section
            .children
            .iter()
            .find(|node| matches!(node, N::List(_)))
        else {
            panic!("no list");
        };
        &list.children
    }

    #[test]
    fn lists_are_sections_of_rows_named_for_their_items() {
        let node = read(board());
        let [todo, done] = sections(&node) else {
            panic!("not two lists");
        };
        let N::Section(first) = todo else {
            panic!("not a section");
        };
        assert_eq!(first.id.as_deref(), Some("board-list-todo"));
        assert_eq!(first.title.as_deref(), Some("To do"));
        assert_eq!(first.subtitle.as_deref(), Some("3"));
        let [N::Row(login), N::Row(search), N::Row(vendor)] = rows(todo) else {
            panic!("not three rows");
        };
        assert_eq!(login.id.as_deref(), Some("board-item-c1"));
        assert_eq!(login.title, "Fix login");
        // An item with no body of its own is its name, not said twice.
        assert!(
            login.detail.iter().all(|part| !matches!(part, N::Text(_))),
            "{:?}",
            login.detail
        );
        assert_eq!(search.title, "Add search");
        assert!(search.detail.contains(&N::text("Search the whole board")));
        // A locked item says why, and has no form to move it.
        assert!(
            vendor
                .detail
                .iter()
                .any(|part| matches!(part, N::Text(text) if text.value == "Waiting on review"))
        );
        assert!(
            !vendor
                .detail
                .iter()
                .any(|part| matches!(part, N::Disclosure(_)))
        );
        // A list with nothing in it says so.
        let N::Section(empty) = done else {
            panic!("not a section");
        };
        assert!(empty.children.iter().any(
            |part| matches!(part, N::Text(text) if text.value == "Nothing here yet")
        ));
    }

    #[test]
    fn an_item_carries_its_move_as_the_form_the_page_has_for_it() {
        let node = read(board());
        let [todo, _] = sections(&node) else {
            panic!("not two lists");
        };
        let [N::Row(login), ..] = rows(todo) else {
            panic!("no rows");
        };
        let Some(N::Disclosure(moves)) = login.detail.last() else {
            panic!("no move: {:?}", login.detail);
        };
        assert_eq!(moves.label, "Move");
        let [N::Form(form)] = moves.children.as_slice() else {
            panic!("not one form");
        };
        assert_eq!(form.method, n::Method::Post);
        assert_eq!(form.action, "/cards/move");
        let hidden: Vec<(&str, &str)> = form
            .hidden
            .iter()
            .map(|hidden| (hidden.name.as_str(), hidden.value.as_str()))
            .collect();
        assert_eq!(
            hidden,
            [
                ("_csrf", "tok"),
                ("item", "c1"),
                ("from_list", "todo"),
                ("before", ""),
                ("after", "")
            ]
        );
        let [N::Field(to), N::Field(position), N::Button(send)] = form.children.as_slice() else {
            panic!("not two fields and a button: {:?}", form.children);
        };
        assert_eq!(
            (to.name.as_str(), to.label.as_deref()),
            ("to_list", Some("To"))
        );
        let i::Input::Choice(choice) = &to.input else {
            panic!("not a choice");
        };
        assert_eq!(choice.value.as_deref(), Some("todo"));
        assert_eq!(choice.options.len(), 2);
        assert_eq!(position.name, "position");
        assert_eq!(position.label.as_deref(), Some("Position"));
        assert_eq!(send.action, Some(Action::submit()));
    }

    #[test]
    fn a_board_is_lists_side_by_side_and_a_list_that_goes_on_says_where() {
        let node = read(
            Sortable::new("board", "Cards")
                .board()
                .list(
                    SortList::new("todo", "To do")
                        .item(SortItem::new("c1", "Fix login"))
                        .more("/cards/more", [("list", "todo"), ("after", "c1")]),
                )
                .list(SortList::new("done", "Done").total(60).from(0)),
        );
        let N::Grid(grid) = &node else {
            panic!("a board is not a grid");
        };
        assert_eq!(grid.children.len(), 2);
        let N::Section(todo) = &grid.children[0] else {
            panic!("not a section");
        };
        let Some(N::List(list)) = todo.children.first() else {
            panic!("no list");
        };
        assert_eq!(list.more.as_deref(), Some("/cards/more?list=todo&after=c1"));
        // Without a post address nothing moves: no form under an item.
        let N::Row(row) = &rows(&grid.children[0])[0] else {
            panic!("not a row");
        };
        assert!(
            !row.detail
                .iter()
                .any(|part| matches!(part, N::Disclosure(_)))
        );
    }

    #[test]
    fn what_a_pointer_takes_hold_of_is_left_out_and_said() {
        let node: Node = board().into();
        let lowered = crate::native::lower(&node);
        assert!(lowered.is_native(), "{:?}", lowered.misses);
        assert!(lowered.dropped.iter().any(|name| name == "sortable-handle"));
    }

    #[test]
    fn a_hook_no_reader_knows_inside_a_sortable_is_still_a_miss() {
        for name in [
            "sortable",
            "sortable-list",
            "sortable-item",
            "sortable-body",
            "sortable-form",
        ] {
            let node: Node = board().into();
            assert_eq!(
                misses(tamper(node, name, "data-ui-wizard")),
                ["hook:data-ui-wizard"],
                "{name}"
            );
        }
    }
}
