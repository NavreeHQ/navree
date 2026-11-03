//! `navree_phone`: phone numbers parsed from any text, in any region.
//!
//! Input: a region byte (0 for none, else an index into the regions), then
//! the text.
//!
//! Invariants: a parsed number is `+` and at most 15 digits, fits its plan
//! (it has a type, and is valid for its region), and reads back to the same
//! number from its international and RFC 3966 forms, and from E.164 without
//! its extension; its national
//! form reads back, in its region, to a number written the same way
//! nationally. A number accepted only as possible has a possible length and
//! reads back from E.164. Nothing panics.
#![no_main]

use libfuzzer_sys::fuzz_target;
use navree_phone::{PhoneFormat, PhoneNumber, Region};

fn check_valid(number: PhoneNumber) {
    let e164 = number.as_str();
    assert!(e164.starts_with('+'), "{e164}");
    assert!(
        (3..=16).contains(&e164.len()) && e164[1..].bytes().all(|b| b.is_ascii_digit()),
        "{e164} is not E.164"
    );
    assert!(number.number_type().is_some(), "{e164} has no type");
    if let Some(region) = number.region() {
        assert!(number.is_valid_for(region), "{e164} not valid for {region}");
        assert_eq!(region.calling_code(), number.country_code());
    }
    if let Some(extension) = number.extension() {
        assert!(extension.bytes().all(|b| b.is_ascii_digit()), "{extension:?}");
    }
    for how in [
        PhoneFormat::E164,
        PhoneFormat::International,
        PhoneFormat::Rfc3966,
    ] {
        let written = number.format(how);
        // E.164 has no room for an extension: it reads back without one.
        let want = if how == PhoneFormat::E164 {
            number.without_extension()
        } else {
            number
        };
        assert_eq!(
            PhoneNumber::parse(&written, None),
            Ok(want),
            "{e164} written {how:?} as {written:?}"
        );
    }
    if let Some(region) = number.region() {
        let national = number.national();
        let read = PhoneNumber::parse(&national, Some(region))
            .unwrap_or_else(|error| panic!("{e164} written nationally as {national:?}: {error}"));
        assert_eq!(read.national(), national, "{e164} national form");
    }
    let _ = number.is_internationally_diallable();
}

fn check_possible(number: PhoneNumber) {
    let e164 = number.as_str();
    assert!(e164.len() <= 16, "{e164}");
    let again = PhoneNumber::parse_possible(&number.without_extension().to_string(), None);
    assert_eq!(again, Ok(number.without_extension()), "{e164}");
    let _ = (number.international(), number.national(), number.rfc3966());
}

fuzz_target!(|data: &[u8]| {
    let Some((&region, rest)) = data.split_first() else {
        return;
    };
    let Ok(text) = std::str::from_utf8(rest) else {
        return;
    };
    let regions: Vec<Region> = Region::all().collect();
    let region = (region != 0).then(|| regions[usize::from(region) % regions.len()]);
    if let Ok(number) = PhoneNumber::parse(text, region) {
        check_valid(number);
    }
    if let Ok(number) = PhoneNumber::parse_possible(text, region) {
        check_possible(number);
    }
    let _ = text.parse::<Region>();
});
