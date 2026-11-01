//! The deal form: one struct is the form, its rules, its errors and its input.

use navree::http::Response;
use navree::http::response::IntoResponse;
use navree::ui::form::{FormView, Money, Rejected};
use navree::ui::{AppShell, Card, Form, Options, Page, Reply, Submit, ToastTone, Validate};
use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize, Options)]
#[allow(clippy::upper_case_acronyms)]
pub enum Currency {
    USD,
    EUR,
    GBP,
    CAD,
    AUD,
    JPY,
}

#[derive(Clone, Copy, Debug, Deserialize, Options, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    #[option(label = "Lead (10%)")]
    Lead,
    #[option(label = "Qualified (30%)")]
    Qualified,
    #[option(label = "Proposal (50%)")]
    Proposal,
    #[option(label = "Negotiation (80%)")]
    Negotiation,
    #[option(label = "Won (100%)")]
    Won,
}

#[derive(Debug, Deserialize, Validate, Form)]
pub struct NewDeal {
    /// Deal name
    #[validate(length(min = 1, max = 255))]
    #[form(placeholder = "Annual platform license", autofocus, wide, error = "Give the deal a name.")]
    pub title: String,
    /// Company
    #[validate(length(min = 1))]
    #[form(widget = select, prompt = "Choose a company", error = "Choose the company this deal is with.")]
    pub company_id: String,
    /// Main contact
    #[validate(skip)]
    #[form(widget = select, prompt = "Choose a contact")]
    pub contact_id: Option<String>,
    /// Value
    #[validate(custom(not_negative))]
    #[form(addon = "currency", placeholder = "12,500", error = "Enter an amount of 0 or more, like 12,500.")]
    pub amount: Money,
    #[validate(skip)]
    pub currency: Currency,
    /// Expected close
    #[validate(skip)]
    pub expected_close_date: Option<chrono::NaiveDate>,
    #[validate(skip)]
    pub stage: Stage,
    /// Owner
    #[validate(skip)]
    #[form(widget = select, prompt = "Unassigned")]
    pub owner_id: Option<String>,
}

fn not_negative(amount: &Money) -> Result<(), String> {
    if amount.0 < 0 { Err("Enter an amount of 0 or more.".to_owned()) } else { Ok(()) }
}

fn choices(names: &[&str]) -> Vec<(String, String)> {
    names.iter().enumerate().map(|(id, name)| (id.to_string(), (*name).to_owned())).collect()
}

/// The page around a form, new or sent back with errors.
fn page_with(shell: &AppShell, form: FormView<NewDeal>) -> Page {
    let form = form
        .post("/deals")
        .submit("Create deal")
        .cancel("Cancel", "/deals?per=1000")
        .options("company_id", choices(&["Acme", "Globex", "Initech"]))
        .options("contact_id", choices(&["Ada Lovelace", "Alan Turing"]))
        .options("owner_id", choices(&["Grace Hopper", "Edsger Dijkstra"]));
    shell.page("New deal").crumb("Deals", "/deals?per=1000").body(Card::new().description("Track an opportunity through the pipeline.").body(form))
}

/// `GET /deals/new`.
pub fn page(shell: &AppShell) -> Page {
    page_with(shell, FormView::new())
}

/// `POST /deals`.
pub fn store(shell: &AppShell, submit: Result<Submit<NewDeal>, Rejected<NewDeal>>) -> Response {
    match submit {
        Ok(Submit(deal)) => Reply::redirect("/deals?per=1000").toast(ToastTone::Success, format!("{} created", deal.title)).into_response(),
        Err(rejected) => rejected.respond(|form| page_with(shell, form)),
    }
}
