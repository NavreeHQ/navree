//! Reading what the server answered.
//!
//! An app in a store is older than its server sooner or later, so an answer may
//! hold a kind this build has never heard of. That is never a crash and never a
//! hole in a screen: a node of an unknown kind is drawn from its `alt` when it
//! has one, and otherwise the whole page is the web view's. An op this build
//! does not know is dropped and the screen is fetched again.

use navree_native::node::Node;
use navree_native::{CONTENT_TYPE, Envelope, Input, Kind, MAJOR, Screen};
use serde::Deserialize;
use serde_json::Value;

use crate::config::Config;
use crate::event::{Response, TransportError};
use crate::origin::{Origin, Target};
use crate::state::Failure;
use crate::tree::{Slot, slots};

/// An envelope, read as far as this build can.
#[derive(Debug)]
pub(crate) struct Read {
    pub envelope: Envelope,
    /// What could not be drawn, each as `kind:name`. With anything here the
    /// screen is the web view's.
    pub missing: Vec<String>,
    /// An op was dropped: what the screen shows may be out of date.
    pub reload: bool,
    /// The stamp of the app, when the answer carries one.
    pub app: Option<String>,
    /// The stamp of the shell, when the answer carries one.
    pub shell: Option<String>,
    /// Device calls the reply starts, as they came.
    pub device: Vec<Value>,
}

/// What a request was answered with.
#[derive(Debug)]
pub(crate) enum Answer {
    /// An envelope. `url` is where it came from in the end.
    Tree {
        url: String,
        status: u16,
        read: Box<Read>,
        body: String,
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

#[derive(Default, Deserialize)]
struct Stamps {
    #[serde(default)]
    app: Option<Value>,
    #[serde(default)]
    shell: Option<Value>,
}

fn stamp(value: Option<Value>) -> Option<String> {
    match value? {
        Value::String(text) => Some(text),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

const ACTIONS: [&str; 7] = ["navigate", "submit", "request", "commands", "back", "refresh", "dismiss"];
const COMMANDS: [&str; 14] = [
    "toggle", "show", "hide", "open", "close", "copy", "set", "flip", "focus", "submit", "text", "count", "value",
    "toast",
];
const INPUTS: [&str; 8] = ["text", "toggle", "choice", "choices", "date", "slider", "colour", "file"];
const OPS: [&str; 15] = [
    "morph", "replace", "append", "prepend", "remove", "errors", "toast", "open", "close", "focus", "url",
    "navigate", "navigate_external", "reload", "invalidate",
];

fn known_kind(name: &str) -> bool {
    Kind::ALL.iter().any(|kind| kind.as_str() == name)
}

fn tag<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

/// Puts `alt` in place of every node of a kind this build does not know, and
/// names what has none. `value` stands where a node is expected.
fn mend_node(value: &mut Value, missing: &mut Vec<String>) {
    if let Some(kind) = tag(value, "t").map(str::to_owned) {
        if !known_kind(&kind) {
            match value.get_mut("alt").map(Value::take) {
                Some(alt) if alt.is_object() => {
                    *value = alt;
                    mend_node(value, missing);
                }
                _ => missing.push(format!("kind:{kind}")),
            }
            return;
        }
    }
    let Some(fields) = value.as_object_mut() else {
        return;
    };
    for (key, child) in fields {
        match key.as_str() {
            "action" => mend_action(child, missing),
            "input" => {
                if let Some(kind) = tag(child, "t").filter(|kind| !INPUTS.contains(kind)) {
                    missing.push(format!("input:{kind}"));
                }
            }
            _ => mend_inside(child, missing),
        }
    }
}

/// Mends whatever nodes a field of a node holds, however they are wrapped.
fn mend_inside(value: &mut Value, missing: &mut Vec<String>) {
    match value {
        Value::Array(items) => items.iter_mut().for_each(|item| mend_inside(item, missing)),
        Value::Object(_) => mend_node(value, missing),
        _ => {}
    }
}

fn mend_action(value: &mut Value, missing: &mut Vec<String>) {
    match tag(value, "t") {
        Some("commands") => {
            for command in value.get("list").and_then(Value::as_array).into_iter().flatten() {
                if let Some(name) = tag(command, "c").filter(|name| !COMMANDS.contains(name)) {
                    missing.push(format!("command:{name}"));
                }
            }
        }
        Some(kind) if !ACTIONS.contains(&kind) => missing.push(format!("action:{kind}")),
        _ => {}
    }
}

/// Mends an envelope that did not read: unknown kinds, ops and device calls.
fn mend(value: &mut Value, missing: &mut Vec<String>, reload: &mut bool, device: &mut Vec<Value>) {
    if let Some(screen) = value.get_mut("screen") {
        for key in ["actions", "body"] {
            if let Some(part) = screen.get_mut(key) {
                mend_inside(part, missing);
            }
        }
    }
    if let Some(ops) = value.get_mut("ops").and_then(Value::as_array_mut) {
        let mut kept = Vec::with_capacity(ops.len());
        for mut op in ops.drain(..) {
            let name = tag(&op, "op").map(str::to_owned);
            match name.as_deref() {
                Some("device") => device.push(op),
                Some(name) if OPS.contains(&name) => {
                    for key in ["node", "nodes"] {
                        if let Some(part) = op.get_mut(key) {
                            mend_inside(part, missing);
                        }
                    }
                    kept.push(op);
                }
                _ => *reload = true,
            }
        }
        *ops = kept;
    }
}

/// Names what this build can't draw in `node`, and puts `alt` in place of a
/// plugin view the build does not have.
fn check(node: &mut Node, config: &Config, missing: &mut Vec<String>) {
    loop {
        let Node::Native(view) = node else { break };
        if config.views.contains(&view.name) {
            break;
        }
        match view.alt.take() {
            Some(alt) => *node = *alt,
            None => {
                missing.push(format!("native:{}", view.name));
                return;
            }
        }
    }
    let kind = node.kind().as_str();
    if config.undrawable.iter().any(|name| name == kind) {
        missing.push(format!("client:{kind}"));
    }
    if let Node::Field(field) = node {
        let input = match &field.input {
            Input::Text(_) => "text",
            Input::Toggle(_) => "toggle",
            Input::Choice(_) => "choice",
            Input::Choices(_) => "choices",
            Input::Date(_) => "date",
            Input::Slider(_) => "slider",
            Input::Colour(_) => "colour",
            Input::File(_) => "file",
            _ => "unknown",
        };
        let name = format!("input:{input}");
        if config.undrawable.contains(&name) {
            missing.push(format!("client:{name}"));
        }
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

fn check_screen(screen: &mut Screen, config: &Config, missing: &mut Vec<String>) {
    for action in &mut screen.actions {
        check(action, config, missing);
    }
    check(&mut screen.body, config, missing);
}

fn blank() -> Envelope {
    Envelope::ops(String::new(), Vec::new())
}

/// Reads an envelope. `Err` when it is not one at all, or speaks another
/// major of the protocol.
pub(crate) fn envelope(body: &str, config: &Config) -> Result<Read, Failure> {
    let mut read = Read {
        envelope: blank(),
        missing: Vec::new(),
        reload: false,
        app: None,
        shell: None,
        device: Vec::new(),
    };
    match serde_json::from_str::<Envelope>(body) {
        Ok(envelope) => read.envelope = envelope,
        Err(error) => {
            // A tree nested past what the reader allows is not parsed at all.
            if error.to_string().contains("recursion limit") {
                read.missing.push("client:depth".to_owned());
                return Ok(read);
            }
            let mut value: Value = serde_json::from_str(body).map_err(|_| Failure::Protocol)?;
            if !value.is_object() {
                return Err(Failure::Protocol);
            }
            if value.get("navree").and_then(Value::as_u64) != Some(u64::from(MAJOR)) {
                return Err(Failure::Protocol);
            }
            mend(&mut value, &mut read.missing, &mut read.reload, &mut read.device);
            if !read.missing.is_empty() {
                // What holds something this build can't draw is not read at all:
                // the page is the web view's, and a change that brings it asks for a reload.
                if let Some(fields) = value.as_object_mut() {
                    fields.remove("screen");
                }
                if let Some(ops) = value.get_mut("ops").and_then(Value::as_array_mut) {
                    let before = ops.len();
                    ops.retain(|op| op.get("node").is_none() && op.get("nodes").is_none());
                    read.reload |= ops.len() != before;
                }
            }
            match serde_json::from_value::<Envelope>(value) {
                Ok(envelope) => read.envelope = envelope,
                Err(_) => {
                    read.missing.push("client:decode".to_owned());
                    read.missing.sort();
                    read.missing.dedup();
                    read.reload = true;
                    return Ok(read);
                }
            }
        }
    }
    if read.envelope.navree != MAJOR {
        return Err(Failure::Protocol);
    }
    if let Ok(stamps) = serde_json::from_str::<Stamps>(body) {
        read.app = stamp(stamps.app);
        read.shell = stamp(stamps.shell);
    }
    if let Some(screen) = &mut read.envelope.screen {
        check_screen(screen, config, &mut read.missing);
    }
    for op in &mut read.envelope.ops {
        match op {
            navree_native::Op::Morph { node, .. } | navree_native::Op::Replace { node, .. } => {
                check(node, config, &mut read.missing);
            }
            navree_native::Op::Append { nodes, .. } | navree_native::Op::Prepend { nodes, .. } => {
                nodes.iter_mut().for_each(|node| check(node, config, &mut read.missing));
            }
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
        return match envelope(&response.body, config) {
            Ok(read) => Answer::Tree {
                url,
                status,
                read: Box::new(read),
                body: response.body,
            },
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
        let mut config = Config::new("https://shop.example", Client::new(Platform::Android, FormFactor::Phone));
        config.undrawable = vec!["chart".to_owned(), "input:file".to_owned()];
        config
    }

    fn body(screen_body: &str) -> String {
        format!(r#"{{"navree":1,"build":"b1","vocab":1,"screen":{{"title":"Orders","body":{screen_body}}}}}"#)
    }

    #[test]
    fn an_unknown_kind_becomes_its_alt() {
        let read = envelope(
            &body(
                r#"{"t":"stack","children":[{"t":"hologram","depth":3,"alt":{"t":"marquee","alt":{"t":"text","value":"Paid"}}},{"t":"text","value":"x","glow":1}]}"#,
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
            &body(r#"{"t":"section","children":[{"t":"hologram"}],"footer":[{"t":"button","label":"Go","action":{"t":"teleport"}}]}"#),
            &config(),
        )
        .unwrap();
        assert_eq!(read.missing, ["action:teleport", "kind:hologram"]);
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
    fn an_unknown_op_is_dropped_and_asks_for_a_reload() {
        let read = envelope(
            r#"{"navree":1,"build":"b1","vocab":1,"shell":"s7","app":4,"ops":[{"op":"confetti"},{"op":"toast","text":"Saved"},{"op":"device","name":"haptic","args":{"kind":"success"}}]}"#,
            &config(),
        )
        .unwrap();
        assert!(read.reload);
        assert_eq!(read.envelope.ops.len(), 1);
        assert_eq!(read.device.len(), 1);
        assert_eq!(read.shell.as_deref(), Some("s7"));
        assert_eq!(read.app.as_deref(), Some("4"));
        // A value this build can't read anywhere else: nothing of the answer is trusted.
        let read = envelope(&body(r#"{"t":"button","label":"Go","role":"teleporter"}"#), &config()).unwrap();
        assert_eq!(read.missing, ["client:decode"]);
        assert!(read.envelope.screen.is_none());
    }

    #[test]
    fn another_major_and_what_is_not_an_envelope_are_refused() {
        assert_eq!(envelope(r#"{"navree":2,"build":"b","vocab":1}"#, &config()).unwrap_err(), Failure::Protocol);
        assert_eq!(envelope(r#"{"navree":2,"build":"b","vocab":1,"screen":{"t":"x"}}"#, &config()).unwrap_err(), Failure::Protocol);
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
        let missing = response(404, "application/vnd.navree.native+json", &body(r#"{"t":"text","value":"x"}"#));
        assert!(matches!(read(missing), Answer::Tree { status: 404, .. }));
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
