//! `Schedule::command`: due ticks run a registered CLI command in-process.

use std::sync::{Arc, Mutex};

use keel_kernel::{AppContext, BoxError, CommandSpec, Module, ModuleBuilder};
use keel_schedule::RunStatus;

use crate::support::{Tasks, TestDb, at, clock, scheduler};

/// The arguments each `reports:build` run received.
#[derive(Default)]
struct Calls(Mutex<Vec<Vec<String>>>);

/// Registers `reports:build` (records its arguments; fails on `--fail`).
struct ReportsModule;

impl Module for ReportsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.command(CommandSpec::new(
            "reports:build",
            "Build the reports",
            |ctx: AppContext, args: Vec<String>| async move {
                let calls = ctx.get::<Calls>().await?;
                let fail = args.iter().any(|a| a == "--fail");
                calls.0.lock().unwrap().push(args);
                if fail {
                    return Err::<(), BoxError>("report build failed".into());
                }
                Ok(())
            },
        ));
    }
}

#[tokio::test]
async fn command_task_runs_registered_command() {
    let db = TestDb::new();
    let calls = Arc::new(Calls::default());
    let app = db
        .app(
            &clock("2026-10-25T06:00:00Z"),
            Tasks::new(|s| {
                s.command("reports:build", &["--fast"]).hourly();
                s.command("reports:build", &["--fail"]).hourly();
            }),
        )
        .module(ReportsModule)
        .override_value(calls.clone())
        .boot()
        .await
        .expect("boots");

    let mut outcomes = scheduler(&app)
        .await
        .tick(at("2026-10-25T06:00:00Z"))
        .await
        .unwrap();
    outcomes.sort_by(|a, b| a.task.cmp(&b.task));
    assert_eq!(outcomes[0].task, "reports:build --fail");
    assert_eq!(
        outcomes[0].status,
        RunStatus::Failed("report build failed".to_owned())
    );
    assert_eq!(outcomes[1].task, "reports:build --fast");
    assert_eq!(outcomes[1].status, RunStatus::Succeeded);

    let mut seen = calls.0.lock().unwrap().clone();
    seen.sort();
    assert_eq!(seen, [vec!["--fail".to_owned()], vec!["--fast".to_owned()]]);
}

#[tokio::test]
async fn scheduling_an_unknown_command_fails_the_boot() {
    let db = TestDb::new();
    let error = db
        .app(
            &clock("2026-10-25T06:00:00Z"),
            Tasks::new(|s| {
                s.command("reports:nope", &[]).daily();
            }),
        )
        .boot()
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("`reports:nope`") && error.contains("no module registers"),
        "{error}"
    );
}
