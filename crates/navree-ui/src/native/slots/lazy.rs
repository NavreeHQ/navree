//! `LazyList`: a list that loads by cursor, and `LiveState`, the dot that says a
//! region is following its stream.
//!
//! A lazy list is its items and, at its end, the address of the next page by
//! cursor. That is a native list that goes on (`more`); the client asks for the
//! next rows the way it asks for any list's. When the page keeps it live, a
//! stream says when to ask again: a list at a revision says where it asks what
//! changed since, and the stream that nudges it; one with no revision stands in
//! a region that is fetched again. What the list does for a browser (the window
//! it keeps, the announcements) is the script's.

use std::borrow::Cow;

use navree_native::node as n;

use super::super::lower::{
    Cx, N, Parts, Read, attr, elements, fail, has, hooks, id, one, slot, with_id,
};
use crate::node::Element;

/// The hooks the root of a lazy list carries that this reader answers for.
pub(crate) const HOOKS: &[&str] = &[
    "data-ui-lazy-list",
    "data-ui-src",
    "data-ui-order",
    "data-ui-max",
    "data-ui-current",
    "data-ui-seen",
    "data-ui-live-src",
    "data-ui-live-event",
    "data-ui-live-catch-up",
    "data-ui-live-at",
];

/// The hooks the list of items carries.
const ITEMS: &[&str] = &[
    "data-ui-keep",
    "data-ui-start-done",
    "data-ui-end-done",
    "data-ui-since",
];

/// `LazyList`.
pub(crate) fn lazy_list(cx: &mut Cx, element: &Element) -> Read {
    let mut parts = Parts::of(element);
    let mut before = None;
    let mut after = None;
    for link in parts.slots("lazy-more") {
        match attr(link, "data-dir").as_deref() {
            Some("start") => before = Some(link),
            Some("end") => after = Some(link),
            _ => return fail("a link to more items that goes nowhere"),
        }
    }
    let items = parts.slot("lazy-items");
    let empty = parts.slot("lazy-empty");
    // What is told to assistive technology as items arrive.
    let _status = parts.slot("lazy-status");
    parts.done()?;
    let Some(items) = items else {
        return fail("a lazy list with no items");
    };
    hooks(items, ITEMS)?;
    let mut rows = Vec::new();
    for item in elements(items)? {
        if item.tag() != "li" {
            return fail(format!("a <{}> among the items of a list", item.tag()));
        }
        // What an item says when it arrives on its own, and that it arrives unannounced:
        // a client announces what it shows its own way.
        hooks(item, &["data-ui-say", "data-ui-quiet"])?;
        let key = attr(item, "data-key").map(Cow::into_owned);
        rows.push(with_id(one(cx.of(item)), key));
    }
    // The link back to newer items is shown only when there are some to load before.
    let rows_before = before
        .filter(|link| !has(link, "hidden"))
        .and_then(|link| attr(link, "href").map(Cow::into_owned));
    if has(element, "data-ui-current") {
        cx.dropped.push("lazy-current".to_owned());
    }
    let more = after
        .filter(|link| !has(link, "hidden"))
        .and_then(|link| attr(link, "href").map(Cow::into_owned));
    let live = attr(element, "data-ui-live-src");
    // A list at a revision asks what changed since it, whenever its stream says: new rows
    // join the end it follows the stream at.
    let revision = attr(items, "data-ui-since").map(Cow::into_owned);
    if let (Some(live), Some(since), Some(list_id), Some(src)) =
        (&live, &revision, id(element), attr(element, "data-ui-src"))
    {
        let at = if attr(element, "data-ui-live-at").as_deref() == Some("start") {
            "start"
        } else {
            "end"
        };
        let joiner = if src.contains('?') { '&' } else { '?' };
        let mut children = vec![N::List(n::List {
            id: Some(list_id),
            children: rows,
            more,
            before: rows_before,
            since: Some(since.clone()),
            changes: Some(format!("{src}{joiner}dir=live&at={at}")),
            live: Some(n::Live {
                src: live.to_string(),
                event: attr(element, "data-ui-live-event").map(Cow::into_owned),
            }),
            ..n::List::default()
        })];
        if let Some(empty) = empty {
            children.extend(cx.of(empty));
        }
        return Ok(vec![one(children)]);
    }
    // The list a client names when it asks for the next rows has the id the page gave
    // the component; inside a region of its own, the list keeps the one of its items.
    let mut children = vec![N::List(n::List {
        id: if live.is_some() {
            id(items)
        } else {
            id(element)
        },
        children: rows,
        more,
        before: rows_before,
        ..n::List::default()
    })];
    if let Some(empty) = empty {
        children.extend(cx.of(empty));
    }
    let Some(live) = live else {
        return Ok(vec![one(children)]);
    };
    let (Some(region), Some(src)) = (id(element), attr(element, "data-ui-src")) else {
        return fail("a lazy list that follows a stream with no id or no address");
    };
    Ok(vec![N::Region(n::Region {
        id: region,
        src: src.into_owned(),
        lazy: false,
        live: Some(n::Live {
            src: live.into_owned(),
            event: attr(element, "data-ui-live-event").map(Cow::into_owned),
        }),
        children,
    })])
}

/// `LiveState`: the dot and the words that say a region follows its stream. The region
/// says it follows one; a client has its own way to show it.
pub(crate) fn live_state(cx: &mut Cx, element: &Element) -> Read {
    hooks(element, &[])?;
    if slot(element).as_deref() != Some("live-state") {
        return fail("a live state that is not one");
    }
    cx.dropped.push("live-state".to_owned());
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::super::testing::{misses, read, tamper};
    use super::*;
    use crate::native::TestNative;
    use crate::node::Node;
    use crate::{LazyItem, LazyList, Text, el};
    use navree_native::{Client, FormFactor, Platform};

    fn items() -> [LazyItem; 2] {
        [
            LazyItem::new("1", "c-1", Text::new("First")),
            LazyItem::new("2", "c-2", Text::new("Second")),
        ]
    }

    #[test]
    fn a_lazy_list_is_its_items_and_where_the_next_ones_are() {
        let N::List(list) = read(
            LazyList::new("convs", "Conversations", "/inbox/rows")
                .items(items())
                .more_after("/inbox/rows?after=c-2"),
        ) else {
            panic!("not a list");
        };
        assert_eq!(list.id.as_deref(), Some("convs"));
        assert_eq!(list.more.as_deref(), Some("/inbox/rows?after=c-2"));
        assert_eq!(list.children.len(), 2);
        assert_eq!(list.children[0].id(), Some("1"));
        assert_eq!(
            list.children[1],
            with_id(N::text("Second"), Some("2".into()))
        );
    }

    #[test]
    fn a_list_that_is_all_there_has_no_more_and_one_with_nothing_shows_what_the_page_does() {
        let N::List(list) =
            read(LazyList::new("convs", "Conversations", "/inbox/rows").items(items()))
        else {
            panic!("not a list");
        };
        assert_eq!(list.more, None);
        let N::Stack(stack) = read(
            LazyList::new("convs", "Conversations", "/inbox/rows").empty(Text::new("Nothing open")),
        ) else {
            panic!("not a stack");
        };
        assert!(matches!(&stack.children[0], N::List(list) if list.children.is_empty()));
        assert_eq!(stack.children[1], N::text("Nothing open"));
    }

    #[test]
    fn a_list_that_follows_a_stream_stands_in_a_region_that_says_so() {
        let N::Region(region) = read(
            LazyList::new("convs", "Conversations", "/inbox/rows")
                .items(items())
                .live_from("/events", Some("conversations"), crate::LazyEnd::Start),
        ) else {
            panic!("not a region");
        };
        assert_eq!(region.id, "convs");
        assert_eq!(region.src, "/inbox/rows");
        let live = region.live.expect("a stream");
        assert_eq!(live.src, "/events");
        assert_eq!(live.event.as_deref(), Some("conversations"));
        assert!(matches!(region.children.as_slice(), [N::List(_)]));
    }

    #[test]
    fn a_list_at_a_revision_asks_what_changed_since_it_when_its_stream_says() {
        let N::List(list) = read(
            LazyList::new("convs", "Conversations", "/inbox/rows")
                .items(items())
                .since("r7")
                .more_before("/inbox/rows?dir=start&cursor=c-1")
                .live_from("/events", Some("conversations"), crate::LazyEnd::Start),
        ) else {
            panic!("not a list");
        };
        // No region to fetch again: the list itself follows its stream.
        assert_eq!(list.id.as_deref(), Some("convs"));
        assert_eq!(list.since.as_deref(), Some("r7"));
        assert_eq!(
            list.changes.as_deref(),
            Some("/inbox/rows?dir=live&at=start")
        );
        assert_eq!(
            list.live,
            Some(n::Live {
                src: "/events".into(),
                event: Some("conversations".into())
            })
        );
        assert_eq!(
            list.before.as_deref(),
            Some("/inbox/rows?dir=start&cursor=c-1")
        );
        assert_eq!(list.children.len(), 2);
    }

    #[test]
    fn what_only_a_script_uses_is_left_out_and_said() {
        let node: Node = LazyList::new("convs", "Conversations", "/inbox/rows")
            .items(items())
            .current("1")
            .into();
        let lowered = crate::native::lower(&node);
        assert!(lowered.is_native(), "{:?}", lowered.misses);
        let mut dropped = lowered.dropped;
        dropped.sort();
        assert_eq!(dropped, ["lazy-current"]);
    }

    #[test]
    fn the_dot_that_says_a_region_is_live_is_left_out() {
        let page = TestNative::build(Client::new(Platform::Android, FormFactor::Phone), || {
            crate::View::new("x").body(el("span").slot("live-state").child("Live"))
        });
        assert!(page.is_native(), "{:?}", page.why());
        assert!(page.page().dropped.iter().any(|name| name == "live-state"));
    }

    #[test]
    fn a_hook_no_reader_knows_inside_a_lazy_list_is_still_a_miss() {
        for name in ["lazy-list", "lazy-items"] {
            let node: Node = LazyList::new("convs", "Conversations", "/inbox/rows")
                .items(items())
                .into();
            assert_eq!(
                misses(tamper(node, name, "data-ui-wizard")),
                ["hook:data-ui-wizard"],
                "{name}"
            );
        }
        let node: Node = el("span").slot("live-state").into();
        assert_eq!(
            misses(tamper(node, "live-state", "data-ui-wizard")),
            ["hook:data-ui-wizard"]
        );
    }
}
