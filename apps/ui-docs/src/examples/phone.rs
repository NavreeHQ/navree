//! PhoneInput.

use navree::phone::{PhoneNumber, Region};
use navree::ui::*;

use crate::catalog::{Example, gallery, single};

const SOURCE: &str = include_str!("phone.rs");

pub fn all() -> Vec<Example> {
    vec![
        single!("phone-input", "thumb", "A phone input", phone_thumb).start(),
        single!("phone-input", "default", "A phone field", phone_default).start(),
        single!("phone-input", "saved", "A saved number", phone_saved).start(),
        gallery!("phone-input", "states", "States", phone_states).medium(),
        single!(
            "phone-input",
            "preferred",
            "Suggested countries first",
            phone_preferred
        )
        .start(),
        single!("phone-input", "rtl", "Right to left", phone_rtl).start(),
    ]
}

fn phone_thumb() -> PhoneInput {
    PhoneInput::new("phone").country("GB")
}

fn phone_default() -> Field {
    Field::new("contact-mobile", "Mobile")
        .help("Type it as you would dial it, or paste it with its country code.")
        .control(PhoneInput::new("mobile").country("GB"))
}

fn phone_saved() -> Field {
    let saved = PhoneNumber::parse("+1 650 253 0000", None).ok();
    let mut input = PhoneInput::new("office").country("GB");
    if let Some(saved) = &saved {
        input = input.number(saved);
    }
    Field::new("contact-office", "Office").control(input)
}

fn phone_states() -> Vec<(&'static str, Node)> {
    vec![
        ("No country yet", PhoneInput::new("a").into()),
        (
            "Invalid",
            Field::new("state-invalid", "Mobile")
                .error("Enter a valid number for France.")
                .control(
                    PhoneInput::new("b")
                        .country("FR")
                        .text("06 12")
                        .invalid(true),
                )
                .into(),
        ),
        (
            "Disabled",
            PhoneInput::new("c").country("DE").disabled(true).into(),
        ),
        (
            "Read only",
            PhoneInput::new("d")
                .country("IN")
                .text("098765 43210")
                .readonly()
                .into(),
        ),
    ]
}

fn phone_preferred() -> PhoneInput {
    PhoneInput::new("phone").country("AE").preferred(
        ["AE", "SA", "QA", "GB", "IN"]
            .into_iter()
            .filter_map(Region::parse),
    )
}

fn phone_rtl() -> Direction {
    Direction::rtl(Field::new("rtl-mobile", "رقم الجوال").control(
        PhoneInput::new("mobile").country("AE").labels(PhoneLabels {
            country: "الدولة".into(),
            choose: "الدولة".into(),
            preferred: "مقترحة".into(),
            search: "ابحث عن دولة".into(),
            empty: "لا توجد دولة.".into(),
            country_set: "تم اختيار {country}".into(),
            count: "{n} دولة".into(),
        }),
    ))
}
