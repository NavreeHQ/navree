//! `NotifyModule` wiring: configuration, global export, the job and the table.

use std::sync::Arc;

use navis_contracts::JobRegistration;
use navis_db::{DatabaseModule, Db};
use navis_kernel::{Factory, Module, ModuleBuilder, ProviderKey};
use navis_notify::{NOTIFICATIONS_TABLE, Notifier, NotifyModule, NotifyOptions};
use navis_testing::TestApp;
use sea_orm::{ConnectionTrait, Statement};

/// A module that injects the notifier without importing `NotifyModule`.
struct DealsModule;

/// Holds the injected notifier.
struct DealService(#[allow(dead_code)] Arc<Notifier>);

impl Module for DealsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.factory(
            Factory::<DealService>::new(|r| async move {
                Ok(Arc::new(DealService(r.get::<Notifier>().await?)))
            })
            .deps(vec![ProviderKey::of::<Notifier>()]),
        );
    }
}

#[tokio::test]
async fn from_config_reads_the_notify_section() {
    let app = TestApp::builder()
        .temp_database()
        .config("notify.queue", true)
        .config("notify.routes", false)
        .module(DatabaseModule)
        .module(NotifyModule::from_config())
        .build()
        .await;

    assert_eq!(
        app.get::<Notifier>().await.options(),
        &NotifyOptions {
            queue: true,
            routes: false,
        }
    );
}

#[tokio::test]
async fn a_plain_module_uses_the_defaults() {
    let app = TestApp::builder()
        .temp_database()
        .module(DatabaseModule)
        .module(NotifyModule)
        .build()
        .await;

    assert_eq!(
        app.get::<Notifier>().await.options(),
        &NotifyOptions::default()
    );
}

#[tokio::test]
async fn the_notifier_is_exported_globally() {
    let app = TestApp::builder()
        .temp_database()
        .module(DatabaseModule)
        .module(NotifyModule::for_root(NotifyOptions::default()))
        .module(DealsModule)
        .build()
        .await;

    app.get::<DealService>().await;
}

#[tokio::test]
async fn the_module_registers_its_job_and_migrates_its_table() {
    let app = TestApp::builder()
        .temp_database()
        .module(DatabaseModule)
        .module(NotifyModule)
        .build()
        .await;

    let jobs: Vec<&str> = app
        .ctx()
        .contributions::<JobRegistration>()
        .iter()
        .map(|c| c.value.name)
        .collect();
    assert!(jobs.contains(&"notify.send"), "{jobs:?}");
    let db = app.get::<Db>().await;
    let table = db
        .conn()
        .query_one_raw(Statement::from_string(
            db.backend(),
            format!("SELECT name FROM sqlite_master WHERE type = 'table' AND name = '{NOTIFICATIONS_TABLE}'"),
        ))
        .await
        .unwrap();
    assert!(table.is_some(), "the notifications table exists");
}
