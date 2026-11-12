//! The parts of a conversation: `Message`, `Bubble`, `Attachment` and `Marker`.
//!
//! The vocabulary has no message yet. A message is read as the nearest thing
//! it has: a row with the writer's picture, the writer's name, what was written
//! under it, and when at its end. Which side a bubble is on (the reader's own
//! messages on the far side) is left out, and said.

use navree_native::node as n;

use super::super::lower::{Cx, N, Parts, Read, attr, elements, fail, hooks, id, slot, text_of};
use crate::node::{Element, Node};

/// `Message`: who wrote what, and when.
pub(crate) fn message(cx: &mut Cx, element: &Element) -> Read {
    let mut parts = Parts::of(element);
    let avatar = parts.slot("message-avatar");
    let content = parts.slot("message-content");
    parts.done()?;
    let Some(content) = content else {
        return fail("a message with no content");
    };
    let mut row = n::Row {
        id: id(element),
        ..n::Row::default()
    };
    if let Some(avatar) = avatar {
        row.leading = Some(Box::new(super::super::lower::one(cx.of(avatar))));
    }
    // The header and the footer have their places; every other part is what was said.
    for part in elements(content)? {
        match slot(part).as_deref() {
            Some("message-header") => {
                if !words_only(part) {
                    return fail("a message whose header holds more than words");
                }
                row.title = text_of(part);
            }
            Some("message-footer") => {
                if !words_only(part) {
                    return fail("a message whose footer holds more than words");
                }
                row.trailing = Some(Box::new(N::Text(n::Text {
                    value: text_of(part),
                    style: n::TextStyle::Caption,
                    muted: true,
                    ..n::Text::default()
                })));
            }
            _ => {
                let mut one = Vec::new();
                cx.element(part, &mut one);
                row.detail.extend(one);
            }
        }
    }
    if attr(element, "data-align").as_deref() == Some("end") {
        cx.dropped.push("message-side".to_owned());
    }
    Ok(vec![N::Row(row)])
}

/// `Bubble`: what one message says. A note for the team stands apart.
pub(crate) fn bubble(cx: &mut Cx, element: &Element) -> Read {
    let mut parts = Parts::of(element);
    let content = parts.slot("bubble-content");
    parts.done()?;
    let Some(content) = content else {
        return fail("a bubble with no content");
    };
    hooks(content, &[])?;
    let children = cx.of(content);
    Ok(vec![if attr(element, "data-variant").as_deref() == Some("note") {
        N::Section(n::Section {
            id: id(element),
            style: n::SectionStyle::Inset,
            children,
            ..n::Section::default()
        })
    } else {
        N::Stack(n::Stack {
            id: id(element),
            children,
            ..n::Stack::default()
        })
    }])
}

/// `Attachment`: a file, with its size, that opens when tapped.
pub(crate) fn attachment(cx: &mut Cx, element: &Element) -> Read {
    let mut parts = Parts::of(element);
    let media = parts.slot("attachment-media");
    let content = parts.slot("attachment-content");
    // What the page puts after it, such as a way to remove it, has no slot of its own.
    let after = parts.left().to_vec();
    let Some(content) = content else {
        return fail("a file with no name");
    };
    let mut row = n::Row {
        id: id(element),
        ..n::Row::default()
    };
    if let Some(media) = media {
        let marks = elements(media)?;
        if let [mark] = marks.as_slice() {
            row.leading = Some(Box::new(N::Icon(super::icon(mark)?)));
        }
    }
    let mut inside = Parts::of(content);
    let title = inside.slot("attachment-title");
    let description = inside.slot("attachment-description");
    inside.done()?;
    let Some(title) = title else {
        return fail("a file with no name");
    };
    row.title = text_of(title);
    row.subtitle = description.map(text_of);
    if title.tag() == "a" {
        row.action = cx.action_of(title)?;
    }
    let after = cx.flat_sequence(&after);
    if !after.is_empty() {
        row.trailing = Some(Box::new(super::super::lower::one(after)));
    }
    Ok(vec![N::Row(row)])
}

/// `Marker`: a small note between messages, a day or "joined the chat".
pub(crate) fn marker(_: &mut Cx, element: &Element) -> Read {
    let mut parts = Parts::of(element);
    let words = parts.slot("marker-content");
    parts.done()?;
    let Some(words) = words else {
        return fail("a marker with no words");
    };
    Ok(vec![N::Text(n::Text {
        id: id(element),
        value: text_of(words),
        style: n::TextStyle::Caption,
        muted: true,
        ..n::Text::default()
    })])
}

/// Whether an element holds words, and the pictures and groups that set them.
fn words_only(element: &Element) -> bool {
    element.child_nodes().iter().all(|node| match node {
        Node::Text(_) | Node::Int(_) | Node::Empty => true,
        Node::Fragment(nodes) => nodes.iter().all(|node| match node {
            Node::Element(inner) => words_only(inner),
            _ => true,
        }),
        Node::Element(inner) => {
            matches!(
                inner.tag(),
                "span" | "div" | "p" | "bdi" | "strong" | "b" | "em" | "i" | "time" | "small"
                    | "svg" | "path" | "circle" | "rect" | "line" | "polyline"
            ) && words_only(inner)
        }
        Node::Raw(_) | Node::Static(_) => true,
    })
}

#[cfg(test)]
mod tests {
    use super::super::testing::{misses, read, tamper};
    use super::*;
    use crate::{
        Attachment, Avatar, Bubble, BubbleSide, Button, Marker, MarkerVariant, Message, Row, Text,
        el, icons,
    };

    fn row_of(node: N) -> n::Row {
        let N::Row(row) = node else {
            panic!("not a row: {node:?}");
        };
        row
    }

    #[test]
    fn a_message_is_a_row_with_its_writer_its_words_and_its_time() {
        let row = row_of(read(
            el("article").attr("id", "message-3").child(
                Message::new(BubbleSide::Start)
                    .avatar(Avatar::placeholder("Tanvi Sharma"))
                    .header(Row::gap(1).children(Text::new("Tanvi Sharma").inline()))
                    .content((
                        Bubble::new(BubbleSide::Start, Text::new("Where is my order?")),
                        Attachment::new("invoice.pdf")
                            .meta("40 KB")
                            .icon(icons::FILE)
                            .href("/files/1"),
                    ))
                    .footer(el("time").child("9:41 am")),
            ),
        ));
        assert_eq!(row.id.as_deref(), Some("message-3"));
        assert_eq!(row.title, "Tanvi Sharma");
        assert!(matches!(row.leading.as_deref(), Some(N::Avatar(avatar)) if avatar.name == "Tanvi Sharma"));
        assert!(matches!(row.trailing.as_deref(), Some(N::Text(time)) if time.value == "9:41 am"));
        let [N::Stack(said), N::Row(file)] = row.detail.as_slice() else {
            panic!("not a bubble and a file: {:?}", row.detail);
        };
        assert_eq!(said.children, [N::text("Where is my order?")]);
        assert_eq!(file.title, "invoice.pdf");
        assert_eq!(file.subtitle.as_deref(), Some("40 KB"));
        assert_eq!(file.action, Some(navree_native::Action::navigate("/files/1")));
        assert!(matches!(file.leading.as_deref(), Some(N::Icon(icon)) if icon.name.as_deref() == Some("file")));
    }

    #[test]
    fn a_note_for_the_team_stands_apart() {
        let row = row_of(read(
            Message::new(BubbleSide::End)
                .header(Text::new("Internal note · Simran Grewal"))
                .content(Bubble::new(BubbleSide::End, Text::new("Refund approved.")).note()),
        ));
        let [N::Section(note)] = row.detail.as_slice() else {
            panic!("not a note");
        };
        assert_eq!(note.style, n::SectionStyle::Inset);
        assert_eq!(note.children, [N::text("Refund approved.")]);
    }

    #[test]
    fn the_side_a_message_is_on_is_left_out_and_said() {
        let node: Node = Message::new(BubbleSide::End)
            .header(Text::new("Simran Grewal"))
            .content(Bubble::new(BubbleSide::End, Text::new("On it.")))
            .into();
        let lowered = crate::native::lower(&node);
        assert!(lowered.is_native(), "{:?}", lowered.misses);
        assert_eq!(lowered.dropped, ["message-side"]);
    }

    #[test]
    fn a_marker_is_a_small_quiet_line() {
        assert_eq!(
            read(Marker::new(MarkerVariant::Separator, "Closed by Simran Grewal")),
            N::Text(n::Text {
                value: "Closed by Simran Grewal".into(),
                style: n::TextStyle::Caption,
                muted: true,
                ..n::Text::default()
            })
        );
    }

    #[test]
    fn a_header_that_holds_a_control_is_a_miss() {
        let node: Node = Message::new(BubbleSide::Start)
            .header(Button::new("Reply").href("/reply"))
            .content(Bubble::new(BubbleSide::Start, Text::new("Hi")))
            .into();
        assert_eq!(misses(node), ["slot:message"]);
    }

    #[test]
    fn a_hook_no_reader_knows_inside_a_message_is_still_a_miss() {
        for name in ["message", "bubble", "attachment", "marker"] {
            let node: Node = el("div")
                .children((
                    Message::new(BubbleSide::Start)
                        .header(Text::new("Tanvi"))
                        .content((
                            Bubble::new(BubbleSide::Start, Text::new("Hi")),
                            Attachment::new("a.pdf").href("/a"),
                        )),
                    Marker::new(MarkerVariant::Plain, "Today"),
                ))
                .into();
            assert_eq!(
                misses(tamper(node, name, "data-ui-wizard")),
                ["hook:data-ui-wizard"],
                "{name}"
            );
        }
    }
}
