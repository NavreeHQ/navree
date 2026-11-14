//! The conversations in `transcripts/`: each a sequence of events in and the
//! updates expected out, replayed against a fresh core.
//!
//! A step holds `send` (the event), `body` (a file under `transcripts/fixtures`
//! whose text is the body of a `response`, or the value of a `cache` event),
//! `expect` (the whole update) and
//! `check` (pointers into the view state after the step, with what they must
//! hold). `expect` is written by the tool:
//!
//! `NAVREE_CORE_TRANSCRIPTS=write cargo test -p navree-native-core --test transcripts`
//!
//! writes every `expect` again after a change that is meant. `check` is written
//! by hand and never by the tool, so a change that moves a `check` is seen.
//!
//! Every step of every transcript is also held to three rules that no
//! transcript states: each part of the view state that differs after the step
//! is named in the update's `changed`; reading a changed part alone gives what
//! the whole view state holds for it; and the event, the update and the view
//! state fit the schema the crate exports.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::PathBuf;

use navree_native_core::{Change, Config, Core, Event, Query, Update};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Deserialize, Serialize)]
struct Check {
    /// A JSON pointer into the whole view state.
    at: String,
    /// What it must hold. `null` also for what is not there.
    is: Value,
}

#[derive(Debug, Deserialize, Serialize)]
struct Step {
    #[serde(default)]
    note: Option<String>,
    send: Event,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    expect: Option<Update>,
    #[serde(default)]
    check: Vec<Check>,
    /// What the core gives a client to store, after this step. The tool
    /// writes it where a step has the field at all.
    #[serde(default)]
    snapshot: Option<Value>,
    /// The system ended the app before this step: the core is made again.
    #[serde(default)]
    restart: bool,
    /// The step's `restore` carries the snapshot taken last. The tool writes it in.
    #[serde(default)]
    last: bool,
}

#[derive(Debug, Deserialize, Serialize)]
struct Transcript {
    about: String,
    #[serde(default)]
    config: Value,
    steps: Vec<Step>,
}

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("transcripts")
}

fn config(over: &Value) -> Config {
    let mut base = json!({
        "origin": "https://shop.example",
        "client": {"platform": "android", "form": "phone", "vocab": 0, "text_percent": 100},
    });
    if let (Some(base), Some(over)) = (base.as_object_mut(), over.as_object()) {
        for (key, value) in over {
            base.insert(key.clone(), value.clone());
        }
    }
    serde_json::from_value(base).expect("the transcript's config reads")
}

fn compact<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("it writes")
}

/// Writes a transcript so a person can read it: one event, one change, one
/// effect and one check a line.
fn written(transcript: &Transcript) -> String {
    let mut out = String::from("{\n");
    let _ = writeln!(out, "  \"about\": {},", compact(&transcript.about));
    if !transcript.config.is_null() {
        let _ = writeln!(out, "  \"config\": {},", compact(&transcript.config));
    }
    out.push_str("  \"steps\": [\n");
    for (at, step) in transcript.steps.iter().enumerate() {
        out.push_str("    {\n");
        let mut lines = Vec::new();
        if let Some(note) = &step.note {
            lines.push(format!("      \"note\": {}", compact(note)));
        }
        if step.restart {
            lines.push("      \"restart\": true".to_owned());
        }
        if step.last {
            lines.push("      \"last\": true".to_owned());
        }
        lines.push(format!("      \"send\": {}", compact(&step.send)));
        if let Some(body) = &step.body {
            lines.push(format!("      \"body\": {}", compact(body)));
        }
        if let Some(expect) = &step.expect {
            let list = |items: Vec<String>| {
                if items.is_empty() {
                    "[]".to_owned()
                } else {
                    format!("[\n          {}\n        ]", items.join(",\n          "))
                }
            };
            let changed = list(expect.changed.iter().map(compact).collect());
            let effects = list(expect.effects.iter().map(compact).collect());
            lines.push(format!("      \"expect\": {{\n        \"changed\": {changed},\n        \"effects\": {effects}\n      }}"));
        }
        if !step.check.is_empty() {
            let checks: Vec<String> = step.check.iter().map(compact).collect();
            lines.push(format!(
                "      \"check\": [\n        {}\n      ]",
                checks.join(",\n        ")
            ));
        }
        if let Some(snapshot) = &step.snapshot {
            lines.push(format!("      \"snapshot\": {}", compact(snapshot)));
        }
        out.push_str(&lines.join(",\n"));
        out.push_str(if at + 1 == transcript.steps.len() {
            "\n    }\n"
        } else {
            "\n    },\n"
        });
    }
    out.push_str("  ]\n}\n");
    out
}

/// The part of the whole view state a change names.
fn part_of(view: &Value, change: &Change) -> Option<Value> {
    match change {
        Change::App => Some(view.get("app").cloned().unwrap_or(Value::Null)),
        Change::Screen { id } => Some(
            view.pointer(&format!("/screens/{id}"))
                .cloned()
                .unwrap_or(Value::Null),
        ),
        Change::Form { screen, form } => {
            let form = form.replace('~', "~0").replace('/', "~1");
            Some(
                view.pointer(&format!("/screens/{screen}/forms/{form}"))
                    .cloned()
                    .unwrap_or(Value::Null),
            )
        }
        Change::Messages => Some(view["messages"].clone()),
        _ => None,
    }
}

fn query_of(change: &Change) -> Query {
    match change {
        Change::App => Query::App,
        Change::Nav => Query::Nav,
        Change::Screen { id } => Query::Screen { id: *id },
        Change::Form { screen, form } => Query::Form {
            screen: *screen,
            form: form.clone(),
        },
        Change::Rows {
            screen,
            id,
            from,
            count,
        } => Query::Rows {
            screen: *screen,
            id: id.clone(),
            from: *from,
            count: *count,
        },
        Change::Redraw {
            screen,
            id,
            rows,
            gone,
        } => Query::Redraw {
            screen: *screen,
            id: id.clone(),
            rows: rows.clone(),
            gone: gone.clone(),
        },
        Change::Messages => Query::Messages,
        other => panic!("a change the runner does not know: {other:?}"),
    }
}

/// The list or table `id` in a screen's tree: the node itself, or the table
/// in the region of that id.
fn list_in<'a>(node: &'a mut Value, id: &str) -> Option<&'a mut Value> {
    let kind = node.get("t").and_then(Value::as_str).map(str::to_owned);
    let named = node.get("id").and_then(Value::as_str) == Some(id);
    match (kind.as_deref(), named) {
        (Some("list" | "table"), true) => return Some(node),
        (Some("region"), true) => {
            return node
                .get_mut("children")?
                .as_array_mut()?
                .iter_mut()
                .find(|child| child.get("t").and_then(Value::as_str) == Some("table"));
        }
        _ => {}
    }
    match node {
        Value::Object(map) => map.values_mut().find_map(|value| list_in(value, id)),
        Value::Array(list) => list.iter_mut().find_map(|value| list_in(value, id)),
        _ => None,
    }
}

/// The rows of a list or table, taken out of it, with what it says of
/// itself: what is left of the screen is everything else.
fn take_rows(screen: &mut Value, id: &str) -> Vec<Value> {
    let Some(list) = list_in(screen, id).and_then(Value::as_object_mut) else {
        return Vec::new();
    };
    for said in ["more", "before", "prev", "since", "summary"] {
        list.remove(said);
    }
    let rows = list
        .get_mut("children")
        .map(Value::take)
        .or_else(|| list.get_mut("rows").map(Value::take));
    rows.and_then(|rows| rows.as_array().cloned())
        .unwrap_or_default()
}

/// The ids of the rows that are chosen, with the mark taken off each: a tick
/// is said by the list of the rows chosen, not by a row drawn again.
fn take_chosen(rows: &mut [Value]) -> Vec<Value> {
    let mut chosen = Vec::new();
    for row in rows {
        if let Some(map) = row.as_object_mut()
            && map.remove("selected") == Some(Value::Bool(true))
        {
            chosen.push(map.get("id").cloned().unwrap_or(Value::Null));
        }
    }
    chosen
}

/// A change that names rows names every one: the rows it left alone are as
/// they were and where they were, and doing to the old rows what the part
/// says gives the rows the list holds now.
fn rows_are_as_said(place: &str, core: &Core, was: &mut Value, now: &mut Value, change: &Change) {
    let (id, part) = match change {
        Change::Rows { id, .. } | Change::Redraw { id, .. } => (id, core.part(&query_of(change))),
        _ => return,
    };
    let mut rows = take_rows(&mut was["content"], id);
    let mut held = take_rows(&mut now["content"], id);
    let named = |row: &Value| row.get("id").and_then(Value::as_str).map(str::to_owned);
    match change {
        Change::Rows { from, count, .. } => {
            assert_eq!(
                rows.len(),
                *from,
                "{place}: {change:?}: the list held another number of rows before"
            );
            let mut added = part
                .get("nodes")
                .or(part.get("rows"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            assert_eq!(added.len(), *count, "{place}: {change:?} read alone");
            // Which rows are chosen is said for the whole table at once.
            take_chosen(&mut rows);
            take_chosen(&mut added);
            let chosen = take_chosen(&mut held);
            if let Some(said) = part.get("selected").and_then(Value::as_array) {
                assert_eq!(said, &chosen, "{place}: {change:?}: the rows chosen");
            }
            rows.extend(added);
        }
        _ => {
            let gone: Vec<String> = part["gone"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect();
            let put = part["rows"].as_array().cloned().unwrap_or_default();
            rows.retain(|row| {
                named(row).is_none_or(|id| {
                    !gone.contains(&id)
                        && !put
                            .iter()
                            .any(|put| put["id"].as_str() == Some(id.as_str()))
                })
            });
            // From the last to the first, each before the row it stands before now.
            for put in put.iter().rev() {
                let row = put
                    .get("node")
                    .or(put.get("row"))
                    .cloned()
                    .unwrap_or(Value::Null);
                let at = match put["before"].as_str() {
                    Some(before) => rows
                        .iter()
                        .position(|row| named(row).as_deref() == Some(before))
                        .unwrap_or_else(|| {
                            panic!("{place}: {change:?}: `{before}` is not in the list")
                        }),
                    None => rows.len(),
                };
                rows.insert(at, row);
            }
        }
    }
    assert_eq!(
        rows, held,
        "{place}: {change:?} does not name everything that changed in the list"
    );
}

const NAV: [&str; 9] = [
    "stage",
    "failure",
    "upgrade",
    "destinations",
    "selected",
    "modals",
    "front",
    "can_go_back",
    "window",
];

/// Every part of the view state that differs is named in `changed`.
fn nothing_changes_unsaid(
    name: &str,
    at: usize,
    core: &Core,
    before: &Value,
    after: &Value,
    update: &Update,
) {
    let said = |change: &Change| update.changed.contains(change);
    let place = format!("{name}, at {}", at + 1);
    if before["app"] != after["app"] {
        assert!(
            said(&Change::App),
            "{place}: `app` changed and the update does not say so"
        );
    }
    if NAV.iter().any(|key| before[key] != after[key]) {
        assert!(
            said(&Change::Nav),
            "{place}: where the person is changed and the update does not say so"
        );
    }
    if before["messages"] != after["messages"] {
        assert!(
            said(&Change::Messages),
            "{place}: the messages changed and the update does not say so"
        );
    }
    let empty = serde_json::Map::new();
    let screens = after["screens"].as_object().unwrap_or(&empty);
    for (id, screen) in screens {
        let was = &before["screens"][id];
        if was == screen {
            continue;
        }
        let id: u64 = id.parse().expect("a screen's number");
        if said(&Change::Screen { id }) {
            continue;
        }
        // Without the whole screen, each difference is a form or the rows of a list.
        let mut was = was.clone();
        let mut now = screen.clone();
        let mut rows = false;
        for change in &update.changed {
            match change {
                Change::Form { screen, form } if *screen == id => {
                    was["forms"][form] = Value::Null;
                    now["forms"][form] = Value::Null;
                }
                Change::Rows { screen, .. } | Change::Redraw { screen, .. } if *screen == id => {
                    rows = true;
                    rows_are_as_said(&place, core, &mut was, &mut now, change);
                }
                _ => {}
            }
        }
        if rows {
            // With the rows taken out, the tree is what it was. The forms in
            // the rows and the list's own state are read with the rows.
            for key in ["forms", "lists"] {
                was[key] = Value::Null;
                now[key] = Value::Null;
            }
        }
        assert_eq!(
            was, now,
            "{place}: screen {id} changed and the update does not say so"
        );
    }
}

/// Reading a changed part alone gives what the whole view state holds for it.
fn parts_read_alone(name: &str, at: usize, core: &Core, after: &Value, update: &Update) {
    for change in &update.changed {
        let alone = core.part(&query_of(change));
        if let Some(whole) = part_of(after, change) {
            assert_eq!(
                alone,
                whole,
                "{name}, at {}: {change:?} read alone differs from the whole",
                at + 1
            );
        }
        if *change == Change::Nav {
            for key in NAV {
                assert_eq!(
                    alone.get(key).unwrap_or(&Value::Null),
                    &after[key],
                    "{name}, at {}: nav.{key}",
                    at + 1
                );
            }
        }
    }
}

fn names() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir())
        .expect("the transcripts are there")
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            (path.extension()? == "json").then(|| path.file_stem()?.to_str().map(str::to_owned))?
        })
        .collect();
    names.sort();
    names
}

fn replay(name: &str, write: bool, schema: &schema::Schema) -> Result<(), String> {
    let path = dir().join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).map_err(|error| format!("{name}: {error}"))?;
    let mut transcript: Transcript =
        serde_json::from_str(&text).map_err(|error| format!("{name}: {error}"))?;
    let config = config(&transcript.config);
    schema
        .check("Config", &serde_json::to_value(&config).unwrap())
        .map_err(|error| format!("{name}: config: {error}"))?;
    let mut core = Core::new(config.clone());
    let mut failures = Vec::new();
    let mut kept = Value::Null;
    for (at, step) in transcript.steps.iter_mut().enumerate() {
        if step.restart {
            // The system ended the app. It starts again from what was stored.
            core = Core::new(config.clone());
        }
        if step.last {
            match &mut step.send {
                Event::Restore { snapshot } if write => *snapshot = kept.clone(),
                Event::Restore { snapshot } if *snapshot != kept => failures.push(format!(
                    "{name}, at {}: the snapshot restored is not the one taken last",
                    at + 1
                )),
                Event::Restore { .. } => {}
                other => {
                    return Err(format!(
                        "{name}, at {}: `last` is for a `restore`, not {other:?}",
                        at + 1
                    ));
                }
            }
        }
        let mut event = step.send.clone();
        if let Some(body) = &step.body {
            let text = std::fs::read_to_string(dir().join("fixtures").join(body))
                .map_err(|error| format!("{name}: {body}: {error}"))?;
            match &mut event {
                Event::Response(response) => response.body = text,
                Event::Cache { value, .. } => *value = Some(text),
                other => {
                    return Err(format!(
                        "{name}, at {}: `body` is for a response or a cache event, not {other:?}",
                        at + 1
                    ));
                }
            }
        }
        schema
            .check("Event", &serde_json::to_value(&step.send).unwrap())
            .map_err(|error| format!("{name}, at {}: event: {error}", at + 1))?;
        let before = serde_json::to_value(core.view()).unwrap();
        let update = core.send(event);
        let after = serde_json::to_value(core.view()).unwrap();
        schema
            .check("Update", &serde_json::to_value(&update).unwrap())
            .map_err(|error| format!("{name}, at {}: update: {error}", at + 1))?;
        schema
            .check("ViewState", &after)
            .map_err(|error| format!("{name}, at {}: view: {error}", at + 1))?;
        nothing_changes_unsaid(name, at, &core, &before, &after, &update);
        parts_read_alone(name, at, &core, &after, &update);
        // The update and the view state read back as what they were.
        let round: Update = serde_json::from_value(serde_json::to_value(&update).unwrap())
            .map_err(|error| format!("{name}: {error}"))?;
        assert_eq!(
            round,
            update,
            "{name}, at {}: the update does not read back",
            at + 1
        );
        for check in &step.check {
            let held = after.pointer(&check.at).cloned().unwrap_or(Value::Null);
            if held != check.is {
                failures.push(format!(
                    "{name}, at {}: `{}` is {held}, not {}",
                    at + 1,
                    check.at,
                    check.is
                ));
            }
        }
        if step.snapshot.is_some() {
            kept = core.part(&Query::Snapshot);
            schema
                .check("Snapshot", &kept)
                .map_err(|error| format!("{name}, at {}: snapshot: {error}", at + 1))?;
            // It reads back as what the core gave.
            let read: navree_native_core::Snapshot =
                serde_json::from_value(kept.clone()).map_err(|error| format!("{name}: {error}"))?;
            assert_eq!(
                serde_json::to_value(&read).unwrap(),
                kept,
                "{name}, at {}: the snapshot does not read back",
                at + 1
            );
            if write {
                step.snapshot = Some(kept.clone());
            } else if step.snapshot.as_ref() != Some(&kept) {
                failures.push(format!(
                    "{name}, at {}: the snapshot differs\n  expected {}\n  got      {}",
                    at + 1,
                    compact(&step.snapshot),
                    compact(&kept)
                ));
            }
        }
        if write {
            step.expect = Some(update);
        } else if step.expect.as_ref() != Some(&update) {
            failures.push(format!(
                "{name}, at {}: the update differs\n  expected {}\n  got      {}",
                at + 1,
                step.expect
                    .as_ref()
                    .map_or_else(|| "nothing: write it".to_owned(), compact),
                compact(&update)
            ));
        }
    }
    if write {
        std::fs::write(&path, written(&transcript)).map_err(|error| format!("{name}: {error}"))?;
    } else if written(&transcript) != text.replace("\r\n", "\n") {
        failures.push(format!(
            "{name}: the file is not as the tool writes it; write it again"
        ));
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n"))
    }
}

/// The conversations that must be written down. One that goes missing fails here.
const REQUIRED: &[&str] = &[
    "start",
    "pages",
    "link-presentations",
    "route-rules",
    "back",
    "refresh",
    "form-errors",
    "form-success",
    "form-redirect",
    "form-get",
    "double-tap",
    "ops",
    "duplicate-id",
    "missing-target",
    "commands",
    "regions",
    "region-live",
    "list-cursor",
    "table-query",
    "web-answer",
    "upgrade",
    "unknown-kind",
    "session-ended",
    "sign-in-session",
    "network-error",
    "stale-answer",
    "deep-link",
    "notification",
    "width-change",
    "sign-out",
    "cache",
    "deep-tree",
    "device-reply",
    "form-file",
    "redirects",
    "page-moved",
    "post-landing",
    "device-call",
    "live-list",
    "table-rows",
    "question",
    "reorder",
    "field-changed",
    "date-range",
    "searched-choice",
    "menu-checks",
    "menu-post",
    "present-task",
    "platform-sign-in",
    "hand-over",
    "second-step",
    "push",
    "signed-out",
    "signed-out-no-token",
    "table-words",
    "phone-country",
    "tabs",
    "snapshot",
    "snapshot-refused",
    "health-charts",
    "bar-pager",
    "composed-ops",
    "opened-from",
    "push-destination",
    "addresses",
    "sign-in-declined",
    "keep",
    "keep-owner",
    "keep-answers",
    "keep-store",
    "keep-reads",
    "keep-cache-control",
    "keep-person",
    "keep-sign-in",
    "keep-who",
    "keep-session",
    "keep-ids",
];

#[test]
fn every_transcript_replays() {
    let write = std::env::var("NAVREE_CORE_TRANSCRIPTS").is_ok_and(|mode| mode == "write");
    let schema = schema::Schema::new(navree_native_core::schema::schema());
    let names = names();
    let failures: Vec<String> = names
        .iter()
        .filter_map(|name| replay(name, write, &schema).err())
        .collect();
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
    let have: BTreeSet<&str> = names.iter().map(String::as_str).collect();
    let missing: Vec<&&str> = REQUIRED
        .iter()
        .filter(|name| !have.contains(**name))
        .collect();
    assert!(
        missing.is_empty(),
        "these conversations are not written down: {missing:?}"
    );
}

/// The names of the forms a tagged definition of the schema has.
fn forms_of(schema: &Value, name: &str) -> BTreeSet<String> {
    schema["$defs"][name]["oneOf"]
        .as_array()
        .expect("a tagged definition")
        .iter()
        .filter_map(|form| form["properties"]["t"]["const"].as_str().map(str::to_owned))
        .collect()
}

/// Every event a client can send and every effect the core can ask for is in
/// at least one transcript: a client's author can see each one at work.
#[test]
fn every_event_and_effect_is_in_a_transcript() {
    let schema = navree_native_core::schema::schema();
    let (mut events, mut effects) = (BTreeSet::new(), BTreeSet::new());
    for name in names() {
        let text = std::fs::read_to_string(dir().join(format!("{name}.json"))).unwrap();
        let transcript: Value = serde_json::from_str(&text).unwrap();
        for step in transcript["steps"].as_array().unwrap() {
            events.extend(step["send"]["t"].as_str().map(str::to_owned));
            for effect in step["expect"]["effects"].as_array().into_iter().flatten() {
                effects.extend(effect["t"].as_str().map(str::to_owned));
            }
        }
    }
    let unseen: Vec<_> = forms_of(&schema, "Event")
        .difference(&events)
        .cloned()
        .collect();
    assert!(
        unseen.is_empty(),
        "no transcript sends these events: {unseen:?}"
    );
    let unseen: Vec<_> = forms_of(&schema, "Effect")
        .difference(&effects)
        .cloned()
        .collect();
    assert!(
        unseen.is_empty(),
        "no transcript asks for these effects: {unseen:?}"
    );
}

/// The schema file is what the crate writes now.
#[test]
fn the_schema_file_is_current() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("schema/navree-native-core.schema.json");
    let text = format!(
        "{}\n",
        serde_json::to_string_pretty(&navree_native_core::schema::schema()).unwrap()
    );
    if std::env::var("NAVREE_CORE_TRANSCRIPTS").is_ok_and(|mode| mode == "write") {
        std::fs::write(&path, &text).unwrap();
    }
    let held = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    assert_eq!(held, text, "write it again: NAVREE_CORE_TRANSCRIPTS=write");
}

/// A small reader of the JSON Schema the crate exports: enough of the standard
/// for what that schema uses.
mod schema {
    use serde_json::Value;

    pub struct Schema {
        root: Value,
    }

    impl Schema {
        pub fn new(root: Value) -> Self {
            Self { root }
        }

        /// Holds `value` to the definition `name`.
        pub fn check(&self, name: &str, value: &Value) -> Result<(), String> {
            if self.root.is_null() {
                return Ok(());
            }
            let schema = self
                .root
                .pointer(&format!("/$defs/{name}"))
                .ok_or_else(|| format!("the schema has no `{name}`"))?;
            self.fits(schema, value, name)
        }

        fn fits(&self, schema: &Value, value: &Value, at: &str) -> Result<(), String> {
            if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
                let target = self
                    .root
                    .pointer(reference.trim_start_matches('#'))
                    .ok_or_else(|| format!("{at}: nothing at {reference}"))?;
                return self.fits(target, value, at);
            }
            if let Some(options) = schema.get("oneOf").and_then(Value::as_array) {
                let fitting = options
                    .iter()
                    .filter(|option| self.fits(option, value, at).is_ok())
                    .count();
                if fitting != 1 {
                    let why: Vec<String> = options
                        .iter()
                        .filter_map(|option| self.fits(option, value, at).err())
                        .collect();
                    return Err(format!(
                        "{at}: fits {fitting} of {} forms, not one: {value} ({})",
                        options.len(),
                        why.join("; ")
                    ));
                }
                return Ok(());
            }
            if let Some(constant) = schema.get("const") {
                return if constant == value {
                    Ok(())
                } else {
                    Err(format!("{at}: {value} is not {constant}"))
                };
            }
            if let Some(allowed) = schema.get("enum").and_then(Value::as_array) {
                return if allowed.contains(value) {
                    Ok(())
                } else {
                    Err(format!("{at}: {value} is none of {allowed:?}"))
                };
            }
            let kinds: Vec<&str> = match schema.get("type") {
                Some(Value::String(kind)) => vec![kind.as_str()],
                Some(Value::Array(kinds)) => kinds.iter().filter_map(Value::as_str).collect(),
                _ => Vec::new(),
            };
            let kind = match value {
                Value::Null => "null",
                Value::Bool(_) => "boolean",
                Value::Number(number) if number.is_u64() || number.is_i64() => "integer",
                Value::Number(_) => "number",
                Value::String(_) => "string",
                Value::Array(_) => "array",
                Value::Object(_) => "object",
            };
            if !kinds.is_empty()
                && !kinds.contains(&kind)
                && !(kind == "integer" && kinds.contains(&"number"))
            {
                return Err(format!("{at}: {value} is not {kinds:?}"));
            }
            if let (Some(items), Some(list)) = (schema.get("items"), value.as_array()) {
                for (index, item) in list.iter().enumerate() {
                    self.fits(items, item, &format!("{at}[{index}]"))?;
                }
            }
            if let Some(fields) = value.as_object() {
                let known = schema.get("properties").and_then(Value::as_object);
                for required in schema
                    .get("required")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                {
                    if !fields.contains_key(required) {
                        return Err(format!("{at}: `{required}` is missing"));
                    }
                }
                for (key, field) in fields {
                    match (
                        known.and_then(|known| known.get(key)),
                        schema.get("additionalProperties"),
                    ) {
                        (Some(rule), _) => self.fits(rule, field, &format!("{at}.{key}"))?,
                        (None, Some(Value::Bool(false))) => {
                            return Err(format!("{at}: `{key}` is not a field of it"));
                        }
                        (None, Some(rule @ Value::Object(_))) => {
                            self.fits(rule, field, &format!("{at}.{key}"))?
                        }
                        _ => {}
                    }
                }
            }
            Ok(())
        }
    }
}
