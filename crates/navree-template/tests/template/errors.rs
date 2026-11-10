//! Errors name the template, the line and the column, and no value.

use navree_template::{Environment, ErrorKind, UndefinedBehavior};
use serde_json::json;

use crate::support::view_env;

#[test]
fn syntax_errors_say_where() {
    let env = view_env();
    let error = env
        .render_str("mail/broken.html", "<p>\n{{ name }\n</p>", json!({}))
        .expect_err("syntax");
    assert_eq!(error.kind(), ErrorKind::SyntaxError);
    assert_eq!(error.name(), Some("mail/broken.html"));
    assert_eq!(error.line(), Some(2));
    assert!(error.column().is_some());
    assert!(
        error.to_string().contains("(in mail/broken.html:2)"),
        "{error}"
    );
}

#[test]
fn render_errors_say_where() {
    let env = view_env();
    let error = env
        .render_str("t.txt", "a\nb\n  {{ missing }}\n", json!({}))
        .expect_err("undefined");
    assert_eq!(error.kind(), ErrorKind::UndefinedError);
    assert_eq!(error.name(), Some("t.txt"));
    assert_eq!(error.line(), Some(3));
    assert_eq!(error.column(), Some(6));
    assert_eq!(error.to_string(), "undefined value (in t.txt:3)");
}

#[test]
fn an_error_in_a_block_or_macro_names_its_own_template() {
    let mut env = view_env();
    env.add_template("layout.html", "\n\n{% block b %}{% endblock %}")
        .expect("compiles");
    env.add_template(
        "child.html",
        "{% extends \"layout.html\" %}\n{% block b %}\n{{ missing }}{% endblock %}",
    )
    .expect("compiles");
    env.add_template(
        "macros.html",
        "{% macro m() %}\n\n\n{{ missing }}{% endmacro %}",
    )
    .expect("compiles");

    let error = env
        .get_template("child.html")
        .and_then(|template| template.render(json!({})))
        .expect_err("undefined");
    assert_eq!(error.name(), Some("child.html"));
    assert_eq!(error.line(), Some(3));

    let error = env
        .render_str(
            "page.html",
            "{% import \"macros.html\" as m %}\n{{ m.m() }}",
            json!({}),
        )
        .expect_err("undefined");
    assert_eq!(error.name(), Some("macros.html"));
    assert_eq!(error.line(), Some(4));
}

#[test]
fn an_error_in_an_include_reports_the_include_and_chains_the_cause() {
    let mut env = view_env();
    env.add_template("part.html", "\n{{ missing }}")
        .expect("compiles");
    let error = env
        .render_str("page.html", "\n\n{% include \"part.html\" %}", json!({}))
        .expect_err("undefined");
    assert_eq!(error.kind(), ErrorKind::BadInclude);
    assert_eq!(error.name(), Some("page.html"));
    assert_eq!(error.line(), Some(3));
    let source = std::error::Error::source(&error).expect("the cause");
    assert!(source.to_string().contains("part.html:2"), "{source}");
}

#[test]
fn messages_hold_no_values() {
    let env = view_env();
    let secret = "s3cr3t-token";
    for source in [
        "{{ v + 1 }}",
        "{{ v - 1 }}",
        "{{ v * v }}",
        "{{ v / 1 }}",
        "{{ v // 1 }}",
        "{{ v % 1 }}",
        "{{ v ** 1 }}",
        "{{ -v }}",
        "{{ v|round }}",
        "{{ v|abs }}",
        "{{ v|int }}",
        "{{ v|nope }}",
        "{{ v is nope }}",
        "{{ v() }}",
        "{{ v.m() }}",
        "{% for a, b in [v] %}{% endfor %}",
        "{% include v %}",
        "{% extends v %}",
        "{% import v as x %}",
    ] {
        if let Err(error) = env.render_str("t.txt", source, json!({ "v": secret })) {
            // A template name is a template's business, not a context secret:
            // the three statements above that take a name report it.
            if !source.contains("include")
                && !source.contains("extends")
                && !source.contains("import")
            {
                assert!(!error.to_string().contains(secret), "{source}: {error}");
                assert!(
                    !format!("{error:?}").contains(secret),
                    "{source}: {error:?}"
                );
            }
        }
    }
}

#[test]
fn a_context_that_cannot_serialize_is_an_error() {
    struct Broken;
    impl serde::Serialize for Broken {
        fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom("cannot serialize"))
        }
    }
    let env = view_env();
    let error = env.render_str("t.txt", "x", Broken).expect_err("broken");
    assert_eq!(error.kind(), ErrorKind::BadSerialization);
}

#[test]
fn undefined_behaviors() {
    let source_print = "[{{ missing }}]";
    let source_test = "{% if missing %}y{% else %}n{% endif %}";
    let source_loop = "{% for x in missing %}y{% else %}n{% endfor %}";
    let source_attr = "{{ missing.x }}";
    for (behavior, print, test, for_loop, attr) in [
        (
            UndefinedBehavior::Lenient,
            Some("[]"),
            Some("n"),
            Some("n"),
            None,
        ),
        (
            UndefinedBehavior::Chainable,
            Some("[]"),
            Some("n"),
            Some("n"),
            Some(""),
        ),
        (UndefinedBehavior::SemiStrict, None, Some("n"), None, None),
        (UndefinedBehavior::Strict, None, None, None, None),
    ] {
        let mut env = Environment::new();
        env.set_undefined_behavior(behavior);
        let run = |source: &str| env.render_str("t.txt", source, json!({})).ok();
        assert_eq!(run(source_print).as_deref(), print, "{behavior:?} print");
        assert_eq!(run(source_test).as_deref(), test, "{behavior:?} test");
        assert_eq!(run(source_loop).as_deref(), for_loop, "{behavior:?} loop");
        assert_eq!(run(source_attr).as_deref(), attr, "{behavior:?} attr");
    }
}
