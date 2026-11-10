//! Button, ButtonGroup, Toggle.

use navree::ui::*;

use crate::catalog::{Example, gallery, single};

const SOURCE: &str = include_str!("actions.rs");

pub fn all() -> Vec<Example> {
    vec![
        single!("button", "thumb", "Buttons", button_thumb),
        single!("button", "default", "A button", button_default),
        gallery!("button", "variants", "Variants", button_variants),
        gallery!("button", "sizes", "Sizes", button_sizes).narrow(),
        single!("button", "icon", "With an icon", button_icon),
        single!("button", "icon-only", "Icon only", button_icon_only),
        gallery!("button", "states", "States", button_states).narrow(),
        single!(
            "button",
            "link",
            "A link that looks like a button",
            button_link
        ),
        single!("button", "submit", "Submitting a form", button_submit),
        single!("button-group", "thumb", "A group", button_group_thumb),
        single!("button-group", "default", "A group", button_group_default),
        single!(
            "button-group",
            "vertical",
            "Vertical",
            button_group_vertical
        ),
        single!("toggle", "thumb", "Toggles", toggle_thumb),
        single!("toggle", "default", "Toggles", toggle_default),
        gallery!("toggle", "states", "States", toggle_states),
        gallery!("toggle", "sizes", "Sizes", toggle_sizes),
    ]
}

fn button_thumb() -> Row {
    Row::gap(2).children((
        Button::primary("Save").icon(icons::CHECK),
        Button::outline("Cancel"),
    ))
}

fn button_default() -> Row {
    Row::gap(2).wrap().children((
        Button::primary("Create deal").icon(icons::PLUS),
        Button::new("Export"),
        Button::ghost("Cancel"),
    ))
}

fn button_variants() -> impl IntoIterator<Item = (&'static str, Button)> {
    [
        ("Primary", Button::primary("Save")),
        ("Secondary", Button::new("Save")),
        ("Outline", Button::outline("Save")),
        ("Ghost", Button::ghost("Save")),
        ("Destructive", Button::destructive("Delete")),
        (
            "Link",
            Button::new("Read more").variant(ButtonVariant::Link),
        ),
    ]
}

fn button_sizes() -> impl IntoIterator<Item = (&'static str, Button)> {
    [
        ("Xs · 24px", Button::new("Save").size(Size::Xs)),
        ("Sm · 28px", Button::new("Save").size(Size::Sm)),
        ("Md · 32px", Button::new("Save")),
        ("Lg · 36px", Button::new("Save").size(Size::Lg)),
    ]
}

fn button_icon() -> Row {
    Row::gap(2).wrap().children((
        Button::primary("New deal").icon(icons::PLUS),
        Button::outline("Download").icon(icons::DOWNLOAD),
        Button::new("Next").icon_end(icons::ARROW_RIGHT),
    ))
}

fn button_icon_only() -> Row {
    Row::gap(2).children((
        Button::icon_only(icons::SETTINGS, "Settings"),
        Button::icon_only(icons::PENCIL, "Edit").variant(ButtonVariant::Outline),
        Button::icon_only(icons::TRASH, "Delete").variant(ButtonVariant::Ghost),
    ))
}

fn button_states() -> impl IntoIterator<Item = (&'static str, Button)> {
    [
        ("Disabled", Button::primary("Save").disabled(true)),
        ("Pending", Button::primary("Saving").pending(true)),
        (
            "Pending, with words",
            Button::primary("Save")
                .loading_text("Saving…")
                .pending(true),
        ),
        (
            "Disabled link",
            Button::outline("Open").href("/deals").disabled(true),
        ),
        ("Block", Button::new("Full width").block()),
    ]
}

fn button_link() -> Button {
    Button::primary("Edit deal")
        .icon(icons::PENCIL)
        .href("/deals/42/edit")
}

fn button_submit() -> Element {
    el("form")
        .attr("action", "/deals")
        .attr("method", "post")
        .child(Row::gap(2).children((
            Button::primary("Save").submit(),
            Button::new("Reset").kind(ButtonKind::Reset),
        )))
}

fn button_group_thumb() -> ButtonGroup {
    ButtonGroup::new("View").children((
        Button::outline("List").size(Size::Sm),
        Button::outline("Board").size(Size::Sm),
        Button::outline("Week").size(Size::Sm),
    ))
}

fn button_group_default() -> ButtonGroup {
    ButtonGroup::new("View").children((
        Button::outline("List").icon(icons::LIST),
        Button::outline("Board").icon(icons::KANBAN),
        Button::outline("Calendar").icon(icons::CALENDAR),
    ))
}

fn button_group_vertical() -> ButtonGroup {
    ButtonGroup::new("Zoom").vertical().children((
        Button::icon_only(icons::PLUS, "Zoom in").variant(ButtonVariant::Outline),
        Button::icon_only(icons::MINUS, "Zoom out").variant(ButtonVariant::Outline),
    ))
}

fn toggle_thumb() -> Row {
    Row::gap(2).children((
        Toggle::new("starred", "Starred")
            .icon(icons::STAR)
            .pressed(true)
            .size(Size::Sm),
        Toggle::new("flagged", "Flag").size(Size::Sm),
    ))
}

fn toggle_default() -> Row {
    Row::gap(2).children((
        Toggle::new("starred", "Starred")
            .icon(icons::STAR)
            .pressed(true),
        Toggle::new("flagged", "Flagged").icon(icons::FLAG),
        Toggle::new("watching", "Watching").icon(icons::EYE),
    ))
}

fn toggle_states() -> impl IntoIterator<Item = (&'static str, Toggle)> {
    [
        ("Off", Toggle::new("starred", "Starred").icon(icons::STAR)),
        (
            "On",
            Toggle::new("starred", "Starred")
                .icon(icons::STAR)
                .pressed(true),
        ),
        (
            "Disabled",
            Toggle::new("starred", "Starred")
                .icon(icons::STAR)
                .disabled(true),
        ),
    ]
}

fn toggle_sizes() -> impl IntoIterator<Item = (&'static str, Toggle)> {
    [
        ("Sm", Toggle::new("pin", "Pin").size(Size::Sm)),
        ("Md", Toggle::new("pin", "Pin")),
        ("Lg", Toggle::new("pin", "Pin").size(Size::Lg)),
    ]
}
