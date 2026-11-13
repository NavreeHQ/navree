//! `PhoneInput`: a phone number with the country it is read in.
//!
//! The page writes a select that posts the country (`phone_country`), the button and the
//! list that stand in for it where a script runs, the number, and a hidden value a script
//! writes the number's full form to. A native client has its own way to pick a country,
//! so what is read is the number, as a phone field, with the country as the second value
//! posted beside it (`country`): a client with a picker offers it, one without posts it as
//! it came.

use navree_native::input as i;
use navree_native::node as n;

use super::super::lower::{Control, Cx, N, Parts, Read, attr, elements, fail, has, hooks, id};
use crate::node::{Element, Node};

/// The hooks the root of a phone input carries that this reader answers for.
pub(crate) const HOOKS: &[&str] = &["data-ui-phone"];

/// `PhoneInput`.
pub(crate) fn phone(cx: &mut Cx, element: &Element) -> Read {
    let mut parts = Parts::of(element);
    let picker = parts.slot("phone-country");
    // What stands in for the select where a script runs.
    let trigger = parts.slot("phone-country-trigger");
    let list = parts.slot("phone-countries");
    let number = parts.slot("input");
    // What the script writes: the number in its full form, and what it says aloud.
    let _full = parts.slot("phone-e164");
    let _status = parts.slot("phone-status");
    let _flags = parts.slot("phone-flags");
    let mut fixed = None;
    for node in parts.left() {
        match node {
            Node::Element(hidden)
                if hidden.tag() == "input" && attr(hidden, "type").as_deref() == Some("hidden") =>
            {
                fixed = attr(hidden, "value").map(std::borrow::Cow::into_owned);
            }
            Node::Text(text) if text.trim().is_empty() => {}
            _ => return fail("a part of a phone input no reader here knows"),
        }
    }
    let (Some(picker), Some(number)) = (picker, number) else {
        return fail("a phone input with no country or no number");
    };
    if let Some(trigger) = trigger {
        hooks(trigger, &[])?;
    }
    if let Some(list) = list {
        hooks(list, &["data-ui-keep"])?;
    }
    hooks(picker, &[])?;
    let Some(name) = attr(picker, "name") else {
        return fail("a country that is posted under no name");
    };
    let chosen = selected(picker)?.or(fixed).unwrap_or_default();
    let Control::Field(mut field) = cx.control(number)? else {
        return fail("a phone input whose number is not a field");
    };
    let i::Input::Text(text) = &mut field.input else {
        return fail("a phone input whose number is not typed text");
    };
    text.country = Some(i::PhoneCountry {
        name: name.into_owned(),
        value: chosen,
    });
    field.id = id(number).or_else(|| id(element));
    Ok(vec![N::Field(*field)])
}

/// The code of the country the select has chosen: its selected option that holds one.
fn selected(picker: &Element) -> Read<Option<String>> {
    fn walk(element: &Element, out: &mut Option<String>) -> Read<()> {
        for child in elements(element)? {
            match child.tag() {
                "option" if has(child, "selected") => {
                    if out.is_none()
                        && let Some(value) = attr(child, "value").filter(|value| !value.is_empty())
                    {
                        *out = Some(value.into_owned());
                    }
                }
                "optgroup" => walk(child, out)?,
                _ => {}
            }
        }
        Ok(())
    }
    let mut out = None;
    walk(picker, &mut out)?;
    Ok(out)
}

#[cfg(all(test, feature = "phone"))]
mod tests {
    use navree_phone::Region;

    use super::super::testing::{misses, read, tamper};
    use super::*;
    use crate::PhoneInput;

    fn field_of(node: N) -> n::Field {
        let N::Field(field) = node else {
            panic!("not a field: {node:?}");
        };
        field
    }

    #[test]
    fn a_phone_input_is_a_phone_field_with_the_country_it_is_read_in() {
        let field = field_of(read(
            PhoneInput::new("phone")
                .region(Region::parse("IN"))
                .text("98765 43210")
                .required(),
        ));
        assert_eq!(field.name, "phone");
        assert!(field.required);
        let i::Input::Text(text) = &field.input else {
            panic!("not text");
        };
        assert_eq!(text.kind, i::TextKind::Phone);
        assert_eq!(text.value, "098765 43210");
        assert_eq!(
            text.country,
            Some(i::PhoneCountry {
                name: "phone_country".into(),
                value: "IN".into()
            })
        );
        assert!(text.placeholder.is_some());
    }

    #[test]
    fn a_country_not_chosen_is_said_empty_and_a_fixed_one_is_still_the_country() {
        let none = field_of(read(PhoneInput::new("phone")));
        let i::Input::Text(text) = &none.input else {
            panic!("not text");
        };
        assert_eq!(text.country.as_ref().map(|c| c.value.as_str()), Some(""));
        let fixed = field_of(read(
            PhoneInput::new("phone").region(Region::parse("IN")).readonly(),
        ));
        let i::Input::Text(text) = &fixed.input else {
            panic!("not text");
        };
        assert_eq!(text.country.as_ref().map(|c| c.value.as_str()), Some("IN"));
        assert!(text.readonly);
    }

    #[test]
    fn a_hook_no_reader_knows_inside_a_phone_input_is_still_a_miss() {
        for name in ["phone-input", "phone-country", "phone-country-trigger", "phone-countries"] {
            let node: Node = PhoneInput::new("phone").into();
            assert_eq!(
                misses(tamper(node, name, "data-ui-wizard")),
                ["hook:data-ui-wizard"],
                "{name}"
            );
        }
    }
}
