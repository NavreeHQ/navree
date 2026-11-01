//! The engine's limits, set through `Views::configure`, in template files and
//! in templates kept as text.

use navree_view::{ViewError, Views, template};
use serde_json::json;

#[test]
fn views_apply_the_limits_they_are_configured_with() {
    let mut views = Views::from_templates([("a.txt", "{% for i in range(100) %}x{% endfor %}")])
        .expect("views");
    assert_eq!(views.render("a", json!({})).expect("renders").len(), 100);

    views.configure(|env| {
        let mut limits = env.limits();
        limits.max_loop_iterations = 10;
        env.set_limits(limits);
    });

    for error in [
        views.render("a", json!({})).expect_err("over the limit"),
        views
            .render_source("b.txt", "{% for i in range(100) %}{% endfor %}", json!({}))
            .expect_err("over the limit"),
    ] {
        let ViewError::Template { source, .. } = error else {
            panic!("a template error");
        };
        assert_eq!(source.kind(), template::ErrorKind::LimitExceeded);
        assert_eq!(source.line(), Some(1));
    }
}

#[test]
fn the_default_limits_stop_a_recursive_macro() {
    let views = Views::from_templates([(
        "a.txt",
        "{% macro m(n) %}{{ m(n + 1) }}{% endmacro %}{{ m(0) }}",
    )])
    .expect("views");
    let error = views.render("a", json!({})).expect_err("recursion");
    assert!(error.to_string().contains("limit exceeded"), "{error}");
}
