//! Field, Input, InputGroup, Textarea, NativeSelect, Checkbox, Switch, RadioGroup.

use navree::ui::*;

use crate::catalog::{Example, gallery, single};

const SOURCE: &str = include_str!("inputs.rs");

pub fn all() -> Vec<Example> {
    vec![
        single!("field", "thumb", "A field", field_thumb),
        single!("field", "help", "Label, control and help", field_help).start(),
        single!("field", "error", "With an error", field_error).start(),
        single!("field", "required", "Required", field_required).start(),
        single!("field", "set", "A field set", field_set)
            .start()
            .width(820)
            .height(220),
        single!("field", "label", "A label on its own", field_label).start(),
        single!("input", "default", "An input", input_default).start(),
        gallery!("input", "kinds", "Kinds", input_kinds).medium(),
        gallery!("input", "states", "States", input_states).medium(),
        single!(
            "input-group",
            "default",
            "Fixed text either side",
            input_group_default
        )
        .start(),
        single!(
            "input-group",
            "icon",
            "An icon and a button",
            input_group_icon
        )
        .start(),
        single!("textarea", "default", "A textarea", textarea_default).start(),
        gallery!("textarea", "states", "States", textarea_states).medium(),
        single!(
            "native-select",
            "default",
            "A select",
            native_select_default
        )
        .start(),
        gallery!("native-select", "states", "States", native_select_states).medium(),
        single!("checkbox", "default", "Checkboxes", checkbox_default),
        gallery!("checkbox", "states", "States", checkbox_states),
        single!("switch", "thumb", "A switch", switch_thumb),
        single!("switch", "default", "Switches", switch_default),
        gallery!("switch", "states", "States", switch_states),
        single!("radio-group", "thumb", "A radio group", radio_group_thumb),
        single!(
            "radio-group",
            "default",
            "A radio group",
            radio_group_default
        ),
        single!("radio-group", "disabled", "Disabled", radio_group_disabled),
    ]
}

fn field_thumb() -> Field {
    Field::new("thumb-email", "Email")
        .help("We never share it.")
        .control(Input::email("email").placeholder("gurpreet@example.com"))
}

fn field_help() -> Field {
    Field::new("deal-title", "Deal name")
        .help("Shown on the pipeline board.")
        .control(Input::text("title").placeholder("Annual platform license"))
}

fn field_error() -> Field {
    Field::new("deal-amount", "Value")
        .error("Enter an amount of 0 or more, like 1,25,000.")
        .control(Input::text("amount").value("-40"))
}

fn field_required() -> Field {
    Field::new("signup-email", "Email")
        .required()
        .control(Input::email("email").required().autocomplete("email"))
}

fn field_set() -> FieldSet {
    FieldSet::new("Billing address").children(
        Grid::cols(2).gap(3).children((
            Field::new("street", "Street").control(Input::text("street")),
            Field::new("city", "City").control(Input::text("city")),
            Field::new("postcode", "PIN code").control(Input::text("postcode")),
            Field::new("country", "Country").control(
                NativeSelect::new("country")
                    .prompt("Choose")
                    .option("in", "India")
                    .option("ae", "United Arab Emirates")
                    .selected("in"),
            ),
        )),
    )
}

fn field_label() -> Stack {
    Stack::gap(2).children((
        Label::new("search-deals", "Search deals"),
        Input::search("q").id("search-deals").placeholder("Sutlej"),
    ))
}

fn input_default() -> Input {
    Input::text("title").placeholder("Annual platform license")
}

fn input_kinds() -> impl IntoIterator<Item = (&'static str, Input)> {
    [
        ("text", Input::text("title").placeholder("Title")),
        (
            "email",
            Input::email("email").placeholder("gurpreet@example.com"),
        ),
        (
            "password",
            Input::password("password").placeholder("Password"),
        ),
        ("search", Input::search("q").placeholder("Search")),
        ("number", Input::number("seats").value("12")),
        ("date", Input::date("closes_on").value("2026-11-30")),
        (
            "tel",
            Input::new(InputKind::Tel, "phone").placeholder("+91 98765 43210"),
        ),
        (
            "url",
            Input::new(InputKind::Url, "site").placeholder("https://"),
        ),
    ]
}

fn input_states() -> impl IntoIterator<Item = (&'static str, Input)> {
    [
        ("Empty", Input::text("a").placeholder("Placeholder")),
        ("Filled", Input::text("b").value("Sutlej Supply renewal")),
        ("Invalid", Input::text("c").value("-40").invalid(true)),
        ("Disabled", Input::text("d").value("Locked").disabled(true)),
        (
            "Read only",
            Input::text("e").value("SUTLEJ-0042").readonly(),
        ),
    ]
}

fn input_group_default() -> Field {
    Field::new("deal-value", "Value").control(
        InputGroup::new(
            Input::text("amount")
                .placeholder("1,25,000")
                .inputmode("decimal"),
        )
        .start("₹")
        .end("INR"),
    )
}

fn input_group_icon() -> InputGroup {
    InputGroup::new(Input::search("q").placeholder("Search deals"))
        .start(components::icon(icons::SEARCH))
        .end(Kbd::new("/"))
}

fn textarea_default() -> Field {
    Field::new("deal-notes", "Notes")
        .help("Only your team sees these.")
        .control(
            Textarea::new("notes")
                .rows(4)
                .placeholder("What was agreed"),
        )
}

fn textarea_states() -> impl IntoIterator<Item = (&'static str, Textarea)> {
    [
        (
            "Empty",
            Textarea::new("a").rows(3).placeholder("Placeholder"),
        ),
        (
            "Filled",
            Textarea::new("b")
                .rows(3)
                .value("Renewal at the same price."),
        ),
        (
            "Disabled",
            Textarea::new("c").rows(3).value("Locked").disabled(true),
        ),
    ]
}

fn native_select_default() -> Field {
    Field::new("deal-stage", "Stage").control(
        NativeSelect::new("stage")
            .prompt("Choose a stage")
            .option("lead", "Lead")
            .option("qualified", "Qualified")
            .option("proposal", "Proposal")
            .option("won", "Won")
            .selected("proposal"),
    )
}

fn native_select_states() -> impl IntoIterator<Item = (&'static str, NativeSelect)> {
    [
        (
            "Prompt",
            NativeSelect::new("a")
                .prompt("Choose")
                .option("1", "One")
                .option("2", "Two"),
        ),
        (
            "Chosen",
            NativeSelect::new("b")
                .option("1", "One")
                .option("2", "Two")
                .selected("2"),
        ),
        (
            "Disabled",
            NativeSelect::new("c").option("1", "One").disabled(true),
        ),
    ]
}

fn checkbox_default() -> Stack {
    Stack::gap(2).children((
        Checkbox::new("terms").label("Accept the terms").required(),
        Checkbox::new("updates")
            .label("Send me product updates")
            .checked(true),
    ))
}

fn checkbox_states() -> impl IntoIterator<Item = (&'static str, Checkbox)> {
    [
        ("Unchecked", Checkbox::new("a").label("Archive")),
        ("Checked", Checkbox::new("b").label("Archive").checked(true)),
        (
            "Disabled",
            Checkbox::new("c").label("Archive").disabled(true),
        ),
        (
            "Checked, disabled",
            Checkbox::new("d")
                .label("Archive")
                .checked(true)
                .disabled(true),
        ),
    ]
}

fn switch_thumb() -> Switch {
    Switch::new("live").label("Live updates").checked(true)
}

fn switch_default() -> Stack {
    Stack::gap(3).children((
        Switch::new("notify")
            .label("Email me when a deal closes")
            .checked(true),
        Switch::new("digest").label("Weekly digest"),
    ))
}

fn switch_states() -> impl IntoIterator<Item = (&'static str, Switch)> {
    [
        ("Off", Switch::new("a").label("Live")),
        ("On", Switch::new("b").label("Live").checked(true)),
        ("Disabled", Switch::new("c").label("Live").disabled(true)),
    ]
}

fn radio_group_thumb() -> RadioGroup {
    RadioGroup::new("seats", "Seats")
        .option("5", "5 seats")
        .option("20", "20 seats")
        .selected("20")
}

fn radio_group_default() -> RadioGroup {
    RadioGroup::new("plan", "Plan")
        .option("starter", "Starter")
        .option("team", "Team")
        .option("business", "Business")
        .selected("team")
}

fn radio_group_disabled() -> RadioGroup {
    RadioGroup::new("region", "Data region")
        .option("mum", "Mumbai")
        .option("del", "Delhi")
        .selected("mum")
        .disabled(true)
}
