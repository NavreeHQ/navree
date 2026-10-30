//! The parsers behind navree-validation's `email` and `url` rules, and the
//! derive's rules on a whole DTO.
//!
//! Input: a mode byte (0 email, 1 URL, 2 a JSON body for a derived DTO),
//! then the text.
//!
//! Invariants: an accepted email is ASCII, at most 254 bytes, one `@`
//! between a dot-atom local part of at most 64 bytes and a domain without
//! an empty label; an accepted URL is ASCII with no space or control
//! character, and parses with the `url` crate too (punycode labels aside,
//! which the `url` crate decodes and the rule leaves to the host); the
//! rule functions agree with the parsers; a DTO's validation never panics
//! and every violation's path is one of the DTO's fields.
#![no_main]

use libfuzzer_sys::fuzz_target;
use navree_validation::rules::{self, is_email, is_url};
use navree_validation::{Messages, Validate, validate_json};
use serde::Deserialize;

fn email(text: &str) {
    let accepted = is_email(text);
    assert_eq!(rules::email(text).is_ok(), accepted);
    if !accepted {
        return;
    }
    assert!(text.is_ascii() && text.len() <= 254, "{text:?}");
    let (local, domain) = text.split_once('@').expect("an `@`");
    assert!(!domain.contains('@'), "{text:?} has two `@`");
    assert!(!local.is_empty() && local.len() <= 64, "{text:?}");
    assert!(
        local.split('.').all(|atom| !atom.is_empty()),
        "{text:?} has an empty atom"
    );
    assert!(
        domain.starts_with('[') || domain.split('.').all(|label| !label.is_empty()),
        "{text:?} has an empty label"
    );
    assert!(
        !text
            .bytes()
            .any(|b| b.is_ascii_whitespace() || b.is_ascii_control()),
        "{text:?}"
    );
    assert!(!is_email(&format!("{text} ")));
}

fn url(text: &str) {
    let accepted = is_url(text);
    assert_eq!(rules::url(text).is_ok(), accepted);
    if !accepted {
        return;
    }
    assert!(text.is_ascii(), "{text:?}");
    assert!(
        !text
            .bytes()
            .any(|b| b.is_ascii_whitespace() || b.is_ascii_control()),
        "{text:?}"
    );
    let (scheme, _) = text.split_once(':').expect("a scheme");
    assert!(scheme.starts_with(|c: char| c.is_ascii_alphabetic()));
    if !text.to_ascii_lowercase().contains("xn--") {
        assert!(
            url::Url::parse(text).is_ok(),
            "{text:?} passed the rule but not the url crate"
        );
    }
}

#[derive(Debug, Deserialize, Validate)]
struct Signup {
    #[validate(required, email)]
    email: Option<String>,
    #[validate(url)]
    website: Option<String>,
    #[validate(length(min = 1, max = 64), custom(rules::unicode_name))]
    name: String,
    #[validate(
        length(max = 8),
        each(length(min = 1, max = 16), pattern("^[a-z0-9-]+$"))
    )]
    #[serde(default)]
    tags: Vec<String>,
    #[validate(range(min = 13, max = 130))]
    age: Option<u16>,
    #[validate(nested)]
    #[serde(default)]
    addresses: Vec<Address>,
}

#[derive(Debug, Deserialize, Validate)]
struct Address {
    #[validate(length(min = 1, max = 100))]
    city: String,
    #[validate(custom(rules::country))]
    country: String,
}

const FIELDS: &[&str] = &[
    "email",
    "website",
    "name",
    "tags",
    "age",
    "addresses",
    "input",
];

fn dto(text: &str) {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(text) else {
        return;
    };
    match validate_json::<Signup>(&json, &Messages::default()) {
        Ok(signup) => assert!(signup.validate().is_ok()),
        Err(errors) => {
            for (path, messages) in errors.iter() {
                let field = path.split('.').next().unwrap_or_default();
                assert!(FIELDS.contains(&field), "unexpected path {path:?}");
                assert!(!messages.is_empty());
            }
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    let Ok(text) = std::str::from_utf8(rest) else {
        return;
    };
    match mode % 3 {
        0 => email(text),
        1 => url(text),
        _ => dto(text),
    }
});
