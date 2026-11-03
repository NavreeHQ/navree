//! Timeline, ActivityFeed, FactCard, the Badge extensions and live regions:
//! the markup they write, the words they print, and what hostile text does.

use navree_ui::*;

// 2026-11-02 04:11:07 UTC, which is 09:41:07 in Asia/Kolkata.
const T0: i64 = 1_793_592_667;
const KOLKATA: i32 = 19_800;

fn html(node: impl Into<Node>) -> String {
    render(&node.into())
}

fn count(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

fn timeline(events: Vec<TimelineEvent>) -> String {
    html(
        Timeline::new("steps", "What happened")
            .time_zone("Asia/Kolkata", KOLKATA)
            .events(events),
    )
}

// Timeline

#[test]
fn a_timeline_with_no_events_says_so() {
    let out = timeline(vec![]);
    assert!(out.contains("No events yet"), "{out}");
    assert!(!out.contains("timeline-day"), "{out}");
    assert!(!out.contains("<ol"), "{out}");
}

#[test]
fn one_event_has_no_gap_chip_and_a_time_element() {
    let out = timeline(vec![TimelineEvent::new("Sent").at(T0)]);
    assert!(out.contains("Monday, November 2, 2026"), "{out}");
    assert!(
        out.contains(r#"<time data-slot="timeline-time" datetime="2026-11-02T09:41:07+05:30" dir="ltr">09:41:07</time>"#),
        "{out}"
    );
    assert!(!out.contains("timeline-gap"), "{out}");
    assert_eq!(count(&out, "<li"), 1);
}

#[test]
fn the_time_zone_note_names_the_zone_and_its_offset() {
    let out = timeline(vec![TimelineEvent::new("Sent").at(T0)]);
    assert!(out.contains("Times in Asia/Kolkata (GMT+05:30)"), "{out}");
    let utc = html(Timeline::new("x", "x").event(TimelineEvent::new("Sent").at(T0)));
    assert!(!utc.contains("Times in"), "no zone given, no note: {utc}");
}

#[test]
fn events_are_grouped_by_day_in_the_zone_and_keep_their_order() {
    let day = 86_400;
    let out = timeline(vec![
        TimelineEvent::new("Monday morning").at(T0),
        TimelineEvent::new("Monday night").at(T0 + 3 * 3600 + 1500),
        TimelineEvent::new("Tuesday").at(T0 + day),
        TimelineEvent::new("Thursday").at(T0 + 3 * day),
    ]);
    assert_eq!(count(&out, "<section"), 3, "{out}");
    let monday = out.find("Monday, November 2, 2026").unwrap();
    let tuesday = out.find("Tuesday, November 3, 2026").unwrap();
    let thursday = out.find("Thursday, November 5, 2026").unwrap();
    assert!(monday < tuesday && tuesday < thursday);
    assert!(out.find("Monday morning").unwrap() < out.find("Monday night").unwrap());
    // The day starts at midnight in the zone, not in UTC: 18:30 UTC is 00:00 the next day.
    let late = timeline(vec![
        TimelineEvent::new("a").at(1_793_644_199),
        TimelineEvent::new("b").at(1_793_644_200),
    ]);
    assert_eq!(count(&late, "<section"), 2, "{late}");
}

#[test]
fn every_day_heading_labels_its_list() {
    let out = timeline(vec![
        TimelineEvent::new("a").at(T0),
        TimelineEvent::new("b").at(T0 + 86_400),
    ]);
    assert!(out.contains(r#"aria-labelledby="steps-day-1""#), "{out}");
    assert!(out.contains(r#"id="steps-day-1""#), "{out}");
    assert!(out.contains(r#"id="steps-day-2""#), "{out}");
    assert!(out.contains("<h3"), "{out}");
    let h2 = html(
        Timeline::new("s", "s")
            .heading(2)
            .event(TimelineEvent::new("a").at(T0)),
    );
    assert!(h2.contains("<h2"), "{h2}");
}

#[test]
fn gaps_are_worked_out_from_the_times() {
    let out = timeline(vec![
        TimelineEvent::new("a").at(T0),
        TimelineEvent::new("b").at(T0),
        TimelineEvent::new("c").at(T0 + 45),
        TimelineEvent::new("d").at(T0 + 45 + 60),
        TimelineEvent::new("e").at(T0 + 45 + 60 + 7500),
        TimelineEvent::new("f").at(T0 + 45 + 60 + 7500 + 90_000),
    ]);
    for gap in ["under 1 s", "+45 s", "+1 min", "+2 h 5 min", "+1 d 1 h"] {
        assert!(out.contains(&format!(">{gap}</span>")), "{gap}: {out}");
    }
    assert_eq!(count(&out, "timeline-gap"), 5, "the first has none");
}

#[test]
fn a_given_gap_wins_and_no_gap_leaves_it_out() {
    let out = timeline(vec![
        TimelineEvent::new("a").at(T0),
        TimelineEvent::new("b")
            .at(T0 + 60)
            .gap("+2 min after the reminder"),
        TimelineEvent::new("c").at(T0 + 120).no_gap(),
    ]);
    assert!(out.contains("+2 min after the reminder"), "{out}");
    assert!(!out.contains(">+1 min<"), "{out}");
    assert_eq!(count(&out, "timeline-gap"), 1, "{out}");
}

#[test]
fn a_gap_never_prints_a_minus_whatever_the_order() {
    // Times going backwards in a list that says it runs forwards.
    let out = timeline(vec![
        TimelineEvent::new("a").at(T0 + 120),
        TimelineEvent::new("b").at(T0),
    ]);
    assert!(out.contains(">+2 min<"), "{out}");
    assert!(!out.contains("-2"), "{out}");
}

#[test]
fn a_newest_first_list_measures_to_the_event_below() {
    let out = html(Timeline::new("t", "t").newest_first().events([
        TimelineEvent::new("later").at(T0 + 600),
        TimelineEvent::new("earlier").at(T0),
    ]));
    // "later" came 10 minutes after "earlier"; "earlier" has nothing before it.
    assert_eq!(count(&out, "timeline-gap"), 1, "{out}");
    assert!(out.contains(">+10 min<"), "{out}");
    let chip = out.find("timeline-gap").unwrap();
    assert!(out.find("later").unwrap() < chip && chip < out.find("earlier").unwrap());
}

#[test]
fn the_gap_chip_says_what_it_measures() {
    let out = timeline(vec![
        TimelineEvent::new("a").at(T0),
        TimelineEvent::new("b").at(T0 + 60),
    ]);
    assert!(
        out.contains(r#"title="1 min after the previous step""#),
        "{out}"
    );
}

#[test]
fn text_times_and_explicit_days_need_no_unix_seconds() {
    let out = html(Timeline::new("t", "t").day(
        "Last week",
        [
            TimelineEvent::new("Signed up").time_text("Mon"),
            TimelineEvent::new("Verified").time_text("Tue"),
        ],
    ));
    assert!(out.contains("Last week"), "{out}");
    assert!(out.contains(">Mon<") && out.contains(">Tue<"), "{out}");
    assert!(!out.contains("<time"), "no moment, no <time>: {out}");
    assert!(!out.contains("timeline-gap"), "{out}");
}

#[test]
fn an_event_with_a_time_text_keeps_its_machine_time() {
    let out = timeline(vec![
        TimelineEvent::new("Sent").at(T0).time_text("just now"),
    ]);
    assert!(
        out.contains(r#"datetime="2026-11-02T09:41:07+05:30""#),
        "{out}"
    );
    assert!(out.contains(">just now</time>"), "{out}");
}

#[test]
fn states_tones_and_icons_are_attributes_not_styles() {
    let out = timeline(vec![
        TimelineEvent::new("a").at(T0).tone(Tone::Success),
        TimelineEvent::new("b").at(T0 + 1).current(),
        TimelineEvent::new("c").at(T0 + 2).upcoming(),
        TimelineEvent::new("d").at(T0 + 3).muted().icon(icons::BOT),
        TimelineEvent::new("e")
            .at(T0 + 4)
            .tone(Tone::named("brand")),
    ]);
    assert!(out.contains(r#"data-tone="success""#), "{out}");
    assert!(out.contains(r#"data-state="current""#), "{out}");
    assert!(out.contains(r#"aria-current="step""#), "{out}");
    assert!(out.contains(r#"data-state="upcoming""#), "{out}");
    assert!(out.contains(r#"data-state="muted""#), "{out}");
    assert!(out.contains(r#"data-tone="brand""#), "{out}");
    assert!(
        out.contains("<svg"),
        "an icon indicator draws its glyph: {out}"
    );
    assert!(!out.contains("style="), "{out}");
    assert_eq!(count(&out, r#"aria-current="step""#), 1);
}

#[test]
fn an_upcoming_event_says_so_in_words() {
    let out = timeline(vec![TimelineEvent::new("Renewal").at(T0).upcoming()]);
    assert!(out.contains("Upcoming: "), "{out}");
}

#[test]
fn the_dot_the_connector_and_the_icon_are_hidden_from_assistive_technology() {
    let out = timeline(vec![TimelineEvent::new("a").at(T0).icon(icons::MAIL)]);
    assert!(
        out.contains(r#"<span data-slot="timeline-dot" aria-hidden="true" data-icon>"#),
        "{out}"
    );
}

#[test]
fn meta_items_hold_an_icon_or_any_node_in_the_flag_slot() {
    let flag: Node = el("svg")
        .attr("viewBox", "0 0 3 2")
        .child(Node::Static(
            r##"<rect width="3" height="2" fill="#f93"/>"##,
        ))
        .into();
    let out = timeline(vec![
        TimelineEvent::new("Opened")
            .at(T0)
            .meta(Meta::icon(icons::MONITOR, "Safari on macOS"))
            .meta(Meta::flag(flag, "Mumbai"))
            .meta(Meta::new("maya@example.com").ltr()),
    ]);
    assert_eq!(count(&out, "meta-item"), 3, "{out}");
    assert!(out.contains(r##"fill="#f93""##), "{out}");
    assert!(
        out.contains(r#"<span data-slot="meta-start" aria-hidden="true">"#),
        "{out}"
    );
    assert!(
        out.contains(r#"dir="ltr"><bdi>maya@example.com</bdi>"#),
        "{out}"
    );
}

#[test]
fn hostile_text_stays_text() {
    let nasty = "<script>alert(1)</script><img src=x onerror=alert(2)>";
    let out = timeline(vec![
        TimelineEvent::new(nasty)
            .at(T0)
            .detail(nasty)
            .meta(Meta::new(nasty))
            .gap(nasty),
    ]);
    assert!(!out.contains("<script"), "{out}");
    assert!(!out.contains("<img"), "{out}");
    assert!(out.contains("&lt;script&gt;"), "{out}");
    let feed = html(
        ActivityFeed::new("a", nasty)
            .summary(nasty)
            .item(ActivityItem::new(nasty, nasty).object(nasty).tag(nasty)),
    );
    assert!(
        !feed.contains("<script") && !feed.contains("<img"),
        "{feed}"
    );
    let fact = html(FactCard::new(nasty, nasty).detail(nasty));
    assert!(
        !fact.contains("<script") && !fact.contains("<img"),
        "{fact}"
    );
}

#[test]
fn a_hostile_tone_name_is_neutral_and_writes_nothing() {
    for bad in ["x;y", "", "Brand", "a\"onload=\"x", "9"] {
        let out = timeline(vec![TimelineEvent::new("a").at(T0).tone(Tone::Named(bad))]);
        assert!(!out.contains("data-tone"), "{bad:?}: {out}");
        let badge = html(Badge::new("x").tone(Tone::Named(bad)));
        assert!(!badge.contains("data-tone"), "{bad:?}: {badge}");
    }
}

#[test]
fn extreme_times_do_not_panic() {
    for at in [i64::MIN, -1, 0, i64::MAX] {
        let out = timeline(vec![
            TimelineEvent::new("a").at(at),
            TimelineEvent::new("b").at(i64::MAX),
        ]);
        assert!(out.contains("<time"), "{out}");
    }
}

#[test]
fn filtered_events_are_in_the_page_with_a_notice_that_counts_them() {
    let out = timeline(vec![
        TimelineEvent::new("Visible").at(T0),
        TimelineEvent::new("Machine one")
            .at(T0 + 1)
            .muted()
            .filtered(),
        TimelineEvent::new("Machine two")
            .at(T0 + 2)
            .muted()
            .filtered(),
    ]);
    assert_eq!(count(&out, "data-filtered"), 2, "{out}");
    assert!(out.contains("2 items were hidden."), "{out}");
    assert!(out.contains(r#"type="checkbox""#), "{out}");
    assert!(out.contains("Show them"), "{out}");
    assert!(out.contains("Hide them"), "{out}");
    assert!(out.contains(r#"aria-label="Dismiss""#), "{out}");
    let one = timeline(vec![TimelineEvent::new("x").at(T0).filtered()]);
    assert!(one.contains("1 item was hidden."), "{one}");
    let none = timeline(vec![TimelineEvent::new("x").at(T0)]);
    assert!(!none.contains("hidden-notice"), "{none}");
}

#[test]
fn the_notice_dismisses_with_a_command_and_loads_the_runtime_only_for_that() {
    let with = html(Timeline::new("t", "t").event(TimelineEvent::new("x").at(T0).filtered()));
    assert!(
        with.contains(r#"data-ui-click="[[&quot;hide&quot;,&quot;t-hidden&quot;]]""#)
            || with.contains("data-ui-click"),
        "{with}"
    );
    let view = View::new("x")
        .body(Timeline::new("t", "t").event(TimelineEvent::new("x").at(T0).filtered()));
    assert!(view.uses().has(Uses::CORE));
    let plain = View::new("x").body(Timeline::new("t", "t").event(TimelineEvent::new("x").at(T0)));
    assert_eq!(
        plain.uses(),
        Uses::NONE,
        "a timeline with nothing hidden ships no script"
    );
}

#[test]
fn blocks_and_notes_sit_in_the_list_and_after_it() {
    let out = html(
        Timeline::new("t", "t")
            .event(TimelineEvent::new("a").at(T0))
            .block(Progress::new(40, 100, "Course 40%"))
            .event(TimelineEvent::new("b").at(T0 + 5))
            .note("Nothing else happened."),
    );
    assert!(out.contains("timeline-block"), "{out}");
    assert!(out.contains("Nothing else happened."), "{out}");
    assert!(out.find("timeline-block").unwrap() < out.find("Nothing else happened.").unwrap());
}

#[test]
fn loading_marks_the_timeline_busy() {
    let out = html(Timeline::new("t", "t").loading());
    assert!(out.contains(r#"aria-busy="true""#), "{out}");
    assert!(!out.contains("No events yet"), "{out}");
}

#[test]
fn the_labels_can_be_replaced() {
    let words = DisplayLabels {
        times_in: "Heures : {zone}".into(),
        timeline_empty: "Rien".into(),
        ..DisplayLabels::default()
    };
    let out = html(
        Timeline::new("t", "t")
            .labels(words)
            .time_zone("Europe/Paris", 3600),
    );
    assert!(out.contains("Heures : Europe/Paris (GMT+01:00)"), "{out}");
    assert!(out.contains("Rien"), "{out}");
}

// ActivityFeed

fn row(name: &'static str) -> ActivityItem {
    ActivityItem::new(name, "opened")
        .object("Autumn launch")
        .tone(Tone::Info)
        .icon(icons::MAIL)
        .at(T0)
}

#[test]
fn a_feed_has_a_labelled_list_a_header_and_rows() {
    let out = html(
        ActivityFeed::new("activity", "Recent activity")
            .summary("14 events")
            .time_zone("Asia/Kolkata", KOLKATA)
            .item(
                row("Maya Patel")
                    .meta(Meta::icon(icons::MONITOR, "Safari"))
                    .tag("Autumn"),
            ),
    );
    assert!(out.contains(r#"aria-labelledby="activity-title""#), "{out}");
    assert!(out.contains(r#"id="activity-title""#), "{out}");
    assert!(
        out.contains(r#"<ol data-slot="activity-list" id="activity-list">"#),
        "{out}"
    );
    assert!(out.contains("<bdi>Maya Patel</bdi>"), "{out}");
    assert!(out.contains("14 events"), "{out}");
    assert!(out.contains("Times in Asia/Kolkata (GMT+05:30)"), "{out}");
    assert!(
        out.contains(r#"datetime="2026-11-02T09:41:07+05:30""#),
        "{out}"
    );
    assert!(out.contains(">09:41:07</time>"), "{out}");
    assert!(out.contains(r#"data-tone="info""#), "{out}");
    assert!(out.contains("Autumn"), "{out}");
    assert!(!out.contains("activity-empty"), "{out}");
}

#[test]
fn an_empty_feed_keeps_its_list_so_rows_have_somewhere_to_land() {
    let out = html(ActivityFeed::new("a", "Recent activity"));
    assert!(
        out.contains(r#"<ol data-slot="activity-list" id="a-list">"#),
        "{out}"
    );
    assert!(out.contains(r#"id="a-empty""#), "{out}");
    assert!(out.contains("Nothing yet"), "{out}");
    let custom = html(
        ActivityFeed::new("a", "t")
            .empty(Empty::new("Quiet window").description("Try a longer one.")),
    );
    assert!(
        custom.contains("Quiet window") && custom.contains("Try a longer one."),
        "{custom}"
    );
}

#[test]
fn a_linked_row_is_a_real_link_on_the_actor() {
    let out = html(ActivityFeed::new("a", "t").item(row("Maya").href("/people/7")));
    assert!(
        out.contains(r#"<a data-slot="activity-actor" href="/people/7"><bdi>Maya</bdi></a>"#),
        "{out}"
    );
    assert!(out.contains("data-link"), "{out}");
    let plain = html(ActivityFeed::new("a", "t").item(row("Maya")));
    assert!(!plain.contains("<a "), "{plain}");
}

#[test]
fn a_javascript_url_on_a_row_goes_nowhere() {
    let out = html(ActivityFeed::new("a", "t").item(row("Maya").href("javascript:alert(1)")));
    assert!(!out.contains("javascript:"), "{out}");
}

#[test]
fn the_live_dot_has_a_word_for_every_state() {
    let out = html(ActivityFeed::new("a", "t").live());
    for word in ["Live", "Reconnecting", "Paused"] {
        assert!(out.contains(&format!(">{word}</span>")), "{word}: {out}");
    }
    assert!(out.contains(r#"role="status""#), "{out}");
    assert!(!html(ActivityFeed::new("a", "t")).contains("live-state"));
}

#[test]
fn a_loading_feed_is_busy_and_a_failed_one_offers_a_retry() {
    let loading = html(ActivityFeed::new("a", "t").loading());
    assert!(
        loading.contains(r#"aria-busy="true""#) && loading.contains("Loading"),
        "{loading}"
    );
    let failed = html(
        ActivityFeed::new("a", "t")
            .failed("The activity could not load.")
            .retry("/activity?retry=1"),
    );
    assert!(failed.contains("The activity could not load."), "{failed}");
    assert!(
        failed.contains(r#"href="/activity?retry=1""#) && failed.contains("Try again"),
        "{failed}"
    );
    assert!(failed.contains(r#"role="alert""#), "{failed}");
    assert!(
        failed.contains("activity-list"),
        "the header and the list stay: {failed}"
    );
}

#[test]
fn a_feed_scrolls_inside_a_height_from_a_closed_list() {
    let out = html(
        ActivityFeed::new("a", "t")
            .height(FeedHeight::Md)
            .item(row("x")),
    );
    assert!(out.contains(r#"data-height="md""#), "{out}");
    assert!(!out.contains("style="), "{out}");
}

#[test]
fn rows_left_out_come_back_with_the_notice() {
    let out = html(
        ActivityFeed::new("a", "t")
            .item(row("Visible"))
            .item(row("Hidden one").filtered().muted())
            .item(row("Hidden two").filtered()),
    );
    assert!(out.contains("2 items were hidden."), "{out}");
    assert_eq!(count(&out, "data-filtered"), 2, "{out}");
    assert!(out.contains(r#"data-state="muted""#), "{out}");
    // The notice sits between the header and the list.
    assert!(out.find("activity-header").unwrap() < out.find("hidden-notice\"").unwrap());
    assert!(out.find("hidden-notice\"").unwrap() < out.find("activity-list").unwrap());
}

#[test]
fn a_row_with_an_id_can_be_told_from_one_that_arrives_again() {
    let out = html(ActivityFeed::new("a", "t").item(row("x").id("event-42")));
    assert!(out.contains(r#"id="event-42""#), "{out}");
}

#[test]
fn a_feed_in_a_region_listens_to_a_stream_and_feeds_its_list() {
    let region = ActivityFeed::new("activity", "Recent activity").in_region(
        "activity-region",
        "/fragments/activity",
        "/events",
        "42",
    );
    let out = html(region);
    assert!(out.contains(r#"id="activity-region""#), "{out}");
    assert!(
        out.contains(r#"data-ui-src="/fragments/activity""#),
        "{out}"
    );
    assert!(out.contains(r#"data-ui-live-src="/events""#), "{out}");
    assert!(out.contains(r#"data-ui-feed="activity-list""#), "{out}");
    assert!(out.contains(r#"data-ui-after="42""#), "{out}");
    assert!(
        out.contains(r#"data-ui-feed-empty="activity-empty""#),
        "{out}"
    );
    assert!(out.contains(r#"data-ui-feed-say="{n} new items""#), "{out}");
    assert!(
        out.contains(r#"data-ui-feed-say-one="1 new item""#),
        "{out}"
    );
    assert!(
        out.contains("live-state"),
        "the feed shows its live dot: {out}"
    );
}

#[test]
fn the_rows_alone_are_what_a_live_source_answers() {
    let out = html(ActivityFeed::rows(
        [row("Maya").id("event-1"), row("Tomas").id("event-2")],
        KOLKATA,
    ));
    assert!(out.starts_with("<li"), "{out}");
    assert_eq!(count(&out, "<li"), 2, "{out}");
    assert!(!out.contains("<ol") && !out.contains("<section"), "{out}");
    assert!(
        out.contains(">09:41:07</time>"),
        "the zone is the feed's own: {out}"
    );
    assert_eq!(html(ActivityFeed::rows([], 0)), "");
}

// Region

#[test]
fn live_from_names_a_stream_and_loads_the_core_runtime() {
    let region = Region::new("deals")
        .src("/fragments/deals")
        .live_from("/events")
        .live_event("deals");
    let view = View::new("x").body(region.clone());
    assert!(view.uses().has(Uses::CORE));
    let out = html(region);
    assert!(out.contains(r#"data-ui-live-src="/events""#), "{out}");
    assert!(out.contains(r#"data-ui-live-event="deals""#), "{out}");
}

#[test]
fn an_event_name_is_checked_and_a_bad_one_is_dropped() {
    for ok in ["deals", "a.b:c-d_e", "X1"] {
        assert!(
            html(Region::new("r").live_event(ok)).contains("data-ui-live-event"),
            "{ok}"
        );
    }
    for bad in [
        "",
        "a b",
        "a\"b",
        "a<b",
        "é",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ] {
        assert!(
            !html(Region::new("r").live_event(bad)).contains("data-ui-live-event"),
            "{bad:?}"
        );
    }
}

#[test]
fn a_stream_on_another_site_is_not_a_url_the_toolkit_writes() {
    let out = html(Region::new("r").live_from("javascript:alert(1)"));
    assert!(!out.contains("javascript:"), "{out}");
}

#[test]
fn feed_limit_and_announcements_are_attributes() {
    let out = html(
        Region::new("r")
            .feed("list", "0", None)
            .feed_limit(50)
            .announce_new("{n} new", "one new"),
    );
    assert!(out.contains(r#"data-ui-feed-max="50""#), "{out}");
    assert!(out.contains(r#"data-ui-feed-say="{n} new""#), "{out}");
    assert!(out.contains(r#"data-ui-feed-say-one="one new""#), "{out}");
}

// FactCard and FactGrid

#[test]
fn a_fact_card_is_a_term_and_its_descriptions() {
    let out = html(
        FactCard::new("Device", "iPhone 15")
            .icon(icons::MONITOR)
            .tone(Tone::Info)
            .detail("Safari 18")
            .detail("iOS 19"),
    );
    assert!(out.contains("<dl"), "{out}");
    assert!(
        out.contains(r#"<dt data-slot="fact-label">Device</dt>"#),
        "{out}"
    );
    assert!(
        out.contains(r#"<dd data-slot="fact-value"><bdi>iPhone 15</bdi></dd>"#),
        "{out}"
    );
    assert_eq!(count(&out, "fact-detail"), 2, "{out}");
    assert!(out.contains(r#"data-tone="info""#), "{out}");
    assert!(out.contains(r#"aria-hidden="true""#), "{out}");
}

#[test]
fn a_fact_card_without_an_icon_has_no_icon_column() {
    let out = html(FactCard::new("Opens", "3"));
    assert!(
        !out.contains("fact-icon") && !out.contains("data-icon"),
        "{out}"
    );
}

#[test]
fn a_fact_grid_holds_cards_and_can_fix_its_columns() {
    let out = html(
        FactGrid::new()
            .cols(4)
            .cards((0..5).map(|n| FactCard::new("Label", n.to_string()))),
    );
    assert_eq!(count(&out, r#"data-slot="fact-card""#), 5, "{out}");
    assert!(out.contains(r#"data-cols="4""#), "{out}");
    assert!(!html(FactGrid::new()).contains("data-cols"));
    assert!(html(FactGrid::new().cols(99)).contains(r#"data-cols="6""#));
}

#[test]
fn a_page_of_display_components_ships_no_script() {
    let page = View::new("x").body((
        Timeline::new("t", "t").event(TimelineEvent::new("a").at(T0)),
        ActivityFeed::new("a", "t").item(row("x")),
        FactGrid::new().card(FactCard::new("a", "b")),
        Badge::outcome("Opened", true),
    ));
    assert_eq!(page.uses(), Uses::NONE);
    assert!(!page.to_html().contains("<script"));
}

// Badge

#[test]
fn a_badge_can_have_a_dot_a_tone_and_icons_at_both_ends() {
    let out = html(
        Badge::new("Delivered")
            .tone(Tone::Success)
            .dot()
            .icon(icons::CHECK)
            .icon_end(icons::ARROW_RIGHT),
    );
    assert!(out.contains(r#"data-tone="success""#), "{out}");
    assert!(
        out.contains(r#"<span data-slot="badge-dot" aria-hidden="true"></span>"#),
        "{out}"
    );
    assert_eq!(count(&out, "<svg"), 2, "{out}");
    let dot = out.find("badge-dot").unwrap();
    let text = out.find("Delivered").unwrap();
    let end = out.rfind("<svg").unwrap();
    assert!(
        dot < text && text < end,
        "dot, then text, then the trailing icon: {out}"
    );
}

#[test]
fn the_existing_variants_still_write_what_they_did() {
    let out = html(Badge::new("Won").variant(BadgeVariant::Success));
    assert_eq!(
        out,
        r#"<span data-slot="badge" data-variant="success">Won</span>"#
    );
}

#[test]
fn an_outcome_chip_says_yes_or_no_in_words_for_a_screen_reader() {
    let yes = html(Badge::outcome("Opened", true).tone(Tone::Info));
    assert!(yes.contains(r#"data-state="reached""#), "{yes}");
    assert!(
        yes.contains(r#"<span data-ui-sr>yes</span>"#) || yes.contains("yes</span>"),
        "{yes}"
    );
    let no = html(Badge::outcome("Clicked", false));
    assert!(no.contains(r#"data-state="missed""#), "{no}");
    assert!(no.contains("no</span>"), "{no}");
    let french = html(Badge::outcome("Ouvert", true).outcome_words("oui", "non"));
    assert!(french.contains("oui</span>"), "{french}");
}
