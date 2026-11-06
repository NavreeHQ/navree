//! Quiet operations: the write happens, no listener hears it, the audit
//! trail still records it.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use navree_kernel::{Module, ModuleBuilder};
use navree_orm::{
    ExecContext, Model, ModelLifecycle, ModuleBuilderOrmExt, Observer, OrmError, Record, Result,
};

use crate::models::{Company, Deal};
use crate::support::{app, count_rows};

/// Every event of companies and deals, by name.
#[derive(Clone, Default)]
struct Heard(Arc<Mutex<Vec<String>>>);

impl Heard {
    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}

struct Listens(Heard);

macro_rules! listen_to_everything {
    ($model:ty) => {
        impl Observer<$model> for Listens {
            async fn saving(
                &self,
                _r: &mut Record<$model>,
                _c: &ExecContext,
            ) -> Result<navree_orm::hooks::Flow> {
                self.0.0.lock().unwrap().push("saving".into());
                Ok(navree_orm::hooks::Flow::Continue)
            }

            async fn updating(
                &self,
                _r: &mut Record<$model>,
                _c: &ExecContext,
            ) -> Result<navree_orm::hooks::Flow> {
                self.0.0.lock().unwrap().push("updating".into());
                Ok(navree_orm::hooks::Flow::Continue)
            }

            async fn updated(&self, _r: &Record<$model>, _c: &ExecContext) -> Result<()> {
                self.0.0.lock().unwrap().push("updated".into());
                Ok(())
            }

            async fn created(&self, _r: &Record<$model>, _c: &ExecContext) -> Result<()> {
                self.0.0.lock().unwrap().push("created".into());
                Ok(())
            }

            async fn deleted(&self, _r: &Record<$model>, _c: &ExecContext) -> Result<()> {
                self.0.0.lock().unwrap().push("deleted".into());
                Ok(())
            }
        }
    };
}

listen_to_everything!(Company);
listen_to_everything!(Deal);

struct Listening(Heard);

impl Module for Listening {
    fn register(&self, m: &mut ModuleBuilder) {
        m.observe::<Company>(Listens(self.0.clone()));
        m.observe::<Deal>(Listens(self.0.clone()));
    }
}

#[tokio::test]
async fn quiet_writes_reach_the_database_and_the_audit_trail_only() {
    let heard = Heard::default();
    let (_app, db, clock) = app(vec![Box::new(Listening(heard.clone()))]).await;
    let uow = db.uow(None, None).await;

    let mut agra = Company::create_quietly(
        &uow,
        Company {
            name: "Agra".into(),
            ..Company::default()
        },
    )
    .await
    .unwrap();
    assert!(agra.exists());
    let mut gulati = Company::query()
        .create_quietly(
            &uow,
            Company {
                name: "Gulati".into(),
                ..Company::default()
            },
        )
        .await
        .unwrap();

    clock.advance(Duration::from_secs(5));
    agra.name = "Agra Corp".into();
    assert!(agra.save_quietly(&uow).await.unwrap());
    assert!(
        agra.update_quietly(&uow, |c| c.employees = 9)
            .await
            .unwrap()
    );
    clock.advance(Duration::from_secs(5));
    assert!(agra.touch_quietly(&uow).await.unwrap());
    assert!(
        agra.increment_quietly(&uow, Company::EMPLOYEES, 3)
            .await
            .unwrap()
    );
    assert_eq!(agra.employees, 12);
    assert!(
        agra.decrement_quietly(&uow, Company::EMPLOYEES, 2)
            .await
            .unwrap()
    );
    assert_eq!(agra.employees, 10);
    assert!(gulati.delete_quietly(&uow).await.unwrap());

    assert!(heard.take().is_empty(), "no listener heard anything");
    assert_eq!(
        uow.changes().len(),
        8,
        "two creates, five updates and a delete in the audit trail"
    );
    uow.commit().await.unwrap();
    assert!(
        heard.take().is_empty(),
        "nothing waited for the commit either"
    );
    assert_eq!(count_rows(&db, "companies").await, 1);
    let stored = Company::find_or_fail(&*db, agra.id).await.unwrap();
    assert_eq!((stored.name.as_str(), stored.employees), ("Agra Corp", 10));

    // The loud versions are heard.
    agra.name = "Agra Inc".into();
    agra.save(&*db).await.unwrap();
    assert_eq!(heard.take(), ["saving", "updating", "updated"]);
}

#[tokio::test]
async fn several_columns_step_in_one_statement_quietly_or_not() {
    let heard = Heard::default();
    let (_app, db, _clock) = app(vec![Box::new(Listening(heard.clone()))]).await;
    let mut deal = Deal::create(&*db, Deal::new("Steps", 100)).await.unwrap();
    heard.take();

    assert!(
        deal.increment_each(
            &*db,
            [
                (Deal::AMOUNT_CENTS, 50),
                (Deal::AMOUNT_CENTS.unqualified(), 1)
            ]
        )
        .await
        .is_err(),
        "a column steps once per statement"
    );
    deal.discard_changes();

    let mut agra = Company::create(
        &*db,
        Company {
            name: "Steps".into(),
            employees: 10,
            ..Company::default()
        },
    )
    .await
    .unwrap();
    heard.take();
    assert!(
        agra.increment_each(&*db, [(Company::EMPLOYEES, 5)])
            .await
            .unwrap()
    );
    assert_eq!(agra.employees, 15);
    assert_eq!(heard.take(), ["updating", "updated"]);
    assert!(
        agra.decrement_each_quietly(&*db, [(Company::EMPLOYEES, 4)])
            .await
            .unwrap()
    );
    assert!(
        agra.increment_each_quietly(&*db, [(Company::EMPLOYEES, 1)])
            .await
            .unwrap()
    );
    assert!(heard.take().is_empty());
    assert_eq!(agra.employees, 12);
    assert!(agra.is_clean());
    assert_eq!(
        Company::find_or_fail(&*db, agra.id)
            .await
            .unwrap()
            .employees,
        12
    );

    let error = agra
        .increment_each(&*db, Vec::<(navree_orm::Column<Company, i32>, i32)>::new())
        .await
        .unwrap_err();
    assert!(matches!(error, OrmError::InvalidQuery(_)), "{error:?}");
}
