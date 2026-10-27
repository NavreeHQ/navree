//! Lifecycle hooks registered by the app: cancelling, changing records,
//! seeing retrievals.

use std::sync::{Arc, Mutex};

use navree_kernel::{BoxFuture, Factory, Module, ModuleBuilder};
use navree_orm::hooks::{Flow, ModelEvent, ModelHook};
use navree_orm::{ExecContext, Model, Record, Result};

use crate::models::Company;
use crate::support::{app, count_rows};

/// What the hook saw, in order.
#[derive(Default)]
struct Seen(Mutex<Vec<String>>);

/// Refuses companies named "Blocked", upper-cases country codes before
/// saving, refuses to delete companies with employees, and notes every
/// event.
struct Guard(Arc<Seen>);

impl ModelHook<Company> for Guard {
    fn before<'a>(
        &'a self,
        event: ModelEvent,
        _ctx: &'a ExecContext,
        record: &'a mut Record<Company>,
    ) -> BoxFuture<'a, Result<Flow>> {
        Box::pin(async move {
            self.0.0.lock().unwrap().push(format!("{event} {}", record.name));
            if event == ModelEvent::Saving {
                if record.name == "Blocked" {
                    return Ok(Flow::Cancel);
                }
                if let Some(country) = &mut record.country {
                    *country = country.to_uppercase();
                }
            }
            if event == ModelEvent::Deleting && record.employees > 0 {
                return Ok(Flow::Cancel);
            }
            Ok(Flow::Continue)
        })
    }

    fn after<'a>(
        &'a self,
        event: ModelEvent,
        _ctx: &'a ExecContext,
        record: &'a mut Record<Company>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.0.0.lock().unwrap().push(format!("{event} {}", record.name));
            if event == ModelEvent::Retrieved {
                record.set_extra("seen", true);
            }
            Ok(())
        })
    }
}

struct Hooks(Arc<Seen>);

impl Module for Hooks {
    fn register(&self, m: &mut ModuleBuilder) {
        let seen = self.0.clone();
        m.multi(Factory::<dyn ModelHook<Company>>::new(move |_| {
            let seen = seen.clone();
            async move { Ok(Arc::new(Guard(seen)) as Arc<dyn ModelHook<Company>>) }
        }));
    }
}

#[tokio::test]
async fn hooks_see_and_steer_the_lifecycle() {
    let seen = Arc::new(Seen::default());
    let (_app, db, _clock) = app(vec![Box::new(Hooks(seen.clone()))]).await;

    let acme = Company::create(
        &*db,
        Company {
            name: "Acme".into(),
            country: Some("us".into()),
            ..Company::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(acme.country.as_deref(), Some("US"), "the saving hook changed the record");
    assert_eq!(
        std::mem::take(&mut *seen.0.lock().unwrap()),
        ["saving Acme", "creating Acme", "created Acme", "saved Acme"]
    );

    let mut blocked = Record::new(Company {
        name: "Blocked".into(),
        ..Company::default()
    });
    assert!(!blocked.save(&*db).await.unwrap(), "cancelled");
    assert!(!blocked.exists());
    assert_eq!(count_rows(&db, "companies").await, 1);

    let mut found = Company::find_or_fail(&*db, acme.id).await.unwrap();
    assert_eq!(found.extra::<bool>("seen").unwrap(), Some(true));
    seen.0.lock().unwrap().clear();

    found.employees = 4;
    found.save(&*db).await.unwrap();
    assert_eq!(
        std::mem::take(&mut *seen.0.lock().unwrap()),
        ["saving Acme", "updating Acme", "updated Acme", "saved Acme"]
    );

    assert!(!found.delete(&*db).await.unwrap(), "companies with employees stay");
    assert_eq!(count_rows(&db, "companies").await, 1);
    found.employees = 0;
    found.save(&*db).await.unwrap();
    assert!(found.delete(&*db).await.unwrap());
    assert_eq!(count_rows(&db, "companies").await, 0);
}

#[tokio::test]
async fn a_pool_without_an_app_runs_no_app_hooks() {
    let t = crate::support::sqlite().await;
    let company = Company::create(
        &t.db,
        Company {
            name: "Plain".into(),
            country: Some("us".into()),
            ..Company::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(company.country.as_deref(), Some("us"));
    assert!(Company::meta().name() == "Company");
}
