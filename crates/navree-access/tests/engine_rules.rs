//! Rule semantics of the ability engine: order, `manage`/`all`, inversion,
//! conditions, type-level checks, fields, permissions and token restriction.

use navree_access::engine::{
    ALL, Ability, CREATE, DELETE, MANAGE, READ, Rule, RuleError, Rules, UPDATE, field,
};
use serde_json::json;

const POST: &str = "Post";
const COMMENT: &str = "Comment";

fn ability(define: impl FnOnce(&mut Rules)) -> Ability {
    let mut rules = Rules::new();
    define(&mut rules);
    rules.build()
}

#[test]
fn last_matching_rule_wins() {
    let post = json!({ "id": 1, "locked": true });

    let deny_last = ability(|r| {
        r.can(DELETE, POST);
        r.cannot(DELETE, POST).when(field("locked").eq(true));
    });
    assert!(!deny_last.can(DELETE, POST, Some(&post)));

    let allow_last = ability(|r| {
        r.cannot(DELETE, POST).when(field("locked").eq(true));
        r.can(DELETE, POST);
    });
    assert!(allow_last.can(DELETE, POST, Some(&post)));

    // A later rule that does not match the record does not decide.
    let unlocked = json!({ "id": 2, "locked": false });
    assert!(deny_last.can(DELETE, POST, Some(&unlocked)));
}

#[test]
fn manage_matches_any_action_all_matches_any_subject() {
    let manage_posts = ability(|r| {
        r.can(MANAGE, POST);
    });
    for action in [READ, CREATE, UPDATE, DELETE, "publish"] {
        assert!(
            manage_posts.can(action, POST, None),
            "manage covers {action}"
        );
    }
    assert!(!manage_posts.can(READ, COMMENT, None));

    let read_all = ability(|r| {
        r.can(READ, ALL);
    });
    assert!(read_all.can(READ, POST, None));
    assert!(read_all.can(READ, COMMENT, None));
    assert!(!read_all.can(UPDATE, POST, None));

    let admin = ability(|r| {
        r.can(MANAGE, ALL);
    });
    assert!(admin.can("export", "Invoice", None));

    // A rule for a concrete action never answers a `manage` question.
    let read_only = ability(|r| {
        r.can(READ, POST);
    });
    assert!(!read_only.can(MANAGE, POST, None));
}

#[test]
fn inverted_rule_denies() {
    let no_rules = ability(|_| {});
    assert!(!no_rules.can(READ, POST, None), "deny by default");

    let denied = ability(|r| {
        r.can(MANAGE, ALL);
        r.cannot(DELETE, POST);
    });
    assert!(!denied.can(DELETE, POST, None));
    assert!(!denied.can(DELETE, POST, Some(&json!({ "id": 1 }))));
    assert!(denied.can(UPDATE, POST, None));
    assert!(denied.can(DELETE, COMMENT, None));
    assert!(denied.cannot(DELETE, POST, None));
}

#[test]
fn rules_accept_multiple_actions_and_subjects() {
    let rules = ability(|r| {
        r.can(READ, POST);
    });
    let rule = &rules.rules()[0];
    assert_eq!(rule.action, vec![READ.to_owned()]);
    assert_eq!(rule.subject, vec![POST.to_owned()]);
    assert!(!rule.inverted);

    let mut raw = rule.clone();
    raw.action.push(UPDATE.to_owned());
    raw.subject.push(COMMENT.to_owned());
    let multi = Ability::from_rules(vec![raw]).expect("valid rule");
    assert!(multi.can(UPDATE, COMMENT, None));
    assert!(multi.can(READ, POST, None));
    assert!(!multi.can(DELETE, POST, None));
}

fn allows(conditions: navree_access::engine::Conditions, record: serde_json::Value) -> bool {
    let ability = ability(|r| {
        r.can(READ, POST).when(conditions);
    });
    ability.can(READ, POST, Some(&record))
}

#[test]
fn conditions_eq_ne_lt_in_nin() {
    // $eq
    assert!(allows(field("author_id").eq(7), json!({ "author_id": 7 })));
    assert!(allows(
        field("author_id").eq(7),
        json!({ "author_id": 7.0 })
    ));
    assert!(!allows(field("author_id").eq(7), json!({ "author_id": 8 })));
    assert!(
        !allows(field("author_id").eq(7), json!({ "author_id": "7" })),
        "no type coercion"
    );
    assert!(allows(
        field("status").eq("draft"),
        json!({ "status": "draft" })
    ));
    assert!(!allows(
        field("status").eq("draft"),
        json!({ "status": "Draft" })
    ));
    assert!(allows(
        field("published").eq(false),
        json!({ "published": false })
    ));

    // $ne
    assert!(allows(field("author_id").ne(7), json!({ "author_id": 8 })));
    assert!(!allows(field("author_id").ne(7), json!({ "author_id": 7 })));

    // $lt / $lte / $gt / $gte on numbers and strings
    assert!(allows(field("rank").lt(3), json!({ "rank": 2 })));
    assert!(!allows(field("rank").lt(3), json!({ "rank": 3 })));
    assert!(allows(field("rank").lte(3), json!({ "rank": 3 })));
    assert!(allows(field("rank").gt(2.5), json!({ "rank": 3 })));
    assert!(!allows(field("rank").gt(3), json!({ "rank": 3 })));
    assert!(allows(field("rank").gte(3), json!({ "rank": 3 })));
    assert!(allows(field("title").lt("b"), json!({ "title": "abc" })));
    assert!(
        !allows(field("title").lt("B"), json!({ "title": "abc" })),
        "code point order"
    );
    let at = "2026-10-25T10:00:00.000000Z";
    assert!(allows(
        field("created_at").gte(at),
        json!({ "created_at": "2026-10-25T10:00:00.000001Z" })
    ));
    assert!(
        !allows(field("rank").lt(3), json!({ "rank": "2" })),
        "no cross-type order"
    );

    // $in / $nin
    assert!(allows(
        field("status").is_in(["draft", "review"]),
        json!({ "status": "review" })
    ));
    assert!(!allows(
        field("status").is_in(["draft", "review"]),
        json!({ "status": "live" })
    ));
    assert!(!allows(
        field("status").is_in(Vec::<&str>::new()),
        json!({ "status": "live" })
    ));
    assert!(allows(
        field("status").not_in(["draft", "review"]),
        json!({ "status": "live" })
    ));
    assert!(!allows(
        field("status").not_in(["draft", "review"]),
        json!({ "status": "draft" })
    ));
    assert!(allows(
        field("status").not_in(Vec::<&str>::new()),
        json!({ "status": "draft" })
    ));

    // Several conditions are a conjunction, also on one field.
    let range = field("rank").gt(1).and(field("rank").lt(5));
    assert!(allows(range.clone(), json!({ "rank": 3 })));
    assert!(!allows(range, json!({ "rank": 5 })));
    let both = field("author_id").eq(7).and(field("published").eq(true));
    assert!(allows(
        both.clone(),
        json!({ "author_id": 7, "published": true })
    ));
    assert!(!allows(both, json!({ "author_id": 7, "published": false })));
}

#[test]
fn eq_null_and_ne_null_semantics() {
    let is_null = || field("deleted_at").eq(None::<&str>);
    let not_null = || field("deleted_at").ne(None::<&str>);
    let deleted = json!({ "deleted_at": "2026-10-25T10:00:00.000000Z" });
    let live = json!({ "deleted_at": null });

    assert!(allows(is_null(), live.clone()));
    assert!(!allows(is_null(), deleted.clone()));
    assert!(allows(not_null(), deleted.clone()));
    assert!(!allows(not_null(), live.clone()));

    // $ne of a value matches null fields (SQL IS DISTINCT FROM, JS !==).
    assert!(allows(field("team_id").ne(3), json!({ "team_id": null })));
    // $eq of a value never matches null.
    assert!(!allows(field("team_id").eq(3), json!({ "team_id": null })));
    // Range operators never match null.
    for c in [
        field("rank").lt(3),
        field("rank").lte(3),
        field("rank").gt(-3),
        field("rank").gte(-3),
    ] {
        assert!(!allows(c, json!({ "rank": null })));
    }
    // $in matches null only through a null member; $nin is its negation.
    assert!(allows(
        field("team_id").is_in([Some(1), None]),
        json!({ "team_id": null })
    ));
    assert!(!allows(
        field("team_id").is_in([1, 2]),
        json!({ "team_id": null })
    ));
    assert!(allows(
        field("team_id").not_in([1, 2]),
        json!({ "team_id": null })
    ));
    assert!(!allows(
        field("team_id").not_in([Some(1), None]),
        json!({ "team_id": null })
    ));
    assert!(allows(
        field("team_id").not_in([Some(1), None]),
        json!({ "team_id": 2 })
    ));

    // A field the record lacks counts as null.
    assert!(allows(is_null(), json!({})));
    assert!(!allows(field("team_id").eq(3), json!({})));
    assert!(allows(field("team_id").ne(3), json!({})));
}

#[test]
fn type_level_check_with_conditional_rule_is_true() {
    // "Can this user read some posts?" — yes, the published ones.
    let some = ability(|r| {
        r.can(READ, POST).when(field("published").eq(true));
    });
    assert!(some.can(READ, POST, None));
    assert!(!some.can(READ, POST, Some(&json!({ "published": false }))));

    // A conditional `cannot` does not deny at type level (CASL)...
    let conditional_deny = ability(|r| {
        r.can(DELETE, POST);
        r.cannot(DELETE, POST).when(field("locked").eq(true));
    });
    assert!(conditional_deny.can(DELETE, POST, None));

    // ...but a later unconditional `cannot` does.
    let unconditional_deny = ability(|r| {
        r.can(READ, POST).when(field("published").eq(true));
        r.cannot(READ, POST);
    });
    assert!(!unconditional_deny.can(READ, POST, None));

    // A conditional `cannot` alone grants nothing.
    let only_deny = ability(|r| {
        r.cannot(READ, POST).when(field("locked").eq(true));
    });
    assert!(!only_deny.can(READ, POST, None));
}

#[test]
fn non_object_record_has_no_fields() {
    let ability = ability(|r| {
        r.can(READ, POST).when(field("deleted_at").eq(None::<&str>));
        r.cannot(READ, POST).when(field("locked").eq(true));
    });
    // Treated as a record whose fields are all null, not as a type-level check.
    assert!(ability.can(READ, POST, Some(&serde_json::Value::Null)));
    let only_published = self::ability(|r| {
        r.can(READ, POST).when(field("published").eq(true));
    });
    assert!(!only_published.can(READ, POST, Some(&serde_json::Value::Null)));
}

#[test]
fn fields_restrict_permitted_fields() {
    let all_fields = ["id", "title", "body", "author_id", "published"];
    let own = json!({ "id": 1, "author_id": 7 });
    let other = json!({ "id": 2, "author_id": 8 });

    let ability = ability(|r| {
        r.can(READ, POST);
        r.can(UPDATE, POST)
            .fields(["title", "body", "author_id"])
            .when(field("author_id").eq(7));
        r.cannot(UPDATE, POST).fields(["author_id"]);
    });

    // Whole-record checks ignore field-only `cannot` rules (CASL).
    assert!(ability.can(UPDATE, POST, Some(&own)));
    assert!(!ability.can(UPDATE, POST, Some(&other)));

    assert!(ability.can_field(UPDATE, POST, Some(&own), "title"));
    assert!(!ability.can_field(UPDATE, POST, Some(&own), "author_id"));
    assert!(!ability.can_field(UPDATE, POST, Some(&own), "published"));
    assert!(!ability.can_field(UPDATE, POST, Some(&other), "title"));
    assert!(ability.can_field(READ, POST, Some(&other), "published"));

    assert_eq!(
        ability.permitted_fields(UPDATE, POST, Some(&own), &all_fields),
        vec!["title", "body"]
    );
    assert!(
        ability
            .permitted_fields(UPDATE, POST, Some(&other), &all_fields)
            .is_empty()
    );
    assert_eq!(
        ability.permitted_fields(READ, POST, Some(&other), &all_fields),
        all_fields
    );
    // Type level: the conditional grant counts.
    assert_eq!(
        ability.permitted_fields(UPDATE, POST, None, &all_fields),
        vec!["title", "body"]
    );

    // A later unrestricted `cannot` takes every field back.
    let locked = self::ability(|r| {
        r.can(UPDATE, POST).fields(["title"]);
        r.cannot(UPDATE, POST).when(field("locked").eq(true));
    });
    let locked_post = json!({ "locked": true });
    assert!(!locked.can_field(UPDATE, POST, Some(&locked_post), "title"));
    assert!(locked.can_field(UPDATE, POST, Some(&json!({ "locked": false })), "title"));
}

#[test]
fn relevant_rule_explains_the_decision() {
    let ability = ability(|r| {
        r.can(DELETE, POST).because("authors delete their posts");
        r.cannot(DELETE, POST)
            .when(field("locked").eq(true))
            .because("locked posts are kept");
    });
    let locked = json!({ "locked": true });
    let rule = ability
        .relevant_rule(DELETE, POST, Some(&locked), None)
        .expect("a rule decides");
    assert!(rule.inverted);
    assert_eq!(rule.reason.as_deref(), Some("locked posts are kept"));

    let open = json!({ "locked": false });
    let rule = ability
        .relevant_rule(DELETE, POST, Some(&open), None)
        .expect("a rule decides");
    assert_eq!(rule.reason.as_deref(), Some("authors delete their posts"));

    assert!(ability.relevant_rule(READ, POST, None, None).is_none());
}

fn prefixes() -> std::collections::BTreeMap<String, String> {
    [("posts", POST), ("comments", COMMENT)]
        .into_iter()
        .map(|(prefix, subject)| (prefix.to_owned(), subject.to_owned()))
        .collect()
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn has_unconditional_requires_unconditional_grant() {
    let granted = ability(|r| {
        r.can("publish", POST);
    });
    assert!(granted.has_unconditional("publish", POST));
    assert!(!granted.has_unconditional("publish", COMMENT));
    assert!(!granted.has_unconditional(READ, POST));

    let conditional = ability(|r| {
        r.can("publish", POST).when(field("author_id").eq(7));
    });
    assert!(conditional.can("publish", POST, None));
    assert!(!conditional.has_unconditional("publish", POST));

    // A later conditional grant does not hide an earlier unconditional one.
    let both = ability(|r| {
        r.can("publish", POST);
        r.can("publish", POST).when(field("author_id").eq(7));
    });
    assert!(both.has_unconditional("publish", POST));

    // A later `cannot` that takes some records back breaks it.
    let taken_back = ability(|r| {
        r.can(MANAGE, ALL);
        r.cannot("publish", POST).when(field("locked").eq(true));
    });
    assert!(!taken_back.has_unconditional("publish", POST));
    assert!(taken_back.has_unconditional(READ, POST));

    // A field-only `cannot` limits fields, not records.
    let field_limited = ability(|r| {
        r.can(UPDATE, POST);
        r.cannot(UPDATE, POST).fields(["author_id"]);
    });
    assert!(field_limited.has_unconditional(UPDATE, POST));

    let denied = ability(|r| {
        r.can(MANAGE, ALL);
        r.cannot(DELETE, ALL);
    });
    assert!(!denied.has_unconditional(DELETE, POST));

    // Empty conditions match every record.
    let empty = ability(|r| {
        r.can("publish", POST)
            .when(navree_access::engine::Conditions::new());
    });
    assert!(empty.has_unconditional("publish", POST));
}

#[test]
fn from_permissions_maps_prefix_to_subject() {
    let mut rules = Rules::new();
    rules.from_permissions(
        &strings(&[
            "posts.publish",
            "comments.delete",
            "posts.*",
            "invoices.export",
            "malformed",
            ".read",
            "posts.",
        ]),
        &prefixes(),
    );
    let ability = rules.build();

    assert!(ability.has_unconditional("publish", POST));
    assert!(ability.has_unconditional(DELETE, COMMENT));
    assert!(!ability.can(READ, COMMENT, None));
    // `posts.*` is `manage Post`.
    assert!(ability.has_unconditional("archive", POST));
    // Unknown prefixes and malformed strings grant nothing.
    assert!(!ability.can("export", "Invoice", None));
    assert_eq!(ability.rules().len(), 3);
}

#[test]
fn restrict_to_token_abilities() {
    let user = ability(|r| {
        r.can(READ, POST);
        r.can(UPDATE, POST).when(field("author_id").eq(7));
        r.can(MANAGE, COMMENT);
        r.cannot(DELETE, COMMENT).when(field("locked").eq(true));
    });
    let own = json!({ "author_id": 7 });
    let other = json!({ "author_id": 8 });

    // "*" keeps everything.
    let all = user.restrict_to(&strings(&["*"]), &prefixes());
    assert_eq!(all.rules(), user.rules());

    let token = user.restrict_to(&strings(&["posts.update", "comments.delete"]), &prefixes());
    // Only what both the user and the token allow.
    assert!(!token.can(READ, POST, None), "token lacks posts.read");
    assert!(token.can(UPDATE, POST, Some(&own)));
    assert!(
        !token.can(UPDATE, POST, Some(&other)),
        "user rule conditions still apply"
    );
    assert!(token.can(DELETE, COMMENT, Some(&json!({ "locked": false }))));
    assert!(
        !token.can(DELETE, COMMENT, Some(&json!({ "locked": true }))),
        "order kept"
    );
    assert!(
        !token.can(UPDATE, COMMENT, None),
        "manage narrowed to the token's action"
    );

    // A token cannot grant what the user lacks.
    let wider = user.restrict_to(&strings(&["posts.delete", "posts.*"]), &prefixes());
    assert!(!wider.can(DELETE, POST, None));
    assert!(wider.can(READ, POST, None));

    // A `manage all` user is narrowed to the token's pairs.
    let admin = ability(|r| {
        r.can(MANAGE, ALL);
    });
    let narrowed = admin.restrict_to(
        &strings(&["posts.read", "unknown.read", "bad"]),
        &prefixes(),
    );
    assert!(narrowed.can(READ, POST, None));
    assert!(!narrowed.can(UPDATE, POST, None));
    assert!(!narrowed.can(READ, COMMENT, None));
    assert!(!narrowed.can(READ, "Invoice", None));

    // No token abilities: nothing.
    let none = admin.restrict_to(&[], &prefixes());
    assert!(!none.can(READ, POST, None));
}

fn build_error(define: impl FnOnce(&mut Rules)) -> RuleError {
    let mut rules = Rules::new();
    define(&mut rules);
    rules.try_build().expect_err("rules must be rejected")
}

#[test]
fn rejects_nested_and_invalid_fields_when_rules_are_defined() {
    assert_eq!(
        build_error(|r| {
            r.can(READ, POST).when(field("author.id").eq(7));
        }),
        RuleError::NestedField("author.id".to_owned())
    );
    assert_eq!(
        build_error(|r| {
            r.can(UPDATE, POST).fields(["meta.tags"]);
        }),
        RuleError::NestedField("meta.tags".to_owned())
    );
    assert_eq!(
        build_error(|r| {
            r.can(UPDATE, POST).fields(["title*"]);
        }),
        RuleError::InvalidField("title*".to_owned())
    );
    assert_eq!(
        build_error(|r| {
            r.can(READ, POST).when(field("").eq(1));
        }),
        RuleError::InvalidField(String::new())
    );
    assert_eq!(
        build_error(|r| {
            r.can(READ, POST).when(field("a\" OR 1=1 --").eq(1));
        }),
        RuleError::InvalidField("a\" OR 1=1 --".to_owned())
    );
    assert_eq!(
        build_error(|r| {
            r.can(READ, POST).when(field("$where").eq(1));
        }),
        RuleError::UnsupportedOperator("$where".to_owned())
    );
}

#[test]
fn rejects_operands_of_the_wrong_type() {
    let invalid = |error: RuleError, field_name: &str, op: &str| match error {
        RuleError::InvalidValue {
            field, operator, ..
        } => {
            assert_eq!((field.as_str(), operator), (field_name, op));
        }
        other => panic!("expected InvalidValue, got {other:?}"),
    };
    invalid(
        build_error(|r| {
            r.can(READ, POST).when(field("published").lt(true));
        }),
        "published",
        "$lt",
    );
    invalid(
        build_error(|r| {
            r.can(READ, POST).when(field("rank").gte(None::<i64>));
        }),
        "rank",
        "$gte",
    );
    invalid(
        build_error(|r| {
            r.can(READ, POST)
                .when(field("id").eq(9_007_199_254_740_992_u64));
        }),
        "id",
        "$eq",
    );
    invalid(
        build_error(|r| {
            r.can(READ, POST)
                .when(field("id").is_in([1, -9_007_199_254_740_992_i64]));
        }),
        "id",
        "$in",
    );
    invalid(
        build_error(|r| {
            r.can(READ, POST).when(field("score").gt(f64::NAN));
        }),
        "score",
        "$gt",
    );
    invalid(
        build_error(|r| {
            r.can(READ, POST).when(field("tags").eq(json!(["a"])));
        }),
        "tags",
        "$eq",
    );
    // The largest safe integer and floats are fine.
    let ok = ability(|r| {
        r.can(READ, POST).when(
            field("id")
                .lte(9_007_199_254_740_991_u64)
                .and(field("score").gt(0.5)),
        );
    });
    assert!(ok.can(READ, POST, Some(&json!({ "id": 1, "score": 0.75 }))));
}

#[test]
fn rejects_duplicate_operator_and_empty_rule() {
    assert_eq!(
        build_error(|r| {
            r.can(READ, POST)
                .when(field("rank").gt(1))
                .when(field("rank").gt(2));
        }),
        RuleError::DuplicateOperator {
            field: "rank".to_owned(),
            operator: "$gt",
        }
    );
    assert!(matches!(
        build_error(|r| {
            r.can("", POST);
        }),
        RuleError::InvalidRule(_)
    ));
    assert!(matches!(
        build_error(|r| {
            r.push(Rule {
                subject: Vec::new(),
                ..Rule::allow(READ, POST)
            });
        }),
        RuleError::InvalidRule(_)
    ));
}

#[test]
#[should_panic(expected = "invalid ability rule")]
fn build_panics_on_an_invalid_rule() {
    let mut rules = Rules::new();
    rules.can(READ, POST).when(field("a.b").eq(1));
    let _ = rules.build();
}

#[test]
fn cond_values_from_uuids_and_timestamps() {
    use chrono::{TimeZone, Utc};
    use navree_access::engine::Cond;

    let user = uuid::Uuid::from_u128(0x0190_aaaa_bbbb_7ccc_8ddd_eeee_ffff_0001);
    let owned = field("author_id").eq(user);
    assert_eq!(
        owned.get("author_id"),
        Some(&[Cond::Eq(json!("0190aaaa-bbbb-7ccc-8ddd-eeeeffff0001"))][..])
    );
    // Matches the record as serde serializes it.
    let record = json!({ "author_id": serde_json::to_value(user).unwrap() });
    assert!(allows(owned, record));

    // Timestamps: RFC 3339 UTC, fixed microsecond precision, so text order
    // is time order.
    let at = Utc.with_ymd_and_hms(2026, 10, 25, 10, 0, 0).unwrap();
    let since = field("created_at").gte(at);
    assert_eq!(
        since.get("created_at"),
        Some(&[Cond::Gte(json!("2026-10-25T10:00:00.000000Z"))][..])
    );
    assert!(allows(
        since,
        json!({ "created_at": "2026-10-25T10:00:00.500000Z" })
    ));
}

#[test]
fn subject_and_action_markers_name_the_check() {
    use navree_access::engine::{ActionMarker, Delete, Manage, Read, Subject, Update};

    struct Post;
    impl Subject for Post {
        const SUBJECT: &'static str = "Post";
        const PERMISSION_PREFIX: &'static str = "posts";
    }
    assert_eq!(<Read as ActionMarker>::NAME, READ);
    assert_eq!(Update::NAME, UPDATE);
    assert_eq!(Delete::NAME, DELETE);
    assert_eq!(Manage::NAME, MANAGE);
    assert_eq!(navree_access::engine::Create::NAME, CREATE);

    let ability = ability(|r| {
        r.can(Read::NAME, Post::SUBJECT);
    });
    assert!(ability.can(Read::NAME, Post::SUBJECT, None));
    assert_eq!(Post::PERMISSION_PREFIX, "posts");
}

#[test]
fn field_refs_can_be_constants() {
    use navree_access::engine::FieldRef;

    const AUTHOR_ID: FieldRef = FieldRef::new("author_id");
    assert_eq!(AUTHOR_ID.name(), "author_id");
    assert_eq!(AUTHOR_ID.eq(7), field("author_id").eq(7));
}
