//! The pieces the dashboard's pages share: badges, forms, links, the search
//! box and the code blocks.

use std::time::Duration;

use navree_ui::form::csrf_field;
use navree_ui::{
    AttrValue, Badge, BadgeVariant, Button, ButtonVariant, CodeBlock, Element, Empty, Heading,
    Node, Row, Size, Text, Uses, el, icons,
};
use serde_json::Value;

use super::Frame;

/// How often a part of a page that changes redraws itself.
pub(super) const REFRESH: Duration = Duration::from_secs(5);

/// The badge of a job's state or an attempt's outcome: green for what is
/// going well, amber for what will be tried again, red for what failed.
pub(super) fn state_badge(state: &str) -> Badge {
    let variant = match state {
        "succeeded" | "running" => BadgeVariant::Success,
        "released" | "delayed" | "paused" => BadgeVariant::Warning,
        "failed" => BadgeVariant::Danger,
        _ => BadgeVariant::Outline,
    };
    Badge::new(state.to_owned()).variant(variant)
}

/// A link with `label` to `href`.
pub(super) fn link(href: impl Into<AttrValue>, label: impl Into<Node>) -> Element {
    el("a").attr("href", href).child(label)
}

/// A job type or a queue name, in code type.
pub(super) fn code(text: impl Into<String>) -> Node {
    let text: String = text.into();
    Text::code(text).into()
}

/// A heading between the sections of a page.
pub(super) fn section(title: &'static str) -> Node {
    Heading::h2(title).into()
}

/// A hidden field.
fn hidden(name: &'static str, value: impl Into<AttrValue>) -> Element {
    el("input")
        .attr("type", "hidden")
        .attr("name", name)
        .attr("value", value)
}

/// A form that posts to `action` and comes back to `next`. It works with
/// script off, and carries the session's CSRF token. `fields` are the hidden
/// fields it posts (a supervisor, a search).
pub(super) fn action_form(
    frame: &Frame,
    action: String,
    next: &str,
    fields: &[(&'static str, &str)],
    button: Button,
) -> Element {
    el("form")
        .attr("method", "post")
        .attr("action", action)
        .flag("data-ui-form", true)
        .uses(Uses::CORE)
        .children((
            frame.csrf.clone().map(csrf_field),
            hidden("next", next.to_owned()),
            fields
                .iter()
                .map(|&(name, value)| hidden(name, value.to_owned()))
                .collect::<Vec<_>>(),
            button.submit(),
        ))
}

/// A small button for a row's action.
pub(super) fn small(label: &'static str) -> Button {
    Button::outline(label).size(Size::Sm)
}

/// A small button for a row's quieter action.
pub(super) fn small_quiet(label: &'static str) -> Button {
    Button::ghost(label).size(Size::Sm)
}

/// A row of links that choose between views of one page; the one shown is
/// marked.
pub(super) fn choices(label: &'static str, items: Vec<(String, String, bool)>) -> Node {
    let buttons: Vec<Node> = items
        .into_iter()
        .map(|(text, href, current)| {
            let button = Button::new(text)
                .variant(if current {
                    ButtonVariant::Secondary
                } else {
                    ButtonVariant::Ghost
                })
                .size(Size::Sm)
                .href(href);
            if current {
                button.attr("aria-current", "page").into()
            } else {
                button.into()
            }
        })
        .collect();
    el("nav")
        .attr("aria-label", label)
        .child(Row::gap(1).wrap().children(buttons))
        .into()
}

/// A search box: a form that asks the page again as the user types. `keep`
/// are the other query values the page needs to stay as it is.
pub(super) fn search_form(
    action: &str,
    region: &'static str,
    value: &str,
    placeholder: &'static str,
    keep: &[(&'static str, &str)],
) -> Node {
    el("form")
        .attr("role", "search")
        .attr("method", "get")
        .attr("action", action.to_owned())
        .attr("data-ui-target", region)
        .children((
            keep.iter()
                .map(|&(name, value)| hidden(name, value.to_owned()))
                .collect::<Vec<_>>(),
            navree_ui::Input::search("search")
                .value(value)
                .placeholder(placeholder)
                .attr("aria-label", placeholder)
                .autocomplete("off"),
        ))
        .into()
}

/// What a list says when nothing is in it.
pub(super) fn nothing(message: &'static str) -> Node {
    Empty::new(message).icon(icons::INBOX).into()
}

/// A job's payload, as indented JSON.
pub(super) fn payload(value: &Value) -> Node {
    let text = serde_json::to_string_pretty(value).unwrap_or_default();
    CodeBlock::new("json", text).into()
}
