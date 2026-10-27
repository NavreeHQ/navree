//! Factories through an executor: stored records, parents, children,
//! attachments, recycling, lazy parents, hooks, quiet creates, bulk
//! inserts and named connections.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use navree_db::{Db, DbOptions};
use navree_kernel::{BoxFuture, Factory as Provider, Module, ModuleBuilder};
use navree_orm::factory::{BelongsTo, FactoryOptions, HasMany, Pivot};
use navree_orm::hooks::{Flow, ModelEvent, ModelHook};
use navree_orm::{ExecContext, Model, OrmError, Record, Value, col, table};
use uuid::Uuid;

use crate::models::{Activity, Company, Deal, DealFactory, Note, Stage, Tag};
use crate::support::{app, count_rows, create_tables, epoch, sqlite};

#[tokio::test]
async fn create_stores_records_with_keys_timestamps_and_change_events() {
    let (_app, db, _clock) = app(Vec::new()).await;
    let uow = db.uow(None, None).await;
    let companies = Company::factory().count(3).create(&uow).await.unwrap();
    assert_eq!(companies.len(), 3);
    assert!(companies.iter().all(|c| c.exists() && c.id > 0));
    assert!(
        companies.iter().all(|c| c.created_at == epoch()),
        "the app's clock"
    );
    assert_eq!(uow.changes().len(), 3, "audited like any save");
    uow.commit().await.unwrap();
    assert_eq!(count_rows(&db, "companies").await, 3);

    let note = Note::factory().create_one(&*db).await.unwrap();
    assert!(!note.id.is_nil(), "a ULID was generated");
    let notes = Note::factory().create_many(&*db, 2).await.unwrap();
    assert_eq!(notes.len(), 2);
    assert_eq!(count_rows(&db, "notes").await, 3);
}

#[tokio::test]
async fn definition_parents_are_created_one_per_record() {
    let test = sqlite().await;
    let deals = Deal::factory().count(3).create(&test.db).await.unwrap();
    assert_eq!(count_rows(&test.db, "companies").await, 3);
    let mut companies: Vec<i64> = deals.iter().map(|d| d.company_id.unwrap()).collect();
    companies.dedup();
    assert_eq!(companies.len(), 3, "each deal has its own company");
}

#[tokio::test]
async fn states_and_sets_override_a_definitions_parent() {
    let test = sqlite().await;
    let acme = Company::factory().create_one(&test.db).await.unwrap();
    let deals = Deal::factory()
        .count(2)
        .set(Deal::COMPANY_ID, acme.id)
        .create(&test.db)
        .await
        .unwrap();
    assert!(deals.iter().all(|d| d.company_id == Some(acme.id)));
    assert_eq!(
        count_rows(&test.db, "companies").await,
        1,
        "no parent created"
    );
}

#[tokio::test]
async fn for_shares_one_parent_across_the_batch() {
    let test = sqlite().await;
    let deals = Deal::factory()
        .count(3)
        .for_(
            Company::factory().set(Company::NAME, "Acme"),
            Deal::COMPANY_ID,
        )
        .create(&test.db)
        .await
        .unwrap();
    assert_eq!(count_rows(&test.db, "companies").await, 1);
    let company = Company::find_or_fail(&test.db, deals[0].company_id.unwrap())
        .await
        .unwrap();
    assert_eq!(company.name, "Acme");
    assert!(deals.iter().all(|d| d.company_id == Some(company.id)));

    // A stored parent, by typed column or by the factory's name for it.
    let more = Deal::factory()
        .count(2)
        .for_record(&company, "company")
        .create(&test.db)
        .await
        .unwrap();
    assert!(more.iter().all(|d| d.company_id == Some(company.id)));
    assert_eq!(count_rows(&test.db, "companies").await, 1);
}

#[tokio::test]
async fn lazy_parents_are_created_once_and_shared() {
    let test = sqlite().await;
    let shared = Company::factory().set(Company::NAME, "Shared").lazy();
    assert!(shared.created().is_none());
    Deal::factory()
        .for_(shared.clone(), Deal::COMPANY_ID)
        .create(&test.db)
        .await
        .unwrap();
    Deal::factory()
        .count(2)
        .for_(shared.clone(), Deal::COMPANY_ID)
        .create(&test.db)
        .await
        .unwrap();
    assert_eq!(count_rows(&test.db, "companies").await, 1);
    let company = shared.get(&test.db).await.unwrap();
    assert_eq!(company.name, "Shared");
    assert_eq!(
        Deal::query()
            .where_(Deal::COMPANY_ID.eq(company.id))
            .count(&test.db)
            .await
            .unwrap(),
        3
    );
}

#[tokio::test]
async fn parents_can_be_turned_off() {
    let test = sqlite().await;
    let orphans = Deal::factory()
        .count(2)
        .without_parents()
        .create(&test.db)
        .await
        .unwrap();
    assert!(orphans.iter().all(|d| d.company_id.is_none()));
    let excluded = Deal::factory()
        .without_parent(Deal::COMPANY_ID)
        .create_one(&test.db)
        .await
        .unwrap();
    assert!(excluded.company_id.is_none());
    assert_eq!(count_rows(&test.db, "companies").await, 0);
}

/// Registers app-wide factory options.
struct NoParents;

impl Module for NoParents {
    fn register(&self, m: &mut ModuleBuilder) {
        m.global()
            .value(FactoryOptions::new().expand_relationships(false).seed(7))
            .exports::<FactoryOptions>();
    }
}

#[tokio::test]
async fn the_apps_factory_options_apply_to_its_executors() {
    let (_app, db, _clock) = app(vec![Box::new(NoParents)]).await;
    let deal = Deal::factory().create_one(&*db).await.unwrap();
    assert!(deal.company_id.is_none(), "expand_relationships = false");
    let first = Company::factory().create_one(&*db).await.unwrap();
    let second = Company::factory().create_one(&*db).await.unwrap();
    assert_eq!(first.name, second.name, "the app's seed");
    let forced = Company::factory().seed(99).create_one(&*db).await.unwrap();
    assert_ne!(forced.name, first.name, "a builder's own seed wins");
}

#[tokio::test]
async fn recycled_records_replace_new_parents_everywhere() {
    let test = sqlite().await;
    let companies = Company::factory().count(3).create(&test.db).await.unwrap();
    let deals = Deal::factory()
        .count(30)
        .seed(1)
        .recycle(&companies)
        .create(&test.db)
        .await
        .unwrap();
    assert_eq!(count_rows(&test.db, "companies").await, 3, "no new company");
    let mut used: Vec<i64> = deals.iter().filter_map(|d| d.company_id).collect();
    used.sort_unstable();
    used.dedup();
    assert!(used.len() > 1, "a random pick per record: {used:?}");
    assert!(used.iter().all(|id| companies.contains(*id)));

    // Nested factories inherit the pool: the deals a tag gets attached to
    // reuse the companies too.
    Tag::factory()
        .recycle(&companies)
        .has_attached(
            Deal::factory().count(2),
            [],
            Pivot::<Tag, Deal>::new("deal_tag", "tag_id", "deal_id"),
        )
        .create(&test.db)
        .await
        .unwrap();
    assert_eq!(count_rows(&test.db, "deals").await, 32);
    assert_eq!(
        count_rows(&test.db, "companies").await,
        3,
        "the nested deals reused the recycled companies"
    );
}

#[tokio::test]
async fn has_creates_children_for_each_record() {
    let test = sqlite().await;
    let deals = Deal::factory()
        .count(2)
        .has(
            crate::models::Activity::factory().count(3),
            Activity::DEAL_ID,
        )
        .create(&test.db)
        .await
        .unwrap();
    for deal in &deals {
        let activities = Activity::query()
            .where_(Activity::DEAL_ID.eq(deal.id))
            .count(&test.db)
            .await
            .unwrap();
        assert_eq!(activities, 3);
    }
    // By the name the factory declares.
    let named = Deal::factory()
        .has(Activity::factory().count(2), "activities")
        .create_one(&test.db)
        .await
        .unwrap();
    assert_eq!(
        Activity::query()
            .where_(Activity::DEAL_ID.eq(named.id))
            .count(&test.db)
            .await
            .unwrap(),
        2
    );
}

#[tokio::test]
async fn polymorphic_children_and_parents_get_their_type() {
    let test = sqlite().await;
    let deal = Deal::factory()
        .has(
            Activity::factory().count(2),
            HasMany::morph(Activity::SUBJECT_TYPE, Activity::SUBJECT_ID, "deal"),
        )
        .create_one(&test.db)
        .await
        .unwrap();
    let activities = Activity::query()
        .where_(Activity::SUBJECT_ID.eq(deal.id))
        .get(&test.db)
        .await
        .unwrap();
    assert_eq!(activities.len(), 2);
    assert!(
        activities
            .iter()
            .all(|a| a.subject_type.as_deref() == Some("deal"))
    );

    let on_deal = Activity::factory()
        .for_(
            &deal,
            BelongsTo::morph(Activity::SUBJECT_TYPE, Activity::SUBJECT_ID, "deal"),
        )
        .create_one(&test.db)
        .await
        .unwrap();
    assert_eq!(on_deal.subject_type.as_deref(), Some("deal"));
    assert_eq!(on_deal.subject_id, Some(deal.id));
}

#[tokio::test]
async fn has_attached_links_records_through_the_pivot() {
    let (_app, db, _clock) = app(Vec::new()).await;
    let vip = Tag::factory()
        .set(Tag::NAME, "vip")
        .create_one(&*db)
        .await
        .unwrap();
    let deals = Deal::factory()
        .count(2)
        .has_attached(
            Tag::factory().count(2),
            [("source", Value::from("import"))],
            "tags",
        )
        .has_attached(
            &vip,
            [],
            Pivot::<Deal, Tag>::new("deal_tag", "deal_id", "tag_id"),
        )
        .create(&*db)
        .await
        .unwrap();
    assert_eq!(
        count_rows(&db, "tags").await,
        5,
        "two new tags per deal, plus vip"
    );
    for deal in &deals {
        let rows = table("deal_tag")
            .where_(col("deal_id").eq(deal.id))
            .order_by("tag_id")
            .get(&*db)
            .await
            .unwrap();
        assert_eq!(rows.len(), 3);
        let sources: Vec<Option<String>> = rows
            .iter()
            .map(|row| row.get::<Option<String>>("source").unwrap())
            .collect();
        assert_eq!(
            sources,
            [None, Some("import".to_owned()), Some("import".to_owned())],
            "vip (the oldest tag) has no pivot values"
        );
        let stamped = rows
            .iter()
            .filter(|row| row.get::<Option<String>>("created_at").unwrap().is_some())
            .count();
        assert_eq!(stamped, 2, "the named pivot has timestamps, the other not");
    }
}

#[tokio::test]
async fn after_creating_hooks_run_with_the_executor() {
    let (_app, db, _clock) = app(Vec::new()).await;
    let uow = db.uow(None, None).await;
    let deals = Deal::factory()
        .count(2)
        .after_creating(|deal, exec| {
            Box::pin(async move {
                assert!(deal.exists());
                Activity::factory()
                    .set(Activity::DEAL_ID, deal.id)
                    .create(exec)
                    .await?;
                Ok(())
            })
        })
        .create(&uow)
        .await
        .unwrap();
    assert_eq!(deals.len(), 2);
    // Everything ran in the unit of work: rolling it back removes it all.
    uow.rollback().await.unwrap();
    assert_eq!(count_rows(&db, "activities").await, 0);
    assert_eq!(count_rows(&db, "deals").await, 0);
    assert_eq!(count_rows(&db, "companies").await, 0);

    let removed = Deal::factory()
        .after_creating(|_, _| Box::pin(async { Err(OrmError::InvalidQuery("no".into())) }))
        .without_after_creating()
        .create(&*db)
        .await;
    assert!(removed.is_ok());
}

/// Counts the `created` events of deals it sees.
struct CountCreated(Arc<AtomicUsize>);

impl ModelHook<Deal> for CountCreated {
    fn after<'a>(
        &'a self,
        event: ModelEvent,
        _ctx: &'a ExecContext,
        _record: &'a mut Record<Deal>,
    ) -> BoxFuture<'a, navree_orm::Result<()>> {
        Box::pin(async move {
            if event == ModelEvent::Created {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
            Ok(())
        })
    }
}

/// Registers [`CountCreated`].
struct Observed(Arc<AtomicUsize>);

impl Module for Observed {
    fn register(&self, m: &mut ModuleBuilder) {
        let counter = self.0.clone();
        m.multi(Provider::<dyn ModelHook<Deal>>::new(move |_| {
            let counter = counter.clone();
            async move { Ok(Arc::new(CountCreated(counter)) as Arc<dyn ModelHook<Deal>>) }
        }));
    }
}

#[tokio::test]
async fn quiet_creates_skip_the_apps_model_hooks_but_keep_the_audit_trail() {
    let created = Arc::new(AtomicUsize::new(0));
    let (_app, db, _clock) = app(vec![Box::new(Observed(created.clone()))]).await;
    Deal::factory().count(2).create(&*db).await.unwrap();
    assert_eq!(created.load(Ordering::SeqCst), 2);

    let uow = db.uow(None, None).await;
    Deal::factory().count(3).create_quietly(&uow).await.unwrap();
    Deal::factory().create_one_quietly(&uow).await.unwrap();
    Deal::factory().create_many_quietly(&uow, 2).await.unwrap();
    assert_eq!(created.load(Ordering::SeqCst), 2, "no observer ran");
    assert!(
        uow.changes().iter().filter(|c| c.entity == "deals").count() == 6,
        "change events are still recorded"
    );
    uow.commit().await.unwrap();
    assert_eq!(count_rows(&db, "deals").await, 8);
}

#[tokio::test]
async fn insert_writes_one_statement_without_events_or_relationships() {
    let created = Arc::new(AtomicUsize::new(0));
    let (_app, db, _clock) = app(vec![Box::new(Observed(created.clone()))]).await;
    let uow = db.uow(None, None).await;
    let inserted = Deal::factory()
        .count(4)
        .state(DealFactory::won)
        .insert(&uow)
        .await
        .unwrap();
    assert_eq!(inserted, 4);
    assert!(uow.changes().is_empty(), "no change events");
    uow.commit().await.unwrap();
    assert_eq!(created.load(Ordering::SeqCst), 0, "no model events");
    assert_eq!(count_rows(&db, "companies").await, 0, "no parents");
    let deals = Deal::all(&*db).await.unwrap();
    assert_eq!(deals.len(), 4);
    assert!(
        deals
            .iter()
            .all(|d| d.stage == Stage::Won && !d.id.is_nil())
    );
    assert!(deals.iter().all(|d| d.created_at == epoch()));

    // Incrementing keys are left to the database.
    assert_eq!(Company::factory().count(3).insert(&*db).await.unwrap(), 3);
    let ids = Company::query().pluck(&*db, Company::ID).await.unwrap();
    assert_eq!(ids, [1, 2, 3]);
    assert_eq!(Company::factory().count(0).insert(&*db).await.unwrap(), 0);
}

/// A second database, registered under the name `archive`.
struct Archive(String);

impl Module for Archive {
    fn register(&self, m: &mut ModuleBuilder) {
        let url = self.0.clone();
        m.factory(
            Provider::<Db>::new(move |_| {
                let url = url.clone();
                async move {
                    let db = Db::connect(&DbOptions {
                        url,
                        ..DbOptions::default()
                    })
                    .await?;
                    create_tables(&db).await;
                    Ok(Arc::new(db))
                }
            })
            .named("archive"),
        );
    }
}

#[tokio::test]
async fn connection_creates_on_a_named_database() {
    let archive = tempfile::tempdir().unwrap();
    let url = format!(
        "sqlite://{}?mode=rwc",
        archive.path().join("archive.db").display()
    );
    let (app, db, _clock) = app(vec![Box::new(Archive(url))]).await;
    Company::factory()
        .count(2)
        .connection("archive")
        .create(&*db)
        .await
        .unwrap();
    assert_eq!(count_rows(&db, "companies").await, 0);
    let named = app.ctx().get_named::<Db>("archive").await.unwrap();
    assert_eq!(count_rows(&named, "companies").await, 2);

    let missing = Company::factory()
        .connection("nowhere")
        .create(&*db)
        .await
        .unwrap_err();
    assert!(
        missing
            .to_string()
            .contains("no database connection named `nowhere`"),
        "{missing}"
    );
}

/// Refuses to save deals titled "Refused".
struct RefuseTitle;

impl ModelHook<Deal> for RefuseTitle {
    fn before<'a>(
        &'a self,
        event: ModelEvent,
        _ctx: &'a ExecContext,
        deal: &'a mut Record<Deal>,
    ) -> BoxFuture<'a, navree_orm::Result<Flow>> {
        Box::pin(async move {
            Ok(if event == ModelEvent::Saving && deal.title == "Refused" {
                Flow::Cancel
            } else {
                Flow::Continue
            })
        })
    }
}

struct Refusing;

impl Module for Refusing {
    fn register(&self, m: &mut ModuleBuilder) {
        m.multi(Provider::<dyn ModelHook<Deal>>::new(|_| async {
            Ok(Arc::new(RefuseTitle) as Arc<dyn ModelHook<Deal>>)
        }));
    }
}

#[tokio::test]
async fn a_cancelled_save_fails_the_factory() {
    let (_app, db, _clock) = app(vec![Box::new(Refusing)]).await;
    let error = Deal::factory()
        .set(Deal::TITLE, "Refused")
        .create(&*db)
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "the Deal factory failed: a model hook cancelled saving a factory record"
    );
}

#[tokio::test]
async fn named_parents_of_the_wrong_model_fail_before_writing() {
    let test = sqlite().await;
    let error = Deal::factory()
        .for_(crate::models::Tag::factory(), "company")
        .create(&test.db)
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .ends_with("`company` of Deal links Company records, not Tag"),
        "{error}"
    );
    assert_eq!(count_rows(&test.db, "deals").await, 0);
    let _: Uuid = Uuid::nil();
}
