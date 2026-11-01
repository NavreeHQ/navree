//! Escaping decides XSS safety: exact outputs for the rules, and recorded
//! outputs for the OWASP XSS filter-evasion vectors in every position.

use serde_json::json;

use crate::support::{describe, fixture, fnv, view_env};

#[test]
fn html_templates_escape_every_special_character() {
    let env = view_env();
    let out = env
        .render_str("t.html", "{{ v }}", json!({ "v": "&<>\"'/ plain é" }))
        .expect("renders");
    assert_eq!(out, "&amp;&lt;&gt;&quot;&#x27;&#x2f; plain é");
}

#[test]
fn the_extension_picks_the_escaping() {
    let env = view_env();
    let ctx = json!({ "v": "<a href='x'>&</a>" });
    let escaped = "&lt;a href=&#x27;x&#x27;&gt;&amp;&lt;&#x2f;a&gt;";
    for name in [
        "t.html",
        "t.htm",
        "t.xml",
        "t.html.j2",
        "t.xml.jinja",
        "t.htm.jinja2",
        "a/b/c.html",
        "html",
    ] {
        assert_eq!(
            env.render_str(name, "{{ v }}", &ctx).expect("renders"),
            escaped,
            "{name}"
        );
    }
    for name in ["t.txt", "t.md", "t.j2", "t", "t.html.txt", "t.rs.j2"] {
        assert_eq!(
            env.render_str(name, "{{ v }}", &ctx).expect("renders"),
            "<a href='x'>&</a>",
            "{name}"
        );
    }
    for name in ["t.json", "t.json5", "t.js", "t.yaml", "t.yml", "t.json.j2"] {
        assert_eq!(
            env.render_str(name, "{{ v }}", &ctx).expect("renders"),
            "\"<a href='x'>&</a>\"",
            "{name}"
        );
    }
}

#[test]
fn values_other_than_strings_escape_as_they_print() {
    let env = view_env();
    let ctx = json!({ "l": ["<a>", 1, null], "m": {"k": "'v'"}, "n": 1, "b": true, "z": null });
    assert_eq!(
        env.render_str("t.html", "{{ l }}|{{ m }}|{{ n }}|{{ b }}|{{ z }}", &ctx)
            .expect("renders"),
        "[&#x27;&lt;a&gt;&#x27;, 1, None]|{&#x27;k&#x27;: &quot;&#x27;v&#x27;&quot;}|1|True|None"
    );
}

#[test]
fn safe_prints_as_it_is_and_only_safe_does() {
    let env = view_env();
    let ctx = json!({ "v": "<b>" });
    for (source, expected) in [
        ("{{ v|safe }}", "<b>"),
        ("{{ v|e }}", "&lt;b&gt;"),
        ("{{ v|escape }}", "&lt;b&gt;"),
        ("{{ v|e|e }}", "&lt;b&gt;"),
        ("{{ v|safe|e }}", "<b>"),
        ("{{ v|safe|upper }}", "<B>"),
        ("{{ v|upper }}", "&lt;B&gt;"),
        // Joining as text loses safety.
        ("{{ v|safe ~ v|safe }}", "&lt;b&gt;&lt;b&gt;"),
        ("{{ v|safe + v|safe }}", "&lt;b&gt;&lt;b&gt;"),
        // A safe joiner or item makes join escape the unsafe parts.
        ("{{ [v, v|safe]|join(\"/\") }}", "&lt;b&gt;&#x2f;<b>"),
        ("{{ [v]|join(\"/\"|safe) }}", "&lt;b&gt;"),
        ("{{ [v, v]|join(\"/\") }}", "&lt;b&gt;&#x2f;&lt;b&gt;"),
        // What macros, call blocks and set blocks return is safe.
        (
            "{% macro m(a) %}<i>{{ a }}</i>{% endmacro %}{{ m(v) }}",
            "<i>&lt;b&gt;</i>",
        ),
        (
            "{% set x %}<i>{{ v }}</i>{% endset %}{{ x }}",
            "<i>&lt;b&gt;</i>",
        ),
        (
            "{% macro m() %}{{ caller() }}{% endmacro %}{% call m() %}<u>{{ v }}</u>{% endcall %}",
            "<u>&lt;b&gt;</u>",
        ),
        // JSON for a script is safe in HTML.
        ("{{ v|tojson }}", "\"\\u003cb\\u003e\""),
    ] {
        assert_eq!(
            env.render_str("t.html", source, &ctx).expect("renders"),
            expected,
            "{source}"
        );
    }
    // In a text template, nothing escapes.
    assert_eq!(
        env.render_str("t.txt", "{{ v }}{{ v|safe }}", &ctx)
            .expect("renders"),
        "<b><b>"
    );
    // `|e` still escapes in a text template.
    assert_eq!(
        env.render_str("t.txt", "{{ v|e }}", &ctx).expect("renders"),
        "&lt;b&gt;"
    );
}

#[test]
fn autoescape_tags_change_the_mode_for_their_body() {
    let env = view_env();
    let ctx = json!({ "v": "<b>" });
    for (source, expected) in [
        (
            "{% autoescape false %}{{ v }}{% endautoescape %}{{ v }}",
            "<b>&lt;b&gt;",
        ),
        ("{% autoescape \"none\" %}{{ v }}{% endautoescape %}", "<b>"),
        (
            "{% autoescape true %}{{ v }}{% endautoescape %}",
            "&lt;b&gt;",
        ),
        (
            "{% autoescape \"json\" %}{{ v }}{% endautoescape %}",
            "\"<b>\"",
        ),
    ] {
        assert_eq!(
            env.render_str("t.html", source, &ctx).expect("renders"),
            expected,
            "{source}"
        );
    }
    assert_eq!(
        env.render_str(
            "t.txt",
            "{% autoescape true %}{{ v }}{% endautoescape %}",
            &ctx
        )
        .expect("renders"),
        "&lt;b&gt;"
    );
}

#[test]
fn included_templates_escape_by_their_own_name() {
    let mut env = view_env();
    env.add_template("a.txt", "{{ v }}").expect("compiles");
    env.add_template("b.html", "{{ v }}").expect("compiles");
    let ctx = json!({ "v": "<b>" });
    assert_eq!(
        env.render_str("t.html", "{% include \"a.txt\" %}|{{ v }}", &ctx)
            .expect("renders"),
        "<b>|&lt;b&gt;"
    );
    assert_eq!(
        env.render_str("t.txt", "{% include \"b.html\" %}|{{ v }}", &ctx)
            .expect("renders"),
        "&lt;b&gt;|<b>"
    );
}

/// The OWASP XSS filter-evasion vectors, as values.
fn vectors() -> Vec<String> {
    serde_json::from_str(include_str!("../fixtures/xss-vectors.json")).expect("the vectors")
}

#[test]
fn xss_vectors_in_every_position() {
    let env = view_env();
    let vectors = vectors();
    let mut compared = 0;
    let mut failures = Vec::new();
    for row in fixture(include_str!("../fixtures/escaping.jsonl")) {
        let name = row["n"].as_str().expect("a name");
        let source = row["t"].as_str().expect("a template");
        let outputs: Vec<String> = vectors
            .iter()
            .map(
                |vector| match env.render_str(name, source, json!({ "v": vector })) {
                    Ok(output) => output,
                    Err(error) => format!("E:{}", describe(&error)),
                },
            )
            .collect();
        compared += outputs.len();
        assert_eq!(row["c"], outputs.len(), "{name} {source}");
        if fnv(&outputs.join("\n")) != row["h"].as_str().expect("a hash") {
            failures.push(format!("{name}: {source}"));
        }
    }
    assert!(compared > 10_000, "the corpus is there: {compared}");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn no_vector_survives_html_escaping() {
    let env = view_env();
    for vector in vectors() {
        for source in ["{{ v }}", "<a href=\"{{ v }}\" title='{{ v }}'>{{ v }}</a>"] {
            let out = env
                .render_str("t.html", source, json!({ "v": vector }))
                .expect("renders");
            let without_template = out
                .replace("<a href=\"", "")
                .replace("\" title='", "")
                .replace("'>", "")
                .replace("</a>", "");
            assert!(
                !without_template.contains(['<', '>', '"', '\'']),
                "{vector}: {out}"
            );
        }
    }
}
