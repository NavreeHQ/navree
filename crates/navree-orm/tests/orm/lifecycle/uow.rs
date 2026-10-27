//! Work dispatched from listeners joins the unit of work: in its
//! transaction for in-transaction observers, after the commit otherwise.

use std::sync::{Arc, Mutex};

use navree_contracts::{Dispatcher, JobEnvelope};
use navree_db::Uow;
use navree_kernel::{BoxError, BoxFuture, Module, ModuleBuilder};
use navree_orm::{ExecContext, Model, ModuleBuilderOrmExt, Observer, Record, Result};

use crate::models::Company;
use crate::support::epoch;

/// Notes, for every dispatched job, whether a unit of work was current and
/// open when it was dispatched.
#[derive(Default)]
struct WatchingDispatcher(Mutex<Vec<String>>);

impl Dispatcher for WatchingDispatcher {
    fn dispatch_envelope(&self, envelope: JobEnvelope) -> BoxFuture<'_, Result<String, BoxError>> {
        Box::pin(async move {
            let state = match Uow::current() {
                Some(uow) if !uow.is_finished().await => "in an open unit of work",
                Some(_) => "after its unit of work",
                None => "outside any unit of work",
            };
            self.0
                .lock()
                .unwrap()
                .push(format!("{} {state}", envelope.job_type));
            Ok("job".to_owned())
        })
    }
}

/// A job the observers dispatch.
#[derive(serde::Serialize, serde::Deserialize)]
struct Welcome {
    company: i64,
}

impl navree_contracts::Job for Welcome {
    const NAME: &'static str = "welcome";

    async fn handle(self, _ctx: navree_contracts::JobContext) -> std::result::Result<(), BoxError> {
        Ok(())
    }
}

async fn dispatch(ctx: &ExecContext, company: &Record<Company>) -> Result<()> {
    let app = ctx.app().expect("an app");
    let dispatcher = app.get::<dyn Dispatcher>().await.expect("a dispatcher");
    dispatcher
        .dispatch(Welcome { company: company.id })
        .await
        .map_err(|error| navree_orm::OrmError::Hook {
            model: "Company".into(),
            message: error.to_string(),
        })?;
    Ok(())
}

/// Dispatches inside the transaction.
struct InTransaction;

impl Observer<Company> for InTransaction {
    const IN_TRANSACTION: bool = true;

    async fn created(&self, company: &Record<Company>, ctx: &ExecContext) -> Result<()> {
        dispatch(ctx, company).await
    }
}

/// Dispatches after the commit.
struct AfterCommit;

impl Observer<Company> for AfterCommit {
    async fn saved(&self, company: &Record<Company>, ctx: &ExecContext) -> Result<()> {
        dispatch(ctx, company).await
    }
}

struct Observers;

impl Module for Observers {
    fn register(&self, m: &mut ModuleBuilder) {
        m.observe::<Company>(InTransaction)
            .observe::<Company>(AfterCommit);
    }
}

#[tokio::test]
async fn listeners_dispatch_inside_their_unit_of_work_or_after_it() {
    let dispatcher = Arc::new(WatchingDispatcher::default());
    let app = navree_testing::TestApp::builder()
        .temp_database()
        .override_value::<dyn navree_contracts::Clock>(Arc::new(navree_contracts::FixedClock::new(
            epoch(),
        )))
        .override_value::<dyn Dispatcher>(dispatcher.clone())
        .module(navree_db::DatabaseModule::from_config())
        .module(Observers)
        .build()
        .await;
    let db = app.get::<navree_db::Db>().await;
    crate::support::create_tables(&db).await;

    // A job's unit of work, not made current with `scope`: the ORM makes
    // it current for its listeners.
    let uow = db.uow(None, None).await;
    Company::create(
        &uow,
        Company {
            name: "Acme".into(),
            ..Company::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        *dispatcher.0.lock().unwrap(),
        ["welcome in an open unit of work"]
    );
    uow.commit().await.unwrap();
    assert_eq!(
        *dispatcher.0.lock().unwrap(),
        [
            "welcome in an open unit of work",
            "welcome outside any unit of work"
        ],
        "after the commit the job is queued on its own"
    );
}
