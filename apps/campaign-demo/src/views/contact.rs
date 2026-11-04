//! A contact's details: one struct is the form, its rules and what the server
//! reads back. The mobile number is read in the country chosen beside it, the
//! country is found by typing, and the date is typed or picked.

use chrono::NaiveDate;
use navree::phone::Region;
use navree::ui::flags::CountryCode;
use navree::ui::form::FormView;
use navree::ui::*;
use navree::validation::Phone;
use serde::Deserialize;

use crate::data::Recipient;

/// A contact, as the form edits it.
#[derive(Debug, Deserialize, Validate, Form)]
#[form(id = "contact")]
pub struct ContactForm {
    /// Name
    #[validate(length(min = 1, max = 120))]
    #[form(error = "Give the contact a name.")]
    pub name: String,
    /// Email
    #[validate(email)]
    #[form(widget = email, error = "Enter an email address, such as maya@example.com.")]
    pub email: String,
    /// Role
    #[validate(length(max = 120))]
    #[form(placeholder = "Product manager")]
    pub role: String,
    /// Mobile
    #[validate(skip)]
    #[form(help = "Type it as you would dial it, or paste it with its country code.")]
    pub mobile: Option<Phone>,
    /// Country
    #[validate(length(min = 2, max = 2))]
    #[form(
        widget = combobox,
        placeholder = "Search countries",
        error = "Choose a country from the list."
    )]
    pub country: String,
    /// Customer since
    #[validate(skip)]
    #[form(widget = date_picker, help = "Type it as 3/14/2025, or pick it.")]
    pub customer_since: Option<NaiveDate>,
}

impl ContactForm {
    /// The form for a recipient as stored.
    pub fn of(person: &Recipient) -> Self {
        Self {
            name: person.name.clone(),
            email: person.email.clone(),
            role: person.role.clone(),
            mobile: Phone::parse(&person.phone, None).ok(),
            country: person.country.clone(),
            customer_since: person.customer_since,
        }
    }
}

/// The flag of a country as a picture the app serves, for a list of choices.
pub fn flag_picture(code: &str) -> String {
    format!("/flags/{}.svg", code.to_ascii_lowercase())
}

/// A flag in a phone input's country list.
fn phone_flag(region: Region) -> Node {
    el("img")
        .attr("src", flag_picture(region.as_str()))
        .attr("alt", "")
        .attr("width", 20_i64)
        .attr("height", 15_i64)
        .attr("loading", "lazy")
        .into()
}

/// Every country, each with its flag, found by its name or its code.
pub fn countries() -> Vec<Choice> {
    let mut all: Vec<CountryCode> = CountryCode::countries().collect();
    all.sort_by_key(|code| code.name());
    all.into_iter()
        .map(|code| {
            Choice::new(code.as_str(), code.name())
                .avatar(flag_picture(code.as_str()))
                .keywords([code.as_str()])
        })
        .collect()
}

/// The form, filled from `form` (a recipient, or what was sent back).
pub fn contact_form(form: FormView<ContactForm>, id: i64, today: NaiveDate) -> Card {
    Card::titled("Contact details")
        .description("What the list knows about this person. Numbers are checked on the server against each country's numbering plan.")
        .body(
            form.post(format!("/recipients/{id}/edit"))
                .choices("country", countries())
                .phone_flags(phone_flag)
                .dates(navree::ui::dates::DateLocale::english(), today)
                .submit("Save contact")
                .cancel("Cancel", format!("/recipients/{id}")),
        )
}
