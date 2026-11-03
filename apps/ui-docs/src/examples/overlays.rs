//! Dialog, AlertDialog, Sheet, Drawer, Popover, Tooltip, HoverCard.

use navree::ui::*;

use crate::catalog::{Example, gallery, single};

const SOURCE: &str = include_str!("overlays.rs");

pub fn all() -> Vec<Example> {
    vec![
        single!("dialog", "thumb", "A dialog", dialog_thumb)
            .show()
            .height(140),
        single!("dialog", "default", "A dialog", dialog_default)
            .show()
            .height(420),
        single!("dialog", "form", "A form in a dialog", dialog_form)
            .show()
            .height(440),
        single!(
            "alert-dialog",
            "thumb",
            "An alert dialog",
            alert_dialog_thumb
        )
        .show()
        .height(140),
        single!(
            "alert-dialog",
            "default",
            "An alert dialog",
            alert_dialog_default
        )
        .show()
        .height(320),
        single!("sheet", "thumb", "A sheet", sheet_thumb)
            .show()
            .height(140),
        single!("sheet", "default", "A sheet", sheet_default)
            .show()
            .height(460),
        gallery!("sheet", "sides", "Sides", sheet_sides).height(160),
        single!("drawer", "thumb", "A drawer", drawer_thumb)
            .show()
            .height(140),
        single!("drawer", "default", "A drawer", drawer_default)
            .show()
            .height(340),
        single!("popover", "thumb", "A popover", popover_thumb)
            .show()
            .height(140),
        single!("popover", "default", "A popover", popover_default)
            .show()
            .height(280),
        gallery!("popover", "sides", "Sides", popover_sides).height(160),
        single!("tooltip", "thumb", "A tooltip", tooltip_thumb)
            .show()
            .height(140),
        single!("tooltip", "default", "A tooltip", tooltip_default)
            .show()
            .height(160),
        single!("hover-card", "thumb", "A hover card", hover_card_thumb)
            .show()
            .height(140),
        single!("hover-card", "default", "A hover card", hover_card_default)
            .show()
            .height(240),
    ]
}

fn dialog_thumb() -> Dialog {
    Dialog::new("thumb-dialog", "Rename deal").footer(Button::primary("Save").size(Size::Sm))
}

fn dialog_default() -> (Button, Dialog) {
    (
        Button::outline("Share deal").opens_dialog("share"),
        Dialog::new("share", "Share Acme renewal")
            .description("People with access can see the deal and its notes.")
            .body(
                Field::new("share-email", "Email")
                    .control(Input::email("email").placeholder("alan@example.com")),
            )
            .footer((
                Button::outline("Cancel").closes_dialog("share"),
                Button::primary("Share"),
            )),
    )
}

fn dialog_form() -> (Button, Dialog) {
    (
        Button::primary("New contact")
            .icon(icons::USER_PLUS)
            .opens_dialog("new-contact"),
        Dialog::new("new-contact", "New contact")
            .body(
                el("form")
                    .attr("action", "/contacts")
                    .attr("method", "post")
                    .id("new-contact-form")
                    .child(Stack::gap(3).children((
                        Field::new("contact-name", "Name").control(Input::text("name")),
                        Field::new("contact-email", "Email").control(Input::email("email")),
                    ))),
            )
            .footer((
                Button::outline("Cancel").closes_dialog("new-contact"),
                Button::primary("Create")
                    .submit()
                    .attr("form", "new-contact-form"),
            )),
    )
}

fn alert_dialog_thumb() -> AlertDialog {
    AlertDialog::new("thumb-alert", "Delete deal?", "This can't be undone.")
        .action(Button::destructive("Delete").size(Size::Sm))
}

fn alert_dialog_default() -> (Button, AlertDialog) {
    (
        Button::destructive("Delete deal").opens_dialog("delete-deal"),
        AlertDialog::new(
            "delete-deal",
            "Delete Acme renewal?",
            "Its notes and files go too. This can't be undone.",
        )
        .action(
            el("form")
                .attr("action", "/deals/42/delete")
                .attr("method", "post")
                .child(Button::destructive("Delete").submit()),
        ),
    )
}

fn sheet_thumb() -> Sheet {
    Sheet::new("thumb-sheet", "Filters").body(Text::muted("Stage, owner, value"))
}

fn sheet_default() -> (Button, Sheet) {
    (
        Button::outline("Filters")
            .icon(icons::LIST_FILTER)
            .opens_dialog("filters"),
        Sheet::new("filters", "Filters")
            .description("Narrow the deals list.")
            .body(
                Stack::gap(3).children((
                    Field::new("filter-owner", "Owner").control(
                        NativeSelect::new("owner")
                            .prompt("Anyone")
                            .option("ada", "Ada Lovelace")
                            .option("alan", "Alan Turing"),
                    ),
                    Switch::new("hot").label("Hot deals only"),
                )),
            )
            .footer((
                Button::outline("Reset").closes_dialog("filters"),
                Button::primary("Apply"),
            )),
    )
}

fn sheet_sides() -> impl IntoIterator<Item = (&'static str, Node)> {
    [
        ("Right", SheetSide::Right),
        ("Left", SheetSide::Left),
        ("Top", SheetSide::Top),
        ("Bottom", SheetSide::Bottom),
    ]
    .map(|(name, side)| {
        let id = format!("sheet-{}", name.to_lowercase());
        let open = Button::outline("Open").opens_dialog(id.clone());
        let sheet = Sheet::new(id, format!("From the {}", name.to_lowercase()))
            .side(side)
            .body(Text::muted("Press Esc or click outside to close."));
        (name, Node::list((open, sheet)))
    })
}

fn drawer_thumb() -> Drawer {
    Drawer::new("thumb-drawer", "Quick add").body(Text::muted("Add a task"))
}

fn drawer_default() -> (Button, Drawer) {
    (
        Button::outline("Quick add")
            .icon(icons::PLUS)
            .opens_dialog("quick-add"),
        Drawer::new("quick-add", "Quick add")
            .description("Add a task to Acme renewal.")
            .body(Field::new("task-title", "Task").control(Input::text("title")))
            .footer((
                Button::outline("Cancel").closes_dialog("quick-add"),
                Button::primary("Add"),
            )),
    )
}

fn popover_thumb() -> Popover {
    Popover::new("thumb-popover", "Owner")
        .label("Owner")
        .body(Text::muted("Ada Lovelace"))
}

fn popover_default() -> Popover {
    Popover::new("deal-owner", "Change owner")
        .label("Change owner")
        .align(PopupAlign::Start)
        .body(
            Stack::gap(2).children((
                Field::new("owner-search", "Owner")
                    .control(Input::search("owner").placeholder("Search people")),
                Button::primary("Assign").size(Size::Sm),
            )),
        )
}

fn popover_sides() -> impl IntoIterator<Item = (&'static str, Popover)> {
    [
        (
            "Top",
            Popover::new("side-top", "Top")
                .side(Side::Top)
                .body("Opens above"),
        ),
        (
            "Bottom",
            Popover::new("side-bottom", "Bottom").body("Opens below"),
        ),
        (
            "Left",
            Popover::new("side-left", "Left")
                .side(Side::Left)
                .body("Opens left"),
        ),
        (
            "Right",
            Popover::new("side-right", "Right")
                .side(Side::Right)
                .body("Opens right"),
        ),
    ]
}

fn tooltip_thumb() -> Tooltip {
    Tooltip::new(
        "thumb-tip",
        Button::icon_only(icons::COPY, "Copy link"),
        "Copy link",
    )
}

fn tooltip_default() -> Row {
    Row::gap(2).wrap().children((
        Tooltip::new(
            "tip-copy",
            Button::icon_only(icons::COPY, "Copy link").variant(ButtonVariant::Outline),
            "Copy link",
        ),
        Tooltip::new(
            "tip-archive",
            Button::icon_only(icons::ARCHIVE, "Archive").variant(ButtonVariant::Outline),
            "Archive, then find it under Archived",
        )
        .side(Side::Bottom),
    ))
}

fn hover_card_thumb() -> HoverCard {
    HoverCard::new("thumb-hover", el("a").href("/people/ada").child("@ada"))
        .body(Text::muted("Ada Lovelace · Sales"))
}

fn hover_card_default() -> HoverCard {
    HoverCard::new(
        "ada-card",
        el("a").href("/people/ada").child("Ada Lovelace"),
    )
    .body(
        Item::new("Ada Lovelace")
            .description("Account executive · 42 open deals")
            .media(Avatar::placeholder("Ada Lovelace")),
    )
}
