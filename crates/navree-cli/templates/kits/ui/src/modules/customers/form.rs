//! The form that adds a customer and edits one. Its doc comments are the
//! labels, its types pick the widgets, and its `#[validate]` rules run in the
//! browser and again on the server.

use chrono::NaiveDate;
use navree::ui::{Form, Money, Validate};
use serde::Deserialize;

use super::entity::{Customer, Plan, Status};
use crate::ui::format::e164;

/// A customer as the form has it.
#[derive(Debug, Deserialize, Validate, Form)]
pub struct CustomerForm {
    /// Name
    #[validate(length(min = 2, max = 120))]
    #[form(
        placeholder = "Anjali Verma",
        wide,
        autofocus,
        error = "Enter a name of 2 to 120 characters."
    )]
    pub name: String,
    /// Email
    #[validate(email)]
    #[form(
        placeholder = "anjali.verma@example.com",
        error = "Enter a valid email address."
    )]
    pub email: String,
    /// Phone
    #[validate(phone)]
    #[form(
        placeholder = "+91 98765 43210",
        help = "With the country code.",
        error = "Enter a phone number with its country code."
    )]
    pub phone: String,
    /// City
    #[validate(length(min = 2, max = 80))]
    #[form(placeholder = "Ludhiana", error = "Enter the city.")]
    pub city: String,
    /// Plan
    #[validate(skip)]
    pub plan: Plan,
    /// Status
    #[validate(skip)]
    pub status: Status,
    /// Monthly fee (₹)
    #[validate(skip)]
    #[form(placeholder = "29,999", help = "Per month, in rupees.")]
    pub monthly: Money,
    /// Customer since
    #[validate(skip)]
    pub since: NaiveDate,
    /// Auto-pay
    #[validate(skip)]
    pub autopay: bool,
    /// Notes
    #[validate(length(max = 500))]
    #[form(widget = textarea, wide, rows = 3)]
    pub notes: Option<String>,
}

impl CustomerForm {
    /// The form for a customer that is not there yet: a Growth plan on trial,
    /// joining today.
    pub fn blank(today: NaiveDate) -> Self {
        Self {
            name: String::new(),
            email: String::new(),
            phone: String::new(),
            city: String::new(),
            plan: Plan::Growth,
            status: Status::Trial,
            monthly: Money(0),
            since: today,
            autopay: false,
            notes: None,
        }
    }

    /// The form filled with what is stored about `customer`.
    pub fn of(customer: &Customer) -> Self {
        Self {
            name: customer.name.clone(),
            email: customer.email.clone(),
            phone: customer.phone.clone(),
            city: customer.city.clone(),
            plan: Plan::parse(&customer.plan),
            status: Status::parse(&customer.status),
            monthly: Money(customer.monthly_paise),
            since: customer.since,
            autopay: customer.autopay,
            notes: customer.notes.clone(),
        }
    }

    /// Writes the form's values onto `customer`. The email is kept in lower
    /// case and the phone number as `+` and digits.
    pub fn apply(self, customer: &mut Customer) {
        customer.name = self.name.trim().to_owned();
        customer.email = self.email.trim().to_lowercase();
        customer.phone = e164(&self.phone);
        customer.city = self.city.trim().to_owned();
        customer.plan = self.plan.key().to_owned();
        customer.status = self.status.key().to_owned();
        customer.monthly_paise = self.monthly.0;
        customer.since = self.since;
        customer.autopay = self.autopay;
        customer.notes = self.notes.filter(|notes| !notes.trim().is_empty());
    }

    /// A new customer with the form's values.
    pub fn into_customer(self) -> Customer {
        let mut customer = Customer::default();
        self.apply(&mut customer);
        customer
    }
}
