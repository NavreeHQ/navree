//! Extra audit sinks added by the app (`ModuleBuilderAuditExt`), beside `AuditLog`
//! and in an app without `AuditModule`, and the refusal of a community plugin's
//! own registration.

use std::any::type_name;
use std::sync::{Arc, Mutex};

use navree_audit::{AuditLog, ModuleBuilderAuditExt};
use navree_contracts::{AuditEntry, AuditKind, AuditSink};
use navree_db::Db;
use navree_kernel::{
    BoxError, BoxFuture, Injectable, KernelError, Module, ModuleBuilder, Navree, Resolver,
};
use navree_orm::Model as _;
use serde_json::json;
use uuid::Uuid;

use crate::support::{self, Note, User, audit_log, audit_rows, new_note};

/// Keeps every `AuditEntry` handed to it, in order.
#[derive(Clone, Default)]
struct RecordingSink(Arc<Mutex<Vec<AuditEntry>>>);

impl RecordingSink {
    fn entries(&self) -> Vec<AuditEntry> {
        self.0.lock().unwrap().clone()
    }

    fn actions(&self) -> Vec<String> {
        self.entries().into_iter().map(|e| e.action).collect()
    }
}

impl AuditSink for RecordingSink {
    fn record(&self, entry: AuditEntry) -> BoxFuture<'_, Result<(), BoxError>> {
        self.0.lock().unwrap().push(entry);
        Box::pin(async { Ok(()) })
    }
}

/// An injectable sink built by the container.
#[derive(Default)]
struct InjectableSink(Mutex<Vec<String>>);

impl Injectable for InjectableSink {
    fn construct(_r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self::default()) })
    }
}

impl AuditSink for InjectableSink {
    fn record(&self, entry: AuditEntry) -> BoxFuture<'_, Result<(), BoxError>> {
        self.0.lock().unwrap().push(entry.action);
        Box::pin(async { Ok(()) })
    }
}

struct ExtraSinkModule {
    second: RecordingSink,
}

impl Module for ExtraSinkModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.audit_sink_value(self.second.clone())
            .audit_sink::<InjectableSink>();
    }
}

#[tokio::test]
async fn an_app_adds_a_second_audit_sink_and_both_receive_every_record() {
    let second = RecordingSink::default();
    let app = support::builder()
        .0
        .module(ExtraSinkModule {
            second: second.clone(),
        })
        .build()
        .await;

    let sink = app.get::<dyn AuditSink>().await;
    let log = audit_log(&app).await;
    assert!(
        std::ptr::addr_eq(Arc::as_ptr(&sink), Arc::as_ptr(&log)),
        "AuditModule's AuditLog remains the app's dyn AuditSink"
    );

    let harpreet = Uuid::now_v7();
    let db = app.get::<Db>().await;

    // 1. Security entry through `dyn AuditSink` outside any transaction, carrying a secret in
    // metadata that must be stripped before reaching either sink.
    sink.record(
        AuditEntry::new(AuditKind::Security, "access.denied")
            .actor(harpreet)
            .metadata(json!({"route": "/khata/ledgers", "password": "do-not-forward"})),
    )
    .await
    .unwrap();

    // 2. Entry through `AuditLog::record` directly.
    log.record(AuditEntry::new(AuditKind::Auth, "auth.login").actor(harpreet))
        .await
        .unwrap();

    // 3. Rolled-back unit of work: neither `audit_logs` nor the added sinks receive its data row
    // or `record_in` entry.
    let rolled_back = db.uow(Some(harpreet), Some("req-rollback".into())).await;
    Note::create(&rolled_back, new_note("draft")).await.unwrap();
    log.record_in(
        &rolled_back,
        AuditEntry::new(AuditKind::Custom, "ledger.aborted"),
    )
    .await
    .unwrap();
    rolled_back.rollback().await.unwrap();

    // 4. Committed unit of work: both ORM data changes (with secret fields stripped) and
    // `record_in` entries reach `audit_logs` and every added sink once committed.
    let committed = db.uow(Some(harpreet), Some("req-commit".into())).await;
    User::create(
        &committed,
        User {
            id: harpreet,
            email: "harpreet@ludhiana.example".to_owned(),
            password_hash: "argon2-secret".to_owned(),
            remember_token: Some("remember-secret".to_owned()),
            ssn: None,
            settings: json!({"city": "Ludhiana"}),
        },
    )
    .await
    .unwrap();
    log.record_in(
        &committed,
        AuditEntry::new(AuditKind::Custom, "ledger.closed").subject("Ledger", "7"),
    )
    .await
    .unwrap();
    committed.commit().await.unwrap();
    log.flush().await;

    let expected = vec![
        "access.denied".to_owned(),
        "auth.login".to_owned(),
        "user.created".to_owned(),
        "ledger.closed".to_owned(),
    ];
    let db_rows = audit_rows(&app).await;
    let db_actions: Vec<String> = db_rows.iter().map(|r| r.action.clone()).collect();
    assert_eq!(db_actions, expected);
    assert_eq!(second.actions(), expected);

    let injected = app.get::<InjectableSink>().await;
    assert_eq!(*injected.0.lock().unwrap(), expected);

    let forwarded = second.entries();
    assert_eq!(forwarded[0].metadata, json!({"route": "/khata/ledgers"}));
    let user_new = forwarded[2]
        .new_values
        .as_ref()
        .expect("user.created new_values");
    assert!(user_new.get("password_hash").is_none(), "{user_new}");
    assert!(user_new.get("remember_token").is_none(), "{user_new}");
    assert_eq!(user_new["email"], json!("harpreet@ludhiana.example"));

    app.shutdown().await.unwrap();

    // Without `AuditModule`, two sinks added by the app form the app's exported `dyn AuditSink`
    // (supporting constructor injection as well as `try_get`) and both receive every recorded
    // entry with secret fields stripped.
    let left = RecordingSink::default();
    let right = RecordingSink::default();
    struct WritesAudit(Arc<dyn AuditSink>);
    impl Injectable for WritesAudit {
        fn dependencies() -> Vec<navree_kernel::ProviderKey> {
            vec![navree_kernel::ProviderKey::of::<dyn AuditSink>()]
        }
        fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
            Box::pin(async move { Ok(Self(r.get::<dyn AuditSink>().await?)) })
        }
    }
    struct TwoMemorySinks(RecordingSink, RecordingSink);
    impl Module for TwoMemorySinks {
        fn register(&self, m: &mut ModuleBuilder) {
            m.audit_sink_value(self.0.clone())
                .audit_sink_value(self.1.clone())
                .provider::<WritesAudit>()
                .exports::<WritesAudit>();
        }
    }
    let standalone = Navree::app()
        .module(TwoMemorySinks(left.clone(), right.clone()))
        .boot()
        .await
        .expect("standalone sinks boot and satisfy constructor injection");
    let writer = standalone.ctx().get::<WritesAudit>().await.unwrap();
    writer
        .0
        .record(
            AuditEntry::new(AuditKind::Access, "role.granted")
                .metadata(json!({"password": "do-not-forward", "role": "editor"})),
        )
        .await
        .unwrap();
    assert_eq!(left.actions(), vec!["role.granted".to_owned()]);
    assert_eq!(right.actions(), vec!["role.granted".to_owned()]);
    assert_eq!(left.entries()[0].metadata, json!({"role": "editor"}));
    assert_eq!(right.entries()[0].metadata, json!({"role": "editor"}));
    standalone.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_custom_audit_sink_beside_added_sinks_fails_the_boot() {
    struct UnexportedDirectSink;
    impl Module for UnexportedDirectSink {
        fn register(&self, m: &mut ModuleBuilder) {
            m.value_arc::<dyn AuditSink>(Arc::new(RecordingSink::default()));
        }
    }

    struct ExportedDirectSink;
    impl Module for ExportedDirectSink {
        fn register(&self, m: &mut ModuleBuilder) {
            m.value_arc::<dyn AuditSink>(Arc::new(RecordingSink::default()))
                .exports::<dyn AuditSink>();
        }
    }

    struct AddedSink;
    impl Module for AddedSink {
        fn register(&self, m: &mut ModuleBuilder) {
            m.audit_sink_value(RecordingSink::default());
        }
    }

    let unexported_err = Navree::app()
        .module(UnexportedDirectSink)
        .module(AddedSink)
        .boot()
        .await
        .expect_err("unexported direct dyn AuditSink beside ModuleBuilderAuditExt must fail boot")
        .to_string();
    assert!(
        unexported_err.contains("module `UnexportedDirectSink` registers `dyn AuditSink` directly alongside sinks added with ModuleBuilderAuditExt"),
        "{unexported_err}"
    );

    let exported_err = Navree::app()
        .module(ExportedDirectSink)
        .module(AddedSink)
        .boot()
        .await
        .expect_err("exported direct dyn AuditSink beside ModuleBuilderAuditExt must fail boot")
        .to_string();
    assert!(
        exported_err.contains("multiple modules export `dyn AuditSink` alongside sinks added with ModuleBuilderAuditExt"),
        "{exported_err}"
    );
}

/// A community plugin shipping an audit sink for the app to add, and a module for testing its
/// own refused registrations.
mod navree_plugin_khata_watch {
    use std::sync::{Arc, Mutex};

    use navree_contracts::{AuditEntry, AuditSink};
    use navree_kernel::{BoxError, BoxFuture, Factory, Module, ModuleBuilder, PluginInfo};

    const KHATA_WATCH: PluginInfo = PluginInfo::new(
        "navree-plugin-khata-watch",
        module_path!(),
        "khata_watch",
        "1.0.0",
    );

    #[derive(Clone, Default)]
    pub struct WatchSink(Arc<Mutex<Vec<String>>>);

    impl WatchSink {
        pub fn actions(&self) -> Vec<String> {
            self.0.lock().unwrap().clone()
        }
    }

    impl AuditSink for WatchSink {
        fn record(&self, entry: AuditEntry) -> BoxFuture<'_, Result<(), BoxError>> {
            self.0.lock().unwrap().push(entry.action);
            Box::pin(async { Ok(()) })
        }
    }

    pub fn sink_factory(sink: WatchSink) -> Factory<dyn AuditSink> {
        Factory::new(move |_| {
            let sink = sink.clone();
            async move { Ok(Arc::new(sink) as Arc<dyn AuditSink>) }
        })
    }

    pub struct WatchPlugin(pub fn(&mut ModuleBuilder));

    impl Module for WatchPlugin {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(KHATA_WATCH);
            (self.0)(m);
        }
    }
}

#[tokio::test]
async fn a_plugins_sink_added_by_the_app_receives_records_beside_the_audit_log() {
    use navree_plugin_khata_watch::{WatchPlugin, WatchSink, sink_factory};

    let watch = WatchSink::default();
    struct AppAddsWatchSink(WatchSink);
    impl Module for AppAddsWatchSink {
        fn register(&self, m: &mut ModuleBuilder) {
            m.audit_sink_factory(sink_factory(self.0.clone()));
        }
    }

    let app = support::builder()
        .0
        .module(WatchPlugin(|_| {}))
        .module(AppAddsWatchSink(watch.clone()))
        .build()
        .await;

    let sink = app.get::<dyn AuditSink>().await;
    let log = app.get::<AuditLog>().await;
    assert!(std::ptr::addr_eq(Arc::as_ptr(&sink), Arc::as_ptr(&log)));

    let simran = Uuid::now_v7();
    sink.record(AuditEntry::new(AuditKind::Auth, "auth.login").actor(simran))
        .await
        .unwrap();
    let db = app.get::<Db>().await;
    let uow = db.uow(Some(simran), Some("req-simran".into())).await;
    Note::create(&uow, new_note("Amritsar ledger"))
        .await
        .unwrap();
    uow.commit().await.unwrap();
    log.flush().await;

    let expected = vec!["auth.login".to_owned(), "note.created".to_owned()];
    let db_actions: Vec<String> = audit_rows(&app)
        .await
        .into_iter()
        .map(|r| r.action)
        .collect();
    assert_eq!(db_actions, expected);
    assert_eq!(watch.actions(), expected);

    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_plugins_own_registration_of_a_sink_is_still_refused_and_the_text_says_how_the_app_adds_it()
 {
    use navree_plugin_khata_watch::{WatchPlugin, WatchSink, sink_factory};

    let expected = format!(
        "plugin `navree-plugin-khata-watch` provides `{}` in `WatchPlugin`, which receives the app's audit records; only a module of the app registers it: remove the plugin or update it to a version that leaves it to the app, and to use the plugin's sink, add it from a module of the app (m.audit_sink, m.audit_sink_value or m.audit_sink_factory)",
        type_name::<dyn AuditSink>()
    );

    let attempts: [fn(&mut ModuleBuilder); 3] = [
        |m| {
            m.audit_sink_value(WatchSink::default());
        },
        |m| {
            m.audit_sink_factory(sink_factory(WatchSink::default()));
        },
        |m| {
            m.factory(sink_factory(WatchSink::default()))
                .exports::<dyn AuditSink>();
        },
    ];

    for attempt in attempts {
        let validated = Navree::app()
            .module(WatchPlugin(attempt))
            .validate()
            .expect_err("a plugin's own sink registration must fail validate")
            .to_string();
        assert_eq!(validated, expected);

        let booted = match support::builder()
            .0
            .module(WatchPlugin(attempt))
            .try_build()
            .await
        {
            Ok(_) => panic!("the app booted with a plugin's own sink registration"),
            Err(error) => error.to_string(),
        };
        assert!(booted.contains(&expected), "{booted}");
    }
}
