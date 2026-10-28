//! `navree_validation`'s international validators: E.164 phone numbers,
//! unicode names, countries, locales, digit folding and sanitized HTML.
//!
//! Input: a region byte (0 for none, else an index into the countries), the
//! text, and optionally after a NUL byte a custom HTML policy: allowed tags
//! separated by commas, a NUL, the allowed URL schemes separated by commas,
//! a NUL, the `rel` to set on links.
//!
//! Invariants: a parsed phone number is `+` and at most 15 digits, reads
//! back from its own E.164, international and national forms to the same
//! number; digit folding is idempotent; a locale or country normalizes to
//! a value that parses to itself; sanitized HTML never keeps a script,
//! style or frame element, and text-only output has no raw `<`.
#![no_main]

use std::collections::{BTreeMap, BTreeSet};

use libfuzzer_sys::fuzz_target;
use navree_validation::{Country, HtmlPolicy, Locale, Phone, fold_digits, rules};

fn phone(text: &str, region: u8) {
    let countries: Vec<Country> = Country::all().collect();
    let region = (region != 0).then(|| countries[usize::from(region) % countries.len()]);
    let Ok(number) = Phone::parse(text, region) else {
        return;
    };
    let e164 = number.as_str();
    assert!(e164.starts_with('+'), "{e164}");
    assert!(
        e164[1..].bytes().all(|b| b.is_ascii_digit()) && (2..=16).contains(&e164.len()),
        "{e164} is not E.164"
    );
    assert!(!number.international().is_empty());
    for (name, form, from) in [
        ("e164", e164.to_owned(), None),
        ("international", number.international(), None),
        ("national", number.national(), number.region()),
    ] {
        match Phone::parse(&form, from) {
            Ok(again) => assert_eq!(
                again.as_str(),
                e164,
                "{name} form {form:?} reads as another number"
            ),
            // A national form without a region cannot be read back.
            Err(_) => assert!(
                name == "national" && number.region().is_none(),
                "{name} {form:?}"
            ),
        }
    }
}

fn names_and_tags(text: &str) {
    if rules::unicode_name(text, &()).is_ok() {
        assert!(
            !text.chars().any(|c| c.is_control()
                || matches!(c, '<' | '>' | '&' | '"' | '/' | '\\' | ';' | '=' | '`')),
            "{text:?} passed as a name"
        );
        assert!(text.chars().any(char::is_alphanumeric));
    }
    let folded = fold_digits(text);
    assert_eq!(fold_digits(&folded), folded, "folding is idempotent");
    let _ = rules::phone(text, &());
    let _ = rules::country(text, &());
    let _ = rules::locale(text, &());
    if let Ok(country) = Country::parse(text) {
        assert_eq!(Country::parse(country.as_str()), Ok(country));
    }
    if let Ok(locale) = Locale::parse(text) {
        let again = Locale::parse(locale.as_str()).expect("a normalized locale parses");
        assert_eq!(again.as_str(), locale.as_str(), "locale normalization");
        let _ = (
            locale.language(),
            locale.script(),
            locale.region(),
            locale.country(),
        );
    }
}

fn check_safe(clean: &str) {
    let lower = clean.to_ascii_lowercase();
    for forbidden in ["<script", "<style", "<iframe", "<object", "<embed"] {
        assert!(!lower.contains(forbidden), "{forbidden} kept in {clean:?}");
    }
}

fn html(text: &str, policy: Option<(&str, &str, &str)>) {
    let text_only = HtmlPolicy::text_only().clean(text);
    assert!(!text_only.contains('<'), "raw `<` in {text_only:?}");
    check_safe(&HtmlPolicy::default().clean(text));

    if let Some((tags, schemes, rel)) = policy {
        let list = |text: &str| -> BTreeSet<String> {
            text.split(',').map(str::to_owned).collect()
        };
        let tags = list(tags);
        let policy = HtmlPolicy {
            tag_attributes: tags
                .iter()
                .map(|tag| (tag.clone(), list("href,src,title,style,class")))
                .collect::<BTreeMap<_, _>>(),
            tags,
            generic_attributes: BTreeSet::new(),
            url_schemes: list(schemes),
            link_rel: rel.to_owned(),
        };
        // The module refuses to boot with a policy that does not check.
        if policy.check().is_ok() {
            check_safe(&policy.clean(text));
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((&region, rest)) = data.split_first() else {
        return;
    };
    let Ok(rest) = std::str::from_utf8(rest) else {
        return;
    };
    let (text, policy) = match rest.split_once('\0') {
        Some((text, policy)) => (text, Some(policy)),
        None => (rest, None),
    };
    phone(text, region);
    names_and_tags(text);
    html(
        text,
        policy.map(|policy| {
            let mut parts = policy.splitn(3, '\0');
            (
                parts.next().unwrap_or(""),
                parts.next().unwrap_or(""),
                parts.next().unwrap_or(""),
            )
        }),
    );
});
