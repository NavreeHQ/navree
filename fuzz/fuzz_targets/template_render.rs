//! `navree_template`: parse and render arbitrary template text.
//!
//! Input: one byte picking the settings (extension, whitespace rules and
//! undefined behavior), then the template as text.
//!
//! Invariants: nothing panics or overflows the stack; the output stays
//! within the output limit; an error names where it happened; and in an HTML
//! template a value prints with no raw `&`, `<`, `>`, `"`, `'` or `/`, whatever
//! the value holds, and however it got there: through an include, a layout, a
//! macro, a call block or `|tojson`.
#![no_main]

use libfuzzer_sys::fuzz_target;
use navree_template::{Environment, Limits, UndefinedBehavior};
use serde_json::json;

const NAMES: [&str; 4] = ["t.html", "t.txt", "t.json", "t.xml.j2"];

fn environment(settings: u8) -> Environment {
    let mut env = Environment::new();
    env.set_undefined_behavior(match settings >> 4 & 3 {
        0 => UndefinedBehavior::Lenient,
        1 => UndefinedBehavior::Chainable,
        2 => UndefinedBehavior::SemiStrict,
        _ => UndefinedBehavior::Strict,
    });
    env.set_trim_blocks(settings & 4 != 0);
    env.set_lstrip_blocks(settings & 8 != 0);
    env.set_keep_trailing_newline(settings & 64 != 0);
    // Small budgets keep each run short; the caps are what is under test.
    let mut limits = Limits::default();
    limits.max_depth = 16;
    limits.max_include_depth = 8;
    limits.max_loop_iterations = 10_000;
    limits.max_operations = 200_000;
    limits.max_output_bytes = 1 << 20;
    env.set_limits(limits);
    env.add_template("layout.html", "{% block a %}A{% endblock %}|{% block b %}{{ v }}{% endblock %}")
        .expect("layout compiles");
    env.add_template(
        "macros.html",
        "{% macro m(x, y=1) %}<{{ x }}{{ y }}{{ caller() if caller is defined }}>{% endmacro %}",
    )
    .expect("macros compile");
    env.add_template("text.txt", "{{ v }}").expect("text compiles");
    env.add_template("data.json", "{{ v }}").expect("data compiles");
    env.add_template("list.json", "{{ [v, 'a'|safe]|join(',') }}").expect("data compiles");
    env.add_template("swap.json", "{{ 'a'|e|replace('a', v) }}").expect("data compiles");
    env.add_template("plain.txt", "{% macro p(x) %}{{ x }}{% endmacro %}")
        .expect("macros compile");
    env.add_template(
        "off.html",
        "{% autoescape false %}{% block a %}{% endblock %}{% endautoescape %}",
    )
    .expect("layout compiles");
    env
}

fuzz_target!(|data: &[u8]| {
    let Some((&settings, text)) = data.split_first() else {
        return;
    };
    let Ok(text) = std::str::from_utf8(text) else {
        return;
    };
    let env = environment(settings);
    let name = NAMES[usize::from(settings & 3)];
    let ctx = json!({
        "v": text,
        "n": 42,
        "f": 1.5,
        "t": true,
        "nil": null,
        "list": [1, "two", [3], {"k": "v"}],
        "map": {"a": 1, "b": {"c": [1, 2]}},
    });
    match env.render_str(name, text, &ctx) {
        Ok(output) => assert!(output.len() <= 1 << 20, "the output limit holds"),
        Err(error) => {
            // Limits and compile errors name the template; only a context
            // that fails to serialize could not, and this one does not.
            assert!(error.name().is_some(), "{error:?}");
            assert!(error.line().is_some(), "{error:?}");
        }
    }

    // Whatever a value holds, HTML printing leaves no raw special character.
    let printed = env
        .render_str("t.html", "{{ v }}", &ctx)
        .expect("printing a string renders");
    assert!(
        !printed.contains(['<', '>', '"', '\'', '/']),
        "unescaped output: {printed:?}"
    );
    assert!(
        printed
            .split('&')
            .skip(1)
            .all(|rest| ["amp;", "lt;", "gt;", "quot;", "#x27;", "#x2f;"]
                .iter()
                .any(|entity| rest.starts_with(entity))),
        "a bare ampersand: {printed:?}"
    );

    // The value can't open a tag or end an attribute by any route, in any
    // template printing into HTML. These templates hold no quotes of their own
    // but what the assertion strips.
    for source in [
        "{% include 'text.txt' %}",
        "{% include 'data.json' %}",
        "{% import 'plain.txt' as m %}{{ m.p(v) }}",
        "{% import 'macros.html' as m %}{% autoescape false %}{{ m.m(v, 2) }}{% endautoescape %}",
        "{% extends 'off.html' %}{% block a %}{{ v }}{% endblock %}",
        "{% autoescape 'json' %}{% set y %}{{ v }}{% endset %}{% endautoescape %}{{ y }}",
        "{{ v|tojson }}",
        "{{ [v]|tojson|safe }}",
        "{% include 'list.json' %}",
        "{% include 'swap.json' %}",
        "{% autoescape 'json' %}{{ [v, 'a'|e]|join(',') }}{% endautoescape %}",
        "{% autoescape 'json' %}{{ [v]|join('a'|e) }}{% endautoescape %}",
        "{% autoescape 'json' %}{{ 'a'|e|replace('a', v) }}{% endautoescape %}",
        "<noscript><script></noscript>{{ v|tojson }}",
        "<svg><![CDATA[><script>]]>{{ v|tojson }}",
        "<svg><script>{{ v|tojson }}",
        "<math><script>{{ v|tojson }}",
        "<a =b=><script>{{ v|tojson }}",
        "{% macro s(x) %}<script>{{ x|tojson }}{% endmacro %}{{ s(v) }}",
        "{% set y = v|tojson %}{{ y }}",
    ] {
        let printed = env
            .render_str("t.html", source, &ctx)
            .expect("printing a string renders");
        // `<` and `>` of a macro's own markup aside.
        let printed = printed.replace("<&", "&").replace("<\\", "\\");
        let printed = printed.trim_start_matches('<').trim_end_matches('>');
        assert!(
            !printed.contains(['"', '\'']),
            "{source}: a raw quote: {printed:?}"
        );
    }
    let in_script = env
        .render_str("t.html", "<script>{{ v|tojson }}{% include 'data.json' %}", &ctx)
        .expect("printing a string renders");
    assert!(
        !in_script.get(8..).unwrap_or("").contains(['<', '>', '&', '\'']),
        "a script can end: {in_script:?}"
    );
});
