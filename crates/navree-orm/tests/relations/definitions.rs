//! Declaring relations: names, kinds and metadata, the relation attributes
//! of `#[model(..)]`, relations other modules add, and unknown names.

use navree_orm::OrmError;
use navree_orm::prelude::*;
use navree_orm::relations::{HasMany, Relation, RelationKind, RelationRegistry};

use crate::fixtures::seed;
use crate::models::{
    Account, Activity, Company, Contact, Deal, DealTag, Image, Note, Tag, Team, User,
};
use crate::support::sqlite;

#[test]
fn relation_methods_are_registered_under_their_names() {
    let names = Deal::relation_names();
    for name in [
        "company",
        "owner",
        "activities",
        "latest_activity",
        "tags",
        "watchers",
        "notes",
        "image",
        "labels",
        "contacts",
    ] {
        assert!(
            names.iter().any(|known| known == name),
            "{name} in {names:?}"
        );
    }
    let kind =
        |relation: Option<navree_orm::relations::DynRelation<Deal>>| relation.unwrap().kind();
    assert_eq!(kind(Deal::relation("company")), RelationKind::BelongsTo);
    assert_eq!(kind(Deal::relation("activities")), RelationKind::HasMany);
    assert_eq!(
        kind(Deal::relation("latest_activity")),
        RelationKind::HasOne
    );
    assert_eq!(kind(Deal::relation("tags")), RelationKind::BelongsToMany);
    assert_eq!(kind(Deal::relation("notes")), RelationKind::MorphMany);
    assert_eq!(kind(Deal::relation("image")), RelationKind::MorphOne);
    assert_eq!(kind(Deal::relation("labels")), RelationKind::MorphToMany);
    assert_eq!(
        kind(Deal::relation("contacts")),
        RelationKind::HasManyThrough
    );
    assert_eq!(
        Tag::relation("labeled_deals").unwrap().kind(),
        RelationKind::MorphedByMany
    );
    assert_eq!(
        Note::relation("notable").unwrap().kind(),
        RelationKind::MorphTo
    );
    assert_eq!(
        Team::relation("biggest_deal").unwrap().kind(),
        RelationKind::HasOneThrough
    );
    assert!(RelationKind::HasManyThrough.is_to_many());
    assert!(!RelationKind::MorphTo.is_to_many());
    assert_eq!(RelationKind::MorphedByMany.to_string(), "morphed_by_many");

    // The relation itself carries the name of its method.
    let deal = Deal::default();
    assert_eq!(deal.company().name(), "company");
    assert_eq!(deal.latest_activity().name(), "latest_activity");
    assert_eq!(deal.tags().relation_name(), "tags");
    assert!(Deal::relation("nope").is_none());
    assert!(
        Contact::relation_names()
            .iter()
            .any(|name| name == "company")
    );
}

#[test]
fn relation_metadata_describes_models_tables_and_keys() {
    let company = Deal::relation_meta("company").unwrap();
    assert_eq!(company.kind, RelationKind::BelongsTo);
    assert_eq!(company.parent, "Deal");
    assert_eq!(company.related, Some("Company"));
    assert_eq!(company.related_table, Some("companies"));
    assert_eq!(company.foreign_key.as_deref(), Some("company_id"));
    assert_eq!(company.owner_key.as_deref(), Some("id"));

    let activities = Deal::relation_meta("activities").unwrap();
    assert_eq!(activities.foreign_key.as_deref(), Some("deal_id"));
    assert_eq!(activities.owner_key.as_deref(), Some("id"));

    let tags = Deal::relation_meta("tags").unwrap();
    assert_eq!(tags.pivot_table.as_deref(), Some("deal_tag"));
    assert_eq!(tags.foreign_key.as_deref(), Some("deal_id"));
    assert_eq!(tags.related_pivot_key.as_deref(), Some("tag_id"));

    let notes = Deal::relation_meta("notes").unwrap();
    assert_eq!(notes.morph_type.as_deref(), Some("notable_type"));
    assert_eq!(notes.morph_name.as_deref(), Some("deal"));
    assert_eq!(notes.foreign_key.as_deref(), Some("notable_id"));

    let deals = Team::relation_meta("deals").unwrap();
    assert_eq!(deals.through, Some("User"));
    assert_eq!(deals.related, Some("Deal"));

    let notable = Note::relation_meta("notable").unwrap();
    assert_eq!(notable.related, None);
    assert_eq!(notable.morph_type.as_deref(), Some("notable_type"));
    assert_eq!(notable.foreign_key.as_deref(), Some("notable_id"));
}

#[test]
fn model_attributes_name_morphs_foreign_keys_and_touches() {
    // `morph_name` given, or the snake_case type name.
    assert_eq!(Company::morph_name(), "company");
    assert_eq!(Deal::morph_name(), "deal");
    assert_eq!(DealTag::morph_name(), "deal_tag");
    // Foreign keys to a model: snake_case name, `_`, key column.
    assert_eq!(Company::foreign_key_name(), "company_id");
    assert_eq!(DealTag::foreign_key_name(), "deal_tag_tag_id");
    assert_eq!(Activity::touched_relations(), ["deal"]);
    assert!(Deal::touched_relations().is_empty());
    // A pivot model without an id is keyed by its related pivot column.
    assert_eq!(DealTag::meta().key_column(), "tag_id");
    assert_eq!(
        DealTag::meta().pivot_key_columns(),
        Some(("deal_id", "tag_id"))
    );
    assert_eq!(Image::meta().pivot_key_columns(), None);
}

#[tokio::test]
async fn model_defaults_eager_load_and_count_on_every_query() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;

    let accounts = Account::query()
        .order_by(Account::ID)
        .get(&t.db)
        .await
        .unwrap();
    let acme = &accounts[0];
    let owner = acme
        .relation::<Option<Record<User>>>("owner")
        .unwrap()
        .as_ref()
        .unwrap();
    assert_eq!(owner.name, "Ada");
    // The nested default (`owner.team`) loaded too.
    let team = owner
        .relation::<Option<Record<Team>>>("team")
        .unwrap()
        .as_ref()
        .unwrap();
    assert_eq!(team.id, crm.team.id);
    assert_eq!(acme.extra::<i64>("contacts_count").unwrap(), Some(2));
    // Every way of reading applies them.
    let found = Account::find_or_fail(&t.db, crm.globex.id).await.unwrap();
    assert!(found.relation_loaded("owner"));
    assert_eq!(found.extra::<i64>("contacts_count").unwrap(), Some(1));
    let by_filter = Account::where_(Account::NAME.eq("Acme"))
        .first(&t.db)
        .await
        .unwrap()
        .unwrap();
    assert!(by_filter.relation_loaded("owner"));

    // `without` and `without_eager_loads` take them off one query.
    let plain = Account::query()
        .without("owner")
        .without("contacts")
        .first(&t.db)
        .await
        .unwrap()
        .unwrap();
    assert!(!plain.relation_loaded("owner"));
    assert_eq!(plain.extra::<i64>("contacts_count").unwrap(), None);
    let bare = Account::query()
        .without_eager_loads()
        .first(&t.db)
        .await
        .unwrap()
        .unwrap();
    assert!(!bare.relation_loaded("owner"));
    assert_eq!(bare.extra::<i64>("contacts_count").unwrap(), None);
    assert!(
        Account::query()
            .with_only(["contacts"])
            .eager_loads()
            .eq(&["contacts".to_owned()])
    );
}

/// A relation another module adds to `Deal`, with a typed accessor.
trait DealCalls {
    fn long_calls(&self) -> HasMany<Deal, Activity>;
}

impl DealCalls for Deal {
    fn long_calls(&self) -> HasMany<Deal, Activity> {
        self.has_many(Activity::DEAL_ID)
            .where_(Activity::KIND.eq("call"))
            .where_(Activity::MINUTES.gte(40))
    }
}

#[tokio::test]
async fn other_modules_add_relations_by_name() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    RelationRegistry::define("long_calls", <Deal as DealCalls>::long_calls);
    assert!(
        Deal::relation_names()
            .iter()
            .any(|name| name == "long_calls")
    );
    assert_eq!(
        Deal::relation("long_calls").unwrap().kind(),
        RelationKind::HasMany
    );

    // It works wherever a relation name does.
    let deals = Deal::query()
        .with("long_calls")
        .has("long_calls")
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(deals.len(), 1);
    let calls = deals[0]
        .relation::<Collection<Activity>>("long_calls")
        .unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].minutes, 45);
    // And typed, through the accessor.
    assert_eq!(crm.renewal.long_calls().count(&t.db).await.unwrap(), 1);

    RelationRegistry::remove::<Deal>("long_calls");
    assert!(Deal::relation("long_calls").is_none());
}

#[tokio::test]
async fn unknown_relations_name_the_model_and_the_relation() {
    let t = sqlite().await;
    // Even without records to load into.
    let error = Deal::query().with("compnay").get(&t.db).await.unwrap_err();
    let OrmError::RelationNotFound {
        model,
        relation,
        known,
    } = &error
    else {
        panic!("{error:?}");
    };
    assert_eq!(model, "Deal");
    assert_eq!(relation, "compnay");
    assert!(known.iter().any(|name| name == "company"), "{known:?}");
    let message = error.to_string();
    assert!(
        message.contains("compnay") && message.contains("Deal") && message.contains("company"),
        "{message}"
    );

    seed(&t.db).await;
    // A nested step names the model it is looked up on.
    let error = Deal::query()
        .with("company.ownr")
        .get(&t.db)
        .await
        .unwrap_err();
    assert!(
        matches!(&error, OrmError::RelationNotFound { model, relation, .. } if model == "Company" && relation == "ownr"),
        "{error:?}"
    );
    // Existence queries and aggregates too.
    for error in [
        Deal::query().has("nope").get(&t.db).await.unwrap_err(),
        Deal::query()
            .with_count("nope")
            .get(&t.db)
            .await
            .unwrap_err(),
        Deal::query()
            .where_relation("nope", Company::NAME.eq("x"))
            .get(&t.db)
            .await
            .unwrap_err(),
    ] {
        assert!(error.to_string().contains("nope"), "{error}");
    }
    let mut deal = Deal::query().first(&t.db).await.unwrap().unwrap();
    let error = deal.load(&t.db, "nope").await.unwrap_err();
    assert!(
        matches!(&error, OrmError::RelationNotFound { relation, .. } if relation == "nope"),
        "{error:?}"
    );
}
