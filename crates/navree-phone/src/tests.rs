//! Tests over the whole of the plans: every example number they hold reads
//! back to its territory and type, in every format.

use crate::meta::{self, Kind, Territory};
use crate::{NumberType, PhoneFormat, PhoneNumber, Region};

const KINDS: [Kind; 11] = [
    Kind::FixedLine,
    Kind::Mobile,
    Kind::TollFree,
    Kind::PremiumRate,
    Kind::SharedCost,
    Kind::PersonalNumber,
    Kind::Voip,
    Kind::Pager,
    Kind::Uan,
    Kind::Voicemail,
    Kind::NoInternationalDialling,
];

/// Whether `found` is what an example of `kind` should be.
fn type_fits(kind: Kind, found: NumberType) -> bool {
    match kind {
        Kind::FixedLine => matches!(found, NumberType::FixedLine | NumberType::FixedLineOrMobile),
        Kind::Mobile => matches!(found, NumberType::Mobile | NumberType::FixedLineOrMobile),
        Kind::TollFree => found == NumberType::TollFree,
        Kind::PremiumRate => found == NumberType::PremiumRate,
        Kind::SharedCost => found == NumberType::SharedCost,
        Kind::PersonalNumber => found == NumberType::PersonalNumber,
        Kind::Voip => found == NumberType::Voip,
        Kind::Pager => found == NumberType::Pager,
        Kind::Uan => found == NumberType::Uan,
        Kind::Voicemail => found == NumberType::Voicemail,
        Kind::NoInternationalDialling => true,
    }
}

/// Every example in the plans, with its territory and type.
fn examples() -> Vec<(&'static Territory, Option<Kind>, &'static str)> {
    let mut out = Vec::new();
    for territory in meta::all() {
        for kind in KINDS {
            if let Some(desc) = territory.desc(kind) {
                out.push((territory, Some(kind), desc.example.get()));
            }
        }
        if !territory.general.example.get().is_empty() {
            out.push((territory, None, territory.general.example.get()));
        }
    }
    out
}

fn parse_example(territory: &'static Territory, example: &str) -> Result<PhoneNumber, String> {
    let result = match Region::of(territory) {
        Some(region) => PhoneNumber::parse(example, Some(region)),
        None => PhoneNumber::parse(&format!("+{}{example}", territory.code), None),
    };
    result.map_err(|error| error.to_string())
}

#[test]
fn the_plans_are_all_there() {
    assert_eq!(crate::PLANS_RELEASE, "v9.0.42");
    assert_eq!(Region::all().count(), 245);
    assert_eq!(meta::all().len(), 254);
    let examples = examples();
    assert!(examples.len() > 1100, "{}", examples.len());
    // Every territory but the global services has a name and formats live
    // with each code's main territory.
    for territory in meta::geographic() {
        assert!(!territory.name.get().is_empty(), "{}", territory.id_str());
        let main = meta::main_for(territory.code).unwrap();
        // The main territory of a code holds the formats its code uses.
        assert!(
            !main.formats().is_empty()
                || meta::territories_for(territory.code).all(|t| t.formats().is_empty()),
            "{}",
            territory.id_str()
        );
    }
}

#[test]
fn every_example_number_is_valid_for_its_territory_and_type() {
    let mut failures = Vec::new();
    for (territory, kind, example) in examples() {
        let id = territory.id_str();
        if example.is_empty() {
            // No international dialling descriptions carry no example.
            if kind.is_some_and(|kind| kind != Kind::NoInternationalDialling) {
                failures.push(format!("{id} {kind:?}: no example"));
            }
            continue;
        }
        let number = match parse_example(territory, example) {
            Ok(number) => number,
            Err(error) => {
                failures.push(format!("{id} {kind:?} {example}: {error}"));
                continue;
            }
        };
        let found = number.number_type();
        if let Some(kind) = kind
            && !found.is_some_and(|found| type_fits(kind, found))
        {
            failures.push(format!("{id} {kind:?} {example}: type {found:?}"));
        }
        // A number its code's territories share (`+1 800`) belongs to the
        // first of them whose plan it fits, as in the reference library.
        let own =
            crate::parse::territory_of(number.country_code(), number.national_number().as_bytes());
        if !own.is_some_and(|own| own.code == territory.code) {
            failures.push(format!("{id} {kind:?} {example}: no territory"));
        }
        if let Some(region) = Region::of(territory)
            && !number.is_valid_for(region)
        {
            failures.push(format!("{id} {kind:?} {example}: not valid for its region"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn every_example_number_reads_back_from_every_format() {
    let mut failures = Vec::new();
    for (territory, kind, example) in examples() {
        let Ok(number) = parse_example(territory, example) else {
            continue;
        };
        let region = Region::of(territory);
        for how in [
            PhoneFormat::E164,
            PhoneFormat::International,
            PhoneFormat::Rfc3966,
            PhoneFormat::National,
        ] {
            let written = number.format(how);
            let read = match how {
                // The national form only means something in the region.
                PhoneFormat::National => match region {
                    Some(region) => PhoneNumber::parse(&written, Some(region)),
                    None => continue,
                },
                _ => PhoneNumber::parse(&written, None),
            };
            if read != Ok(number) {
                failures.push(format!(
                    "{} {kind:?} {example} as {how:?} `{written}` reads as {read:?}",
                    territory.id_str()
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn every_region_has_a_placeholder_example() {
    let missing: Vec<&str> = Region::all()
        .filter(|region| region.placeholder_example().is_none())
        .map(Region::as_str)
        .collect();
    // Territories whose plans list no lines of their own.
    assert!(missing.len() < 10, "{missing:?}");
}

/// What the plans cost in a binary that uses them: the tables, measured.
#[test]
fn the_plans_take_at_most_256_kib() {
    use std::mem::size_of_val;

    use crate::data::{CODE, CODE_TERRITORIES, CODES, DESCS, FORMATS, TERRITORIES, TEXT};
    let sizes = [
        ("patterns", size_of_val(&CODE)),
        ("text", TEXT.len()),
        ("descriptions", size_of_val(&DESCS)),
        ("formats", size_of_val(&FORMATS)),
        ("territories", size_of_val(&TERRITORIES)),
        (
            "calling codes",
            size_of_val(&CODES) + size_of_val(&CODE_TERRITORIES),
        ),
    ];
    let total: usize = sizes.iter().map(|(_, size)| size).sum();
    println!("{sizes:?}: {total} bytes");
    assert!(total <= 256 * 1024, "{total}");
}
