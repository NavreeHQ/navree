//! Live parts of a page on the app's live hub. A `LiveStream` names
//! the page's topics and channels once; each region, table, list and sidebar
//! group listens for its own names on it (several at once), and loads the
//! live module.

use navree_ui::assets;
use navree_ui::{
    ActivityFeed, DataTable, LIVE_STREAM_PATH, LazyEnd, LazyItem, LazyList, Live, LiveStream,
    NavGroup, NavItem, Node, Region, Shell, TableQuery, TableRow, Uses, View, render,
};

#[derive(Clone, TableRow)]
struct Row {
    #[col(skip)]
    id: i64,
    #[col(sortable)]
    title: String,
}

/// The value of attribute `name` on the first element holding it.
fn attr(html: &str, name: &str) -> Option<String> {
    let start = html.find(&format!("{name}=\""))? + name.len() + 2;
    let end = html[start..].find('"')? + start;
    Some(html[start..end].to_owned())
}

fn inbox_stream() -> LiveStream {
    LiveStream::topics(["conversations", "conversation.42", "customer.7", "clock"])
}

#[test]
fn the_stream_is_the_hubs_route_with_the_pages_topics_and_channels() {
    assert_eq!(LIVE_STREAM_PATH, "/broadcasting/stream");
    let stream = inbox_stream().with_channels(["private-crm.deals"]);
    assert_eq!(
        stream.href().as_str(),
        "/broadcasting/stream?topics=conversations,conversation.42,customer.7,clock&channels=private-crm.deals"
    );
}

#[test]
fn a_region_listens_for_one_name_or_several_on_one_stream() {
    let stream = inbox_stream();
    let list = render(
        &Region::new("list")
            .src("/inbox/list")
            .live_on(stream.on("conversations"))
            .into(),
    );
    let heading = render(
        &Region::new("heading")
            .src("/inbox/42/heading")
            .live_on(stream.on("conversation.42").event("clock"))
            .into(),
    );
    assert_eq!(
        attr(&list, "data-ui-live-src"),
        attr(&heading, "data-ui-live-src"),
        "one connection"
    );
    assert_eq!(
        attr(&list, "data-ui-live-src").as_deref(),
        Some(stream.href().as_str())
    );
    assert_eq!(
        attr(&list, "data-ui-live-event").as_deref(),
        Some("conversations")
    );
    assert_eq!(
        attr(&heading, "data-ui-live-event").as_deref(),
        Some("conversation.42 clock")
    );

    // Any stream on this site still works, with its unnamed events.
    let plain = render(&Region::new("feed").src("/feed").live_on("/events").into());
    assert_eq!(attr(&plain, "data-ui-live-src").as_deref(), Some("/events"));
    assert_eq!(attr(&plain, "data-ui-live-event"), None);
    // And the older builder says the same.
    let older = render(
        &Region::new("feed")
            .src("/feed")
            .live_from("/events")
            .live_event("orders")
            .into(),
    );
    let newer = render(
        &Region::new("feed")
            .src("/feed")
            .live_on(Live::new("/events").event("orders"))
            .into(),
    );
    assert_eq!(older, newer);
}

#[test]
fn every_live_part_loads_the_live_module() {
    let stream = LiveStream::topics(["orders", "activity"]);
    let rows = vec![Row {
        id: 1,
        title: "A".to_owned(),
    }];
    let query = TableQuery::<Row>::parse("/orders", "");
    let page = |body: Node| View::new("x").body(body);

    let region = page(
        Region::new("r")
            .src("/r")
            .live_on(stream.on("orders"))
            .into(),
    );
    assert_eq!(region.uses(), Uses::CORE.with(Uses::LIVE));
    assert!(region.to_html().contains(assets::LIVE_URL));

    let table = page(
        DataTable::new("orders", &query, &rows)
            .live_on(stream.on("orders"))
            .into(),
    );
    for module in [Uses::CORE, Uses::TABLE, Uses::LIVE] {
        assert!(table.uses().has(module));
    }
    let html = table.to_html();
    assert_eq!(attr(&html, "data-ui-live-event").as_deref(), Some("orders"));
    assert_eq!(
        attr(&html, "data-ui-live-src").as_deref(),
        Some(stream.href().as_str())
    );

    let list = page(
        LazyList::new("thread", "Messages", "/thread/42")
            .items([LazyItem::new("1", "1", "Hello")])
            .live_on(stream.on("activity"), LazyEnd::End)
            .into(),
    );
    assert_eq!(list.uses(), Uses::LAZY.with(Uses::LIVE));
    let html = list.to_html();
    assert_eq!(
        attr(&html, "data-ui-live-event").as_deref(),
        Some("activity")
    );
    assert_eq!(attr(&html, "data-ui-live-at").as_deref(), Some("end"));

    let feed = render(
        &ActivityFeed::new("activity", "Recent activity")
            .in_region(
                "activity-region",
                "/fragments/activity",
                stream.on("activity"),
                "0",
            )
            .into(),
    );
    assert_eq!(
        attr(&feed, "data-ui-live-event").as_deref(),
        Some("activity")
    );
    assert_eq!(
        attr(&feed, "data-ui-live-src").as_deref(),
        Some(stream.href().as_str())
    );
}

#[test]
fn a_sidebar_group_keeps_its_counts_on_the_hub() {
    let stream = LiveStream::topics(["conversations"]);
    let shell = Shell::new("Inbox").group(
        NavGroup::new("Inboxes")
            .item(NavItem::new("All", "/inbox"))
            .live_on("inbox-counts", "/inbox/counts", stream.on("conversations")),
    );
    let page = shell.page("Inbox", "/inbox").to_view();
    assert!(page.uses().has(Uses::LIVE));
    let html = page.to_html();
    assert_eq!(
        attr(&html, "data-ui-live-event").as_deref(),
        Some("conversations")
    );
    assert_eq!(
        attr(&html, "data-ui-live-src").as_deref(),
        Some(stream.href().as_str())
    );
}
