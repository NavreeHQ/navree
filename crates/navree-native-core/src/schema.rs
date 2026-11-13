//! The JSON forms of everything that crosses the C interface, as a JSON
//! Schema. A client that generates or checks its models uses this; the file
//! `schema/navree-native-core.schema.json` is what [`schema`] writes.
//!
//! What belongs to the protocol (a screen, a node, an action) is named here and
//! described there: this schema says only that it is an object, and which of
//! the protocol's types it is.
//!
//! The tests hold the schema to the code: every event, update and view state
//! of every transcript fits it, and nothing in them has a field it does not name.

use serde_json::{Map, Value, json};

fn string() -> Value {
    json!({"type": "string"})
}

fn integer() -> Value {
    json!({"type": "integer"})
}

fn boolean() -> Value {
    json!({"type": "boolean"})
}

fn list(items: Value) -> Value {
    json!({"type": "array", "items": items})
}

fn map(values: Value) -> Value {
    json!({"type": "object", "additionalProperties": values})
}

fn one(name: &str) -> Value {
    json!({"$ref": format!("#/$defs/{name}")})
}

fn words(values: &[&str]) -> Value {
    json!({"type": "string", "enum": values})
}

/// A value the protocol crate describes.
fn protocol(name: &str) -> Value {
    json!({"type": "object", "description": format!("`{name}` of the protocol: see the schema of `navree-native`.")})
}

/// An object with exactly these fields. A name that ends in `?` may be left out.
fn object(fields: &[(&str, Value)]) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for (name, schema) in fields {
        match name.strip_suffix('?') {
            Some(name) => {
                properties.insert(name.to_owned(), schema.clone());
            }
            None => {
                properties.insert((*name).to_owned(), schema.clone());
                required.push(json!(name));
            }
        }
    }
    json!({"type": "object", "properties": properties, "required": required, "additionalProperties": false})
}

/// One form of a tagged value: an object whose `tag` field is `name`.
fn form(tag: &str, name: &str, fields: &[(&str, Value)]) -> Value {
    let mut all = vec![(tag, json!({"const": name}))];
    all.extend(fields.iter().cloned());
    object(&all)
}

fn forms(tag: &str, each: &[(&str, &[(&str, Value)])]) -> Value {
    let options: Vec<Value> = each.iter().map(|(name, fields)| form(tag, name, fields)).collect();
    json!({"oneOf": options})
}

fn failure() -> Value {
    words(&["offline", "timeout", "server", "protocol", "config"])
}

fn notice() -> Value {
    words(&["offline", "timeout", "failed", "unsaved", "loading", "copied", "fields_need_attention", "loaded", "second_step"])
}

fn headers() -> Value {
    list(json!({"type": "array", "items": string()}))
}

fn message() -> Vec<(&'static str, Value)> {
    vec![
        ("id", integer()),
        ("tone", words(&["neutral", "accent", "success", "warning", "danger", "info"])),
        ("text?", string()),
        ("title?", string()),
        ("notice?", notice()),
        ("action?", string()),
    ]
}

/// The schema of [`Config`](crate::Config), [`Event`](crate::Event),
/// [`Update`](crate::Update), [`Query`](crate::Query) and
/// [`ViewState`](crate::ViewState), each under `$defs`.
pub fn schema() -> Value {
    let screen = integer;
    let event: &[(&str, &[(&str, Value)])] = &[
        ("start", &[]),
        (
            "response",
            &[
                ("request", integer()),
                ("status?", integer()),
                ("url?", string()),
                ("content_type?", string()),
                ("location?", string()),
                ("headers?", headers()),
                ("body?", string()),
                ("error?", words(&["offline", "timeout", "other"])),
                ("left?", string()),
            ],
        ),
        ("select", &[("destination", integer())]),
        (
            "act",
            &[
                ("screen?", screen()),
                ("action", protocol("Action")),
                ("form?", string()),
                ("overlay?", string()),
                ("text?", string()),
                ("now?", integer()),
                ("ask?", protocol("Question")),
                ("from?", string()),
            ],
        ),
        ("answer", &[("screen", screen()), ("question", integer()), ("yes", boolean())]),
        ("choose", &[("screen", screen()), ("table", string()), ("row?", string()), ("on", boolean()), ("across?", boolean())]),
        ("tab", &[("screen", screen()), ("id", string()), ("index", integer())]),
        ("random", &[("request", integer()), ("bytes", string())]),
        ("push_token", &[("token", string()), ("environment?", words(&["sandbox", "production"]))]),
        ("restore", &[("snapshot", json!({}))]),
        ("move", &[("screen", screen()), ("item", string()), ("from", string()), ("to", string()), ("before?", string())]),
        ("options", &[("screen", screen()), ("form?", string()), ("field", string()), ("text?", string()), ("more?", boolean())]),
        ("field", &[("screen", screen()), ("form?", string()), ("field", string()), ("value", one("FieldValue"))]),
        ("search", &[("screen", screen()), ("text", string())]),
        ("back", &[]),
        ("pop_to", &[("screen", screen())]),
        ("dismissed", &[("screen", screen())]),
        ("overlay", &[("screen", screen()), ("id", string()), ("open", boolean())]),
        ("refresh", &[("screen", screen())]),
        ("region_retry", &[("screen", screen()), ("region", string())]),
        ("more", &[("screen", screen()), ("id", string()), ("before?", boolean())]),
        ("region_go", &[("screen", screen()), ("region", string()), ("href", string())]),
        ("message_shown", &[("id", integer())]),
        ("message_action", &[("id", integer())]),
        ("web_link", &[("screen", screen()), ("url", string())]),
        ("open_link", &[("url", string())]),
        ("notification", &[("href", string())]),
        ("window", &[("width", one("Width"))]),
        (
            "environment",
            &[
                ("scheme?", words(&["light", "dark"])),
                ("contrast?", words(&["normal", "more"])),
                ("text_percent?", integer()),
                ("locale?", string()),
                ("tz?", string()),
            ],
        ),
        ("foreground", &[]),
        ("background", &[]),
        ("signed_out", &[]),
        ("timer", &[("timer", integer())]),
        (
            "stream",
            &[
                ("stream", integer()),
                ("state", words(&["open", "event", "closed"])),
                ("name?", string()),
                ("status?", integer()),
            ],
        ),
        ("cache", &[("key", string()), ("value?", string())]),
        ("device_result", &[("call", integer()), ("ok", boolean()), ("refusal?", string()), ("fields?", list(protocol("Pair")))]),
        ("sign_in_done", &[("url?", string())]),
        ("focused", &[("screen", screen()), ("serial", integer())]),
        ("open_upgrade", &[]),
    ];
    let fetch = [
        ("request", integer()),
        ("method", words(&["get", "post", "delete"])),
        ("url", string()),
        ("headers", headers()),
        ("body?", one("Body")),
        ("timeout_ms", integer()),
        ("xsrf?", boolean()),
    ];
    let message = message();
    let effect: &[(&str, &[(&str, Value)])] = &[
        ("fetch", &fetch),
        ("cancel", &[("request", integer())]),
        ("open_url", &[("url", string()), ("inside?", boolean())]),
        ("sign_in", &[("url", string()), ("callback?", string()), ("ephemeral?", boolean())]),
        ("message", message.as_slice()),
        ("haptic", &[("kind", string())]),
        ("device", &[("call", integer()), ("device", protocol("device::Call")), ("posts?", boolean()), ("options?", json!({"type": "object"}))]),
        ("random", &[("request", integer()), ("bytes", integer())]),
        ("forget_session", &[]),
        ("cache_read", &[("key", string())]),
        ("cache_write", &[("key", string()), ("value?", string())]),
        ("cache_clear", &[]),
        ("announce", &[("text?", string()), ("notice?", notice()), ("count?", integer())]),
        ("copy", &[("text", string()), ("sensitive?", boolean())]),
        ("timer", &[("timer", integer()), ("ms", integer())]),
        ("cancel_timer", &[("timer", integer())]),
        ("stream_open", &[("stream", integer()), ("url", string()), ("headers", headers())]),
        ("stream_close", &[("stream", integer())]),
        ("exit", &[]),
        ("report", &[("what", string()), ("detail", string())]),
    ];
    let part: &[(&str, &[(&str, Value)])] = &[
        ("all", &[]),
        ("app", &[]),
        ("nav", &[]),
        ("screen", &[("id", screen())]),
        ("form", &[("screen", screen()), ("form", string())]),
        ("rows", &[("screen", screen()), ("id", string()), ("from", integer()), ("count", integer())]),
        ("redraw", &[("screen", screen()), ("id", string()), ("rows", list(string())), ("gone?", list(string()))]),
        ("snapshot", &[]),
        ("messages", &[]),
    ];
    let change: Vec<(&str, &[(&str, Value)])> = part.iter().filter(|(name, _)| !matches!(*name, "all" | "snapshot")).copied().collect();
    let kept_screen = object(&[("url", string()), ("title?", string()), ("fields?", map(one("FieldValue"))), ("open?", list(string())), ("from?", string())]);
    let defs = json!({
        "Config": object(&[
            ("origin", string()),
            ("also?", list(string())),
            ("loopback_http?", boolean()),
            ("app_id?", string()),
            ("client", protocol("ClientDescriptor")),
            ("back_to_first?", boolean()),
            ("deep_links?", words(&["routes", "origin"])),
            ("cache?", object(&[("pages?", integer()), ("persist?", boolean())])),
            ("undrawable?", list(string())),
            ("views?", list(string())),
            ("sign_in?", object(&[("paths?", list(string())), ("callback?", string()), ("native?", list(string()))])),
        ]),
        "Event": forms("t", event),
        "FieldValue": forms("t", &[
            ("text", &[("text", string())]),
            ("flag", &[("on", boolean())]),
            ("many", &[("values", list(string()))]),
            ("files", &[("files", list(object(&[("token", string()), ("name", string()), ("type?", string()), ("size?", integer())])))]),
            ("none", &[]),
            ("phone", &[("text", string()), ("country", string())]),
            ("range", &[("from?", string()), ("to?", string())]),
        ]),
        "Snapshot": object(&[
            ("version", integer()),
            ("origin", string()),
            ("person?", string()),
            ("selected?", string()),
            ("destinations?", list(object(&[("href", string()), ("stack", list(kept_screen.clone()))]))),
            ("modals?", list(object(&[("present", words(&["sheet", "full", "task"])), ("stack", list(kept_screen))]))),
        ]),
        "Update": object(&[("changed", list(one("Change"))), ("effects", list(one("Effect")))]),
        "Change": forms("t", &change),
        "Effect": forms("t", effect),
        "Body": forms("t", &[
            ("form", &[("text", string())]),
            ("multipart", &[("parts", list(one("Part")))]),
            ("json", &[("text", string())]),
        ]),
        "Part": forms("t", &[
            ("text", &[("name", string()), ("value", string())]),
            ("file", &[("name", string()), ("token", string()), ("filename", string()), ("type", string())]),
        ]),
        "Query": forms("part", part),
        "Width": words(&["compact", "medium", "expanded"]),
        "Message": object(&message),
        "ViewState": object(&[
            ("stage", words(&["starting", "ready", "failed", "upgrade"])),
            ("failure?", failure()),
            ("upgrade?", object(&[("min_app", string()), ("url", string())])),
            ("app?", object(&[
                ("name", string()),
                ("context?", string()),
                ("home", string()),
                ("theme?", protocol("shell::Theme")),
                ("user?", protocol("shell::User")),
            ])),
            ("destinations", list(object(&[
                ("label", string()),
                ("icon?", protocol("node::Icon")),
                ("href", string()),
                ("badge?", string()),
                ("group?", string()),
                ("items", list(protocol("shell::NavItem"))),
                ("stack", list(integer())),
            ]))),
            ("selected", integer()),
            ("modals", list(object(&[
                ("present", words(&["sheet", "full", "task"])),
                ("locked", boolean()),
                ("stack", list(integer())),
            ]))),
            ("front?", integer()),
            ("can_go_back", boolean()),
            ("window", object(&[("width", one("Width"))])),
            ("screens", map(one("Screen"))),
            ("messages", list(one("Message"))),
        ]),
        "Screen": object(&[
            ("id", integer()),
            ("url", string()),
            ("title?", string()),
            ("content", forms("t", &[
                ("blank", &[]),
                ("native", &[("screen", protocol("Screen"))]),
                ("web", &[("url", string()), ("title?", string()), ("why", list(string())), ("serial", integer())]),
            ])),
            ("loading", boolean()),
            ("refreshing", boolean()),
            ("cached", boolean()),
            ("failure?", failure()),
            ("status", integer()),
            ("chrome", boolean()),
            ("overlays", list(string())),
            ("states", map(list(words(&["open", "hidden", "checked", "selected", "disabled", "active", "invalid", "pending"])))),
            ("forms", map(one("Form"))),
            ("regions", map(object(&[("loading", boolean()), ("failure?", failure())]))),
            ("lists", map(object(&[("loading", boolean()), ("failed", boolean()), ("loading_before?", boolean()), ("failed_before?", boolean()), ("across?", boolean())]))),
            ("focus?", object(&[("target", string()), ("serial", integer())])),
            ("question?", object(&[("id", integer()), ("title", string()), ("text?", string()), ("confirm", string()), ("cancel?", string())])),
            ("from?", string()),
        ]),
        "Form": object(&[
            ("pending", boolean()),
            ("fields", map(object(&[
                ("name", string()),
                ("value", one("FieldValue")),
                ("errors", list(string())),
                ("options?", list(protocol("ChoiceOption"))),
                ("more?", boolean()),
                ("searching?", boolean()),
            ]))),
        ]),
    });
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "https://navree.com/schema/navree-native-core.schema.json",
        "title": "Navree Native runtime core",
        "description": "What crosses the core's C interface: the configuration, events in, updates out, queries and the view state.",
        "$defs": defs,
    })
}
