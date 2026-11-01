//! `Limits::max_allocated_bytes` bounds what a render holds, however it holds
//! it. Every template here wants far more than the cap as its input grows; the
//! measured peak must stay at the cap at every scale.
//!
//! Bound on paper: the caps are 2 and 4 MiB and each case stops at its cap
//! when the engine charges first; if it did not, the worst case (one character
//! string per byte of a 1 MiB string, 60 bytes each) would hold 60 MiB. Well
//! below anything that could hurt this machine.

use std::sync::Mutex;

use navree_template::{Environment, ErrorKind, Limits};
use navree_template_peak::{Counting, live, peak_above, reset_peak};
use serde_json::json;

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// The allocator's counters are global: one measurement at a time.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// What a render may hold beyond its cap: scopes and other bookkeeping, of a
/// size that does not depend on the input.
const SLACK: usize = 64 << 10;

fn env(cap: u64, max_output: usize) -> Environment {
    let mut env = Environment::new();
    let mut limits = Limits::default();
    limits.max_allocated_bytes = cap;
    limits.max_output_bytes = max_output;
    env.set_limits(limits);
    env.add_template("macros.html", "{% macro m(u) %}{{ u }}{% endmacro %}")
        .expect("compiles");
    env
}

/// The templates, for a string `s` of `size` bytes the template builds.
fn cases(size: usize) -> Vec<(&'static str, String)> {
    let s = format!("{{% set s = \"x\" * {size} %}}");
    let copies = |filter: &str, count: usize| {
        let mut template = s.clone();
        for index in 0..count {
            template.push_str(&format!("{{% set a{index} = s{filter} %}}"));
        }
        template
    };
    vec![
        ("concat", copies("~''", 60)),
        ("plus", copies("+''", 60)),
        ("lower", copies("|lower", 60)),
        ("trim", copies("|trim", 60)),
        ("tojson", copies("|tojson", 60)),
        ("e", copies("|e", 60)),
        ("replace", copies("|replace('y', 'z')", 60)),
        ("urlencode", copies("|urlencode", 60)),
        ("capitalize", copies("|capitalize", 60)),
        ("slice", copies("[1:]", 60)),
        ("repeat", copies(" * 1", 60)),
        ("join", copies("|split('x')|join('y')", 20)),
        (
            "nested macro buffers",
            format!(
                "{s}{{% macro m(n) %}}{{{{ s }}}}{{% if n %}}{{{{ m(n-1) }}}}{{% endif %}}{{% endmacro %}}{{{{ m(60) }}}}"
            ),
        ),
        ("nested set buffers", {
            let mut template = s.clone();
            for index in 0..30 {
                template.push_str(&format!("{{% set b{index} %}}{{{{ s }}}}"));
            }
            template.push_str(&"{% endset %}".repeat(30));
            template
        }),
        ("split chars", format!("{s}{{% set p = s|split('') %}}")),
        ("split on x", format!("{s}{{% set p = s|split('x') %}}")),
        ("list of chars", format!("{s}{{% set p = s|list %}}")),
        (
            "loop over chars",
            format!("{s}{{% for c in s %}}{{% break %}}{{% endfor %}}"),
        ),
        (
            "loop filter over chars",
            format!("{s}{{% for c in s if false %}}{{% endfor %}}"),
        ),
        (
            "splat of chars",
            format!("{s}{{% macro f() %}}{{% endmacro %}}{{{{ f(*s) }}}}"),
        ),
        ("unpack of chars", format!("{s}{{% set a, b = s %}}")),
        (
            "growing list",
            format!(
                "{{% set l = [0] * {} %}}{{% for i in range(60) %}}{{% set a = l + [i] %}}{{% set b = l|list %}}{{% endfor %}}",
                size / 32
            ),
        ),
        (
            "items",
            format!(
                "{{% set l = range({}) %}}{{% for i in range(60) %}}{{% set a = dict(x=l)|items %}}{{% endfor %}}",
                (size / 32).min(100_000)
            ),
        ),
        (
            "output",
            format!("{s}{{% for i in range(60) %}}{{{{ s }}}}{{% endfor %}}"),
        ),
    ]
}

/// Runs `source` and returns the peak above what was live before.
fn measure(env: &Environment, source: &str) -> (usize, Result<usize, ErrorKind>) {
    let base = live();
    reset_peak();
    let result = env
        .render_str("t.txt", source, json!({}))
        .map(|out| out.len())
        .map_err(|error| error.kind());
    (peak_above(base), result)
}

#[test]
fn what_a_render_holds_stays_at_the_cap_at_every_scale() {
    let _guard = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let mut over = Vec::new();
    for cap in [2_u64 << 20, 4 << 20] {
        for size in [200_000, 400_000, 800_000] {
            let env = env(cap, 1 << 20);
            for (name, source) in cases(size) {
                let (peak, result) = measure(&env, &source);
                let limit = usize::try_from(cap).expect("small") + SLACK;
                println!("{name:24} size {size:>7} cap {cap:>8} peak {peak:>9} {result:?}");
                if peak > limit {
                    over.push(format!("{name}: size {size}, cap {cap}: peak {peak}"));
                }
            }
        }
    }
    assert!(over.is_empty(), "over the cap:\n{}", over.join("\n"));
}

#[test]
fn held_copies_stop_at_the_cap_with_the_default_output_limit() {
    // The same class with the default output limit: a thousand held copies
    // of a 1 MiB string are refused at a 16 MiB cap, not after 1 GiB.
    let _guard = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let mut env = Environment::new();
    let mut limits = env.limits();
    limits.max_allocated_bytes = 16 << 20;
    env.set_limits(limits);
    let mut source = String::from("{% set s = 'x' * 1000000 %}");
    for index in 0..1000 {
        source.push_str(&format!("{{% set a{index} = s|trim %}}"));
    }
    let (peak, result) = measure(&env, &source);
    assert_eq!(result, Err(ErrorKind::LimitExceeded));
    assert!(
        peak <= (16 << 20) + SLACK + source.len() * 64,
        "peak {peak}"
    );
}

#[test]
fn shared_strings_cost_nothing() {
    // `|safe` shares the string it marks: holding the mark holds no copy.
    let _guard = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let env = env(8 << 20, 1 << 20);
    let mut source = String::from("{% set s = 'x' * 500000 %}");
    for index in 0..200 {
        source.push_str(&format!("{{% set a{index} = s|safe %}}"));
    }
    let (peak, result) = measure(&env, &source);
    assert_eq!(result, Ok(0));
    assert!(peak < 4 << 20, "peak {peak}");
}
