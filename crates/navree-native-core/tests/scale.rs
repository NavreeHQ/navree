//! Size: a list read by cursor to ten thousand rows. The core's work for each
//! page of rows stays the size of the page, not of the list, and a client is
//! told which rows were added so it need not read the screen again.

use std::time::Instant;

use navree_native::{Client, FormFactor, Platform};
use navree_native_core::{Change, Config, Core, Effect, Event, Query, Response};
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
    });
    assert!(done.effects.is_empty());

    println!("10,000 rows in 399 pages: {took:?} in all, the slowest page {slowest:?}");
    // A page of rows costs the page, not the list: far under a frame in a release
    // build. The bound here is loose, for a debug build on a busy machine.
    assert!(slowest.as_millis() < 500, "the slowest page took {slowest:?}");
}
