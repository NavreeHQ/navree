//! The CRM's models against a real app (the platform modules plus
//! `CrmCoreModule` on a temporary SQLite database): every column round
//! trips, relations eager load (deleted companies and contacts included
//! where a record keeps naming them), aggregates count what the API counts,
//! scopes search beyond ASCII, and the activity timeline is a time series.

use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
use crm_core::embeds::{CompanyRef, ContactRef, OwnerRef};
use crm_core::enums::{ActivityKind, CompanySize, ContactStatus, Stage, SubjectType};
use crm_core::factories::{ActivityFactory, ContactFactory, DealFactory};
use crm_core::models::{
    Activity, ActivityScopes, Company, CompanyScopes, Contact, ContactScopes, Deal, DealScopes,
    Role, User, UserScopes,
};
use crm_core::seed::ensure_demo_accounts;
use crm_core::{CrmCoreModule, platform};
use navree::access::Subject;
use navree::db::{Db, Migrator};
use navree::orm::{Model, Record};
use navree::prelude::*;
use navree::schema::Schema;
use navree::testing::{TempDirGuard, TestApp, temp_sqlite_url};
use serde_json::Value;
use uuid::Uuid;

struct Core {
    app: TestApp,
    db: Arc<Db>,
    /// The demo accounts: admin, manager, rep 1, rep 2.
    users: Vec<Uuid>,
    _dir: TempDirGuard,
}

async fn boot() -> Core {
    let (url, dir) = temp_sqlite_url();
    let sources = ConfigSources::new()
        .set("app.key", navree::crypto::generate_key())
        .set("database.url", url)
        .set("database.migrate_on_boot", true)
        .set("http.per_ip_rate_limit", Value::Null)
        .set("mail.transport", "array")
        .set("hashing.memory_kib", 1024)
        .set("hashing.iterations", 1)
        .set("observe.metrics", false)
        .set("log.level", "warn");
    let app = TestApp::from_builder(platform(Navree::app(), sources).module(CrmCoreModule)).await;
    let users = ensure_demo_accounts(app.ctx())
        .await
        .expect("the demo accounts")
        .iter()
        .map(|user| user.id)
        .collect();
    let db = app.get::<Db>().await;
    Core {
        app,
        db,
        users,
        _dir: dir,
    }
}

fn at(day: u32, hour: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, day, hour, 0, 0).unwrap()
}

async fn company(core: &Core, owner: Uuid, name: &str) -> Record<Company> {
    Company::factory()
        .set(Company::OWNER_ID, owner)
        .set(Company::NAME, name)
        .create_one(&*core.db)
        .await
        .expect("the company is created")
}

#[tokio::test]
async fn every_model_round_trips_its_columns() {
    let core = boot().await;
    let owner = core.users[2];
    let agra = Company::create(
        &*core.db,
        Company {
            name: "Agra".into(),
            domain: Some("agra.test".into()),
            industry: Some("Software".into()),
            size: Some(CompanySize::Medium),
            website: Some("https://agra.test".into()),
            city: Some("Ludhiana".into()),
            country: Some("India".into()),
            owner_id: owner,
            ..Company::default()
        },
    )
    .await
    .expect("company");
    let ada = Contact::factory()
        .set(Contact::OWNER_ID, owner)
        .set(Contact::FIRST_NAME, "Anu")
        .set(Contact::LAST_NAME, "Lakhotia")
        .set(Contact::STATUS, ContactStatus::Prospect)
        .state(ContactFactory::at(agra.id))
        .create_one(&*core.db)
        .await
        .expect("contact");
    let renewal = Deal::factory()
        .set(Deal::OWNER_ID, owner)
        .set(Deal::COMPANY_ID, agra.id)
        .set(Deal::CONTACT_ID, ada.id)
        .set(Deal::STAGE, Stage::Negotiation)
        .set(Deal::AMOUNT_CENTS, 1_250_000_i64)
        .set(
            Deal::EXPECTED_CLOSE_DATE,
            NaiveDate::from_ymd_opt(2026, 12, 1),
        )
        .create_one(&*core.db)
        .await
        .expect("deal");
    let call = Activity::factory()
        .set(Activity::OWNER_ID, owner)
        .set(Activity::ASSIGNEE_ID, owner)
        .set(Activity::BODY, "Include the SLA")
        .state(ActivityFactory::on(SubjectType::Deal, renewal.id))
        .state(ActivityFactory::task_due(at(26, 9)))
        .create_one(&*core.db)
        .await
        .expect("activity");

    let found = Company::find(&*core.db, agra.id).await.unwrap().unwrap();
    assert_eq!(found.model(), agra.model());
    let found = Contact::find(&*core.db, ada.id).await.unwrap().unwrap();
    assert_eq!(found.model(), ada.model());
    assert_eq!(found.full_name(), "Anu Lakhotia");
    let found = Deal::find(&*core.db, renewal.id).await.unwrap().unwrap();
    assert_eq!(found.model(), renewal.model());
    assert_eq!((found.stage, found.probability()), (Stage::Negotiation, 75));
    let found = Activity::find(&*core.db, call.id).await.unwrap().unwrap();
    assert_eq!(found.model(), call.model());
    assert_eq!(found.kind, ActivityKind::Task);

    // Enums are stored as the API strings, which ability conditions see.
    let record = navree::orm::access::record_json(&renewal);
    assert_eq!(record["stage"], "negotiation");
    assert_eq!(record["owner_id"], owner.to_string());
    core.app.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn relations_load_with_the_records_that_name_them() {
    let core = boot().await;
    let (rep1, rep2) = (core.users[2], core.users[3]);
    let agra = company(&core, rep1, "Agra").await;
    let ada = Contact::factory()
        .set(Contact::OWNER_ID, rep2)
        .set(Contact::FIRST_NAME, "Anu")
        .set(Contact::LAST_NAME, "Lakhotia")
        .state(ContactFactory::at(agra.id))
        .create_one(&*core.db)
        .await
        .unwrap();
    let deal = Deal::factory()
        .set(Deal::OWNER_ID, rep1)
        .set(Deal::COMPANY_ID, agra.id)
        .set(Deal::CONTACT_ID, ada.id)
        .create_one(&*core.db)
        .await
        .unwrap();
    for (subject_type, id) in [
        (SubjectType::Company, agra.id),
        (SubjectType::Contact, ada.id),
        (SubjectType::Deal, deal.id),
    ] {
        Activity::factory()
            .set(Activity::OWNER_ID, rep2)
            .state(ActivityFactory::on(subject_type, id))
            .create_one(&*core.db)
            .await
            .unwrap();
    }
    // A deleted company and contact keep being named by what points at
    // them.
    let mut gone = Company::find(&*core.db, agra.id).await.unwrap().unwrap();
    gone.delete(&*core.db).await.unwrap();
    let mut gone = Contact::find(&*core.db, ada.id).await.unwrap().unwrap();
    gone.delete(&*core.db).await.unwrap();

    let deals = Deal::query()
        .with_all(["company", "contact", "owner"])
        .get(&*core.db)
        .await
        .unwrap();
    let loaded = &deals[0];
    let company = loaded.loaded(Deal::company).and_then(Option::as_ref);
    assert_eq!(company.map(|c| c.name.as_str()), Some("Agra"));
    let contact = loaded.loaded(Deal::contact).and_then(Option::as_ref);
    assert_eq!(contact.map(|c| c.full_name()), Some("Anu Lakhotia".into()));
    let owner = loaded.loaded(Deal::owner).and_then(Option::as_ref);
    assert_eq!(owner.map(|u| u.id), Some(rep1));

    // The API's embeds come from the loaded relations.
    assert_eq!(
        CompanyRef::loaded(loaded.loaded(Deal::company), loaded.company_id),
        CompanyRef {
            id: agra.id,
            name: "Agra".into()
        }
    );
    assert_eq!(
        contact.map(ContactRef::from),
        Some(ContactRef {
            id: ada.id,
            full_name: "Anu Lakhotia".into()
        })
    );
    assert_eq!(
        OwnerRef::loaded(loaded.loaded(Deal::owner), loaded.owner_id),
        OwnerRef {
            id: rep1,
            name: "Priya Verma".into()
        }
    );
    // Without its relation loaded, an embed keeps the id and has no name.
    let bare = Deal::find(&*core.db, deal.id).await.unwrap().unwrap();
    assert_eq!(
        OwnerRef::loaded(bare.loaded(Deal::owner), bare.owner_id),
        OwnerRef {
            id: rep1,
            name: String::new()
        }
    );

    let activities = Activity::query()
        .with_all(["subject_record", "owner", "assignee"])
        .get(&*core.db)
        .await
        .unwrap();
    assert_eq!(activities.len(), 3);
    for activity in activities.iter() {
        let subject = activity
            .loaded(Activity::subject_record)
            .and_then(Option::as_ref)
            .expect("the subject is loaded, deleted or not");
        let expected = match activity.subject_type {
            SubjectType::Company => "Company",
            SubjectType::Contact => "Contact",
            SubjectType::Deal => "Deal",
        };
        assert_eq!(subject.model_name(), expected);
        assert_eq!(subject.key_string(), activity.subject_id.to_string());
        let owner = activity.loaded(Activity::owner).and_then(Option::as_ref);
        assert_eq!(owner.map(|user| user.id), Some(rep2));
    }
    assert!(activities.iter().all(|activity| {
        activity
            .loaded(Activity::assignee)
            .is_some_and(Option::is_none)
    }));
    core.app.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn aggregates_count_live_contacts_and_open_deals() {
    let core = boot().await;
    let rep = core.users[2];
    let agra = company(&core, rep, "Agra").await;
    let contacts = Contact::factory()
        .count(3)
        .set(Contact::OWNER_ID, rep)
        .state(ContactFactory::at(agra.id))
        .create(&*core.db)
        .await
        .unwrap();
    let mut gone = contacts[0].clone();
    gone.delete(&*core.db).await.unwrap();
    let deal = || {
        Deal::factory()
            .set(Deal::OWNER_ID, rep)
            .set(Deal::COMPANY_ID, agra.id)
    };
    deal()
        .set(Deal::AMOUNT_CENTS, 1_000_i64)
        .create_one(&*core.db)
        .await
        .unwrap();
    deal()
        .set(Deal::AMOUNT_CENTS, 2_500_i64)
        .set(Deal::STAGE, Stage::Proposal)
        .create_one(&*core.db)
        .await
        .unwrap();
    deal()
        .set(Deal::AMOUNT_CENTS, 99_000_i64)
        .state(DealFactory::won_at(at(20, 12)))
        .create_one(&*core.db)
        .await
        .unwrap();

    let mut found = Company::find(&*core.db, agra.id).await.unwrap().unwrap();
    found
        .load_count(&*core.db, ["contacts", "open_deals"])
        .await
        .unwrap();
    found
        .load_sum(&*core.db, "open_deals", Deal::AMOUNT_CENTS)
        .await
        .unwrap();
    assert_eq!(found.extra::<i64>("contacts_count").unwrap(), Some(2));
    assert_eq!(found.extra::<i64>("open_deals_count").unwrap(), Some(2));
    assert_eq!(
        found.extra::<i64>("open_deals_sum_amount_cents").unwrap(),
        Some(3_500)
    );
    core.app.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn scopes_search_beyond_ascii_and_name_the_crm_queries() {
    let core = boot().await;
    let (rep1, rep2) = (core.users[2], core.users[3]);
    let lyon = company(&core, rep1, "Équipe Lyon").await;
    company(&core, rep2, "Ölwechsel GmbH").await;
    let names = |companies: navree::orm::Collection<Company>| -> Vec<String> {
        companies.iter().map(|c| c.name.clone()).collect()
    };
    for term in ["équipe", "ÉQUIPE", "lyon"] {
        let found = Company::query().search(term).get(&*core.db).await.unwrap();
        assert_eq!(names(found), ["Équipe Lyon"], "{term}");
    }
    let found = Company::query()
        .owned_by(rep2)
        .get(&*core.db)
        .await
        .unwrap();
    assert_eq!(names(found), ["Ölwechsel GmbH"]);

    Contact::factory()
        .set(Contact::OWNER_ID, rep1)
        .set(Contact::FIRST_NAME, "Élodie")
        .set(Contact::LAST_NAME, "Åberg")
        .create_one(&*core.db)
        .await
        .unwrap();
    let found = Contact::query()
        .search("élodie ÅBERG")
        .count(&*core.db)
        .await
        .unwrap();
    assert_eq!(found, 1);

    let deal = |stage: Stage| {
        Deal::factory()
            .set(Deal::OWNER_ID, rep1)
            .set(Deal::COMPANY_ID, lyon.id)
            .set(Deal::STAGE, stage)
    };
    deal(Stage::Lead).create_one(&*core.db).await.unwrap();
    deal(Stage::Negotiation)
        .create_one(&*core.db)
        .await
        .unwrap();
    deal(Stage::Won)
        .state(DealFactory::won_at(at(5, 12)))
        .create_one(&*core.db)
        .await
        .unwrap();
    deal(Stage::Lost)
        .state(DealFactory::lost_at(at(6, 12), "No budget"))
        .create_one(&*core.db)
        .await
        .unwrap();
    assert_eq!(Deal::query().open().count(&*core.db).await.unwrap(), 2);
    assert_eq!(
        Deal::query()
            .won_between(at(1, 0), at(6, 0))
            .count(&*core.db)
            .await
            .unwrap(),
        1
    );

    let task = |assignee: Option<Uuid>, owner: Uuid| {
        Activity::factory()
            .set(Activity::OWNER_ID, owner)
            .set(Activity::ASSIGNEE_ID, assignee)
            .state(ActivityFactory::on(SubjectType::Company, lyon.id))
            .state(ActivityFactory::task_due(at(27, 9)))
    };
    task(Some(rep1), rep2).create_one(&*core.db).await.unwrap();
    task(None, rep1).create_one(&*core.db).await.unwrap();
    task(Some(rep2), rep1).create_one(&*core.db).await.unwrap();
    let mut done = task(Some(rep1), rep1).create_one(&*core.db).await.unwrap();
    done.update(&*core.db, |task| task.completed_at = Some(at(27, 10)))
        .await
        .unwrap();
    assert_eq!(
        Activity::query()
            .open_tasks()
            .for_user(rep1)
            .count(&*core.db)
            .await
            .unwrap(),
        2,
        "assigned to rep 1, or theirs and unassigned"
    );
    assert_eq!(
        Activity::query()
            .attached_to(SubjectType::Company, lyon.id)
            .count(&*core.db)
            .await
            .unwrap(),
        4
    );
    core.app.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn users_load_their_roles_and_search_beyond_ascii() {
    let core = boot().await;
    let users = User::query()
        .with("roles")
        .order_by(User::EMAIL)
        .get(&*core.db)
        .await
        .unwrap();
    let roles: Vec<(String, Vec<String>)> = users
        .iter()
        .map(|user| {
            let roles = user
                .loaded(User::roles)
                .map(|roles| roles.iter().map(|role| role.name.clone()).collect())
                .unwrap_or_default();
            (user.email.clone(), roles)
        })
        .collect();
    assert_eq!(
        roles,
        [
            ("admin@navree.test".to_owned(), vec!["admin".to_owned()]),
            ("manager@navree.test".to_owned(), vec!["manager".to_owned()]),
            ("rep1@navree.test".to_owned(), vec!["rep".to_owned()]),
            ("rep2@navree.test".to_owned(), vec!["rep".to_owned()]),
        ]
    );
    let found = User::query().search("PRIYA").get(&*core.db).await.unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].email, "rep1@navree.test");
    assert_eq!(<Role as Model>::table_name(), "roles");
    core.app.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn models_are_the_access_subjects_and_name_their_morphs() {
    fn subject<S: Subject>() -> (&'static str, &'static str) {
        (S::SUBJECT, S::PERMISSION_PREFIX)
    }
    for (subject, expected) in [
        (subject::<Company>(), ("Company", "companies")),
        (subject::<Contact>(), ("Contact", "contacts")),
        (subject::<Deal>(), ("Deal", "deals")),
        // `Activity::SUBJECT` is the `subject` column's constant.
        (subject::<Activity>(), ("Activity", "activities")),
        (subject::<User>(), ("User", "users")),
    ] {
        assert_eq!(subject, expected);
    }
    assert_eq!(Company::morph_name(), SubjectType::Company.as_str());
    assert_eq!(Contact::morph_name(), SubjectType::Contact.as_str());
    assert_eq!(Deal::morph_name(), SubjectType::Deal.as_str());
}

#[tokio::test]
async fn the_activity_timeline_is_a_time_series() {
    let core = boot().await;
    let status = Migrator::status(core.app.ctx()).await.expect("status");
    let crm: Vec<(&str, bool)> = status
        .iter()
        .filter(|migration| migration.module == "crm")
        .map(|migration| (migration.name.as_str(), migration.applied))
        .collect();
    assert_eq!(
        crm,
        [
            ("m20261025_000001_create_crm_tables", true),
            ("m20261027_000001_make_activities_a_time_series", true),
        ]
    );
    // SQLite has no TimescaleDB: a regular table with its time index.
    let schema = Schema::new(&core.db);
    assert!(!schema.is_hypertable("activities").await.unwrap());
    let indexes = schema.indexes("activities").await.unwrap();
    assert!(
        indexes
            .iter()
            .any(|index| index.columns == ["created_at"] && !index.unique),
        "{indexes:#?}"
    );

    // Activities still come back newest first.
    let rep = core.users[2];
    let agra = company(&core, rep, "Agra").await;
    for hours in [3, 1, 2] {
        Activity::factory()
            .set(Activity::OWNER_ID, rep)
            .set(Activity::CREATED_AT, at(20, 9) + Duration::hours(hours))
            .state(ActivityFactory::on(SubjectType::Company, agra.id))
            .create_one(&*core.db)
            .await
            .unwrap();
    }
    let created: Vec<DateTime<Utc>> = Activity::query()
        .newest_first()
        .pluck(&*core.db, Activity::CREATED_AT)
        .await
        .unwrap();
    assert_eq!(created, [at(20, 12), at(20, 11), at(20, 10)]);
    core.app.shutdown().await.expect("shutdown");
}
