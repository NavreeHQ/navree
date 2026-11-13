//! `navree native:coverage`: how much of the app a native client gets as
//! screens that belong to its system.
//!
//! The app does the asking (its `native:coverage` command, from the UI
//! toolkit's module, asks every page route through its own router, with no
//! browser or device); this reads `native.toml` for the example values of
//! route parameters, builds the app with the cargo feature the preview needs,
//! prints the answer as a table or as JSON, and decides the exit code for a
//! gate (`--fail-on`).

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};

use crate::check::app_commands;
use crate::forward::{Runner, forward_with};
use crate::native::{self, Preview};

/// The app's command.
pub const COMMAND: &str = "native:coverage";

/// What the app's report may say about itself.
const FORMAT: &str = "navree-native-coverage";

/// The newest report this reads.
const VERSION: u32 = 1;

/// The grade at which `--fail-on` fails.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum FailOn {
    /// Fail when any page is `layout` or `web`, or is a screen written for
    /// native clients (`designed`) that has something to fix.
    Layout,
    /// Fail when any page is `web`.
    Web,
}

/// What `navree native:coverage` was asked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoverageOptions {
    /// The system to ask as (`ios`, `android`, ...); the app's default is
    /// Android.
    pub platform: Option<String>,
    /// The kind of device to ask as (`phone`, `tablet`, ...); the app's
    /// default is a phone.
    pub form: Option<String>,
    /// Print JSON instead of the table.
    pub json: bool,
    /// Fail when a page has this grade or a worse one.
    pub fail_on: Option<FailOn>,
    /// Cargo features to build the app with, instead of the one found in its
    /// manifest.
    pub features: Vec<String>,
    /// The file of example values; default `native.toml` in the app.
    pub file: Option<PathBuf>,
    /// Environment variables for the app (its Dock settings).
    pub env: Vec<(String, String)>,
}

/// One page, as the app asked it.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Page {
    /// The route's pattern, or the address for an extra one.
    pub route: String,
    /// The address that was asked.
    pub path: String,
    /// `idiom`, `designed`, `layout` or `web`.
    pub grade: String,
    /// Whether the client got native parts.
    pub native: bool,
    /// What kept the page in the web view (the readers' codes).
    #[serde(default)]
    pub why: Vec<String>,
    /// What keeps a native screen from belonging to its system: from being
    /// idiom when it was read from a page, from being sound when it was
    /// written for native clients.
    #[serde(default)]
    pub missing: Vec<String>,
    /// Decoration left out of the screen.
    #[serde(default)]
    pub dropped: Vec<String>,
    /// The page's size as HTML.
    #[serde(default)]
    pub html_bytes: usize,
    /// The native answer's size.
    #[serde(default)]
    pub envelope_bytes: usize,
    /// What the page lacks, in words (added here).
    #[serde(default)]
    pub said: Vec<String>,
}

/// A route that could not be read as a page.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Skipped {
    /// The route's pattern.
    pub route: String,
    /// The address tried, when there was one.
    #[serde(default)]
    pub path: Option<String>,
    /// Why it was not read.
    pub reason: String,
}

/// Routes that are not pages, by what they are.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct NotPages {
    /// What they are.
    pub reason: String,
    /// How many.
    pub count: usize,
}

/// The pages by grade.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Totals {
    /// Pages asked.
    pub pages: usize,
    /// Pages that belong to their system.
    pub idiom: usize,
    /// Screens written for native clients, not read from a page.
    pub designed: usize,
    /// Pages of native parts in a web-shaped layout.
    pub layout: usize,
    /// Pages shown in the web view.
    pub web: usize,
    /// Routes that could not be read.
    pub skipped: usize,
}

/// The app's report, with the totals and the gate.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Report {
    /// The format's name.
    pub format: String,
    /// Its version.
    pub version: u32,
    /// The system asked as.
    pub platform: String,
    /// The device asked as.
    pub form: String,
    /// Every page, graded.
    pub pages: Vec<Page>,
    /// Routes that could not be read.
    #[serde(default)]
    pub skipped: Vec<Skipped>,
    /// Routes that are not pages.
    #[serde(default)]
    pub not_pages: Vec<NotPages>,
    /// The pages by grade (added here).
    #[serde(default, skip_deserializing)]
    pub totals: Totals,
    /// The grade at which the gate fails, when there is one (added here).
    #[serde(default, skip_deserializing)]
    pub fail_on: Option<String>,
    /// Whether the gate holds (added here).
    #[serde(default, skip_deserializing)]
    pub passed: bool,
}

impl Report {
    /// Reads the JSON the app printed: the report is its last line that
    /// starts a JSON object, so a line of log before it does no harm.
    pub fn read(output: &str) -> anyhow::Result<Self> {
        let line = output
            .lines()
            .rev()
            .find(|line| line.trim_start().starts_with('{'))
            .context("the app printed no report")?;
        let report: Self = serde_json::from_str(line.trim())
            .context("the app's report is not one this navree reads")?;
        anyhow::ensure!(
            report.format == FORMAT,
            "the app's report is `{}`, not `{FORMAT}`",
            report.format
        );
        anyhow::ensure!(
            report.version <= VERSION,
            "the app's report is version {}, newer than this navree reads ({VERSION}); update navree",
            report.version
        );
        Ok(report)
    }

    /// Words for each page, the totals, and the gate.
    fn finish(mut self, fail_on: Option<FailOn>, file: &str) -> Self {
        for page in &mut self.pages {
            page.said = if page.native {
                page.missing.clone()
            } else {
                page.why.iter().map(|code| native::say(code)).collect()
            };
        }
        for skipped in &mut self.skipped {
            skipped.reason = with_the_line_to_add(&skipped.reason, file);
        }
        let count = |grade: &str| self.pages.iter().filter(|p| p.grade == grade).count();
        self.totals = Totals {
            pages: self.pages.len(),
            idiom: count("idiom"),
            designed: count("designed"),
            layout: count("layout"),
            web: count("web"),
            skipped: self.skipped.len(),
        };
        self.fail_on = fail_on.map(|f| {
            f.to_possible_value()
                .map_or_else(String::new, |v| v.get_name().to_owned())
        });
        self.passed = self.refusal(fail_on).is_none();
        self
    }

    /// Why the gate fails, if it does.
    pub fn refusal(&self, fail_on: Option<FailOn>) -> Option<String> {
        let totals = &self.totals;
        // A screen written for native clients holds the gate unless it has
        // something to fix.
        let unsound = self
            .pages
            .iter()
            .filter(|p| p.grade == "designed" && !p.missing.is_empty())
            .count();
        let (bad, name) = match fail_on? {
            FailOn::Web => (totals.web, "web"),
            FailOn::Layout if unsound == 0 => (totals.web + totals.layout, "layout or web"),
            FailOn::Layout => (
                totals.web + totals.layout + unsound,
                "layout, web or designed with something to fix",
            ),
        };
        if totals.pages == 0 {
            return Some(
                "no page was asked, so there is nothing to hold the gate to; see the skipped routes"
                    .to_owned(),
            );
        }
        (bad > 0).then(|| {
            let routes: Vec<&str> = self
                .pages
                .iter()
                .filter(|p| match fail_on {
                    Some(FailOn::Layout) => {
                        !(p.grade == "idiom" || (p.grade == "designed" && p.missing.is_empty()))
                    }
                    _ => p.grade == "web",
                })
                .map(|p| p.path.as_str())
                .collect();
            format!(
                "{bad} of {} pages are {name}: {}",
                totals.pages,
                routes.join(", ")
            )
        })
    }
}

/// The reason with, for a missing parameter, the line to add to the file.
fn with_the_line_to_add(reason: &str, file: &str) -> String {
    let Some(names) = reason.strip_prefix("needs a value for ") else {
        return reason.to_owned();
    };
    let lines: Vec<String> = names
        .split(", ")
        .map(|name| format!("{} = ...", name.trim_matches(['{', '}'])))
        .collect();
    format!(
        "{reason}; add {} under [coverage.params] in {file}",
        lines
            .iter()
            .map(|line| format!("`{line}`"))
            .collect::<Vec<_>>()
            .join(" and ")
    )
}

fn pad(text: &str, width: usize) -> String {
    let len = text.chars().count();
    format!("{text}{}", " ".repeat(width.saturating_sub(len)))
}

/// The report as text for a person.
pub fn render(report: &Report, app: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Native coverage of {app}, asked as {} on {}\n\n",
        report.platform, report.form
    ));
    if report.pages.is_empty() {
        out.push_str("No page was asked.\n");
    } else {
        let route_width = report
            .pages
            .iter()
            .map(|p| p.path.chars().count())
            .max()
            .unwrap_or(5)
            .max("ROUTE".len());
        // As wide as the longest grade: `designed` where an app has such screens.
        let grade_width = report
            .pages
            .iter()
            .map(|p| p.grade.chars().count())
            .max()
            .unwrap_or(6)
            .max(6);
        out.push_str(&format!(
            "{}  {}  WHAT IS MISSING\n",
            pad("ROUTE", route_width),
            pad("GRADE", grade_width)
        ));
        for page in &report.pages {
            let line = format!(
                "{}  {}  {}",
                pad(&page.path, route_width),
                pad(&page.grade, grade_width),
                page.said.join("; ")
            );
            out.push_str(line.trim_end());
            out.push('\n');
        }
    }
    let totals = &report.totals;
    // Screens written for native clients are counted where an app has some.
    let designed = match totals.designed {
        0 => String::new(),
        count => format!("{count} designed, "),
    };
    out.push_str(&format!(
        "\n{} idiom, {designed}{} layout, {} web: {} of {} pages are native screens\n",
        totals.idiom,
        totals.layout,
        totals.web,
        totals.idiom + totals.designed + totals.layout,
        totals.pages
    ));
    if !report.skipped.is_empty() {
        out.push_str(&format!("\nSkipped ({}):\n", report.skipped.len()));
        for skipped in &report.skipped {
            let at = match &skipped.path {
                Some(path) if *path != skipped.route => format!("{} ({path})", skipped.route),
                _ => skipped.route.clone(),
            };
            out.push_str(&format!("  {at}: {}\n", skipped.reason));
        }
    }
    if !report.not_pages.is_empty() {
        let parts: Vec<String> = report
            .not_pages
            .iter()
            .map(|n| format!("{} ({})", n.reason, n.count))
            .collect();
        out.push_str(&format!(
            "\nNot pages, so not asked: {}.\n",
            parts.join("; ")
        ));
    }
    out
}

/// What to do when the app has no `native:coverage` command.
fn missing_command(preview: &Preview) -> String {
    match preview {
        Preview::Off => "this app does not turn the native preview on, so it has no `native:coverage` command. \
Add `\"ui-native-preview\"` to the features of the `navree` dependency in Cargo.toml, \
or a feature of the app that turns it on (`native-preview = [\"navree/ui-native-preview\"]`), and run this again; \
`navree native:doctor` lists everything a native build needs"
            .to_owned(),
        _ => "the app has no `native:coverage` command: the native preview is on in its manifest, \
so check that the app names `UiModule` (and `HttpModule`) in its modules; \
`navree native:doctor` lists everything a native build needs"
            .to_owned(),
    }
}

/// Runs `navree native:coverage` for the app in `root`; whether the gate
/// holds (always, without `--fail-on`).
pub fn run_coverage(
    root: &Path,
    options: &CoverageOptions,
    runner: &mut dyn Runner,
    out: &mut dyn Write,
) -> anyhow::Result<bool> {
    let path = options
        .file
        .clone()
        .map(|file| {
            if file.is_absolute() {
                file
            } else {
                root.join(file)
            }
        })
        .unwrap_or_else(|| native::path_in(root));
    let plan = native::read(&path)?;
    let file_name = path
        .strip_prefix(root)
        .unwrap_or(&path)
        .display()
        .to_string()
        .replace('\\', "/");
    let preview = native::preview(root);
    let features = if options.features.is_empty() {
        match &preview {
            Preview::Feature(feature) => vec![feature.clone()],
            _ => Vec::new(),
        }
    } else {
        options.features.clone()
    };
    let asking = |command: &str, args: Vec<String>| {
        forward_with(&features, command, &args).with_env(options.env.clone())
    };
    let (ok, help) = runner.capture(&asking("help", Vec::new()), root)?;
    if !ok {
        bail!("cannot list the app's commands (does it build?)");
    }
    if !app_commands(&help).iter().any(|c| c == COMMAND) {
        bail!("{}", missing_command(&preview));
    }
    let mut args = Vec::new();
    for (flag, value) in [("--platform", &options.platform), ("--form", &options.form)] {
        if let Some(value) = value {
            args.push(flag.to_owned());
            args.push(value.clone());
        }
    }
    args.push("--plan".to_owned());
    args.push(plan.json());
    let (ok, output) = runner.capture(&asking(COMMAND, args), root)?;
    if !ok {
        bail!("`{COMMAND}` failed in the app; its messages are above");
    }
    let report = Report::read(&output)?.finish(options.fail_on, &file_name);
    if options.json {
        writeln!(out, "{}", serde_json::to_string_pretty(&report)?)?;
    } else {
        let app = crate::project::package_name(root).unwrap_or_else(|| "the app".to_owned());
        write!(out, "{}", render(&report, &app))?;
    }
    if let Some(reason) = report.refusal(options.fail_on) {
        if !options.json {
            writeln!(out, "\nnative:coverage: {reason}")?;
        }
        return Ok(false);
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::tests::FakeRunner;

    const HELP: &str = "Usage: <command>\n\nCommands:\n  serve  Serve HTTP\n  native:coverage  Ask every page route\n";

    fn report_json() -> String {
        serde_json::json!({
            "format": "navree-native-coverage", "version": 1,
            "platform": "android", "form": "phone",
            "pages": [
                {"route": "/", "path": "/", "grade": "idiom", "native": true,
                 "why": [], "missing": [], "dropped": [], "html_bytes": 900, "envelope_bytes": 300},
                {"route": "/orders/{id}", "path": "/orders/1", "grade": "layout", "native": true,
                 "why": [], "missing": ["the orders table has no title column", "the action “Export” has no role"],
                 "dropped": [], "html_bytes": 5000, "envelope_bytes": 2000},
                {"route": "/board", "path": "/board", "grade": "web", "native": false,
                 "why": ["tag:canvas"], "missing": [], "dropped": [], "html_bytes": 1000, "envelope_bytes": 90}
            ],
            "skipped": [
                {"route": "/orders/{id}/lines/{line}", "path": null, "reason": "needs a value for {line}"},
                {"route": "/me", "path": "/me", "reason": "answered 303 to /sign-in: signed out; give a session in `[coverage.headers]`"}
            ],
            "not_pages": [{"reason": "not a GET", "count": 4}, {"reason": "routes under the API prefix", "count": 2}]
        })
        .to_string()
    }

    fn app_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"shop\"\nversion = \"0.1.0\"\n[features]\nnative-preview = [\"navree/ui-native-preview\"]\n[dependencies]\nnavree = \"1\"\n",
        )
        .unwrap();
        dir
    }

    fn run(options: &CoverageOptions, runner: &mut FakeRunner, dir: &Path) -> (bool, String) {
        let mut out = Vec::new();
        let held = run_coverage(dir, options, runner, &mut out).unwrap();
        (held, String::from_utf8(out).unwrap())
    }

    fn scripted() -> FakeRunner {
        FakeRunner::default()
            .answer(
                "cargo run --quiet --features native-preview -- help",
                true,
                HELP,
            )
            .answer(
                "cargo run --quiet --features native-preview -- native:coverage",
                true,
                &format!("some log line\n{}\n", report_json()),
            )
    }

    #[test]
    fn the_table_lists_each_route_with_its_grade() {
        let dir = app_dir();
        let (held, out) = run(&CoverageOptions::default(), &mut scripted(), dir.path());
        assert!(held);
        let expected = "\
Native coverage of shop, asked as android on phone

ROUTE      GRADE   WHAT IS MISSING
/          idiom
/orders/1  layout  the orders table has no title column; the action “Export” has no role
/board     web     the page has a <canvas> element, which has no native part; keep it for the web with native::web_only, or replace it with a component that has one

1 idiom, 1 layout, 1 web: 2 of 3 pages are native screens

Skipped (2):
  /orders/{id}/lines/{line}: needs a value for {line}; add `line = ...` under [coverage.params] in native.toml
  /me: answered 303 to /sign-in: signed out; give a session in `[coverage.headers]`

Not pages, so not asked: not a GET (4); routes under the API prefix (2).
";
        assert_eq!(out, expected);
    }

    #[test]
    fn totals_are_per_grade() {
        let report = Report::read(&report_json())
            .unwrap()
            .finish(None, "native.toml");
        assert_eq!(
            report.totals,
            Totals {
                pages: 3,
                idiom: 1,
                designed: 0,
                layout: 1,
                web: 1,
                skipped: 2
            }
        );
        assert!(report.passed);
    }

    /// An app with two screens written for native clients: one sound, one that says
    /// something a designed screen should not.
    fn designed_json() -> String {
        serde_json::json!({
            "format": "navree-native-coverage", "version": 1,
            "platform": "ios", "form": "phone",
            "pages": [
                {"route": "/", "path": "/", "grade": "designed", "native": true,
                 "why": [], "missing": [], "dropped": [], "html_bytes": 900, "envelope_bytes": 700},
                {"route": "/orders", "path": "/orders", "grade": "idiom", "native": true,
                 "why": [], "missing": [], "dropped": [], "html_bytes": 5000, "envelope_bytes": 2000},
                {"route": "/shelves", "path": "/shelves", "grade": "designed", "native": true,
                 "why": [], "missing": ["a card stands inside a card"],
                 "dropped": [], "html_bytes": 0, "envelope_bytes": 1200}
            ],
            "skipped": [], "not_pages": []
        })
        .to_string()
    }

    #[test]
    fn a_screen_written_for_native_clients_is_listed_as_designed_with_what_it_should_fix() {
        let report = Report::read(&designed_json())
            .unwrap()
            .finish(None, "native.toml");
        assert_eq!(
            (
                report.totals.designed,
                report.totals.idiom,
                report.totals.layout
            ),
            (2, 1, 0)
        );
        let expected = "\
Native coverage of shop, asked as ios on phone

ROUTE     GRADE     WHAT IS MISSING
/         designed
/orders   idiom
/shelves  designed  a card stands inside a card

1 idiom, 2 designed, 0 layout, 0 web: 3 of 3 pages are native screens
";
        assert_eq!(render(&report, "shop"), expected);
    }

    #[test]
    fn a_designed_screen_holds_the_gate_unless_it_has_something_to_fix() {
        let report = Report::read(&designed_json())
            .unwrap()
            .finish(Some(FailOn::Layout), "native.toml");
        assert!(!report.passed);
        assert_eq!(
            report.refusal(Some(FailOn::Layout)).as_deref(),
            Some("1 of 3 pages are layout, web or designed with something to fix: /shelves")
        );
        // The web gate is not about it.
        assert!(report.refusal(Some(FailOn::Web)).is_none());
        // With nothing to fix, every screen holds the gate.
        let sound = designed_json().replace("a card stands inside a card", "");
        let sound = sound.replace("\"missing\":[\"\"]", "\"missing\":[]");
        let report = Report::read(&sound)
            .unwrap()
            .finish(Some(FailOn::Layout), "native.toml");
        assert!(report.passed, "{:?}", report.refusal(Some(FailOn::Layout)));
    }

    #[test]
    fn fail_on_web_fails_only_on_web() {
        let dir = app_dir();
        let options = CoverageOptions {
            fail_on: Some(FailOn::Web),
            ..CoverageOptions::default()
        };
        let (held, out) = run(&options, &mut scripted(), dir.path());
        assert!(!held);
        assert!(
            out.ends_with("native:coverage: 1 of 3 pages are web: /board\n"),
            "{out}"
        );
        // Layout alone does not fail it.
        let layout_only = report_json().replace("\"grade\":\"web\"", "\"grade\":\"idiom\"");
        let report = Report::read(&layout_only)
            .unwrap()
            .finish(Some(FailOn::Web), "native.toml");
        assert!(report.passed);
    }

    #[test]
    fn fail_on_layout_fails_on_layout_and_web() {
        let dir = app_dir();
        let options = CoverageOptions {
            fail_on: Some(FailOn::Layout),
            ..CoverageOptions::default()
        };
        let (held, out) = run(&options, &mut scripted(), dir.path());
        assert!(!held);
        assert!(
            out.ends_with("native:coverage: 2 of 3 pages are layout or web: /orders/1, /board\n"),
            "{out}"
        );
    }

    #[test]
    fn a_gate_with_no_pages_does_not_pass() {
        let empty = serde_json::json!({
            "format": "navree-native-coverage", "version": 1, "platform": "android", "form": "phone",
            "pages": [], "skipped": [], "not_pages": []
        })
        .to_string();
        let report = Report::read(&empty)
            .unwrap()
            .finish(Some(FailOn::Web), "native.toml");
        assert!(!report.passed);
        assert!(
            report
                .refusal(Some(FailOn::Web))
                .unwrap()
                .contains("no page was asked")
        );
        // Without a gate an empty report is only empty.
        assert!(
            Report::read(&empty)
                .unwrap()
                .finish(None, "native.toml")
                .passed
        );
    }

    #[test]
    fn skipped_routes_are_listed_with_the_reason() {
        let report = Report::read(&report_json())
            .unwrap()
            .finish(None, "native.toml");
        let text = render(&report, "shop");
        assert!(text.contains("Skipped (2):"));
        assert!(text.contains("/me: answered 303"));
    }

    #[test]
    fn json_is_stable() {
        let dir = app_dir();
        let options = CoverageOptions {
            json: true,
            fail_on: Some(FailOn::Web),
            ..CoverageOptions::default()
        };
        let (held, out) = run(&options, &mut scripted(), dir.path());
        assert!(!held);
        let json: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(json["format"], "navree-native-coverage");
        assert_eq!(json["totals"]["web"], 1);
        assert_eq!(json["totals"]["pages"], 3);
        assert_eq!(json["fail_on"], "web");
        assert_eq!(json["passed"], false);
        assert_eq!(json["pages"][2]["route"], "/board");
        assert_eq!(json["pages"][2]["why"][0], "tag:canvas");
        assert!(
            json["pages"][2]["said"][0]
                .as_str()
                .unwrap()
                .contains("<canvas>")
        );
        assert_eq!(json["skipped"][1]["route"], "/me");
        assert_eq!(json["not_pages"][0]["count"], 4);
        // Nothing but the JSON on standard output.
        assert!(!out.contains("native:coverage:"));
    }

    #[test]
    fn an_app_without_the_command_is_told_how() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"shop\"\nversion = \"0.1.0\"\n[dependencies]\nnavree = \"1\"\n",
        )
        .unwrap();
        let mut runner = FakeRunner::default().answer(
            "cargo run --quiet -- help",
            true,
            "Commands:\n  serve  x\n",
        );
        let error = run_coverage(
            dir.path(),
            &CoverageOptions::default(),
            &mut runner,
            &mut Vec::new(),
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("does not turn the native preview on"),
            "{error}"
        );
        assert!(error.contains("ui-native-preview"), "{error}");
        // With the feature in the manifest, the app is the one lacking the module.
        let dir = app_dir();
        let mut runner = FakeRunner::default().answer(
            "cargo run --quiet --features native-preview -- help",
            true,
            "Commands:\n  serve  x\n",
        );
        let error = run_coverage(
            dir.path(),
            &CoverageOptions::default(),
            &mut runner,
            &mut Vec::new(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("names `UiModule`"), "{error}");
    }

    #[test]
    fn the_feature_is_passed_to_cargo() {
        let dir = app_dir();
        let mut runner = scripted();
        run(&CoverageOptions::default(), &mut runner, dir.path());
        assert_eq!(
            runner.ran[0],
            "cargo run --quiet --features native-preview -- help"
        );
        assert!(
            runner.ran[1].starts_with(
                "cargo run --quiet --features native-preview -- native:coverage --plan "
            )
        );
        // A feature given on the command line replaces the one found.
        let mut runner = FakeRunner::default()
            .answer("cargo run --quiet --features mobile -- help", true, HELP)
            .answer(
                "cargo run --quiet --features mobile -- native:coverage",
                true,
                &report_json(),
            );
        let options = CoverageOptions {
            features: vec!["mobile".to_owned()],
            ..CoverageOptions::default()
        };
        run(&options, &mut runner, dir.path());
        assert_eq!(runner.ran.len(), 2);
    }

    #[test]
    fn the_plan_and_the_client_reach_the_app() {
        let dir = app_dir();
        std::fs::write(
            dir.path().join("native.toml"),
            "[coverage.params]\nid = 1\n",
        )
        .unwrap();
        let options = CoverageOptions {
            platform: Some("ios".to_owned()),
            form: Some("tablet".to_owned()),
            ..CoverageOptions::default()
        };
        let mut runner = scripted();
        run(&options, &mut runner, dir.path());
        assert!(
            runner.ran[1].contains(
                "native:coverage --platform ios --form tablet --plan {\"values\":{\"id\":\"1\"}"
            ),
            "{}",
            runner.ran[1]
        );
    }

    #[test]
    fn a_file_of_values_can_be_named() {
        let dir = app_dir();
        std::fs::write(
            dir.path().join("values.toml"),
            "[coverage.params]\nid = 9\n",
        )
        .unwrap();
        let options = CoverageOptions {
            file: Some(PathBuf::from("values.toml")),
            ..CoverageOptions::default()
        };
        let mut runner = scripted();
        run(&options, &mut runner, dir.path());
        assert!(runner.ran[1].contains("\"id\":\"9\""), "{}", runner.ran[1]);
    }

    #[test]
    fn a_report_of_another_kind_or_a_newer_one_is_refused() {
        let error = Report::read(
            "{\"format\":\"other\",\"version\":1,\"platform\":\"a\",\"form\":\"b\",\"pages\":[]}",
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("not `navree-native-coverage`"), "{error}");
        let error = Report::read(&report_json().replace("\"version\":1", "\"version\":2"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("update navree"), "{error}");
        assert!(
            Report::read("only log lines\n")
                .unwrap_err()
                .to_string()
                .contains("printed no report")
        );
    }

    #[test]
    fn a_failing_app_command_fails_the_run() {
        let dir = app_dir();
        let mut runner = FakeRunner::default()
            .answer(
                "cargo run --quiet --features native-preview -- help",
                true,
                HELP,
            )
            .answer(
                "cargo run --quiet --features native-preview -- native:coverage",
                false,
                "",
            );
        let error = run_coverage(
            dir.path(),
            &CoverageOptions::default(),
            &mut runner,
            &mut Vec::new(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("failed in the app"), "{error}");
    }
}
