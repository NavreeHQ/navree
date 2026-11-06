//! JSON conditions on SQLite: overlaps, key existence and containment on
//! typed columns and paths; hand-written operators and null-safe
//! inequality.

use navree_orm::{Expr, Filter, Model, OrmError, Value, col};

use crate::models::{Company, Deal, DealMeta};
use crate::support::sqlite;

async fn seed(db: &navree_db::Db) {
    for (title, tags, source) in [
        ("a", vec!["vip", "q1"], "web"),
        ("b", vec!["q2"], "ads"),
        ("c", vec![], "web"),
    ] {
        Deal::create(
            db,
            Deal {
                tags: tags.into_iter().map(Into::into).collect(),
                meta: DealMeta {
                    source: source.into(),
                    score: 1,
                },
                ..Deal::new(title, 10)
            },
        )
        .await
        .unwrap();
    }
}

async fn titles(db: &navree_db::Db, query: navree_orm::Builder<Deal>) -> Vec<String> {
    query
        .order_by(Deal::TITLE)
        .pluck(db, Deal::TITLE)
        .await
        .unwrap()
}

#[tokio::test]
async fn json_arrays_overlap_and_keys_exist() {
    let t = sqlite().await;
    seed(&t.db).await;
    assert_eq!(
        titles(
            &t.db,
            Deal::query().where_json_overlaps(Deal::TAGS, ["q1", "q2"])
        )
        .await,
        ["a", "b"]
    );
    assert_eq!(
        titles(
            &t.db,
            Deal::query().where_json_doesnt_overlap(Deal::TAGS, "vip")
        )
        .await,
        ["b", "c"]
    );
    assert_eq!(
        titles(&t.db, Deal::query().where_(Deal::TAGS.overlaps(["q2"]))).await,
        ["b"]
    );
    assert_eq!(
        titles(&t.db, Deal::query().where_json_contains_key("tags[1]")).await,
        ["a"],
        "only `a` has a second tag"
    );
    assert_eq!(
        titles(&t.db, Deal::query().where_json_contains_key("tags[-1]")).await,
        ["a", "b"],
        "a last tag"
    );
    assert_eq!(
        titles(&t.db, Deal::query().where_(Deal::META.has_key("source"))).await,
        ["a", "b", "c"]
    );
    assert!(
        titles(&t.db, Deal::query().where_json_contains_key("meta->owner"))
            .await
            .is_empty()
    );
    assert_eq!(
        titles(
            &t.db,
            Deal::query()
                .where_json_doesnt_contain_key("meta->owner")
                .where_(Deal::TAGS.contains("vip"))
        )
        .await,
        ["a"]
    );
    let error = Deal::query()
        .where_(Deal::TAGS.contained_by(["vip"]))
        .get(&t.db)
        .await
        .unwrap_err();
    assert!(matches!(error, OrmError::Unsupported { .. }), "{error}");
}

#[tokio::test]
async fn hand_written_operators_and_null_safe_inequality() {
    let t = sqlite().await;
    for (name, country) in [
        ("Agra", Some("US")),
        ("Gulati", None),
        ("Indrani", Some("DE")),
    ] {
        Company::create(
            &t.db,
            Company {
                name: name.into(),
                country: country.map(Into::into),
                ..Company::default()
            },
        )
        .await
        .unwrap();
    }
    let not_us: Vec<String> = Company::query()
        .where_(Company::COUNTRY.is_distinct_from("US"))
        .order_by(Company::NAME)
        .pluck(&t.db, Company::NAME)
        .await
        .unwrap();
    assert_eq!(not_us, ["Gulati", "Indrani"], "NULL is distinct from US");
    let globbed: Vec<String> = Company::query()
        .where_(Filter::op_raw(Company::NAME, "glob", Expr::val("*ech")))
        .pluck(&t.db, Company::NAME)
        .await
        .unwrap();
    assert_eq!(globbed, ["Indrani"]);
    let loud: Vec<Value> = Company::query()
        .order_by(Company::NAME)
        .pluck(&t.db, Expr::col("name").binary("||", Expr::val("!")))
        .await
        .unwrap();
    assert_eq!(loud[0], Value::from("Agra!"));
    let error = Company::query()
        .where_(Filter::op_raw(Company::NAME, "@>", Expr::val("x")))
        .get(&t.db)
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("is not an operator SQLite conditions take"),
        "{error}"
    );
    // Without a registered `regexp` function SQLite cannot run it.
    let error = Company::query()
        .where_(Company::NAME.regex("^A"))
        .get(&t.db)
        .await
        .unwrap_err();
    assert!(
        matches!(error, navree_orm::OrmError::Unsupported { .. }),
        "{error:?}"
    );
    assert_eq!(
        error.to_string(),
        "a regular expression without a `regexp` function registered on the connection is not supported on SQLite"
    );
    let _ = col("unused");
    assert_eq!(Company::meta().table(), "companies");
}
