//! The caps. Attacks run at reduced scale: each test sets small limits or
//! relies on a cap well below what could hurt, and fails on an assertion.

use navree_template::{Environment, ErrorKind, Limits, UndefinedBehavior};
use serde_json::json;

fn env_with(limits: impl FnOnce(&mut Limits)) -> Environment {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::SemiStrict);
    let mut current = env.limits();
    limits(&mut current);
    env.set_limits(current);
    env
}

fn assert_limited(env: &Environment, source: &str) {
    let error = env
        .render_str("t.txt", source, json!({}))
        .expect_err(source);
    assert_eq!(error.kind(), ErrorKind::LimitExceeded, "{source}: {error}");
    assert_eq!(error.name(), Some("t.txt"), "{source}: {error}");
    assert!(error.line().is_some(), "{source}: {error}");
}

/// Runs `check` on a thread with the stack of a Windows main thread.
fn on_small_stack(check: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(check)
        .expect("a thread")
        .join()
        .expect("no overflow");
}

#[test]
fn the_defaults() {
    let limits = Limits::default();
    assert_eq!(limits.max_depth, 100);
    assert_eq!(limits.max_include_depth, 32);
    assert_eq!(limits.max_loop_iterations, 10_000_000);
    assert_eq!(limits.max_operations, 100_000_000);
    assert_eq!(limits.max_output_bytes, 64 << 20);
    assert_eq!(limits.max_allocated_bytes, 1 << 30);
    assert_eq!(limits.max_stack_bytes, 768 << 10);
}

#[test]
fn macro_recursion_stops() {
    let env = env_with(|limits| limits.max_depth = 10);
    assert_limited(
        &env,
        "{% macro m(n) %}{{ m(n + 1) }}{% endmacro %}{{ m(0) }}",
    );
    // Below the cap it runs.
    assert_eq!(
        env.render_str(
            "t.txt",
            "{% macro m(n) %}{% if n %}{{ m(n - 1) }}{% endif %}x{% endmacro %}{{ m(5) }}",
            json!({})
        )
        .expect("renders"),
        "xxxxxx"
    );
}

#[test]
fn every_kind_of_nesting_stops_on_a_small_stack() {
    on_small_stack(|| {
        let mut env = Environment::new();
        env.add_template("i.html", "{% include 'i.html' %}")
            .expect("compiles");
        env.add_template("j.html", "{% import 'j.html' as x %}")
            .expect("compiles");
        env.add_template(
            "b.html",
            "{% block b %}{% include 'c.html' %}{% endblock %}",
        )
        .expect("compiles");
        env.add_template(
            "c.html",
            "{% block c %}{% include 'b.html' %}{% endblock %}",
        )
        .expect("compiles");
        let n = 2000;
        for source in [
            "{% macro m(n) %}{{ m(n + 1) }}{% endmacro %}{{ m(0) }}".to_owned(),
            "{% macro m(n) %}{{ [m(n + 1)]|join('')|upper|lower|replace('a', 'b') }}{% endmacro %}{{ m(0) }}".to_owned(),
            format!(
                "{{% macro m(n) %}}{}{{{{ (((m(n + 1) ~ 'a') ~ 'b') ~ 'c') }}}}{}{{% endmacro %}}{{{{ m(0) }}}}",
                "{% if true %}{% for x in [1] %}{% with y = 1 %}".repeat(20),
                "{% endwith %}{% endfor %}{% endif %}".repeat(20),
            ),
            "{% macro m() %}{% call m() %}x{% endcall %}{{ caller() }}{% endmacro %}{% call m() %}{% endcall %}".to_owned(),
            "{% include 'i.html' %}".to_owned(),
            "{% import 'j.html' as x %}".to_owned(),
            "{% include 'b.html' %}".to_owned(),
            format!("{{{{ {}1{} }}}}", "(".repeat(n), ")".repeat(n)),
            format!("{{{{ {}1{} }}}}", "[".repeat(n), "]".repeat(n)),
            format!("{{{{ {}1{} }}}}", "{1:".repeat(n), "}".repeat(n)),
            format!("{{{{ {}1{} }}}}", "dict(".repeat(n), ")".repeat(n)),
            format!("{{{{ {}1 }}}}", "not ".repeat(n)),
            format!("{{{{ {}1 }}}}", "-".repeat(n)),
            format!("{{{{ 1{} }}}}", "|string".repeat(n)),
            format!("{{{{ 1{} }}}}", "+1".repeat(n)),
            format!("{{{{ 1{} }}}}", "~1".repeat(n)),
            format!("{{{{ a{} }}}}", ".b".repeat(n)),
            format!("{{{{ a{} }}}}", "[0]".repeat(n)),
            format!("{{{{ a{} }}}}", "()".repeat(n)),
            format!("{{{{ a{} }}}}", " is defined".repeat(n)),
            format!("{{{{ 1{} }}}}", " if 1".repeat(n)),
            format!("{{{{ 1{} }}}}", " if 1 else 2".repeat(n)),
            format!("{}x{}", "{% if true %}".repeat(n), "{% endif %}".repeat(n)),
            format!("{}x{}", "{% for a in [1] %}".repeat(n), "{% endfor %}".repeat(n)),
            format!("{}x{}", "{% with a = 1 %}".repeat(n), "{% endwith %}".repeat(n)),
            format!("{}x{}", "{% set a %}".repeat(n), "{% endset %}".repeat(n)),
            format!("{}x{}", "{% filter upper %}".repeat(n), "{% endfilter %}".repeat(n)),
            format!("{}x{}", "{% autoescape true %}".repeat(n), "{% endautoescape %}".repeat(n)),
            format!("{}x{}", "{% macro a() %}".repeat(n), "{% endmacro %}".repeat(n)),
            format!(
                "{{% macro m() %}}{}{{% endmacro %}}",
                "{% call m() %}".repeat(n) + &"{% endcall %}".repeat(n)
            ),
            format!("{{% if false %}}{}{{% endif %}}", "{% elif false %}".repeat(n)),
            format!("{{% for {} in x %}}{{% endfor %}}", "(a,".repeat(n)),
            "{% set x = [1] %}{% for i in range(1000) %}{% set x = [x] %}{% endfor %}{{ x }}".to_owned(),
        ] {
            let shown: String = source.chars().take(80).collect();
            let error = env
                .render_str("t.html", &source, json!({"a": {}}))
                .expect_err(&shown);
            assert!(
                matches!(
                    error.kind(),
                    ErrorKind::LimitExceeded
                        | ErrorKind::SyntaxError
                        | ErrorKind::BadInclude
                ),
                "{shown}: {error}"
            );
        }
    });
}

#[test]
fn includes_stop() {
    let mut env = env_with(|limits| limits.max_include_depth = 5);
    env.add_template("i.txt", "{% include 'i.txt' %}")
        .expect("compiles");
    let error = env
        .get_template("i.txt")
        .and_then(|template| template.render(json!({})))
        .expect_err("recursion");
    assert_eq!(error.kind(), ErrorKind::BadInclude);
    // The chain of causes ends in the limit.
    let mut source: &dyn std::error::Error = &error;
    while let Some(next) = source.source() {
        source = next;
    }
    assert!(source.to_string().contains("limit exceeded"), "{source}");
}

#[test]
fn an_extends_chain_stops() {
    let mut env = env_with(|limits| limits.max_include_depth = 3);
    for (name, parent) in [
        ("a.txt", "b.txt"),
        ("b.txt", "c.txt"),
        ("c.txt", "d.txt"),
        ("d.txt", "e.txt"),
    ] {
        env.add_template(name, format!("{{% extends '{parent}' %}}"))
            .expect("compiles");
    }
    env.add_template("e.txt", "end").expect("compiles");
    let error = env
        .get_template("a.txt")
        .and_then(|template| template.render(json!({})))
        .expect_err("too long");
    assert_eq!(error.kind(), ErrorKind::LimitExceeded, "{error}");
    let error = env
        .get_template("b.txt")
        .and_then(|template| template.render(json!({})))
        .expect("three parents are allowed");
    assert_eq!(error, "end");
}

#[test]
fn loops_stop() {
    let env = env_with(|limits| limits.max_loop_iterations = 1_000);
    assert_limited(
        &env,
        "{% for a in range(100) %}{% for b in range(100) %}{% endfor %}{% endfor %}",
    );
    assert_eq!(
        env.render_str(
            "t.txt",
            "{% for a in range(10) %}{% for b in range(10) %}x{% endfor %}{% endfor %}",
            json!({})
        )
        .expect("110 iterations are allowed")
        .len(),
        100
    );
    // A loop filter counts too.
    assert_limited(&env, "{% for a in range(5000) if false %}{% endfor %}");
}

#[test]
fn work_stops_even_without_loops_or_output() {
    let env = env_with(|limits| limits.max_operations = 5_000);
    assert_limited(
        &env,
        "{% for a in range(1000) %}{% if a == 0 or a == 1 or a == 2 %}{% endif %}{% endfor %}",
    );
    // Going through a large list costs its items, wherever it happens.
    assert_limited(
        &env,
        "{% set big = range(4000) %}{% for i in range(3) %}{{ -1 in big }}{{ big|join('') }}{% endfor %}",
    );
}

#[test]
fn output_stops() {
    let env = env_with(|limits| limits.max_output_bytes = 1 << 10);
    assert_limited(&env, "{% for a in range(5000) %}x{% endfor %}");
    assert_limited(&env, "{{ 'x' * 2000 }}");
    assert_limited(&env, "{% set s = 'x' * 1000 %}{{ s }}{{ s }}");
    assert_limited(&env, "{% set s = 'x' * 1000 %}{{ s ~ s }}");
    assert_limited(&env, "{% set s = 'x' * 600 %}{{ [s, s]|join('') }}");
    assert_limited(
        &env,
        "{% set s = 'x' * 600 %}{% set t %}{{ s }}{{ s }}{% endset %}",
    );
    assert_limited(&env, "{% set s = 'x' * 100 %}{{ s|replace('x', s) }}");
    assert_limited(
        &env,
        "{% macro m() %}{{ 'x' * 800 }}{{ 'x' * 800 }}{% endmacro %}{{ m() }}",
    );
    assert_eq!(
        env.render_str("t.txt", "{{ 'x' * 1024 }}", json!({}))
            .expect("exactly the limit")
            .len(),
        1024
    );
}

#[test]
fn sequences_stop() {
    let env = env_with(|limits| limits.max_output_bytes = 16 * 1000);
    // 1,000 items at these limits.
    assert_limited(&env, "{{ [1] * 2000 }}");
    assert_limited(&env, "{% set x = range(600) %}{{ x + x }}");
    assert_limited(
        &env,
        "{% set x = [1] %}{% for i in range(40) %}{% set x = x + x %}{% endfor %}",
    );
    assert_limited(
        &env,
        "{% set x = [1] %}{% for i in range(40) %}{% set x = [x, x] %}{% endfor %}{{ x == x }}",
    );
    assert_limited(
        &env,
        "{% set x = {} %}{% for i in range(40) %}{% set x = {'a': x, 'b': x} %}{% endfor %}{{ x }}",
    );
    assert_limited(
        &env,
        "{% set x = 1 %}{% for i in range(40) %}{% set x = (x, x) %}{% endfor %}{{ x|tojson }}",
    );
    assert_limited(
        &env,
        "{% set x = 1 %}{% for i in range(40) %}{% set x = dict(a=x, b=x) %}{% endfor %}{{ x }}",
    );
}

#[test]
fn values_nest_at_most_two_hundred_deep() {
    let env = env_with(|_| {});
    assert!(
        env.render_str(
            "t.txt",
            "{% set x = 1 %}{% for i in range(190) %}{% set x = [x] %}{% if loop.last %}{{ x|tojson }}{{ x == x }}{{ x }}{% endif %}{% endfor %}",
            json!({})
        )
        .is_ok()
    );
    assert_limited(
        &env,
        "{% set x = 1 %}{% for i in range(250) %}{% set x = [x] %}{% endfor %}",
    );
    // A context that nests too deeply is refused too.
    let mut deep = json!(1);
    for _ in 0..120 {
        deep = json!([deep]);
    }
    let mut deeper = deep.clone();
    for _ in 0..120 {
        deeper = json!({ "a": deeper });
    }
    assert!(env.render_str("t.txt", "x", json!({ "v": deep })).is_ok());
    let error = env
        .render_str("t.txt", "x", json!({ "v": deeper }))
        .expect_err("too deep");
    assert_eq!(error.kind(), ErrorKind::LimitExceeded);
}

#[test]
fn range_is_capped() {
    let env = env_with(|_| {});
    let error = env
        .render_str("t.txt", "{{ range(100001) }}", json!({}))
        .expect_err("too many");
    assert_eq!(error.kind(), ErrorKind::InvalidOperation);
    assert!(
        env.render_str("t.txt", "{{ range(100000)|length }}", json!({}))
            .is_ok()
    );
    for source in [
        "{{ range(-9223372036854775807, 9223372036854775807) }}",
        "{{ range(9223372036854775807, -9223372036854775807, -1) }}",
        "{{ range(0, 9223372036854775807, 1) }}",
    ] {
        assert!(
            env.render_str("t.txt", source, json!({})).is_err(),
            "{source}"
        );
    }
}

#[test]
fn nesting_in_the_source_is_capped_when_it_compiles() {
    let mut env = Environment::new();
    let deep = format!(
        "{}x{}",
        "{% if a %}".repeat(5000),
        "{% endif %}".repeat(5000)
    );
    let error = env.add_template("t.txt", &deep).expect_err("too deep");
    assert_eq!(error.kind(), ErrorKind::SyntaxError);
    assert_eq!(error.name(), Some("t.txt"));
    // A long flat template is fine.
    env.add_template("flat.txt", "{{ a }}".repeat(100_000))
        .expect("flat");
}

#[test]
fn a_tiny_stack_budget_ends_in_an_error() {
    // A budget this small is reached by any nesting: the error says so.
    let env = env_with(|limits| limits.max_stack_bytes = 1);
    let error = env
        .render_str("t.txt", "{% if true %}{{ 1 + 1 }}{% endif %}", json!({}))
        .expect_err("over budget");
    assert!(
        matches!(
            error.kind(),
            ErrorKind::SyntaxError | ErrorKind::LimitExceeded
        ),
        "{error}"
    );
}

#[test]
fn what_a_render_allocates_is_capped() {
    let env = env_with(|limits| limits.max_allocated_bytes = 1 << 20);
    // Each of these holds on to what it builds, or builds it again and
    // again, in pieces every one of which is within the other limits.
    assert_limited(
        &env,
        "{% for i in range(100) %}{% set s = 'x' * 100000 %}{% endfor %}",
    );
    assert_limited(
        &env,
        "{% macro m(s) %}{% macro inner() %}{{ s }}{% endmacro %}{% endmacro %}{% for i in range(100) %}{{ m('x' * 100000) }}{% endfor %}",
    );
    assert_limited(
        &env,
        "{% for i in range(100) %}{% set l = range(10000) %}{% endfor %}",
    );
    assert_limited(
        &env,
        "{% macro outer() %}{% macro inner() %}{{ x }}{% endmacro %}{% endmacro %}{% for i in range(100000) %}{{ outer() }}{% endfor %}",
    );
}

#[test]
fn numbers_do_not_overflow_silently() {
    let env = env_with(|_| {});
    for source in [
        "{{ 170141183460469231731687303715884105727 + 1 }}",
        "{{ 170141183460469231731687303715884105727 * 2 }}",
        "{{ -170141183460469231731687303715884105727 - 2 }}",
        "{{ 2 ** 1000 }}",
        "{{ 2 ** 99999999999 }}",
        "{{ 1 / 0 }}",
        "{{ 1 // 0 }}",
        "{{ 1 % 0 }}",
        "{{ 0 ** -1 }}",
    ] {
        let error = env
            .render_str("t.txt", source, json!({}))
            .expect_err(source);
        assert_eq!(
            error.kind(),
            ErrorKind::InvalidOperation,
            "{source}: {error}"
        );
    }
}
