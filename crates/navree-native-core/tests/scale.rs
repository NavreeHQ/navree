//! Size: a list read by cursor to ten thousand rows. The core's work for each
//! page of rows stays the size of the page, not of the list, and a client is
//! told which rows were added so it need not read the screen again.

use std::time::Instant;

use navree_native::{Client, FormFactor, Platform};
use navree_native_core::{Change, Config, Core, Effect, Event, Query, Response, StreamState};
use serde_json::{Value, json};

const NATIVE: &str = "application/vnd.navree.native+json";

fn answer(core: &mut Core, request: u64, body: &Value) -> navree_native_core::Update {
    core.send(Event::Response(Response {
        request,
        status: 200,
        content_type: Some(NATIVE.to_owned()),
        body: body.to_string(),
        ..Response::default()
    }))
}

/// A row as the store's orders are: a name, a line under it, and a small form
/// that goes by its own address.
fn row(number: usize) -> Value {
    json!({
        "t": "row", "id": format!("order-{number}"), "title": format!("CS-{}", 220_480 - number),
        "subtitle": "Tejinder Randhawa, Ludhiana · ₹507",
        "detail": [{"t": "form", "method": "post", "action": format!("/orders/{number}/note"), "children": [
            {"t": "field", "name": "note", "label": "Note", "input": {"t": "text"}},
            {"t": "button", "label": "Save", "action": {"t": "submit"}},
        ]}],
    })
}

fn request_of(update: &navree_native_core::Update) -> u64 {
    update
        .effects
        .iter()
        .find_map(|effect| match effect {
            Effect::Fetch(request) => Some(request.request),
            _ => None,
        })
        .expect("a fetch was asked for")
}

#[test]
fn ten_thousand_rows_by_cursor() {
    let mut core = Core::new(Config::new(
        "https://shop.example",
        Client::new(Platform::Android, FormFactor::Phone),
    ));
    let shell = request_of(&core.send(Event::Start));
    let page = request_of(&answer(
        &mut core,
        shell,
        &json!({"navree": 1, "build": "b1", "name": "Charkha Supply Co.", "home": "/"}),
    ));
    let first: Vec<Value> = (0..25).map(row).collect();
    answer(
        &mut core,
        page,
        &json!({"navree": 1, "build": "b1", "vocab": 1, "screen": {"title": "Orders", "body": {
            "t": "list", "id": "orders", "more": "/orders?cursor=25", "children": first}}}),
    );
    let screen = core.view().front.expect("a screen is in front");

    let started = Instant::now();
    let mut slowest = std::time::Duration::ZERO;
    for at in (25..10_000).step_by(25) {
        let asked = core.send(Event::More {
            screen,
            id: "orders".to_owned(),
            before: false,
        });
        let request = request_of(&asked);
        // Each page also brings the last row of the page before: a row is never in a list twice.
        let nodes: Vec<Value> = (at - 1..at + 25).map(row).collect();
        let more = (at + 25 < 10_000).then(|| format!("/orders?cursor={}", at + 25));
        let page = json!({"navree": 1, "build": "b1", "vocab": 1, "ops": [
            {"op": "append", "target": "orders", "nodes": nodes, "more": more}]});
        let step = Instant::now();
        let update = answer(&mut core, request, &page);
        slowest = slowest.max(step.elapsed());
        // The client is told which rows came, and nothing asks it to read the screen again.
        assert_eq!(
            update.changed,
            [Change::Rows {
                screen,
                id: "orders".to_owned(),
                from: at,
                count: 25
            }]
        );
        if at == 5_000 {
            // Reading the rows alone gives the rows and the state of their forms, not the list.
            let part = core.view().part(&Query::Rows {
                screen,
                id: "orders".to_owned(),
                from: at,
                count: 25,
            });
            assert_eq!(part["nodes"].as_array().map(Vec::len), Some(25));
            assert_eq!(part["nodes"][0]["id"], "order-5000");
            assert_eq!(
                part["forms"].as_object().map(serde_json::Map::len),
                Some(25)
            );
        }
    }
    let took = started.elapsed();

    let view = core.view();
    let content = serde_json::to_value(&view.screens[&screen].content).unwrap();
    let rows = content["screen"]["body"]["children"].as_array().unwrap();
    assert_eq!(rows.len(), 10_000);
    assert_eq!(rows[9_999]["id"], "order-9999");
    assert!(
        content["screen"]["body"].get("more").is_none(),
        "the list has ended"
    );
    assert_eq!(view.screens[&screen].forms.len(), 10_000);
    // The end of the list asks for nothing.
    let done = core.send(Event::More {
        screen,
        id: "orders".to_owned(),
        before: false,
    });
    assert!(done.effects.is_empty());

    println!("10,000 rows in 399 pages: {took:?} in all, the slowest page {slowest:?}");
    // A page of rows costs the page, not the list: far under a frame in a release
    // build. The bound here is loose, for a debug build on a busy machine.
    assert!(
        slowest.as_millis() < 500,
        "the slowest page took {slowest:?}"
    );
}

/// What a change costs once ten thousand rows are loaded: a merge into a list
/// that is kept up to date, and a tick in a table whose rows can be chosen.
/// A merge names the rows it changed, and a tick says only that the table's
/// state changed: neither asks for the screen to be read again.
#[test]
fn a_change_among_ten_thousand_rows() {
    let mut core = Core::new(Config::new(
        "https://shop.example",
        Client::new(Platform::Android, FormFactor::Phone),
    ));
    let shell = request_of(&core.send(Event::Start));
    let page = request_of(&answer(
        &mut core,
        shell,
        &json!({"navree": 1, "build": "b1", "name": "Charkha Supply Co.", "home": "/"}),
    ));
    let rows: Vec<Value> = (0..10_000).map(row).collect();
    let records: Vec<Value> = (0..10_000)
        .map(|number| json!({"id": format!("r-{number}"), "cells": [{"t": "text", "value": format!("CS-{number}")}, {"t": "text", "value": "Gurpreet Kaur, Ludhiana"}, {"t": "text", "value": "₹1,24,050"}]}))
        .collect();
    let arrived = answer(
        &mut core,
        page,
        &json!({"navree": 1, "build": "b1", "vocab": 0, "screen": {"title": "Orders", "body": {"t": "stack", "children": [
            {"t": "list", "id": "orders", "since": "41", "changes": "/orders/changes", "live": {"src": "/stream?topics=orders"}, "children": rows},
            {"t": "table", "id": "ledger", "select": {"name": "ids[]"}, "columns": [{"label": "Order"}, {"label": "Customer"}, {"label": "Total"}], "rows": records},
        ]}}}),
    );
    let screen = core.view().front.expect("a screen is in front");
    let stream = arrived
        .effects
        .iter()
        .find_map(|effect| match effect {
            Effect::StreamOpen { stream, .. } => Some(*stream),
            _ => None,
        })
        .expect("the list's stream is opened");
    let spoke = |state| Event::Stream {
        stream,
        state,
        name: None,
        status: None,
    };
    core.send(spoke(StreamState::Open));
    let nudged = core.send(spoke(StreamState::Event));
    let timer = nudged
        .effects
        .iter()
        .find_map(|effect| match effect {
            Effect::Timer { timer, .. } => Some(*timer),
            _ => None,
        })
        .expect("the ask waits for the stream to go quiet");
    let asked = request_of(&core.send(Event::Timer { timer }));

    // One row changed in the middle, one is new, one left.
    let mut changed = row(5_000);
    changed["subtitle"] = json!("Simran Bhatia, Chandigarh · ₹1,499");
    let merge = json!({"navree": 1, "build": "b1", "vocab": 0, "ops": [
        {"op": "merge", "target": "orders", "since": "42", "at": "start", "gone": ["order-7"], "nodes": [changed, row(10_000)]}]});
    let step = Instant::now();
    let update = answer(&mut core, asked, &merge);
    let merged = step.elapsed();
    // The update names the three rows, and nothing asks for the screen to be read again.
    let named = Change::Redraw {
        screen,
        id: "orders".to_owned(),
        rows: vec!["order-10000".to_owned(), "order-5000".to_owned()],
        gone: vec!["order-7".to_owned()],
    };
    assert_eq!(update.changed, [named]);
    let step = Instant::now();
    let part = core.part(&Query::Redraw {
        screen,
        id: "orders".to_owned(),
        rows: vec!["order-10000".to_owned(), "order-5000".to_owned()],
        gone: vec!["order-7".to_owned()],
    });
    let reading = step.elapsed();
    assert_eq!(part["rows"].as_array().map(Vec::len), Some(2));
    assert_eq!(
        (&part["rows"][0]["id"], &part["rows"][0]["before"]),
        (&json!("order-10000"), &json!("order-0"))
    );
    assert_eq!(
        part["rows"][1]["node"]["subtitle"],
        "Simran Bhatia, Chandigarh · ₹1,499"
    );
    assert_eq!(part["rows"][1]["before"], "order-5001");
    assert_eq!(part["gone"], json!(["order-7"]));
    assert_eq!(part["since"], "42");
    assert_eq!(part["forms"].as_object().map(serde_json::Map::len), Some(2));
    // For comparison: what reading the whole screen again would have cost.
    let step = Instant::now();
    let whole = core.part(&Query::Screen { id: screen });
    let whole_read = step.elapsed();
    assert_eq!(
        whole["content"]["screen"]["body"]["children"][0]["children"]
            .as_array()
            .map(Vec::len),
        Some(10_000)
    );

    // A tick says only that the table's state changed, and the part read holds no row.
    let step = Instant::now();
    let update = core.send(Event::Choose {
        screen,
        table: "ledger".to_owned(),
        row: Some("r-9000".to_owned()),
        on: true,
        across: false,
    });
    let ticked = step.elapsed();
    let state = Change::Rows {
        screen,
        id: "ledger".to_owned(),
        from: 10_000,
        count: 0,
    };
    assert_eq!(update.changed, [state]);
    let step = Instant::now();
    let part = core.part(&Query::Rows {
        screen,
        id: "ledger".to_owned(),
        from: 10_000,
        count: 0,
    });
    let part_read = step.elapsed();
    assert_eq!(part["selected"], json!(["r-9000"]));
    assert_eq!(part["rows"].as_array().map(Vec::len), Some(0));

    println!(
        "among 10,000 rows: a merge of 3 rows took {merged:?} and its rows read again {reading:?} (the whole screen: {whole_read:?}); a tick took {ticked:?} and its part {part_read:?}"
    );
}

/// A screen that is a field and a list of `rows` plain rows.
fn screen_of(rows: usize) -> (Core, u64) {
    let mut core = Core::new(Config::new(
        "https://shop.example",
        Client::new(Platform::Android, FormFactor::Phone),
    ));
    let shell = request_of(&core.send(Event::Start));
    let page = request_of(&answer(
        &mut core,
        shell,
        &json!({"navree": 1, "build": "b1", "name": "Charkha Supply Co.", "home": "/"}),
    ));
    let list: Vec<Value> = (0..rows)
        .map(|number| json!({"t": "row", "id": format!("order-{number}"), "title": format!("CS-{number}")}))
        .collect();
    answer(
        &mut core,
        page,
        &json!({"navree": 1, "build": "b1", "vocab": 1, "screen": {"title": "Orders", "body": {
            "t": "stack", "children": [
                {"t": "field", "id": "note", "name": "note", "input": {"t": "text"}},
                {"t": "list", "id": "orders", "children": list}]}}}),
    );
    let screen = core.view().front.expect("a screen is in front");
    (core, screen)
}

/// Sends `ops` as the answer to the screen asked for again.
fn ops_answer(core: &mut Core, screen: u64, ops: Value) -> navree_native_core::Update {
    let asked = request_of(&core.send(Event::Refresh { screen }));
    answer(
        core,
        asked,
        &json!({"navree": 1, "build": "b1", "vocab": 1, "ops": ops}),
    )
}

/// What an answer may ask of a client is bounded: its ops that look through
/// the screen, times the nodes the screen holds, may not pass four million.
#[test]
fn an_answer_whose_ops_would_look_through_a_big_screen_too_often_is_refused() {
    let focus = |count: usize| Value::Array(vec![json!({"op": "focus", "target": "note"}); count]);
    // A thousand ops on a screen of fifty rows: each finds its field.
    let (mut small, screen) = screen_of(50);
    ops_answer(&mut small, screen, focus(1_000));
    assert_eq!(
        small.view().screens[&screen]
            .focus
            .as_ref()
            .map(|focus| focus.serial),
        Some(1_000)
    );
    // The same answer to a screen of thirty thousand rows is thirty million
    // visits: none of its ops is applied, and it is said.
    let (mut big, screen) = screen_of(30_000);
    let update = ops_answer(&mut big, screen, focus(1_000));
    assert_eq!(big.view().screens[&screen].focus, None);
    assert!(
        update
            .effects
            .iter()
            .any(|effect| matches!(effect, Effect::Report { what, .. } if what == "size")),
        "{update:?}"
    );
    // A few ops on that screen are applied.
    ops_answer(&mut big, screen, focus(13));
    assert!(big.view().screens[&screen].focus.is_some());
}

/// A merge costs the rows it brings, not the rows it brings times the rows
/// the list holds.
#[test]
fn a_merge_costs_the_rows_it_brings_and_not_the_list_times_them() {
    let (mut core, screen) = screen_of(40_000);
    let rows: Vec<Value> = (0..40_000)
        .map(|number| json!({"t": "row", "id": format!("newer-{number}"), "title": format!("CS-{number}")}))
        .collect();
    let gone: Vec<String> = (0..20_000)
        .map(|number| format!("order-{number}"))
        .collect();
    let started = Instant::now();
    ops_answer(
        &mut core,
        screen,
        json!([{"op": "merge", "target": "orders", "since": "1", "at": "end", "gone": gone, "nodes": rows}]),
    );
    let took = started.elapsed();
    let content = serde_json::to_value(&core.view().screens[&screen].content).unwrap();
    let list = &content["screen"]["body"]["children"][1]["children"];
    assert_eq!(list.as_array().map(Vec::len), Some(60_000));
    println!("a merge of 40,000 rows and 20,000 gone, into 40,000 rows: {took:?}");
    assert!(took.as_secs() < 3, "the merge took {took:?}");
}

/// A screen has a largest size, four times what one answer may hold, however
/// it comes to hold it. A list that is kept up to date and a server that
/// answers every ask with a hundred thousand rows more and says more follow
/// is asked for at most that many: the answer that would pass the largest
/// size is not applied, and the list stops asking.
#[test]
fn a_screen_has_a_largest_size_and_a_live_list_stops_asking_there() {
    use navree_native::MAX_NODES;
    let mut core = Core::new(Config::new(
        "https://shop.example",
        Client::new(Platform::Android, FormFactor::Phone),
    ));
    let shell = request_of(&core.send(Event::Start));
    let page = request_of(&answer(
        &mut core,
        shell,
        &json!({"navree": 1, "build": "b1", "name": "Charkha Supply Co.", "home": "/"}),
    ));
    let arrived = answer(
        &mut core,
        page,
        &json!({"navree": 1, "build": "b1", "vocab": 0, "screen": {"title": "Orders", "body": {
            "t": "list", "id": "orders", "since": "0", "changes": "/orders/changes",
            "live": {"src": "/stream?topics=orders"}, "children": []}}}),
    );
    let screen = core.view().front.expect("a screen is in front");
    let stream = arrived
        .effects
        .iter()
        .find_map(|effect| match effect {
            Effect::StreamOpen { stream, .. } => Some(*stream),
            _ => None,
        })
        .expect("the list's stream is opened");
    let spoke = |state| Event::Stream {
        stream,
        state,
        name: None,
        status: None,
    };
    core.send(spoke(StreamState::Open));
    let nudged = core.send(spoke(StreamState::Event));
    let timer = nudged
        .effects
        .iter()
        .find_map(|effect| match effect {
            Effect::Timer { timer, .. } => Some(*timer),
            _ => None,
        })
        .expect("the ask waits for the stream to go quiet");
    let mut asked = Some(request_of(&core.send(Event::Timer { timer })));

    let per_answer = MAX_NODES - 10;
    let mut answers = 0;
    while let Some(request) = asked.take() {
        answers += 1;
        assert!(
            answers <= 12,
            "the core is still asking after {answers} answers"
        );
        let nodes: Vec<Value> = (0..per_answer)
            .map(|number| json!({"t": "row", "id": format!("r{answers}-{number}"), "title": "CS"}))
            .collect();
        let update = answer(
            &mut core,
            request,
            &json!({"navree": 1, "build": "b1", "vocab": 0, "ops": [
                {"op": "merge", "target": "orders", "since": answers.to_string(), "at": "end", "more": true, "nodes": nodes}]}),
        );
        // What the list asks for is what changed in it.
        asked = update.effects.iter().find_map(|effect| match effect {
            Effect::Fetch(request) if request.url.contains("/orders/changes") => {
                Some(request.request)
            }
            _ => None,
        });
        if answers == 5 {
            // The fifth would take the screen to nearly five times what one
            // answer may hold: it is not applied, it is said, and no more
            // changes are asked for (the screen itself is fetched again, as
            // an answer that failed leaves it out of date).
            assert_eq!(asked, None, "{:?}", update.effects);
            assert!(
                update
                    .effects
                    .iter()
                    .any(|effect| matches!(effect, Effect::Report { what, .. } if what == "size")),
                "{:?}",
                update.effects
            );
        }
    }
    assert_eq!(answers, 5);
    let content = serde_json::to_value(&core.view().screens[&screen].content).unwrap();
    let rows = content["screen"]["body"]["children"]
        .as_array()
        .map(Vec::len);
    assert_eq!(rows, Some(4 * per_answer));
}

/// Naming what an answer repeats is linear: fields of one name in one form,
/// and forms of one id, each get a name of their own in one pass.
#[test]
fn naming_what_an_answer_repeats_costs_what_it_repeats() {
    let page = |body: &Value| {
        let mut core = Core::new(Config::new(
            "https://shop.example",
            Client::new(Platform::Android, FormFactor::Phone),
        ));
        let shell = request_of(&core.send(Event::Start));
        let page = request_of(&answer(
            &mut core,
            shell,
            &json!({"navree": 1, "build": "b1", "name": "Charkha Supply Co.", "home": "/"}),
        ));
        let started = Instant::now();
        answer(
            &mut core,
            page,
            &json!({"navree": 1, "build": "b1", "vocab": 1, "screen": {"title": "Orders", "body": body}}),
        );
        let took = started.elapsed();
        (core, took)
    };
    // Ninety-nine thousand fields called `note` in one form.
    let fields: Vec<Value> = (0..99_000)
        .map(|_| json!({"t": "field", "name": "note", "input": {"t": "text"}}))
        .collect();
    let (core, took) = page(
        &json!({"t": "form", "id": "notes", "method": "post", "action": "/notes", "children": fields}),
    );
    let screen = core.view().front.expect("a screen is in front");
    assert_eq!(
        core.view().screens[&screen].forms["notes"].fields.len(),
        99_000
    );
    println!("99,000 fields of one name: {took:?}");
    assert!(took.as_secs() < 20, "naming the fields took {took:?}");
    // Thirty thousand forms of one id.
    let forms: Vec<Value> = (0..30_000)
        .map(|_| json!({"t": "form", "id": "note", "method": "post", "action": "/note", "children": []}))
        .collect();
    let (core, took) = page(&json!({"t": "stack", "children": forms}));
    let screen = core.view().front.expect("a screen is in front");
    assert_eq!(core.view().screens[&screen].forms.len(), 30_000);
    println!("30,000 forms of one id: {took:?}");
    assert!(took.as_secs() < 20, "naming the forms took {took:?}");
}

/// A screen of the largest size: ninety-nine thousand nine hundred and ninety
/// rows, and three more answers of as many. The list may say where more rows
/// are.
fn full_screen(more: Option<&str>) -> (Core, u64) {
    let (mut core, screen) = screen_of(99_990);
    if let Some(more) = more {
        // The list names its next page.
        let asked = request_of(&core.send(Event::Refresh { screen }));
        answer(
            &mut core,
            asked,
            &json!({"navree": 1, "build": "b1", "vocab": 1, "screen": {"title": "Orders", "body": {
                "t": "stack", "children": [
                    {"t": "field", "id": "note", "name": "note", "input": {"t": "text"}},
                    {"t": "list", "id": "orders", "more": more, "children": (0..99_990)
                        .map(|number| json!({"t": "row", "id": format!("order-{number}"), "title": "CS"}))
                        .collect::<Vec<Value>>()}]}}}),
        );
    }
    for round in 0..3 {
        let rows: Vec<Value> = (0..99_990)
            .map(
                |number| json!({"t": "row", "id": format!("more-{round}-{number}"), "title": "CS"}),
            )
            .collect();
        ops_answer(
            &mut core,
            screen,
            json!([{"op": "append", "target": "orders", "nodes": rows, "more": more}]),
        );
    }
    (core, screen)
}

fn list_rows(core: &Core, screen: u64) -> Vec<Value> {
    let content = serde_json::to_value(&core.view().screens[&screen].content).unwrap();
    content["screen"]["body"]["children"][1]["children"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

/// An answer's changes are applied whole or not at all: one that would take
/// the screen past its largest size leaves it as it was, marks it out of date
/// (it is fetched again), and says so. What it said that changes nothing on
/// the screen (its toast) is still said.
#[test]
fn an_answer_that_would_pass_the_largest_screen_changes_nothing() {
    let (mut core, screen) = full_screen(None);
    let before = list_rows(&core, screen).len();
    assert_eq!(before, 4 * 99_990);
    let rows: Vec<Value> = (0..50)
        .map(|number| json!({"t": "row", "id": format!("late-{number}"), "title": "CS"}))
        .collect();
    let update = ops_answer(
        &mut core,
        screen,
        json!([
            {"op": "remove", "target": "order-5"},
            {"op": "append", "target": "orders", "nodes": rows},
            {"op": "toast", "text": "Saved"}
        ]),
    );
    let after = list_rows(&core, screen);
    assert_eq!(
        after.len(),
        before,
        "the remove was applied without the append"
    );
    assert!(after.iter().any(|row| row["id"] == "order-5"));
    assert!(
        update
            .effects
            .iter()
            .any(|effect| matches!(effect, Effect::Report { what, .. } if what == "size")),
        "{:?}",
        update.effects
    );
    // The screen is out of date: it is asked for again.
    assert!(
        update.effects.iter().any(
            |effect| matches!(effect, Effect::Fetch(request) if request.url == "https://shop.example/")
        ),
        "{:?}",
        update.effects
    );
    // The toast costs nothing on the screen, so it is shown; what is said of
    // the screen is that it is loading, not that the write failed.
    assert!(
        update.effects.iter().any(
            |effect| matches!(effect, Effect::Message(message) if message.text.as_deref() == Some("Saved"))
        ),
        "{:?}",
        update.effects
    );
    assert!(
        !update.effects.iter().any(
            |effect| matches!(effect, Effect::Message(message) if message.notice == Some(navree_native_core::Notice::Failed))
        ),
        "{:?}",
        update.effects
    );
}

/// A list that asked for more rows and was refused is not at its end: it
/// shows its failed state, with its retry, and keeps where the rows are.
#[test]
fn a_page_the_screen_has_no_room_for_leaves_the_list_failed_and_not_finished() {
    let (mut core, screen) = full_screen(Some("/orders?cursor=1"));
    let asked = request_of(&core.send(Event::More {
        screen,
        id: "orders".to_owned(),
        before: false,
    }));
    let rows: Vec<Value> = (0..99_999)
        .map(|number| json!({"t": "row", "id": format!("next-{number}"), "title": "CS"}))
        .collect();
    answer(
        &mut core,
        asked,
        &json!({"navree": 1, "build": "b1", "vocab": 1, "ops": [
            {"op": "append", "target": "orders", "nodes": rows, "more": "/orders?cursor=2"}]}),
    );
    let view = core.view();
    assert!(
        view.screens[&screen].lists["orders"].failed,
        "the list shows its failure"
    );
    let content = serde_json::to_value(&view.screens[&screen].content).unwrap();
    assert_eq!(
        content["screen"]["body"]["children"][1]["more"], "/orders?cursor=1",
        "the list keeps where its rows are"
    );
    assert_eq!(list_rows(&core, screen).len(), 4 * 99_990);
}

/// What one visit to a node costs an op that looks through the screen, for
/// the ops that cost most per node. `MAX_WORK` is set from this. Run it with
/// `cargo test --release -p navree-native-core --test scale -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, not a check"]
fn what_a_visit_to_a_node_costs() {
    const ROWS: usize = 20_000;
    const OPS: usize = 150;
    type Op = fn(usize) -> Value;
    let kinds: [(&str, Op); 6] = [
        (
            "focus naming nothing",
            |_| json!({"op": "focus", "target": "nowhere"}),
        ),
        (
            "remove a row",
            |at| json!({"op": "remove", "target": format!("order-{at}")}),
        ),
        (
            "append a row",
            |at| json!({"op": "append", "target": "orders", "nodes": [{"t": "row", "id": format!("new-{at}"), "title": "CS"}]}),
        ),
        (
            "morph a row",
            |at| json!({"op": "morph", "target": format!("order-{at}"), "node": {"t": "row", "id": format!("order-{at}"), "title": "CS 2"}}),
        ),
        (
            "merge a row",
            |at| json!({"op": "merge", "target": "orders", "since": format!("{at}"), "nodes": [{"t": "row", "id": format!("order-{at}"), "title": "CS 3"}]}),
        ),
        (
            "open nothing",
            |_| json!({"op": "open", "target": "nowhere"}),
        ),
    ];
    for (name, op) in kinds {
        let (mut core, screen) = screen_of(ROWS);
        let ops = Value::Array((0..OPS).map(|at| op(at * 3 + 7)).collect());
        let started = Instant::now();
        ops_answer(&mut core, screen, ops);
        let took = started.elapsed();
        let visits = (ROWS * OPS) as f64;
        println!(
            "{name}: {OPS} ops on {ROWS} rows took {took:?}: {:.1} ns for each node an op may visit",
            took.as_nanos() as f64 / visits
        );
    }
}

/// The work of an answer is counted as it is done: what an op brings is
/// looked through by every op after it, so an answer that starts within the
/// budget and brings a great deal early is refused at the start, and nothing
/// of it is applied.
#[test]
fn what_an_early_op_brings_is_looked_through_by_every_op_after_it() {
    // A thousand ops on this screen are within the budget as it stands.
    let (mut core, screen) = screen_of(19_000);
    let rows: Vec<Value> = (0..99_000)
        .map(|number| json!({"t": "row", "id": format!("big-{number}"), "title": "CS"}))
        .collect();
    let mut ops = vec![json!({"op": "append", "target": "orders", "nodes": rows})];
    ops.extend((0..999).map(|_| json!({"op": "focus", "target": "nowhere"})));
    let started = Instant::now();
    let update = ops_answer(&mut core, screen, Value::Array(ops));
    let took = started.elapsed();
    println!("the refused answer took {took:?}");
    assert_eq!(
        list_rows(&core, screen).len(),
        19_000,
        "nothing of it was applied"
    );
    assert!(
        update
            .effects
            .iter()
            .any(|effect| matches!(effect, Effect::Report { what, .. } if what == "size")),
        "{:?}",
        update.effects
    );
}

/// A page that carries ops is one answer: when its ops would be refused (their
/// work passes the budget against the page's own screen), the page is not
/// drawn either, the screen shows what a screen that could not be read shows
/// and has its retry, and nothing is fetched again by itself.
#[test]
fn a_page_whose_ops_would_be_refused_is_not_drawn_and_is_not_fetched_again() {
    let mut core = Core::new(Config::new(
        "https://shop.example",
        Client::new(Platform::Android, FormFactor::Phone),
    ));
    let shell = request_of(&core.send(Event::Start));
    let first = request_of(&answer(
        &mut core,
        shell,
        &json!({"navree": 1, "build": "b1", "name": "Charkha Supply Co.", "home": "/"}),
    ));
    // Five thousand rows, and a thousand ops that each look through them:
    // five million visits, over the four million an answer may ask for.
    let rows: Vec<Value> = (0..5_000)
        .map(|number| json!({"t": "row", "id": format!("order-{number}"), "title": "CS"}))
        .collect();
    let page = json!({"navree": 1, "build": "b1", "vocab": 1,
        "screen": {"title": "Orders", "body": {"t": "stack", "children": [
            {"t": "field", "id": "note", "name": "note", "input": {"t": "text"}},
            {"t": "list", "id": "orders", "children": rows}]}},
        "ops": vec![json!({"op": "focus", "target": "note"}); 1_000]});
    let (mut fetches, mut next) = (0, Some(first));
    for _ in 0..10 {
        let Some(request) = next.take() else { break };
        fetches += 1;
        let update = answer(&mut core, request, &page);
        next = update.effects.iter().find_map(|effect| match effect {
            Effect::Fetch(request) if request.url == "https://shop.example/" => {
                Some(request.request)
            }
            _ => None,
        });
    }
    assert!(fetches <= 2, "the page was asked for {fetches} times");
    let screen = core.view().front.expect("a screen is in front");
    let view = &core.view().screens[&screen];
    assert!(
        matches!(view.content, navree_native_core::Content::Blank),
        "the page was drawn though its ops were refused"
    );
    assert_eq!(view.failure, Some(navree_native_core::Failure::Protocol));
    // The person's retry asks again.
    let retried = core.send(Event::Refresh { screen });
    assert!(request_of(&retried) > 0);
}
