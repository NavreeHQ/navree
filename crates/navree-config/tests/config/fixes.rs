//! Bugs of the loader that wrapped the `config` crate, fixed in ours. Each
//! test here failed (or panicked) on the old loader; the rest of the suite
//! passes on both.

use std::collections::HashMap;

use navree_config::{Config, ConfigSources};
use serde::Deserialize;
use serde_json::json;

fn config(layer: serde_json::Value) -> Config {
    Config::load(&ConfigSources::new().set("", layer)).expect("load")
}

fn message<T: serde::de::DeserializeOwned + std::fmt::Debug>(config: &Config) -> String {
    config.section::<T>("").expect_err("rejected").to_string()
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
enum Shape {
    Circle(u32),
    Pair(u32, u32),
    Rect { w: u32 },
    Dot,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Drawing {
    shape: Shape,
}

#[test]
fn a_bare_name_for_a_variant_with_content_is_an_error_not_a_panic() {
    for variant in ["Circle", "Pair", "Rect"] {
        let text = message::<Drawing>(&config(json!({ "shape": variant })));
        assert!(
            text.contains("invalid type: unit variant, expected")
                && text.ends_with("for key `shape`"),
            "{variant}: {text}"
        );
    }
    // The same names with their content, and a unit variant, are fine.
    let ok = |shape| {
        config(json!({ "shape": shape }))
            .section::<Drawing>("")
            .map(|d| d.shape)
            .expect("valid")
    };
    assert!(matches!(ok(json!("Dot")), Shape::Dot));
    assert!(matches!(ok(json!({ "Circle": 3 })), Shape::Circle(3)));
    assert!(matches!(ok(json!({ "Pair": [1, 2] })), Shape::Pair(1, 2)));
    assert!(matches!(
        ok(json!({ "Rect": { "w": 4 } })),
        Shape::Rect { w: 4 }
    ));
}

#[test]
fn keys_inside_lists_of_tables_have_a_dot_before_the_field() {
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Server {
        port: u16,
    }
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Root {
        servers: Vec<Server>,
    }
    let wrong_type = config(json!({ "servers": [{ "port": 1 }, { "port": "x" }] }));
    assert_eq!(
        message::<Root>(&wrong_type),
        "configuration section `` is invalid: invalid type: string [redacted], expected an integer for key `servers[1].port` in ConfigSources::set"
    );
    let missing = config(json!({ "servers": [{ "port": 1 }, {}] }));
    assert_eq!(
        message::<Root>(&missing),
        "configuration section `` is invalid: missing configuration field \"servers[1].port\""
    );
}

#[test]
fn a_float_with_a_fraction_does_not_silently_round_into_an_integer() {
    #[derive(Debug, Deserialize)]
    struct Timeout {
        secs: u32,
    }
    assert_eq!(
        message::<Timeout>(&config(json!({ "secs": 2.5 }))),
        "configuration section `` is invalid: invalid type: floating point `2.5`, expected an integer for key `secs` in ConfigSources::set"
    );
    for bad in [json!(0.5), json!(-0.5), json!(1e30)] {
        assert!(
            config(json!({ "secs": bad }))
                .section::<Timeout>("")
                .is_err()
        );
    }
    // A float that is a whole number is fine, from TOML or JSON.
    let whole = config(json!({ "secs": 30.0 }));
    assert_eq!(whole.section::<Timeout>("").expect("whole").secs, 30);
}

#[test]
fn an_out_of_range_integer_names_where_it_came_from() {
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Port {
        port: u16,
    }
    assert_eq!(
        message::<Port>(&config(json!({ "port": 70000 }))),
        "configuration section `` is invalid: invalid type: 64-bit integer `70000`, expected an unsigned 16 bit integer for key `port` in ConfigSources::set"
    );
    // An environment string that does not fit is not quoted as a number.
    let env = Config::load(&ConfigSources::new().env_vars("APP_", [("APP_PORT", "70000")]))
        .expect("load");
    assert_eq!(
        message::<Port>(&env),
        "configuration section `` is invalid: invalid type: string [redacted], expected an unsigned 16 bit integer for key `port` in environment"
    );
}

#[test]
fn signed_range_errors_read_a_signed_not_an_signed() {
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Small {
        n: i8,
    }
    let text = message::<Small>(&config(json!({ "n": 200 })));
    assert!(
        text.contains("expected a signed 8 bit integer for key `n`"),
        "{text}"
    );
}

#[test]
fn an_unknown_variant_is_not_echoed_because_it_is_a_value() {
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    enum Level {
        Debug,
        Info,
    }
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct Log {
        level: Level,
    }
    let text = message::<Log>(&config(json!({ "level": "hunter2" })));
    assert!(!text.contains("hunter2"), "{text}");
    assert_eq!(
        text,
        "configuration section `` is invalid: enum Level has no variant matching this value, expected one of `Debug`, `Info` for key `level`"
    );
}

#[test]
fn integers_wider_than_64_bits_are_supported() {
    #[derive(Debug, Deserialize)]
    struct Wide {
        id: u128,
        offset: i128,
    }
    let wide = config(json!({ "id": "340282366920938463463374607431768211455", "offset": "-5" }))
        .section::<Wide>("")
        .expect("wide");
    assert_eq!((wide.id, wide.offset), (u128::MAX, -5));
}

#[test]
fn the_first_unknown_field_reported_is_the_same_on_every_run() {
    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
    struct Strict {
        a: u32,
    }
    let layer = json!({ "a": 1, "z": 1, "m": 1, "b": 1, "y": 1 });
    for _ in 0..20 {
        assert_eq!(
            message::<Strict>(&config(layer.clone())),
            "configuration section `` is invalid: unknown field `b`, expected `a`"
        );
    }
}

#[test]
fn tables_are_ordered_by_key_in_every_view() {
    let config = config(json!({ "b": 1, "a": 2, "c": { "z": 1, "y": 2 } }));
    let keys: Vec<String> = config
        .section::<HashMap<String, serde_json::Value>>("")
        .expect("map")
        .into_keys()
        .collect();
    assert_eq!(keys.len(), 3);
    assert_eq!(
        config.redacted().to_string(),
        r#"{"a":2,"b":1,"c":{"y":2,"z":1}}"#
    );
}
