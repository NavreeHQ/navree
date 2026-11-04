//! Decimal numbers, dates with a time and instants in a derived form: the widget each
//! gets, how a value is shown when a form is edited, and how the post is read back.

use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use navree_ui::form::{Form, FormView, Widget, decode};
use navree_ui::{Form, InputKind, Validate, render};
use rust_decimal::Decimal;
use serde::Deserialize;

#[derive(Debug, Deserialize, Validate, Form)]
struct Invoice {
    /// Total
    #[validate(skip)]
    total: Decimal,
    /// Discount
    #[validate(skip)]
    discount: Option<Decimal>,
    /// Due
    #[validate(skip)]
    due: NaiveDateTime,
    /// Sent at
    #[validate(skip)]
    sent_at: DateTime<Utc>,
    /// Reminder
    #[validate(skip)]
    reminder: Option<DateTime<Utc>>,
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
        .collect()
}

fn good() -> Vec<(String, String)> {
    pairs(&[
        ("total", "1234.50"),
        ("discount", ""),
        ("due", "2026-11-04T14:30"),
        ("sent_at", "2026-11-04T09:05"),
        ("reminder", ""),
    ])
}

fn field(name: &str) -> &'static navree_ui::form::FormField {
    Invoice::FIELDS
        .iter()
        .find(|field| field.name == name)
        .unwrap()
}

#[test]
fn the_types_pick_a_number_input_and_a_date_and_time_input() {
    assert_eq!(field("total").widget, Widget::Input(InputKind::Number));
    assert_eq!(field("discount").widget, Widget::Input(InputKind::Number));
    assert!(field("discount").optional);
    assert_eq!(field("due").widget, Widget::Input(InputKind::DateTime));
    assert_eq!(field("sent_at").widget, Widget::Input(InputKind::DateTime));
    assert_eq!(field("reminder").widget, Widget::Input(InputKind::DateTime));
}

#[test]
fn a_decimal_draws_a_number_input_that_takes_fractions() {
    let html = render(&FormView::<Invoice>::new().post("/invoices").into());
    let at = html.find("name=\"total\"").expect("the total field");
    let tag = &html[html[..at].rfind("<input").unwrap()..];
    let tag = &tag[..tag.find('>').unwrap()];
    assert!(tag.contains("type=\"number\""), "{tag}");
    assert!(tag.contains("step=\"any\""), "{tag}");
    let at = html.find("name=\"due\"").expect("the due field");
    let tag = &html[html[..at].rfind("<input").unwrap()..];
    let tag = &tag[..tag.find('>').unwrap()];
    assert!(tag.contains("type=\"datetime-local\""), "{tag}");
}

#[test]
fn an_edit_form_shows_the_values_the_browser_can_hold() {
    let invoice = Invoice {
        total: "1234.50".parse().unwrap(),
        discount: None,
        due: NaiveDate::from_ymd_opt(2026, 11, 4)
            .unwrap()
            .and_hms_opt(14, 30, 0)
            .unwrap(),
        sent_at: NaiveDate::from_ymd_opt(2026, 11, 4)
            .unwrap()
            .and_hms_opt(9, 5, 0)
            .unwrap()
            .and_utc(),
        reminder: Some(
            NaiveDate::from_ymd_opt(2026, 11, 5)
                .unwrap()
                .and_hms_opt(8, 0, 30)
                .unwrap()
                .and_utc(),
        ),
    };
    let values = Invoice::values(&invoice);
    let value = |name: &str| {
        values
            .iter()
            .find(|(field, _)| *field == name)
            .map(|(_, value)| value.as_str())
    };
    assert_eq!(value("total"), Some("1234.50"), "the scale is kept");
    assert_eq!(value("discount"), None, "an empty optional shows empty");
    assert_eq!(value("due"), Some("2026-11-04T14:30"));
    assert_eq!(
        value("sent_at"),
        Some("2026-11-04T09:05"),
        "an instant shows in UTC"
    );
    assert_eq!(
        value("reminder"),
        Some("2026-11-05T08:00:30"),
        "seconds that are not zero are not dropped"
    );
    let html = render(&FormView::<Invoice>::new().of(&invoice).into());
    assert!(html.contains("value=\"1234.50\""), "{html}");
    assert!(html.contains("value=\"2026-11-04T14:30\""), "{html}");
}

#[test]
fn a_good_post_decodes() {
    let invoice: Invoice = decode(&good()).unwrap();
    assert_eq!(invoice.total, "1234.50".parse::<Decimal>().unwrap());
    assert_eq!(invoice.discount, None, "an empty optional is absent");
    assert_eq!(
        invoice.due,
        NaiveDate::from_ymd_opt(2026, 11, 4)
            .unwrap()
            .and_hms_opt(14, 30, 0)
            .unwrap(),
        "a browser posts the minute and no seconds"
    );
    assert_eq!(
        invoice.sent_at,
        NaiveDate::from_ymd_opt(2026, 11, 4)
            .unwrap()
            .and_hms_opt(9, 5, 0)
            .unwrap()
            .and_utc(),
        "an instant is read as UTC"
    );
    assert_eq!(invoice.reminder, None);
}

#[test]
fn seconds_a_fraction_and_signs_are_read_too() {
    let mut post = good();
    post[0].1 = "-0.005".to_owned();
    post[1].1 = "7".to_owned();
    post[2].1 = "2026-11-04T14:30:15".to_owned();
    post[3].1 = "2026-11-04T09:05:07".to_owned();
    post[4].1 = "2026-11-05T08:00".to_owned();
    let invoice: Invoice = decode(&post).unwrap();
    assert_eq!(invoice.total, "-0.005".parse::<Decimal>().unwrap());
    assert_eq!(invoice.discount, Some(Decimal::from(7)));
    assert_eq!(invoice.due.format("%H:%M:%S").to_string(), "14:30:15");
    assert_eq!(invoice.sent_at.format("%H:%M:%S").to_string(), "09:05:07");
    assert_eq!(
        invoice.reminder.map(|at| at.format("%d %H:%M").to_string()),
        Some("05 08:00".to_owned())
    );
}

#[test]
fn text_that_is_no_number_or_no_moment_is_a_field_error_not_a_crash() {
    let mut post = good();
    post[0].1 = "lots".to_owned();
    post[2].1 = "tomorrow".to_owned();
    post[3].1 = "2026-13-45T10:00".to_owned();
    let errors = decode::<Invoice>(&post).unwrap_err();
    assert!(errors["total"][0].contains("enter a number"), "{errors:?}");
    assert!(
        errors["due"][0].contains("enter a date and time"),
        "{errors:?}"
    );
    assert!(
        errors["sent_at"][0].contains("enter a date and time"),
        "{errors:?}"
    );
    // A number that is not there at all is refused as well.
    let mut post = good();
    post[0].1 = String::new();
    let errors = decode::<Invoice>(&post).unwrap_err();
    assert!(errors["total"][0].contains("enter a number"), "{errors:?}");
}

#[test]
fn a_leap_second_a_fraction_and_a_year_the_calendar_lacks_are_refused() {
    for bad in [
        "2026-12-31T23:59:60",
        "2026-11-04T10:00:00.5",
        "+12026-11-04T10:00",
        "0000-01-01T10:00",
    ] {
        let mut post = good();
        post[2].1 = bad.to_owned();
        let errors = decode::<Invoice>(&post).unwrap_err();
        assert!(errors.contains_key("due"), "{bad}: {errors:?}");
        let mut post = good();
        post[3].1 = bad.to_owned();
        let errors = decode::<Invoice>(&post).unwrap_err();
        assert!(errors.contains_key("sent_at"), "{bad}: {errors:?}");
    }
}

#[test]
fn an_instant_typed_with_its_own_offset_is_kept_as_typed() {
    // A field drawn as plain text takes RFC 3339 as written; the form adds no zone to it.
    let mut post = good();
    post[3].1 = "2026-11-04T09:05:00Z".to_owned();
    let invoice: Invoice = decode(&post).unwrap();
    assert_eq!(invoice.sent_at.to_rfc3339(), "2026-11-04T09:05:00+00:00");
    post[3].1 = "2026-11-04T14:35:00+05:30".to_owned();
    let invoice: Invoice = decode(&post).unwrap();
    assert_eq!(invoice.sent_at.to_rfc3339(), "2026-11-04T09:05:00+00:00");
}

fn input_tag(html: &str, name: &str) -> String {
    let at = html
        .find(&format!("name=\"{name}\""))
        .unwrap_or_else(|| panic!("no field {name} in {html}"));
    let start = html[..at].rfind("<input").unwrap();
    let end = at + html[at..].find('>').unwrap();
    html[start..end].to_owned()
}

#[test]
fn a_value_with_seconds_lets_the_browser_step_by_the_second() {
    let with = |due: &str| {
        render(
            &FormView::<Invoice>::new()
                .with_values(pairs(&[("due", due)]))
                .into(),
        )
    };
    let tag = input_tag(&with("2026-11-04T14:30"), "due");
    assert!(!tag.contains("step="), "to the minute needs no step: {tag}");
    let tag = input_tag(&with("2026-11-04T14:30:15"), "due");
    assert!(
        tag.contains("step=\"1\""),
        "seconds need a step of one: {tag}"
    );
}

#[test]
fn the_year_of_an_instant_is_held_to_the_calendar_in_utc_too() {
    // Wall time in range, UTC out of it, and the other way round.
    for bad in ["9999-12-31T23:30:00-05:00", "0001-01-01T00:30:00+05:00"] {
        let mut post = good();
        post[3].1 = bad.to_owned();
        let errors = decode::<Invoice>(&post).unwrap_err();
        assert_eq!(errors["sent_at"].len(), 1, "{bad}: {errors:?}");
    }
    // The edges that are in range in both are taken.
    for good_edge in [
        "9999-12-31T23:30:00+00:00",
        "0001-01-01T00:30:00+00:00",
        "9999-12-31T23:30:00+05:00",
        "0001-01-01T05:30:00+05:00",
    ] {
        let mut post = good();
        post[3].1 = good_edge.to_owned();
        assert!(decode::<Invoice>(&post).is_ok(), "{good_edge}");
    }
    // A date and time with no zone is wall time and nothing else, so an offset is not its.
    let mut post = good();
    post[2].1 = "2026-11-04T14:30:00+05:30".to_owned();
    let errors = decode::<Invoice>(&post).unwrap_err();
    assert_eq!(errors["due"].len(), 1, "{errors:?}");
    assert!(
        errors["due"][0].contains("enter a date and time"),
        "{errors:?}"
    );
}

#[derive(Debug, Deserialize, Validate, Form)]
struct Booking {
    /// Starts
    #[validate(custom(after_2000))]
    starts: chrono::DateTime<chrono::Utc>,
}

fn after_2000(at: &chrono::DateTime<chrono::Utc>) -> Result<(), String> {
    use chrono::Datelike;
    if at.year() < 2000 {
        return Err("Pick a time after 1999.".to_owned());
    }
    Ok(())
}

#[test]
fn a_bad_date_and_time_says_one_thing_about_itself() {
    // Both its own rule and the calendar are against it: the rule's message is the one.
    let errors = decode::<Booking>(&pairs(&[("starts", "0001-01-01T10:00")])).unwrap_err();
    assert_eq!(errors["starts"], ["Pick a time after 1999."], "{errors:?}");
    // The calendar alone is one message too.
    let mut post = good();
    post[3].1 = "2026-12-31T23:59:60".to_owned();
    let errors = decode::<Invoice>(&post).unwrap_err();
    assert_eq!(errors["sent_at"].len(), 1, "{errors:?}");
}

#[test]
fn a_full_path_instant_is_read_as_utc_and_says_so() {
    let starts = Booking::FIELDS.iter().find(|f| f.name == "starts").unwrap();
    assert!(
        starts.instant,
        "chrono::DateTime<chrono::Utc> is an instant"
    );
    assert!(!field("total").instant && !field("due").instant);
    // The input tells the person which clock it is.
    let html = render(&FormView::<Invoice>::new().post("/i").into());
    let hint = |name: &str| {
        let at = html.find(&format!("name=\"{name}\"")).unwrap();
        let rest = &html[at..];
        // The field's own box ends where the next one starts.
        let end = rest.find("<div data-slot=\"field\"").unwrap_or(rest.len());
        rest[..end].contains("Time in UTC")
    };
    assert!(hint("sent_at") && hint("reminder"), "{html}");
    assert!(
        !hint("due"),
        "a date and time with no zone is the person's own clock"
    );
    assert!(!hint("total"));
}

#[test]
fn a_decimal_is_read_exactly_or_refused() {
    let mut post = good();
    // 28 places are the most a Decimal holds.
    post[0].1 = "0.1234567890123456789012345678".to_owned();
    let invoice: Invoice = decode(&post).unwrap();
    assert_eq!(invoice.total.scale(), 28);
    // One more would be rounded: it is refused instead, with one message.
    post[0].1 = "0.12345678901234567890123456789".to_owned();
    let errors = decode::<Invoice>(&post).unwrap_err();
    assert_eq!(errors["total"].len(), 1, "{errors:?}");
    assert!(
        errors["total"][0].contains("at most 28 decimal places"),
        "{errors:?}"
    );
    // An optional decimal too.
    let mut post = good();
    post[1].1 = "1.00000000000000000000000000001".to_owned();
    let errors = decode::<Invoice>(&post).unwrap_err();
    assert!(errors.contains_key("discount"), "{errors:?}");
    // Not a number is still "enter a number", once.
    let mut post = good();
    post[0].1 = "many".to_owned();
    let errors = decode::<Invoice>(&post).unwrap_err();
    assert_eq!(errors["total"].len(), 1, "{errors:?}");
    assert!(errors["total"][0].contains("enter a number"), "{errors:?}");
}

#[derive(Debug, Deserialize, Validate, Form)]
struct Account {
    #[validate(length(min = 1))]
    email: String,
    #[validate(length(min = 8))]
    password: String,
    #[validate(skip)]
    new_password: Option<String>,
    #[validate(skip)]
    password_confirmation: String,
    #[validate(skip)]
    password_hint: String,
    #[validate(skip)]
    #[form(widget = text)]
    password_shown: String,
}

#[test]
fn a_text_field_named_for_a_password_is_a_password_input() {
    let widget = |name: &str| {
        Account::FIELDS
            .iter()
            .find(|field| field.name == name)
            .unwrap()
            .widget
    };
    assert_eq!(widget("password"), Widget::Input(InputKind::Password));
    assert_eq!(widget("new_password"), Widget::Input(InputKind::Password));
    assert_eq!(
        widget("password_confirmation"),
        Widget::Input(InputKind::Password)
    );
    assert_eq!(widget("email"), Widget::Input(InputKind::Text));
    assert_eq!(
        widget("password_hint"),
        Widget::Input(InputKind::Text),
        "a hint for a password is not one"
    );
    assert_eq!(
        widget("password_shown"),
        Widget::Input(InputKind::Text),
        "a widget that was named is the one used"
    );
    // And a refusal never writes any of them back, as the posted pairs go through it.
    let post = pairs(&[
        ("email", "a@b.example"),
        ("password", "short"),
        ("new_password", "hunter2hunter2"),
        ("password_confirmation", "hunter3hunter3"),
        ("password_hint", "my dog"),
        ("password_shown", "visible"),
    ]);
    let errors = decode::<Account>(&post).unwrap_err();
    assert!(errors.contains_key("password"), "{errors:?}");
    let html = render(&FormView::<Account>::new().into());
    for name in ["password", "new_password", "password_confirmation"] {
        let tag = input_tag(&html, name);
        assert!(tag.contains("type=\"password\""), "{name}: {tag}");
    }
}
