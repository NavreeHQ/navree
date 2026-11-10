//! One stack budget per thread, measured against what the thread has left.
//! Sizes are small: the threads are 1 or 2 MiB.

use std::sync::{Arc, OnceLock};

use navree_template::{Environment, Error, ErrorKind, TemplateSource, Value};
use serde_json::json;

fn on_stack(bytes: usize, check: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(bytes)
        .spawn(check)
        .expect("a thread")
        .join()
        .expect("no overflow");
}

/// Uses about `bytes` of stack, then calls `then`.
#[inline(never)]
fn burn(bytes: usize, then: &mut dyn FnMut()) {
    let mut pad = [0_u8; 4096];
    std::hint::black_box(&mut pad);
    if bytes > 4096 {
        burn(bytes - 4096, then);
    } else {
        then();
    }
    std::hint::black_box(&mut pad);
}

/// A macro that nests `levels` deep, with `wrap` list levels around each call:
/// a lot of stack for each level of macro calls.
fn deep_macro(levels: usize, wrap: usize, bottom: &str) -> String {
    let open = "[".repeat(wrap);
    let close = "]".repeat(wrap);
    format!(
        "{{% macro m(n) %}}{{% if n %}}{{% set r = {open}m(n-1){close} %}}{{% else %}}{bottom}{{% endif %}}{{% endmacro %}}{{{{ m({levels}) }}}}"
    )
}

/// Whether the render ended the way a stack budget ends one.
fn stopped_or_finished(result: Result<String, Error>) {
    if let Err(error) = result {
        assert!(
            matches!(
                error.kind(),
                ErrorKind::LimitExceeded | ErrorKind::BadInclude | ErrorKind::SyntaxError
            ),
            "{error}"
        );
    }
}

#[test]
fn a_render_that_starts_with_the_stack_partly_used_stops_in_time() {
    for used in [100 << 10, 300 << 10, 500 << 10, 700 << 10] {
        on_stack(1 << 20, move || {
            burn(used, &mut || {
                let source = deep_macro(100, 140, "x");
                stopped_or_finished(Environment::new().render_str("t.txt", &source, json!({})));
            });
        });
    }
}

#[test]
fn a_compile_during_a_render_shares_the_renders_budget() {
    on_stack(1 << 20, || {
        let mut env = Environment::new();
        env.set_loader(|name| {
            Ok((name == "deep.txt").then(|| {
                TemplateSource::new(format!("{{{{ {}1{} }}}}", "[".repeat(140), "]".repeat(140)))
            }))
        });
        let source = deep_macro(100, 140, "{% include 'deep.txt' %}");
        stopped_or_finished(env.render_str("t.txt", &source, json!({})));
        // At each depth the budget leaves, the compile still ends in an error
        // and not an overflow.
        for levels in (5..100).step_by(5) {
            for wrap in (10..=140).step_by(10) {
                let source = deep_macro(levels, wrap, "{% include 'deep.txt' %}");
                stopped_or_finished(env.render_str("t.txt", &source, json!({})));
            }
        }
    });
}

#[test]
fn a_render_inside_a_render_shares_its_budget() {
    on_stack(2 << 20, || {
        let shared = Arc::new(OnceLock::<Environment>::new());
        let inner = shared.clone();
        let mut env = Environment::new();
        env.add_function("partial", move |n: i64| -> Result<Value, Error> {
            let env = inner.get().expect("set");
            env.render_str(
                "p.txt",
                "{{ partial(n - 1) if n else 'end' }}",
                json!({ "n": n }),
            )
            .map(Value::from)
        });
        assert!(shared.set(env).is_ok());
        let error = shared
            .get()
            .expect("set")
            .render_str("t.txt", "{{ partial(100000) }}", json!({}))
            .expect_err("too deep");
        // Whichever runs out first: a render, or the compile of the next one.
        assert!(
            matches!(
                error.kind(),
                ErrorKind::LimitExceeded | ErrorKind::SyntaxError
            ),
            "{error}"
        );
    });
}

#[test]
fn the_budget_comes_back_when_the_outermost_render_ends() {
    on_stack(1 << 20, || {
        let env = Environment::new();
        let source = "{% macro m(n) %}{% if n %}{{ m(n - 1) }}{% endif %}{% endmacro %}{{ m(50) }}";
        for _ in 0..3 {
            let _ = env.render_str("t.txt", &deep_macro(100, 140, "x"), json!({}));
            assert!(env.render_str("t.txt", source, json!({})).is_ok());
        }
    });
}

/// At the deepest a render gets, a walk over a value 199 deep still has room.
#[test]
fn walking_a_value_at_the_edge_of_the_budget_does_not_overflow() {
    fn nested(depth: usize) -> serde_json::Value {
        let mut value = json!(1);
        for _ in 0..depth {
            value = serde_json::Value::Array(vec![value]);
        }
        value
    }
    for bottom in [
        "{{ v }}",
        "{{ v|e }}",
        "{{ v|tojson(indent=1) }}",
        "{{ v == w }}",
        "{% set c = [v] %}",
    ] {
        on_stack(1 << 20, move || {
            let context = json!({ "v": nested(199), "w": nested(199) });
            for wrap in [30, 60, 90, 120, 140] {
                let source = deep_macro(95, wrap, bottom);
                stopped_or_finished(Environment::new().render_str("t.html", &source, &context));
            }
        });
    }
}
