//! Numbers people write, read and written the way their countries write
//! them.

use navree_phone::{NumberType, ParseError, PhoneFormat, PhoneNumber, Region};

fn region(code: &str) -> Option<Region> {
    Some(Region::parse(code).unwrap_or_else(|| panic!("no region {code}")))
}

fn parse(text: &str, code: Option<&str>) -> PhoneNumber {
    PhoneNumber::parse(text, code.and_then(region))
        .unwrap_or_else(|error| panic!("{text} in {code:?}: {error}"))
}

#[test]
fn numbers_in_their_countries_formats() {
    // (input, region, E.164, international, national, type)
    let cases: &[(&str, &str, &str, &str, &str, NumberType)] = &[
        (
            "650-253-0000",
            "US",
            "+16502530000",
            "+1 650-253-0000",
            "(650) 253-0000",
            NumberType::FixedLineOrMobile,
        ),
        (
            "020 7946 0018",
            "GB",
            "+442079460018",
            "+44 20 7946 0018",
            "020 7946 0018",
            NumberType::FixedLine,
        ),
        (
            "07400 123456",
            "GB",
            "+447400123456",
            "+44 7400 123456",
            "07400 123456",
            NumberType::Mobile,
        ),
        (
            "030 123456",
            "DE",
            "+4930123456",
            "+49 30 123456",
            "030 123456",
            NumberType::FixedLine,
        ),
        (
            "02 1234 5678",
            "IT",
            "+390212345678",
            "+39 02 1234 5678",
            "02 1234 5678",
            NumberType::FixedLine,
        ),
        (
            "06 12 34 56 78",
            "FR",
            "+33612345678",
            "+33 6 12 34 56 78",
            "06 12 34 56 78",
            NumberType::Mobile,
        ),
        (
            "0412 345 678",
            "AU",
            "+61412345678",
            "+61 412 345 678",
            "0412 345 678",
            NumberType::Mobile,
        ),
        (
            "098765 43210",
            "IN",
            "+919876543210",
            "+91 98765 43210",
            "098765 43210",
            NumberType::Mobile,
        ),
        (
            "(11) 96123-4567",
            "BR",
            "+5511961234567",
            "+55 11 96123-4567",
            "(11) 96123-4567",
            NumberType::Mobile,
        ),
        (
            "8 (495) 123-45-67",
            "RU",
            "+74951234567",
            "+7 495 123-45-67",
            "8 (495) 123-45-67",
            NumberType::FixedLine,
        ),
        (
            "050 123 4567",
            "AE",
            "+971501234567",
            "+971 50 123 4567",
            "050 123 4567",
            NumberType::Mobile,
        ),
        (
            "0100 123 4567",
            "EG",
            "+201001234567",
            "+20 10 01234567",
            "010 01234567",
            NumberType::Mobile,
        ),
    ];
    let mut failures = Vec::new();
    for &(input, code, e164, international, national, kind) in cases {
        let number = match PhoneNumber::parse(input, region(code)) {
            Ok(number) => number,
            Err(error) => {
                failures.push(format!("{input} in {code}: {error}"));
                continue;
            }
        };
        let got = (
            number.as_str().to_owned(),
            number.international(),
            number.national(),
            number.number_type(),
            number.region(),
        );
        let want = (
            e164.to_owned(),
            international.to_owned(),
            national.to_owned(),
            Some(kind),
            region(code),
        );
        if got != want {
            failures.push(format!("{input} in {code}: {got:?}, want {want:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn argentine_mobiles_gain_their_nine() {
    // The national form of a mobile has 15 after the area code; E.164 has 9
    // before it.
    let number = parse("011 15-2345-6789", Some("AR"));
    assert_eq!(number.as_str(), "+5491123456789");
    assert_eq!(number.international(), "+54 9 11 2345-6789");
    assert_eq!(number.number_type(), Some(NumberType::Mobile));
    assert_eq!(parse("+54 9 11 2345-6789", None), number);
}

#[test]
fn what_people_type_reads_as_the_same_number() {
    let want = "+447400123456";
    for (input, code) in [
        ("+44 7400 123456", None),
        ("+44 (0) 7400 123456", None),
        ("+44-7400-123-456", None),
        ("  +44.7400.123456  ", None),
        ("tel:+44-7400-123456", None),
        ("tel:7400123456;phone-context=+44", None),
        ("＋４４ ７４００ １２３４５６", None),
        ("Call me on +44 7400 123456.", None),
        ("00 44 7400 123456", Some("FR")),
        ("011 44 7400 123456", Some("US")),
        ("44 7400 123456", Some("GB")),
        ("07400123456", Some("GB")),
        ("+447400123456", Some("US")),
    ] {
        assert_eq!(parse(input, code).as_str(), want, "{input} in {code:?}");
    }
    assert_eq!(parse("+٩٧١ ٥٠ ١٢٣ ٤٥٦٧", None).as_str(), "+971501234567");
    assert_eq!(parse("+۹۸ ۹۱۲ ۳۴۵ ۶۷۸۹", None).as_str(), "+989123456789");
    assert_eq!(parse("+९१ ८१२३४ ५६७८९", None).as_str(), "+918123456789");
    assert_eq!(parse("٠١٠٠ ١٢٣ ٤٥٦٧", Some("EG")).as_str(), "+201001234567");
}

#[test]
fn letters_on_a_keypad() {
    let number = parse("1-800-FLOWERS", Some("US"));
    assert_eq!(number.as_str(), "+18003569377");
    assert_eq!(number.number_type(), Some(NumberType::TollFree));
}

#[test]
fn extensions_ride_along_outside_e164() {
    let number = parse("+44 7400 123456 ext. 12", None);
    assert_eq!(number.as_str(), "+447400123456");
    assert_eq!(number.extension(), Some("12"));
    assert_eq!(number.international(), "+44 7400 123456 x12");
    assert_eq!(number.national(), "07400 123456 x12");
    assert_eq!(
        number.format(PhoneFormat::Rfc3966),
        "tel:+44-7400-123456;ext=12"
    );
    assert_eq!(
        format!("{number} {number:?}"),
        "+447400123456 PhoneNumber(+447400123456 ext. 12)"
    );
    assert_ne!(number, number.without_extension());
    assert_eq!(
        parse("+1 650 253 0000 x 1234", None).international(),
        "+1 650-253-0000 ext. 1234"
    );
    assert_eq!(
        parse("650 253 0000;ext=1234", Some("US")).extension(),
        Some("1234")
    );
    // Found by fuzzing: ` x` reads at most 9 digits back, so a longer
    // extension is written with the explicit label.
    let long = parse("+44 808 066 8442 ext. 166066666055", None);
    assert_eq!(long.international(), "+44 808 066 8442 ext. 166066666055");
    assert_eq!(parse(&long.international(), None), long);
    assert_eq!(parse(&long.national(), Some("GB")), long);
}

#[test]
fn shared_calling_codes_find_their_region() {
    for (input, code) in [
        ("+1 650 253 0000", "US"),
        ("+1 416 979 5000", "CA"),
        ("+1 242 357 1234", "BS"),
        ("+7 701 123 4567", "KZ"),
        ("+7 495 123 4567", "RU"),
        ("+44 1534 456789", "JE"),
        ("+44 7400 123456", "GB"),
    ] {
        let number = parse(input, None);
        assert_eq!(number.region(), region(code), "{input}");
        assert!(number.is_valid_for(region(code).unwrap()), "{input}");
    }
    let canadian = parse("+1 416 979 5000", None);
    assert!(!canadian.is_valid_for(Region::parse("US").unwrap()));
}

#[test]
fn global_service_numbers_have_no_region() {
    let number = parse("+800 1234 5678", None);
    assert_eq!(number.region(), None);
    assert_eq!(number.number_type(), Some(NumberType::TollFree));
    assert_eq!(number.international(), "+800 1234 5678");
    assert!(number.is_internationally_diallable());
}

#[test]
fn errors_say_why() {
    let gb = region("GB");
    for (input, code, error) in [
        ("", gb, ParseError::NotANumber),
        ("hello", gb, ParseError::NotANumber),
        ("1", gb, ParseError::NotANumber),
        ("07400 123456", None, ParseError::NoRegion),
        ("+999 1234 5678", None, ParseError::InvalidCountryCode),
        ("+44 12", None, ParseError::Invalid),
        ("+44 7400 123456 789 012", None, ParseError::TooLong),
        ("+49 917 834 278 342 582", None, ParseError::TooLong),
        ("0044", gb, ParseError::TooShortAfterIdd),
        // RFC 3966 extensions take digits only, as in the reference library.
        ("+376 712 345;ext=206#", None, ParseError::NotANumber),
        (
            "+971 50 123 4567;ext=395004797#",
            None,
            ParseError::NotANumber,
        ),
    ] {
        assert_eq!(PhoneNumber::parse(input, code), Err(error), "{input}");
    }
    let long = "1".repeat(300);
    assert_eq!(PhoneNumber::parse(&long, gb), Err(ParseError::TooLong));
    assert_eq!(
        ParseError::NoRegion.to_string(),
        "no country calling code, and no region to read the number in"
    );
}

#[test]
fn possible_numbers_need_only_a_possible_length() {
    // A range no plan has assigned yet: the right length, unknown digits.
    let gb = region("GB");
    assert_eq!(
        PhoneNumber::parse("+44 1000 000000", None),
        Err(ParseError::Invalid)
    );
    let possible = PhoneNumber::parse_possible("+44 1000 000000", gb).unwrap();
    assert_eq!(possible.as_str(), "+441000000000");
    assert_eq!(possible.number_type(), None);
    assert_eq!(
        PhoneNumber::parse_possible("+44 12", None),
        Err(ParseError::TooShort)
    );
}

#[test]
fn regions() {
    let gb = Region::parse("gb").unwrap();
    assert_eq!(
        (gb.as_str(), gb.name(), gb.calling_code()),
        ("GB", "United Kingdom", 44)
    );
    assert_eq!(gb.national_prefix(), Some("0"));
    assert_eq!(Region::parse("IT").unwrap().national_prefix(), None);
    assert_eq!(format!("{gb} {gb:?}"), "GB Region(GB)");
    assert_eq!("us".parse::<Region>().map(Region::calling_code), Ok(1));
    for bad in ["", "G", "GBR", "ZZ", "UK", "001", "é"] {
        assert_eq!(Region::parse(bad), None, "{bad}");
    }
    let example = gb.placeholder_example().unwrap();
    assert_eq!(example.number_type(), Some(NumberType::Mobile));
    assert_eq!(
        navree_phone::regions_for(7)
            .map(Region::as_str)
            .collect::<Vec<_>>(),
        ["RU", "KZ"]
    );
    assert_eq!(navree_phone::calling_code("XK"), Some(383));
    let names: Vec<&str> = Region::all().map(Region::name).collect();
    assert!(
        names.contains(&"Åland Islands") && names.contains(&"Côte d’Ivoire"),
        "{names:?}"
    );
}

#[test]
fn the_type_is_small_and_copy() {
    fn copy<T: Copy + Send + Sync + 'static>() {}
    copy::<PhoneNumber>();
    assert!(std::mem::size_of::<PhoneNumber>() <= 40);
}

#[test]
fn stored_e164_reads_back_as_written_whatever_the_plans_say() {
    // What a column holds reads back exactly, even a number the plans compiled
    // in no longer accept (a range withdrawn, a length changed).
    let valid = PhoneNumber::from_e164("+447400123456").unwrap();
    assert_eq!(valid, parse("+44 7400 123456", None));
    let unassigned = PhoneNumber::from_e164("+441000000000").unwrap();
    assert_eq!(unassigned.as_str(), "+441000000000");
    assert_eq!(unassigned.number_type(), None);
    assert_eq!(
        PhoneNumber::from_e164("+390612345678")
            .unwrap()
            .national_number(),
        "0612345678"
    );
    for bad in [
        "",
        "+",
        "447400123456",
        "+44 7400 123456",
        "+999123456",
        "+0123456",
        "+4",
        "+44",
        "+1234567890123456",
        "+44740012345x",
        "\u{ff0b}447400123456",
    ] {
        assert!(PhoneNumber::from_e164(bad).is_err(), "{bad}");
    }
}
