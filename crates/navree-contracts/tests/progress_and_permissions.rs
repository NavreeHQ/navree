//! Job progress and cancellation (`JobContext::progress`, `JobMonitor`) and
//! permission checks outside route declarations (`PermissionCheck`).

use std::sync::{Arc, Mutex};

use navree_contracts::{
    AuthVia, BoxError, BoxFuture, JobCancelled, JobContext, JobMonitor, JobProgress,
    PermissionCheck, Principal, permitted,
};
use navree_kernel::{App, Factory, Module, ModuleBuilder, Navree};
use uuid::Uuid;

/// Records every progress report; jobs whose id starts with `cancelled`
/// are cancelled.
#[derive(Default)]
struct Recorder {
    reports: Mutex<Vec<(String, JobProgress)>>,
}

impl JobMonitor for Recorder {
    fn progress<'a>(
        &'a self,
        job_id: &'a str,
        progress: JobProgress,
    ) -> BoxFuture<'a, Result<(), JobCancelled>> {
        Box::pin(async move {
            self.reports
                .lock()
                .unwrap()
                .push((job_id.to_owned(), progress));
            if job_id.starts_with("cancelled") {
                Err(JobCancelled)
            } else {
                Ok(())
            }
        })
    }

    fn is_cancelled<'a>(&'a self, job_id: &'a str) -> BoxFuture<'a, bool> {
        Box::pin(async move { job_id.starts_with("cancelled") })
    }
}

struct MonitorModule(Arc<Recorder>);

impl Module for MonitorModule {
    fn register(&self, m: &mut ModuleBuilder) {
        let recorder = self.0.clone();
        m.global()
            .value_arc(recorder as Arc<dyn JobMonitor>)
            .exports::<dyn JobMonitor>();
    }
}

struct EmptyModule;

impl Module for EmptyModule {
    fn register(&self, _m: &mut ModuleBuilder) {}
}

async fn boot(module: impl Module) -> App {
    Navree::app().module(module).boot().await.expect("boot")
}

fn job(app: &App, id: &str) -> JobContext {
    JobContext {
        app: app.ctx().clone(),
        job_id: id.to_owned(),
        attempt: 1,
    }
}

#[tokio::test]
async fn progress_without_a_monitor_is_a_no_op() {
    let app = boot(EmptyModule).await;
    let ctx = job(&app, "job-1");
    ctx.progress(1, 10, "Importing rows")
        .await
        .expect("no monitor: nothing to report, never cancelled");
    assert!(!ctx.is_cancelled().await);
    ctx.check_cancelled().await.expect("not cancelled");
}

#[tokio::test]
async fn progress_goes_to_the_monitor() {
    let recorder = Arc::new(Recorder::default());
    let app = boot(MonitorModule(recorder.clone())).await;
    let ctx = job(&app, "job-1");

    ctx.progress(40, 100, "Importing rows").await.unwrap();
    ctx.progress(100, 100, String::from("Done")).await.unwrap();

    let reports = recorder.reports.lock().unwrap().clone();
    assert_eq!(
        reports,
        [
            (
                "job-1".to_owned(),
                JobProgress {
                    done: 40,
                    total: 100,
                    note: "Importing rows".into()
                }
            ),
            (
                "job-1".to_owned(),
                JobProgress {
                    done: 100,
                    total: 100,
                    note: "Done".into()
                }
            ),
        ]
    );
}

#[tokio::test]
async fn a_cancelled_job_learns_it_from_progress_and_checks() {
    let recorder = Arc::new(Recorder::default());
    let app = boot(MonitorModule(recorder)).await;
    let ctx = job(&app, "cancelled-1");

    let error = ctx.progress(1, 2, "half").await.expect_err("cancelled");
    assert_eq!(error, JobCancelled);
    assert!(ctx.is_cancelled().await);
    assert_eq!(ctx.check_cancelled().await, Err(JobCancelled));

    // `?` turns it into the job's error, and the worker recognises it.
    async fn handler(ctx: JobContext) -> Result<(), BoxError> {
        ctx.progress(1, 2, "half").await?;
        Ok(())
    }
    let error = handler(ctx).await.expect_err("stops");
    assert!(error.downcast_ref::<JobCancelled>().is_some());
    assert_eq!(error.to_string(), "the job was cancelled");
}

#[test]
fn progress_percent_is_rounded_down_and_capped() {
    let progress = |done, total| JobProgress {
        done,
        total,
        note: String::new(),
    };
    assert_eq!(progress(0, 0).percent(), 0);
    assert_eq!(progress(1, 3).percent(), 33);
    assert_eq!(progress(2, 3).percent(), 66);
    assert_eq!(progress(3, 3).percent(), 100);
    assert_eq!(progress(7, 3).percent(), 100);
}

// ---------------------------------------------------------------------------
// PermissionCheck
// ---------------------------------------------------------------------------

fn principal(user_id: Uuid) -> Principal {
    Principal {
        user_id,
        via: AuthVia::Test,
        token_abilities: None,
        session_id: None,
    }
}

/// Grants `permission` to `user`, app-wide or in `workspace`; answers only
/// for its own scope.
struct Grants {
    user: Uuid,
    permission: &'static str,
    workspace: Option<Uuid>,
}

impl PermissionCheck for Grants {
    fn check<'a>(
        &'a self,
        principal: &'a Principal,
        permission: &'a str,
        workspace: Option<Uuid>,
    ) -> BoxFuture<'a, Result<Option<bool>, BoxError>> {
        Box::pin(async move {
            if workspace != self.workspace {
                return Ok(None);
            }
            Ok(Some(
                principal.user_id == self.user && permission == self.permission,
            ))
        })
    }
}

/// Fails every check.
struct Broken;

impl PermissionCheck for Broken {
    fn check<'a>(
        &'a self,
        _principal: &'a Principal,
        _permission: &'a str,
        _workspace: Option<Uuid>,
    ) -> BoxFuture<'a, Result<Option<bool>, BoxError>> {
        Box::pin(async { Err("the permission store is down".into()) })
    }
}

struct ChecksModule {
    checks: Vec<Arc<dyn PermissionCheck>>,
}

impl Module for ChecksModule {
    fn register(&self, m: &mut ModuleBuilder) {
        for check in &self.checks {
            let check = check.clone();
            m.multi(Factory::<dyn PermissionCheck>::new(move |_| {
                let check = check.clone();
                async move { Ok(check) }
            }));
        }
    }
}

#[tokio::test]
async fn permitted_asks_every_check_and_any_grant_wins() {
    let ada = Uuid::now_v7();
    let bob = Uuid::now_v7();
    let acme = Uuid::now_v7();
    let app = boot(ChecksModule {
        checks: vec![
            Arc::new(Grants {
                user: ada,
                permission: "operations.view",
                workspace: None,
            }),
            Arc::new(Grants {
                user: bob,
                permission: "operations.view",
                workspace: Some(acme),
            }),
        ],
    })
    .await;
    let ctx = app.ctx();

    // App-wide grant.
    assert!(
        permitted(ctx, &principal(ada), "operations.view", None)
            .await
            .unwrap()
    );
    // Bob holds it in acme only.
    assert!(
        !permitted(ctx, &principal(bob), "operations.view", None)
            .await
            .unwrap()
    );
    assert!(
        permitted(ctx, &principal(bob), "operations.view", Some(acme))
            .await
            .unwrap()
    );
    // Nothing grants this one.
    assert!(
        !permitted(ctx, &principal(bob), "operations.cancel", Some(acme))
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn permitted_is_false_without_checks_and_errors_propagate() {
    let app = boot(EmptyModule).await;
    let nobody = principal(Uuid::now_v7());
    assert!(
        !permitted(app.ctx(), &nobody, "operations.view", None)
            .await
            .unwrap()
    );

    let app = boot(ChecksModule {
        checks: vec![Arc::new(Broken)],
    })
    .await;
    let error = permitted(app.ctx(), &nobody, "operations.view", None)
        .await
        .expect_err("a failing check is an error, not a denial");
    assert!(error.to_string().contains("down"), "{error}");
}
