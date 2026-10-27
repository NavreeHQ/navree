//! The SQL relations generate, pinned per dialect: every snapshot shows
//! the PostgreSQL (TimescaleDB), MySQL and SQLite statement with its bound
//! values (or the error a dialect answers).

use std::fmt::Write as _;

use navree_orm::ir::AggregateFunction;
use navree_orm::prelude::*;
use navree_orm::{Model, Statement};

use crate::models::{Activity, Company, Contact, Deal, Note, Tag, Team, User};
use crate::support::register_morphs;

const DIALECTS: [Dialect; 3] = [Dialect::Postgres, Dialect::MySql, Dialect::Sqlite];

/// Each dialect's statement (or error), one block per dialect.
fn per_dialect(compile: impl Fn(Dialect) -> navree_orm::Result<Statement>) -> String {
    let mut out = String::new();
    for dialect in DIALECTS {
        match compile(dialect) {
            Ok(statement) => {
                writeln!(out, "{dialect}: {}", statement.sql).unwrap();
                if !statement.values.is_empty() {
                    writeln!(out, "  values: {:?}", statement.values).unwrap();
                }
            }
            Err(error) => writeln!(out, "{dialect}: error: {error}").unwrap(),
        }
    }
    out
}

fn query<M: Model>(builder: Builder<M>) -> String {
    per_dialect(|dialect| builder.to_sql(dialect))
}

fn deal() -> Deal {
    Deal {
        id: 7,
        company_id: Some(3),
        owner_id: Some(2),
        ..Deal::new("Renewal", 5_000)
    }
}

#[test]
fn belongs_to_and_has_many_queries() {
    insta::assert_snapshot!(
        "belongs_to",
        per_dialect(|dialect| deal().company().to_sql(dialect))
    );
    insta::assert_snapshot!(
        "has_many_with_caller_conditions",
        per_dialect(|dialect| {
            deal()
                .activities()
                .where_(Activity::KIND.eq("call"))
                .or_where(Activity::MINUTES.gt(30))
                .latest()
                .to_sql(dialect)
        })
    );
    insta::assert_snapshot!(
        "morph_many",
        per_dialect(|dialect| deal().notes().to_sql(dialect))
    );
}

#[test]
fn many_to_many_queries() {
    insta::assert_snapshot!(
        "belongs_to_many_with_pivot_columns",
        per_dialect(|dialect| {
            deal()
                .tags()
                .where_pivot("role", "=", "owner")
                .order_by_pivot_desc("created_at")
                .to_sql(dialect)
        })
    );
    insta::assert_snapshot!(
        "morph_to_many",
        per_dialect(|dialect| deal().labels().to_sql(dialect))
    );
    insta::assert_snapshot!(
        "morphed_by_many",
        per_dialect(|dialect| {
            Tag {
                id: 4,
                ..Tag::default()
            }
            .labeled_deals()
            .to_sql(dialect)
        })
    );
}

#[test]
fn through_and_one_of_many_queries() {
    let team = Team {
        id: 1,
        ..Team::default()
    };
    insta::assert_snapshot!(
        "has_many_through",
        per_dialect(|dialect| team.deals().to_sql(dialect))
    );
    insta::assert_snapshot!(
        "composed_through",
        per_dialect(|dialect| deal().contacts().to_sql(dialect))
    );
    insta::assert_snapshot!(
        "latest_of_many",
        per_dialect(|dialect| deal().latest_activity().to_sql(dialect))
    );
    insta::assert_snapshot!(
        "of_many_with_a_constraint",
        per_dialect(|dialect| deal().longest_call().to_sql(dialect))
    );
    insta::assert_snapshot!(
        "has_one_through_of_many",
        per_dialect(|dialect| team.biggest_deal().to_sql(dialect))
    );
}

#[test]
fn existence_queries() {
    insta::assert_snapshot!("has", query(Deal::query().has("activities")));
    insta::assert_snapshot!(
        "has_count",
        query(Deal::query().has_count("activities", ">=", 3))
    );
    insta::assert_snapshot!(
        "nested_where_has",
        query(Deal::query().where_has("company.contacts", |q: Builder<Contact>| {
            q.where_(Contact::NAME.like("A%"))
        }))
    );
    insta::assert_snapshot!(
        "doesnt_have_or_where_relation",
        query(
            Deal::query()
                .doesnt_have("tags")
                .or_where_relation("owner", User::NAME.eq("Ada"))
        )
    );
    insta::assert_snapshot!(
        "self_relation",
        query(Company::query().where_has("subsidiaries.subsidiaries", |q: Builder<Company>| {
            q.where_(Company::ACTIVE.eq(true))
        }))
    );
    insta::assert_snapshot!(
        "through_existence",
        query(Team::query().where_relation("deals", Deal::AMOUNT_CENTS.gte(1_000)))
    );
    insta::assert_snapshot!(
        "where_belongs_to",
        query(Deal::query().where_belongs_to(&Record::new(Company {
            id: 3,
            ..Company::default()
        })))
    );
}

#[test]
fn polymorphic_existence_queries() {
    register_morphs();
    insta::assert_snapshot!(
        "has_morph",
        query(Note::query().has_morph("notable", ["deal", "company"]))
    );
    // The alternatives stay grouped next to other conditions.
    insta::assert_snapshot!(
        "has_morph_and_another_condition",
        query(
            Note::query()
                .where_(Note::BODY.eq("x"))
                .has_morph("notable", ["deal", "company"])
        )
    );
    insta::assert_snapshot!(
        "doesnt_have_morph",
        query(Note::query().doesnt_have_morph(
            "notable",
            navree_orm::relations::MorphTypes::named(["deal"])
        ))
    );
    insta::assert_snapshot!(
        "where_morphed_to",
        query(Note::query().where_morphed_to("notable", &Record::new(deal())))
    );
}

#[test]
fn relation_aggregates() {
    insta::assert_snapshot!(
        "aggregates",
        query(
            Deal::query()
                .with_count("activities")
                .with_sum("activities as minutes", Activity::MINUTES)
                .with_aggregate("activities", Activity::MINUTES, AggregateFunction::Max)
                .with_exists("tags")
                .with_count("contacts")
        )
    );
}
