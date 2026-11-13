//! Reading what the server answered.
//!
//! An app in a store is older than its server sooner or later, so an answer may
//! hold a kind this build has never heard of. That is never a crash and never a
//! hole in a screen: a node of an unknown kind is drawn from its `alt` when it
//! has one, and otherwise the whole page is the web view's. An op this build
//! does not know is passed over and the screen is fetched again. A tree nested
//! deeper than the protocol allows is not read at all.

use navree_native::action::Command;
use navree_native::device::{Alt, Call, Feature};
use navree_native::node::{Button, Node, Stack, TableRow};
use navree_native::{Action, CONTENT_TYPE, Envelope, Input, MAJOR, MAX_NESTING, Op, Screen};
use serde_json::Value;

use crate::config::Config;
use crate::event::{Response, TransportError};
use crate::origin::{Origin, Target};
use crate::state::Failure;
use crate::tree::{Slot, slots};

/// The response header the server names the shell's stamp in, while it does
/// not write it into the envelope.
const SHELL_STAMP_HEADER: &str = "x-navree-shell";

/// An envelope, read as far as this build can.
#[derive(Debug)]
pub(crate) struct Read {
    pub envelope: Envelope,
    /// What could not be drawn, each as `kind:name`. With anything here the
    /// screen is the web view's.
    pub missing: Vec<String>,
    /// An op was passed over: what the screen shows may be out of date.
    pub reload: bool,
}

/// What a request was answered with.
#[derive(Debug)]
pub(crate) enum Answer {
    /// An envelope. `url` is where it came from in the end; `shell` is the
    /// stamp of the shell it stands in, when the answer names one.
    Tree {
        url: String,
        status: u16,
        read: Box<Read>,
        body: String,
        shell: Option<String>,
    },
    /// A page of markup: it is the web view's.
    Html { url: String },
    /// An answer with nothing in it.
    Empty,
    /// A redirect led to another site.
    Left(String),
    /// No answer that can be used.
    Failed(Failure),
}

/// Whether this build of the app has a device feature. A haptic is never refused.
fn built_with(config: &Config, feature: Option<Feature>) -> bool {
    match feature {
        Some(Feature::Haptic) => true,
        Some(feature) => config.client.caps.iter().any(|cap| cap == feature.as_str()),
        None => false,
    }
}

/// Whether this build can start a device call: it has the call's feature, or,
/// for a sign-in with a provider, the web's way of it.
pub(crate) fn can_start(config: &Config, call: &Call) -> bool {
    if matches!(call, Call::Unknown) {
        return false;
    }
    if built_with(config, call.feature()) {
        return true;
    }
    matches!(call, Call::PlatformSignIn(sign_in) if sign_in.web.is_some()) && built_with(config, Some(Feature::AuthSession))
}

/// Looks at what a control does. `Err` names what this build can't do with
/// it. An action that calls a device feature the build lacks becomes what the
/// call says to do instead: `Ok(Some(text))` puts a line of text in the
/// control's place, `Ok(None)` after a change of `action` keeps the control.
fn settle(action: &mut Option<Action>, config: &Config) -> Result<Option<Option<String>>, String> {
    match action {
        Some(Action::Unknown) => Err("action:unknown".to_owned()),
        Some(Action::Commands { list }) if list.iter().any(|command| matches!(command, Command::Unknown)) => {
            Err("command:unknown".to_owned())
        }
        Some(Action::Device(device)) if !can_start(config, &device.call) => {
            match device.alt.take() {
                Some(Alt::Navigate { href }) => {
                    *action = Some(Action::navigate(href.as_str()));
                    Ok(None)
                }
                Some(Alt::Text { text }) => Ok(Some(Some(text))),
                // Nothing stands in for it: the control is not drawn.
                _ => Ok(Some(None)),
            }
        }
        _ => Ok(None),
    }
}

fn nothing() -> Node {
    Node::Stack(Stack {
        hidden: true,
        ..Stack::default()
    })
}

fn button(button: &mut Button, config: &Config, missing: &mut Vec<String>) -> Option<Option<String>> {
    match settle(&mut button.action, config) {
        Ok(stand_in) => stand_in,
        Err(why) => {
            missing.push(why);
            None
        }
    }
}

/// Names what this build can't draw in `node`, and puts what stands in for a
/// part in its place: the `alt` of a kind or a plugin view it does not have,
/// the stand-in of a device call it was not built with.
fn check(node: &mut Node, config: &Config, missing: &mut Vec<String>) {
    loop {
        match node {
            Node::Unknown(unknown) => match unknown.alt.take() {
                Some(alt) => *node = *alt,
                None => return missing.push(format!("kind:{}", unknown.kind)),
            },
            Node::Native(view) if !config.views.contains(&view.name) => match view.alt.take() {
                Some(alt) => *node = *alt,
                None => return missing.push(format!("native:{}", view.name)),
            },
            _ => break,
        }
    }
    let kind = node.kind().as_str();
    if config.undrawable.iter().any(|name| name == kind) {
        missing.push(format!("client:{kind}"));
    }
    // What a control does: an action or a command this build does not know
    // can't be left to do nothing, and a device call it lacks has a stand-in.
    let mut stand_in = None;
    let one = |action: &mut Option<Action>, missing: &mut Vec<String>| match settle(action, config) {
        Ok(Some(_)) => *action = None,
        Ok(None) => {}
        Err(why) => missing.push(why),
    };
    match node {
        Node::Button(inner) => stand_in = button(inner, config, missing),
        Node::Link(link) => {
            let mut action = Some(std::mem::replace(&mut link.action, Action::Back));
            match settle(&mut action, config) {
                Ok(None) => {}
                Ok(found) => stand_in = found,
                Err(why) => missing.push(why),
            }
            link.action = action.unwrap_or(Action::Back);
        }
        Node::Row(row) => {
            one(&mut row.action, missing);
            row.swipe.retain_mut(|swipe| button(swipe, config, missing).is_none());
        }
        Node::Table(table) => table.rows.iter_mut().for_each(|row| one(&mut row.action, missing)),
        Node::FeedRow(row) => one(&mut row.action, missing),
        Node::Figure(figure) => one(&mut figure.action, missing),
        Node::Toolbar(toolbar) => toolbar.items.retain_mut(|item| match settle(&mut item.action, config) {
            Ok(stand_in) => stand_in.is_none(),
            Err(why) => {
                missing.push(why);
                true
            }
        }),
        Node::Field(field) => {
            let input = match &field.input {
                Input::Text(_) => "text",
                Input::Toggle(_) => "toggle",
                Input::Choice(_) => "choice",
                Input::Choices(_) => "choices",
                Input::Date(_) => "date",
                Input::Slider(_) => "slider",
                Input::Colour(_) => "colour",
                Input::File(_) => "file",
                Input::Range(_) => "range",
                _ => "unknown",
            };
            if input == "unknown" {
                missing.push("input:unknown".to_owned());
            } else if config.undrawable.iter().any(|name| name.strip_prefix("input:") == Some(input)) {
                missing.push(format!("client:input:{input}"));
            }
        }
        _ => {}
    }
    match stand_in {
        Some(Some(text)) => return *node = Node::text(text),
        Some(None) => return *node = nothing(),
        None => {}
    }
    for slot in slots(node) {
        match slot {
            Slot::Many(list) => list.iter_mut().for_each(|child| check(child, config, missing)),
            Slot::Maybe(child) => {
                if let Some(child) = child.as_deref_mut() {
                    check(child, config, missing);
                }
            }
            Slot::One(child) => check(child, config, missing),
        }
    }
}

/// A table row that arrives by itself is looked at as one in a table is.
fn check_row(row: &mut TableRow, config: &Config, missing: &mut Vec<String>) {
    if let Err(why) = settle(&mut row.action, config).map(|stand_in| {
        if stand_in.is_some() {
            row.action = None;
        }
    }) {
        missing.push(why);
    }
    row.nodes_mut().for_each(|node| check(node, config, missing));
}

fn check_screen(screen: &mut Screen, config: &Config, missing: &mut Vec<String>) {
    for root in crate::tree::roots_mut(screen) {
        check(root, config, missing);
    }
}

fn blank() -> Envelope {
    Envelope::ops(String::new(), Vec::new())
}

/// How much stack reading an answer is given, in bytes. A tree as deep as the
/// protocol allows takes about 1.5 MiB to read, in a release build too.
const READING_STACK: usize = 4 * 1024 * 1024;

/// Runs `read` on a thread of its own with room for the deepest tree the
/// protocol allows. A client calls the core from whatever thread it draws on,
/// and that thread's stack may be a third of what reading takes: a secondary
/// thread on Apple's systems has 512 KiB. `None` when the reading failed
/// outright; where no thread can be made, it is done here.
pub(crate) fn with_room<T: Send>(read: impl FnOnce() -> T + Send) -> Option<T> {
    let mut read = Some(read);
    let made = std::thread::scope(|scope| {
        let spawned = std::thread::Builder::new().stack_size(READING_STACK).spawn_scoped(scope, || read.take().map(|read| read()));
        spawned.ok().map(|thread| thread.join().ok().flatten())
    });
    match made {
        Some(done) => done,
        None => read.take().map(|read| read()),
    }
}

/// Reads an envelope. `Err` when it is not one at all, or speaks another
/// major of the protocol.
pub(crate) fn envelope(body: &str, config: &Config) -> Result<Read, Failure> {
    with_room(|| envelope_here(body, config)).unwrap_or(Err(Failure::Protocol))
}

fn envelope_here(body: &str, config: &Config) -> Result<Read, Failure> {
    let mut read = Read {
        envelope: blank(),
        missing: Vec::new(),
        reload: false,
    };
    match serde_json::from_str::<Envelope>(body) {
        Ok(envelope) => read.envelope = envelope,
        Err(error) => {
            // A tree nested past what the protocol allows is not read at all.
            let text = error.to_string();
            if text.contains("nodes deep") || text.contains("recursion limit") || navree_native::nesting(body) > MAX_NESTING {
                read.missing.push("client:depth".to_owned());
                return Ok(read);
            }
            let value: Value = serde_json::from_str(body).map_err(|_| Failure::Protocol)?;
            if value.get("navree").and_then(Value::as_u64) != Some(u64::from(MAJOR)) {
                return Err(Failure::Protocol);
            }
            // It is an envelope, and something in it has a shape this build
            // can't read: nothing of it is trusted.
            read.missing.push("client:decode".to_owned());
            read.reload = true;
            return Ok(read);
        }
    }
    if read.envelope.navree != MAJOR {
        return Err(Failure::Protocol);
    }
    // Icons the answer keeps in one place go back to where they are drawn, so
    // a screen stands on its own.
    read.envelope.inline_icons();
    if let Some(screen) = &mut read.envelope.screen {
        check_screen(screen, config, &mut read.missing);
    }
    for op in &mut read.envelope.ops {
        match op {
            Op::Morph { node, .. } | Op::Replace { node, .. } => check(node, config, &mut read.missing),
            Op::Append { nodes, .. } | Op::Prepend { nodes, .. } | Op::Merge { nodes, .. } => {
                nodes.iter_mut().for_each(|node| check(node, config, &mut read.missing));
            }
            Op::Rows { rows, .. } => rows.iter_mut().for_each(|row| check_row(row, config, &mut read.missing)),
            Op::Row { row, .. } => check_row(row, config, &mut read.missing),
            Op::Unknown => read.reload = true,
            _ => {}
        }
    }
    read.missing.sort();
    read.missing.dedup();
    Ok(read)
}

/// Reads a response. `asked` is the address the request went to.
pub(crate) fn read(response: Response, asked: &str, origin: &Origin, config: &Config) -> Answer {
    if let Some(error) = response.error {
        return Answer::Failed(match error {
            TransportError::Timeout => Failure::Timeout,
            TransportError::Offline | TransportError::Other => Failure::Offline,
        });
    }
    if let Some(url) = response.left {
        return Answer::Left(url);
    }
    let url = if response.url.is_empty() {
        asked.to_owned()
    } else {
        match origin.resolve(&response.url, "/") {
            Target::Inside(url) => url,
            // An answer from another site is never read, whatever it holds.
            Target::Outside(url) => return Answer::Left(url),
            Target::Refused => return Answer::Failed(Failure::Protocol),
        }
    };
    let media = response
        .content_type
        .as_deref()
        .map(|kind| kind.split(';').next().unwrap_or("").trim().to_ascii_lowercase())
        .unwrap_or_default();
    let status = response.status;
    if media == CONTENT_TYPE {
        let header = response.header(SHELL_STAMP_HEADER).map(str::to_owned);
        return match envelope(&response.body, config) {
            Ok(read) => {
                let own = (!read.envelope.shell.is_empty()).then(|| read.envelope.shell.clone());
                Answer::Tree {
                    url,
                    status,
                    shell: own.or(header),
                    read: Box::new(read),
                    body: response.body,
                }
            }
            Err(failure) => Answer::Failed(failure),
        };
    }
    if status >= 500 {
        Answer::Failed(Failure::Server)
    } else if (200..300).contains(&status) && response.body.trim().is_empty() {
        Answer::Empty
    } else if media == "text/html" || media == "application/xhtml+xml" {
        Answer::Html { url }
    } else if status >= 400 {
        Answer::Failed(Failure::Server)
    } else {
        Answer::Failed(Failure::Protocol)
    }
}

/// The stamp a shell document's own answer names.
pub(crate) fn shell_stamp_of(response: &Response) -> Option<String> {
    response.header(SHELL_STAMP_HEADER).map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::walk;
    use navree_native::client::{Client, FormFactor, Platform};

    fn kinds(node: &Node) -> Vec<&'static str> {
        let mut kinds = Vec::new();
        walk(node, &mut |node| kinds.push(node.kind().as_str()));
        kinds
    }

    fn config() -> Config {
        let mut client = Client::new(Platform::Android, FormFactor::Phone);
        client.caps = vec!["share".to_owned()];
        let mut config = Config::new("https://shop.example", client);
        config.undrawable = vec!["chart".to_owned(), "input:file".to_owned()];
        config
    }

    fn body(screen_body: &str) -> String {
        format!(r#"{{"navree":1,"build":"b1","vocab":0,"screen":{{"title":"Orders","body":{screen_body}}}}}"#)
    }

    #[test]
    fn an_unknown_kind_becomes_its_alt() {
        let read = envelope(
            &body(
                r#"{"t":"stack","children":[{"t":"hologram","depth":3,"alt":{"t":"marquee","alt":{"t":"text","value":"Paid"}}},{"t":"text","value":"x","glow":1,"tone":"festive"}]}"#,
            ),
            &config(),
        )
        .unwrap();
        assert!(read.missing.is_empty(), "{:?}", read.missing);
        let screen = read.envelope.screen.unwrap();
        assert_eq!(kinds(&screen.body), ["stack", "text", "text"]);
    }

    #[test]
    fn an_unknown_kind_without_alt_is_named() {
        let read = envelope(
            &body(r#"{"t":"section","children":[{"t":"hologram"}],"footer":[{"t":"button","label":"Go","action":{"t":"teleport"}},{"t":"button","label":"Do","action":{"t":"commands","list":[{"c":"levitate"}]}},{"t":"field","name":"f","input":{"t":"mindreader"}}]}"#),
            &config(),
        )
        .unwrap();
        assert_eq!(read.missing, ["action:unknown", "command:unknown", "input:unknown", "kind:hologram"]);
        // What this build was told it can't draw yet is named too.
        let read = envelope(
            &body(r#"{"t":"stack","children":[{"t":"chart"},{"t":"field","name":"photo","input":{"t":"file"}},{"t":"native","name":"map"}]}"#),
            &config(),
        )
        .unwrap();
        assert_eq!(read.missing, ["client:chart", "client:input:file", "native:map"]);
        // A plugin view the build lacks is drawn from its alt.
        let read = envelope(
            &body(r#"{"t":"native","name":"map","alt":{"t":"text","value":"Ludhiana"}}"#),
            &config(),
        )
        .unwrap();
        assert!(read.missing.is_empty());
        assert_eq!(kinds(&read.envelope.screen.unwrap().body), ["text"]);
    }

    #[test]
    fn an_unknown_op_is_passed_over_and_asks_for_a_reload() {
        let read = envelope(
            r#"{"navree":1,"build":"b1","vocab":0,"shell":"s7","app":"a4","ops":[{"op":"confetti"},{"op":"toast","text":"Saved"},{"op":"navigate","href":"/orders/7","present":"sheet"},{"op":"device","call":{"f":"haptic","kind":"success"}}]}"#,
            &config(),
        )
        .unwrap();
        assert!(read.reload);
        assert_eq!(read.envelope.ops.len(), 4);
        assert!(matches!(read.envelope.ops[2], Op::Navigate { present: navree_native::Present::Sheet, .. }));
        assert_eq!((read.envelope.shell.as_str(), read.envelope.app.as_str()), ("s7", "a4"));
        // A word this build can't read in a closed list is the field's default, and the page is drawn.
        let read = envelope(&body(r#"{"t":"button","label":"Go","role":"teleporter"}"#), &config()).unwrap();
        assert!(read.missing.is_empty() && read.envelope.screen.is_some());
        // A shape it can't read is another matter: nothing of the answer is trusted.
        let read = envelope(&body(r#"{"t":"button","label":7}"#), &config()).unwrap();
        assert_eq!(read.missing, ["client:decode"]);
        assert!(read.envelope.screen.is_none());
    }

    #[test]
    fn a_device_call_the_build_lacks_is_what_stands_in_for_it() {
        let call = |feature: &str, alt: &str| format!(r#"{{"t":"button","label":"Go","action":{{"t":"device","call":{{"f":"{feature}"}}{alt}}}}}"#);
        let read = envelope(
            &body(&format!(
                r#"{{"t":"stack","children":[{},{},{},{},{}]}}"#,
                call("share", ""),
                call("scan_code", ""),
                call("scan_code", r#","alt":{"t":"text","text":"Type the code instead."}"#),
                call("scan_code", r#","alt":{"t":"navigate","href":"/codes/new"}"#),
                call("teleport", r#","alt":{"t":"hide"}"#),
            )),
            &config(),
        )
        .unwrap();
        assert!(read.missing.is_empty(), "{:?}", read.missing);
        let screen = serde_json::to_value(read.envelope.screen.unwrap()).unwrap();
        let children = screen["body"]["children"].as_array().unwrap();
        // The build has `share`: the control stays as it came.
        assert_eq!(children[0]["action"]["call"]["f"], "share");
        // It lacks `scan_code`: not drawn, a line of text, or another page.
        assert_eq!(children[1], serde_json::json!({"t": "stack", "hidden": true}));
        assert_eq!(children[2], serde_json::json!({"t": "text", "value": "Type the code instead."}));
        assert_eq!(children[3]["action"], serde_json::json!({"t": "navigate", "href": "/codes/new"}));
        assert_eq!(children[4], serde_json::json!({"t": "stack", "hidden": true}));
    }

    #[test]
    fn icons_kept_in_one_place_go_back_where_they_are_drawn() {
        let read = envelope(
            r#"{"navree":1,"build":"b1","vocab":0,"icons":{"truck":{"paths":["M1 3h15v13H1z"]}},"screen":{"title":"Orders","body":{"t":"button","label":"Ship","icon":{"name":"truck"}}}}"#,
            &config(),
        )
        .unwrap();
        let screen = serde_json::to_value(read.envelope.screen.unwrap()).unwrap();
        assert_eq!(screen["body"]["icon"]["paths"][0], "M1 3h15v13H1z");
        assert!(read.envelope.icons.is_empty());
    }

    #[test]
    fn another_major_and_what_is_not_an_envelope_are_refused() {
        assert_eq!(envelope(r#"{"navree":2,"build":"b","vocab":0}"#, &config()).unwrap_err(), Failure::Protocol);
        assert_eq!(envelope(r#"{"navree":2,"build":"b","vocab":0,"screen":{"t":"x"}}"#, &config()).unwrap_err(), Failure::Protocol);
        assert_eq!(envelope("<html>", &config()).unwrap_err(), Failure::Protocol);
        assert_eq!(envelope("[1]", &config()).unwrap_err(), Failure::Protocol);
    }

    #[test]
    fn a_tree_500_deep_is_not_parsed() {
        let mut tree = String::from(r#"{"t":"text","value":"end"}"#);
        for _ in 0..500 {
            tree = format!(r#"{{"t":"stack","children":[{tree}]}}"#);
        }
        let read = envelope(&body(&tree), &config()).unwrap();
        assert_eq!(read.missing, ["client:depth"]);
        assert!(read.envelope.screen.is_none());
        // One level past what the protocol allows, though its JSON still reads.
        let mut tree = String::from(r#"{"t":"text","value":"end"}"#);
        for _ in 0..navree_native::MAX_DEPTH {
            tree = format!(r#"{{"t":"stack","children":[{tree}]}}"#);
        }
        let read = envelope(&body(&tree), &config()).unwrap();
        assert_eq!(read.missing, ["client:depth"]);
    }

    fn response(status: u16, content_type: &str, body: &str) -> Response {
        Response {
            request: 1,
            status,
            url: "https://shop.example/orders".to_owned(),
            content_type: Some(content_type.to_owned()),
            body: body.to_owned(),
            ..Response::default()
        }
    }

    #[test]
    fn markup_is_the_web_views() {
        let origin = Origin::parse("https://shop.example", &[], false).unwrap();
        let config = config();
        let read = |response| read(response, "/orders", &origin, &config);
        assert!(matches!(read(response(200, "text/html; charset=utf-8", "<p>")), Answer::Html { url } if url == "/orders"));
        assert!(matches!(read(response(204, "", "")), Answer::Empty));
        assert!(matches!(read(response(503, "text/html", "<p>")), Answer::Failed(Failure::Server)));
        assert!(matches!(read(response(415, "application/json", "{}")), Answer::Failed(Failure::Server)));
        assert!(matches!(read(response(200, "application/json", "{}")), Answer::Failed(Failure::Protocol)));
        // A page that says it is missing is still an envelope, with its status.
        let mut missing = response(404, "application/vnd.navree.native+json", &body(r#"{"t":"text","value":"x"}"#));
        missing.headers = vec![("X-Navree-Shell".to_owned(), "s1".to_owned())];
        assert!(matches!(read(missing), Answer::Tree { status: 404, shell: Some(shell), .. } if shell == "s1"));
        let mut elsewhere = response(200, "application/vnd.navree.native+json", "{}");
        elsewhere.url = "https://evil.example/orders".to_owned();
        assert!(matches!(read(elsewhere), Answer::Left(_)));
        let offline = Response {
            request: 1,
            error: Some(TransportError::Offline),
            ..Response::default()
        };
        assert!(matches!(read(offline), Answer::Failed(Failure::Offline)));
    }
}
