//! Existence queries: records that have (or lack) related records, through
//! nested paths, with counts, conditions, belongs-to and attached-to
//! shortcuts, self-relations, and the related model's global scopes.

use navree_orm::prelude::*;

use crate::fixtures::{names, seed, titles};
use crate::models::{Activity, Company, Contact, Deal, Tag, User};
use crate::support::sqlite;

#[tokio::test]
async fn records_with_related_records() {
    let t = sqlite().await;
    seed(&t.db).await;
    let ordered = || Deal::query().order_by(Deal::ID);

    let busy = ordered().has("activities").get(&t.db).await.unwrap();
    assert_eq!(titles(&busy), ["Renewal", "Upgrade"]);
    let busy = ordered().has(Deal::activities).get(&t.db).await.unwrap();
    assert_eq!(titles(&busy), ["Renewal", "Upgrade"]);

    // Nested: deals whose company has contacts, whose company has an owner.
    let reachable = ordered().has("company.contacts").get(&t.db).await.unwrap();
    assert_eq!(titles(&reachable), ["Renewal", "Upgrade", "Platform"]);
    let owned = ordered().has("company.owner").get(&t.db).await.unwrap();
    assert_eq!(titles(&owned), ["Renewal", "Upgrade", "Platform"]);

    // With a count.
    let several = ordered()
        .has_count("activities", ">=", 2)
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(titles(&several), ["Renewal"]);
    let exactly_one = ordered()
        .has_count(Deal::activities, "=", 1)
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(titles(&exactly_one), ["Upgrade"]);

    // `OR`.
    let either = ordered()
        .where_(Deal::AMOUNT_CENTS.gt(50_000))
        .or_has("activities")
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(titles(&either), ["Renewal", "Upgrade", "Platform"]);
}

#[tokio::test]
async fn records_without_related_records() {
    let t = sqlite().await;
    seed(&t.db).await;
    let ordered = || Deal::query().order_by(Deal::ID);
    let idle = ordered().doesnt_have("activities").get(&t.db).await.unwrap();
    assert_eq!(titles(&idle), ["Platform", "Orphan"]);
    let without_company = ordered().doesnt_have(Deal::company).get(&t.db).await.unwrap();
    assert_eq!(titles(&without_company), ["Orphan"]);
    let either = ordered()
        .where_(Deal::TITLE.eq("Renewal"))
        .or_doesnt_have("company")
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(titles(&either), ["Renewal", "Orphan"]);
}

#[tokio::test]
async fn related_records_matching_conditions() {
    let t = sqlite().await;
    seed(&t.db).await;
    let ordered = || Deal::query().order_by(Deal::ID);

    let with_long_calls = ordered()
        .where_has(Deal::activities, |q| {
            q.where_(Activity::KIND.eq("call"))
                .where_(Activity::MINUTES.gte(40))
        })
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(titles(&with_long_calls), ["Renewal"]);

    // A string path names the closure's model; the closure applies to the
    // last relation of a nested path.
    let acme_owned_by_ada = ordered()
        .where_has("company.owner", |q: Builder<User>| {
            q.where_(User::NAME.eq("Ada"))
        })
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(titles(&acme_owned_by_ada), ["Renewal", "Upgrade"]);

    let either = ordered()
        .where_has(Deal::activities, |q| q.where_(Activity::KIND.eq("meeting")))
        .or_where_has(Deal::company, |q| q.where_(Company::NAME.eq("Globex")))
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(titles(&either), ["Upgrade", "Platform"]);

    let two_calls = ordered()
        .where_has_count(
            Deal::activities,
            |q| q.where_(Activity::KIND.eq("call")),
            ">=",
            2,
        )
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(titles(&two_calls), ["Renewal"]);

    // The relation's own conditions apply inside the subquery.
    let with_calls = ordered().has("calls").get(&t.db).await.unwrap();
    assert_eq!(titles(&with_calls), ["Renewal"]);
}

#[tokio::test]
async fn no_related_records_matching_conditions() {
    let t = sqlite().await;
    seed(&t.db).await;
    let ordered = || Deal::query().order_by(Deal::ID);
    let no_calls = ordered()
        .where_doesnt_have(Deal::activities, |q| q.where_(Activity::KIND.eq("call")))
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(titles(&no_calls), ["Upgrade", "Platform", "Orphan"]);
    let either = ordered()
        .where_(Deal::TITLE.eq("Renewal"))
        .or_where_doesnt_have(Deal::activities, |q| q.where_(Activity::MINUTES.gt(0)))
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(titles(&either), ["Renewal", "Platform", "Orphan"]);
}

#[tokio::test]
async fn filtering_and_eager_loading_with_the_same_conditions() {
    let t = sqlite().await;
    seed(&t.db).await;
    let deals = Deal::query()
        .with_where_has(Deal::activities, |q| q.where_(Activity::KIND.eq("call")))
        .order_by(Deal::ID)
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(titles(&deals), ["Renewal"]);
    let calls = deals[0].loaded(Deal::activities).unwrap();
    assert_eq!(calls.len(), 2);
    assert!(calls.iter().all(|activity| activity.kind == "call"));

    let deals = Deal::query()
        .with_where_relation("company", Company::NAME.eq("Globex"))
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(titles(&deals), ["Platform"]);
    assert_eq!(
        deals[0]
            .loaded(Deal::company)
            .unwrap()
            .as_ref()
            .unwrap()
            .name,
        "Globex"
    );
}

#[tokio::test]
async fn conditions_on_one_related_column() {
    let t = sqlite().await;
    seed(&t.db).await;
    let ordered = || Deal::query().order_by(Deal::ID);
    let acme = ordered()
        .where_relation("company", Company::NAME.eq("Acme"))
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(titles(&acme), ["Renewal", "Upgrade"]);
    let nested = ordered()
        .where_relation("company.contacts", Contact::NAME.eq("Dave"))
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(titles(&nested), ["Platform"]);
    let either = ordered()
        .where_relation(Deal::company, Company::NAME.eq("Globex"))
        .or_where_relation("activities", Activity::KIND.eq("meeting"))
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(titles(&either), ["Upgrade", "Platform"]);

    let not_acme = ordered()
        .where_doesnt_have_relation("company", Company::NAME.eq("Acme"))
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(titles(&not_acme), ["Platform", "Orphan"]);
    let either = ordered()
        .where_(Deal::TITLE.eq("Renewal"))
        .or_where_doesnt_have_relation("activities", Activity::KIND.eq("call"))
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(titles(&either), ["Renewal", "Upgrade", "Platform", "Orphan"]);
}

#[tokio::test]
async fn belongs_to_a_record_or_one_of_several() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    let ordered = || Deal::query().order_by(Deal::ID);

    // Through the relation named after the owner's model (`company`).
    let acme = ordered().where_belongs_to(&crm.acme).get(&t.db).await.unwrap();
    assert_eq!(titles(&acme), ["Renewal", "Upgrade"]);
    // Through a named relation.
    let ada = ordered()
        .where_belongs_to_named(&crm.ada, "owner")
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(titles(&ada), ["Renewal", "Platform"]);
    let companies = Company::query()
        .where_key_in([crm.acme.id, crm.globex.id])
        .get(&t.db)
        .await
        .unwrap();
    let both = ordered()
        .where_belongs_to_any(&companies)
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(titles(&both), ["Renewal", "Upgrade", "Platform"]);
    let either = ordered()
        .where_belongs_to(&crm.globex)
        .or_where_belongs_to(&crm.acme)
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(either.len(), 3);

    // The relation must be a belongs-to to the owner's model.
    let error = ordered()
        .where_belongs_to(&crm.ada)
        .get(&t.db)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("`user`"), "{error}");
    let error = ordered()
        .where_belongs_to_named(&crm.ada, "company")
        .get(&t.db)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("belongs_to"), "{error}");
}

#[tokio::test]
async fn attached_to_a_record_or_records() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    crm.renewal
        .tags()
        .attach(&t.db, [crm.vip.id, crm.q1.id])
        .await
        .unwrap();
    crm.platform.tags().attach(&t.db, [crm.q1.id]).await.unwrap();
    let ordered = || Deal::query().order_by(Deal::ID);

    let vip = ordered().where_attached_to(&crm.vip).get(&t.db).await.unwrap();
    assert_eq!(titles(&vip), ["Renewal"]);
    let q1 = ordered()
        .where_attached_to_named(&crm.q1, "tags")
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(titles(&q1), ["Renewal", "Platform"]);
    let tags = Tag::query()
        .where_key_in([crm.vip.id, crm.hot.id])
        .get(&t.db)
        .await
        .unwrap();
    let any = ordered().where_attached_to_any(&tags).get(&t.db).await.unwrap();
    assert_eq!(titles(&any), ["Renewal"]);
    let either = ordered()
        .where_(Deal::TITLE.eq("Orphan"))
        .or_where_attached_to(&crm.q1)
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(titles(&either), ["Renewal", "Platform", "Orphan"]);

    let error = ordered()
        .where_attached_to_named(&crm.vip, "company")
        .get(&t.db)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("many-to-many"), "{error}");
}

#[tokio::test]
async fn self_relations_alias_the_inner_table() {
    let t = sqlite().await;
    seed(&t.db).await;
    let parents = Company::query()
        .has("subsidiaries")
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(names(&parents), ["Acme"]);
    let children = Company::query()
        .has("parent")
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(names(&children), ["Acme Labs"]);
    let of_acme = Company::query()
        .where_relation("parent", Company::NAME.eq("Acme"))
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(names(&of_acme), ["Acme Labs"]);
    // Two levels deep, and a count of the same table.
    let grandparents = Company::query()
        .has("subsidiaries.subsidiaries")
        .get(&t.db)
        .await
        .unwrap();
    assert!(grandparents.is_empty());
    let counted = Company::query()
        .with_count("subsidiaries")
        .order_by(Company::ID)
        .get(&t.db)
        .await
        .unwrap();
    let counts: Vec<Option<i64>> = counted
        .iter()
        .map(|company| company.extra::<i64>("subsidiaries_count").unwrap())
        .collect();
    assert_eq!(counts, [Some(1), Some(0), Some(0)]);

    let sql = Company::query()
        .where_has("subsidiaries", |q: Builder<Company>| {
            q.where_(Company::NAME.like("Acme%"))
        })
        .to_sql(Dialect::Sqlite)
        .unwrap()
        .sql;
    assert!(sql.contains(r#""companies" AS "navree_1""#), "{sql}");
    assert!(
        sql.contains(r#""companies"."id" = "navree_1"."parent_id""#),
        "{sql}"
    );
    assert!(sql.contains(r#""navree_1"."name" LIKE"#), "{sql}");
}

#[tokio::test]
async fn existence_subqueries_apply_the_related_models_global_scopes() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    let mut platform = Deal::find_or_fail(&t.db, crm.platform.id).await.unwrap();
    platform.delete(&t.db).await.unwrap();
    // Globex's only deal is soft-deleted.
    let with_deals = Company::query()
        .has("deals")
        .order_by(Company::ID)
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(names(&with_deals), ["Acme"]);
    let counts = Company::query()
        .with_count("deals")
        .order_by(Company::ID)
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(counts[2].extra::<i64>("deals_count").unwrap(), Some(0));
    // A constraint can ask for trashed records.
    let any = Company::query()
        .where_has(Company::deals, |q| q.with_trashed())
        .order_by(Company::ID)
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(names(&any), ["Acme", "Globex"]);
}

#[tokio::test]
async fn merging_another_querys_constraints() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    let big_or_acme = Deal::query()
        .where_(Deal::AMOUNT_CENTS.gte(25_000))
        .or_where(Deal::COMPANY_ID.eq(crm.acme.id));
    let deals = crm
        .ada
        .deals()
        .into_query()
        .unwrap()
        .merge_constraints_from(&big_or_acme)
        .order_by(Deal::ID)
        .get(&t.db)
        .await
        .unwrap();
    // The `OR` stays inside its group: only Ada's deals.
    assert_eq!(titles(&deals), ["Renewal", "Platform"]);
    // The global scopes the other query leaves out are left out.
    let trashed_too = Deal::query().with_trashed();
    let sql = Deal::query()
        .merge_constraints_from(&trashed_too)
        .to_sql(Dialect::Sqlite)
        .unwrap()
        .sql;
    assert!(!sql.contains("WHERE"), "{sql}");
}
