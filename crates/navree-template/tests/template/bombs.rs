//! Small templates with large effects. Each runs at reduced scale and fails on
//! an assertion: a value that hides how it nests, a scan the work budget does
//! not see, or text that grows past a limit while it is built.

use navree_template::{Environment, ErrorKind, Limits, Value};
use serde_json::json;

fn env_with(limits: impl FnOnce(&mut Limits)) -> Environment {
    let mut env = Environment::new();
    let mut current = env.limits();
    limits(&mut current);
    env.set_limits(current);
    env
}

fn on_stack(bytes: usize, check: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(bytes)
        .spawn(check)
        .expect("a thread")
        .join()
        .expect("no overflow");
}

#[test]
fn cycle_refuses_keyword_arguments() {
    let env = env_with(|_| {});
    let error = env
        .render_str(
            "t.txt",
            "{% for i in [1] %}{{ loop.cycle(a=1) }}{% endfor %}",
            json!({}),
        )
        .expect_err("kwargs are not an item");
    assert_eq!(error.kind(), ErrorKind::InvalidOperation, "{error}");
    assert_eq!(error.line(), Some(1));
    // Positional arguments go round as ever.
    assert_eq!(
        env.render_str(
            "t.txt",
            "{% for i in [1, 2, 3] %}{{ loop.cycle('a', 'b') }}{% endfor %}",
            json!({})
        )
        .expect("renders"),
        "aba"
    );
}

#[test]
fn kwargs_cannot_nest_a_value_out_of_sight() {
    // 12,000 levels would overflow the stack when the value is dropped.
    on_stack(1 << 20, || {
        let env = env_with(|_| {});
        for source in [
            "{% set k = 1 %}{% for i in range(12000) %}{% set k = loop.cycle(a=k) %}{% endfor %}",
            "{% set k = 1 %}{% for i in range(12000) %}{% set k = loop.cycle(k, a=k) %}{% endfor %}",
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
    });
}

#[test]
fn kwargs_cannot_share_their_way_past_the_work_budget() {
    let env = env_with(|limits| limits.max_operations = 2_000);
    let source = "{% set k = 1 %}{% for i in range(22) %}{% set k = loop.cycle(a=k, b=k) %}{% if loop.last %}{{ k == k }}{% endif %}{% endfor %}";
    assert!(env.render_str("t.txt", source, json!({})).is_err());
}

#[test]
fn modules_cannot_nest_a_value_out_of_sight() {
    on_stack(1 << 20, || {
        let mut env = env_with(|_| {});
        env.add_template("mod.txt", "{% set y = m %}")
            .expect("compiles");
        let error = env
            .render_str(
                "t.txt",
                "{% for i in range(12000) %}{% import \"mod.txt\" as m %}{% endfor %}done",
                json!({}),
            )
            .expect_err("too deep");
        assert_eq!(error.kind(), ErrorKind::LimitExceeded, "{error}");
        assert!(error.to_string().contains("nests too deeply"), "{error}");
    });
}

#[test]
fn modules_cannot_share_their_way_past_the_work_budget() {
    let mut env = env_with(|limits| limits.max_operations = 5_000);
    env.add_template("mod.txt", "{% set a = m %}{% set b = m %}")
        .expect("compiles");
    let source = "{% set m = 1 %}{% for i in range(40) %}{% import \"mod.txt\" as m %}{% if loop.last %}{{ m == m }}{% endif %}{% endfor %}";
    let error = env
        .render_str("t.txt", source, json!({}))
        .expect_err("over budget");
    assert_eq!(error.kind(), ErrorKind::LimitExceeded, "{error}");
}

/// Budget 8,000 operations, 300 iterations: each body runs out of budget with
/// a large value, and fits with a small one.
#[test]
fn every_scan_is_charged_by_its_size() {
    let env = env_with(|limits| limits.max_operations = 8_000);
    let context = |n: usize, items: usize| {
        json!({
            "s": "x".repeat(n),
            "t": "x".repeat(n),
            "sp": " ".repeat(n),
            "l": vec![1; items],
            "m": vec![1; items],
        })
    };
    for body in [
        "{% set a = s|length %}",
        "{% set a = s[0] %}",
        "{% set a = s[-1] %}",
        "{% set a = s is lower %}",
        "{% set a = s is upper %}",
        "{% set a = s|trim %}",
        "{% set a = sp|trim %}",
        "{% set a = s is eq(t) %}",
        "{% set a = s is ne(t) %}",
        "{% set a = s is lt(t) %}",
        "{% set a = s is sameas(t) %}",
        "{% set a = s is startingwith(t) %}",
        "{% set a = s is endingwith(t) %}",
        "{% set a = s|int(0) %}",
        "{% set a = l is eq(m) %}",
        "{% set a = 2 is in(l) %}",
        "{% set a = loop.changed(l) %}{% set a = loop.changed(m) %}",
        "{% set a = [s, t]|min %}",
    ] {
        let source = format!("{{% for i in range(300) %}}{body}{{% endfor %}}");
        // Sizes far past what the budget pays for in 300 iterations.
        for (n, items) in [(1 << 20, 1 << 16), (1 << 21, 1 << 17)] {
            let error = env
                .render_str("t.txt", &source, context(n, items))
                .expect_err(body);
            assert_eq!(error.kind(), ErrorKind::LimitExceeded, "{body}: {error}");
        }
        assert!(
            env.render_str("t.txt", &source, context(16, 4)).is_ok(),
            "{body} fits with small values"
        );
    }
}

/// What takes no scan fits at any size: a string's mark is on a shared string,
/// and a loop over a sequence goes through only the items it reaches.
#[test]
fn what_does_not_scan_is_not_charged() {
    let env = env_with(|limits| limits.max_operations = 8_000);
    let context = json!({ "s": "x".repeat(1 << 20), "l": vec![1; 1 << 17] });
    for body in [
        "{% set a = s|safe %}",
        "{% for x in l %}{% break %}{% endfor %}",
        "{% set a = l|length %}",
        "{% set a = l|first %}",
    ] {
        let source = format!("{{% for i in range(300) %}}{body}{{% endfor %}}");
        assert!(env.render_str("t.txt", &source, &context).is_ok(), "{body}");
    }
}

/// What a filter's output can grow to, against the output limit.
#[test]
fn text_stays_within_the_output_limit_while_it_grows() {
    let limit = 1 << 16;
    let env = env_with(|limits| limits.max_output_bytes = limit);
    // Each character grows: ΐ (2 bytes) to 6 in upper case, İ (2) to 3 in
    // lower case, `<` (1) to 6 in JSON, in strings the limit lets through.
    for (value, source) in [
        ("\u{390}".repeat(limit / 4), "{% set a = s|upper %}"),
        ("\u{130}".repeat(limit / 3 + 1), "{% set a = s|lower %}"),
        ("<".repeat(limit / 2), "{% set a = s|tojson %}"),
        ("<".repeat(limit / 2), "{% set a = [s]|tojson %}"),
        ("\u{1}".repeat(limit / 2), "{% set a = s|tojson %}"),
        (
            "<".repeat(limit / 2),
            "{% set a = {'k': s}|tojson(indent=2) %}",
        ),
        ("<".repeat(limit / 2), "{% set a = s|urlencode %}"),
    ] {
        let error = env
            .render_str("t.txt", source, json!({ "s": value }))
            .expect_err(source);
        assert_eq!(error.kind(), ErrorKind::LimitExceeded, "{source}: {error}");
    }
    // And what fits still builds, to the byte.
    let value = "\u{390}".repeat(limit / 8);
    assert_eq!(
        env.render_str("t.txt", "{{ s|upper }}", json!({ "s": value }))
            .expect("fits")
            .len(),
        value.len() * 3
    );
}

#[test]
fn a_registered_function_can_return_what_it_built() {
    // Registered functions own what they allocate: their result still has to
    // pass the caps on values.
    let mut env = env_with(|_| {});
    env.add_function("big", || -> Value {
        Value::from_iter((0..10).map(|n| n.to_string()))
    });
    assert_eq!(
        env.render_str("t.txt", "{{ big()|length }}", json!({}))
            .expect("renders"),
        "10"
    );
}
