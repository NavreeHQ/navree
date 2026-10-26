//! Conformity checks: modules contribute checks, `conformity:check` runs them
//! in one booted app, and companion commands (`i18n:check`, `migrate:lint`)
//! run when the app registers them.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use navree_contracts::{
    CONFORMITY_COMMAND, CheckOutcome, ConformityCheck, ConformityModule, ConformityOptions,
    ConformityReport, Finding, ModuleBuilderConformityExt, Severity,
};
use navree_kernel::{App, CommandSpec, Module, ModuleBuilder, Navree};

/// Contributes one passing and one failing check.
struct RoutesModule;

impl Module for RoutesModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.conformity_check(ConformityCheck::new(
            "demo.routes",
            "Every route is documented",
            |_cx| async { Ok(CheckOutcome::checked(3)) },
        ))
        .conformity_check(ConformityCheck::new(
            "demo.entities",
            "Every entity is classified",
            |_cx| async {
                Ok(CheckOutcome::checked(2).finding(
                    Finding::error("notes", "not classified").hint("register it in its module"),
                ))
            },
        ));
    }
}

/// Contributes a check that only warns, and one that skips.
struct OtherModule;

impl Module for OtherModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.conformity_check(ConformityCheck::new(
            "other.style",
            "Names follow the conventions",
            |cx| async move {
                let root = cx.root().display().to_string();
                Ok(CheckOutcome::checked(1).finding(Finding::warning(root, "odd name")))
            },
        ))
        .conformity_check(ConformityCheck::new(
            "other.tables",
            "Tables exist",
            |_cx| async { Ok(CheckOutcome::skipped("no database")) },
        ));
    }
}

/// A check whose run fails.
struct BrokenModule;

impl Module for BrokenModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.conformity_check(ConformityCheck::new(
            "broken.check",
            "Always fails to run",
            |_cx| async { Err("the database is down".into()) },
        ));
    }
}

/// Registers `i18n:check`, counting its runs and failing when told to.
struct I18nCommandModule {
    runs: Arc<AtomicUsize>,
    fail: bool,
}

impl Module for I18nCommandModule {
    fn register(&self, m: &mut ModuleBuilder) {
        let runs = self.runs.clone();
        let fail = self.fail;
        m.command(CommandSpec::new(
            "i18n:check",
            "Check translations",
            move |_ctx, _args| {
                let runs = runs.clone();
                async move {
                    runs.fetch_add(1, Ordering::SeqCst);
                    if fail {
                        Err("fr is missing deal-won".into())
                    } else {
                        Ok(())
                    }
                }
            },
        ));
    }
}

async fn boot(modules: Vec<Box<dyn Module>>) -> App {
    let mut builder = Navree::app();
    for module in modules {
        builder = builder.module(Boxed(module));
    }
    builder.boot().await.expect("the app boots")
}

struct Boxed(Box<dyn Module>);

impl Module for Boxed {
    fn id(&self) -> navree_kernel::ModuleId {
        self.0.id()
    }

    fn name(&self) -> &'static str {
        self.0.name()
    }

    fn register(&self, m: &mut ModuleBuilder) {
        self.0.register(m);
    }
}

fn options() -> ConformityOptions {
    ConformityOptions::new(PathBuf::from("app-root"))
}

#[tokio::test]
async fn contributed_checks_run_in_registration_order_with_their_outcomes() {
    let app = boot(vec![Box::new(RoutesModule), Box::new(OtherModule)]).await;
    let report = ConformityReport::run(app.ctx(), &options()).await;
    let names: Vec<_> = report.checks.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "demo.routes",
            "demo.entities",
            "other.style",
            "other.tables",
            "i18n.translations",
            "db.migrations"
        ]
    );
    assert_eq!(report.checks[0].outcome.checked, 3);
    assert!(report.checks[0].outcome.findings.is_empty());
    assert_eq!(
        report.checks[1].outcome.findings,
        vec![Finding::error("notes", "not classified").hint("register it in its module")]
    );
    // The source root reaches the checks.
    assert_eq!(report.checks[2].outcome.findings[0].subject, "app-root");
    assert_eq!(
        report.checks[2].outcome.findings[0].severity,
        Severity::Warning
    );
    assert_eq!(
        report.checks[3].outcome.skipped.as_deref(),
        Some("no database")
    );
    assert_eq!(report.errors(), 1);
    assert_eq!(report.warnings(), 1);
    assert!(!report.is_ok());
}

#[tokio::test]
async fn companion_commands_are_skipped_when_the_app_has_none() {
    let app = boot(vec![Box::new(RoutesModule)]).await;
    let report = ConformityReport::run(app.ctx(), &options()).await;
    let translations = report.check("i18n.translations").unwrap();
    assert!(
        translations
            .outcome
            .skipped
            .as_deref()
            .unwrap()
            .contains("i18n:check"),
        "{translations:?}"
    );
    let migrations = report.check("db.migrations").unwrap();
    assert!(
        migrations
            .outcome
            .skipped
            .as_deref()
            .unwrap()
            .contains("migrate:lint"),
        "{migrations:?}"
    );
}

#[tokio::test]
async fn companion_commands_run_when_registered() {
    let runs = Arc::new(AtomicUsize::new(0));
    let app = boot(vec![
        Box::new(RoutesModule),
        Box::new(I18nCommandModule {
            runs: runs.clone(),
            fail: false,
        }),
    ])
    .await;
    let report = ConformityReport::run(app.ctx(), &options()).await;
    assert_eq!(runs.load(Ordering::SeqCst), 1);
    let translations = report.check("i18n.translations").unwrap();
    assert_eq!(translations.outcome.skipped, None);
    assert!(translations.outcome.findings.is_empty());

    let runs = Arc::new(AtomicUsize::new(0));
    let app = boot(vec![
        Box::new(RoutesModule),
        Box::new(I18nCommandModule {
            runs: runs.clone(),
            fail: true,
        }),
    ])
    .await;
    let report = ConformityReport::run(app.ctx(), &options()).await;
    let translations = report.check("i18n.translations").unwrap();
    assert_eq!(
        translations.outcome.findings,
        vec![Finding::error("i18n:check", "fr is missing deal-won")]
    );
}

#[tokio::test]
async fn a_check_that_fails_to_run_is_an_error_finding() {
    let app = boot(vec![Box::new(BrokenModule)]).await;
    let report = ConformityReport::run(app.ctx(), &options()).await;
    let broken = report.check("broken.check").unwrap();
    assert_eq!(
        broken.outcome.findings,
        vec![Finding::error(
            "broken.check",
            "the check could not run: the database is down"
        )]
    );
    assert_eq!(report.errors(), 1);
}

#[tokio::test]
async fn only_selects_checks_by_name_or_prefix() {
    let app = boot(vec![Box::new(RoutesModule), Box::new(OtherModule)]).await;
    let report = ConformityReport::run(
        app.ctx(),
        &options().only(["demo.routes".to_owned(), "other".to_owned()]),
    )
    .await;
    let names: Vec<_> = report.checks.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["demo.routes", "other.style", "other.tables"]);
}

#[tokio::test]
async fn the_command_is_registered_once_and_fails_on_errors() {
    // Both modules import ConformityModule through `conformity_check`.
    let app = boot(vec![Box::new(RoutesModule), Box::new(OtherModule)]).await;
    let commands: Vec<_> = app
        .ctx()
        .commands()
        .into_iter()
        .filter(|c| c.name == CONFORMITY_COMMAND)
        .collect();
    assert_eq!(commands.len(), 1);
    let error = app
        .run_command(CONFORMITY_COMMAND, vec!["--root".into(), "app-root".into()])
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "conformity: 1 error, 1 warning");

    // Selecting only clean checks passes.
    app.run_command(
        CONFORMITY_COMMAND,
        vec!["--only".into(), "demo.routes,other.tables".into()],
    )
    .await
    .unwrap();
    // `--json` prints the report as JSON and keeps the exit status.
    app.run_command(
        CONFORMITY_COMMAND,
        vec!["--json".into(), "--only=demo.routes".into()],
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn unknown_arguments_are_refused() {
    let app = boot(vec![Box::new(ConformityModule)]).await;
    let error = app
        .run_command(CONFORMITY_COMMAND, vec!["--fast".into()])
        .await
        .unwrap_err();
    assert!(error.to_string().contains("--fast"), "{error}");
}

#[tokio::test]
async fn the_report_reads_as_text_and_json() {
    let app = boot(vec![Box::new(RoutesModule), Box::new(OtherModule)]).await;
    let report = ConformityReport::run(app.ctx(), &options()).await;
    let text = report.to_string();
    assert!(text.starts_with("Conformity checks\n"), "{text}");
    let routes = text
        .lines()
        .find(|line| line.starts_with("  ok    demo.routes "))
        .unwrap_or_else(|| panic!("{text}"));
    assert!(
        routes.ends_with("  Every route is documented (3 checked)"),
        "{text}"
    );
    assert!(text.contains("FAIL  demo.entities"), "{text}");
    assert!(text.contains("error  notes: not classified"), "{text}");
    assert!(text.contains("hint: register it in its module"), "{text}");
    assert!(text.contains("warn  other.style"), "{text}");
    assert!(text.contains("skip  other.tables"), "{text}");
    assert!(text.contains("no database"), "{text}");
    assert!(text.ends_with("1 error, 1 warning\n"), "{text}");

    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["errors"], 1);
    assert_eq!(json["warnings"], 1);
    assert_eq!(json["checks"][1]["name"], "demo.entities");
    assert_eq!(json["checks"][1]["status"], "fail");
    assert_eq!(json["checks"][1]["findings"][0]["severity"], "error");
    assert_eq!(
        json["checks"][1]["findings"][0]["hint"],
        "register it in its module"
    );
    assert_eq!(json["checks"][3]["status"], "skip");
    assert_eq!(json["checks"][3]["skipped"], "no database");
}

#[test]
fn outcome_counts_errors_and_warnings() {
    let outcome = CheckOutcome::checked(4).findings([
        Finding::error("a", "x"),
        Finding::warning("b", "y"),
        Finding::error("c", "z"),
    ]);
    assert_eq!(outcome.errors(), 2);
    assert_eq!(outcome.warnings(), 1);
}
