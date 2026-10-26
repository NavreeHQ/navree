//! `validate`: the rules and messages of the extractors, for values that
//! do not come from an HTTP body (GraphQL arguments, message payloads).

use garde::Validate;
use navree_validation::{Messages, validate};
use serde_json::json;

#[derive(Validate)]
struct Line {
    #[garde(length(min = 1))]
    sku: String,
}

#[derive(Validate)]
struct Order {
    #[garde(length(min = 2))]
    items: Vec<String>,
    #[garde(length(min = 2))]
    note: String,
    #[garde(dive)]
    lines: Vec<Line>,
}

fn order() -> Order {
    Order {
        items: vec!["one".to_owned()],
        note: "x".to_owned(),
        lines: vec![Line { sku: String::new() }],
    }
}

#[test]
fn a_valid_value_passes() {
    let order = Order {
        items: vec!["a".to_owned(), "b".to_owned()],
        note: "fine".to_owned(),
        lines: Vec::new(),
    };
    assert!(validate(&order, None, &Messages::default(), "input").is_ok());
}

#[test]
fn failures_are_dotted_paths_with_the_usual_messages() {
    let input = json!({ "items": ["one"], "note": "x", "lines": [{ "sku": "" }] });
    let errors = validate(&order(), Some(&input), &Messages::default(), "input").unwrap_err();
    assert_eq!(
        errors.get("items"),
        Some(&["items must contain at least 2 elements".to_owned()][..])
    );
    assert_eq!(
        errors.get("note"),
        Some(&["note must be longer than or equal to 2 characters".to_owned()][..])
    );
    assert!(errors.get("lines.0.sku").is_some());
}

#[test]
fn the_app_messages_apply() {
    let messages = Messages::default().with_message("note.min.string", "Write a longer note.");
    let errors = validate(&order(), None, &messages, "input").unwrap_err();
    assert_eq!(
        errors.get("note"),
        Some(&["Write a longer note.".to_owned()][..])
    );
}
