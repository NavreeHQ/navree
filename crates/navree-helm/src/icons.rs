//! The panel's icons: a set of common [Lucide](https://lucide.dev) icons
//! (ISC License, Copyright (c) Lucide Icons and Contributors), embedded so
//! the panel makes no external requests. Each is drawn inline as a 16px
//! SVG with a 1.75 stroke, decorative (`aria-hidden`): the text next to it
//! names the link or button.

use std::collections::HashMap;
use std::sync::LazyLock;

use serde_json::Value;

/// One icon per line: its name, a tab, the SVG elements of its 24x24
/// drawing.
const ICONS: &str = include_str!("assets/icons.txt");

/// The icon of a resource or page whose `ICON` is empty or not embedded.
pub(crate) const FALLBACK: &str = "folder";

static SET: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    ICONS
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .collect()
});

/// Whether the panel embeds icon `name`.
pub(crate) fn exists(name: &str) -> bool {
    SET.contains_key(name)
}

/// Icon `name` (the fallback when it isn't embedded) as an inline SVG.
pub(crate) fn svg(name: &str) -> String {
    let inner = SET
        .get(name)
        .or_else(|| SET.get(FALLBACK))
        .copied()
        .unwrap_or_default();
    format!(
        "<svg class=\"icon\" xmlns=\"http://www.w3.org/2000/svg\" width=\"16\" height=\"16\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.75\" stroke-linecap=\"round\" stroke-linejoin=\"round\" aria-hidden=\"true\" focusable=\"false\">{inner}</svg>"
    )
}

/// The icons the panel's own templates use: `icons.<key>` in their context.
const UI: [(&str, &str); 18] = [
    ("brand", "ship-wheel"),
    ("dashboard", "layout-dashboard"),
    ("sidebar", "panel-left"),
    ("chevrons", "chevrons-up-down"),
    ("chevron_right", "chevron-right"),
    ("log_out", "log-out"),
    ("search", "search"),
    ("close", "x"),
    ("plus", "plus"),
    ("pencil", "pencil"),
    ("trash", "trash-2"),
    ("filter", "filter"),
    ("check", "check"),
    ("user", "user"),
    ("download", "download"),
    ("circle_plus", "circle-plus"),
    ("trend_up", "trending-up"),
    ("trend_down", "trending-down"),
];

/// [`UI`] drawn, as a JSON object.
pub(crate) fn ui() -> &'static Value {
    static DRAWN: LazyLock<Value> = LazyLock::new(|| {
        Value::Object(
            UI.iter()
                .map(|(key, name)| ((*key).to_owned(), Value::String(svg(name))))
                .collect(),
        )
    });
    &DRAWN
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_are_inline_and_decorative() {
        assert!(exists("handshake"));
        assert!(exists(FALLBACK));
        assert!(!exists("no-such-icon"));
        let handshake = svg("handshake");
        assert!(handshake.starts_with("<svg class=\"icon\""), "{handshake}");
        assert!(handshake.contains("aria-hidden=\"true\""));
        assert!(handshake.contains("stroke-width=\"1.75\""));
        assert!(handshake.contains("<path d=\"m11 17 2 2a1 1 0 1 0 3-3\"/>"));
        assert!(!handshake.contains("style="), "no inline styles");
        // Unknown names fall back to a generic icon.
        assert_eq!(svg("no-such-icon"), svg(FALLBACK));
        for (key, name) in UI {
            assert!(exists(name), "{key}: {name}");
            assert_eq!(ui()[key], svg(name));
        }
    }
}
