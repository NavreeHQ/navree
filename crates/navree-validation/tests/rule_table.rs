//! `#[derive(Validate)]` also states its rules as data: what a form reads to
//! set `required`, `maxlength` and `min` from the one place they're written.

use navree_validation::{FieldRules, Rule, RuleTable, Validate};

const MAX_TITLE: usize = 255;

fn dynamic_max() -> usize {
    9
}

#[derive(Validate)]
#[allow(dead_code)]
struct Deal {
    #[validate(required, length(min = 1, max = MAX_TITLE))]
    title: Option<String>,
    #[validate(length(max = dynamic_max()))]
    code: String,
    #[validate(range(min = 0, max = 100))]
    percent: i32,
    #[validate(range(min = -1.5))]
    tilt: f64,
    #[validate(email)]
    email: Option<String>,
    #[validate(pattern("^[a-z]+$"), starts_with("x"))]
    slug: String,
    #[validate(pattern("^[a-z_]+$", browser))]
    handle: String,
    #[validate(skip)]
    note: String,
    #[validate(rename = "zip_code", length(bytes, max = 8))]
    zip: String,
    #[validate(each(length(max = 30)))]
    tags: Vec<String>,
    #[validate(custom(always_ok))]
    other: String,
}

fn always_ok(_: &String) -> Result<(), String> {
    Ok(())
}

fn rules(name: &str) -> &'static [Rule] {
    Deal::FIELDS
        .iter()
        .find(|field| field.name == name)
        .map_or(&[], |field: &FieldRules| field.rules)
}

#[test]
fn literal_and_constant_bounds_are_data() {
    assert_eq!(
        rules("title"),
        [
            Rule::Required,
            Rule::Length {
                min: Some(1),
                max: Some(255),
                bytes: false
            }
        ]
    );
    assert_eq!(
        rules("percent"),
        [Rule::Range {
            min: Some("0"),
            max: Some("100")
        }]
    );
    assert_eq!(
        rules("tilt"),
        [Rule::Range {
            min: Some("-1.5"),
            max: None
        }]
    );
}

#[test]
fn a_bound_only_known_at_run_time_is_left_out() {
    assert_eq!(
        rules("code"),
        [Rule::Length {
            min: None,
            max: None,
            bytes: false
        }]
    );
}

#[test]
fn text_rules_are_named() {
    assert_eq!(rules("email"), [Rule::Format("email")]);
    assert_eq!(
        rules("slug"),
        [Rule::Pattern("^[a-z]+$"), Rule::StartsWith("x")]
    );
    assert_eq!(rules("other"), [Rule::Custom]);
}

#[test]
fn skipped_fields_have_no_rules_and_renames_apply() {
    assert!(Deal::FIELDS.iter().all(|field| field.name != "note"));
    assert_eq!(
        rules("zip_code"),
        [Rule::Length {
            min: None,
            max: Some(8),
            bytes: true
        }]
    );
    assert!(rules("zip").is_empty());
}

#[test]
fn item_rules_are_nested() {
    const ITEM: &[Rule] = &[Rule::Length {
        min: None,
        max: Some(30),
        bytes: false,
    }];
    assert_eq!(rules("tags"), [Rule::Each(ITEM)]);
}

#[derive(serde::Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct Renamed<'a, const N: usize> {
    #[validate(length(max = N))]
    first_name: String,
    #[serde(skip)]
    #[validate(skip)]
    borrowed: Option<&'a str>,
}

#[test]
fn generics_and_serde_names_work() {
    let fields = <Renamed<'static, 3> as RuleTable>::FIELDS;
    assert_eq!(fields[0].name, "firstName");
    assert_eq!(
        fields[0].rules,
        [Rule::Length {
            min: None,
            max: Some(3),
            bytes: false
        }]
    );
}

#[derive(Validate)]
#[allow(dead_code)]
enum Choice {
    A(#[validate(email)] String),
}

#[test]
fn enums_have_no_field_table() {
    assert!(Choice::FIELDS.is_empty());
}

/// A pattern reaches a page only when its rule says so, and the server checks it
/// either way.
#[test]
fn a_pattern_is_stated_for_the_browser_only_when_its_rule_asks() {
    assert_eq!(rules("slug")[0], Rule::Pattern("^[a-z]+$"));
    assert_eq!(rules("handle"), [Rule::BrowserPattern("^[a-z_]+$")]);
    let deal = |handle: &str| Deal {
        title: Some("t".into()),
        code: String::new(),
        percent: 1,
        tilt: 0.0,
        email: None,
        slug: "xa".into(),
        note: String::new(),
        zip: String::new(),
        tags: Vec::new(),
        other: String::new(),
        handle: handle.into(),
    };
    assert!(deal("ada_l").validate().is_ok());
    let errors = deal("Ada L")
        .validate()
        .expect_err("checked on the server too");
    assert!(format!("{errors:?}").contains("handle"), "{errors:?}");
}
