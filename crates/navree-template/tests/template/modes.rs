//! How escaping crosses templates: each template prints in its own mode, and
//! nothing rendered inside another prints less strictly than where it lands.
//! A hostile value must never reach the output raw.

use navree_template::{Environment, ErrorKind, UndefinedBehavior, default_auto_escape};
use serde_json::{Value, json};

const HOSTILE: &str = "</script><img src=x onerror=alert(1)>'\"&/";

fn ctx() -> Value {
    json!({ "x": HOSTILE, "attr": " onmouseover=alert(1) x=", "o": { "k": HOSTILE } })
}

fn env() -> Environment {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Lenient);
    for (name, source) in [
        (
            "layout.html",
            "<main>{{ x }}{% block b %}{% endblock %}</main>",
        ),
        ("page.txt", "TXT[{{ x }}]"),
        (
            "rawlayout.txt",
            "<main>{{ x }}{% block b %}{% endblock %}</main>",
        ),
        ("data.json", "{{ x }}"),
        ("init.js", "var v = {{ x }};"),
        (
            "macros.html",
            "{% macro card(u) %}<b>{{ u }}</b>{% endmacro %}",
        ),
        (
            "macros.txt",
            "{% macro raw(u) %}<b>{{ u }}</b>{% endmacro %}",
        ),
        ("modset.json", "{% set snippet %}{{ x }}{% endset %}"),
        (
            "rawparent.html",
            "{% autoescape false %}{% block b %}{% endblock %}{% endautoescape %}",
        ),
        ("layer.html", "{% include 'page.txt' %}"),
    ] {
        env.add_template(name, source).expect("compiles");
    }
    env
}

fn render(env: &Environment, name: &str, source: &str) -> String {
    env.render_str(name, source, ctx())
        .unwrap_or_else(|error| panic!("{name}: {source}: {error}"))
}

/// Whether the hostile value reached the output so that it opens a tag.
fn is_raw(output: &str) -> bool {
    output.contains("<img src=x") || output.contains("</script><")
}

#[test]
fn names_pick_the_mode_whatever_their_case() {
    use navree_template::AutoEscape::{Html, Json, None};
    for (name, mode) in [
        ("a.html", Html),
        ("a.HTML", Html),
        ("a.Html", Html),
        ("a.HTML.j2", Html),
        ("a.htm", Html),
        ("a.xml", Html),
        ("a.XML.JINJA", Html),
        ("a.xhtml", Html),
        ("a.xhtml.j2", Html),
        ("a.svg", Html),
        ("a.svg.j2", Html),
        ("a.html.", Html),
        ("a.html ", Html),
        ("a.JSON", Json),
        ("a.js", Json),
        ("a.mjs", Json),
        ("a.YML", Json),
        ("a.txt", None),
        ("a.md", None),
        ("a", None),
    ] {
        assert_eq!(default_auto_escape(name), mode, "{name}");
    }
}

#[test]
fn upper_case_names_escape() {
    let env = env();
    for name in ["p.HTML", "p.HTML.j2", "p.xhtml", "p.svg", "p.html."] {
        assert!(!is_raw(&render(&env, name, "<p>{{ x }}</p>")), "{name}");
    }
}

#[test]
fn an_extending_template_keeps_the_layouts_mode() {
    let env = env();
    for name in ["p.txt", "p.j2", "p"] {
        let out = render(
            &env,
            name,
            r#"{% extends "layout.html" %}{% block b %}{{ x }}{% endblock %}"#,
        );
        assert!(!is_raw(&out), "{name}: {out}");
    }
}

#[test]
fn a_layout_cannot_print_less_strictly_than_the_template_extending_it() {
    let env = env();
    let out = render(
        &env,
        "p.html",
        r#"{% extends "rawlayout.txt" %}{% block b %}{{ x }}{% endblock %}"#,
    );
    assert!(!is_raw(&out), "{out}");
    assert!(
        out.starts_with("<main>"),
        "the layout's own text stays: {out}"
    );
}

#[test]
fn includes_never_print_less_strictly_than_their_includer() {
    let env = env();
    for source in [
        r#"<p>{% include "page.txt" %}</p>"#,
        r#"<p>{% include "data.json" %}</p>"#,
        r#"<script>{% include "init.js" %}</script>"#,
        r#"<div>{% include "layer.html" %}</div>"#,
    ] {
        let out = render(&env, "p.html", source);
        assert!(!is_raw(&out), "{source}: {out}");
    }
    // The included text stays; values print for where they land.
    assert_eq!(
        env.render_str(
            "p.html",
            r#"<p>{% include "page.txt" %}</p>"#,
            json!({ "x": "<b>" })
        )
        .expect("renders"),
        "<p>TXT[&lt;b&gt;]</p>"
    );
    // A script gets JSON it can read, with nothing that ends the script.
    assert_eq!(
        env.render_str(
            "p.html",
            r#"<script>{% include "init.js" %}</script>"#,
            json!({ "x": "</script>\"" })
        )
        .expect("renders"),
        "<script>var v = \"\\u003c/script\\u003e\\\"\";</script>"
    );
}

#[test]
fn autoescape_reaches_only_the_text_inside_it() {
    let env = env();
    for source in [
        // A macro's body, wherever it is called from.
        r#"{% import "macros.html" as m %}{% autoescape false %}{{ m.card(x) }}{% endautoescape %}"#,
        r#"{% macro c(u) %}<b>{{ u }}</b>{% endmacro %}{% autoescape false %}{{ c(x) }}{% endautoescape %}"#,
        // A child's block, through a layout that turned escaping off around it.
        r#"{% extends "rawparent.html" %}{% block b %}{{ x }}{% endblock %}"#,
        // A call block, from a macro that turned escaping off around caller().
        r#"{% macro w() %}{% autoescape false %}{{ caller() }}{% endautoescape %}{% endmacro %}{% call w() %}{{ x }}{% endcall %}"#,
        // An include under the tag still escapes by its own name.
        r#"{% autoescape false %}{% include "layout.html" %}{% endautoescape %}"#,
    ] {
        let out = render(&env, "p.html", source);
        assert!(!is_raw(&out), "{source}: {out}");
    }
    // Inside the tag, text written there is what it says.
    assert!(is_raw(&render(
        &env,
        "p.html",
        "{% autoescape false %}{{ x }}{% endautoescape %}"
    )));
    // A macro written inside the tag prints raw only into its own output,
    // and that output is not safe anywhere else.
    let out = render(
        &env,
        "p.html",
        "{% autoescape false %}{% macro m() %}{{ x }}{% endmacro %}{% endautoescape %}{{ m() }}",
    );
    assert!(!is_raw(&out), "{out}");
}

#[test]
fn macros_from_a_text_template_escape_where_they_land() {
    let env = env();
    let out = render(
        &env,
        "p.html",
        r#"{% import "macros.txt" as m %}{{ m.raw(x) }}"#,
    );
    assert!(!is_raw(&out), "{out}");
    // And an HTML macro stays escaped in a text template.
    let out = render(
        &env,
        "p.txt",
        r#"{% import "macros.html" as m %}{{ m.card(x) }}"#,
    );
    assert!(!is_raw(&out), "{out}");
}

#[test]
fn a_capture_is_safe_only_for_the_mode_it_was_escaped_in() {
    let env = env();
    for source in [
        r#"{% autoescape "json" %}{% set y %}{{ x }}{% endset %}{% endautoescape %}<p>{{ y }}</p>"#,
        r#"{% macro f(u) %}{{ u }}{% endmacro %}{% autoescape "json" %}{% set y = f(x) %}{% endautoescape %}<p>{{ y }}</p>"#,
        r#"{% autoescape "json" %}{% set y = x|e %}{% endautoescape %}<p>{{ y }}</p>"#,
        r#"{% import "modset.json" as m %}<p>{{ m.snippet }}</p>"#,
    ] {
        let out = render(&env, "p.html", source);
        assert!(!is_raw(&out), "{source}: {out}");
    }
    // Printed in the mode it was escaped in, it prints as it is.
    assert_eq!(
        env.render_str(
            "p.json",
            "{% set y %}{{ x }}{% endset %}{{ y }}",
            json!({ "x": "<b>" })
        )
        .expect("renders"),
        "\"<b>\""
    );
    // `|safe` is the author's word, in every mode.
    assert_eq!(
        env.render_str("p.json", "{{ x|safe }}", json!({ "x": "<b>" }))
            .expect("renders"),
        "<b>"
    );
}

#[test]
fn autoescape_takes_only_true_false_and_mode_names() {
    let mut env = env();
    for argument in [
        "undefinedname",
        "none",
        "0",
        "1",
        "[]",
        "'x'",
        "'false'",
        "x",
        "true and false",
    ] {
        let source = format!("{{% autoescape {argument} %}}{{{{ x }}}}{{% endautoescape %}}");
        let error = env.add_template("bad.html", &source).expect_err(argument);
        assert_eq!(error.kind(), ErrorKind::SyntaxError, "{argument}: {error}");
        assert_eq!(error.name(), Some("bad.html"), "{argument}");
    }
    for argument in [
        "true", "false", "True", "False", "'html'", "\"json\"", "'none'",
    ] {
        let source = format!("{{% autoescape {argument} %}}{{{{ x }}}}{{% endautoescape %}}");
        env.add_template("good.html", &source).expect(argument);
    }
}

#[test]
fn tojson_is_safe_wherever_it_can_be_printed() {
    let env = env();
    // In a double-quoted attribute a quote can't end the attribute.
    assert_eq!(
        render(&env, "p.html", r#"<a data-x="{{ attr|tojson }}">"#),
        r#"<a data-x="&quot; onmouseover=alert(1) x=&quot;">"#
    );
    for source in [
        r#"<a data-x="{{ o|tojson }}">"#,
        r#"<a data-x='{{ o|tojson }}'>"#,
        r#"<a data-x="{{ o|tojson|safe }}">"#,
        r#"{% set y = o|tojson %}<a data-x="{{ y }}">"#,
        r#"{% macro j(v) %}{{ v|tojson }}{% endmacro %}<a data-x="{{ j(o) }}">"#,
        r#"<p>{{ o|tojson }}</p>"#,
    ] {
        let out = render(&env, "p.html", source);
        let without_markup = out
            .replacen("<a data-x=\"", "", 1)
            .replacen("<a data-x='", "", 1)
            .replacen("<p>", "", 1)
            .replace("</p>", "")
            .replace("\">", "")
            .replace("'>", "");
        assert!(
            !without_markup.contains(['"', '\'', '<', '>']),
            "{source}: {out}"
        );
    }
    // In a script it stays JSON a script can read.
    assert_eq!(
        env.render_str(
            "p.html",
            "<script>var v = {{ x|tojson }};</script>",
            json!({ "x": "</script>\"'&" })
        )
        .expect("renders"),
        "<script>var v = \"\\u003c/script\\u003e\\\"\\u0027\\u0026\";</script>"
    );
    assert_eq!(
        env.render_str(
            "p.html",
            r#"<SCRIPT type="application/json">{{ x|tojson }}</SCRIPT><a b="{{ x|tojson }}">"#,
            json!({ "x": "y" })
        )
        .expect("renders"),
        r#"<SCRIPT type="application/json">"y"</SCRIPT><a b="&quot;y&quot;">"#
    );
    // What looks like a script is not one inside an attribute or a comment.
    for source in [
        r#"<a title="<script>" data-x="{{ attr|tojson }}">"#,
        r#"<!-- <script> --><a data-x="{{ attr|tojson }}">"#,
        r#"<style><script></style><a data-x="{{ attr|tojson }}">"#,
        r#"<textarea><script></textarea><a data-x="{{ attr|tojson }}">"#,
        r#"<svg><script href="x"/><a data-x="{{ attr|tojson }}">"#,
    ] {
        let out = render(&env, "p.html", source);
        assert!(out.contains("&quot; onmouseover"), "{source}: {out}");
    }
    // A text or JSON template gets plain JSON.
    assert_eq!(
        env.render_str("p.json", "{{ x|tojson }}", json!({ "x": "a" }))
            .expect("renders"),
        "\"a\""
    );
}

#[test]
fn a_mode_set_after_templates_compile_applies_to_them() {
    let mut env = Environment::new();
    env.add_template("a.txt", "{{ x }}").expect("compiles");
    env.set_auto_escape_callback(|_| navree_template::AutoEscape::Html);
    assert_eq!(
        env.get_template("a.txt")
            .expect("found")
            .render(json!({ "x": "<b>" }))
            .expect("renders"),
        "&lt;b&gt;"
    );
}
