//! Factories without the database: definitions, counts, states, sequences,
//! seeds, raw values, hooks and soft-deleted records.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::{Duration, TimeZone, Utc};
use navree_orm::factory::{Factory, HasFactory, ModelFactory, Sequence};
use navree_orm::{Model, OrmError, Record, Value};

use crate::models::{
    Activity, Company, CompanyFactory, Deal, DealFactory, Note, Stage, Tag, TagFactory,
};

#[test]
fn make_one_builds_an_unsaved_record_from_the_definition() {
    let company = Company::factory().make_one().unwrap();
    assert!(!company.exists());
    assert!(!company.name.is_empty());
    assert!((1..=500).contains(&company.employees));
    assert_eq!(company.id, 0, "the database assigns the key when it is saved");
    assert!(company.created_at == chrono::DateTime::<Utc>::UNIX_EPOCH, "timestamps wait for the save");
}

#[test]
fn the_model_and_its_factory_are_linked_by_type() {
    let through_trait: Factory<CompanyFactory> = <Company as HasFactory>::factory();
    assert_eq!(through_trait.make().unwrap().len(), 1);
    let of_value = Factory::of(CompanyFactory).count(2).make().unwrap();
    assert_eq!(of_value.len(), 2);
}

#[test]
fn count_sets_how_many_and_the_context_knows_each_position() {
    let deals = Deal::factory()
        .count(4)
        .state_with(|deal, ctx| deal.title = format!("{} of {}", ctx.index() + 1, ctx.count()))
        .make()
        .unwrap();
    let titles: Vec<&str> = deals.iter().map(|deal| deal.title.as_str()).collect();
    assert_eq!(titles, ["1 of 4", "2 of 4", "3 of 4", "4 of 4"]);
    assert!(Company::factory().count(0).make().unwrap().is_empty());
    assert_eq!(Company::factory().make_many(3).unwrap().len(), 3);
}

#[test]
fn states_run_in_order_after_the_definition() {
    let company = Company::factory()
        .state(CompanyFactory::large)
        .state(|c| c.employees += 1)
        .prepend_state(|c| c.employees = 1)
        .state(CompanyFactory::german)
        .make_one()
        .unwrap();
    assert_eq!(company.employees, 10_001, "prepended first, then in order");
    assert_eq!(company.country.as_deref(), Some("DE"));
}

#[test]
fn set_fixes_one_column_and_when_applies_states_conditionally() {
    let company = Company::factory()
        .set(Company::NAME, "Acme")
        .set(Company::COUNTRY, "NL")
        .when(true, |f| f.state(CompanyFactory::large))
        .when(false, |f| f.set(Company::NAME, "Never"))
        .make_one()
        .unwrap();
    assert_eq!(company.name, "Acme");
    assert_eq!(company.country.as_deref(), Some("NL"));
    assert_eq!(company.employees, 10_000);
}

#[test]
fn sequences_cycle_through_their_states() {
    let deals = Deal::factory()
        .count(5)
        .sequence([DealFactory::won, DealFactory::lost])
        .sequence(Sequence::column(Deal::TITLE, ["A", "B", "C"]))
        .sequence(Sequence::from_fn(|deal: &mut Deal, at| {
            deal.amount_cents = 100 * (at.index() as i64 + 1);
        }))
        .make()
        .unwrap();
    let stages: Vec<Stage> = deals.iter().map(|d| d.stage).collect();
    assert_eq!(
        stages,
        [Stage::Won, Stage::Lost, Stage::Won, Stage::Lost, Stage::Won]
    );
    let titles: Vec<&str> = deals.iter().map(|d| d.title.as_str()).collect();
    assert_eq!(titles, ["A", "B", "C", "A", "B"]);
    let amounts: Vec<i64> = deals.iter().map(|d| d.amount_cents).collect();
    assert_eq!(amounts, [100, 200, 300, 400, 500]);
}

#[test]
fn for_each_sequence_makes_one_record_per_state() {
    let deals = Deal::factory()
        .for_each_sequence(Sequence::column(Deal::TITLE, ["Pilot", "Renewal", "Upsell"]))
        .make()
        .unwrap();
    let titles: Vec<&str> = deals.iter().map(|d| d.title.as_str()).collect();
    assert_eq!(titles, ["Pilot", "Renewal", "Upsell"]);
}

#[test]
fn cross_joined_sequences_make_every_combination() {
    let deals = Deal::factory()
        .cross_join_sequence([
            Sequence::new([DealFactory::won, DealFactory::lost]),
            Sequence::column(Deal::TITLE, ["Small", "Large"]),
        ])
        .make()
        .unwrap();
    let pairs: Vec<(Stage, &str)> = deals.iter().map(|d| (d.stage, d.title.as_str())).collect();
    assert_eq!(
        pairs,
        [
            (Stage::Won, "Small"),
            (Stage::Won, "Large"),
            (Stage::Lost, "Small"),
            (Stage::Lost, "Large"),
        ]
    );
    let capped = Deal::factory()
        .count(2)
        .cross_join_sequence([Sequence::new([DealFactory::won, DealFactory::lost])])
        .make()
        .unwrap();
    assert_eq!(capped.len(), 2, "an explicit count wins");
}

#[test]
fn seeded_factories_build_the_same_records() {
    let first: Vec<String> = Company::factory()
        .count(5)
        .seed(42)
        .make()
        .unwrap()
        .iter()
        .map(|c| c.name.clone())
        .collect();
    let again: Vec<String> = Company::factory()
        .count(5)
        .seed(42)
        .make()
        .unwrap()
        .iter()
        .map(|c| c.name.clone())
        .collect();
    assert_eq!(first, again);
    let other: Vec<String> = Company::factory()
        .count(5)
        .seed(43)
        .make()
        .unwrap()
        .iter()
        .map(|c| c.name.clone())
        .collect();
    assert_ne!(first, other);
}

#[test]
fn raw_gives_the_generated_values_without_empty_keys_and_timestamps() {
    let raw = Company::factory()
        .set(Company::NAME, "Acme")
        .raw_one()
        .unwrap();
    let columns: Vec<&str> = raw.iter().map(|(column, _)| *column).collect();
    assert_eq!(columns, ["name", "country", "employees"]);
    assert_eq!(raw.get("name"), Some(&Value::from("Acme")));
    assert_eq!(raw.to_json()["name"], "Acme");
    assert_eq!(serde_json::to_value(&raw).unwrap()["name"], "Acme");

    let trashed = Company::factory().trashed().raw_one().unwrap();
    assert!(trashed.get("deleted_at").is_some(), "a set timestamp is kept");
    let many = Company::factory().count(3).raw().unwrap();
    assert_eq!(many.len(), 3);
}

#[test]
fn after_making_hooks_run_after_the_states_and_can_be_removed() {
    let seen = Arc::new(AtomicUsize::new(0));
    let counter = seen.clone();
    let tags = Tag::factory()
        .count(2)
        .set(Tag::NAME, "vip")
        .after_making(move |tag| {
            assert_eq!(tag.name, "vip", "after the states");
            assert!(!tag.exists());
            counter.fetch_add(1, Ordering::SeqCst);
        })
        .make()
        .unwrap();
    assert_eq!(tags.len(), 2);
    assert_eq!(seen.load(Ordering::SeqCst), 2);

    let counter = seen.clone();
    Tag::factory()
        .after_making(move |_| {
            counter.fetch_add(100, Ordering::SeqCst);
        })
        .without_after_making()
        .make()
        .unwrap();
    assert_eq!(seen.load(Ordering::SeqCst), 2);
}

/// A factory whose `configure` adds a hook and a state to every builder.
#[derive(Default)]
struct ShoutingTagFactory;

impl ModelFactory for ShoutingTagFactory {
    type Model = Tag;

    fn definition(&self, ctx: &mut navree_orm::factory::FactoryContext<'_, Tag>) -> Tag {
        TagFactory.definition(ctx)
    }

    fn configure(factory: Factory<Self>) -> Factory<Self> {
        factory
            .state(|tag| tag.name = tag.name.to_uppercase())
            .after_making(|tag: &mut Record<Tag>| tag.name.push('!'))
    }
}

#[test]
fn configure_gives_every_builder_its_defaults() {
    let tag = Factory::<ShoutingTagFactory>::new().make_one().unwrap();
    assert!(tag.name.ends_with('!'));
    assert_eq!(tag.name.trim_end_matches('!'), tag.name.trim_end_matches('!').to_uppercase());
    let quiet = Factory::<ShoutingTagFactory>::new()
        .without_after_making()
        .make_one()
        .unwrap();
    assert!(!quiet.name.ends_with('!'));
}

#[test]
fn trashed_sets_the_soft_delete_column() {
    let at = Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap();
    let company = Company::factory().trashed_at(at).make_one().unwrap();
    assert_eq!(company.deleted_at, Some(at));
    assert!(company.trashed());

    let recent = Company::factory().trashed().make_one().unwrap();
    let deleted = recent.deleted_at.expect("deleted");
    let age = Utc::now() - deleted;
    assert!(age > Duration::hours(23) && age < Duration::hours(25), "a day ago: {age}");
}

#[test]
fn trashed_fails_for_a_model_without_soft_deletes() {
    let error = Tag::factory().trashed().make().unwrap_err();
    assert!(matches!(error, OrmError::Factory { .. }));
    assert_eq!(
        error.to_string(),
        "the Tag factory failed: Tag does not soft-delete: trashed() needs #[model(soft_deletes)] on Tag"
    );
}

#[test]
fn make_uses_stored_and_recycled_parents_and_leaves_factory_parents_empty() {
    let mut acme = Record::new(Company {
        id: 7,
        name: "Acme".into(),
        ..Company::default()
    });
    acme.sync_original();
    // A factory parent cannot be created without the database.
    let orphan = Deal::factory().make_one().unwrap();
    assert_eq!(orphan.company_id, None);
    // A stored record is known.
    let owned = Deal::factory().for_(&acme, Deal::COMPANY_ID).make_one().unwrap();
    assert_eq!(owned.company_id, Some(7));
    // A recycled record replaces the definition's parent factory.
    let recycled = Deal::factory().recycle(&acme).count(3).make().unwrap();
    assert!(recycled.iter().all(|deal| deal.company_id == Some(7)));
}

#[test]
fn named_relationships_are_checked() {
    let unknown = Deal::factory()
        .has(Activity::factory(), "comments")
        .make()
        .unwrap_err();
    assert_eq!(
        unknown.to_string(),
        "the Deal factory failed: Deal has no relationship named `comments`; its factory declares company, activities, tags (in ModelFactory::relationships)"
    );
    let wrong_kind = Deal::factory()
        .for_(Company::factory(), "activities")
        .make()
        .unwrap_err();
    assert!(
        wrong_kind
            .to_string()
            .ends_with("`activities` of Deal is a has-many relationship (use it with `has`)"),
        "{wrong_kind}"
    );
    let wrong_model = Deal::factory()
        .has(Note::factory(), "activities")
        .make()
        .unwrap_err();
    assert!(
        wrong_model
            .to_string()
            .ends_with("`activities` of Deal links Activity records, not Note"),
        "{wrong_model}"
    );
    let undeclared = Company::factory()
        .has(Deal::factory(), "deals")
        .make()
        .unwrap_err();
    assert!(
        undeclared.to_string().contains("its factory declares none"),
        "{undeclared}"
    );
}

#[test]
fn a_definitions_unique_values_hold_across_the_batch() {
    let tags = Tag::factory().count(20).make().unwrap();
    let mut names: Vec<&str> = tags.iter().map(|tag| tag.name.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), 20);
    assert_eq!(<Tag as Model>::meta().name(), "Tag");
}
