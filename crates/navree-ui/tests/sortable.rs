//! Sortable lists and boards: what the page draws, what a move posts, and the check
//! the server holds it to.

use http::StatusCode;
use navree_http::body::Body;
use navree_http::extract::FromRequest;
use navree_http::response::IntoResponse;
use navree_ui::sortable::{Placement, RANK_GAP, RankedLists, Spot, rank_between};
use navree_ui::{
    ListState, Move, MoveError, Op, Refusal, SortItem, SortLabels, SortList, Sortable, TestView,
    Uses, View, render,
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
    assert!(html.contains(r#"<section data-slot="sortable-list" id="sprint-list-ready" data-list="ready" aria-labelledby="sprint-list-ready-title" data-total="2">"#), "{html}");
    assert!(html.contains(r#"<ol data-slot="sortable-items" id="sprint-list-ready-items" aria-labelledby="sprint-list-ready-title">"#), "{html}");
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
    // A tabindex of its own: WebKit gives a button it clicks the focus only when it has one, and a
    // lifted card's keys and its second click need the focus on the handle.
    assert!(html.contains(r#"<button data-slot="sortable-handle" type="button" tabindex="0" aria-label="Move Fix the sign-in redirect" aria-describedby="sprint-help">"#), "{html}");
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
        r#"<input type="hidden" name="after" value="">"#,
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
    // The toasts the module shows itself, for a move with no answer, are words too.
    let html = html_of(board());
    assert!(
        html.contains("&quot;failed&quot;:&quot;That move wasn&#39;t saved. Try again.&quot;"),
        "{html}"
    );
    assert!(
        html.contains(
            "&quot;offline&quot;:&quot;No connection. That move may not have been saved.&quot;"
        ),
        "{html}"
    );
    // And what a refused item's redraw says when the server has it out of sight.
    assert!(
        html.contains("&quot;gone&quot;:&quot;{item} is no longer shown here.&quot;"),
        "{html}"
    );
}

#[test]
fn a_sortable_reads_in_a_test_view() {
    let page = TestView::of(&View::new("Board").body(board()));
    assert!(page.sees("Fix the sign-in redirect"));
}

#[test]
fn ids_made_from_keys_never_meet() {
    // "5" with "-to" is the id of 5's move field; a key "5-to" must not make the same.
    let html = html(
        Sortable::new("s", "S").post("/m").list(
            SortList::new("a", "A")
                .item(SortItem::new("5", "Five"))
                .item(SortItem::new("5-to", "Five to"))
                .item(SortItem::new("5-position", "Five position")),
        ),
    );
    let mut ids: Vec<&str> = html
        .split(" id=\"")
        .skip(1)
        .map(|rest| rest.split('"').next().unwrap_or_default())
        .collect();
    let all = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), all, "{ids:?}");
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
fn a_move_a_native_client_posts_is_read_in_the_protocols_words() {
    // Keys for the rows and the lists, and a position counted from 0.
    let body = b"_csrf=t&item=12&from=ready&to=review&before=9&position=2";
    let mv = Move::from_form(body).unwrap();
    // The row it now stands before is where it goes; the number is kept, from 1.
    assert_eq!(mv, Move::new("12", "ready", "review").before("9").at(3));
    // At the end there is no row to go before: the place is the number, which a form counts from 1.
    let body = b"item=12&from=ready&to=ready&before=&position=4";
    let mv = Move::from_form(body).unwrap();
    assert_eq!((mv.before, mv.position), (None, Some(5)));
    // The names are the protocol's.
    #[cfg(feature = "native-preview")]
    {
        use navree_native::node::Reorder;
        let body = format!(
            "{}=12&{}=a&{}=b&{}=&{}=0",
            Reorder::ITEM,
            Reorder::FROM,
            Reorder::TO,
            Reorder::BEFORE,
            Reorder::POSITION
        );
        assert_eq!(Move::from_form(body.as_bytes()).unwrap().position, Some(1));
    }
    // A form's own position counts from 1.
    let mv = Move::from_form(b"item=12&from_list=a&to_list=a&position=3").unwrap();
    assert_eq!(mv.position, Some(3));
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

/* Long lists. */

#[test]
fn a_long_list_counts_every_item_and_ends_with_a_button_for_the_next() {
    let html = html(
        Sortable::new("b", "Board").post("/m").list(
            SortList::new("backlog", "Backlog")
                .total(12_034)
                .item(SortItem::new("1", "First"))
                .more("/columns/backlog", [("after", "65536.1"), ("show", "40")]),
        ),
    );
    assert!(html.contains(r#"data-total="12034" data-paged"#), "{html}");
    assert!(html.contains("<span data-count>12034</span>"), "{html}");
    assert!(html.contains(r#"<form data-slot="sortable-more" id="b-list-backlog-more" method="get" action="/columns/backlog" data-ui-form data-ui-autoload><input type="hidden" name="after" value="65536.1"><input type="hidden" name="show" value="40"><button data-slot="button" type="submit" data-variant="ghost" data-size="sm">Load more</button></form>"#), "{html}");
    let html = html_of(
        Sortable::new("b", "Board")
            .post("/m")
            .list(SortList::new("done", "Done").end_note("Showing the first 200 of 8,000.")),
    );
    assert!(html.contains(r#"<p data-slot="sortable-more" id="b-list-done-more">Showing the first 200 of 8,000.</p>"#), "{html}");
}

fn html_of(sortable: Sortable) -> String {
    render(&sortable.into())
}

#[test]
fn under_a_filter_each_items_move_form_starts_at_its_place_in_the_whole_list() {
    // The filter draws the 3rd and the 9th of the list: their forms say 3 and 9, as
    // the server places by, not 1 and 2. An item with no place counts among those
    // drawn.
    let list = SortList::new("backlog", "Backlog")
        .total(40)
        .item(SortItem::new("7", "Seventh").position(3))
        .item(SortItem::new("31", "Thirty-first").position(9))
        .item(SortItem::new("2", "Second"))
        .item(SortItem::new("5", "Fifth").position(0));
    let html = html_of(
        Sortable::new("b", "Board")
            .post("/m?label=bug")
            .list(list.clone()),
    );
    let value = |id: &str| {
        let at = html.find(&format!(r#"id="b-item-{id}-position""#)).unwrap();
        html[at..]
            .split("value=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap()
            .to_owned()
    };
    assert_eq!(value("7"), "3");
    assert_eq!(value("31"), "9");
    assert_eq!(value("2"), "3", "the third drawn");
    assert_eq!(value("5"), "1", "a place is 1 at least");
    // The address the forms post to keeps the page's query.
    assert!(html.contains(r#"action="/m?label=bug""#), "{html}");
    // A page of the list does the same.
    let json = serde_json::to_string(
        &Sortable::new("b", "Board")
            .post("/m")
            .list(list)
            .page("backlog"),
    )
    .unwrap();
    assert!(json.contains(r#"id=\"b-item-31-position\" type=\"number\" name=\"position\" min=\"1\" inputmode=\"numeric\" value=\"9\""#), "{json}");
}

#[test]
fn the_next_page_of_a_list_is_two_ops() {
    let board = Sortable::new("b", "Board")
        .post("/m")
        .list(SortList::new("ready", "Ready").limit(5))
        .list(
            SortList::new("backlog", "Backlog")
                .from(20)
                .item(SortItem::new("21", "Twenty-first"))
                .more("/columns/backlog", [("after", "9.21")]),
        );
    let ops = board.clone().page("backlog");
    assert_eq!(ops.len(), 2);
    let json = serde_json::to_string(&ops).unwrap();
    assert!(
        json.contains(r#""target":"b-list-backlog-items""#),
        "{json}"
    );
    assert!(json.contains(r#""target":"b-list-backlog-more""#), "{json}");
    // Positions count on from where the page starts.
    assert!(json.contains(r#"value=\"21\""#), "{json}");
    assert!(json.contains(r#"id=\"b-item-21\""#), "{json}");
    // The last page takes the button away.
    let last = Sortable::new("b", "Board").post("/m").list(
        SortList::new("backlog", "Backlog")
            .from(40)
            .item(SortItem::new("41", "Last")),
    );
    let ops = last.page("backlog");
    assert!(
        matches!(&ops[1], Op::Replace { html, .. } if html.is_empty()),
        "{ops:?}"
    );
    assert!(board.page("nowhere").is_empty());
}

#[test]
fn a_move_after_an_item_lands_below_it() {
    let mv = Move::from_form(b"item=1&from_list=ready&to_list=ready&before=&after=3").unwrap();
    assert_eq!(mv, Move::new("1", "ready", "ready").after("3"));
    assert_eq!(mv.check(&lists()).unwrap().order, ["2", "3", "1"]);
    let landing = Move::new("3", "ready", "ready")
        .after("1")
        .check(&lists())
        .unwrap();
    assert_eq!(landing.order, ["1", "3", "2"]);
    // Before wins over after; an anchor that left is stale; after itself makes no sense.
    let landing = Move::new("3", "ready", "ready")
        .before("1")
        .after("2")
        .check(&lists())
        .unwrap();
    assert_eq!(landing.order, ["3", "1", "2"]);
    assert_eq!(
        Move::new("1", "ready", "ready").after("4").check(&lists()),
        Err(Refusal::Moved)
    );
    assert_eq!(
        Move::new("1", "ready", "ready").after("1").check(&lists()),
        Err(Refusal::Malformed)
    );
}

#[test]
fn ranks_leave_room_and_say_when_there_is_none() {
    assert_eq!(rank_between(None, None), Some(0));
    assert_eq!(rank_between(Some(0), None), Some(RANK_GAP));
    assert_eq!(rank_between(None, Some(0)), Some(-RANK_GAP));
    assert_eq!(rank_between(Some(0), Some(RANK_GAP)), Some(RANK_GAP / 2));
    assert_eq!(rank_between(Some(4), Some(5)), None);
    assert_eq!(rank_between(Some(5), Some(4)), None);
    assert_eq!(rank_between(Some(i64::MAX), None), None);
    assert_eq!(rank_between(Some(i64::MIN), Some(i64::MAX)), None);
    assert_eq!(rank_between(None, Some(i64::MIN)), None);
}

/// Lists in memory, kept by rank: what a database answers for [`Move::place`].
#[derive(Clone, Debug)]
struct Ranks(Vec<(ListState, Vec<(String, i64)>)>);

impl Ranks {
    fn of(lists: &[ListState]) -> Self {
        Self(
            lists
                .iter()
                .map(|list| {
                    let items = list
                        .items
                        .iter()
                        .enumerate()
                        .map(|(at, key)| (key.clone(), at as i64 * RANK_GAP))
                        .collect();
                    (list.clone(), items)
                })
                .collect(),
        )
    }

    fn sorted(&self, list: &str, item: &str) -> Vec<i64> {
        let mut ranks: Vec<i64> = self
            .0
            .iter()
            .filter(|(l, _)| l.key == list)
            .flat_map(|(_, items)| items.iter().filter(|(k, _)| k != item).map(|(_, r)| *r))
            .collect();
        ranks.sort_unstable();
        ranks
    }

    /// The orders once `placement` is written.
    fn apply(&self, placement: &Placement) -> Vec<(String, Vec<String>)> {
        let mut all: Vec<(String, String, i64)> = self
            .0
            .iter()
            .flat_map(|(l, items)| items.iter().map(|(k, r)| (l.key.clone(), k.clone(), *r)))
            .filter(|(_, k, _)| *k != placement.item)
            .collect();
        all.push((
            placement.to_list.clone(),
            placement.item.clone(),
            placement.rank.unwrap_or(0),
        ));
        self.0
            .iter()
            .map(|(l, _)| {
                let mut here: Vec<&(String, String, i64)> =
                    all.iter().filter(|(list, ..)| *list == l.key).collect();
                here.sort_by_key(|(_, _, rank)| *rank);
                (
                    l.key.clone(),
                    here.into_iter().map(|(_, k, _)| k.clone()).collect(),
                )
            })
            .collect()
    }
}

impl RankedLists for Ranks {
    type Error = std::convert::Infallible;

    /// Lists in memory are only this test's: nothing else moves them meanwhile.
    async fn lock(&self, _lists: &[&str]) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn spot(&self, key: &str) -> Result<Option<Spot>, Self::Error> {
        Ok(self.0.iter().find_map(|(l, items)| {
            items.iter().find(|(k, _)| k == key).map(|(_, r)| Spot {
                list: l.key.clone(),
                rank: *r,
            })
        }))
    }

    async fn list(&self, key: &str, item: &str) -> Result<Option<(ListState, u64)>, Self::Error> {
        Ok(self.0.iter().find(|(l, _)| l.key == key).map(|(l, items)| {
            (
                l.clone(),
                items.iter().filter(|(k, _)| k != item).count() as u64,
            )
        }))
    }

    async fn beside(
        &self,
        list: &str,
        anchor: &str,
        rank: i64,
        lower: bool,
        item: &str,
    ) -> Result<Option<i64>, Self::Error> {
        // In the list's order: by rank, a tie settled by where the item was put.
        let mut order: Vec<(i64, usize, &str)> = self
            .0
            .iter()
            .filter(|(l, _)| l.key == list)
            .flat_map(|(_, items)| items.iter().enumerate())
            .filter(|(_, (k, _))| k != item)
            .map(|(at, (k, r))| (*r, at, k.as_str()))
            .collect();
        order.sort_unstable();
        let Some(here) = order
            .iter()
            .position(|(r, _, k)| *k == anchor && *r == rank)
        else {
            return Ok(None);
        };
        Ok(if lower {
            here.checked_sub(1).map(|at| order[at].0)
        } else {
            order.get(here + 1).map(|(r, ..)| *r)
        })
    }

    async fn nth(&self, list: &str, index: u64, item: &str) -> Result<Option<i64>, Self::Error> {
        Ok(self.sorted(list, item).get(index as usize).copied())
    }

    async fn last(&self, list: &str, item: &str) -> Result<Option<i64>, Self::Error> {
        Ok(self.sorted(list, item).last().copied())
    }
}

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(future)
}

/// Lists in memory that write down what a move asks of them, in order.
struct Logged {
    ranks: Ranks,
    log: std::sync::Mutex<Vec<String>>,
}

impl Logged {
    fn new(ranks: Ranks) -> Self {
        Self {
            ranks,
            log: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn note(&self, line: String) {
        self.log.lock().unwrap().push(line);
    }

    fn asked(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }
}

impl RankedLists for Logged {
    type Error = std::convert::Infallible;

    async fn lock(&self, lists: &[&str]) -> Result<(), Self::Error> {
        self.note(format!("lock {}", lists.join(",")));
        self.ranks.lock(lists).await
    }

    async fn spot(&self, key: &str) -> Result<Option<Spot>, Self::Error> {
        self.note(format!("spot {key}"));
        self.ranks.spot(key).await
    }

    async fn list(&self, key: &str, item: &str) -> Result<Option<(ListState, u64)>, Self::Error> {
        self.note(format!("list {key}"));
        self.ranks.list(key, item).await
    }

    async fn beside(
        &self,
        list: &str,
        anchor: &str,
        rank: i64,
        lower: bool,
        item: &str,
    ) -> Result<Option<i64>, Self::Error> {
        self.note(format!("beside {list}"));
        self.ranks.beside(list, anchor, rank, lower, item).await
    }

    async fn nth(&self, list: &str, index: u64, item: &str) -> Result<Option<i64>, Self::Error> {
        self.note(format!("nth {list}"));
        self.ranks.nth(list, index, item).await
    }

    async fn last(&self, list: &str, item: &str) -> Result<Option<i64>, Self::Error> {
        self.note(format!("last {list}"));
        self.ranks.last(list, item).await
    }
}

#[test]
fn a_move_names_the_lists_it_touches_sorted_and_once() {
    assert_eq!(Move::new("3", "ready", "doing").lists(), ["doing", "ready"]);
    assert_eq!(Move::new("3", "doing", "ready").lists(), ["doing", "ready"]);
    assert_eq!(Move::new("3", "ready", "ready").lists(), ["ready"]);
}

#[test]
fn a_placed_move_locks_its_lists_in_order_before_it_reads_anything() {
    let asked = |mv: Move| {
        let store = Logged::new(Ranks::of(&lists()));
        let _ = block_on(mv.place(&store)).unwrap();
        store.asked()
    };
    // Between two lists: both, in key order, whichever way the move goes, and first.
    for mv in [
        Move::new("1", "ready", "doing").before("5"),
        Move::new("4", "doing", "ready").after("1"),
    ] {
        let log = asked(mv);
        assert_eq!(log[0], "lock doing,ready", "{log:?}");
        assert_eq!(
            log.iter().filter(|l| l.starts_with("lock")).count(),
            1,
            "{log:?}"
        );
    }
    // Within a list: that list, once.
    assert_eq!(
        asked(Move::new("3", "ready", "ready").at(1))[0],
        "lock ready"
    );
    // A move that is refused locks first too: what refuses it is read under the lock.
    for mv in [
        Move::new("1", "ready", "doing"),
        Move::new("4", "ready", "ready"),
        Move::new("99", "ready", "ready"),
        Move::new("1", "ready", "nowhere"),
    ] {
        let log = asked(mv);
        assert!(log[0].starts_with("lock "), "{log:?}");
    }
}

#[test]
fn a_move_between_tied_ranks_asks_for_fresh_ranks_first() {
    // [1 at 0, 2 at 100, 3 at 100, 4 at 200]: nothing fits between 2 and 3.
    let mut tied = Ranks::of(&[
        ListState::new("a", "A", ["1", "2", "3", "4"]),
        ListState::new("b", "B", ["5"]),
    ]);
    for (at, rank) in [0, 100, 100, 200].into_iter().enumerate() {
        tied.0[0].1[at].1 = rank;
    }
    for mv in [
        Move::new("5", "b", "a").before("3"),
        Move::new("5", "b", "a").after("2"),
    ] {
        let placed = block_on(mv.place(&tied)).unwrap().unwrap();
        assert_eq!(placed.rank, None, "{mv:?}");
    }
    // Next to a tie on its far side there is room.
    let placed = block_on(Move::new("5", "b", "a").before("2").place(&tied))
        .unwrap()
        .unwrap();
    assert_eq!(placed.rank, Some(50));
}

#[test]
fn a_placed_move_writes_one_rank_between_its_neighbours() {
    let ranks = Ranks::of(&lists());
    let placed = block_on(Move::new("3", "ready", "ready").before("2").place(&ranks))
        .unwrap()
        .unwrap();
    assert_eq!(placed.rank, Some(RANK_GAP / 2));
    assert!(placed.moved);
    let placed = block_on(Move::new("1", "ready", "ready").before("2").place(&ranks))
        .unwrap()
        .unwrap();
    assert!(!placed.moved, "dropped where it was");
    assert_eq!(placed.rank, Some(0));
    let refused = block_on(Move::new("1", "ready", "doing").place(&ranks)).unwrap();
    assert!(matches!(refused, Err(Refusal::Full { .. })));
    let refused = block_on(Move::new("4", "ready", "ready").place(&ranks)).unwrap();
    assert!(matches!(refused, Err(Refusal::Stale { list }) if list == "In progress"));
    // Neighbours with no room between them: the list wants fresh ranks first.
    let mut tight = ranks.clone();
    tight.0[0].1[1].1 = 1;
    let placed = block_on(Move::new("3", "ready", "ready").before("2").place(&tight))
        .unwrap()
        .unwrap();
    assert_eq!(placed.rank, None);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1_000))]

    /// Placed by rank, reading only the neighbours, a move ends where the check over
    /// whole lists puts it, and is refused for the same reasons.
    #[test]
    fn placing_by_rank_agrees_with_the_whole_list_check(
        sizes in proptest::collection::vec(0usize..7, 1..5),
        limits in proptest::collection::vec(proptest::option::of(0u32..7), 5),
        item in 0usize..30,
        from in 0usize..6,
        to in 0usize..6,
        before in proptest::option::of(0usize..30),
        after in proptest::option::of(0usize..30),
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
        mv.after = after.map(|a| a.to_string());
        mv.position = position;
        let ranks = Ranks::of(&lists);
        let checked = mv.check(&lists);
        let placed = block_on(mv.place(&ranks)).unwrap();
        match (checked, placed) {
            (Ok(landing), Ok(placement)) => {
                prop_assert!(placement.rank.is_some());
                prop_assert_eq!(landing.moved, placement.moved);
                let orders = ranks.apply(&placement);
                let got = &orders.iter().find(|(key, _)| *key == landing.to_list).unwrap().1;
                prop_assert_eq!(got, &landing.order);
            }
            (Err(a), Err(b)) => prop_assert_eq!(a, b),
            (a, b) => prop_assert!(false, "check {:?} but place {:?}", a, b),
        }
    }
}
