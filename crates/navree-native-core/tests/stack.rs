//! Stack: the core reads, keeps, changes and writes a tree as deep as the
//! protocol allows on a small stack, in a debug build. A client calls the core
//! on whatever thread it draws from, and a secondary thread on Apple's systems
//! has 512 KiB: this is run on exactly that much.
//!
//! A tree deeper than the protocol allows is refused before it is read, so it
//! costs no stack at all: the page is the web view's.
//!
//! `NAVREE_CORE_STACK_KIB` runs the same on another size, to find the margin.

use navree_native::{Client, FormFactor, MAX_DEPTH, Platform};
use navree_native_core::{Config, Core, Effect, Event, Query, Response, Update};

const NATIVE: &str = "application/vnd.navree.native+json";

/// One level of each shape of nesting the vocabulary has, around `inner`.
fn wrap(shape: usize, inner: &str) -> String {
    match shape % 8 {
        0 => format!(r#"{{"t":"stack","gap":2,"children":[{inner}]}}"#),
        1 => format!(r#"{{"t":"section","title":"Orders","style":"card","children":[{inner}]}}"#),
        2 => format!(
            r#"{{"t":"list","children":[{{"t":"row","title":"Tejinder Randhawa","subtitle":"Ludhiana","detail":[{inner}]}}]}}"#
        ),
        3 => format!(
            r#"{{"t":"table","columns":[{{"label":"Order"}}],"rows":[{{"id":"r","cells":[{inner}]}}]}}"#
        ),
        4 => format!(
            r#"{{"t":"form","method":"post","action":"/x","children":[{{"t":"field","name":"q","input":{{"t":"text"}}}},{inner}]}}"#
        ),
        5 => format!(r#"{{"t":"sheet","id":"s","open":true,"children":[{inner}]}}"#),
        6 => format!(
            r#"{{"t":"link","label":"Open","action":{{"t":"navigate","href":"/x"}},"children":[{inner}]}}"#
        ),
        _ => format!(r#"{{"t":"disclosure","label":"More","open":true,"children":[{inner}]}}"#),
    }
}

/// A tree of one shape (`Some`) or of every shape in turn, as deep as the
/// protocol allows when `fits`, and one level deeper than that when not. The
/// protocol has two limits, and which one a shape meets first is its own
/// affair: 64 nodes inside one another, or 127 levels of JSON.
fn tree(shape: Option<usize>, fits: bool) -> String {
    let allowed = |node: &str| {
        navree_native::nesting(&page(node)) <= navree_native::MAX_NESTING
            && nodes(node) <= MAX_DEPTH
    };
    let mut node = r#"{"t":"text","id":"leaf","value":"the bottom"}"#.to_owned();
    for at in 0.. {
        let next = wrap(shape.unwrap_or(at), &node);
        if !allowed(&next) {
            return if fits { node } else { next };
        }
        node = next;
    }
    node
}

/// How many nodes deep a tree written by `wrap` goes: every node says its kind once.
fn nodes(tree: &str) -> usize {
    let all = tree.matches(r#"{"t":"#).count();
    // A field beside the rest is no deeper; a control and an action are not nodes.
    all - tree.matches(r#"{"t":"field""#).count()
        - tree.matches(r#"{"t":"text"}"#).count()
        - tree.matches(r#"{"t":"navigate""#).count()
}

fn page(body: &str) -> String {
    format!(r#"{{"navree":1,"build":"b1","vocab":0,"screen":{{"title":"Deep","body":{body}}}}}"#)
}

fn answer(core: &mut Core, request: u64, body: String) -> Update {
    core.send(Event::Response(Response {
        request,
        status: 200,
        content_type: Some(NATIVE.to_owned()),
        body,
        ..Response::default()
    }))
}

fn request_of(update: &Update) -> u64 {
    update
        .effects
        .iter()
        .find_map(|effect| match effect {
            Effect::Fetch(request) => Some(request.request),
            _ => None,
        })
        .expect("a fetch was asked for")
}

/// Everything the core does with a deep tree, start to end.
fn through_the_core(body: String, native: bool) {
    let mut core = Core::new(Config::new(
        "https://shop.example",
        Client::new(Platform::Ios, FormFactor::Phone),
    ));
    let shell = request_of(&core.send(Event::Start));
    let first = request_of(&answer(
        &mut core,
        shell,
        r#"{"navree":1,"build":"b1","name":"Shop","home":"/"}"#.to_owned(),
    ));
    // Read, named, surveyed and kept.
    answer(&mut core, first, page(&body));
    let screen = core.view().front.expect("a screen is in front");
    let content = core.view().part(&Query::Screen { id: screen });
    assert_eq!(
        content["content"]["t"],
        if native { "native" } else { "web" },
        "{}",
        content["content"]["why"]
    );
    if !native {
        assert_eq!(content["content"]["why"][0], "client:depth");
        return;
    }
    // Changed at its deepest node, by an op and by a command.
    let post = core.send(Event::Act {
        screen: Some(screen),
        action: serde_json::from_str(r#"{"t":"request","method":"post","href":"/deep"}"#).unwrap(),
        form: None,
        overlay: None,
        text: None,
        now: None,
        ask: None,
        from: None,
    });
    let ops = r#"{"navree":1,"build":"b1","vocab":0,"ops":[{"op":"morph","target":"leaf","node":{"t":"text","id":"leaf","value":"changed"}},{"op":"replace","target":"leaf","node":{"t":"text","id":"leaf","value":"again"}}]}"#;
    answer(&mut core, request_of(&post), ops.to_owned());
    core.send(Event::Act {
        screen: Some(screen),
        action: serde_json::from_str(r#"{"t":"commands","list":[{"c":"text","id":"leaf","text":"by a command"},{"c":"hide","id":"leaf"}]}"#).unwrap(),
        form: None,
        overlay: None,
        text: None,
        now: None,
        ask: None,
        from: None,
    });
    // Written out whole, as a client reads it.
    let all = serde_json::to_string(&core.view().part(&Query::All)).unwrap();
    assert!(
        all.contains("by a command"),
        "the deepest node was not reached"
    );
    // And let go of.
    drop(core);
}

#[test]
fn a_tree_at_the_protocol_s_depth_limit_is_safe_on_a_small_stack() {
    let kib: usize = std::env::var("NAVREE_CORE_STACK_KIB")
        .ok()
        .and_then(|size| size.parse().ok())
        .unwrap_or(512);
    let work = move || {
        // Every shape of nesting alone, as deep as the protocol allows, then all of them mixed.
        for shape in 0..8 {
            through_the_core(tree(Some(shape), true), true);
        }
        through_the_core(tree(None, true), true);
        // One level too deep, and far too deep: refused, not read.
        for shape in 0..8 {
            through_the_core(tree(Some(shape), false), false);
        }
        let mut far = tree(Some(0), false);
        for _ in 0..440 {
            far = wrap(0, &far);
        }
        through_the_core(far, false);
    };
    std::thread::Builder::new()
        .stack_size(kib * 1024)
        .spawn(work)
        .expect("a thread")
        .join()
        .expect("the core's work on a deep tree");
    println!("trees as deep as the protocol allows went through the core on {kib} KiB of stack");
}
