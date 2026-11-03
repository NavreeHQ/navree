//! The phone input: what it draws with and without a number, how a form
//! with a phone field posts and reads back, and what a page with one loads.

use navree_phone::Region;
use navree_ui::form::{Form, FormView, Widget, decode};
use navree_ui::{Field, Form, PhoneInput, Uses, Validate, View, assets, render};
use navree_validation::Phone;
use serde::Deserialize;

fn gb() -> Option<Region> {
    Region::parse("GB")
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

#[test]
fn an_empty_input_is_a_country_select_and_a_tel_box() {
    let html = render(&PhoneInput::new("phone").region(gb()).required().into());
    assert!(
        html.starts_with(
            r#"<div data-slot="phone-input" data-ui-phone data-ui-keep-attrs="data-enhanced""#
        ),
        "{html}"
    );
    assert!(html.contains(
        r#"<select data-slot="phone-country" name="phone_country" aria-label="Country""#
    ));
    assert!(html.contains(r#"<option value="GB" selected>United Kingdom +44</option>"#));
    assert!(html.contains(r#"<option value="IN">India +91</option>"#));
    // Names in alphabetical order, accents and all.
    let aland = html.find("Åland Islands").unwrap();
    assert!(html.find("Afghanistan").unwrap() < aland && aland < html.find("Albania").unwrap());
    assert!(html.contains(r#"aria-label="Country: United Kingdom +44""#));
    assert!(html.contains(r#"<span data-slot="phone-flag" aria-hidden="true">🇬🇧</span><span data-slot="phone-dial">+44</span>"#));
    assert!(
        html.contains(r#"type="tel" name="phone" autocomplete="tel" inputmode="tel" dir="ltr""#)
    );
    assert!(html.contains(" required"));
    assert!(html.contains(r#"placeholder="07400 123456""#));
    assert!(html.contains(r#"type="hidden" name="phone_e164" value="" disabled"#));
    assert!(html.contains(r#"role="status" aria-live="polite""#));
    assert!(!html.contains("<template"));
}

#[test]
fn a_number_is_written_the_way_its_country_writes_it() {
    let number = navree_phone::PhoneNumber::parse("+44 7400 123456", None).unwrap();
    // In its own country: nationally.
    let html = render(&PhoneInput::new("phone").number(&number).into());
    assert!(html.contains(r#"value="07400 123456""#), "{html}");
    assert!(html.contains(r#"<option value="GB" selected>"#));
    assert!(html.contains(r#"name="phone_e164" value="+447400123456""#));
    // A number of another country than the one asked for picks its own.
    let html = render(
        &PhoneInput::new("phone")
            .region(Region::parse("US"))
            .text("+447400123456")
            .into(),
    );
    assert!(
        html.contains(r#"value="07400 123456""#)
            && html.contains(r#"<option value="GB" selected>"#)
    );
    // Text that isn't a valid number stays as typed.
    let html = render(
        &PhoneInput::new("phone")
            .region(gb())
            .text("0740 x")
            .invalid(true)
            .into(),
    );
    assert!(html.contains(r#"value="0740 x""#) && html.contains(r#"aria-invalid="true""#));
    // No country yet: the select says to choose one.
    let html = render(&PhoneInput::new("phone").into());
    assert!(html.contains(r#"<option value="" selected>Country</option>"#));
    assert!(!html.contains("placeholder="));
}

#[test]
fn preferred_countries_and_own_flags() {
    fn flag(region: Region) -> navree_ui::Node {
        navree_ui::el("svg")
            .attr("data-code", region.as_str())
            .into()
    }
    let html = render(
        &PhoneInput::new("phone")
            .region(Region::parse("IN"))
            .preferred([Region::parse("IN").unwrap(), Region::parse("AE").unwrap()])
            .flags(flag)
            .into(),
    );
    assert!(html.contains(r#"<optgroup label="Suggested"><option value="IN" selected>India +91</option><option value="AE">United Arab Emirates +971</option></optgroup>"#), "{html}");
    // Listed once more among all, not chosen twice.
    assert_eq!(html.matches(r#"value="IN" selected"#).count(), 1);
    assert!(html.contains(r#"data-preferred="IN AE""#));
    assert!(html.contains(r#"<template data-slot="phone-flags"><span data-flag="AC"><svg data-code="AC"></svg></span>"#));
    assert!(html.contains(
        r#"<span data-slot="phone-flag" aria-hidden="true"><svg data-code="IN"></svg></span>"#
    ));
}

#[test]
fn a_field_labels_the_number_box() {
    let html = render(
        &Field::new("contact-phone", "Phone")
            .error("Enter a valid number.")
            .control(PhoneInput::new("phone").region(gb()))
            .into(),
    );
    assert!(html.contains(r#"<label data-slot="label" for="contact-phone">Phone</label>"#));
    assert!(html.contains(r#"name="phone" autocomplete="tel" inputmode="tel" dir="ltr" maxlength="40" placeholder="07400 123456" id="contact-phone" aria-invalid="true" aria-describedby="contact-phone-error""#), "{html}");
    assert!(!html.contains(r#"name="phone_country" aria-label="Country" autocomplete="tel-country-code" data-field-skip id="#));
}

#[test]
fn a_page_with_a_phone_input_loads_the_phone_and_overlay_modules() {
    let page = View::new("x").body(PhoneInput::new("phone").region(gb()));
    assert_eq!(page.uses(), Uses::PHONE.with(Uses::OVERLAY));
    let html = page.to_html();
    assert!(html.contains(assets::PHONE_URL) && html.contains(assets::OVERLAY_URL));
    assert!(!html.contains(assets::CORE_URL));
    // The plans are fetched on first use, not loaded with the page.
    assert!(!html.contains(&format!(r#"src="{}""#, assets::PHONE_DATA_URL)));
}

#[derive(Debug, Deserialize, Validate, Form)]
struct Contact {
    /// Name
    #[validate(length(min = 1))]
    name: String,
    /// Mobile
    #[validate(skip)]
    mobile: Phone,
    /// Office
    #[validate(skip)]
    office: Option<Phone>,
    /// Fax
    #[form(widget = phone)]
    #[validate(phone)]
    fax: Option<String>,
}

#[test]
fn the_derive_draws_phone_types_as_phone_inputs() {
    let widgets: Vec<(&str, Widget)> = Contact::FIELDS.iter().map(|f| (f.name, f.widget)).collect();
    assert_eq!(
        widgets[1..],
        [
            ("mobile", Widget::Phone),
            ("office", Widget::Phone),
            ("fax", Widget::Phone)
        ]
    );
    let html = render(
        &FormView::<Contact>::new()
            .post("/contacts")
            .phone_region(gb())
            .into(),
    );
    assert_eq!(
        html.matches(r#"data-slot="phone-input""#).count(),
        3,
        "{html}"
    );
    assert!(html.contains(r#"<label data-slot="label" for="contact-mobile">Mobile</label>"#));
    assert!(html.contains(r#"name="mobile" autocomplete="tel" inputmode="tel" dir="ltr" maxlength="40" placeholder="07400 123456" id="contact-mobile""#));
}

#[test]
fn a_posted_number_is_read_in_its_country() {
    let contact = decode::<Contact>(&pairs(&[
        ("name", "Ada"),
        ("mobile", "07400 123456"),
        ("mobile_country", "GB"),
        ("office", "020 7946 0018 ext. 12"),
        ("office_country", "GB"),
        ("fax", "(650) 253-0000"),
        ("fax_country", "US"),
        ("mobile_e164", "+15550000000"),
    ]))
    .unwrap();
    assert_eq!(contact.mobile.as_str(), "+447400123456");
    let office = contact.office.unwrap();
    assert_eq!(
        (office.as_str(), office.extension()),
        ("+442079460018", Some("12"))
    );
    assert_eq!(contact.fax.as_deref(), Some("+16502530000"));

    // An international number needs no country; an empty optional one is none.
    let contact = decode::<Contact>(&pairs(&[
        ("name", "Ada"),
        ("mobile", "+91 98765 43210"),
        ("mobile_country", ""),
        ("office", ""),
        ("office_country", "GB"),
    ]))
    .unwrap();
    assert_eq!(contact.mobile.as_str(), "+919876543210");
    assert!(contact.office.is_none() && contact.fax.is_none());
}

#[test]
fn a_number_that_is_not_valid_in_its_country_is_a_field_error() {
    // Valid in the United Kingdom, not in France: the country chosen counts.
    let errors = decode::<Contact>(&pairs(&[
        ("name", "Ada"),
        ("mobile", "07400 123456"),
        ("mobile_country", "FR"),
    ]))
    .unwrap_err();
    assert_eq!(
        errors["mobile"],
        ["Mobile: enter a phone number, with its country."]
    );
    // Without a country, a national number can't be read.
    let errors =
        decode::<Contact>(&pairs(&[("name", "Ada"), ("mobile", "07400 123456")])).unwrap_err();
    assert!(errors.contains_key("mobile"), "{errors:?}");
    // A text field checked by the phone rule reports through the rule.
    let errors = decode::<Contact>(&pairs(&[
        ("name", "Ada"),
        ("mobile", "+447400123456"),
        ("fax", "12"),
        ("fax_country", "GB"),
    ]))
    .unwrap_err();
    assert!(errors.contains_key("fax"), "{errors:?}");
}

#[test]
fn a_rejected_form_shows_the_number_and_country_again() {
    let html = render(
        &FormView::<Contact>::new()
            .post("/contacts")
            .with_values(pairs(&[("mobile", "0612"), ("mobile_country", "FR")]))
            .with_errors(
                [(
                    "mobile".to_owned(),
                    vec!["Mobile: enter a phone number, with its country.".to_owned()],
                )]
                .into(),
            )
            .into(),
    );
    assert!(html.contains(r#"value="0612""#), "{html}");
    assert!(html.contains(r#"<option value="FR" selected>France +33</option>"#));
    assert!(html.contains(r#"aria-invalid="true""#));
    // A saved number is shown nationally, in its own country.
    let html = render(
        &FormView::<Contact>::new()
            .with_values(pairs(&[("mobile", "+33612345678")]))
            .phone_region(gb())
            .into(),
    );
    assert!(
        html.contains(r#"value="06 12 34 56 78""#)
            && html.contains(r#"<option value="FR" selected>"#),
        "{html}"
    );
}
