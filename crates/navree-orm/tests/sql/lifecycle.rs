//! The SQL of the lifecycle: trashed filters, mass restores and force
//! deletes, runtime global scopes, per dialect.

use std::sync::{Arc, Mutex};

use navree_contracts::{Clock, FixedClock};
use navree_kernel::{AppContext, BoxFuture, Navree};
use navree_orm::ir::{Aggregate, DeleteIr, InsertIr, QueryIr, UpdateIr};
use navree_orm::sql::{compile_delete, compile_update};
use navree_orm::{
    Access, DocumentDriver, DocumentTarget, Executor, InsertOutcome, Model, OrmError, Row, Target,
    Value,
};

use super::{Deal, per_dialect, query};

#[test]
fn trashed_filters_apply_last_and_grouped() {
    insta::assert_snapshot!(query(
        Deal::query()
            .only_trashed()
            .where_(Deal::TITLE.eq("a"))
            .or_where(Deal::TITLE.eq("b"))
    ));
    insta::assert_snapshot!(query(Deal::query().with_trashed().without_trashed()));
    insta::assert_snapshot!(query(Deal::query().without_global_scopes().only_trashed()));
    insta::assert_snapshot!(query(Deal::query().with_trashed_if(true)));
}

/// Writes the statements a mass write would run, instead of running them.
#[derive(Default)]
struct Capture {
    updates: Mutex<Vec<UpdateIr>>,
    deletes: Mutex<Vec<DeleteIr>>,
}

impl DocumentDriver for Capture {
    fn name(&self) -> &'static str {
        "capture"
    }

    fn find<'a>(&'a self, _query: &'a QueryIr) -> BoxFuture<'a, Result<Vec<Row>, OrmError>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn aggregate<'a>(
        &'a self,
        _query: &'a QueryIr,
        _aggregate: &'a Aggregate,
    ) -> BoxFuture<'a, Result<Value, OrmError>> {
        Box::pin(async { Ok(Value::BigInt(Some(0))) })
    }

    fn insert<'a>(
        &'a self,
        insert: &'a InsertIr,
    ) -> BoxFuture<'a, Result<InsertOutcome, OrmError>> {
        Box::pin(async move { Ok(InsertOutcome::affected(insert.rows.len() as u64)) })
    }

    fn update<'a>(&'a self, update: &'a UpdateIr) -> BoxFuture<'a, Result<u64, OrmError>> {
        self.updates.lock().unwrap().push(update.clone());
        Box::pin(async { Ok(0) })
    }

    fn delete<'a>(&'a self, delete: &'a DeleteIr) -> BoxFuture<'a, Result<u64, OrmError>> {
        self.deletes.lock().unwrap().push(delete.clone());
        Box::pin(async { Ok(0) })
    }
}

/// Runs operations on [`Capture`], in an app whose clock stands still.
struct Capturing {
    driver: Arc<Capture>,
    app: AppContext,
}

impl Executor for Capturing {
    async fn target(&self, _access: Access) -> Result<Target<'_>, OrmError> {
        Ok(Target::Document(DocumentTarget::new(
            self.driver.clone(),
            Some(self.app.clone()),
        )))
    }
}

async fn capturing() -> (Capturing, navree_kernel::App) {
    let app = Navree::app()
        .override_value::<dyn Clock>(Arc::new(FixedClock::new(super::at())))
        .boot()
        .await
        .unwrap();
    let capturing = Capturing {
        driver: Arc::new(Capture::default()),
        app: app.ctx().clone(),
    };
    (capturing, app)
}

#[tokio::test]
async fn mass_restores_clear_the_column_of_trashed_matches_only() {
    let (exec, _app) = capturing().await;
    Deal::query()
        .where_(Deal::AMOUNT_CENTS.lt(100))
        .restore(&exec)
        .await
        .unwrap();
    let update = exec.driver.updates.lock().unwrap().remove(0);
    insta::assert_snapshot!(per_dialect(|dialect| compile_update(&update, dialect)));
}

#[tokio::test]
async fn mass_force_deletes_remove_trashed_rows_too() {
    let (exec, _app) = capturing().await;
    Deal::query()
        .where_(Deal::AMOUNT_CENTS.lt(100))
        .force_delete(&exec)
        .await
        .unwrap();
    Deal::query()
        .only_trashed()
        .latest()
        .limit(10)
        .force_delete(&exec)
        .await
        .unwrap();
    let deletes = std::mem::take(&mut *exec.driver.deletes.lock().unwrap());
    insta::assert_snapshot!(per_dialect(|dialect| compile_delete(&deletes[0], dialect)));
    insta::assert_snapshot!(per_dialect(|dialect| compile_delete(&deletes[1], dialect)));
}
