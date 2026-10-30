//! Properties of the loader: arbitrary files and environment overlays never
//! panic, the environment prefix holds, and merging is associative in the
//! documented way (layers that agree on which paths are tables).

use std::collections::{BTreeMap, BTreeSet};

use navree_config::{Config, ConfigSources};
use proptest::collection::{btree_map, vec};
use proptest::prelude::*;
use serde::Deserialize;
use serde_json::{Map, Value};

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

/// A key without a double underscore, so it can be an environment segment.
fn key() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9]{0,5}(_[a-z0-9]{1,3})?"
}

fn scalar() -> impl Strategy<Value = Value> {
    prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::from),
        any::<i64>().prop_map(Value::from),
        // Quarters are exact in binary, so every parser reads them back as written.
        (-4_000_000i64..4_000_000).prop_map(|n| Value::from(n as f64 / 4.0)),
        "[ -~]{0,12}".prop_map(Value::from),
    ]
}

/// Any JSON value up to five levels deep.
fn any_json() -> impl Strategy<Value = Value> {
    scalar().prop_recursive(5, 48, 4, |inner| {
        prop_oneof![
            vec(inner.clone(), 0..4).prop_map(Value::Array),
            btree_map(key(), inner, 0..4).prop_map(|m| Value::Object(m.into_iter().collect())),
        ]
    })
}

/// A JSON object: a layer.
fn any_layer() -> impl Strategy<Value = Value> {
    btree_map(key(), any_json(), 0..5).prop_map(|m| Value::Object(m.into_iter().collect()))
}

/// A layer in which the name of a key fixes its kind: `t…` keys hold tables
/// and `v…` keys hold leaves (scalars, arrays and nulls), in every layer.
fn shaped_layer(depth: u32) -> BoxedStrategy<Value> {
    let leaves = btree_map(
        "v[a-d]",
        prop_oneof![scalar(), vec(scalar(), 0..3).prop_map(Value::Array)],
        0..4,
    );
    if depth == 0 {
        return leaves
            .prop_map(|m| Value::Object(m.into_iter().collect()))
            .boxed();
    }
    (leaves, btree_map("t[a-c]", shaped_layer(depth - 1), 0..3))
        .prop_map(|(leaves, tables)| {
            let mut object = Map::new();
            object.extend(leaves);
            object.extend(tables);
            Value::Object(object)
        })
        .boxed()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// The tree of layers stacked in order.
fn stack(layers: &[&Value]) -> Value {
    let mut sources = ConfigSources::new();
    for layer in layers {
        sources = sources.set("", (*layer).clone());
    }
    Config::load(&sources)
        .expect("layers load")
        .section::<Value>("")
        .expect("tree")
}

fn leaves(value: &Value, path: &mut Vec<String>, out: &mut Vec<(String, Value)>) {
    match value {
        Value::Object(map) if !map.is_empty() => {
            for (key, child) in map {
                path.push(key.clone());
                leaves(child, path, out);
                path.pop();
            }
        }
        _ => out.push((format!("/{}", path.join("/")), value.clone())),
    }
}

/// Loads `src` as a file called `name` and exercises everything on the tree.
fn load_file(name: &str, src: &[u8]) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(name);
    std::fs::write(&path, src).expect("write");
    if let Ok(config) = Config::load(&ConfigSources::new().file(&path)) {
        config
            .section::<Value>("")
            .expect("a loaded tree reads back");
        let _ = format!("{config:?}");
        let _ = config.redacted();
        let _ = config.get_string("a.b");
        let _ = config.get_bool("a");
    }
}

fn toml_soup() -> impl Strategy<Value = String> {
    vec(
        prop_oneof![
            Just("[".to_owned()),
            Just("]".to_owned()),
            Just("[[".to_owned()),
            Just("=".to_owned()),
            Just("\"".to_owned()),
            Just("\"\"\"".to_owned()),
            Just("'".to_owned()),
            Just(".".to_owned()),
            Just(",".to_owned()),
            Just("{".to_owned()),
            Just("}".to_owned()),
            Just("\n".to_owned()),
            Just("\r\n".to_owned()),
            Just("#".to_owned()),
            Just(" ".to_owned()),
            Just("true".to_owned()),
            Just("1979-05-27T07:32:00Z".to_owned()),
            Just("0x1F".to_owned()),
            Just("1e400".to_owned()),
            Just("nan".to_owned()),
            "[a-z]{1,4}",
            "[0-9]{1,4}",
            "\\PC{1,3}",
        ],
        0..60,
    )
    .prop_map(|parts| parts.concat())
}

fn json_soup() -> impl Strategy<Value = String> {
    vec(
        prop_oneof![
            Just("{".to_owned()),
            Just("}".to_owned()),
            Just("[".to_owned()),
            Just("]".to_owned()),
            Just(":".to_owned()),
            Just(",".to_owned()),
            Just("\"".to_owned()),
            Just("null".to_owned()),
            Just("true".to_owned()),
            Just("-".to_owned()),
            Just("1e999".to_owned()),
            Just("\\u0000".to_owned()),
            Just(" ".to_owned()),
            Just("\n".to_owned()),
            "[a-z]{1,4}",
            "[0-9]{1,6}",
            "\\PC{1,3}",
        ],
        0..60,
    )
    .prop_map(|parts| parts.concat())
}

// ---------------------------------------------------------------------------
// Files never panic
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(192))]

    #[test]
    fn arbitrary_text_as_toml_never_panics(src in any::<String>()) {
        load_file("app.toml", src.as_bytes());
    }

    #[test]
    fn toml_shaped_text_never_panics(src in toml_soup()) {
        load_file("app.toml", src.as_bytes());
    }

    #[test]
    fn arbitrary_text_as_json_never_panics(src in any::<String>()) {
        load_file("app.json", src.as_bytes());
    }

    #[test]
    fn json_shaped_text_never_panics(src in json_soup()) {
        load_file("app.json", src.as_bytes());
    }

    #[test]
    fn arbitrary_bytes_as_a_file_or_dotenv_never_panic(bytes in vec(any::<u8>(), 0..200)) {
        load_file("app.toml", &bytes);
        load_file("app.json", &bytes);
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(".env");
        std::fs::write(&path, &bytes).expect("write");
        let _ = Config::load(&ConfigSources::new().dotenv(&path));
    }

    #[test]
    fn dotenv_shaped_text_never_panics(parts in vec(prop_oneof![
        Just("APP_A=".to_owned()), Just("\"".to_owned()), Just("'".to_owned()), Just("\n".to_owned()),
        Just("${".to_owned()), Just("}".to_owned()), Just("export ".to_owned()), Just("#".to_owned()),
        Just("\\".to_owned()), Just("__".to_owned()), "[A-Za-z0-9_]{1,5}", "\\PC{1,3}",
    ], 0..40)) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(".env");
        std::fs::write(&path, parts.concat()).expect("write");
        if let Ok(config) = Config::load(&ConfigSources::new().dotenv(&path)) {
            config.section::<Value>("").expect("tree");
        }
    }

    /// What a JSON file says is what the tree holds.
    #[test]
    fn a_json_file_loads_to_the_object_it_holds(layer in any_layer()) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("app.json");
        std::fs::write(&path, serde_json::to_string_pretty(&layer).expect("json")).expect("write");
        let config = Config::load(&ConfigSources::new().file(&path)).expect("load");
        prop_assert_eq!(config.section::<Value>("").expect("tree"), layer);
    }

    /// The same object written as TOML (when TOML can say it) loads to it too.
    #[test]
    fn a_toml_file_loads_to_the_table_it_holds(layer in any_layer()) {
        let Ok(table) = toml::Value::try_from(&layer) else { return Ok(()); };
        let Ok(text) = toml::to_string(&table) else { return Ok(()); };
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("app.toml");
        std::fs::write(&path, text).expect("write");
        let config = Config::load(&ConfigSources::new().file(&path)).expect("load");
        prop_assert_eq!(config.section::<Value>("").expect("tree"), layer);
    }
}

// ---------------------------------------------------------------------------
// The environment
// ---------------------------------------------------------------------------

fn env_name() -> impl Strategy<Value = String> {
    prop_oneof![
        "(APP_|app_|App_|APP|AP|_|)[A-Za-z0-9_é.-]{0,20}",
        "APP_[A-Z]{1,3}(__[A-Z]{1,3}){0,5}",
        "[A-Za-z_]{0,12}",
    ]
}

fn under_prefix(name: &str) -> bool {
    name.get(..4)
        .is_some_and(|head| head.eq_ignore_ascii_case("APP_"))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Whatever the names, the load does not panic, and a variable outside
    /// the prefix leaves no trace in the tree.
    #[test]
    fn environment_overlays_never_panic_and_stay_inside_the_prefix(
        names in vec(env_name(), 0..12),
        noise in "[ -~]{0,8}",
    ) {
        let vars: Vec<(String, String)> = names
            .iter()
            .enumerate()
            .map(|(i, name)| (name.clone(), format!("<value-{i}:{noise}>")))
            .collect();
        let config = Config::load(&ConfigSources::new().env_vars("APP_", vars.clone())).expect("shallow names load");
        let tree = config.section::<Value>("").expect("tree").to_string();
        for (name, value) in &vars {
            if !under_prefix(name) {
                prop_assert!(!tree.contains(&value.replace('"', "\\\"")) , "{name} leaked");
            }
        }
        let _ = format!("{config:?}");
    }

    /// A variable lands at the key its name spells, whatever the order the
    /// variables are listed in.
    #[test]
    fn names_land_at_their_paths_in_any_order(
        paths in btree_map(vec(key(), 1..4), "[ -~]{0,10}", 1..8)
            .prop_filter("no path is the parent of another", |paths| {
                !paths.keys().any(|a| paths.keys().any(|b| a != b && b.starts_with(a)))
            }),
        seed in any::<u64>(),
    ) {
        let mut vars: Vec<(String, String)> = paths
            .iter()
            .map(|(path, value)| (format!("APP_{}", path.join("__").to_uppercase()), value.clone()))
            .collect();
        // A reproducible shuffle.
        let mut state = seed | 1;
        for i in (1..vars.len()).rev() {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            vars.swap(i, (state % (i as u64 + 1)) as usize);
        }
        let config = Config::load(&ConfigSources::new().env_vars("APP_", vars)).expect("load");
        for (path, value) in &paths {
            let got = config.get_string(&path.join("."));
            prop_assert_eq!(got.as_deref(), Some(value.as_str()));
        }
    }
}

// ---------------------------------------------------------------------------
// Merging
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Stacking is a left fold: merging the first two layers first changes nothing.
    #[test]
    fn stacking_is_a_left_fold(a in any_layer(), b in any_layer(), c in any_layer()) {
        let ab = stack(&[&a, &b]);
        prop_assert_eq!(stack(&[&a, &b, &c]), stack(&[&ab, &c]));
    }

    /// Associative when the layers agree on which paths are tables.
    #[test]
    fn merging_is_associative_for_layers_that_agree_on_tables(
        a in shaped_layer(3), b in shaped_layer(3), c in shaped_layer(3),
    ) {
        let bc = stack(&[&b, &c]);
        prop_assert_eq!(stack(&[&a, &b, &c]), stack(&[&a, &bc]));
    }

    #[test]
    fn the_empty_layer_changes_nothing(a in any_layer()) {
        let empty = Value::Object(Map::new());
        prop_assert_eq!(stack(&[&a]), stack(&[&empty, &a]));
        prop_assert_eq!(stack(&[&a]), stack(&[&a, &empty]));
    }

    #[test]
    fn stacking_a_layer_twice_is_stacking_it_once(a in any_layer()) {
        prop_assert_eq!(stack(&[&a]), stack(&[&a, &a]));
    }

    /// Every leaf of the last layer is in the result as it was written.
    #[test]
    fn the_last_layer_wins_on_every_leaf(a in any_layer(), b in any_layer()) {
        let merged = stack(&[&a, &b]);
        let mut found = Vec::new();
        leaves(&b, &mut Vec::new(), &mut found);
        for (pointer, value) in found {
            if pointer == "/" {
                continue; // an empty layer has no leaves
            }
            // An empty table adds no keys, so the earlier layers' keys under
            // it stay; the table itself is there.
            if value.as_object().is_some_and(Map::is_empty) {
                prop_assert!(merged.pointer(&pointer).is_some_and(Value::is_object), "at {}", pointer);
                continue;
            }
            prop_assert_eq!(merged.pointer(&pointer), Some(&value), "at {}", pointer);
        }
    }

    /// Stacking only adds: every key of every layer is still reachable.
    #[test]
    fn no_key_is_lost(a in shaped_layer(3), b in shaped_layer(3)) {
        let merged = stack(&[&a, &b]);
        let mut found = Vec::new();
        leaves(&a, &mut Vec::new(), &mut found);
        for (pointer, _) in found {
            if pointer != "/" {
                prop_assert!(merged.pointer(&pointer).is_some(), "{} lost", pointer);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Reading sections
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
enum Shape {
    Circle(u32),
    Pair(u32, u32),
    Rect { w: u32, h: u32 },
    Dot,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Everything {
    a: Option<Shape>,
    b: Vec<Shape>,
    c: (u8, String),
    d: BTreeMap<String, u16>,
    e: Option<f64>,
    f: char,
    g: bool,
    h: u128,
}

/// Reads `layer` as every kind of target; none of them may panic.
fn read_everything(layer: &Value) {
    let config = Config::load(&ConfigSources::new().set("", layer.clone())).expect("load");
    let _ = config.section::<Shape>("");
    let _ = config.section::<Everything>("");
    let _ = config.section::<Vec<Shape>>("a");
    let _ = config.section::<BTreeMap<String, Shape>>("");
    let _ = config.section::<BTreeMap<u16, i8>>("a");
    let _ = config.section::<(Shape, Shape)>("b");
    let _ = config.section::<Option<Shape>>("a.b");
    let _ = config.section::<String>("a");
    let _ = config.section::<f32>("c");
    let _ = config.section::<()>("d");
    let _ = config.section_or_default::<BTreeSet<String>>("z");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// The old deserializer panicked on a bare name for a variant with
    /// content; ours reads any tree as any type without panicking.
    #[test]
    fn reading_any_tree_as_any_type_never_panics(layer in any_layer()) {
        read_everything(&layer);
    }

    #[test]
    fn reading_trees_built_from_variant_names_never_panics(
        shape in prop_oneof![
            Just(Value::from("Circle")), Just(Value::from("Pair")), Just(Value::from("Rect")),
            Just(Value::from("Dot")), Just(Value::from("Nope")),
            btree_map(prop_oneof![Just("Circle"), Just("Pair"), Just("Rect"), Just("Dot"), Just("Nope")], any_json(), 0..3)
                .prop_map(|m| Value::Object(m.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())),
        ],
    ) {
        let mut object = Map::new();
        object.insert("a".to_owned(), shape.clone());
        object.insert("b".to_owned(), Value::Array(vec![shape.clone(), shape]));
        read_everything(&Value::Object(object));
    }
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Wants<T> {
    x: T,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
enum Level {
    Debug,
    Info,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Inner {
    n: u8,
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// A string that cannot be converted is never quoted in the error.
    #[test]
    fn errors_never_contain_a_string_value(marker in "Zq[A-Za-z0-9]{12}") {
        let config = Config::load(&ConfigSources::new().set("", serde_json::json!({
            "x": marker,
            "nested": { "x": marker, "list": [marker, marker] },
        }))).expect("load");
        let messages = [
            config.section::<Wants<u8>>("").expect_err("u8").to_string(),
            config.section::<Wants<bool>>("").expect_err("bool").to_string(),
            config.section::<Wants<f64>>("").expect_err("f64").to_string(),
            config.section::<Wants<Vec<u8>>>("").expect_err("vec").to_string(),
            config.section::<Wants<char>>("").expect_err("char").to_string(),
            config.section::<Wants<Level>>("").expect_err("enum").to_string(),
            config.section::<Wants<Shape>>("").expect_err("shape").to_string(),
            config.section::<Wants<Option<u16>>>("").expect_err("option").to_string(),
            config.section::<Wants<Inner>>("").expect_err("struct").to_string(),
            config.section::<Wants<(u8, u8)>>("").expect_err("tuple").to_string(),
            config.section::<Wants<Vec<u8>>>("nested").expect_err("nested").to_string(),
            config.section::<Wants<BTreeMap<String, u8>>>("nested").expect_err("map").to_string(),
            config.section::<Level>("x").expect_err("root enum").to_string(),
            config.section::<u8>("x").expect_err("root u8").to_string(),
        ];
        for message in messages {
            prop_assert!(!message.contains(&marker), "{}", message);
        }
    }

    /// Under a secret-looking key, nothing about the value is printed.
    #[test]
    fn secret_values_are_in_no_debug_or_error_output(marker in "Zq[A-Za-z0-9]{12}") {
        let layer = serde_json::json!({
            "auth": { "api_key": marker, "client_secret": marker, "nested": { "password": marker } },
            "db": { "url": format!("postgres://u:{marker}@h/d?token={marker}") },
        });
        let config = Config::load(&ConfigSources::new().set("", layer)).expect("load");
        let debug = format!("{config:?}");
        let redacted = config.redacted().to_string();
        prop_assert!(!debug.contains(&marker));
        prop_assert!(!redacted.contains(&marker));
        let error = config.section::<Wants<u8>>("auth").expect_err("missing field").to_string();
        prop_assert!(!error.contains(&marker));
        let error = config.section::<Wants<Level>>("db").expect_err("missing field").to_string();
        prop_assert!(!error.contains(&marker));
    }
}
