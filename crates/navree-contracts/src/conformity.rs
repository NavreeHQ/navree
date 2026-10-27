//! Conformity checks: what `navree check` verifies about an app beyond its
//! module graph.
//!
//! A module contributes a [`ConformityCheck`] with
//! [`ModuleBuilderConformityExt::conformity_check`] (navree-api checks that
//! every route is documented, navree-workspace that every entity is
//! classified and reached through `WorkspaceRepo`). Doing so imports
//! [`ConformityModule`], which registers the command `conformity:check`: it
//! boots the app once, runs every check and prints a [`ConformityReport`].
//! Besides the contributed checks, the report runs two companion commands
//! when the app registers them: `i18n:check` (translations complete) and
//! `migrate:lint` (migrations linted).
//!
//! ```
//! use navree_contracts::{
//!     CheckOutcome, ConformityCheck, Finding, ModuleBuilderConformityExt,
//! };
//! use navree_kernel::{Module, ModuleBuilder};
//!
//! struct BillingModule;
//!
//! impl Module for BillingModule {
//!     fn register(&self, m: &mut ModuleBuilder) {
//!         m.conformity_check(ConformityCheck::new(
//!             "billing.plans",
//!             "Every plan has a price",
//!             |_cx| async {
//!                 Ok(CheckOutcome::checked(3)
//!                     .finding(Finding::error("plan `team`", "has no price").hint("set billing.plans.team.price")))
//!             },
//!         ));
//!     }
//! }
//! ```
//!
//! A check never runs on the request path: it costs a registration until
//! `conformity:check` runs.

use std::fmt;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use navree_kernel::{AppContext, BoxError, BoxFuture, CommandSpec, Module, ModuleBuilder};
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize, Serializer};

/// The command that runs every conformity check: `conformity:check [--json]
/// [--root <dir>] [--only <check,...>]`. `navree check` runs it after the
/// module graph check.
pub const CONFORMITY_COMMAND: &str = "conformity:check";

/// How serious a [`Finding`] is: errors fail `conformity:check` (and
/// `navree check`), warnings are reported only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Reported, does not fail the check.
    Warning,
    /// Fails the check.
    Error,
}

impl Severity {
    /// `warning` or `error`.
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Warning => "warning",
            Severity::Error => "error",
        }
    }
}

/// One problem a check found.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    /// Error or warning.
    pub severity: Severity,
    /// What it is about: a route (`GET /api/v1/deals`), a table, a file and
    /// line (`src/deals/service.rs:42`).
    pub subject: String,
    /// What is wrong, in a sentence without a trailing period.
    pub message: String,
    /// How to fix it, when there is a usual fix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

impl Finding {
    /// An error about `subject`.
    pub fn error(subject: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            subject: subject.into(),
            message: message.into(),
            hint: None,
        }
    }

    /// A warning about `subject`.
    pub fn warning(subject: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            ..Self::error(subject, message)
        }
    }

    /// Adds how to fix it.
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

/// What one check found.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckOutcome {
    /// How many items the check looked at (routes, entities, files).
    pub checked: usize,
    /// The problems found.
    pub findings: Vec<Finding>,
    /// Why the check did not run, when it did not (the app has no
    /// workspaces, the isolation strategy makes it moot, ...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skipped: Option<String>,
}

impl CheckOutcome {
    /// `n` items looked at, nothing found yet.
    pub fn checked(n: usize) -> Self {
        Self {
            checked: n,
            ..Self::default()
        }
    }

    /// The check did not run, for `reason`.
    pub fn skipped(reason: impl Into<String>) -> Self {
        Self {
            skipped: Some(reason.into()),
            ..Self::default()
        }
    }

    /// Adds `finding`.
    pub fn finding(mut self, finding: Finding) -> Self {
        self.findings.push(finding);
        self
    }

    /// Adds every finding of `findings`.
    pub fn findings(mut self, findings: impl IntoIterator<Item = Finding>) -> Self {
        self.findings.extend(findings);
        self
    }

    /// The number of error findings.
    pub fn errors(&self) -> usize {
        self.count(Severity::Error)
    }

    /// The number of warning findings.
    pub fn warnings(&self) -> usize {
        self.count(Severity::Warning)
    }

    fn count(&self, severity: Severity) -> usize {
        self.findings
            .iter()
            .filter(|f| f.severity == severity)
            .count()
    }
}

/// What a check runs with: the booted app and the root of the app's source
/// (`--root`, default the working directory, which `navree check` sets to
/// the app's directory).
#[derive(Clone)]
pub struct CheckContext {
    app: AppContext,
    root: PathBuf,
}

impl CheckContext {
    /// A context for `app` with source root `root`.
    pub fn new(app: AppContext, root: impl Into<PathBuf>) -> Self {
        Self {
            app,
            root: root.into(),
        }
    }

    /// The booted app.
    pub fn app(&self) -> &AppContext {
        &self.app
    }

    /// The app's source root.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl fmt::Debug for CheckContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CheckContext")
            .field("root", &self.root)
            .finish_non_exhaustive()
    }
}

/// The function behind a [`ConformityCheck`].
type CheckFn =
    Arc<dyn Fn(CheckContext) -> BoxFuture<'static, Result<CheckOutcome, BoxError>> + Send + Sync>;

/// A named check `conformity:check` runs. Contribute it with
/// [`ModuleBuilderConformityExt::conformity_check`].
#[derive(Clone)]
pub struct ConformityCheck {
    name: &'static str,
    about: &'static str,
    run: CheckFn,
}

impl ConformityCheck {
    /// A check named `name` (dotted, the owning section first:
    /// `api.docs`, `workspace.entities`), described by `about` (what holds
    /// when it passes), running `f`. An `Err` from `f` is reported as an
    /// error finding ("the check could not run: ...").
    pub fn new<F, Fut>(name: &'static str, about: &'static str, f: F) -> Self
    where
        F: Fn(CheckContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<CheckOutcome, BoxError>> + Send + 'static,
    {
        Self {
            name,
            about,
            run: Arc::new(move |cx| Box::pin(f(cx))),
        }
    }

    /// A check that runs the app's command `command` (no arguments) when
    /// the app registers it: success passes, an error is one error finding.
    /// Without the command the check is skipped with reason `missing`.
    pub fn command(
        name: &'static str,
        about: &'static str,
        command: &'static str,
        missing: &'static str,
    ) -> Self {
        Self::new(name, about, move |cx| async move {
            let Some(spec) = cx.app().commands().into_iter().find(|c| c.name == command) else {
                return Ok(CheckOutcome::skipped(missing));
            };
            Ok(match (spec.run)(cx.app().clone(), Vec::new()).await {
                Ok(()) => CheckOutcome::checked(1),
                Err(error) => {
                    CheckOutcome::checked(1).finding(Finding::error(command, error.to_string()))
                }
            })
        })
    }

    /// The check's name.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// What holds when the check passes.
    pub fn about(&self) -> &'static str {
        self.about
    }

    /// Runs the check.
    pub async fn run(&self, cx: CheckContext) -> Result<CheckOutcome, BoxError> {
        (self.run)(cx).await
    }
}

impl fmt::Debug for ConformityCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConformityCheck")
            .field("name", &self.name)
            .field("about", &self.about)
            .finish_non_exhaustive()
    }
}

/// The checks every report ends with: companion commands other modules
/// register.
fn companion_checks() -> [ConformityCheck; 2] {
    [
        ConformityCheck::command(
            "i18n.translations",
            "Every message is translated in every locale",
            "i18n:check",
            "the app has no `i18n:check` command (the localization module registers it)",
        ),
        ConformityCheck::command(
            "db.migrations",
            "Pending migrations pass the migration lint",
            "migrate:lint",
            "the app has no `migrate:lint` command",
        ),
    ]
}

/// Conformity registrations on a [`ModuleBuilder`].
pub trait ModuleBuilderConformityExt {
    /// Contributes `check` to `conformity:check`, and imports
    /// [`ConformityModule`] (which registers the command once per app).
    fn conformity_check(&mut self, check: ConformityCheck) -> &mut Self;
}

impl ModuleBuilderConformityExt for ModuleBuilder {
    fn conformity_check(&mut self, check: ConformityCheck) -> &mut Self {
        self.import(ConformityModule).contribute(check)
    }
}

/// Registers the command `conformity:check`. Modules that contribute checks
/// import it; add it to an app yourself only to run the companion checks.
#[derive(Clone, Copy, Debug, Default)]
pub struct ConformityModule;

impl Module for ConformityModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.command(CommandSpec::new(
            CONFORMITY_COMMAND,
            "Run the conformity checks ([--json] [--root <dir>] [--only <check,...>])",
            command,
        ));
    }
}

/// What [`ConformityReport::run`] runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConformityOptions {
    /// The app's source root, given to every check.
    pub root: PathBuf,
    /// Only the checks named here, or whose name starts with an entry and a
    /// dot (`workspace` selects `workspace.entities`); every check when
    /// empty.
    pub only: Vec<String>,
}

impl ConformityOptions {
    /// Every check, with source root `root`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            only: Vec::new(),
        }
    }

    /// Only the checks `names` select.
    pub fn only(mut self, names: impl IntoIterator<Item = String>) -> Self {
        self.only.extend(names);
        self
    }

    fn selects(&self, name: &str) -> bool {
        self.only.is_empty()
            || self.only.iter().any(|only| {
                name == only
                    || name
                        .strip_prefix(only.as_str())
                        .is_some_and(|rest| rest.starts_with('.'))
            })
    }
}

/// How a check ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    /// No findings.
    Ok,
    /// Warnings only.
    Warn,
    /// At least one error.
    Fail,
    /// Not run.
    Skip,
}

impl CheckStatus {
    fn of(outcome: &CheckOutcome) -> Self {
        if outcome.errors() > 0 {
            CheckStatus::Fail
        } else if outcome.warnings() > 0 {
            CheckStatus::Warn
        } else if outcome.skipped.is_some() {
            CheckStatus::Skip
        } else {
            CheckStatus::Ok
        }
    }

    fn label(self) -> &'static str {
        match self {
            CheckStatus::Ok => "ok",
            CheckStatus::Warn => "warn",
            CheckStatus::Fail => "FAIL",
            CheckStatus::Skip => "skip",
        }
    }
}

/// One check's result in a [`ConformityReport`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CheckReport {
    /// The check's name.
    pub name: String,
    /// What holds when it passes.
    pub about: String,
    /// How it ended.
    pub status: CheckStatus,
    /// What it found.
    #[serde(flatten)]
    pub outcome: CheckOutcome,
}

/// The result of every selected check, in order: contributed checks in
/// module order, then the companion commands.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConformityReport {
    /// One entry per check.
    pub checks: Vec<CheckReport>,
}

impl ConformityReport {
    /// Runs every check `options` selects against `app`, one after another.
    pub async fn run(app: &AppContext, options: &ConformityOptions) -> Self {
        let contributed = app
            .contributions::<ConformityCheck>()
            .into_iter()
            .map(|c| (*c.value).clone());
        let mut checks = Vec::new();
        for check in contributed.chain(companion_checks()) {
            if !options.selects(check.name) {
                continue;
            }
            let cx = CheckContext::new(app.clone(), options.root.clone());
            let outcome = match check.run(cx).await {
                Ok(outcome) => outcome,
                Err(error) => CheckOutcome::default().finding(Finding::error(
                    check.name,
                    format!("the check could not run: {error}"),
                )),
            };
            checks.push(CheckReport {
                name: check.name.to_owned(),
                about: check.about.to_owned(),
                status: CheckStatus::of(&outcome),
                outcome,
            });
        }
        Self { checks }
    }

    /// The result of check `name`.
    pub fn check(&self, name: &str) -> Option<&CheckReport> {
        self.checks.iter().find(|c| c.name == name)
    }

    /// Error findings across every check.
    pub fn errors(&self) -> usize {
        self.checks.iter().map(|c| c.outcome.errors()).sum()
    }

    /// Warning findings across every check.
    pub fn warnings(&self) -> usize {
        self.checks.iter().map(|c| c.outcome.warnings()).sum()
    }

    /// Whether no check found an error.
    pub fn is_ok(&self) -> bool {
        self.errors() == 0
    }

    /// `1 error, 2 warnings`.
    pub fn summary(&self) -> String {
        format!(
            "{}, {}",
            plural(self.errors(), "error"),
            plural(self.warnings(), "warning")
        )
    }
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

impl Serialize for ConformityReport {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut report = serializer.serialize_struct("ConformityReport", 3)?;
        report.serialize_field("checks", &self.checks)?;
        report.serialize_field("errors", &self.errors())?;
        report.serialize_field("warnings", &self.warnings())?;
        report.end()
    }
}

/// The report as text: one line per check, its findings under it, and the
/// totals.
impl fmt::Display for ConformityReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Conformity checks")?;
        let width = self.checks.iter().map(|c| c.name.len()).max().unwrap_or(0);
        for check in &self.checks {
            let mut about = check.about.clone();
            match &check.outcome.skipped {
                Some(reason) => {
                    about.push_str(": ");
                    about.push_str(reason);
                }
                None => about.push_str(&format!(" ({} checked)", check.outcome.checked)),
            }
            writeln!(
                f,
                "  {:<6}{:<width$}  {about}",
                check.status.label(),
                check.name
            )?;
            for finding in &check.outcome.findings {
                let label = finding.severity.as_str();
                writeln!(
                    f,
                    "          {label}  {}: {}",
                    finding.subject, finding.message
                )?;
                if let Some(hint) = &finding.hint {
                    let pad = " ".repeat(label.len() + 2);
                    writeln!(f, "          {pad}hint: {hint}")?;
                }
            }
        }
        writeln!(f, "{}", self.summary())
    }
}

/// `conformity:check [--json] [--root <dir>] [--only <check,...>]`.
async fn command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let mut json = false;
    let mut root: Option<PathBuf> = None;
    let mut only = Vec::new();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((flag, value)) if flag.starts_with("--") => {
                (flag.to_owned(), Some(value.to_owned()))
            }
            _ => (arg.clone(), None),
        };
        let mut value = |name: &str| {
            inline
                .clone()
                .or_else(|| args.next())
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match flag.as_str() {
            "--json" => json = true,
            "--root" => root = Some(PathBuf::from(value("--root")?)),
            "--only" => only.extend(
                value("--only")?
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned),
            ),
            _ => {
                return Err(format!(
                    "unknown argument `{arg}` ({CONFORMITY_COMMAND} [--json] [--root <dir>] [--only <check,...>])"
                )
                .into());
            }
        }
    }
    let root = match root {
        Some(root) => root,
        None => std::env::current_dir()?,
    };
    let report = ConformityReport::run(&ctx, &ConformityOptions::new(root).only(only)).await;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print!("{report}");
    }
    if report.is_ok() {
        Ok(())
    } else {
        Err(format!("conformity: {}", report.summary()).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_matches_whole_names_and_dotted_prefixes() {
        let options =
            ConformityOptions::new(".").only(["api".to_owned(), "workspace.repo".to_owned()]);
        assert!(options.selects("api.docs"));
        assert!(options.selects("workspace.repo"));
        assert!(!options.selects("apis.docs"));
        assert!(!options.selects("workspace.entities"));
        assert!(ConformityOptions::new(".").selects("anything"));
    }

    #[test]
    fn status_follows_the_worst_finding() {
        assert_eq!(CheckStatus::of(&CheckOutcome::checked(1)), CheckStatus::Ok);
        assert_eq!(
            CheckStatus::of(&CheckOutcome::skipped("no database")),
            CheckStatus::Skip
        );
        assert_eq!(
            CheckStatus::of(&CheckOutcome::checked(1).finding(Finding::warning("a", "b"))),
            CheckStatus::Warn
        );
        assert_eq!(
            CheckStatus::of(
                &CheckOutcome::checked(1)
                    .finding(Finding::warning("a", "b"))
                    .finding(Finding::error("c", "d"))
            ),
            CheckStatus::Fail
        );
    }

    #[test]
    fn summary_pluralizes() {
        let report = ConformityReport::default();
        assert_eq!(report.summary(), "0 errors, 0 warnings");
    }
}
