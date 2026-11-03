//! The markup of a list's options: the listbox's `role="option"` rows, and the
//! `<option>`s of the native `<select>` that carries the value. The same writer
//! draws an inline list and a server's answer ([`ChoiceList`](super::ChoiceList)),
//! so the runtime reads one shape.

use std::collections::HashSet;

use super::choice::{Choice, Entries, Entry};
use crate::components::icon;
use crate::html::Href;
use crate::node::{Element, Node, el};

/// One row of the listbox.
///
/// A plain choice is one element with its label as text. A choice with a description,
/// an icon or a picture has a body. `multiple` lists mark every row selected or not,
/// as a checkbox list needs; a single list marks only the chosen row.
pub(crate) fn option_element(choice: &Choice, selected: bool, multiple: bool) -> Element {
    let row = match &choice.href {
        Some(href) => el("a")
            .attr("role", "option")
            .attr("href", Href::from(href.as_str()))
            .slot("command-item"),
        None => el("div").attr("role", "option"),
    }
    .attr("data-value", choice.value.clone());
    let row = if selected {
        row.attr("aria-selected", "true")
    } else if multiple {
        row.attr("aria-selected", "false")
    } else {
        row
    };
    let row = row
        .attr_opt("aria-disabled", choice.disabled.then_some("true"))
        .attr_opt("data-keywords", choice.keyword_text())
        .attr_opt("data-selected-label", choice.selected_label.clone());
    if !choice.is_rich() {
        return row.attr("dir", "auto").child(choice.label.clone());
    }
    let avatar = choice.avatar.as_ref().map(|src| {
        el("img")
            .slot("option-avatar")
            .attr("src", Href::from(src.as_str()))
            .attr("alt", "")
            .attr("loading", "lazy")
    });
    row.children((
        choice.icon.map(icon),
        avatar,
        el("span").slot("option-body").children((
            el("span")
                .slot("option-text")
                .attr("dir", "auto")
                .child(choice.label.clone()),
            choice.description.clone().map(|text| {
                el("span")
                    .slot("option-description")
                    .attr("dir", "auto")
                    .child(text)
            }),
        )),
    ))
}

/// The rows for `entries`, groups and dividers included.
pub(crate) fn rows(entries: &Entries, chosen: &HashSet<&str>, multiple: bool) -> Vec<Node> {
    let mut out = Vec::with_capacity(entries.0.len());
    for entry in &entries.0 {
        match entry {
            Entry::Choice(choice) => out.push(
                option_element(choice, chosen.contains(choice.value.as_str()), multiple).into(),
            ),
            Entry::Group { label, choices } => {
                let rows: Vec<Node> = choices
                    .iter()
                    .map(|choice| {
                        option_element(choice, chosen.contains(choice.value.as_str()), multiple)
                            .into()
                    })
                    .collect();
                out.push(
                    el("div")
                        .attr("role", "group")
                        .attr("aria-label", label.clone())
                        .children((
                            el("div")
                                .slot("option-group-label")
                                .attr("aria-hidden", "true")
                                .child(label.clone()),
                            rows,
                        ))
                        .into(),
                );
            }
            Entry::Separator => {
                out.push(
                    el("div")
                        .attr("role", "presentation")
                        .slot("option-separator")
                        .into(),
                );
            }
        }
    }
    out
}

/// One `<option>` of the carrier.
fn native_option(choice: &Choice, selected: bool) -> Element {
    el("option")
        .attr("value", choice.value.clone())
        .flag("selected", selected)
        .flag("disabled", choice.disabled)
        .child(choice.label.clone())
}

/// The carrier's options. `empty` is the empty first choice of a single select: its
/// text is the placeholder, and it is the chosen one when nothing else is. A value
/// that is chosen but not offered stays, so a form does not drop it.
pub(crate) fn native_rows(entries: &Entries, chosen: &[String], empty: Option<&str>) -> Vec<Node> {
    let set: HashSet<&str> = chosen.iter().map(String::as_str).collect();
    let mut out: Vec<Node> = Vec::with_capacity(entries.0.len() + 2);
    if let Some(text) = empty {
        out.push(
            el("option")
                .attr("value", "")
                .flag("selected", chosen.is_empty())
                .child(text.to_owned())
                .into(),
        );
    }
    let mut seen: HashSet<&str> = HashSet::new();
    for entry in &entries.0 {
        match entry {
            Entry::Choice(choice) => {
                seen.insert(choice.value.as_str());
                out.push(native_option(choice, set.contains(choice.value.as_str())).into());
            }
            Entry::Group { label, choices } => {
                let options: Vec<Node> = choices
                    .iter()
                    .map(|choice| {
                        seen.insert(choice.value.as_str());
                        native_option(choice, set.contains(choice.value.as_str())).into()
                    })
                    .collect();
                out.push(
                    el("optgroup")
                        .attr("label", label.clone())
                        .child(options)
                        .into(),
                );
            }
            Entry::Separator => {}
        }
    }
    for value in chosen {
        if !seen.contains(value.as_str()) && !value.is_empty() {
            out.push(
                el("option")
                    .attr("value", value.clone())
                    .flag("selected", true)
                    .child(value.clone())
                    .into(),
            );
        }
    }
    out
}
