//! `NotifyModule` wiring: configuration, global export, the job and the table.

use std::sync::Arc;

use navree_contracts::JobRegistration;
use navree_db::{DatabaseModule, Db};
use navree_kernel::{Factory, Module, ModuleBuilder, ProviderKey};
use navree_notify::{
    DELIVERIES_TABLE, DeliveryLog, NOTIFICATIONS_TABLE, Notifier, NotifyModule, NotifyOptions,
    OPT_OUTS_TABLE, OptOuts, PUSH_SUBSCRIPTIONS_TABLE, PushSubscriptions,
};
use navree_testing::TestApp;
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
            ..NotifyOptions::default()
        }
    );
}

#[tokio::test]
async fn the_channel_services_are_exported_globally() {
    let app = TestApp::builder()
        .temp_database()
        .module(DatabaseModule)
        .module(NotifyModule)
        .build()
        .await;

    app.get::<DeliveryLog>().await;
    app.get::<OptOuts>().await;
    app.get::<PushSubscriptions>().await;
    let command = app
        .ctx()
        .commands()
        .into_iter()
        .find(|c| c.name == "notify:vapid")
        .expect("the module registers `notify:vapid`");
    assert!(command.about.contains("VAPID"), "{}", command.about);
    app.run_command("notify:vapid", &[])
        .await
        .expect("notify:vapid runs");
}

#[tokio::test]
async fn a_configured_driver_without_credentials_fails_the_boot() {
    let result = TestApp::builder()
        .temp_database()
        .config("notify.sms.driver", "twilio")
        .module(DatabaseModule)
        .module(NotifyModule::from_config())
        .try_build()
        .await;
    let Err(error) = result else {
        panic!("the boot fails");
    };
    let error = error.to_string();
    assert!(error.contains("notify.twilio.account_sid"), "{error}");
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
    for name in [
        NOTIFICATIONS_TABLE,
        DELIVERIES_TABLE,
        OPT_OUTS_TABLE,
        PUSH_SUBSCRIPTIONS_TABLE,
    ] {
        let table = db
            .conn()
            .query_one_raw(Statement::from_string(
                db.backend(),
                format!("SELECT name FROM sqlite_master WHERE type = 'table' AND name = '{name}'"),
            ))
            .await
            .unwrap();
        assert!(table.is_some(), "the {name} table exists");
    }
}
