//! `schema/navree-native.schema.json`: written from the types' descriptions,
//! held to what they describe now, and every example envelope held to it.
//!
//! `NAVREE_NATIVE_CORPUS=write cargo test -p navree-native` writes the schema
//! again, with the corpus, after a change that is meant.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde_json::{Map, Value};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn schema() -> Value {
    navree_native::schema::document()
}

#[test]
fn the_schema_file_is_what_the_types_describe() {
    let path = root().join("schema").join("navree-native.schema.json");
    let mut written = serde_json::to_string_pretty(&schema()).unwrap();
    written.push('\n');
    let asked = std::env::var("NAVREE_NATIVE_CORPUS").is_ok_and(|mode| mode == "write");
    if asked || !path.exists() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &written).unwrap();
    }
    let on_disk = std::fs::read_to_string(&path)
        .unwrap()
        .replace("\r\n", "\n");
    assert!(
        on_disk == written,
        "schema/navree-native.schema.json is not what the types describe now; if the change is \
         meant, write it again with NAVREE_NATIVE_CORPUS=write"
    );
}

/// What is wrong with `value` as the schema's `shape`, each with where.
struct Problems<'a> {
    defs: &'a Map<String, Value>,
    found: Vec<String>,
}

impl Problems<'_> {
    fn check(&mut self, shape: &Value, value: &Value, at: &str) {
        if let Some(path) = shape.get("$ref").and_then(Value::as_str) {
            let name = path.trim_start_matches("#/$defs/");
            let def = &self.defs[name];
            match def["x-kind"].as_str() {
                Some("object") => self.object(name, def, value, at, None),
                Some("enum") => {
                    let known = def["enum"].as_array().unwrap();
                    if !known.contains(value) {
                        self.found.push(format!("{at}: {value} is not a {name}"));
                    }
                }
                Some("union") => self.union(name, def, value, at),
                Some("string") => {
                    if !value.is_string() {
                        self.found.push(format!("{at}: {value} is not a {name}"));
                    }
                }
                kind => self.found.push(format!("{at}: {name} is of no kind: {kind:?}")),
            }
            return;
        }
        if let Some(Value::Array(any)) = shape.get("anyOf") {
            if !value.is_null() {
                self.check(&any[0], value, at);
            }
            return;
        }
        if shape.get("x-json").is_some() {
            return;
        }
        let fits = match shape["type"].as_str() {
            Some("string") => value.is_string(),
            Some("boolean") => value.is_boolean(),
            Some("integer") => value.is_i64() || value.is_u64(),
            Some("number") => value.is_number(),
            Some("array") => {
                for (index, item) in value.as_array().into_iter().flatten().enumerate() {
                    self.check(&shape["items"], item, &format!("{at}[{index}]"));
                }
                value.is_array()
            }
            Some("object") => {
                for (key, item) in value.as_object().into_iter().flatten() {
                    self.check(&shape["additionalProperties"], item, &format!("{at}.{key}"));
                }
                value.is_object()
            }
            _ => false,
        };
        if !fits {
            self.found.push(format!("{at}: {value} is not {shape}"));
        }
    }

    fn object(&mut self, name: &str, def: &Value, value: &Value, at: &str, tag: Option<&str>) {
        let Some(fields) = value.as_object() else {
            self.found.push(format!("{at}: {value} is not a {name}"));
            return;
        };
        let properties = def["properties"].as_object().unwrap();
        for (key, item) in fields {
            if Some(key.as_str()) == tag {
                continue;
            }
            match properties.get(key) {
                Some(property) => self.check(property, item, &format!("{at}.{key}")),
                None => self
                    .found
                    .push(format!("{at}: {name} has no field `{key}` in the schema")),
            }
        }
        for required in def["required"].as_array().unwrap() {
            let required = required.as_str().unwrap();
            if !fields.contains_key(required) {
                self.found.push(format!("{at}: {name} needs `{required}`"));
            }
        }
    }

    fn union(&mut self, name: &str, def: &Value, value: &Value, at: &str) {
        let tag = def["x-tag"].as_str().unwrap();
        let Some(label) = value.get(tag).and_then(Value::as_str) else {
            self.found.push(format!("{at}: {name} names no `{tag}`"));
            return;
        };
        let variant = def["x-variants"]
            .as_array()
            .unwrap()
            .iter()
            .find(|variant| variant["tag"] == label);
        let Some(variant) = variant else {
            self.found
                .push(format!("{at}: `{label}` is not a variant of {name}"));
            return;
        };
        match variant["type"].as_str() {
            Some(payload) => {
                let def = &self.defs[payload];
                self.object(payload, def, value, &format!("{at}<{label}>"), Some(tag));
            }
            None => {
                if value.as_object().is_some_and(|fields| fields.len() > 1) {
                    self.found
                        .push(format!("{at}: the variant `{label}` holds nothing, yet has fields"));
                }
            }
        }
    }
}

#[test]
fn every_example_holds_only_what_the_schema_describes() {
    let schema = schema();
    let defs = schema["$defs"].as_object().unwrap();
    let mut checked = BTreeSet::new();
    for entry in std::fs::read_dir(root().join("corpus")).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        // Written by hand to hold what no build knows.
        if name == "from-the-future.json" {
            continue;
        }
        let value: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let root = if name == "shell.json" { "Shell" } else { "Envelope" };
        let mut problems = Problems {
            defs,
            found: Vec::new(),
        };
        problems.check(
            &serde_json::json!({ "$ref": format!("#/$defs/{root}") }),
            &value,
            &name,
        );
        assert!(
            problems.found.is_empty(),
            "{}",
            problems.found.join("\n")
        );
        checked.insert(name);
    }
    assert!(checked.len() > 10, "{checked:?}");
}

#[test]
fn an_example_with_a_field_the_schema_lacks_is_caught() {
    let schema = schema();
    let mut problems = Problems {
        defs: schema["$defs"].as_object().unwrap(),
        found: Vec::new(),
    };
    let value = serde_json::json!({
        "navree": 1, "build": "b", "vocab": 0,
        "screen": {"title": "x", "body": {"t": "badge", "text": "Paid", "glow": 3}},
        "ops": [{"op": "toast"}, {"op": "teleport"}],
    });
    problems.check(&serde_json::json!({"$ref": "#/$defs/Envelope"}), &value, "e");
    assert_eq!(
        problems.found,
        [
            "e.ops[0]<toast>: Toast needs `text`",
            "e.ops[1]: `teleport` is not a variant of Op",
            "e.screen.body<badge>: Badge has no field `glow` in the schema",
        ]
    );
}
