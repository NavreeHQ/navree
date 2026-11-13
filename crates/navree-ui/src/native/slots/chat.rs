//! The parts of a conversation: `Message`, `Bubble`, `Attachment` and `Marker`.
//!
//! A message is the protocol's `message`: who wrote it, their picture, whether it is
//! the reader's own (the far side of the conversation) and whether it is a note
//! only the team sees, what it says, when, and the files with it. The bubbles
//! are what it says; a bubble outside a message stands as a group of its parts.

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
    let mut message = n::Message {
        id: id(element),
        // The reader's own messages stand on the far side.
        mine: attr(element, "data-align").as_deref() == Some("end"),
        ..n::Message::default()
    };
    if let Some(avatar) = avatar {
        match cx.of(avatar).as_slice() {
            [N::Avatar(person)] => message.avatar = Some(person.clone()),
            _ => return fail("a message whose picture is not an avatar"),
        }
    }
    // The header and the footer have their places; the bubbles are what was said and
    // every other part (a file) goes with it.
    for part in elements(content)? {
        match slot(part).as_deref() {
            Some("message-header") => {
                if !words_only(part) {
                    return fail("a message whose header holds more than words");
                }
                message.author = text_of(part);
            }
            Some("message-footer") => {
                if !words_only(part) {
                    return fail("a message whose footer holds more than words");
                }
                message.time = Some(text_of(part)).filter(|time| !time.is_empty());
            }
            Some("bubble") => {
                message.note |= attr(part, "data-variant").as_deref() == Some("note");
                message.body.extend(said(cx, part)?);
            }
            _ => cx.element(part, &mut message.body),
        }
    }
    if message.author.is_empty() {
        return fail("a message with no writer");
    }
    Ok(vec![N::Message(message)])
}

/// What one bubble says.
fn said(cx: &mut Cx, bubble: &Element) -> Read<Vec<N>> {
    hooks(bubble, &[])?;
    let mut parts = Parts::of(bubble);
    let content = parts.slot("bubble-content");
    parts.done()?;
    let Some(content) = content else {
        return fail("a bubble with no content");
    };
    hooks(content, &[])?;
    Ok(cx.of(content))
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
    Ok(vec![
        if attr(element, "data-variant").as_deref() == Some("note") {
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
        },
    ])
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
                "span"
                    | "div"
                    | "p"
                    | "bdi"
                    | "strong"
                    | "b"
                    | "em"
                    | "i"
                    | "time"
                    | "small"
                    | "svg"
                    | "path"
                    | "circle"
                    | "rect"
                    | "line"
                    | "polyline"
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

    fn message_of(node: N) -> n::Message {
        let N::Message(message) = node else {
            panic!("not a message: {node:?}");
        };
        message
    }

    #[test]
    fn a_message_is_its_writer_picture_words_files_and_time() {
        let message = message_of(read(
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
        assert_eq!(message.id.as_deref(), Some("message-3"));
        assert_eq!(message.author, "Tanvi Sharma");
        assert_eq!(
            message.avatar.as_ref().map(|person| person.name.as_str()),
            Some("Tanvi Sharma")
        );
        assert_eq!(message.time.as_deref(), Some("9:41 am"));
        assert!(!message.mine && !message.note);
        let [said, N::Row(file)] = message.body.as_slice() else {
            panic!("not words and a file: {:?}", message.body);
        };
        assert_eq!(*said, N::text("Where is my order?"));
        assert_eq!(file.title, "invoice.pdf");
        assert_eq!(file.subtitle.as_deref(), Some("40 KB"));
        assert_eq!(
            file.action,
            Some(navree_native::Action::navigate("/files/1"))
        );
        assert!(
            matches!(file.leading.as_deref(), Some(N::Icon(icon)) if icon.name.as_deref() == Some("file"))
        );
    }

    #[test]
    fn the_far_side_is_the_readers_own_and_a_note_is_a_note() {
        let message = message_of(read(
            Message::new(BubbleSide::End)
                .header(Text::new("Simran Grewal"))
                .content(Bubble::new(BubbleSide::End, Text::new("Refund approved.")).note()),
        ));
        assert!(message.mine);
        assert!(message.note);
        assert_eq!(message.body, [N::text("Refund approved.")]);
        let theirs = message_of(read(
            Message::new(BubbleSide::Start)
                .header(Text::new("Tanvi Sharma"))
                .content(Bubble::new(BubbleSide::Start, Text::new("Hello"))),
        ));
        assert!(!theirs.mine && !theirs.note);
    }

    #[test]
    fn a_bubble_alone_stands_as_a_group_of_its_parts_and_a_note_apart() {
        let N::Section(note) = read(Bubble::new(BubbleSide::End, Text::new("Careful.")).note())
        else {
            panic!("not a section");
        };
        assert_eq!(note.style, n::SectionStyle::Inset);
        assert_eq!(note.children, [N::text("Careful.")]);
    }

    #[test]
    fn a_marker_is_a_small_quiet_line() {
        assert_eq!(
            read(Marker::new(
                MarkerVariant::Separator,
                "Closed by Simran Grewal"
            )),
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
