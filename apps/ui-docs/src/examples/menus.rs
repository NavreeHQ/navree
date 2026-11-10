//! Tabs, ToggleGroup, DropdownMenu, Toast.

use navree::ui::*;

use crate::catalog::{Example, gallery, single};

const SOURCE: &str = include_str!("menus.rs");

pub fn all() -> Vec<Example> {
    vec![
        single!("tabs", "thumb", "Tabs", tabs_thumb)
            .start()
            .height(110),
        single!("tabs", "default", "Tabs", tabs_default)
            .start()
            .height(150),
        single!("tabs", "segmented", "Segmented", tabs_segmented)
            .start()
            .height(130),
        single!("tabs", "vertical", "Vertical", tabs_vertical)
            .start()
            .height(170),
        gallery!("tabs", "sizes", "Sizes", tabs_sizes)
            .wide()
            .height(220),
        single!("tabs", "icons", "Icons and counts", tabs_icons)
            .start()
            .height(130),
        single!("tabs", "manual", "Manual activation", tabs_manual)
            .start()
            .height(130),
        single!(
            "toggle-group",
            "thumb",
            "A toggle group",
            toggle_group_thumb
        ),
        single!(
            "toggle-group",
            "default",
            "One choice",
            toggle_group_default
        )
        .height(120),
        single!(
            "toggle-group",
            "multiple",
            "Several choices",
            toggle_group_multiple
        )
        .height(120),
        gallery!("toggle-group", "looks", "Looks", toggle_group_looks)
            .wide()
            .height(360),
        single!("toggle-group", "icons", "Icons", toggle_group_icons).height(120),
        single!(
            "toggle-group",
            "vertical",
            "Vertical",
            toggle_group_vertical
        )
        .height(160),
        gallery!("toggle-group", "states", "States", toggle_group_states)
            .medium()
            .height(200),
        single!("dropdown-menu", "thumb", "A menu", dropdown_thumb)
            .show()
            .height(170),
        single!("dropdown-menu", "default", "A menu", dropdown_default)
            .show()
            .height(330),
        single!(
            "dropdown-menu",
            "groups",
            "Groups and shortcuts",
            dropdown_groups
        )
        .show()
        .height(370),
        single!(
            "dropdown-menu",
            "choices",
            "Checkboxes and radios",
            dropdown_choices
        )
        .show()
        .height(340),
        single!("dropdown-menu", "submenu", "A submenu", dropdown_submenu).height(200),
        single!(
            "dropdown-menu",
            "destructive",
            "Destructive and disabled",
            dropdown_destructive
        )
        .show()
        .height(250),
        single!("dropdown-menu", "row", "A row's menu", dropdown_row)
            .start()
            .height(170),
        single!("toast", "thumb", "A toast", toast_thumb)
            .show()
            .height(120),
        gallery!("toast", "tones", "Tones", toast_tones)
            .wide()
            .show()
            .height(320),
        single!("toast", "action", "A title and an action", toast_action)
            .show()
            .height(130),
        single!("toast", "live", "From a command", toast_live)
            .live()
            .height(230),
    ]
}

fn tabs_thumb() -> Tabs {
    Tabs::new("tabs-thumb", "Deal sections")
        .tab(Tab::new("overview", "Overview").panel(Text::muted("Stage, owner, value")))
        .tab(Tab::new("notes", "Notes").panel(Text::muted("Notes about the deal")))
}

fn tabs_default() -> Tabs {
    Tabs::new("deal-tabs", "Deal sections")
        .tab(
            Tab::new("overview", "Overview")
                .panel(Text::muted("Sutlej Supply renewal, in Proposal.")),
        )
        .tab(Tab::new("notes", "Notes").panel(Text::muted("Three notes, the last from Gurpreet.")))
        .tab(Tab::new("files", "Files").disabled(true))
        .tab(Tab::new("history", "History").panel(Text::muted("Every change to this deal.")))
}

fn tabs_segmented() -> Tabs {
    Tabs::new("range-tabs", "Range")
        .variant(TabsVariant::Segmented)
        .tab(Tab::new("day", "Day").panel(Text::muted("Today so far.")))
        .tab(Tab::new("week", "Week").panel(Text::muted("The last seven days.")))
        .tab(Tab::new("month", "Month").panel(Text::muted("The last thirty days.")))
}

fn tabs_vertical() -> Tabs {
    Tabs::new("settings-tabs", "Settings")
        .vertical()
        .tab(Tab::new("profile", "Profile").panel(Text::muted("Your name and photo.")))
        .tab(Tab::new("billing", "Billing").panel(Text::muted("Plan, card and invoices.")))
        .tab(Tab::new("team", "Team").panel(Text::muted("Who can see this workspace.")))
}

fn tabs_sizes() -> impl IntoIterator<Item = (&'static str, Tabs)> {
    [
        ("Small", Size::Sm),
        ("Default", Size::Md),
        ("Large", Size::Lg),
    ]
    .map(|(caption, size)| {
        let id = format!("size-{}", caption.to_lowercase());
        (
            caption,
            Tabs::new(id.clone(), format!("{caption} tabs"))
                .size(size)
                .tab(Tab::new("a", "First").panel(Text::muted("The first panel.")))
                .tab(Tab::new("b", "Second").panel(Text::muted("The second panel."))),
        )
    })
}

fn tabs_icons() -> Tabs {
    Tabs::new("icon-tabs", "Inbox")
        .tab(
            Tab::new("all", "All")
                .icon(icons::INBOX)
                .count("24")
                .panel(Text::muted("Every message.")),
        )
        .tab(
            Tab::new("starred", "Starred")
                .icon(icons::STAR)
                .count("3")
                .panel(Text::muted("The ones you kept.")),
        )
        .tab(
            Tab::icon_only("settings", icons::SETTINGS, "Inbox settings")
                .panel(Text::muted("Rules and labels.")),
        )
}

fn tabs_manual() -> Tabs {
    Tabs::new("manual-tabs", "Reports")
        .activation(Activation::Manual)
        .tab(Tab::new("sales", "Sales").panel(Text::muted("Press Enter to open it.")))
        .tab(Tab::new("pipeline", "Pipeline").panel(Text::muted("Arrow keys only move focus.")))
        .tab(Tab::new("forecast", "Forecast").panel(Text::muted("Space opens it too.")))
}

fn toggle_group_thumb() -> ToggleGroup {
    ToggleGroup::single("thumb-view", "Show as")
        .variant(ToggleVariant::Outline)
        .joined()
        .item(ToggleItem::new("list", "List"))
        .item(ToggleItem::new("grid", "Grid"))
        .value("list")
}

fn toggle_group_default() -> ToggleGroup {
    ToggleGroup::single("view", "Show deals as")
        .variant(ToggleVariant::Outline)
        .size(Size::Sm)
        .joined()
        .item(ToggleItem::new("list", "List").icon(icons::LIST))
        .item(ToggleItem::new("board", "Board").icon(icons::LAYOUT_GRID))
        .item(ToggleItem::new("chart", "Chart"))
        .value("board")
}

fn toggle_group_multiple() -> ToggleGroup {
    ToggleGroup::multiple("days", "Working days")
        .variant(ToggleVariant::Outline)
        .size(Size::Sm)
        .item(ToggleItem::new("mon", "Mon"))
        .item(ToggleItem::new("tue", "Tue"))
        .item(ToggleItem::new("wed", "Wed"))
        .item(ToggleItem::new("thu", "Thu"))
        .item(ToggleItem::new("fri", "Fri"))
        .values(["mon", "tue", "wed", "thu"])
}

fn toggle_group_looks() -> impl IntoIterator<Item = (&'static str, ToggleGroup)> {
    let group = |name: &'static str| {
        ToggleGroup::single(name, "Range")
            .item(ToggleItem::new("7", "7 days"))
            .item(ToggleItem::new("30", "30 days"))
            .item(ToggleItem::new("90", "90 days"))
            .value("30")
    };
    [
        ("Default", group("look-default")),
        (
            "Outline",
            group("look-outline").variant(ToggleVariant::Outline),
        ),
        (
            "Outline, joined",
            group("look-joined")
                .variant(ToggleVariant::Outline)
                .joined(),
        ),
        (
            "Small",
            group("look-small")
                .variant(ToggleVariant::Outline)
                .joined()
                .size(Size::Sm),
        ),
    ]
}

fn toggle_group_icons() -> ToggleGroup {
    ToggleGroup::single("layout", "Layout")
        .variant(ToggleVariant::Outline)
        .joined()
        .item(ToggleItem::icon_only("list", icons::LIST, "As a list"))
        .item(ToggleItem::icon_only(
            "grid",
            icons::LAYOUT_GRID,
            "As a grid",
        ))
        .item(ToggleItem::icon_only("table", icons::TABLE, "As a table"))
        .value("grid")
        .deselectable()
}

fn toggle_group_vertical() -> ToggleGroup {
    ToggleGroup::multiple("format", "Text format")
        .variant(ToggleVariant::Outline)
        .joined()
        .vertical()
        .item(ToggleItem::new("bold", "Bold"))
        .item(ToggleItem::new("italic", "Italic"))
        .item(ToggleItem::new("underline", "Underline"))
        .values(["bold"])
}

fn toggle_group_states() -> impl IntoIterator<Item = (&'static str, ToggleGroup)> {
    [
        (
            "One item off",
            ToggleGroup::single("state-item", "Stage")
                .variant(ToggleVariant::Outline)
                .joined()
                .item(ToggleItem::new("lead", "Lead"))
                .item(ToggleItem::new("won", "Won").disabled(true))
                .item(ToggleItem::new("lost", "Lost"))
                .value("lead"),
        ),
        (
            "The whole group",
            ToggleGroup::single("state-all", "Stage")
                .variant(ToggleVariant::Outline)
                .joined()
                .disabled(true)
                .item(ToggleItem::new("lead", "Lead"))
                .item(ToggleItem::new("lost", "Lost"))
                .value("lost"),
        ),
    ]
}

fn dropdown_thumb() -> DropdownMenu {
    DropdownMenu::new("thumb-menu", "Actions")
        .item(MenuItem::link("Edit", "/deals/1/edit"))
        .item(MenuItem::action("Duplicate"))
        .item(MenuItem::action("Archive"))
}

fn dropdown_default() -> DropdownMenu {
    DropdownMenu::new("deal-menu", "Actions")
        .caret()
        .item(MenuItem::link("Edit", "/deals/42/edit").icon(icons::PENCIL))
        .item(MenuItem::action("Duplicate").icon(icons::COPY))
        .item(MenuItem::action("Archive").icon(icons::ARCHIVE))
        .separator()
        .item(
            MenuItem::post("Delete", "/deals/42/delete")
                .icon(icons::TRASH)
                .destructive(),
        )
}

fn dropdown_groups() -> DropdownMenu {
    DropdownMenu::new("groups-menu", "Deal")
        .group(
            MenuGroup::new("Edit")
                .item(MenuItem::action("Rename").icon(icons::PENCIL).shortcut("R"))
                .item(
                    MenuItem::action("Duplicate")
                        .icon(icons::COPY)
                        .shortcut("Ctrl+D"),
                ),
        )
        .separator()
        .group(
            MenuGroup::new("Share")
                .item(MenuItem::action("Copy link").icon(icons::LINK))
                .item(
                    MenuItem::link("Email the owner", "mailto:gurpreet@example.com")
                        .icon(icons::MAIL),
                ),
        )
}

fn dropdown_choices() -> DropdownMenu {
    DropdownMenu::new("choices-menu", "View")
        .checkbox(MenuCheckbox::new("archived", "Show archived").checked(true))
        .checkbox(MenuCheckbox::new("owner", "Show owner").keep_open())
        .separator()
        .radios(
            MenuRadios::new("sort", "Sort by")
                .option("name", "Name")
                .option("value", "Value")
                .option("closed", "Close date")
                .value("value"),
        )
}

fn dropdown_submenu() -> DropdownMenu {
    DropdownMenu::new("sub-menu", "Move")
        .item(MenuItem::action("Rename"))
        .submenu(
            MenuSub::new("Move to")
                .icon(icons::FOLDER)
                .item(MenuItem::action("Inbox"))
                .item(MenuItem::action("Archive"))
                .submenu(
                    MenuSub::new("Label as")
                        .item(MenuItem::action("Urgent"))
                        .item(MenuItem::action("Later")),
                ),
        )
        .item(MenuItem::action("Delete").destructive())
}

fn dropdown_destructive() -> DropdownMenu {
    DropdownMenu::new("danger-menu", "More")
        .item(MenuItem::action("Edit"))
        .item(MenuItem::action("Archive").disabled(true))
        .separator()
        .item(MenuItem::action("Delete deal").destructive())
        .item(
            MenuItem::action("Delete forever")
                .destructive()
                .disabled(true),
        )
}

fn dropdown_row() -> Table {
    Table::new()
        .caption("Deals")
        .head((TableHead::new("Deal"), TableHead::new("Actions")))
        .rows(
            [("Sutlej Supply renewal", 1), ("Haveli Home expansion", 2)].map(|(title, id)| {
                TableRowEl::new((
                    TableCell::new(title),
                    TableCell::new(
                        DropdownMenu::icon(
                            format!("row-menu-{id}"),
                            icons::ELLIPSIS,
                            format!("Actions for {title}"),
                        )
                        .size(Size::Sm)
                        .align(PopupAlign::End)
                        .item(MenuItem::link("Open", format!("/deals/{id}")))
                        .item(
                            MenuItem::post("Remove", format!("/deals/{id}/remove")).destructive(),
                        ),
                    ),
                ))
            }),
        )
}

fn toast_thumb() -> Toaster {
    Toaster::new().toast(Toast::new(ToastTone::Success, "Deal saved"))
}

fn toast_tones() -> impl IntoIterator<Item = (&'static str, Toaster)> {
    [
        ("Info", Toast::new(ToastTone::Info, "Imports run at night.")),
        ("Success", Toast::new(ToastTone::Success, "Deal created.")),
        (
            "Warning",
            Toast::new(ToastTone::Warning, "Trial ends in 3 days."),
        ),
        ("Danger", Toast::new(ToastTone::Danger, "Payment failed.")),
    ]
    .map(|(caption, toast)| (caption, Toaster::new().toast(toast)))
}

fn toast_action() -> Toaster {
    Toaster::new().toast(
        Toast::new(
            ToastTone::Success,
            "Sutlej Supply renewal moved to the archive.",
        )
        .title("Archived")
        .action(ToastAction::post("Undo", "/deals/42/restore")),
    )
}

fn toast_live() -> Row {
    Row::gap(2).wrap().children((
        Button::outline("Saved").on_click(Cmd::toast(ToastTone::Success, "Deal saved.")),
        Button::outline("Warning")
            .on_click(Cmd::toast(ToastTone::Warning, "Trial ends in 3 days.")),
        Button::outline("Failed").on_click(Cmd::toast(ToastTone::Danger, "Payment failed.")),
    ))
}
