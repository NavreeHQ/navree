//! Phone numbers: a contact form whose numbers the server reads in the
//! country chosen with each, and the phone input in its states.

use std::sync::Arc;

use http::Method;
use navree::http::response::IntoResponse;
use navree::http::{Controller, Response, RouteDef};
use navree::kernel::{BoxFuture, Injectable, KernelError, ProviderKey, Resolver};
use navree::phone::{NumberType, Region};
use navree::ui::form::{FormView, Rejected};
use navree::ui::*;
use navree::validation::Phone;
use serde::Deserialize;

/// A contact: one required mobile, one optional office number.
#[derive(Debug, Deserialize, Validate, Form)]
#[form(id = "contact")]
pub struct NewContact {
    /// Name
    #[validate(length(min = 1, max = 120))]
    #[form(error = "Give the contact a name.")]
    pub name: String,
    /// Mobile
    #[validate(skip)]
    #[form(help = "Type it as you would dial it, or paste it with its country code.")]
    pub mobile: Phone,
    /// Office
    #[validate(skip)]
    pub office: Option<Phone>,
}

fn gb() -> Option<Region> {
    Region::parse("GB")
}

fn form_card(form: FormView<NewContact>) -> Card {
    Card::new()
        .title("New contact")
        .description("Numbers are checked on the server against each country's numbering plan.")
        .body(
            form.post("/phone")
                .phone_region(gb())
                .one_column()
                .submit("Save contact"),
        )
}

fn kind(kind: Option<NumberType>) -> &'static str {
    match kind {
        Some(NumberType::Mobile) => "Mobile",
        Some(NumberType::FixedLine) => "Landline",
        Some(NumberType::FixedLineOrMobile) => "Landline or mobile",
        Some(NumberType::TollFree) => "Toll free",
        Some(_) => "Other",
        None => "Unknown",
    }
}

/// What the server made of a saved number.
fn facts(label: &'static str, phone: &Phone) -> Element {
    let number = phone.number();
    el("dl").attr("data-proof", label).children((
        el("dt").child("E.164"),
        el("dd")
            .attr("data-fact", "e164")
            .child(phone.as_str().to_owned()),
        el("dt").child("International"),
        el("dd")
            .attr("data-fact", "international")
            .child(phone.international()),
        el("dt").child("National"),
        el("dd")
            .attr("data-fact", "national")
            .child(phone.national()),
        el("dt").child("Type"),
        el("dd")
            .attr("data-fact", "type")
            .child(kind(phone.number_type())),
        el("dt").child("Country"),
        el("dd")
            .attr("data-fact", "region")
            .child(number.region().map_or("None", |region| region.name())),
    ))
}

fn states() -> Element {
    let saved = navree::phone::PhoneNumber::parse("+1 650 253 0000", None).ok();
    let mut prefilled = PhoneInput::new("saved").country("GB");
    if let Some(saved) = &saved {
        prefilled = prefilled.number(saved);
    }
    let section = Stack::gap(4).children((
        Heading::h2("States"),
        Grid::cols(2).gap(4).children((
            Field::new("state-empty", "Empty, United Kingdom")
                .control(PhoneInput::new("empty").country("GB")),
            Field::new("state-saved", "A saved number, United States").control(prefilled),
            Field::new("state-none", "No country chosen").control(PhoneInput::new("none")),
            Field::new("state-invalid", "Invalid")
                .error("Enter a valid number for France.")
                .control(
                    PhoneInput::new("invalid")
                        .country("FR")
                        .text("06 12")
                        .invalid(true),
                ),
            Field::new("state-disabled", "Disabled")
                .control(PhoneInput::new("disabled").country("DE").disabled(true)),
            Field::new("state-preferred", "Suggested countries first").control(
                PhoneInput::new("preferred").country("IN").preferred(
                    ["IN", "AE", "SA", "GB", "US"]
                        .into_iter()
                        .filter_map(Region::parse),
                ),
            ),
        )),
        Heading::h2("Right to left"),
        Direction::rtl(Field::new("state-rtl", "رقم الجوال").control(
            PhoneInput::new("rtl").country("AE").labels(PhoneLabels {
                country: "الدولة".into(),
                choose: "الدولة".into(),
                preferred: "مقترحة".into(),
                search: "ابحث عن دولة".into(),
                empty: "لا توجد دولة.".into(),
                country_set: "تم اختيار {country}".into(),
                count: "{n} دولة".into(),
            }),
        )),
    ));
    el("section")
        .attr("data-gallery", "Phone input")
        .child(section)
}

fn page(shell: &AppShell, form: FormView<NewContact>, saved: Option<&NewContact>) -> AppPage {
    let result = saved.map(|contact| {
        Card::new().title(format!("Saved {}", contact.name)).body((
            facts("mobile", &contact.mobile),
            contact
                .office
                .as_ref()
                .map(|office| facts("office", office)),
        ))
    });
    shell.page("Phone numbers").body(Stack::gap(6).children((
        Grid::cols(2).gap(4).children((form_card(form), result)),
        states(),
    )))
}

/// `POST /phone`.
fn store(shell: &AppShell, submit: Result<Submit<NewContact>, Rejected<NewContact>>) -> Response {
    match submit {
        Ok(Submit(contact)) => {
            let form = FormView::new().of(&contact);
            page(shell, form, Some(&contact)).into_response()
        }
        Err(rejected) => rejected.respond(|form| page(shell, form, None)),
    }
}

pub struct PhonePages;

impl Injectable for PhonePages {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for PhonePages {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(Method::GET, "!/phone", |shell: AppShell| async move {
                page(&shell, FormView::new(), None)
            })
            .public(),
            RouteDef::on(
                Method::POST,
                "!/phone",
                |shell: AppShell, submit: Result<Submit<NewContact>, Rejected<NewContact>>| async move {
                    store(&shell, submit)
                },
            )
            .public(),
        ]
    }
}
