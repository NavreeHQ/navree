//! A value as the panel's pages draw it: in a cell of a list, on a record's page.

use navree_ui::described::DescribedCell;
use navree_ui::{
    Attachment, Badge, Markdown, Node, SafeHtml, Text, Tone, el, icon_labelled, icons,
};

use crate::present::{Display, Item};

/// The colours a select field's badges take, one per value, always the same for it.
const TONES: [Tone; 6] = [
    Tone::Neutral,
    Tone::Info,
    Tone::Success,
    Tone::Warning,
    Tone::Named("violet"),
    Tone::Named("cyan"),
];

/// `text` in lines, each break a line break.
fn lines(text: &str) -> Vec<Node> {
    let mut out = Vec::new();
    for (at, line) in text.lines().enumerate() {
        if at > 0 {
            out.push(el("br").into());
        }
        out.push(Node::from(line.to_owned()));
    }
    out
}

/// `items` with their date-times shown in `zone`, the reader's (`CurrentZone`), and the
/// zone's short name: `26 Oct 2026 17:30 IST`. What the record holds stays as it is.
pub(crate) fn in_zone(mut items: Vec<Item>, zone: navree_time::Zone) -> Vec<Item> {
    for cell in items.iter_mut().flat_map(|item| item.cells.iter_mut()) {
        if let Display::Time {
            iso,
            text,
            date_only: false,
        } = &mut cell.display
            && let Ok(at) = chrono::DateTime::parse_from_rfc3339(iso)
        {
            *text = zone_text(at.with_timezone(&chrono::Utc), zone);
        }
    }
    items
}

/// `at` on `zone`'s clock, with the zone's short name.
pub(crate) fn zone_text(at: chrono::DateTime<chrono::Utc>, zone: navree_time::Zone) -> String {
    zone.local(at).format("%-d %b %Y %H:%M %Z").to_string()
}

/// How `display` is drawn.
pub(crate) fn node(display: &Display) -> Node {
    match display {
        Display::Empty => Text::muted("—").inline().attr("aria-label", "Empty").into(),
        Display::Hidden => Text::muted("Hidden")
            .inline()
            .attr("title", "You may not read this field")
            .into(),
        Display::Text { text } => Node::from(el("bdi").child(text.clone())),
        Display::Multiline { text } => Text::new(lines(text)).into(),
        Display::Markdown { text } => Markdown::trusted(text.clone()).into(),
        Display::Html { html } => Node::Raw(SafeHtml::trusted(html.clone())),
        Display::Link {
            href,
            text,
            external,
        } => el("a")
            .href(href.clone())
            .attr_opt("rel", external.then_some("noopener noreferrer"))
            .child(el("bdi").child(text.clone()))
            .into(),
        Display::Badge { text, tone } => Badge::new(text.clone())
            .tone(TONES[usize::from(*tone) % TONES.len()])
            .into(),
        Display::Bool { value: true } => icon_labelled(icons::CHECK, "Yes"),
        Display::Bool { value: false } => icon_labelled(icons::X, "No"),
        Display::Time { iso, text, .. } => el("time")
            .attr("datetime", iso.clone())
            .child(text.clone())
            .into(),
        Display::Image { href, text, .. } => Attachment::new(text.clone())
            .href(href.clone())
            .icon(icons::IMAGE)
            .into(),
    }
}

/// `display` as a cell of a list, linked to `link` (the record's page) when given.
pub(crate) fn cell(display: &Display, link: Option<&str>) -> DescribedCell {
    let text = display.text();
    match (display, link) {
        (Display::Text { text }, None) => DescribedCell::text(text.clone()),
        (Display::Text { text }, Some(link)) => DescribedCell::node(
            el("a")
                .href(link.to_owned())
                .child(el("bdi").child(text.clone())),
            text.clone(),
        ),
        // A link of its own can't sit inside the record's.
        (Display::Link { .. }, _) | (_, None) => DescribedCell::node(node(display), text),
        (_, Some(link)) => {
            DescribedCell::node(el("a").href(link.to_owned()).child(node(display)), text)
        }
    }
}

#[cfg(test)]
mod tests {
    use navree_ui::render;

    use super::*;

    #[test]
    fn values_are_drawn_escaped() {
        let html = render(&node(&Display::Text {
            text: "<b>Agra</b>".into(),
        }));
        assert_eq!(html, "<bdi>&lt;b&gt;Agra&lt;/b&gt;</bdi>");
        let html = render(&node(&Display::Multiline {
            text: "one\ntwo".into(),
        }));
        assert!(html.contains("one<br>two"), "{html}");
        let html = render(&node(&Display::Bool { value: true }));
        assert!(html.contains(r#"aria-label="Yes""#), "{html}");
    }

    #[test]
    fn the_first_cell_links_to_the_record() {
        let cell = cell(
            &Display::Text {
                text: "Gulati renewal".into(),
            },
            Some("/admin/resources/deals/7"),
        );
        assert_eq!(cell.as_text(), "Gulati renewal");
        let hidden = super::cell(&Display::Hidden, Some("/admin/resources/deals/7"));
        assert_eq!(
            hidden.as_text(),
            "",
            "what a user may not read says nothing"
        );
    }
}
