//! Live parts of a screen and the answers of the page they stand on: what a
//! stream says while the page is on its way, or while the screen is away, is
//! taken by the page that is on the screen afterwards, and never counted as
//! taken by one that is gone.

use navree_native::{Client, FormFactor, Platform};
use navree_native_core::{Config, Core, Effect, Event, Response, StreamState, Update};
use serde_json::{Value, json};

const NATIVE: &str = "application/vnd.navree.native+json";
const STREAM: &str = "/broadcasting/stream?topics=board";

fn answer(core: &mut Core, request: u64, body: &Value) -> Update {
    core.send(Event::Response(Response {
        request,
        status: 200,
        content_type: Some(NATIVE.to_owned()),
        body: body.to_string(),
        ..Response::default()
    }))
}

/// A page with a region and a list that a stream keeps up to date.
fn board(title: &str, since: &str) -> Value {
    json!({"navree": 1, "build": "b1", "vocab": 1, "screen": {"title": title, "body": {
        "t": "stack", "children": [
            {"t": "region", "id": "totals", "src": "/fragments/totals",
             "live": {"src": STREAM, "event": "board"},
             "children": [{"t": "text", "value": "₹1,00,000 today"}]},
            {"t": "list", "id": "cards", "since": since, "changes": "/board/changes",
             "live": {"src": STREAM, "event": "board"},
             "children": [{"t": "row", "id": "card-1", "title": "Pack 2 phulkari dupattas"}]},
            {"t": "link", "label": "Another board", "action": {"t": "navigate", "href": "/board/2", "present": "push"}}]}}})
}

fn totals() -> Value {
    json!({"navree": 1, "build": "b1", "vocab": 1, "screen": {"title": "", "body": {
        "t": "region", "id": "totals", "src": "/fragments/totals",
        "live": {"src": STREAM, "event": "board"},
        "children": [{"t": "text", "value": "₹1,20,000 today"}]}}})
}

fn changes(since: &str) -> Value {
    json!({"navree": 1, "build": "b1", "vocab": 1, "ops": [
        {"op": "merge", "target": "cards", "since": since, "gone": [],
         "nodes": [{"t": "row", "id": "card-2", "title": "Ship to Ludhiana"}]}]})
}

/// What an update asks for: each fetch's number and address.
fn fetches(update: &Update) -> Vec<(u64, String)> {
    update
        .effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Fetch(request) => Some((request.request, request.url.clone())),
            _ => None,
        })
        .collect()
}

/// Lets every timer an update set run out, and what that asks for in turn.
fn after_quiet(core: &mut Core, update: &Update) -> Vec<(u64, String)> {
    let mut asked = fetches(update);
    let timers: Vec<u64> = update
        .effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Timer { timer, ms } if *ms < 1_000 => Some(*timer),
            _ => None,
        })
        .collect();
    for timer in timers {
        let fired = core.send(Event::Timer { timer });
        asked.extend(after_quiet(core, &fired));
    }
    asked
}

fn spoke(core: &mut Core, stream: u64) -> Vec<(u64, String)> {
    let update = core.send(Event::Stream {
        stream,
        state: StreamState::Event,
        name: Some("board".to_owned()),
        status: None,
    });
    after_quiet(core, &update)
}

fn opened(update: &Update) -> Option<u64> {
    update.effects.iter().find_map(|effect| match effect {
        Effect::StreamOpen { stream, .. } => Some(*stream),
        _ => None,
    })
}

fn region_of(asked: &[(u64, String)]) -> Option<u64> {
    asked
        .iter()
        .find(|(_, url)| url.ends_with("/fragments/totals"))
        .map(|(request, _)| *request)
}

fn changes_of(asked: &[(u64, String)]) -> Option<(u64, String)> {
    asked
        .iter()
        .find(|(_, url)| url.contains("/board/changes"))
        .cloned()
}

/// A core showing the board, its stream open.
fn on_the_board() -> (Core, u64, u64) {
    let mut core = Core::new(Config::new(
        "https://shop.example",
        Client::new(Platform::Android, FormFactor::Phone),
    ));
    let shell = fetches(&core.send(Event::Start))[0].0;
    let page = fetches(&answer(
        &mut core,
        shell,
        &json!({"navree": 1, "build": "b1", "name": "Charkha Supply Co.", "home": "/"}),
    ))[0]
        .0;
    let shown = answer(&mut core, page, &board("Board", "0000501"));
    let stream = opened(&shown).expect("the board's stream opens");
    core.send(Event::Stream {
        stream,
        state: StreamState::Open,
        name: None,
        status: None,
    });
    let screen = core.view().front.expect("a screen in front");
    (core, screen, stream)
}

/// The stream speaks while the page is fetched again: what it said is taken
/// by the page that arrives, which the region and the list ask after, and
/// the region goes on hearing its stream afterwards.
#[test]
fn what_a_stream_says_while_the_page_is_on_its_way_is_not_lost() {
    let (mut core, screen, stream) = on_the_board();
    let page = fetches(&core.send(Event::Refresh { screen }))[0].0;
    let meanwhile = spoke(&mut core, stream);
    let landed = answer(&mut core, page, &board("Board", "0000502"));
    let after = after_quiet(&mut core, &landed);
    let mut asked = meanwhile.clone();
    asked.extend(after.clone());

    // Whatever was asked before the page landed is answered now, late.
    for (request, url) in &meanwhile {
        if url.ends_with("/fragments/totals") {
            answer(&mut core, *request, &totals());
        } else if url.contains("/board/changes") {
            answer(&mut core, *request, &changes("0000502"));
        }
    }
    let region = region_of(&after)
        .unwrap_or_else(|| panic!("the region is asked after the page landed: {asked:?}"));
    let (list, url) = changes_of(&after)
        .unwrap_or_else(|| panic!("the list is asked after the page landed: {asked:?}"));
    assert!(
        url.ends_with("since=0000502"),
        "asked of the page that is on the screen: {url}"
    );
    answer(&mut core, region, &totals());
    answer(&mut core, list, &changes("0000503"));

    // The region still hears its stream.
    let later = spoke(&mut core, stream);
    assert!(
        region_of(&later).is_some(),
        "the region asks again when its stream speaks: {later:?}"
    );
}

/// A screen that comes back to the front while its stream is open already
/// (the screen over it kept it open) catches up on what it missed meanwhile.
#[test]
fn a_screen_that_comes_back_under_an_open_stream_catches_up() {
    let (mut core, screen, stream) = on_the_board();
    let pushed = core.send(Event::Act {
        screen: Some(screen),
        action: serde_json::from_value(
            json!({"t": "navigate", "href": "/board/2", "present": "push"}),
        )
        .expect("an action"),
        form: None,
        overlay: None,
        text: None,
        now: None,
        ask: None,
        from: None,
    });
    let page = fetches(&pushed)[0].0;
    let shown = answer(&mut core, page, &board("Another board", "0000700"));
    // The board pushed holds the stream now; it stays open when the first
    // comes back, since both listen to it.
    let stream = opened(&shown).unwrap_or(stream);
    core.send(Event::Stream {
        stream,
        state: StreamState::Open,
        name: None,
        status: None,
    });
    // The stream speaks to the board in front.
    let heard = spoke(&mut core, stream);
    for (request, url) in &heard {
        if url.ends_with("/fragments/totals") {
            answer(&mut core, *request, &totals());
        } else if url.contains("/board/changes") {
            answer(&mut core, *request, &changes("0000701"));
        }
    }
    // Back to the first board: it was away while the stream spoke.
    let back = core.send(Event::Back);
    assert!(opened(&back).is_none(), "the stream stays open: {back:?}");
    let asked = after_quiet(&mut core, &back);
    assert!(
        region_of(&asked).is_some(),
        "the region it holds is asked again: {asked:?}"
    );
    let (_, url) =
        changes_of(&asked).unwrap_or_else(|| panic!("its list asks what changed: {asked:?}"));
    assert!(url.ends_with("since=0000501"), "{url}");
}
