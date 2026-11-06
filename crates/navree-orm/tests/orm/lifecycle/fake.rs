//! `ModelEventFake`: tests see which model events fired, and no listener
//! runs.

use std::sync::Arc;

use navree_kernel::{Module, ModuleBuilder};
use navree_orm::events::ModelEventFake;
use navree_orm::hooks::ModelEvent;
use navree_orm::{ExecContext, Model, ModuleBuilderOrmExt, Observer, Record, Result};

use crate::models::Company;
use crate::support::epoch;

/// A listener that must not run while events are faked.
struct Explodes;

impl Observer<Company> for Explodes {
    async fn created(&self, _company: &Record<Company>, _ctx: &ExecContext) -> Result<()> {
        panic!("listeners do not run while model events are faked");
    }
}

struct Exploding;

impl Module for Exploding {
    fn register(&self, m: &mut ModuleBuilder) {
        m.observe::<Company>(Explodes);
    }
}

#[tokio::test]
async fn the_fake_records_events_instead_of_running_listeners() {
    let fake = Arc::new(ModelEventFake::new());
    let app = navree_testing::TestApp::builder()
        .temp_database()
        .override_value::<dyn navree_contracts::Clock>(Arc::new(navree_contracts::FixedClock::new(
            epoch(),
        )))
        .override_value(fake.clone())
        .module(navree_db::DatabaseModule::from_config())
        .module(Exploding)
        .build()
        .await;
    let db = app.get::<navree_db::Db>().await;
    crate::support::create_tables(&db).await;

    let uow = db.uow(None, None).await;
    let mut agra = Company::create(
        &uow,
        Company {
            name: "Agra".into(),
            ..Company::default()
        },
    )
    .await
    .unwrap();
    fake.assert_fired::<Company>(ModelEvent::Creating);
    fake.assert_not_fired::<Company>(ModelEvent::Created);
    uow.commit().await.unwrap();
    fake.assert_fired::<Company>(ModelEvent::Created);
    fake.assert_fired_times::<Company>(ModelEvent::Saved, 1);
    assert_eq!(
        fake.keys::<Company>(ModelEvent::Created),
        [agra.id.to_string()]
    );

    agra.delete_quietly(&*db).await.unwrap();
    fake.assert_not_fired::<Company>(ModelEvent::Deleted);

    let names: Vec<String> = fake
        .fired()
        .iter()
        .map(|fired| format!("{} {}", fired.model, fired.event))
        .collect();
    assert_eq!(
        names,
        [
            "Company saving",
            "Company creating",
            "Company created",
            "Company saved"
        ]
    );
    fake.clear();
    fake.assert_nothing_fired();
}

#[test]
#[should_panic(expected = "expected Company `deleted` to fire, but it did not; fired: (nothing)")]
fn a_failed_assertion_says_what_fired() {
    ModelEventFake::new().assert_fired::<Company>(ModelEvent::Deleted);
}
