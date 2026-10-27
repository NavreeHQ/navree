//! Strict modes chosen by apps through `[orm]` and `OrmModule`. A process
//! of its own: an app with `OrmModule` installs its strictness for code
//! that runs without an executor, which would disturb other suites.

use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use navree_db::sea_orm::ConnectionTrait;
use navree_db::{DatabaseModule, Db};
use navree_orm::strict::Violation;
use navree_orm::{Model, OrmError, OrmModule, OrmOptions, OrmRuntime};
use navree_testing::TestApp;
use serde::Serialize;

#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(timestamps)]
struct Probe {
    id: i64,
    name: String,
    reading: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

async fn app(builder: navree_testing::TestAppBuilder) -> (TestApp, Arc<Db>) {
    let app = builder
        .temp_database()
        .module(DatabaseModule::from_config())
        .build()
        .await;
    let db = app.get::<Db>().await;
    db.conn()
        .execute_unprepared(
            "CREATE TABLE probes (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, reading INTEGER NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL)",
        )
        .await
        .unwrap();
    Probe::create(
        &*db,
        Probe {
            name: "north".into(),
            reading: 21,
            ..Probe::default()
        },
    )
    .await
    .unwrap();
    (app, db)
}

async fn partial(db: &Db) -> Result<i64, OrmError> {
    let probes = Probe::query()
        .select(["id", "name", "created_at", "updated_at"])
        .get(db)
        .await?;
    Ok(probes[0].reading)
}

#[tokio::test]
async fn production_apps_are_lenient_and_the_others_strict() {
    let (production, db) = app(TestApp::builder()
        .config("env", "production")
        .module(OrmModule::from_config()))
    .await;
    assert_eq!(
        partial(&db).await.unwrap(),
        0,
        "the default of a left-out column"
    );
    let runtime = production.get::<OrmRuntime>().await;
    assert_eq!(runtime.environment(), Some("production"));
    assert!(!runtime.strictness().prevents_lazy_loading());

    let (development, db) = app(TestApp::builder()
        .config("app.env", "local")
        .module(OrmModule::from_config()))
    .await;
    assert!(matches!(
        partial(&db).await.unwrap_err(),
        OrmError::MissingAttribute { .. }
    ));
    let runtime = development.get::<OrmRuntime>().await;
    assert!(runtime.strictness().prevents_discarding_attributes());
    assert!(runtime.strictness().prevents_lazy_loading());
}

#[tokio::test]
async fn each_mode_can_be_set_on_its_own() {
    let (_app, db) = app(TestApp::builder()
        .config("orm.strict", true)
        .config("orm.prevent_missing_attributes", false)
        .module(OrmModule::from_config()))
    .await;
    assert_eq!(partial(&db).await.unwrap(), 0);

    let (_app, db) =
        app(TestApp::builder().module(OrmModule::for_root(OrmOptions::lenient()))).await;
    assert_eq!(partial(&db).await.unwrap(), 0);
}

#[tokio::test]
async fn handlers_on_the_module_take_over_from_errors() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let (_app, db) = app(
        TestApp::builder().module(OrmModule::from_config().on_missing_attribute(
            move |violation| {
                log.lock().unwrap().push(violation.to_string());
                Ok(())
            },
        )),
    )
    .await;
    assert_eq!(partial(&db).await.unwrap(), 0);
    assert_eq!(
        *seen.lock().unwrap(),
        ["column `reading` of Probe is missing from the row"]
    );
}

#[tokio::test]
async fn discarded_attributes_and_lazy_loading_have_handlers_too() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (discarded, lazy) = (seen.clone(), seen.clone());
    let app = navree_kernel::Navree::app()
        .module(
            OrmModule::for_root(OrmOptions::strict())
                .on_discarded_attributes(move |violation| {
                    discarded.lock().unwrap().push(violation.to_string());
                    Ok(())
                })
                .on_lazy_loading(move |violation| {
                    lazy.lock().unwrap().push(violation.to_string());
                    Err(OrmError::InvalidQuery("eager load it".into()))
                }),
        )
        .boot()
        .await
        .unwrap();
    let strictness = app
        .ctx()
        .get::<OrmRuntime>()
        .await
        .unwrap()
        .strictness()
        .clone();
    strictness
        .check(Violation::DiscardedAttributes {
            model: "Probe",
            columns: vec!["reading".into()],
        })
        .expect("tolerated by the handler");
    let error = strictness
        .check(Violation::LazyLoading {
            model: "Probe",
            relation: "sensor".into(),
        })
        .unwrap_err();
    assert_eq!(error.to_string(), "invalid query: eager load it");
    assert_eq!(
        *seen.lock().unwrap(),
        [
            "columns reading of Probe are not fillable and were not filled",
            "relation `sensor` of Probe was loaded lazily",
        ]
    );
}

#[tokio::test]
async fn invalid_options_stop_the_boot() {
    let error = navree_kernel::Navree::app()
        .module(navree_config::ConfigModule::for_root(
            navree_config::ConfigSources::new().set("orm.prune_chunk", 0),
        ))
        .module(OrmModule::from_config())
        .boot()
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("prune_chunk must be at least 1"), "{error}");
}

#[tokio::test]
async fn without_config_the_module_uses_its_defaults() {
    let app = navree_kernel::Navree::app()
        .module(OrmModule)
        .boot()
        .await
        .unwrap();
    let runtime = app.ctx().get::<OrmRuntime>().await.unwrap();
    assert_eq!(runtime.options(), &OrmOptions::default());
    assert_eq!(runtime.environment(), None);
    assert!(runtime.strictness().prevents_missing_attributes());
}
