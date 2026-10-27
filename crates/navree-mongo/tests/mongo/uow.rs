//! Units of work on MongoDB: transactions that commit together or not at
//! all, change events for observers, after-commit hooks, the current unit
//! of work, retries after write conflicts, and joining navree-db's unit of
//! work.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use navree_db::{ChangeEvent, ChangeOp, Db, DbOptions};
use navree_kernel::{BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_mongo::{
    ModuleBuilderMongoExt, MongoChangeObserver, MongoDb, MongoError, MongoModule, MongoResult,
    MongoUow,
};
use navree_orm::{Model, Value, table};
use navree_testing::TestApp;
use uuid::Uuid;

use crate::models::Company;
use crate::support::TestMongo;

fn company(name: &str) -> Company {
    Company {
        name: name.into(),
        employees: 1,
        ..Company::default()
    }
}

#[tokio::test]
async fn units_of_work_commit_together_or_not_at_all() {
    let Some(t) = TestMongo::connect("units_of_work_commit_together_or_not_at_all").await else {
        return;
    };
    let mongo = &t.db;
    let actor = Uuid::now_v7();
    let uow = mongo
        .unit_of_work(Some(actor), Some("req-1".into()))
        .await
        .unwrap();
    assert_eq!((uow.actor(), uow.request_id()), (Some(actor), Some("req-1")));
    Company::create(&uow, company("Acme")).await.unwrap();
    let mut globex = Company::create(&uow, company("Globex")).await.unwrap();
    globex.employees = 250;
    globex.save(&uow).await.unwrap();
    assert_eq!(Company::query().count(&uow).await.unwrap(), 2, "the transaction sees its writes");
    assert_eq!(Company::query().count(mongo).await.unwrap(), 0, "nobody else does yet");

    let changes = uow.changes();
    let ops: Vec<(&str, ChangeOp)> = changes
        .iter()
        .map(|change| (change.entity.as_str(), change.op))
        .collect();
    assert_eq!(
        ops,
        [
            ("companies", ChangeOp::Created),
            ("companies", ChangeOp::Created),
            ("companies", ChangeOp::Updated)
        ]
    );
    assert_eq!(changes[2].id, globex.id.to_hex());

    let ran = Arc::new(AtomicBool::new(false));
    let hook = ran.clone();
    uow.after_commit(move || {
        Box::pin(async move {
            hook.store(true, Ordering::SeqCst);
        })
    });
    assert!(!ran.load(Ordering::SeqCst));
    uow.commit().await.unwrap();
    assert!(ran.load(Ordering::SeqCst), "hooks run after the commit");
    assert!(uow.is_finished());
    assert_eq!(Company::query().count(mongo).await.unwrap(), 2);
    uow.commit().await.unwrap();
    let error = Company::query().count(&uow).await.unwrap_err();
    assert!(error.to_string().contains("already committed"), "{error}");

    // Rolled back: nothing stays, no hook runs.
    let uow = mongo.begin().await.unwrap();
    Company::create(&uow, company("Initech")).await.unwrap();
    let ran = Arc::new(AtomicBool::new(false));
    let hook = ran.clone();
    uow.after_commit(move || {
        Box::pin(async move {
            hook.store(true, Ordering::SeqCst);
        })
    });
    uow.rollback().await.unwrap();
    assert!(uow.changes().is_empty());
    uow.commit().await.unwrap();
    assert!(!ran.load(Ordering::SeqCst));
    assert_eq!(Company::query().count(mongo).await.unwrap(), 2);
    t.cleanup().await;
}

#[tokio::test]
async fn the_connection_runs_in_the_current_unit_of_work() {
    let Some(t) = TestMongo::connect("the_connection_runs_in_the_current_unit_of_work").await else {
        return;
    };
    let mongo = &t.db;
    let uow = mongo.begin().await.unwrap();
    uow.scope(async {
        assert!(MongoUow::current().is_some_and(|current| current.same_as(&uow)));
        // A plain connection handle writes in the unit of work...
        Company::create(mongo, company("Acme")).await.unwrap();
        assert_eq!(Company::query().count(mongo).await.unwrap(), 1);
    })
    .await;
    // ...which outside it nobody sees until it commits.
    assert!(MongoUow::current().is_none());
    assert_eq!(Company::query().count(mongo).await.unwrap(), 0);
    assert_eq!(uow.changes().len(), 1);
    uow.commit().await.unwrap();
    assert_eq!(Company::query().count(mongo).await.unwrap(), 1);

    // Another connection does not join it.
    let other = MongoDb::connect(&t.options()).await.unwrap();
    let uow = mongo.begin().await.unwrap();
    uow.scope(async {
        Company::create(&other, company("Globex")).await.unwrap();
    })
    .await;
    uow.rollback().await.unwrap();
    assert_eq!(Company::query().count(mongo).await.unwrap(), 2);
    t.cleanup().await;
}

#[tokio::test]
async fn transactions_run_again_after_a_write_conflict() {
    let Some(t) = TestMongo::connect("transactions_run_again_after_a_write_conflict").await else {
        return;
    };
    let mongo = &t.db;
    let acme = Company::create(mongo, company("Acme")).await.unwrap().into_inner();
    let attempts = Arc::new(AtomicU32::new(0));
    let result = mongo
        .transaction(|uow: MongoUow| {
            let attempts = attempts.clone();
            let mongo = mongo.clone();
            async move {
                let attempt = attempts.fetch_add(1, Ordering::SeqCst) + 1;
                // The first read fixes the transaction's snapshot.
                let read = Company::find_or_fail(&uow, acme.id).await?;
                if attempt == 1 {
                    // Another transaction changes the document and commits.
                    let other = mongo.begin().await?;
                    Company::query()
                        .where_(Company::ID.eq(acme.id))
                        .increment(&other, Company::EMPLOYEES, 10)
                        .await?;
                    other.commit().await?;
                }
                Company::query()
                    .where_(Company::ID.eq(acme.id))
                    .update(&uow, [Company::EMPLOYEES.set(read.employees + 1)])
                    .await?;
                Ok(attempt)
            }
        })
        .await
        .unwrap();
    assert_eq!(result, 2, "the conflicting first attempt ran again");
    assert_eq!(Company::find_or_fail(mongo, acme.id).await.unwrap().employees, 12);

    // Other errors roll back and come back as they are.
    let error = mongo
        .transaction(|uow: MongoUow| async move {
            Company::create(&uow, company("Doomed")).await?;
            Err::<(), _>(MongoError::Custom("changed my mind".into()))
        })
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "changed my mind");
    assert_eq!(Company::query().where_(Company::NAME.eq("Doomed")).count(mongo).await.unwrap(), 0);
    t.cleanup().await;
}

/// Writes an audit document per change, in the transaction; refuses
/// companies named `Forbidden`.
struct AuditTrail;

impl Injectable for AuditTrail {
    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(AuditTrail) })
    }
}

impl MongoChangeObserver for AuditTrail {
    fn observe<'a>(&'a self, uow: &'a MongoUow, changes: &'a [ChangeEvent]) -> BoxFuture<'a, MongoResult> {
        Box::pin(async move {
            for change in changes {
                let name = change
                    .new
                    .as_ref()
                    .and_then(|new| new.get("name"))
                    .and_then(serde_json::Value::as_str);
                if name == Some("Forbidden") {
                    return Err(MongoError::Custom("companies cannot be called Forbidden".into()));
                }
                table("audit")
                    .insert(
                        uow,
                        [[
                            ("entity", Value::from(change.entity.clone())),
                            ("op", Value::from(change.op.as_str())),
                            ("key", Value::from(change.id.clone())),
                            ("actor", Value::from(uow.actor().map(|actor| actor.to_string()))),
                            ("request_id", Value::from(uow.request_id().map(str::to_owned))),
                        ]],
                    )
                    .await?;
            }
            Ok(())
        })
    }
}

struct AuditModule;

impl Module for AuditModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.mongo_change_observer::<AuditTrail>();
    }
}

#[tokio::test]
async fn observers_see_the_changes_inside_the_transaction() {
    let Some(t) = TestMongo::connect("observers_see_the_changes_inside_the_transaction").await else {
        return;
    };
    let app = TestApp::builder()
        .module(MongoModule::for_root(t.options()))
        .module(AuditModule)
        .build()
        .await;
    let mongo = app.get::<MongoDb>().await;
    let actor = Uuid::now_v7();
    let uow = mongo
        .unit_of_work(Some(actor), Some("req-9".into()))
        .await
        .unwrap();
    let acme = Company::create(&uow, company("Acme")).await.unwrap();
    uow.commit().await.unwrap();
    let audit = table("audit").get(&*mongo).await.unwrap();
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0].get::<String>("entity").unwrap(), "companies");
    assert_eq!(audit[0].get::<String>("op").unwrap(), "created");
    assert_eq!(audit[0].get::<String>("key").unwrap(), acme.id.to_hex());
    assert_eq!(audit[0].get::<String>("actor").unwrap(), actor.to_string());
    assert_eq!(audit[0].get::<String>("request_id").unwrap(), "req-9");

    // An observer's error rolls the whole unit of work back.
    let uow = mongo.begin().await.unwrap();
    Company::create(&uow, company("Globex")).await.unwrap();
    Company::create(&uow, company("Forbidden")).await.unwrap();
    let error = uow.commit().await.unwrap_err();
    assert_eq!(error.to_string(), "companies cannot be called Forbidden");
    assert!(uow.is_finished());
    assert_eq!(Company::query().count(&*mongo).await.unwrap(), 1);
    assert_eq!(table("audit").count(&*mongo).await.unwrap(), 1, "the audit rolled back too");
    t.cleanup().await;
}

#[tokio::test]
async fn a_unit_of_work_joins_the_databases_and_commits_after_it() {
    let Some(t) = TestMongo::connect("a_unit_of_work_joins_the_databases_and_commits_after_it").await
    else {
        return;
    };
    let mongo = &t.db;
    let sql = Db::connect(&DbOptions {
        url: "sqlite::memory:".into(),
        ..DbOptions::default()
    })
    .await
    .unwrap();
    let actor = Uuid::now_v7();
    let uow = sql.uow(Some(actor), Some("req-7".into())).await;
    let joined = mongo.begin_in(&uow).await.unwrap();
    assert_eq!((joined.actor(), joined.request_id()), (Some(actor), Some("req-7")));
    assert!(joined.joined().is_some_and(|sql_uow| sql_uow.same_as(&uow)));
    Company::create(&joined, company("Acme")).await.unwrap();
    // The change belongs to the database's unit of work, for its observers.
    let changes = uow.changes();
    assert_eq!(changes.len(), 1);
    assert_eq!((changes[0].entity.as_str(), changes[0].op), ("companies", ChangeOp::Created));
    assert_eq!(Company::query().count(mongo).await.unwrap(), 0);
    uow.commit().await.unwrap();
    assert!(joined.is_finished(), "MongoDB committed right after");
    assert_eq!(Company::query().count(mongo).await.unwrap(), 1);

    // When the database's unit of work rolls back, so does MongoDB's.
    let uow = sql.uow(None, None).await;
    let joined = mongo.begin_in(&uow).await.unwrap();
    Company::create(&joined, company("Globex")).await.unwrap();
    uow.rollback().await.unwrap();
    drop(joined);
    assert_eq!(Company::query().count(mongo).await.unwrap(), 1);
    t.cleanup().await;
}
