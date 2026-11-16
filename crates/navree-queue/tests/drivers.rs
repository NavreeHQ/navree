//! Queue stores a module registers under a name: `queue.driver` selects one,
//! its factory reads its own settings and jobs run on it, and a name that
//! resolves to nothing, or to two things, fails the boot.

#[path = "queue/reference_store.rs"]
mod reference_store;

use std::sync::{Arc, Mutex};

use navree_config::DriverName;
use navree_contracts::{BoxError, Job, JobContext, ModuleBuilderJobsExt};
use navree_kernel::{Module, ModuleBuilder};
use navree_queue::{
    ModuleBuilderQueueExt, Queue, QueueDriver, QueueFake, QueueModule, QueueOptions,
};
use navree_testing::TestApp;
use serde::{Deserialize, Serialize};

use crate::reference_store::ReferenceStore;

/// The customers the receipt job wrote to.
#[derive(Default)]
struct Journal(Mutex<Vec<String>>);

/// Writes a receipt to a customer.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct SendReceipt {
    customer: String,
}

impl Job for SendReceipt {
    const NAME: &'static str = "drivers.send_receipt";

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        let journal = ctx.app.get::<Journal>().await?;
        journal.0.lock().unwrap().push(self.customer);
        Ok(())
    }
}

/// Registers the receipt job.
struct JobsModule;

impl Module for JobsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.job::<SendReceipt>();
    }
}

/// What the `dak` store reads: `[queue.dak]`.
#[derive(Default, Deserialize)]
#[serde(default)]
struct DakSettings {
    region: String,
}

/// Registers the queue store `dak` over a store the test keeps, and records
/// the region and the settings key its factory read.
#[derive(Default)]
struct DakModule {
    store: ReferenceStore,
    seen: Arc<Mutex<Option<(String, String)>>>,
}

impl Module for DakModule {
    fn register(&self, m: &mut ModuleBuilder) {
        let (store, seen) = (self.store.clone(), self.seen.clone());
        m.queue_store("dak", move |ctx| {
            let (store, seen) = (store.clone(), seen.clone());
            async move {
                let settings: DakSettings = ctx.settings()?;
                *seen.lock().unwrap() = Some((settings.region, ctx.settings_key().to_owned()));
                Ok(store)
            }
        });
    }
}

/// Registers `dak` a second time.
struct SecondDakModule;

impl Module for SecondDakModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.queue_store("dak", |_| async { Ok(ReferenceStore::default()) });
    }
}

/// Registers a store under the built-in name `memory`.
struct MemoryNamedModule;

impl Module for MemoryNamedModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.queue_store("memory", |_| async { Ok(ReferenceStore::default()) });
    }
}

/// Registers a store under the name of a key of the `queue` section.
struct WorkersModule;

impl Module for WorkersModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.queue_store("workers", |_| async { Ok(ReferenceStore::default()) });
    }
}

/// Registers a store whose factory fails.
struct BrokenModule;

impl Module for BrokenModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.queue_store("broken", |_| async {
            Err::<ReferenceStore, BoxError>("the Amritsar depot is shut".into())
        });
    }
}

#[tokio::test]
async fn a_registered_queue_store_runs_jobs() {
    let module = DakModule::default();
    let (store, seen) = (module.store.clone(), module.seen.clone());
    let journal = Arc::new(Journal::default());
    let app = TestApp::builder()
        .config("queue.driver", "dak")
        .config("queue.dak.region", "Doaba")
        .module(QueueModule::from_config())
        .module(module)
        .module(JobsModule)
        .override_value(journal.clone())
        .build()
        .await;

    let queue = app.get::<Queue>().await;
    assert!(format!("{queue:?}").contains("dak"), "{queue:?}");
    assert_eq!(queue.options().driver.as_str(), "dak");
    queue
        .dispatch(SendReceipt {
            customer: "Harpreet Sandhu".to_owned(),
        })
        .await
        .unwrap();
    assert_eq!(store.len(), 1, "the job is in the registered store");
    assert_eq!(queue.pending_count("default").await.unwrap(), 1);

    let stats = queue.run_pending().await.unwrap();
    assert_eq!(stats.succeeded, 1);
    assert_eq!(*journal.0.lock().unwrap(), ["Harpreet Sandhu"]);
    assert_eq!(store.len(), 0, "a finished job leaves the store");
    assert_eq!(
        *seen.lock().unwrap(),
        Some(("Doaba".to_owned(), "queue.dak".to_owned()))
    );

    // Given in code, the name selects the store too.
    let module = DakModule::default();
    let (store, seen) = (module.store.clone(), module.seen.clone());
    let app = TestApp::builder()
        .module(QueueModule::for_root(QueueOptions {
            driver: QueueDriver::Registered(DriverName::from_static("dak")),
            ..QueueOptions::default()
        }))
        .module(module)
        .module(JobsModule)
        .override_value(Arc::new(Journal::default()))
        .build()
        .await;
    app.get::<Queue>()
        .await
        .dispatch(SendReceipt {
            customer: "Meera Sharma".to_owned(),
        })
        .await
        .unwrap();
    assert_eq!(store.len(), 1);
    assert_eq!(
        *seen.lock().unwrap(),
        Some((String::new(), "queue.dak".to_owned()))
    );
}

#[tokio::test]
async fn an_unregistered_queue_store_fails_the_boot_naming_m_queue_store() {
    let error = TestApp::builder()
        .config("queue.driver", "tapal")
        .module(QueueModule::from_config())
        .module(DakModule::default())
        .try_build()
        .await
        .expect_err("no store named tapal");
    let message = error.to_string();
    assert!(
        message.contains("queue.driver does not name a built-in queue store or one a module registers with m.queue_store (known: database, memory, redis, dak)"),
        "{message}"
    );
}

#[tokio::test]
async fn a_queue_store_registration_that_cannot_work_fails_the_boot_naming_m_queue_store() {
    let memory = || {
        TestApp::builder()
            .config("queue.driver", "memory")
            .module(QueueModule::from_config())
    };

    let twice = "queue store `dak` is registered by both `DakModule` and `SecondDakModule`; register it once";
    let message = memory()
        .module(DakModule::default())
        .module(SecondDakModule)
        .try_build()
        .await
        .expect_err("two stores named dak")
        .to_string();
    assert!(
        message.contains(twice),
        "checked although memory is selected: {message}"
    );

    // The check does not depend on `Queue` being built: a fake replaces it.
    let fake = QueueFake::new();
    let message = memory()
        .module(DakModule::default())
        .module(SecondDakModule)
        .override_value(Arc::new(fake.queue()))
        .try_build()
        .await
        .expect_err("two stores named dak, with a fake queue")
        .to_string();
    assert!(message.contains(twice), "{message}");

    let built_in = "m.queue_store(\"memory\") in `MemoryNamedModule`: `memory` is a built-in queue store; register yours under another name";
    let message = memory()
        .module(MemoryNamedModule)
        .try_build()
        .await
        .expect_err("a built-in name")
        .to_string();
    assert!(message.contains(built_in), "{message}");

    let setting = "m.queue_store(\"workers\") in `WorkersModule`: `queue.workers` is a setting of the queue section; register the queue store under another name";
    let message = memory()
        .module(WorkersModule)
        .try_build()
        .await
        .expect_err("the name of a setting")
        .to_string();
    assert!(message.contains(setting), "{message}");

    let message = TestApp::builder()
        .config("queue.driver", "broken")
        .module(QueueModule::from_config())
        .module(BrokenModule)
        .try_build()
        .await
        .expect_err("a factory that fails")
        .to_string();
    assert!(
        message.contains("queue store `broken` (registered with m.queue_store in `BrokenModule`) failed to start: the Amritsar depot is shut"),
        "{message}"
    );
}
