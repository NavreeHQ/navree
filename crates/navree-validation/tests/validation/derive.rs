//! `#[derive(Validate)]`: every rule, paths through nested values and
//! collections, serde's field names, enums and the violations' contents.

use std::collections::{BTreeMap, HashSet};

use navree_validation::{FieldPath, Validate, Violation, Violations, rules};
use serde::Deserialize;

/// The violations as `(path, key or text)` pairs, in order.
fn summary(violations: &Violations) -> Vec<(String, String)> {
    violations
        .iter()
        .map(|(path, violation)| {
            let what = violation.key().or(violation.as_text()).unwrap_or_default();
            (path.to_owned(), what.to_owned())
        })
        .collect()
}

fn failures<T: Validate>(value: &T) -> Vec<(String, String)> {
    match value.validate() {
        Ok(()) => Vec::new(),
        Err(violations) => summary(&violations),
    }
}

fn pairs(expected: &[(&str, &str)]) -> Vec<(String, String)> {
    expected
        .iter()
        .map(|(path, what)| ((*path).to_owned(), (*what).to_owned()))
        .collect()
}

#[derive(Default, Validate)]
struct Text {
    #[validate(email)]
    email: String,
    #[validate(url)]
    website: Option<String>,
    #[validate(ip)]
    ip: String,
    #[validate(ipv4)]
    v4: String,
    #[validate(ipv6)]
    v6: String,
    #[validate(ascii)]
    ascii: String,
    #[validate(alphanumeric)]
    code: String,
    #[validate(credit_card)]
    card: String,
    #[validate(contains("@"), starts_with("@"), ends_with("!"))]
    handle: String,
    #[validate(pattern(r"^[A-Z]{3}$"))]
    currency: String,
}

fn valid_text() -> Text {
    Text {
        email: "anu@example.com".into(),
        website: Some("https://example.com".into()),
        ip: "::1".into(),
        v4: "192.0.2.1".into(),
        v6: "2001:db8::1".into(),
        ascii: "plain".into(),
        code: "abc123".into(),
        card: "4111 1111 1111 1111".into(),
        handle: "@anu!".into(),
        currency: "EUR".into(),
    }
}

#[test]
fn text_rules_pass_and_fail() {
    assert_eq!(failures(&valid_text()), []);
    let text = Text {
        email: "anu@".into(),
        website: Some("example.com".into()),
        ip: "localhost".into(),
        v4: "::1".into(),
        v6: "192.0.2.1".into(),
        ascii: "café".into(),
        code: "a-b".into(),
        card: "4111".into(),
        handle: "anu".into(),
        currency: "eur".into(),
    };
    assert_eq!(
        failures(&text),
        pairs(&[
            ("email", "email"),
            ("website", "url"),
            ("ip", "ip"),
            ("v4", "ipv4"),
            ("v6", "ipv6"),
            ("ascii", "ascii"),
            ("code", "alpha_num"),
            ("card", "credit_card"),
            ("handle", "contains"),
            ("handle", "starts_with"),
            ("handle", "ends_with"),
            ("currency", "regex"),
        ])
    );
    let violations = text.validate().unwrap_err();
    assert_eq!(
        violations.get("currency")[0].message(),
        "value must match ^[A-Z]{3}$ regular expression"
    );
    // `None` passes every rule but `required`.
    let none = Text {
        website: None,
        ..valid_text()
    };
    assert_eq!(failures(&none), []);
}

const MAX_NAME: usize = 5;

#[derive(Validate)]
struct Measured {
    #[validate(required, length(min = 2, max = MAX_NAME))]
    name: Option<String>,
    #[validate(length(bytes, max = 3))]
    bytes: String,
    #[validate(length(min = 1, max = 2))]
    tags: Vec<String>,
    #[validate(range(min = 1, max = 10))]
    count: u32,
    #[validate(range(min = -1.5, max = 1.5))]
    ratio: Option<f64>,
    #[validate(range(min = 0))]
    balance: i64,
}

#[test]
fn length_range_and_required() {
    let ok = Measured {
        name: Some("Zoë".into()),
        bytes: "abc".into(),
        tags: vec!["a".into()],
        count: 10,
        ratio: None,
        balance: 0,
    };
    assert_eq!(failures(&ok), []);
    let bad = Measured {
        name: None,
        bytes: "Zoë".into(),
        tags: Vec::new(),
        count: 11,
        ratio: Some(-2.0),
        balance: -1,
    };
    assert_eq!(
        failures(&bad),
        pairs(&[
            ("name", "required"),
            ("bytes", "max.string"),
            ("tags", "min.array"),
            ("count", "max.numeric"),
            ("ratio", "min.numeric"),
            ("balance", "min.numeric"),
        ])
    );
    let long = Measured {
        name: Some("Anu Lakhotia".into()),
        ..ok
    };
    let violations = long.validate().unwrap_err();
    assert_eq!(
        violations.to_string(),
        "name must be shorter than or equal to 5 characters"
    );
    let params: Vec<_> = violations.get("name")[0].params().collect();
    assert_eq!(params, [("max", "5")]);
}

#[derive(Validate)]
struct Order {
    #[validate(nested)]
    customer: Customer,
    #[validate(length(min = 1), nested)]
    items: Vec<Item>,
    #[validate(nested)]
    gift: Option<Item>,
    #[validate(nested)]
    by_warehouse: BTreeMap<String, Item>,
}

#[derive(Validate)]
struct Customer {
    #[validate(email)]
    email: String,
    #[validate(nested)]
    address: Box<Address>,
}

#[derive(Deserialize, Validate)]
struct Address {
    #[validate(length(min = 1))]
    city: String,
}

#[derive(Clone, Validate)]
struct Item {
    #[validate(length(min = 1))]
    sku: String,
    #[validate(range(min = 1, max = 99))]
    quantity: u32,
}

#[test]
fn nested_values_report_at_their_full_path() {
    let item = |sku: &str, quantity| Item {
        sku: sku.into(),
        quantity,
    };
    let order = Order {
        customer: Customer {
            email: "nope".into(),
            address: Box::new(Address {
                city: String::new(),
            }),
        },
        items: vec![item("a", 1), item("", 0)],
        gift: Some(item("g", 100)),
        by_warehouse: BTreeMap::from([("LDN".to_owned(), item("", 1))]),
    };
    assert_eq!(
        failures(&order),
        pairs(&[
            ("customer.email", "email"),
            ("customer.address.city", "min.string"),
            ("items.1.sku", "min.string"),
            ("items.1.quantity", "min.numeric"),
            ("gift.quantity", "max.numeric"),
            ("by_warehouse.LDN.sku", "min.string"),
        ])
    );
    let empty = Order {
        items: Vec::new(),
        gift: None,
        by_warehouse: BTreeMap::new(),
        customer: Customer {
            email: "anu@example.com".into(),
            address: Box::new(Address { city: "x".into() }),
        },
    };
    assert_eq!(failures(&empty), pairs(&[("items", "min.array")]));
}

fn not_blank<T: rules::RuleInput + ?Sized>(value: &T) -> Result<(), &'static str> {
    match value.rule_str() {
        Some(text) if text.trim().is_empty() => Err(":attribute must not be blank"),
        _ => Ok(()),
    }
}

#[derive(Validate)]
struct Tagged {
    #[validate(length(max = 3), each(length(min = 1, max = 4), custom(not_blank)))]
    tags: Vec<String>,
    #[validate(each(range(max = 100)))]
    scores: BTreeMap<String, u32>,
    #[validate(each(email))]
    emails: Option<HashSet<String>>,
    #[validate(each(each(range(min = 0))))]
    grid: Vec<Vec<i32>>,
    #[validate(each(nested))]
    items: [Item; 1],
}

#[test]
fn each_checks_every_item() {
    let tagged = Tagged {
        tags: vec!["vip".into(), " ".into(), "toolong".into()],
        scores: BTreeMap::from([("anu".to_owned(), 101), ("bal".to_owned(), 5)]),
        emails: Some(HashSet::from(["x".to_owned()])),
        grid: vec![vec![0, 1], vec![-1]],
        items: [Item {
            sku: String::new(),
            quantity: 1,
        }],
    };
    assert_eq!(
        failures(&tagged),
        pairs(&[
            ("tags.1", ":attribute must not be blank"),
            ("tags.2", "max.string"),
            ("scores.anu", "max.numeric"),
            ("emails.0", "email"),
            ("grid.1.0", "min.numeric"),
            ("items.0.sku", "min.string"),
        ])
    );
}

fn even(value: &u32) -> Result<(), Violation> {
    if value.is_multiple_of(2) {
        Ok(())
    } else {
        Err(Violation::rule("even").with("value", value))
    }
}

fn shorter_than(limit: usize) -> impl FnOnce(&String) -> Result<(), String> {
    move |value| {
        if value.len() < limit {
            Ok(())
        } else {
            Err(format!(":attribute must be shorter than {limit}"))
        }
    }
}

#[derive(Validate)]
struct Account {
    #[validate(custom(even), custom(|value: &u32| if *value > 10 { Err("big") } else { Ok(()) }))]
    seats: u32,
    #[validate(length(min = 8), custom(rules::confirmed(&self.password_confirmation)))]
    password: String,
    #[validate(skip)]
    password_confirmation: String,
    #[validate(same_as(email))]
    email_again: String,
    #[validate(skip)]
    email: String,
    #[validate(custom(shorter_than(self.max_bio)))]
    bio: String,
    #[validate(skip)]
    max_bio: usize,
}

#[test]
fn custom_rules_see_the_value_and_self() {
    let account = Account {
        seats: 11,
        password: "secret12".into(),
        password_confirmation: "secret13".into(),
        email_again: "b@example.com".into(),
        email: "a@example.com".into(),
        bio: "hello".into(),
        max_bio: 5,
    };
    assert_eq!(
        failures(&account),
        pairs(&[
            ("seats", "even"),
            ("seats", "big"),
            ("password", "confirmed"),
            ("email_again", "same"),
            ("bio", ":attribute must be shorter than 5"),
        ])
    );
    let violations = account.validate().unwrap_err();
    assert_eq!(
        violations.get("email_again")[0]
            .params()
            .collect::<Vec<_>>(),
        [("other", "email")]
    );
}

#[derive(Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
struct Renamed {
    #[validate(length(min = 1))]
    first_name: String,
    #[serde(rename = "surname")]
    #[validate(length(min = 1))]
    last_name: String,
    #[validate(rename = "mail", email)]
    email_address: String,
    #[serde(flatten)]
    #[validate(nested)]
    address: Address,
    #[validate(length(min = 1))]
    r#type: String,
}

#[test]
fn paths_use_the_names_serde_reads() {
    let renamed = Renamed {
        first_name: String::new(),
        last_name: String::new(),
        email_address: "x".into(),
        address: Address {
            city: String::new(),
        },
        r#type: String::new(),
    };
    let paths: Vec<String> = failures(&renamed)
        .into_iter()
        .map(|(path, _)| path)
        .collect();
    assert_eq!(paths, ["firstName", "surname", "mail", "city", "type"]);
}

#[derive(Validate)]
#[validate(skip_unmarked)]
struct Partial {
    #[validate(length(min = 1))]
    name: String,
    #[allow(dead_code)]
    note: String,
}

#[derive(Validate)]
struct Code(#[validate(length(min = 3))] String);

#[derive(Validate)]
struct Pair(#[validate(range(min = 1))] u8, #[validate(email)] String);

#[derive(Validate)]
enum Payment {
    Card {
        #[validate(credit_card)]
        number: String,
    },
    Transfer(#[validate(length(min = 1))] String),
    Pair(#[validate(range(min = 1))] u8, #[validate(skip)] String),
    Cash,
}

#[derive(Validate)]
struct Generic<T: Validate> {
    #[validate(nested)]
    inner: T,
}

#[test]
fn tuples_enums_generics_and_skip_unmarked() {
    let partial = Partial {
        name: String::new(),
        note: String::new(),
    };
    assert_eq!(failures(&partial), pairs(&[("name", "min.string")]));
    // A newtype is its one field; other tuple fields are numbered.
    assert_eq!(failures(&Code("ab".into())), pairs(&[("", "min.string")]));
    assert_eq!(
        failures(&Pair(0, "x".into())),
        pairs(&[("0", "min.numeric"), ("1", "email")])
    );
    assert_eq!(
        failures(&Payment::Card { number: "1".into() }),
        pairs(&[("number", "credit_card")])
    );
    assert_eq!(
        failures(&Payment::Transfer(String::new())),
        pairs(&[("", "min.string")])
    );
    assert_eq!(
        failures(&Payment::Pair(0, String::new())),
        pairs(&[("0", "min.numeric")])
    );
    assert_eq!(failures(&Payment::Cash), []);
    let generic = Generic {
        inner: Code("a".into()),
    };
    assert_eq!(failures(&generic), pairs(&[("inner", "min.string")]));
}

#[test]
fn validate_at_reports_under_a_parent_path() {
    let mut out = Violations::new();
    let root = FieldPath::root();
    let parent = root.field("payload");
    let parent = parent.index(2);
    Code("a".into()).validate_at(&parent, &mut out);
    assert_eq!(summary(&out), pairs(&[("payload.2", "min.string")]));
}

#[test]
fn folded_values_and_safe_text_types_take_the_rules() {
    #[derive(Deserialize, Validate)]
    struct Verify {
        #[validate(length(min = 6, max = 6), pattern(r"^[0-9]+$"))]
        code: navree_validation::Folded<String>,
        #[validate(range(min = 1, max = 99))]
        quantity: Option<navree_validation::Folded<u32>>,
    }
    let input: Verify = serde_json::from_str(r#"{"code": "٦٥٤٣٢١", "quantity": "١٠٠"}"#).unwrap();
    assert_eq!(failures(&input), pairs(&[("quantity", "max.numeric")]));
}
