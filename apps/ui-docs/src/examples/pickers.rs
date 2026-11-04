//! Select, Combobox, MultiSelect, Command.

use navree::ui::*;

use crate::catalog::{Example, gallery, single};

const SOURCE: &str = include_str!("pickers.rs");

pub fn all() -> Vec<Example> {
    vec![
        single!("select", "thumb", "A select", select_thumb),
        // Each frame is tall enough for its list to open under its control, whole.
        single!("select", "default", "A select", select_default)
            .start()
            .height(260),
        single!("select", "open", "The list, open", select_open)
            .start()
            .show()
            .height(300),
        single!(
            "select",
            "groups",
            "Groups and a disabled option",
            select_groups
        )
        .start()
        .show()
        .height(360),
        single!(
            "select",
            "rich",
            "A line under the label, and an icon",
            select_rich
        )
        .start()
        .show()
        .height(280),
        gallery!("select", "states", "States", select_states)
            .start()
            .height(380),
        single!("combobox", "thumb", "A combobox", combobox_thumb),
        single!("combobox", "default", "A combobox", combobox_default)
            .start()
            .height(460),
        single!("combobox", "open", "The list, open", combobox_open)
            .start()
            .show()
            .height(470),
        single!("combobox", "create", "Making a new option", combobox_create)
            .start()
            .height(260),
        gallery!("combobox", "states", "States", combobox_states)
            .start()
            .height(460),
        single!("multi-select", "thumb", "A multi-select", multi_thumb),
        single!("multi-select", "default", "A multi-select", multi_default)
            .start()
            .height(470),
        single!("multi-select", "open", "The list, open", multi_open)
            .start()
            .show()
            .height(470),
        gallery!("multi-select", "limits", "Limits", multi_limits)
            .start()
            .height(470),
        single!("command", "thumb", "A command palette", command_thumb)
            .show()
            .height(250),
        single!("command", "default", "A command palette", command_default)
            .show()
            .height(420),
    ]
}

fn stages() -> Vec<Choice> {
    ["Lead", "Qualified", "Proposal", "Won", "Lost"]
        .into_iter()
        .map(|stage| Choice::new(stage.to_lowercase(), stage))
        .collect()
}

fn countries() -> Vec<Choice> {
    [
        ("au", "Australia"),
        ("bd", "Bangladesh"),
        ("ca", "Canada"),
        ("in", "India"),
        ("ke", "Kenya"),
        ("np", "Nepal"),
        ("sg", "Singapore"),
        ("lk", "Sri Lanka"),
        ("ae", "United Arab Emirates"),
        ("gb", "United Kingdom"),
        ("us", "United States"),
    ]
    .into_iter()
    .map(|(code, name)| Choice::new(code, name))
    .collect()
}

fn select_thumb() -> Select {
    Select::new("thumb-stage")
        .aria_label("Stage")
        .placeholder("Stage")
        .options(stages())
}

fn select_default() -> Field {
    Field::new("deal-stage", "Stage").control(
        Select::new("stage")
            .placeholder("Choose a stage")
            .option("lead", "Lead")
            .option("qualified", "Qualified")
            .option("won", "Won")
            .option("lost", "Lost"),
    )
}

fn select_open() -> Field {
    Field::new("open-stage", "Stage").control(
        Select::new("open_stage")
            .id("open-select")
            .options(stages())
            .selected("qualified"),
    )
}

fn select_groups() -> Field {
    Field::new("zone", "City").control(
        Select::new("zone")
            .id("zone-select")
            .placeholder("Choose a city")
            .group("Punjab", [("ldh", "Ludhiana"), ("asr", "Amritsar")])
            .group(
                "Delhi NCR",
                [
                    Choice::new("del", "Delhi"),
                    Choice::new("ggn", "Gurugram").disabled(true),
                    Choice::new("noi", "Noida"),
                ],
            ),
    )
}

fn select_rich() -> Field {
    Field::new("owner", "Owner").control(
        Select::new("owner")
            .id("owner-select")
            .placeholder("Choose an owner")
            .choice(
                Choice::new("gurpreet", "Gurpreet Singh")
                    .description("Engineering")
                    .icon(icons::USER)
                    .selected_label("Gurpreet"),
            )
            .choice(
                Choice::new("harleen", "Harleen Kaur")
                    .description("Research")
                    .icon(icons::USER),
            ),
    )
}

fn select_states() -> impl IntoIterator<Item = (&'static str, Select)> {
    [
        (
            "Clearable",
            Select::new("s1")
                .aria_label("Clearable")
                .options(stages())
                .selected("lead")
                .clearable(),
        ),
        (
            "Invalid",
            Select::new("s2")
                .aria_label("Invalid")
                .placeholder("Choose")
                .options(stages())
                .invalid(true),
        ),
        (
            "Disabled",
            Select::new("s3")
                .aria_label("Disabled")
                .options(stages())
                .selected("lead")
                .disabled(true),
        ),
        (
            "Read only",
            Select::new("s4")
                .aria_label("Read only")
                .options(stages())
                .selected("won")
                .readonly(),
        ),
        (
            "Small",
            Select::new("s5")
                .aria_label("Small")
                .size(Size::Sm)
                .placeholder("Choose")
                .options(stages()),
        ),
    ]
}

fn combobox_thumb() -> Combobox {
    Combobox::new("thumb-country")
        .aria_label("Country")
        .placeholder("Country")
        .options(countries())
}

fn combobox_default() -> Field {
    Field::new("deal-country", "Country").control(
        Combobox::new("country")
            .placeholder("Search countries")
            .options(countries())
            .selected("in")
            .clearable(),
    )
}

fn combobox_open() -> Field {
    Field::new("open-country", "Country").control(
        Combobox::new("open_country")
            .id("open-combobox")
            .placeholder("Search countries")
            .options(countries()),
    )
}

fn combobox_create() -> Field {
    Field::new("deal-source", "Source")
        .help("Pick one, or type a new one and choose “Create”.")
        .control(
            Combobox::new("source")
                .placeholder("Pick or type")
                .options([("web", "Website"), ("referral", "Referral")])
                .creatable(),
        )
}

fn combobox_states() -> impl IntoIterator<Item = (&'static str, Combobox)> {
    [
        (
            "Invalid",
            Combobox::new("c1")
                .aria_label("Invalid")
                .placeholder("Search")
                .options(countries())
                .invalid(true),
        ),
        (
            "Disabled",
            Combobox::new("c2")
                .aria_label("Disabled")
                .options(countries())
                .selected("in")
                .disabled(true),
        ),
        (
            "Create not allowed",
            Combobox::new("c3")
                .aria_label("Create not allowed")
                .placeholder("Pick")
                .options(countries())
                .creatable()
                .create_denied("Only admins can add countries."),
        ),
    ]
}

fn multi_thumb() -> MultiSelect {
    MultiSelect::new("thumb-countries")
        .aria_label("Countries")
        .placeholder("Countries")
        .options(countries())
        .selected_values(["in", "ae"])
}

fn multi_default() -> Field {
    Field::new("deal-markets", "Markets").control(
        MultiSelect::new("markets")
            .placeholder("Add a market")
            .options(countries())
            .selected_values(["in", "ae"]),
    )
}

fn multi_open() -> Field {
    Field::new("open-markets", "Markets").control(
        MultiSelect::new("open_markets")
            .id("open-multi")
            .placeholder("Add a market")
            .options(countries())
            .selected_values(["in"]),
    )
}

fn multi_limits() -> impl IntoIterator<Item = (&'static str, MultiSelect)> {
    [
        (
            "At most three",
            MultiSelect::new("m1")
                .aria_label("At most three")
                .placeholder("Add")
                .options(countries())
                .max(3)
                .selected_values(["in", "ae", "sg"]),
        ),
        (
            "One chip for many",
            MultiSelect::new("m2")
                .aria_label("One chip for many")
                .placeholder("Add")
                .options(countries())
                .summary_after(2)
                .selected_values(["in", "ae", "sg"]),
        ),
        (
            "Select all",
            MultiSelect::new("m3")
                .aria_label("Select all")
                .placeholder("Add")
                .options(stages())
                .select_all(),
        ),
    ]
}

fn command_thumb() -> Command {
    Command::new("thumb-palette")
        .placeholder("Type a command…")
        .group(
            CommandGroup::new("Go to")
                .item(CommandItem::link("Deals", "/deals").icon(icons::HANDSHAKE))
                .item(CommandItem::link("Contacts", "/contacts").icon(icons::USER)),
        )
}

fn command_default() -> Command {
    Command::new("palette")
        .placeholder("Type a command or search…")
        .recent(4)
        .group(
            CommandGroup::new("Go to")
                .item(
                    CommandItem::link("Deals", "/deals")
                        .icon(icons::HANDSHAKE)
                        .shortcut("alt+d"),
                )
                .item(CommandItem::link("New deal", "/deals/new").icon(icons::CIRCLE_PLUS))
                .item(
                    CommandItem::link("Contacts", "/contacts")
                        .icon(icons::USER)
                        .keywords(["people"]),
                ),
        )
        .group(
            CommandGroup::new("Account")
                .item(CommandItem::post("Sign out", "/sign-out").not_recent())
                .item(CommandItem::disabled(
                    "Billing",
                    "Only owners can open billing.",
                )),
        )
        .csrf("session-token")
}
