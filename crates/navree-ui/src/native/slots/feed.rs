//! `ActivityFeed`: what people did lately, newest first.
//!
//! A feed is a titled section of rows. Each row says who did what to what, in
//! one line, with the kind of event as its icon, a badge for a tag, the facts
//! under it, and when it happened at its end. What the feed adds for a browser
//! (the dot that says it is live, the notice that offers back rows left out by
//! a filter) is the region's job or a script's.

use navree_native::node as n;

use super::super::lower::{
    Cx, N, Parts, Read, attr, elements, fail, has, hooks, id, slot, text_of, tone,
};
use crate::node::Element;

/// `ActivityFeed`.
pub(crate) fn feed(cx: &mut Cx, element: &Element) -> Read {
    let mut parts = Parts::of(element);
    let header = parts.slot("activity-header");
    let notice = parts.slot("hidden-notice");
    let list = parts.slot("activity-list");
    let loading = parts.slot("activity-loading");
    let failed = parts.slot("activity-failed");
    let empty = parts.slot("activity-empty");
    parts.done()?;
    let (Some(header), Some(list)) = (header, list) else {
        return fail("a feed with no header or no list");
    };
    // The notice that offers back rows a filter left out is shown only when
    // there are some; a notice that shows is rows no reader brings back.
    if let Some(notice) = notice
        && !has(notice, "hidden")
    {
        return fail("rows left out by a filter that a reader can bring back");
    }
    let mut section = n::Section {
        id: id(element),
        style: n::SectionStyle::Plain,
        ..n::Section::default()
    };
    heading(cx, header, &mut section)?;
    hooks(list, &[])?;
    let mut rows = Vec::new();
    for item in elements(list)? {
        if slot(item).as_deref() != Some("activity-item") {
            return fail("something that is not an event in a feed");
        }
        rows.push(row(cx, item)?);
    }
    section.children.push(N::List(n::List {
        id: id(list),
        children: rows,
        ..n::List::default()
    }));
    if let Some(loading) = loading {
        // Its rows are placeholders for the first load; a client has its own.
        hooks(loading, &[])?;
        section.children.push(N::Progress(n::Progress::default()));
    }
    if let Some(failed) = failed {
        section.children.extend(cx.of(failed));
    }
    if let Some(empty) = empty {
        section.children.extend(cx.of(empty));
    }
    Ok(vec![N::Section(section)])
}

/// The feed's title, and what stands under it.
fn heading(cx: &mut Cx, header: &Element, section: &mut n::Section) -> Read<()> {
    let mut parts = Parts::of(header);
    let title = parts.slot("activity-heading");
    let summary = parts.slot("activity-summary");
    let zone = parts.slot("activity-zone");
    let period = parts.slot("activity-period");
    parts.done()?;
    let Some(title) = title else {
        return fail("a feed with no title");
    };
    let mut inside = Parts::of(title);
    let words = inside.slot("activity-title");
    // The dot that says the feed is live: its region says so.
    let _live = inside.slot("live-state");
    inside.done()?;
    let Some(words) = words else {
        return fail("a feed with no title");
    };
    section.title = Some(text_of(words));
    section.subtitle = summary.map(text_of);
    if let Some(zone) = zone {
        section.children.push(N::Text(n::Text {
            value: text_of(zone),
            style: n::TextStyle::Caption,
            muted: true,
            ..n::Text::default()
        }));
    }
    if let Some(period) = period {
        section.children.extend(cx.of(period));
    }
    Ok(())
}

/// The events a feed answers a native client with when it asks for what is new: the
/// rows, each an event.
pub(crate) fn rows(cx: &mut Cx, nodes: &[crate::node::Node]) -> Read<Vec<N>> {
    let mut out = Vec::new();
    for node in super::super::lower::flat(nodes) {
        let crate::node::Node::Element(item) = node else {
            return fail("something that is not an event among the rows of a feed");
        };
        if slot(item).as_deref() != Some("activity-item") {
            return fail("something that is not an event among the rows of a feed");
        }
        out.push(row(cx, item)?);
    }
    Ok(out)
}

/// One event.
fn row(cx: &mut Cx, item: &Element) -> Read<N> {
    hooks(item, &[])?;
    if has(item, "data-filtered") {
        return fail("an event left out by a filter");
    }
    let mut parts = Parts::of(item);
    let glyph = parts.slot("activity-icon");
    let body = parts.slot("activity-body");
    let time = parts.slot("activity-time");
    parts.done()?;
    let Some(body) = body else {
        return fail("an event with no body");
    };
    let mut row = n::FeedRow {
        id: id(item),
        tone: attr(item, "data-tone").and_then(|name| tone(&name)),
        // What a machine did is drawn quieter than what people did.
        muted: attr(item, "data-state").as_deref() == Some("muted"),
        ..n::FeedRow::default()
    };
    if let Some(glyph) = glyph {
        let marks = elements(glyph)?;
        if let [mark] = marks.as_slice() {
            row.icon = Some(super::icon(mark)?);
        }
    }
    let mut inside = Parts::of(body);
    let line = inside.slot("activity-line");
    let facts = inside.slot("meta");
    inside.done()?;
    let Some(line) = line else {
        return fail("an event with no line");
    };
    sentence(cx, line, &mut row)?;
    if let Some(facts) = facts {
        cx.element(facts, &mut row.detail);
    }
    row.time = time.map(text_of).filter(|time| !time.is_empty());
    Ok(N::FeedRow(row))
}

/// Who did what to what, in runs so the names stand out: the actor's link is what
/// a tap does, and the tag stands under it as a badge.
fn sentence(cx: &mut Cx, line: &Element, row: &mut n::FeedRow) -> Read<()> {
    let mut parts = Parts::of(line);
    let actor = parts.slot("activity-actor");
    let verb = parts.slot("activity-verb");
    let object = parts.slot("activity-object");
    let tag = parts.slot("activity-tag");
    parts.done()?;
    let (Some(actor), Some(verb)) = (actor, verb) else {
        return fail("an event that does not say who did what");
    };
    let link = if actor.tag() == "a" {
        cx.action_of(actor)?
    } else {
        None
    };
    let href = match &link {
        Some(navree_native::Action::Navigate { href, .. }) => Some(href.clone()),
        _ => None,
    };
    row.action = link;
    let run = |text: String, bold: bool, href: Option<String>| n::Run {
        text,
        bold,
        href,
        ..n::Run::default()
    };
    row.runs.push(run(text_of(actor), true, href));
    row.runs
        .push(run(format!(" {}", text_of(verb)), false, None));
    if let Some(object) = object {
        row.runs
            .push(run(format!(" {}", text_of(object)), true, None));
    }
    if let Some(tag) = tag {
        let read = cx.of(tag);
        if !matches!(read.as_slice(), [N::Badge(_)]) {
            return fail("an event whose tag is not a badge");
        }
        row.detail.extend(read);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::testing::{misses, read, tamper};
    use super::*;
    use crate::{ActivityFeed, ActivityItem, Empty, Meta, Text, Tone, icons};

    fn feed_of(node: N) -> n::Section {
        let N::Section(section) = node else {
            panic!("not a section: {node:?}");
        };
        section
    }

    fn rows_list(section: &n::Section) -> &n::List {
        section
            .children
            .iter()
            .find_map(|node| match node {
                N::List(list) => Some(list),
                _ => None,
            })
            .expect("a list")
    }

    fn rows_of(section: &n::Section) -> &[N] {
        let Some(N::List(list)) = section
            .children
            .iter()
            .find(|node| matches!(node, N::List(_)))
        else {
            panic!("no list: {:?}", section.children);
        };
        &list.children
    }

    #[test]
    fn a_feed_in_a_region_that_asks_for_what_is_newer_is_a_list_at_a_revision() {
        let N::Section(section) = read(
            ActivityFeed::new("act", "Recent activity")
                .live()
                .item(ActivityItem::new("Manpreet Gill", "replied"))
                .in_region(
                    "act-region",
                    "/fragments/activity",
                    "/events",
                    "0009",
                ),
        ) else {
            panic!("not a section");
        };
        let list = rows_list(&section);
        assert_eq!(list.id.as_deref(), Some("act-list"));
        assert_eq!(list.since.as_deref(), Some("0009"));
        assert_eq!(list.changes.as_deref(), Some("/fragments/activity"));
        assert_eq!(list.live.as_ref().map(|live| live.src.as_str()), Some("/events"));
    }

    #[test]
    fn what_a_machine_did_is_a_muted_row() {
        let section = feed_of(read(
            ActivityFeed::new("act", "Recent activity")
                .item(ActivityItem::new("Navree", "assigned").muted())
                .item(ActivityItem::new("Manpreet Gill", "replied")),
        ));
        let [N::FeedRow(machine), N::FeedRow(person)] = rows_of(&section) else {
            panic!("not two feed rows");
        };
        assert!(machine.muted && !person.muted);
        let lowered = crate::native::lower(&crate::Node::from(
            ActivityFeed::new("act", "Recent activity")
                .item(ActivityItem::new("Navree", "assigned").muted()),
        ));
        assert!(lowered.dropped.is_empty(), "{:?}", lowered.dropped);
    }

    #[test]
    fn a_feed_is_a_section_of_feed_rows_that_say_who_did_what() {
        let section = feed_of(read(
            ActivityFeed::new("act", "Recent activity")
                .summary("2 today")
                .item(
                    ActivityItem::new("Manpreet Gill", "replied to")
                        .object("Ticket 2041")
                        .tone(Tone::Info)
                        .icon(icons::MESSAGE_SQUARE)
                        .at(1_793_592_667)
                        .meta(Meta::icon(icons::CLOCK, "First reply in 12 min"))
                        .id("event-9"),
                )
                .item(
                    ActivityItem::new("Simran Grewal", "closed")
                        .href("/people/3")
                        .tag("Billing")
                        .time_text("2 min ago"),
                ),
        ));
        assert_eq!(section.id.as_deref(), Some("act"));
        assert_eq!(section.title.as_deref(), Some("Recent activity"));
        assert_eq!(section.subtitle.as_deref(), Some("2 today"));
        let [N::FeedRow(first), N::FeedRow(second)] = rows_of(&section) else {
            panic!("not two feed rows");
        };
        assert_eq!(first.id.as_deref(), Some("event-9"));
        let words: Vec<(&str, bool)> = first
            .runs
            .iter()
            .map(|run| (run.text.as_str(), run.bold))
            .collect();
        assert_eq!(
            words,
            [
                ("Manpreet Gill", true),
                (" replied to", false),
                (" Ticket 2041", true)
            ]
        );
        assert_eq!(first.tone, Some(n::Tone::Info));
        assert_eq!(
            first.icon.as_ref().and_then(|icon| icon.name.as_deref()),
            Some("message-square")
        );
        assert!(first.time.is_some());
        assert!(!first.detail.is_empty());
        assert_eq!(second.runs[0].text, "Simran Grewal");
        assert_eq!(second.runs[0].href.as_deref(), Some("/people/3"));
        assert_eq!(
            second.action,
            Some(navree_native::Action::navigate("/people/3"))
        );
        assert!(matches!(second.detail.as_slice(), [N::Badge(badge)] if badge.text == "Billing"));
        assert_eq!(second.time.as_deref(), Some("2 min ago"));
    }

    #[test]
    fn a_feed_with_no_rows_shows_what_the_page_shows_instead() {
        let section = feed_of(read(ActivityFeed::new("act", "Recent")));
        assert!(rows_of(&section).is_empty());
        assert!(
            section
                .children
                .iter()
                .any(|node| matches!(node, N::Empty(empty) if empty.title == "Nothing yet"))
        );
        let section = feed_of(read(
            ActivityFeed::new("act", "Recent").empty(Empty::new("Quiet day")),
        ));
        assert!(
            section
                .children
                .iter()
                .any(|node| matches!(node, N::Empty(empty) if empty.title == "Quiet day"))
        );
    }

    #[test]
    fn a_feed_that_is_loading_or_failed_says_so() {
        let loading = feed_of(read(ActivityFeed::new("act", "Recent").loading()));
        assert!(
            loading
                .children
                .iter()
                .any(|node| matches!(node, N::Progress(_)))
        );
        let failed = feed_of(read(
            ActivityFeed::new("act", "Recent")
                .failed("We could not load it")
                .retry("/again"),
        ));
        assert!(failed.children.iter().any(|node| matches!(
            node,
            N::Banner(banner) if banner.title.as_deref() == Some("We could not load it")
        )));
    }

    #[test]
    fn the_period_control_stands_under_the_title_and_the_zone_is_said() {
        let section = feed_of(read(
            ActivityFeed::new("act", "Recent")
                .period(Text::new("Last 7 days"))
                .item(ActivityItem::new("Ravneet Kaur", "refunded")),
        ));
        assert!(
            section
                .children
                .iter()
                .any(|node| *node == N::text("Last 7 days"))
        );
    }

    #[test]
    fn a_row_left_out_by_a_filter_is_a_miss() {
        let node: crate::node::Node = ActivityFeed::new("act", "Recent")
            .item(ActivityItem::new("Ravneet Kaur", "refunded").filtered())
            .into();
        assert_eq!(misses(node), ["slot:activity-feed"]);
    }

    #[test]
    fn a_hook_no_reader_knows_inside_a_feed_is_still_a_miss() {
        for name in ["activity-feed", "activity-item", "activity-list"] {
            let node: crate::node::Node = ActivityFeed::new("act", "Recent")
                .item(ActivityItem::new("Ravneet Kaur", "refunded"))
                .into();
            let tampered = tamper(node, name, "data-ui-wizard");
            let why = misses(tampered);
            assert_eq!(why, ["hook:data-ui-wizard"], "{name}");
        }
    }
}
