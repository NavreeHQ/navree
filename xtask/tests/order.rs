//! The publish order: dependencies first, path-only dev-dependencies
//! ignored, versioned ones counted, cycles named.

mod common;

use common::{member, names, workspace};
use xtask::order::{layers, publish_order};

#[test]
fn dependencies_come_first_in_layers_sorted_by_name() {
    let ws = workspace(&[
        member(
            "navree",
            &[
                ("navree-http", "normal", "=1.0.0"),
                ("navree-kernel", "normal", "=1.0.0"),
            ],
        ),
        member(
            "navree-http",
            &[
                ("navree-kernel", "normal", "=1.0.0"),
                ("tokio", "normal", "^1"),
            ],
        ),
        member("navree-kernel", &[("serde", "normal", "^1")]),
        member("navree-bounded", &[]),
        member("navree-cli", &[("navree", "normal", "=1.0.0")]),
    ]);
    let all = names(&[
        "navree",
        "navree-bounded",
        "navree-cli",
        "navree-http",
        "navree-kernel",
    ]);
    assert_eq!(
        layers(&ws, &all).unwrap(),
        vec![
            names(&["navree-bounded", "navree-kernel"]),
            names(&["navree-http"]),
            names(&["navree"]),
            names(&["navree-cli"]),
        ]
    );
}

#[test]
fn build_dependencies_and_versioned_dev_dependencies_order_the_publish() {
    let ws = workspace(&[
        member("a", &[("b", "build", "=1.0.0")]),
        member("b", &[("c", "dev", "=1.0.0")]),
        member("c", &[]),
    ]);
    assert_eq!(
        publish_order(&ws, &names(&["a", "b", "c"])).unwrap(),
        names(&["c", "b", "a"])
    );
}

#[test]
fn a_path_only_dev_dependency_does_not_order_the_publish() {
    // The macros crate's tests use the HTTP crate, which depends on the
    // macros: path-only, so Cargo drops it and there is no cycle.
    let ws = workspace(&[
        member("navree-macros", &[("navree-http", "dev", "*")]),
        member("navree-http", &[("navree-macros", "normal", "=1.0.0")]),
    ]);
    assert_eq!(
        publish_order(&ws, &names(&["navree-http", "navree-macros"])).unwrap(),
        names(&["navree-macros", "navree-http"])
    );
}

#[test]
fn a_versioned_dev_dependency_cycle_is_named() {
    let ws = workspace(&[
        member("navree-macros", &[("navree-http", "dev", "=1.0.0")]),
        member("navree-http", &[("navree-macros", "normal", "=1.0.0")]),
        member("navree-kernel", &[]),
    ]);
    let all = names(&["navree-http", "navree-kernel", "navree-macros"]);
    assert_eq!(
        layers(&ws, &all).unwrap_err(),
        vec![names(&["navree-http", "navree-macros"])]
    );
    let message = publish_order(&ws, &all).unwrap_err();
    assert!(
        message.contains("navree-http <-> navree-macros"),
        "{message}"
    );
    assert!(message.contains("path-only"), "{message}");
}

#[test]
fn a_crate_that_waits_for_itself_is_a_cycle() {
    // `navree-db = { workspace = true, features = [...] }` in its own
    // dev-dependencies carries the version.
    let ws = workspace(&[member("navree-db", &[("navree-db", "dev", "=1.0.0")])]);
    assert_eq!(
        layers(&ws, &names(&["navree-db"])).unwrap_err(),
        vec![names(&["navree-db"])]
    );

    let fixed = workspace(&[member("navree-db", &[("navree-db", "dev", "*")])]);
    assert_eq!(
        publish_order(&fixed, &names(&["navree-db"])).unwrap(),
        names(&["navree-db"])
    );
}

#[test]
fn a_selection_is_ordered_without_the_crates_left_out() {
    // A partial run: crates outside the selection are on the registry
    // already, so they put no constraint on the order.
    let ws = workspace(&[
        member("a", &[("b", "normal", "=1.0.0"), ("c", "normal", "=1.0.0")]),
        member("b", &[("c", "normal", "=1.0.0")]),
        member("c", &[]),
    ]);
    assert_eq!(
        publish_order(&ws, &names(&["a", "b"])).unwrap(),
        names(&["b", "a"])
    );
}

#[test]
fn optional_and_target_specific_duplicates_count_once() {
    let ws = workspace(&[
        member(
            "a",
            &[
                ("b", "normal", "=1.0.0"),
                ("b", "normal", "=1.0.0"),
                ("b", "dev", "*"),
            ],
        ),
        member("b", &[]),
    ]);
    assert_eq!(
        layers(&ws, &names(&["a", "b"])).unwrap(),
        vec![names(&["b"]), names(&["a"])]
    );
}
