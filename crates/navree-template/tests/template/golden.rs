//! The language, against recorded outputs: a thousand expressions and
//! statements under both settings, and templates working together.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::support::{ctx, fixture, outcome, plain_env, view_env};

fn run_table(text: &str) {
    let context = ctx();
    let (view, plain) = (view_env(), plain_env());
    let mut failures = Vec::new();
    let mut compared = 0;
    for row in fixture(text) {
        let source = row["t"].as_str().expect("a template");
        let name = row["n"].as_str().unwrap_or("t.html");
        for (key, env) in [("v", &view), ("p", &plain)] {
            let Some(expected) = row.get(key) else {
                continue;
            };
            compared += 1;
            let actual = outcome(env.render_str(name, source, &context));
            if &actual != expected {
                failures.push(format!(
                    "{source:?} as {name} [{key}]\n  expected {expected}\n  actual   {actual}"
                ));
            }
        }
    }
    assert!(compared > 100, "the table is there");
    assert!(
        failures.is_empty(),
        "{} of {compared} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn expressions() {
    run_table(include_str!("../fixtures/expressions.jsonl"));
}

#[test]
fn statements_and_whitespace() {
    run_table(include_str!("../fixtures/statements.jsonl"));
}

/// The templates the layout fixtures render, together.
fn layout_templates() -> BTreeMap<&'static str, &'static str> {
    let mut templates = BTreeMap::new();
    for (name, source) in [
        (
            "base.html",
            "<h>{% block title %}T{% endblock %}</h>{% block body %}B{% endblock %}<f>{{ brand }}</f>",
        ),
        (
            "child.html",
            "{% extends \"base.html\" %}{% set brand = \"<B>\" %}ignored{% block body %}[{{ name }}]{% endblock %}ignored",
        ),
        (
            "grand.html",
            "{% extends \"child.html\" %}{% block title %}G{{ name }}{% endblock %}",
        ),
        ("cycle-a.html", "{% extends \"cycle-b.html\" %}"),
        ("cycle-b.html", "{% extends \"cycle-a.html\" %}"),
        ("self.html", "{% extends \"self.html\" %}"),
        (
            "twice.html",
            "{% extends \"base.html\" %}{% extends \"base.html\" %}",
        ),
        ("late.html", "before{% extends \"base.html\" %}after"),
        (
            "blockfirst.html",
            "{% block early %}E{% endblock %}{% extends \"base.html\" %}",
        ),
        ("partial.html", "<p>{{ name }}</p>{% set leaked = 1 %}"),
        ("inc.html", "{% include \"partial.html\" %}[{{ leaked }}]"),
        (
            "incloop.html",
            "{% for name in [\"a\", \"b\"] %}{% include \"partial.html\" %}{% endfor %}",
        ),
        ("incmissing.html", "{% include \"missing.html\" %}"),
        ("incbad.html", "\n\n{% include \"bad.html\" %}"),
        ("bad.html", "\n{{ undefined_thing.x }}"),
        ("text.txt", "text {{ name }}"),
        (
            "htmlinc.html",
            "{{ name }}{% include \"text.txt\" %}{{ name }}",
        ),
        (
            "txtinc.txt",
            "{{ name }}{% include \"partial.html\" %}{{ name }}",
        ),
        (
            "macros.html",
            "{% set hidden = 1 %}{% set _private = 2 %}top {{ name }}{% macro a(x) %}A{{ x }}{{ b() }}{{ hidden }}{% endmacro %}{% macro b() %}B{{ _private }}{% endmacro %}{% macro c() %}{{ name }}{% endmacro %}{% macro d() %}{{ caller() }}{% endmacro %}",
        ),
        (
            "imp.html",
            "{% import \"macros.html\" as m %}{{ m.a(1) }}{{ m.b() }}{{ m.c() }}{{ m.hidden }}[{{ m._private }}]{{ m.nope }}[{{ m }}]{% call m.d() %}cb{{ name }}{% endcall %}",
        ),
        (
            "from.html",
            "{% from \"macros.html\" import a, b as bee, hidden, nope %}{{ a(1) }}{{ bee() }}{{ hidden }}[{{ nope is defined }}]",
        ),
        ("from2.html", "{% from \"macros.html\" import _private %}"),
        (
            "from3.html",
            "{% from \"macros.html\" import a with context %}{{ a(1) }}",
        ),
        ("from4.html", "{% from \"macros.html\" import %}x"),
        (
            "impctx.html",
            "{% set name = \"local\" %}{% import \"macros.html\" as m %}{{ m.c() }}",
        ),
        (
            "childimp.html",
            "{% extends \"base.html\" %}{% import \"macros.html\" as m %}{% block body %}{{ m.a(2) }}{% endblock %}",
        ),
        (
            "childmacro.html",
            "{% extends \"base.html\" %}{% macro z() %}Z{% endmacro %}{% block body %}{{ z() }}{% endblock %}",
        ),
        (
            "childset.html",
            "{% extends \"setbase.html\" %}{% set v = \"child\" %}{% block b %}{{ v }}{% endblock %}",
        ),
        (
            "setbase.html",
            "{% set v = \"base\" %}{% block b %}{% endblock %}",
        ),
        (
            "scoped.html",
            "{% for x in [1, 2] %}{% block b scoped %}{{ x }}{% endblock %}{% endfor %}",
        ),
        (
            "scopedchild.html",
            "{% extends \"scoped.html\" %}{% block b %}[{{ x }}]{% endblock %}",
        ),
        ("required.html", "{% block must required %}{% endblock %}"),
        (
            "requiredchild.html",
            "{% extends \"required.html\" %}{% block must %}ok{% endblock %}",
        ),
        ("requiredchild2.html", "{% extends \"required.html\" %}"),
        (
            "inext.html",
            "{% include \"child.html\" %}|{% include \"grand.html\" %}",
        ),
        ("recinc.html", "{% include \"recinc.html\" %}"),
        ("recimp.html", "{% import \"recimp.html\" as x %}"),
        ("dyn.html", "{% include name2 %}"),
        (
            "dynext.html",
            "{% extends name2 %}{% block body %}D{% endblock %}",
        ),
        (
            "condext.html",
            "{% if t %}{% extends \"base.html\" %}{% endif %}{% block body %}C{% endblock %}",
        ),
        (
            "blockext.html",
            "{% block a %}{% extends \"base.html\" %}{% endblock %}",
        ),
        (
            "superuse.html",
            "{% extends \"base.html\" %}{% block body %}{{ super() }}{% endblock %}",
        ),
        ("jsonish.json", "{{ name }}{{ map }}"),
        (
            "macrojson.html",
            "{% macro m() %}{{ name }}{% endmacro %}{% autoescape \"json\" %}{{ m() }}{% endautoescape %}",
        ),
        (
            "esc1.html",
            "{% include \"esc2.txt\" %}{{ name }}{% import \"esc3.txt\" as e %}{{ e.m() }}{{ e }}",
        ),
        ("esc2.txt", "{{ name }}"),
        (
            "esc3.txt",
            "{% macro m() %}{{ name }}{% endmacro %}raw{{ name }}",
        ),
        (
            "esc4.txt",
            "{% import \"esc5.html\" as e %}{{ e.m() }}{{ e }}",
        ),
        (
            "esc5.html",
            "{% macro m() %}{{ name }}{% endmacro %}raw{{ name }}",
        ),
    ] {
        templates.insert(name, source);
    }
    templates
}

#[test]
fn templates_together() {
    let templates = layout_templates();
    let mut context = ctx();
    let map = context.as_object_mut().expect("an object");
    map.insert("name".to_owned(), json!("<N>"));
    map.insert("brand".to_owned(), json!("<brand>"));
    map.insert("name2".to_owned(), json!("base.html"));

    let (mut view, mut plain) = (view_env(), plain_env());
    for (name, source) in &templates {
        // Templates that do not compile stay out; the rows name them.
        let _ = view.add_template(*name, source);
        let _ = plain.add_template(*name, source);
    }
    let mut compared = 0;
    let mut failures = Vec::new();
    for row in fixture(include_str!("../fixtures/layouts.jsonl")) {
        let name = row["n"].as_str().expect("a name");
        let inline = row["i"].as_bool().expect("inline");
        let Some(source) = templates.get(name) else {
            continue;
        };
        for (key, env) in [("v", &view), ("p", &plain)] {
            let Some(expected) = row.get(key) else {
                continue;
            };
            let result = if inline {
                env.render_str(name, source, &context)
            } else {
                env.get_template(name)
                    .and_then(|template| template.render(&context))
            };
            compared += 1;
            let actual: Value = outcome(result);
            if &actual != expected {
                failures.push(format!(
                    "{name} inline={inline} [{key}]\n  expected {expected}\n  actual   {actual}"
                ));
            }
        }
    }
    assert!(compared > 50, "the table is there: {compared}");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn deviations_are_deliberate() {
    let env = view_env();
    // Dividing by zero in a test is false, not a crash.
    assert_eq!(
        env.render_str("t.txt", "{{ 10 is divisibleby 0 }}", json!({}))
            .expect("renders"),
        "False"
    );
    // A `with` block can be left with `break`.
    assert_eq!(
        env.render_str(
            "t.txt",
            "{% for i in [1, 2] %}{% with x = 1 %}{% break %}{% endwith %}{{ i }}{% endfor %}",
            json!({})
        )
        .expect("renders"),
        ""
    );
}
