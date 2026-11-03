//! Foundations, icons, forms, commands and regions.

use std::collections::BTreeMap;
use std::time::Duration;

use navree::ui::form::{FormView, Money};
use navree::ui::*;
use serde::Deserialize;

use crate::catalog::{Example, gallery, single};

const SOURCE: &str = include_str!("patterns.rs");

pub fn all() -> Vec<Example> {
    vec![
        single!(
            "foundations",
            "radius",
            "4px controls, 6px surfaces",
            foundations_radius
        )
        .start(),
        gallery!("foundations", "sizes", "Control sizes", foundations_sizes).narrow(),
        single!(
            "foundations",
            "colour",
            "Green for the one main action",
            foundations_colour
        ),
        single!("forms", "thumb", "A form", forms_thumb).start(),
        single!("forms", "default", "A form from a struct", forms_default)
            .start()
            .height(420),
        single!("forms", "errors", "Sent back with errors", forms_errors)
            .start()
            .height(440),
        single!(
            "commands",
            "toggle",
            "Toggle, with no request",
            commands_toggle
        )
        .live(),
        single!("commands", "open", "Open and focus", commands_open)
            .live()
            .height(320),
        single!("commands", "text", "Text set by a choice", commands_text).live(),
        single!("commands", "count", "A counter as you type", commands_count).live(),
        single!("commands", "fill", "A row's details", commands_fill)
            .live()
            .height(360),
        single!(
            "regions",
            "lazy",
            "Before a lazy region loads",
            regions_lazy
        )
        .start(),
        single!("regions", "poll", "A region that polls", regions_poll).start(),
    ]
}

fn foundations_radius() -> Row {
    Row::gap(4).wrap().align(Align::Start).children((
        Stack::gap(2).align(Align::Start).children((
            Button::primary("Button"),
            Input::text("q").placeholder("Input"),
            Badge::new("Badge"),
        )),
        Card::titled("Card")
            .description("6px corners")
            .body(Text::muted("Surfaces")),
    ))
}

fn foundations_sizes() -> impl IntoIterator<Item = (&'static str, Button)> {
    [
        ("Size::Xs", Button::new("24px").size(Size::Xs)),
        ("Size::Sm", Button::new("28px").size(Size::Sm)),
        ("Size::Md", Button::new("32px")),
        ("Size::Lg", Button::new("36px").size(Size::Lg)),
    ]
}

fn foundations_colour() -> Row {
    Row::gap(2).children((
        Button::primary("Save changes"),
        Button::new("Preview"),
        Badge::new("Won").variant(BadgeVariant::Success),
    ))
}

#[derive(Clone, Copy, Debug, Deserialize, Options)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Lead,
    Qualified,
    Proposal,
    #[option(label = "Won (100%)")]
    Won,
}

/// A deal, as the form draws it and the handler reads it back.
#[derive(Debug, Deserialize, Validate, Form)]
pub struct NewDeal {
    /// Deal name
    #[validate(length(min = 1, max = 255))]
    #[form(
        placeholder = "Annual platform license",
        wide,
        error = "Give the deal a name."
    )]
    pub title: String,
    /// Value
    #[validate(skip)]
    #[form(placeholder = "12,500", help = "In US dollars.")]
    pub amount: Money,
    /// Stage
    #[validate(skip)]
    pub stage: Stage,
    /// Expected close
    #[validate(skip)]
    pub closes_on: Option<chrono::NaiveDate>,
    /// Hot deal
    #[validate(skip)]
    pub hot: bool,
}

fn forms_thumb() -> Field {
    Field::new("thumb-name", "Deal name")
        .error("Give the deal a name.")
        .control(Input::text("title"))
}

fn forms_default() -> FormView<NewDeal> {
    FormView::<NewDeal>::new()
        .post("/deals")
        .submit("Create deal")
        .cancel("Cancel", "/deals")
}

fn forms_errors() -> FormView<NewDeal> {
    let mut errors = BTreeMap::new();
    errors.insert("title".to_owned(), vec!["Give the deal a name.".to_owned()]);
    FormView::<NewDeal>::new()
        .post("/deals")
        .submit("Create deal")
        .with_values(vec![("amount".to_owned(), "12,500".to_owned())])
        .with_errors(errors)
}

fn commands_toggle() -> Stack {
    Stack::gap(3).align(Align::Center).children((
        Button::outline("Show details").on_click(Cmd::toggle("deal-details")),
        el("div")
            .id("deal-details")
            .flag("hidden", true)
            .child(Text::muted(
                "Shown and hidden in the browser, with no request.",
            )),
    ))
}

fn commands_open() -> (Button, Dialog) {
    (
        Button::outline("Rename").on_click(vec![Cmd::open("rename"), Cmd::focus("rename-name")]),
        Dialog::new("rename", "Rename deal").body(
            Field::new("rename-name", "Name").control(Input::text("title").value("Acme renewal")),
        ),
    )
}

fn commands_text() -> Stack {
    Stack::gap(3).align(Align::Center).children((
        Row::gap(2).children((
            Button::outline("Monthly").on_click(Cmd::text("plan-price", "$12 a month")),
            Button::outline("Yearly").on_click(Cmd::text("plan-price", "$120 a year")),
        )),
        Heading::h3("$12 a month").id("plan-price"),
    ))
}

fn commands_count() -> Stack {
    Stack::gap(1).children((
        Field::new("bio", "Bio").control(
            Textarea::new("bio")
                .attr("maxlength", 160_u32)
                .on(Trigger::Input, Cmd::count("bio-count", 160)),
        ),
        Text::muted("0/160").id("bio-count"),
    ))
}

fn commands_fill() -> (Table, Dialog) {
    let row = |key: &'static str, title: &'static str, amount: &'static str| {
        el("tr").attr("data-key", key).children((
            el("td").child(title),
            el("td").child(amount),
            el("td").child(
                Button::ghost("Details")
                    .size(Size::Sm)
                    .on_click(Cmd::fill("deal-details-dialog")),
            ),
        ))
    };
    (
        Table::new()
            .head((
                el("th").attr("data-col", "title").child("Deal"),
                el("th").attr("data-col", "amount").child("Amount"),
                el("th").child(""),
            ))
            .rows((
                row("7", "Acme renewal", "12,500"),
                row("8", "Globex expansion", "48,000"),
            )),
        Dialog::new("deal-details-dialog", "Deal").body(el("p").children((
            el("strong").fill_cell("title"),
            ", worth ",
            el("span").fill_cell("amount"),
            ".",
        ))),
    )
}

fn regions_lazy() -> Card {
    Card::titled("Revenue by month").body(Region::new("revenue").src("/fragments/revenue").lazy())
}

fn regions_poll() -> Card {
    Card::titled("Activity").body(
        Region::new("activity")
            .src("/fragments/activity")
            .poll(Duration::from_secs(5))
            .children(Stack::gap(1).children((
                Text::muted("Ada moved Acme renewal to Proposal"),
                Text::muted("Alan created Globex renewal"),
            ))),
    )
}
