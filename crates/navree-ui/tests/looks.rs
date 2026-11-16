//! The looks a component takes only when a page asks for one: what each writes, what it
//! links, how assistive technology reads it, and that the sheet they share draws nothing for a
//! part drawn as it always was (`component_looks.rs` holds that markup).

use navree_ui::assets::{self, get};
use navree_ui::*;

fn html(node: impl Into<Node>) -> String {
    render(&node.into())
}

/// The stylesheet's source the looks are drawn by.
fn looks_source() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("styles/39-looks.css");
    std::fs::read_to_string(path)
        .expect("the looks sheet")
        .replace("\r\n", "\n")
}

/// The attribute contract every component keeps, and the sheet a look links.
fn keeps_the_contract(name: &str, node: Node) {
    let out = render(&node);
    assert!(!out.contains("style="), "{name}: {out}");
    assert!(!out.contains("class="), "{name}: {out}");
    assert!(!out.contains(" onclick"), "{name}: {out}");
    let page = View::new("x").body(node);
    assert!(
        page.uses().has(Uses::LOOKS),
        "{name} links the looks sheet: {:?}",
        page.uses()
    );
    assert!(page.to_html().contains(assets::LOOKS_URL), "{name}");
}

fn mode(current: &str) -> ButtonGroup {
    ButtonGroup::new("Mode").segmented().block().children((
        Button::new("Mine")
            .href("/deals/mine")
            .current(current == "mine"),
        Button::new("Team")
            .href("/deals/team")
            .current(current == "team"),
    ))
}

#[test]
fn a_segmented_group_of_links_is_a_named_group_of_real_links_with_one_current() {
    let out = html(mode("team"));
    assert!(
        out.starts_with(r#"<div data-slot="button-group" role="group" aria-label="Mode" data-variant="segmented" data-block>"#),
        "{out}"
    );
    assert_eq!(out.matches("<a ").count(), 2, "{out}");
    assert_eq!(out.matches("<button").count(), 0, "{out}");
    assert!(out.contains(r#"href="/deals/mine">Mine</a>"#), "{out}");
    assert!(
        out.contains(r#"<a data-slot="button" aria-current="page" href="/deals/team">Team</a>"#),
        "{out}"
    );
    assert_eq!(out.matches("aria-current").count(), 1, "{out}");
    // No script: the links work as links, with a link's keys.
    let page = View::new("x").body(mode("mine"));
    assert_eq!(page.uses(), Uses::LOOKS);
    assert!(!page.to_html().contains("<script"));
    keeps_the_contract("segmented", mode("mine").into());
    keeps_the_contract(
        "block",
        ButtonGroup::new("View")
            .block()
            .children(Button::new("List"))
            .into(),
    );
}

#[test]
fn a_current_button_says_so_and_one_that_is_not_writes_nothing() {
    assert!(html(Button::new("Mine").href("/m").current(true)).contains(r#"aria-current="page""#));
    assert!(
        html(Button::new("Now").current(true))
            .starts_with(r#"<button data-slot="button" aria-current="page" type="button">"#)
    );
    assert_eq!(
        html(Button::new("Mine").href("/m").current(false)),
        html(Button::new("Mine").href("/m"))
    );
    // A current button on its own needs no sheet of its own.
    assert!(
        View::new("x")
            .body(Button::new("Mine").href("/m").current(true))
            .uses()
            .is_none()
    );
}

#[test]
fn the_segmented_group_reads_the_segment_roles_toggle_corners_and_control_heights() {
    let css = looks_source();
    for read in [
        "background:var(--segment-track,var(--ink-2))",
        "background:var(--segment-on,var(--ink-3))",
        "border-radius:var(--radius-toggle,var(--radius-lg))",
        "border-radius:var(--radius-toggle,var(--radius))",
        "height:calc(var(--control-md,2.25rem) - .375rem)",
        "height:calc(var(--control-sm,1.875rem) - .375rem)",
        "height:calc(var(--control-lg,2.625rem) - .375rem)",
    ] {
        assert!(css.contains(read), "{read}");
    }
    // The current one is raised by the rule every current button has.
    let core = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("styles/10-controls.css"),
    )
    .unwrap();
    assert!(core.contains(
        "&[aria-current]{background:var(--ink-3);color:var(--fg);box-shadow:var(--shadow-raised)}"
    ));
}

/// The hooks the looks sheet's rules are keyed on: each a value a page writes only when it asks
/// for that look, so none of them reaches a part drawn as it always was.
const HOOKS: [&str; 2] = [
    "[data-slot=button-group][data-variant=segmented]",
    "[data-slot=button-group][data-block]",
];

#[test]
fn every_rule_of_the_looks_sheet_is_keyed_on_a_look_a_page_asks_for() {
    let css = looks_source();
    let mut body = String::new();
    let mut rest = css.as_str();
    // The source without its comments.
    while let Some(at) = rest.find("/*") {
        body.push_str(&rest[..at]);
        rest = rest[at..].split_once("*/").map_or("", |(_, after)| after);
    }
    body.push_str(rest);
    // Each rule at the top, or at the top of a media or container block: its selector, up
    // to its block.
    let mut open: Vec<bool> = Vec::new();
    let mut selector = String::new();
    let mut rules = 0;
    for c in body.chars() {
        let top = open.iter().all(|at_rule| *at_rule);
        match c {
            '{' => {
                let found = selector.trim().to_owned();
                selector.clear();
                let at_rule = top && found.starts_with('@');
                if top && !at_rule {
                    rules += 1;
                    assert!(
                        found
                            .split(',')
                            .all(|one| HOOKS.iter().any(|hook| one.trim().starts_with(hook))),
                        "{found} is keyed on no look"
                    );
                }
                open.push(at_rule);
            }
            '}' => {
                open.pop();
                selector.clear();
            }
            ';' => selector.clear(),
            _ if top => selector.push(c),
            _ => {}
        }
    }
    assert!(rules >= 2, "the sheet was read: {rules}");
    // And the sheet every page loads holds none of them.
    let shared = std::str::from_utf8(get("navree-ui").unwrap().bytes).unwrap();
    for hook in HOOKS {
        assert!(
            !shared.contains(hook),
            "{hook} in the sheet every page loads"
        );
    }
}
