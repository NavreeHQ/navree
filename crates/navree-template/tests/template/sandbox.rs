//! What a template can reach: data, and nothing else.

use navree_template::{Environment, ErrorKind};
use serde_json::json;

use crate::support::view_env;

#[derive(serde::Serialize)]
struct Account {
    name: &'static str,
    #[serde(skip)]
    password: &'static str,
}

impl Account {
    #[allow(dead_code)]
    fn secret(&self) -> &str {
        self.password
    }
}

#[test]
fn only_serialized_fields_exist() {
    let env = view_env();
    let account = Account {
        name: "anu",
        password: "hunter2",
    };
    let ctx = json!({ "a": account });
    assert_eq!(
        env.render_str("t.txt", "{{ a.name }}{{ a.password is defined }}", &ctx)
            .expect("renders"),
        "adaFalse"
    );
    assert!(env.render_str("t.txt", "{{ a.secret() }}", &ctx).is_err());
}

#[test]
fn values_have_no_methods() {
    let env = view_env();
    let ctx = json!({ "s": "text", "l": [1], "m": {"a": 1}, "n": 1 });
    for source in [
        "{{ s.upper() }}",
        "{{ s.__class__ }}",
        "{{ s.__class__.__mro__ }}",
        "{{ l.append(1) }}",
        "{{ l.pop() }}",
        "{{ m.items() }}",
        "{{ m.keys() }}",
        "{{ n.real }}",
        "{{ s.format('x') }}",
    ] {
        match env.render_str("t.txt", source, &ctx) {
            // Unknown attributes are undefined, and printing one fails here.
            Err(error) => assert!(
                matches!(
                    error.kind(),
                    ErrorKind::UnknownMethod | ErrorKind::UndefinedError
                ),
                "{source}: {error}"
            ),
            Ok(output) => panic!("{source} rendered {output}"),
        }
    }
}

#[test]
fn nothing_about_the_engine_is_in_scope() {
    let mut env = Environment::new();
    env.add_template("a.txt", "a").expect("compiles");
    for name in [
        "self",
        "env",
        "environment",
        "config",
        "request",
        "os",
        "sys",
        "__builtins__",
        "cycler",
        "joiner",
        "lipsum",
        "namespace",
        "debug",
        "url_for",
    ] {
        let source = format!("{{{{ {name} is defined }}}}");
        let out = env.render_str("t.txt", &source, json!({})).expect(name);
        assert_eq!(out, "False", "{name}");
    }
}

#[test]
fn templates_are_reached_by_name_through_the_environment_only() {
    let env = view_env();
    for source in [
        "{% include \"/etc/passwd\" %}",
        "{% include \"../secret.html\" %}",
        "{% extends \"C:/Windows/win.ini\" %}",
        "{% import \"/etc/passwd\" as x %}",
    ] {
        let error = env
            .render_str("t.txt", source, json!({}))
            .expect_err(source);
        assert_eq!(
            error.kind(),
            ErrorKind::TemplateNotFound,
            "{source}: {error}"
        );
    }
}

#[test]
fn a_template_cannot_change_the_context() {
    let env = view_env();
    let ctx = json!({ "m": {"a": 1}, "l": [1] });
    assert!(env.render_str("t.txt", "{% set m.a = 2 %}", &ctx).is_err());
    assert_eq!(
        env.render_str(
            "t.txt",
            "{% set l = l + [2] %}{{ l }}{% set m = 1 %}{{ m }}",
            &ctx
        )
        .expect("renders"),
        "[1, 2]1"
    );
    // The value itself is unchanged for whoever renders next.
    assert_eq!(
        env.render_str("t.txt", "{{ l }}{{ m }}", &ctx)
            .expect("renders"),
        "[1]{'a': 1}"
    );
}

#[test]
fn imported_private_names_stay_private() {
    let mut env = view_env();
    env.add_template("m.html", "{% set _hidden = 1 %}{% set shown = 2 %}")
        .expect("compiles");
    assert_eq!(
        env.render_str(
            "t.txt",
            "{% import \"m.html\" as m %}{{ m.shown }}{{ m._hidden is defined }}",
            json!({})
        )
        .expect("renders"),
        "2False"
    );
    let error = env
        .render_str("t.txt", "{% from \"m.html\" import _hidden %}", json!({}))
        .expect_err("private");
    assert_eq!(error.kind(), ErrorKind::SyntaxError);
}
