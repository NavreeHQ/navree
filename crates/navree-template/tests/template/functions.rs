//! Rust functions, filters, tests and globals.

use navree_template::{Environment, Error, ErrorKind, Kwargs, Value};
use serde_json::json;

use crate::support::view_env;

#[test]
fn closures_take_converted_arguments() {
    let mut env = Environment::new();
    env.add_filter("shout", |text: &str| text.to_uppercase());
    env.add_filter("plus", |a: i64, b: i64| a + b);
    env.add_filter("half", |a: f64| a / 2.0);
    env.add_filter("flag", |a: bool| !a);
    env.add_filter("owned", |a: String| format!("{a}!"));
    env.add_filter("opt", |a: Value, b: Option<&str>| {
        format!("{a}-{}", b.unwrap_or("none"))
    });
    env.add_filter("items2", |a: Vec<i64>| a.iter().sum::<i64>());
    env.add_function("zero", || 0);
    env.add_function("five", |a: i64, b: i64, c: i64, d: i64, e: i64| {
        a + b + c + d + e
    });
    env.add_test("big", |a: i64| a > 10);
    env.add_global("brand", "Agra");
    env.add_global("count", 3);
    for (source, expected) in [
        ("{{ 'a'|shout }}", "A"),
        ("{{ 1|plus(2) }}", "3"),
        ("{{ 3|half }}", "1.5"),
        ("{{ true|flag }}", "False"),
        ("{{ 1|owned }}", "1!"),
        ("{{ 1|opt }}", "1-none"),
        ("{{ 1|opt('x') }}", "1-x"),
        ("{{ 1|opt(none) }}", "1-none"),
        ("{{ [1, 2]|items2 }}", "3"),
        ("{{ zero() }}", "0"),
        ("{{ five(1, 2, 3, 4, 5) }}", "15"),
        ("{{ 11 is big }}{{ 9 is big }}", "TrueFalse"),
        ("{{ brand }}{{ count }}", "Agra3"),
    ] {
        assert_eq!(
            env.render_str("t.txt", source, json!({})).expect(source),
            expected,
            "{source}"
        );
    }
    // The context shadows a global.
    assert_eq!(
        env.render_str("t.txt", "{{ brand }}", json!({ "brand": "Mine" }))
            .expect("renders"),
        "Mine"
    );
}

#[test]
fn wrong_arguments_are_errors() {
    let mut env = Environment::new();
    env.add_filter("shout", |text: &str| text.to_uppercase());
    env.add_filter("plus", |a: i64, b: i64| a + b);
    for (source, kind) in [
        ("{{ 1|shout }}", ErrorKind::InvalidOperation),
        ("{{ 1|plus }}", ErrorKind::MissingArgument),
        ("{{ 1|plus(2, 3) }}", ErrorKind::TooManyArguments),
        ("{{ 1|plus('a') }}", ErrorKind::InvalidOperation),
        ("{{ 'a'|shout(b=1) }}", ErrorKind::TooManyArguments),
    ] {
        let error = env
            .render_str("t.txt", source, json!({}))
            .expect_err(source);
        assert_eq!(error.kind(), kind, "{source}: {error}");
    }
}

#[test]
fn keyword_arguments() {
    let mut env = Environment::new();
    env.add_function("tr", |id: &str, kwargs: Kwargs| -> Result<String, Error> {
        let mut out = id.to_owned();
        for name in kwargs.args() {
            let value: Value = kwargs.get(name)?;
            out.push_str(&format!(" {name}={value}"));
        }
        kwargs.assert_all_used()?;
        Ok(out)
    });
    env.add_function("strict", |kwargs: Kwargs| -> Result<i64, Error> {
        let a: i64 = kwargs.get("a")?;
        let b: Option<i64> = kwargs.get("b")?;
        kwargs.assert_all_used()?;
        Ok(a + b.unwrap_or(0))
    });
    assert_eq!(
        env.render_str("t.txt", "{{ tr('x', b=2, a='y') }}", json!({}))
            .expect("renders"),
        "x a=y b=2"
    );
    assert_eq!(
        env.render_str(
            "t.txt",
            "{{ strict(a=1) }}{{ strict(a=1, b=2) }}",
            json!({})
        )
        .expect("renders"),
        "13"
    );
    let error = env
        .render_str("t.txt", "{{ strict(a=1, c=2) }}", json!({}))
        .expect_err("unknown");
    assert_eq!(error.kind(), ErrorKind::TooManyArguments);
    assert!(error.to_string().contains("`c`"), "{error}");
    let error = env
        .render_str("t.txt", "{{ strict() }}", json!({}))
        .expect_err("missing");
    assert_eq!(error.kind(), ErrorKind::MissingArgument);
    assert!(error.to_string().contains("`a`"), "{error}");
}

#[test]
fn a_string_parameter_refuses_undefined_where_those_are_errors() {
    let mut env = view_env();
    env.add_filter("owned", |a: String| a);
    let error = env
        .render_str("t.txt", "{{ missing|owned }}", json!({}))
        .expect_err("undefined");
    assert_eq!(error.kind(), ErrorKind::UndefinedError);

    let mut lenient = Environment::new();
    lenient.add_filter("owned", |a: String| a);
    assert_eq!(
        lenient
            .render_str("t.txt", "[{{ missing|owned }}]", json!({}))
            .expect("renders"),
        "[]"
    );
}

#[test]
fn replacing_a_built_in() {
    let mut env = view_env();
    env.add_filter("upper", |text: &str| format!("up:{text}"));
    assert_eq!(
        env.render_str("t.txt", "{{ 'a'|upper }}", json!({}))
            .expect("renders"),
        "up:a"
    );
}

#[test]
fn values_convert_both_ways() {
    let value = Value::from_serialize(&json!({ "a": [1, 2.5, "x", null, true] })).expect("value");
    assert_eq!(value.get_attr("a").expect("attr").len(), Some(5));
    assert!(value.get_attr("b").expect("attr").is_undefined());
    let list = value.get_attr("a").expect("attr");
    assert_eq!(
        list.get_item(&Value::from(0)).expect("item").as_i64(),
        Some(1)
    );
    assert_eq!(
        list.get_item(&Value::from(-3)).expect("item").as_str(),
        Some("x")
    );
    assert_eq!(
        serde_json::to_value(&value).expect("json"),
        json!({ "a": [1, 2.5, "x", null, true] })
    );
    assert!(Value::UNDEFINED.get_attr("x").is_err());
    let collected: Value = vec![1, 2].into_iter().collect();
    assert_eq!(collected.to_string(), "[1, 2]");
    assert_eq!(Value::from(Some("x")).as_str(), Some("x"));
    assert!(Value::from(None::<&str>).is_none());
    assert!(Value::from_safe_string("<b>".to_owned()).is_safe());
}

#[derive(serde::Serialize)]
struct Page<'a> {
    zeta: &'a str,
    alpha: u32,
    kind: Kind,
    data: Data,
    nested: (i32, &'a str),
}

#[derive(serde::Serialize)]
enum Kind {
    Plain,
}

#[derive(serde::Serialize)]
enum Data {
    Tuple(i32, i32),
}

#[test]
fn structs_keep_their_field_order_and_enums_their_shape() {
    let page = Page {
        zeta: "z",
        alpha: 1,
        kind: Kind::Plain,
        data: Data::Tuple(1, 2),
        nested: (1, "a"),
    };
    let mut env = view_env();
    env.add_global("p", Value::from_serialize(&page).expect("value"));
    assert_eq!(
        env.render_str(
            "t.txt",
            "{% for k in p %}{{ k }},{% endfor %}{{ p.kind }}{{ p.data }}{{ p.nested }}",
            json!({})
        )
        .expect("renders"),
        "zeta,alpha,kind,data,nested,Plain{'Tuple': [1, 2]}(1, 'a')"
    );
}
