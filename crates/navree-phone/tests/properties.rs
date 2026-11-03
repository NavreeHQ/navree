//! Properties on generated input: nothing panics, what parses is E.164 of
//! at most 15 digits, and every number reads back from its own formats.

use std::time::{Duration, Instant};

use navree_phone::{NumberType, PhoneFormat, PhoneNumber, Region};
use proptest::prelude::*;

/// Reads `number`'s formats back and checks they give the same number.
fn reads_back(number: PhoneNumber) {
    let e164 = number.as_str();
    assert!(e164.starts_with('+'), "{e164}");
    assert!(e164.len() <= 16, "{e164}");
    assert!(e164[1..].bytes().all(|b| b.is_ascii_digit()), "{e164}");
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
        // A few plans write two numbers the same way nationally (Gabon's
        // `+2412000000` and `+24102000000` are both `02 00 00 00`), and some
        // national forms read as another kind of number (an Argentine landline
        // whose local number starts with 15, the mobile marker). So the
        // national form, when it reads back, reads as a number written the
        // same way. Every example number in the plans reads back exactly.
        let national = number.national();
        if let Ok(read) = PhoneNumber::parse(&national, Some(region)) {
            assert_eq!(read.national(), national, "{e164} written nationally");
        }
    }
}

/// Some region, by index.
fn region(at: usize) -> Region {
    let all: Vec<Region> = Region::all().collect();
    all[at % all.len()]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4000))]

    #[test]
    fn any_text_parses_or_fails_without_panicking(text in ".{0,80}", at in 0usize..245, with_region: bool) {
        let region = with_region.then(|| region(at));
        if let Ok(number) = PhoneNumber::parse(&text, region) {
            reads_back(number);
        }
        let _ = PhoneNumber::parse_possible(&text, region);
    }

    #[test]
    fn phone_shaped_text_reads_back(
        plus: bool,
        digits in "[0-9]{2,18}",
        separators in prop::collection::vec(prop::sample::select(vec![" ", "-", ".", "(", ")", "", ""]), 0..18),
        at in 0usize..245,
    ) {
        let mut text = String::from(if plus { "+" } else { "" });
        for (i, digit) in digits.chars().enumerate() {
            text.push(digit);
            if let Some(separator) = separators.get(i) {
                text.push_str(separator);
            }
        }
        if let Ok(number) = PhoneNumber::parse(&text, Some(region(at))) {
            reads_back(number);
        }
    }

    #[test]
    fn numbers_near_every_example_read_back(at in 0usize..245, kind in 0usize..11, tail in "[0-9]{1,4}") {
        let region = region(at);
        if let Some(example) = region.example(NumberType::ALL[kind]) {
            // The example with its last digits changed: often another valid number.
            let nsn = example.national_number();
            let keep = nsn.len().saturating_sub(tail.len());
            let changed = format!("+{}{}{}", example.country_code(), &nsn[..keep], tail);
            if let Ok(number) = PhoneNumber::parse(&changed, None) {
                reads_back(number);
            }
        }
    }
}

#[test]
fn hostile_input_is_refused_quickly() {
    let inputs = [
        "+".repeat(250),
        "1".repeat(250),
        "(".repeat(249) + "1",
        "1-".repeat(125),
        ",".repeat(200) + "12",
        format!("+1 650 253 0000{}", " x".repeat(110)),
        format!("+1 650 253 0000 ext{}", ".".repeat(200)),
        "tel:".repeat(60),
        ";phone-context=+1".repeat(14),
        "٤".repeat(250),
        "\u{0}".repeat(250),
        "é".repeat(250),
    ];
    for input in &inputs {
        let start = Instant::now();
        for region in [None, Region::parse("US")] {
            let _ = PhoneNumber::parse(input, region);
            let _ = PhoneNumber::parse_possible(input, region);
        }
        let took = start.elapsed();
        assert!(
            took < Duration::from_millis(50),
            "{:?}...: {took:?}",
            &input[..10]
        );
    }
}

#[test]
fn a_number_with_an_extension_reads_back_from_its_formats() {
    // Found by fuzzing: the extension does not survive E.164, and must not have to.
    let gb = Region::parse("GB");
    let number = PhoneNumber::parse("020 7946 0018,,12", gb).unwrap();
    assert_eq!(number.extension(), Some("12"));
    reads_back(number);
}

#[test]
fn an_argentine_landline_starting_with_15_reads_but_its_national_form_does_not() {
    // Found by fuzzing: the plan reads 15 after the area code as the mobile
    // marker, so this landline's national form names another number.
    let number = PhoneNumber::parse("+54 11 1555 5595", None).unwrap();
    assert_eq!(number.national(), "011 1555-5595");
    assert!(PhoneNumber::parse(&number.national(), Region::parse("AR")).is_err());
    reads_back(number);
}
