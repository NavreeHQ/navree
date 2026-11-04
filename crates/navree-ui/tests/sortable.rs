//! Sortable lists and boards: what the page draws, what a move posts, and the check
//! the server holds it to.

use http::StatusCode;
use navree_http::body::Body;
use navree_http::extract::FromRequest;
use navree_http::response::IntoResponse;
use navree_ui::{
    ListState, Move, MoveError, Refusal, SortItem, SortLabels, SortList, Sortable, TestView, Uses,
    View, render,
};
use proptest::prelude::*;

fn board() -> Sortable {
    Sortable::new("sprint", "Sprint board")
        .board()
        .post("/board/moves")
        .list(
            SortList::new("ready", "Ready")
                .item(SortItem::new("12", "Fix the sign-in redirect"))
                .item(SortItem::new("14", "Write the release notes")),
        )
        .list(
            SortList::new("review", "Review")
                .limit(1)
                .item(SortItem::new("9", "Card export")),
        )
        .list(SortList::new("done", "Done").refuse("Only a lead can move cards to Done."))
}

fn html(sortable: Sortable) -> String {
    render(&sortable.into())
}

#[test]
fn a_board_draws_its_lists_with_titles_counts_and_limits() {
    let html = html(board());
    assert!(html.contains(r#"data-slot="sortable" id="sprint" data-layout="board" role="group" aria-label="Sprint board" data-ui-sortable"#), "{html}");
    assert!(html.contains(r#"<section data-slot="sortable-list" id="sprint-list-ready" data-list="ready" aria-labelledby="sprint-list-ready-title">"#), "{html}");
    assert!(html.contains(
        r#"<h2 data-slot="sortable-list-title" id="sprint-list-ready-title">Ready</h2>"#
    ));
    // A list with a limit says it twice: drawn short, and in words for a screen reader.
    assert!(html.contains(r#"data-limit="1""#));
    assert!(html.contains(r#"<span data-slot="sortable-count" data-full><span aria-hidden="true" data-count>1/1</span><span data-ui-sr data-count-text>1 of at most 1</span></span>"#), "{html}");
    // A list with no limit shows its count alone, read as it is.
    assert!(
        html.contains(r#"<span data-slot="sortable-count"><span data-count>2</span></span>"#),
        "{html}"
    );
    assert!(html.contains(r#"data-refuse="Only a lead can move cards to Done.""#));
    // An empty list says so.
    assert!(html.contains(r#"<p data-slot="sortable-empty">Nothing here yet</p>"#));
}

#[test]
fn each_item_has_a_handle_that_says_how_to_move_it() {
    let html = html(board());
    assert!(html.contains(r#"<li data-slot="sortable-item" id="sprint-item-12" data-key="12" data-name="Fix the sign-in redirect">"#), "{html}");
    assert!(html.contains(r#"<button data-slot="sortable-handle" type="button" aria-label="Move Fix the sign-in redirect" aria-describedby="sprint-help">"#), "{html}");
    assert!(html.contains(
        r#"<p data-slot="sortable-help" id="sprint-help" hidden>Press Space to pick it up"#
    ));
    assert!(html.contains(r#"<div data-slot="sortable-status" id="sprint-status" aria-live="assertive" aria-atomic="true" data-ui-sr></div>"#));
}

#[test]
fn each_item_has_a_form_that_moves_it_with_javascript_off() {
    let html = html(board());
    assert!(html.contains(r#"<form data-slot="sortable-form" method="post" action="/board/moves" data-ui-form data-ui-move data-ui-deadline="15000">"#), "{html}");
    for field in [
        r#"<input type="hidden" name="item" value="14">"#,
        r#"<input type="hidden" name="from_list" value="ready">"#,
        r#"<input type="hidden" name="before" value="">"#,
        r#"<select data-slot="native-select" id="sprint-item-14-to" name="to_list">"#,
        r#"<option value="ready" selected>Ready</option>"#,
        // A full list and one that refuses are offered, switched off: the server says why if they're sent anyway.
        r#"<option value="review" disabled>Review (full)</option>"#,
        r#"<option value="done" disabled>Done</option>"#,
        r#"<input data-slot="input" id="sprint-item-14-position" type="number" name="position" min="1" inputmode="numeric" value="2">"#,
        r#"<label data-slot="sortable-move-field" for="sprint-item-14-to">To</label>"#,
    ] {
        assert!(html.contains(field), "{field} in {html}");
    }
    // The item in a full list keeps its own list as a choice.
    assert!(
        html.contains(r#"<option value="review" selected>Review (full)</option>"#),
        "{html}"
    );
}

#[test]
fn a_page_with_a_sortable_loads_the_core_and_the_sortable_module() {
    let view = View::new("Board").body(board());
    assert_eq!(view.uses(), Uses::CORE.with(Uses::SORTABLE));
    let page = view.to_html();
    assert!(page.contains(navree_ui::assets::SORTABLE_URL));
    assert!(page.contains(navree_ui::assets::CORE_URL));
}

#[test]
fn without_somewhere_to_post_nothing_moves_and_no_script_ships() {
    let still = Sortable::new("plan", "Plan")
        .list(SortList::new("a", "Steps").item(SortItem::new("1", "Measure")));
    let view = View::new("Plan").body(still.clone());
    assert!(view.uses().is_none());
    let html = html(still);
    for absent in [
        "data-ui-sortable",
        "sortable-handle",
        "<form",
        "sortable-status",
        "data-l=",
    ] {
        assert!(!html.contains(absent), "{absent} in {html}");
    }
    assert!(html.contains("<span>Measure</span>"));
}

#[test]
fn a_locked_item_shows_why_and_has_no_form() {
    let html = html(
        Sortable::new("s", "S").post("/m").list(
            SortList::new("a", "A")
                .item(SortItem::new("1", "Invoice run").locked("A payment is being taken.")),
        ),
    );
    assert!(html.contains(r#"data-locked="A payment is being taken.""#));
    assert!(html.contains(r#"aria-disabled="true""#));
    assert!(!html.contains("<form"), "{html}");
}

#[test]
fn text_from_anywhere_is_escaped_and_ids_stay_plain() {
    let evil = r#""><script>alert(1)</script><img src=x onerror=alert(2)>"#;
    let html = html(
        Sortable::new("s", evil)
            .post("/m")
            .list(
                SortList::new(evil, evil)
                    .item(SortItem::new(evil, evil))
                    .item(SortItem::new("a b", "Space in its key"))
                    .actions(evil)
                    .footer(evil),
            )
            .list(
                SortList::new("x", "Second")
                    .limit(1)
                    .item(SortItem::new(evil, "Same key twice")),
            ),
    );
    assert!(!html.contains("<script"), "{html}");
    assert!(!html.contains("<img"), "{html}");
    // Ids made from keys never carry what the keys do.
    for id in html.split(" id=\"").skip(1) {
        let id = id.split('"').next().unwrap_or_default();
        assert!(
            id.starts_with('s') || id.starts_with('&'),
            "an id starts with the sortable's own: {id}"
        );
        assert!(!id.contains(' ') || id.starts_with("&quot;"), "{id}");
    }
    // Two items with one key still get two ids.
    let ids: Vec<&str> = html
        .match_indices(r#"data-slot="sortable-item" id=""#)
        .map(|(at, found)| {
            let rest = &html[at + found.len()..];
            &rest[..rest.find('"').unwrap_or(0)]
        })
        .collect();
    assert_eq!(ids.len(), 3);
    assert_ne!(ids[0], ids[2], "{ids:?}");
}

#[test]
fn the_words_reach_the_browser_as_json_in_an_attribute() {
    let labels = SortLabels {
        picked: "Levantado {item}".into(),
        ..SortLabels::default()
    };
    let html = html(board().labels(labels));
    assert!(html.contains("data-l=\"{&quot;"), "{html}");
    assert!(html.contains("Levantado {item}"));
}

#[test]
fn a_sortable_reads_in_a_test_view() {
    let page = TestView::of(&View::new("Board").body(board()));
    assert!(page.sees("Fix the sign-in redirect"));
}

#[test]
fn heading_levels_are_kept_between_two_and_six() {
    assert!(html(board().heading(3)).contains("<h3 data-slot=\"sortable-list-title\""));
    assert!(html(board().heading(9)).contains("<h6 data-slot=\"sortable-list-title\""));
    assert!(html(board().heading(0)).contains("<h2 data-slot=\"sortable-list-title\""));
}

/* What a move posts. */

#[test]
fn a_move_reads_from_a_form_body() {
    let mv = Move::from_form(b"item=12&from_list=ready&to_list=review&before=9&position=").unwrap();
    assert_eq!(mv, Move::new("12", "ready", "review").before("9"));
    let mv = Move::from_form(b"item=12&from_list=ready&to_list=ready&before=&position=3").unwrap();
    assert_eq!(mv, Move::new("12", "ready", "ready").at(3));
    // The first of each field counts, and fields a move doesn't have are passed over.
    let mv = Move::from_form(b"_csrf=t&item=1&item=2&from_list=a&to_list=b&x=y").unwrap();
    assert_eq!(mv.item, "1");
    // Keys keep what they hold, spaces and all.
    let mv = Move::from_form("item=a%20b&from_list=%C3%A9&to_list=b".as_bytes()).unwrap();
    assert_eq!((mv.item.as_str(), mv.from_list.as_str()), ("a b", "é"));
    // A huge position is the end.
    let mv = Move::from_form(b"item=1&from_list=a&to_list=b&position=999999999999").unwrap();
    assert_eq!(mv.position, Some(1_000_000));
}

#[test]
fn a_body_that_is_not_a_move_is_refused() {
    for (body, why) in [
        (&b""[..], MoveError::Field("item")),
        (b"item=&from_list=a&to_list=b", MoveError::Field("item")),
        (b"item=1&to_list=b", MoveError::Field("from_list")),
        (b"item=1&from_list=a", MoveError::Field("to_list")),
        (
            b"item=1&from_list=a&to_list=b&position=-1",
            MoveError::Field("position"),
        ),
        (
            b"item=1&from_list=a&to_list=b&position=2.5",
            MoveError::Field("position"),
        ),
        (
            b"item=1&from_list=a&to_list=b&position=1e3",
            MoveError::Field("position"),
        ),
        (
            b"item=1&from_list=a&to_list=b&position=9999999999999",
            MoveError::Field("position"),
        ),
    ] {
        assert_eq!(
            Move::from_form(body),
            Err(why),
            "{}",
            String::from_utf8_lossy(body)
        );
    }
    let long = format!("item={}&from_list=a&to_list=b", "k".repeat(257));
    assert_eq!(
        Move::from_form(long.as_bytes()),
        Err(MoveError::Field("item"))
    );
    let long = format!("item=1&from_list=a&to_list=b&before={}", "k".repeat(257));
    assert_eq!(
        Move::from_form(long.as_bytes()),
        Err(MoveError::Field("before"))
    );
    let many = "a=1&".repeat(40);
    assert_eq!(Move::from_form(many.as_bytes()), Err(MoveError::TooLarge));
}

fn post(body: &'static str) -> http::request::Builder {
    let _ = body;
    http::Request::builder()
        .method("POST")
        .uri("/board/moves")
        .header("content-type", "application/x-www-form-urlencoded")
}

#[tokio::test]
async fn a_move_is_read_from_a_post_and_forgery_is_refused() {
    let body = "item=12&from_list=ready&to_list=review";
    let request = post(body).body(Body::from(body)).unwrap();
    let mv = Move::from_request(request).await.unwrap();
    assert_eq!(mv, Move::new("12", "ready", "review"));

    // Another site can't move anything.
    let request = post(body)
        .header("sec-fetch-site", "cross-site")
        .body(Body::from(body))
        .unwrap();
    let refused = Move::from_request(request).await.unwrap_err();
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);

    // A link or a prefetch can't either.
    let request = http::Request::builder()
        .method("GET")
        .uri("/board/moves?item=12&from_list=ready&to_list=review")
        .body(Body::empty())
        .unwrap();
    let refused = Move::from_request(request).await.unwrap_err();
    assert_eq!(refused.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(refused.into_response().headers()["allow"], "POST");

    // A body that isn't a form.
    let request = http::Request::builder()
        .method("POST")
        .uri("/board/moves")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    assert_eq!(
        Move::from_request(request).await.unwrap_err().status(),
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );

    // A form that isn't a move: from the runtime, a toast that says so.
    let request = post("item=")
        .header("x-navree-ui", "1")
        .body(Body::from("item="))
        .unwrap();
    let refused = Move::from_request(request).await.unwrap_err();
    assert_eq!(refused.status(), StatusCode::BAD_REQUEST);
    let response = refused.into_response();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(
        response.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("application/vnd.navree.ops+json")
    );
}

/* The check. */

fn lists() -> Vec<ListState> {
    vec![
        ListState::new("ready", "Ready", ["1", "2", "3"]),
        ListState::new("doing", "In progress", ["4", "5"]).limit(2),
        ListState::new("done", "Done", Vec::<String>::new()).refuse("Only a lead can close cards."),
    ]
}

#[test]
fn a_move_within_a_list_reorders_it() {
    let landing = Move::new("3", "ready", "ready")
        .before("1")
        .check(&lists())
        .unwrap();
    assert_eq!(landing.order, ["3", "1", "2"]);
    assert_eq!(landing.index, 0);
    assert!(landing.left.is_empty());
    assert!(landing.moved);
    let landing = Move::new("1", "ready", "ready").check(&lists()).unwrap();
    assert_eq!(landing.order, ["2", "3", "1"]);
    // Dropped where it was: nothing changes, and the landing says so.
    let landing = Move::new("1", "ready", "ready")
        .before("2")
        .check(&lists())
        .unwrap();
    assert_eq!(landing.order, ["1", "2", "3"]);
    assert!(!landing.moved);
}

#[test]
fn a_move_between_lists_says_both_new_orders() {
    let mut lists = lists();
    lists[1].limit = Some(3);
    let landing = Move::new("2", "ready", "doing")
        .before("5")
        .check(&lists)
        .unwrap();
    assert_eq!(landing.order, ["4", "2", "5"]);
    assert_eq!(landing.left, ["1", "3"]);
    assert_eq!(
        (landing.from_list.as_str(), landing.to_list.as_str()),
        ("ready", "doing")
    );
}

#[test]
fn a_position_from_a_form_counts_from_one_and_stops_at_the_end() {
    let landing = Move::new("1", "ready", "ready")
        .at(2)
        .check(&lists())
        .unwrap();
    assert_eq!(landing.order, ["2", "1", "3"]);
    let landing = Move::new("1", "ready", "ready")
        .at(0)
        .check(&lists())
        .unwrap();
    assert_eq!(landing.order, ["1", "2", "3"]);
    let landing = Move::new("1", "ready", "ready")
        .at(1_000_000)
        .check(&lists())
        .unwrap();
    assert_eq!(landing.order, ["2", "3", "1"]);
    // An anchor wins over a position.
    let landing = Move::new("3", "ready", "ready")
        .before("2")
        .at(1)
        .check(&lists())
        .unwrap();
    assert_eq!(landing.order, ["1", "3", "2"]);
}

#[test]
fn a_full_list_takes_nothing_from_another_but_still_reorders() {
    assert_eq!(
        Move::new("1", "ready", "doing").check(&lists()),
        Err(Refusal::Full {
            list: "In progress".to_owned(),
            limit: 2
        })
    );
    assert_eq!(
        Refusal::Full {
            list: "In progress".to_owned(),
            limit: 2
        }
        .message(),
        "In progress holds at most 2."
    );
    let landing = Move::new("5", "doing", "doing")
        .before("4")
        .check(&lists())
        .unwrap();
    assert_eq!(landing.order, ["5", "4"]);
    // A list already over its limit (it was lowered) still reorders.
    let mut lists = lists();
    lists[1].limit = Some(1);
    assert!(
        Move::new("5", "doing", "doing")
            .before("4")
            .check(&lists)
            .is_ok()
    );
}

#[test]
fn a_list_that_refuses_says_why() {
    let refusal = Move::new("1", "ready", "done").check(&lists()).unwrap_err();
    assert_eq!(refusal.message(), "Only a lead can close cards.");
    assert!(!refusal.is_stale());
}

#[test]
fn a_stale_move_is_refused() {
    // Someone moved it to another list first.
    let refusal = Move::new("4", "ready", "ready")
        .check(&lists())
        .unwrap_err();
    assert_eq!(
        refusal,
        Refusal::Stale {
            list: "In progress".to_owned()
        }
    );
    assert!(refusal.is_stale());
    // The item it was dropped before has left.
    assert_eq!(
        Move::new("1", "ready", "ready").before("4").check(&lists()),
        Err(Refusal::Moved)
    );
    // It was deleted, or the list was.
    assert_eq!(
        Move::new("99", "ready", "ready").check(&lists()),
        Err(Refusal::Gone)
    );
    assert_eq!(
        Move::new("1", "ready", "gone").check(&lists()),
        Err(Refusal::NoList)
    );
    // Before itself makes no sense.
    assert_eq!(
        Move::new("1", "ready", "ready").before("1").check(&lists()),
        Err(Refusal::Malformed)
    );
}

#[test]
fn two_moves_at_once_resolve_in_the_order_the_server_takes_them() {
    // Two people drag the last free place in "In progress": whoever the server takes first
    // gets it, and the other is refused, whatever order they were made in.
    let mut lists = lists();
    lists[1] = ListState::new("doing", "In progress", ["4"]).limit(2);
    let first = Move::new("1", "ready", "doing");
    let second = Move::new("2", "ready", "doing");
    let landing = first.check(&lists).unwrap();
    lists[0].items = landing.left.clone();
    lists[1].items = landing.order.clone();
    assert!(matches!(second.check(&lists), Err(Refusal::Full { .. })));
    // Two people move the same card: the second was made from where it no longer is.
    let a = Move::new("3", "ready", "done");
    let b = Move::new("3", "ready", "ready").before("1");
    lists[2].refuse = None;
    let landing = a.check(&lists).unwrap();
    lists[0].items = landing.left.clone();
    lists[2].items = landing.order.clone();
    assert!(matches!(b.check(&lists), Err(Refusal::Stale { .. })));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2_000))]

    /// No move, however odd, panics; one that lands keeps every item exactly once,
    /// keeps the others in their order, and holds every limit.
    #[test]
    fn any_move_lands_cleanly_or_is_refused(
        sizes in proptest::collection::vec(0usize..6, 1..5),
        limits in proptest::collection::vec(proptest::option::of(0u32..6), 5),
        item in 0usize..30,
        from in 0usize..6,
        to in 0usize..6,
        before in proptest::option::of(0usize..30),
        position in proptest::option::of(0u32..10),
    ) {
        let mut next = 0;
        let lists: Vec<ListState> = sizes
            .iter()
            .enumerate()
            .map(|(at, size)| {
                let items: Vec<String> = (0..*size).map(|_| { next += 1; next.to_string() }).collect();
                ListState::new(format!("l{at}"), format!("List {at}"), items).limit(limits[at])
            })
            .collect();
        let mut mv = Move::new(item.to_string(), format!("l{from}"), format!("l{to}"));
        mv.before = before.map(|b| b.to_string());
        mv.position = position;
        let total: usize = lists.iter().map(|l| l.items.len()).sum();
        if let Ok(landing) = mv.check(&lists) {
            let mut after = lists.clone();
            for list in &mut after {
                if list.key == landing.to_list {
                    list.items = landing.order.clone();
                } else if list.key == landing.from_list {
                    list.items = landing.left.clone();
                }
            }
            let mut all: Vec<&String> = after.iter().flat_map(|l| &l.items).collect();
            prop_assert_eq!(all.len(), total);
            all.sort();
            all.dedup();
            prop_assert_eq!(all.len(), total, "every item once");
            prop_assert_eq!(&landing.order[landing.index], &mv.item);
            // The others in the list it went to keep their order.
            let target = &lists.iter().find(|l| l.key == landing.to_list).unwrap().items;
            let kept: Vec<&String> = target.iter().filter(|k| **k != mv.item).collect();
            let now: Vec<&String> = landing.order.iter().filter(|k| **k != mv.item).collect();
            prop_assert_eq!(kept, now);
            if landing.from_list != landing.to_list
                && let Some(limit) = lists.iter().find(|l| l.key == landing.to_list).unwrap().limit
            {
                prop_assert!(landing.order.len() <= limit as usize);
            }
        }
    }

    /// No body, however odd, panics the reader.
    #[test]
    fn any_body_reads_or_is_refused(body in proptest::collection::vec(any::<u8>(), 0..400)) {
        let _ = Move::from_form(&body);
    }

    /// Anything as a key, a name or a title is drawn escaped.
    #[test]
    fn any_text_is_drawn_escaped(text in "\\PC{0,40}") {
        let html = html(
            Sortable::new("s", text.clone())
                .post("/m")
                .list(SortList::new(text.clone(), text.clone()).item(SortItem::new(text.clone(), text.clone()))),
        );
        prop_assert!(!html.contains("<script"));
        let opened = html.matches('<').count();
        let tags = html.matches("<li").count();
        prop_assert!(opened > 0 && tags == 1);
    }
}
