//! CASL JSON: the rule format `createMongoAbility` in `@casl/ability` accepts.

use keel_access::engine::{
    ALL, Ability, Conditions, DELETE, MANAGE, READ, Rule, RuleError, Rules, UPDATE, field,
};
use serde_json::{Value, json};

fn sample() -> Ability {
    let mut rules = Rules::new();
    rules.can(READ, "Post").when(field("published").eq(true));
    rules
        .can(UPDATE, "Post")
        .fields(["title", "body"])
        .when(field("author_id").eq(7));
    rules.push(Rule {
        action: vec![READ.to_owned(), UPDATE.to_owned()],
        subject: vec!["Comment".to_owned(), "Reply".to_owned()],
        ..Rule::allow(READ, "Comment")
    });
    rules.can(READ, "Invoice").when(
        field("total")
            .gte(10)
            .and(field("total").lt(99.5))
            .and(field("status").is_in(["open", "paid"]))
            .and(field("team_id").ne(None::<i64>))
            .and(field("deleted_at").eq(None::<&str>))
            .and(field("region").not_in(["eu"])),
    );
    rules
        .cannot(DELETE, "Post")
        .when(field("locked").eq(true))
        .because("locked posts are kept");
    rules.can(MANAGE, ALL).when(Conditions::new());
    rules.build()
}

#[test]
fn casl_json_roundtrip_matches_casl_format() {
    let ability = sample();
    let json = ability.to_casl_json();

    insta::with_settings!({ sort_maps => true }, {
        insta::assert_json_snapshot!(json, @r#"
        [
          {
            "action": "read",
            "conditions": {
              "published": true
            },
            "subject": "Post"
          },
          {
            "action": "update",
            "conditions": {
              "author_id": 7
            },
            "fields": [
              "title",
              "body"
            ],
            "subject": "Post"
          },
          {
            "action": [
              "read",
              "update"
            ],
            "subject": [
              "Comment",
              "Reply"
            ]
          },
          {
            "action": "read",
            "conditions": {
              "deleted_at": null,
              "region": {
                "$nin": [
                  "eu"
                ]
              },
              "status": {
                "$in": [
                  "open",
                  "paid"
                ]
              },
              "team_id": {
                "$ne": null
              },
              "total": {
                "$gte": 10,
                "$lt": 99.5
              }
            },
            "subject": "Invoice"
          },
          {
            "action": "delete",
            "conditions": {
              "locked": true
            },
            "inverted": true,
            "reason": "locked posts are kept",
            "subject": "Post"
          },
          {
            "action": "manage",
            "conditions": {},
            "subject": "all"
          }
        ]
        "#);
    });

    let parsed = Ability::from_casl_json(&json).expect("own output parses");
    assert_eq!(parsed.rules(), ability.rules());
    assert_eq!(parsed.to_casl_json(), json);

    // `Rule` itself (de)serializes in the same format.
    let first: Rule = serde_json::from_value(json[0].clone()).expect("rule parses");
    assert_eq!(&first, &ability.rules()[0]);
    assert_eq!(serde_json::to_value(&ability.rules()[4]).unwrap(), json[4]);
    // `Ability` serializes as the rule array.
    assert_eq!(serde_json::to_value(&ability).unwrap(), json);
}

#[test]
fn from_casl_json_accepts_casl_shapes() {
    let ability = Ability::from_casl_json(&json!([
        { "action": ["read"], "subject": ["Post"], "conditions": { "rank": { "$eq": 3 } } },
        { "action": "update", "subject": "Post", "fields": "title", "inverted": false },
        { "action": "delete", "subject": "Post", "inverted": true, "conditions": null, "reason": null },
    ]))
    .expect("CASL rules parse");

    let rules = ability.rules();
    assert_eq!(rules[0].conditions, Some(field("rank").eq(3)));
    assert_eq!(rules[1].fields, Some(vec!["title".to_owned()]));
    assert!(!rules[1].inverted);
    assert!(rules[2].inverted);
    assert_eq!(rules[2].conditions, None);
    assert_eq!(rules[2].reason, None);

    assert!(ability.can(READ, "Post", Some(&json!({ "rank": 3 }))));
    assert!(!ability.can(READ, "Post", Some(&json!({ "rank": 4 }))));
    assert!(ability.can_field(UPDATE, "Post", None, "title"));
    assert!(!ability.can(DELETE, "Post", None));
}

fn parse_error(json: Value) -> RuleError {
    Ability::from_casl_json(&json).expect_err("CASL rules must be rejected")
}

#[test]
fn rejects_unsupported_operator() {
    for operator in [
        "$exists",
        "$regex",
        "$elemMatch",
        "$all",
        "$size",
        "$options",
        "$foo",
    ] {
        let error = parse_error(json!([
            { "action": "read", "subject": "Post", "conditions": { "title": { operator: true } } }
        ]));
        assert_eq!(error, RuleError::UnsupportedOperator(operator.to_owned()));
    }
    for operator in ["$and", "$or", "$nor", "$not", "$where"] {
        let error = parse_error(json!([
            { "action": "read", "subject": "Post", "conditions": { operator: [{ "a": 1 }] } }
        ]));
        assert_eq!(error, RuleError::UnsupportedOperator(operator.to_owned()));
    }
    // Nested fields, in conditions and in field lists.
    assert_eq!(
        parse_error(json!([
            { "action": "read", "subject": "Post", "conditions": { "author.id": 7 } }
        ])),
        RuleError::NestedField("author.id".to_owned())
    );
    assert_eq!(
        parse_error(json!([{ "action": "read", "subject": "Post", "fields": ["meta.*"] }])),
        RuleError::NestedField("meta.*".to_owned())
    );
}

#[test]
fn rejects_malformed_casl_json() {
    let invalid_rule = |json: Value| {
        let error = parse_error(json.clone());
        assert!(
            matches!(error, RuleError::InvalidRule(_)),
            "{json} gave {error:?}"
        );
    };
    invalid_rule(json!({ "action": "read", "subject": "Post" }));
    invalid_rule(json!([{ "subject": "Post" }]));
    invalid_rule(json!([{ "action": "read" }]));
    invalid_rule(json!([{ "action": [], "subject": "Post" }]));
    invalid_rule(json!([{ "action": "read", "subject": "Post", "conditions": [1] }]));
    invalid_rule(json!([{ "action": "read", "subject": "Post", "inverted": "yes" }]));

    // Operands are checked as in the builder.
    assert!(matches!(
        parse_error(json!([
            { "action": "read", "subject": "Post", "conditions": { "tags": { "$in": "a" } } }
        ])),
        RuleError::InvalidValue {
            operator: "$in",
            ..
        }
    ));
    assert!(matches!(
        parse_error(json!([
            { "action": "read", "subject": "Post", "conditions": { "meta": { "kind": "a" } } }
        ])),
        RuleError::InvalidValue {
            operator: "$eq",
            ..
        }
    ));
    assert!(matches!(
        parse_error(json!([
            { "action": "read", "subject": "Post", "conditions": { "meta": {} } }
        ])),
        RuleError::InvalidValue {
            operator: "$eq",
            ..
        }
    ));
    assert!(matches!(
        parse_error(json!([
            { "action": "read", "subject": "Post", "conditions": { "id": 9_007_199_254_740_993_u64 } }
        ])),
        RuleError::InvalidValue {
            operator: "$eq",
            ..
        }
    ));
}
