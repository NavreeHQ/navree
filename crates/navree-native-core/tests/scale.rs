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
    let mut core = Core::new(Config::new("https://shop.example", Client::new(Platform::Android, FormFactor::Phone)));
    let shell = request_of(&core.send(Event::Start));
    let page = request_of(&answer(&mut core, shell, &json!({"navree": 1, "build": "b1", "name": "Charkha Supply Co.", "home": "/"})));
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
            assert_eq!(part["forms"].as_object().map(serde_json::Map::len), Some(25));
        }
    }
    let took = started.elapsed();

    let view = core.view();
    let content = serde_json::to_value(&view.screens[&screen].content).unwrap();
    let rows = content["screen"]["body"]["children"].as_array().unwrap();
    assert_eq!(rows.len(), 10_000);
    assert_eq!(rows[9_999]["id"], "order-9999");
    assert!(content["screen"]["body"].get("more").is_none(), "the list has ended");
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
    assert!(slowest.as_millis() < 500, "the slowest page took {slowest:?}");
}

/// What a change costs once ten thousand rows are loaded: a merge into a list
/// that is kept up to date, and a tick in a table whose rows can be chosen.
/// A merge tells the client to read the screen again; a tick tells it only
/// that the table's state changed.
#[test]
fn a_change_among_ten_thousand_rows() {
    let mut core = Core::new(Config::new("https://shop.example", Client::new(Platform::Android, FormFactor::Phone)));
    let shell = request_of(&core.send(Event::Start));
    let page = request_of(&answer(&mut core, shell, &json!({"navree": 1, "build": "b1", "name": "Charkha Supply Co.", "home": "/"})));
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
    assert_eq!(update.changed, [Change::Screen { id: screen }]);
    let step = Instant::now();
    let read = core.view().part(&Query::Screen { id: screen });
    let reading = step.elapsed();
    let list = &read["content"]["screen"]["body"]["children"][0];
    assert_eq!(list["children"].as_array().map(Vec::len), Some(10_000));
    assert_eq!(list["children"][0]["id"], "order-10000");
    assert_eq!(list["children"][5_000]["subtitle"], "Simran Bhatia, Chandigarh · ₹1,499");
    assert_eq!(list["since"], "42");

    // A tick says only that the table's state changed, and the part read holds no row.
    let step = Instant::now();
    let update = core.send(Event::Choose {
        screen,
        table: "ledger".to_owned(),
        row: Some("r-9000".to_owned()),
        on: true,
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
    let part = core.view().part(&Query::Rows {
        screen,
        id: "ledger".to_owned(),
        from: 10_000,
        count: 0,
    });
    let part_read = step.elapsed();
    assert_eq!(part["selected"], json!(["r-9000"]));
    assert_eq!(part["rows"].as_array().map(Vec::len), Some(0));

    println!("among 10,000 rows: a merge of 3 rows took {merged:?} and the screen read again {reading:?}; a tick took {ticked:?} and its part {part_read:?}");
}
