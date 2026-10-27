//! Polymorphic relations: morph one and many, morph to (typed results,
//! per-type loads and constraints, soft-delete visibility), the morph map,
//! morph ids of every key type, and polymorphic existence queries.

use navree_orm::OrmError;
use navree_orm::prelude::*;
use navree_orm::relations::{AnyRecord, MorphMap, MorphRef, MorphTypes};

use crate::fixtures::seed;
use crate::models::{Activity, Attachment, Company, Deal, Event, Image, Mark, Note, Ticket, User};
use crate::support::sqlite;

navree_orm::morph_types! {
    /// What a note is about.
    pub enum Notable {
        Deal(Deal),
        Company(Company),
    }
}

/// Notes on Renewal, Acme and Globex, and one about nothing.
async fn notes(db: &navree_db::Db, crm: &crate::fixtures::Crm) -> Vec<Record<Note>> {
    let mut notes = Vec::new();
    notes.push(
        crm.renewal
            .notes()
            .create(db, Note::new("renewal call"))
            .await
            .unwrap(),
    );
    notes.push(
        crm.acme
            .notes()
            .create(db, Note::new("acme lunch"))
            .await
            .unwrap(),
    );
    notes.push(
        crm.globex
            .notes()
            .create(db, Note::new("globex visit"))
            .await
            .unwrap(),
    );
    notes.push(Note::create(db, Note::new("loose")).await.unwrap());
    notes
}

fn notable(note: &Record<Note>) -> Option<&AnyRecord> {
    note.loaded(Note::notable)
        .expect("notable is loaded")
        .as_ref()
}

#[tokio::test]
async fn morph_one_and_morph_many() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    notes(&t.db, &crm).await;
    crm.renewal
        .image()
        .create(
            &t.db,
            Image {
                url: "renewal.png".into(),
                ..Image::default()
            },
        )
        .await
        .unwrap();

    // Acme and Renewal share the key 1: the type column keeps them apart.
    assert_eq!(crm.acme.id, crm.renewal.id);
    let deal_notes = crm.renewal.notes().get(&t.db).await.unwrap();
    assert_eq!(deal_notes.len(), 1);
    assert_eq!(deal_notes[0].body, "renewal call");
    assert!(crm.acme.logo().first(&t.db).await.unwrap().is_none());
    assert_eq!(
        crm.renewal.image().first(&t.db).await.unwrap().unwrap().url,
        "renewal.png"
    );

    let deals = Deal::query()
        .with("notes")
        .with("image")
        .order_by(Deal::ID)
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(deals[0].loaded(Deal::notes).unwrap().len(), 1);
    assert!(deals[1].loaded(Deal::notes).unwrap().is_empty());
    assert!(deals[0].loaded(Deal::image).unwrap().is_some());
    let companies = Company::query()
        .with_count("notes")
        .has("notes")
        .order_by(Company::ID)
        .get(&t.db)
        .await
        .unwrap();
    let counts: Vec<(String, Option<i64>)> = companies
        .iter()
        .map(|c| (c.name.clone(), c.extra::<i64>("notes_count").unwrap()))
        .collect();
    assert_eq!(
        counts,
        [("Acme".to_owned(), Some(1)), ("Globex".to_owned(), Some(1))]
    );
}

#[tokio::test]
async fn morph_to_results_are_typed() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    let created = notes(&t.db, &crm).await;

    // One query per type.
    t.log.clear();
    let notes = Note::query()
        .with("notable")
        .order_by(Note::ID)
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(t.log.selects().len(), 3, "notes, deals, companies");
    let deal = notable(&notes[0]).unwrap();
    assert!(deal.is::<Deal>());
    assert_eq!(deal.model_name(), "Deal");
    assert_eq!(deal.morph_name(), "deal");
    assert_eq!(deal.downcast_ref::<Deal>().unwrap().title, "Renewal");
    assert!(deal.downcast_ref::<Company>().is_none());
    let company = notable(&notes[1]).unwrap();
    assert_eq!(company.downcast_ref::<Company>().unwrap().name, "Acme");
    assert_eq!(company.key_string(), crm.acme.id.to_string());
    assert!(notable(&notes[3]).is_none());

    // Into an enum of the possible models.
    match Notable::try_from(notable(&notes[2]).unwrap().clone()) {
        Ok(Notable::Company(globex)) => assert_eq!(globex.name, "Globex"),
        _ => panic!("Globex is a company"),
    }
    let record: Record<Deal> = notable(&notes[0]).unwrap().clone().downcast().unwrap();
    assert_eq!(record.id, crm.renewal.id);
    assert!(
        notable(&notes[0])
            .unwrap()
            .clone()
            .downcast::<Company>()
            .is_err()
    );

    // Queried directly, and loaded afterwards.
    let any = created[1].notable().get(&t.db).await.unwrap().unwrap();
    assert!(any.is::<Company>());
    assert!(created[3].notable().get(&t.db).await.unwrap().is_none());
    let mut note = Note::find_or_fail(&t.db, created[0].id).await.unwrap();
    note.load(&t.db, "notable").await.unwrap();
    assert!(notable(&note).unwrap().is::<Deal>());
    assert!(created[0].notable().is(&crm.renewal));
    assert!(
        created[0].notable().is_not(&crm.acme),
        "same key, other type"
    );

    // Serialized as the related record.
    let json = serde_json::to_value(&notes[1]).unwrap();
    assert_eq!(json["notable"]["name"], "Acme");
    assert!(json["notable_type"] == "company");
}

#[tokio::test]
async fn morph_to_loads_nested_relations_per_type() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    notes(&t.db, &crm).await;
    t.log.clear();
    let notes = Note::query()
        .with_morph(
            "notable",
            MorphWith::new()
                .on::<Deal>(["company", "activities"])
                .on::<Company>(["contacts"])
                .count_on::<Company>(["deals"]),
        )
        .order_by(Note::ID)
        .get(&t.db)
        .await
        .unwrap();
    // Notes, deals, the deals' companies and activities, companies, their
    // contacts (the count is a subquery).
    assert_eq!(t.log.selects().len(), 6);
    let renewal = notable(&notes[0]).unwrap().downcast_ref::<Deal>().unwrap();
    assert_eq!(
        renewal
            .loaded(Deal::company)
            .unwrap()
            .as_ref()
            .unwrap()
            .name,
        "Acme"
    );
    assert_eq!(renewal.loaded(Deal::activities).unwrap().len(), 3);
    let acme = notable(&notes[1])
        .unwrap()
        .downcast_ref::<Company>()
        .unwrap();
    assert_eq!(acme.loaded(Company::contacts).unwrap().len(), 2);
    assert_eq!(acme.extra::<i64>("deals_count").unwrap(), Some(2));
    assert!(!acme.relation_loaded("activities"));

    // Constraints per type.
    let notes = Note::query()
        .with_morph(
            "notable",
            MorphWith::new().constrain::<Company>(|q| q.where_(Company::NAME.ne("Acme"))),
        )
        .order_by(Note::ID)
        .get(&t.db)
        .await
        .unwrap();
    assert!(notable(&notes[0]).is_some(), "deals are not constrained");
    assert!(notable(&notes[1]).is_none(), "Acme is filtered out");
    assert!(notable(&notes[2]).is_some());
}

#[tokio::test]
async fn morph_loads_after_the_fact() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    let created = notes(&t.db, &crm).await;
    let mut note = Note::find_or_fail(&t.db, created[0].id).await.unwrap();
    note.load_morph(&t.db, "notable", MorphWith::new().on::<Deal>(["owner"]))
        .await
        .unwrap();
    let deal = notable(&note).unwrap().downcast_ref::<Deal>().unwrap();
    assert_eq!(
        deal.loaded(Deal::owner).unwrap().as_ref().unwrap().name,
        "Ada"
    );
    note.load_morph_count(
        &t.db,
        "notable",
        MorphWith::new().count_on::<Deal>(["activities"]),
    )
    .await
    .unwrap();
    let deal = notable(&note).unwrap().downcast_ref::<Deal>().unwrap();
    assert_eq!(deal.extra::<i64>("activities_count").unwrap(), Some(3));
    let with = || MorphWith::new().count_on::<Deal>(["activities"]);
    note.load_morph_sum(&t.db, "notable", with(), Activity::MINUTES)
        .await
        .unwrap();
    note.load_morph_min(&t.db, "notable", with(), Activity::MINUTES)
        .await
        .unwrap();
    note.load_morph_max(&t.db, "notable", with(), Activity::MINUTES)
        .await
        .unwrap();
    note.load_morph_avg(&t.db, "notable", with(), Activity::MINUTES)
        .await
        .unwrap();
    let deal = notable(&note).unwrap().downcast_ref::<Deal>().unwrap();
    assert_eq!(
        deal.extra::<i64>("activities_sum_minutes").unwrap(),
        Some(80)
    );
    assert_eq!(
        deal.extra::<i64>("activities_min_minutes").unwrap(),
        Some(5)
    );
    assert_eq!(
        deal.extra::<i64>("activities_max_minutes").unwrap(),
        Some(45)
    );
    assert!(
        deal.extra::<f64>("activities_avg_minutes")
            .unwrap()
            .is_some()
    );

    let mut all = Note::query().order_by(Note::ID).get(&t.db).await.unwrap();
    all.load_morph(&t.db, "notable", MorphWith::new().on::<Company>(["owner"]))
        .await
        .unwrap();
    let acme = notable(&all[1]).unwrap().downcast_ref::<Company>().unwrap();
    assert_eq!(
        acme.loaded(Company::owner).unwrap().as_ref().unwrap().name,
        "Ada"
    );
    all.load_morph_count(
        &t.db,
        "notable",
        MorphWith::new().count_on::<Company>(["contacts"]),
    )
    .await
    .unwrap();
    let globex = notable(&all[2]).unwrap().downcast_ref::<Company>().unwrap();
    assert_eq!(globex.extra::<i64>("contacts_count").unwrap(), Some(1));

    // A missing chain for the records of one type among mixed ones.
    let mut mixed: Vec<AnyRecord> = all
        .iter()
        .filter_map(|note| notable(note).cloned())
        .collect();
    navree_orm::relations::load_missing_chain::<Company, _>(&t.db, &mut mixed, ["contacts"])
        .await
        .unwrap();
    assert!(
        !mixed[0]
            .downcast_ref::<Deal>()
            .unwrap()
            .relation_loaded("contacts")
    );
    assert_eq!(
        mixed[1]
            .downcast_ref::<Company>()
            .unwrap()
            .loaded(Company::contacts)
            .unwrap()
            .len(),
        2
    );
    assert!(
        mixed[1]
            .downcast_ref::<Company>()
            .unwrap()
            .relation_loaded("owner"),
        "kept"
    );

    // `load_morph` on another kind of relation is an error.
    let error = note
        .load_morph(&t.db, "notable_typo", MorphWith::new())
        .await
        .unwrap_err();
    assert!(
        matches!(error, OrmError::RelationNotFound { .. }),
        "{error:?}"
    );
    let mut deal = Deal::find_or_fail(&t.db, crm.renewal.id).await.unwrap();
    let error = deal
        .load_morph(&t.db, "company", MorphWith::new())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("not a morph_to"), "{error}");
}

#[tokio::test]
async fn morph_to_soft_delete_visibility() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    for (type_, id) in [
        ("deal", crm.renewal.id),
        ("deal", crm.upgrade.id),
        ("company", crm.acme.id),
    ] {
        Image::create(
            &t.db,
            Image {
                imageable_type: Some(type_.into()),
                imageable_id: Some(id),
                url: format!("{type_}-{id}.png"),
                ..Image::default()
            },
        )
        .await
        .unwrap();
    }
    let mut upgrade = Deal::find_or_fail(&t.db, crm.upgrade.id).await.unwrap();
    upgrade.delete(&t.db).await.unwrap();

    let found = |images: &Collection<Image>, name: &str| -> Vec<bool> {
        images
            .iter()
            .map(|image| image.relation::<Option<AnyRecord>>(name).unwrap().is_some())
            .collect()
    };
    let images = Image::query()
        .with("imageable")
        .with("imageable_with_trashed")
        .with("imageable_only_trashed")
        .order_by(Image::ID)
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(found(&images, "imageable"), [true, false, true]);
    assert_eq!(found(&images, "imageable_with_trashed"), [true, true, true]);
    // Only trashed records of the soft-deleting models; others as usual.
    assert_eq!(
        found(&images, "imageable_only_trashed"),
        [false, true, true]
    );
}

#[tokio::test]
async fn the_morph_map_names_models() {
    crate::support::register_morphs();
    assert_eq!(MorphMap::model_for("deal"), Some("Deal"));
    assert_eq!(MorphMap::model_for("company"), Some("Company"));
    assert_eq!(MorphMap::name_for::<Ticket>().as_deref(), Some("ticket"));
    assert_eq!(MorphMap::name_for::<User>(), None);
    assert!(MorphMap::names().iter().any(|name| name == "event"));

    // A map of its own for one future.
    MorphMap::new()
        .with::<Deal>()
        .scope(async {
            assert_eq!(MorphMap::model_for("deal"), Some("Deal"));
            assert_eq!(MorphMap::model_for("company"), None);
            assert!(!MorphMap::is_required());
        })
        .await;
    Notable::register();
    assert_eq!(MorphMap::model_for("company"), Some("Company"));
}

#[tokio::test]
async fn a_required_morph_map_refuses_unmapped_models() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    MorphMap::new()
        .with::<Deal>()
        .required()
        .scope(async {
            assert!(MorphMap::is_required());
            // A mapped model is written as usual.
            crm.renewal
                .notes()
                .create(&t.db, Note::new("mapped"))
                .await
                .unwrap();
            // An unmapped one is an error naming it.
            let error = crm
                .acme
                .notes()
                .create(&t.db, Note::new("unmapped"))
                .await
                .unwrap_err();
            let OrmError::MorphNotMapped { model } = &error else {
                panic!("{error:?}");
            };
            assert_eq!(model, "Company");
            let mut note = Record::new(Note::new("associated"));
            assert!(note.associate(Note::notable, &crm.acme).is_err());
        })
        .await;

    // A stored name the map does not know is an error naming it.
    Note::create(
        &t.db,
        Note {
            notable_type: Some("opportunity".into()),
            notable_id: Some(1),
            ..Note::new("renamed model")
        },
    )
    .await
    .unwrap();
    let error = Note::query().with("notable").get(&t.db).await.unwrap_err();
    let OrmError::UnknownMorph { name } = &error else {
        panic!("{error:?}");
    };
    assert_eq!(name, "opportunity");
    // Once the stored names are migrated, they load again.
    crate::support::exec(
        &t.db,
        "UPDATE notes SET notable_type = 'deal' WHERE notable_type = 'opportunity'",
    )
    .await;
    assert_eq!(
        Note::query()
            .with("notable")
            .get(&t.db)
            .await
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn morph_ids_of_every_key_type() {
    let t = sqlite().await;
    // UUID keys.
    let ticket = Ticket::create(
        &t.db,
        Ticket {
            title: "Printer".into(),
            ..Ticket::default()
        },
    )
    .await
    .unwrap();
    ticket
        .attachments()
        .create(
            &t.db,
            Attachment {
                file: "log.txt".into(),
                ..Attachment::default()
            },
        )
        .await
        .unwrap();
    let attachments = Attachment::query()
        .with("attachable")
        .get(&t.db)
        .await
        .unwrap();
    let loaded = attachments[0]
        .loaded(Attachment::attachable)
        .unwrap()
        .as_ref()
        .unwrap();
    assert_eq!(loaded.downcast_ref::<Ticket>().unwrap().id, ticket.id);
    let with_files = Ticket::query().has("attachments").get(&t.db).await.unwrap();
    assert_eq!(with_files.len(), 1);
    let on_tickets = Attachment::query()
        .where_has_morph("attachable", MorphTypes::of::<Ticket>(), |q, _| {
            q.where_(Ticket::TITLE.eq("Printer"))
        })
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(on_tickets.len(), 1);

    // ULID keys.
    let event = Event::create(
        &t.db,
        Event {
            name: "Launch".into(),
            ..Event::default()
        },
    )
    .await
    .unwrap();
    event
        .marks()
        .create(
            &t.db,
            Mark {
                label: "important".into(),
                ..Mark::default()
            },
        )
        .await
        .unwrap();
    let marks = Mark::query().with("markable").get(&t.db).await.unwrap();
    let marked = marks[0].loaded(Mark::markable).unwrap().as_ref().unwrap();
    assert_eq!(marked.downcast_ref::<Event>().unwrap().id, event.id);
    let events = Event::query().with_count("marks").get(&t.db).await.unwrap();
    assert_eq!(events[0].extra::<i64>("marks_count").unwrap(), Some(1));
}

#[tokio::test]
async fn polymorphic_existence_queries() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    notes(&t.db, &crm).await;
    // A note about a deal that is gone.
    Note::create(
        &t.db,
        Note {
            notable_type: Some("deal".into()),
            notable_id: Some(999),
            ..Note::new("dangling")
        },
    )
    .await
    .unwrap();
    let bodies = |notes: &Collection<Note>| -> Vec<String> {
        notes.iter().map(|note| note.body.clone()).collect()
    };
    let ordered = || Note::query().order_by(Note::ID);

    let about_deals = ordered()
        .has_morph("notable", MorphTypes::of::<Deal>())
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(bodies(&about_deals), ["renewal call"]);
    // Every mapped type.
    let about_anything = ordered()
        .has_morph("notable", MorphTypes::all())
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(about_anything.len(), 3);
    let either = ordered()
        .where_(Note::BODY.eq("loose"))
        .or_has_morph("notable", ["company"])
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(bodies(&either), ["acme lunch", "globex visit", "loose"]);
    // Of a listed type, pointing at no record.
    let gone = ordered()
        .doesnt_have_morph("notable", MorphTypes::of::<Deal>())
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(bodies(&gone), ["dangling"]);
    // Over every type, a NULL type counts too.
    let either = ordered()
        .where_(Note::BODY.eq("renewal call"))
        .or_doesnt_have_morph("notable", MorphTypes::all())
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(bodies(&either), ["renewal call", "loose", "dangling"]);

    // With conditions per type: the closure gets the type's name.
    let named_a = ordered()
        .where_has_morph(
            "notable",
            MorphTypes::of::<Deal>().and::<Company>(),
            |q, type_| match type_ {
                "deal" => q.where_(col("title").like("R%")),
                _ => q.where_(col("name").like("A%")),
            },
        )
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(bodies(&named_a), ["renewal call", "acme lunch"]);
    let either = ordered()
        .where_(Note::BODY.eq("loose"))
        .or_where_has_morph("notable", ["company"], |q, _| {
            q.where_(col("name").eq("Globex"))
        })
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(bodies(&either), ["globex visit", "loose"]);
    let not_globex = ordered()
        .where_doesnt_have_morph("notable", ["company"], |q, _| {
            q.where_(col("name").eq("Globex"))
        })
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(bodies(&not_globex), ["acme lunch"]);
    let either = ordered()
        .where_(Note::BODY.eq("globex visit"))
        .or_where_doesnt_have_morph("notable", MorphTypes::all(), |q, _| q)
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(bodies(&either), ["globex visit", "loose", "dangling"]);

    // One condition on a column of every listed type.
    let renewal = ordered()
        .where_morph_relation("notable", ["deal"], col("title").eq("Renewal"))
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(bodies(&renewal), ["renewal call"]);
    let either = ordered()
        .where_morph_relation("notable", ["deal"], col("title").eq("Renewal"))
        .or_where_morph_relation("notable", ["company"], col("name").eq("Acme"))
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(bodies(&either), ["renewal call", "acme lunch"]);
    let not_acme = ordered()
        .where_morph_doesnt_have_relation("notable", ["company"], col("name").eq("Acme"))
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(bodies(&not_acme), ["globex visit"]);
    let either = ordered()
        .where_(Note::BODY.eq("acme lunch"))
        .or_where_morph_doesnt_have_relation("notable", ["deal"], col("title").ne("x"))
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(bodies(&either), ["acme lunch", "dangling"]);
}

#[tokio::test]
async fn morph_to_pointing_at_a_record_or_a_type() {
    let t = sqlite().await;
    let crm = seed(&t.db).await;
    notes(&t.db, &crm).await;
    let bodies = |notes: Collection<Note>| -> Vec<String> {
        notes.iter().map(|note| note.body.clone()).collect()
    };
    let ordered = || Note::query().order_by(Note::ID);
    let acme = ordered()
        .where_morphed_to("notable", &crm.acme)
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(bodies(acme), ["acme lunch"]);
    let companies = ordered()
        .where_morphed_to("notable", MorphRef::type_of::<Company>())
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(bodies(companies), ["acme lunch", "globex visit"]);
    let several = Company::query()
        .where_key_in([crm.acme.id, crm.globex.id])
        .get(&t.db)
        .await
        .unwrap();
    let both = ordered()
        .where_morphed_to("notable", &several)
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(both.len(), 2);
    let not_acme = ordered()
        .where_not_morphed_to("notable", &crm.acme)
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(bodies(not_acme), ["renewal call", "globex visit", "loose"]);
    let either = ordered()
        .where_morphed_to("notable", &crm.renewal)
        .or_where_morphed_to("notable", MorphRef::named("company"))
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(either.len(), 3);
    let either = ordered()
        .where_morphed_to("notable", &crm.renewal)
        .or_where_not_morphed_to("notable", MorphRef::type_of::<Company>())
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(bodies(either), ["renewal call", "loose"]);
    let loose = ordered()
        .where_morphed_to("notable", MorphRef::null())
        .get(&t.db)
        .await
        .unwrap();
    assert_eq!(bodies(loose), ["loose"]);
}
