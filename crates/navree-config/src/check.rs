//! `config:check`: every problem of the configuration at once.
//!
//! The check loads the app's sources, reads every registered section as the
//! boot does (collecting each error instead of stopping at the first), names
//! the keys and sections that nothing reads, lists the keys that `.env` and a
//! config file both set with different values, and, for a production
//! environment, applies a short profile of settings that are unsafe there.
//! When the app has a kit's web app beside it, the settings the web app
//! repeats are compared with the configuration
//! ([`with_web_app`](CheckReport::with_web_app)). It never starts the app.

use std::fmt::Write as _;
use std::path::Path;

pub use crate::web_app::{WEB_APP_ENV, web_app_mismatches};

use crate::registry::{FRAMEWORK_SECTIONS, SectionRegistration, problems};
use crate::{Config, ConfigError, ConfigSources, Environment, Shadowed, UnusedKey};

const USAGE: &str = "usage: config:check [--env <name>]";

/// What `config:check` was asked.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CheckOptions {
    /// Check as if `app.env` were this name, whatever the sources say.
    /// `--env production` applies the production profile.
    pub environment: Option<String>,
}

impl CheckOptions {
    /// Reads the command line after `config:check`.
    pub fn parse(args: &[String]) -> Result<Self, String> {
        let mut options = CheckOptions::default();
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            let name = if arg == "--env" {
                args.next()
                    .ok_or_else(|| format!("`--env` needs an environment name; {USAGE}"))?
                    .clone()
            } else if let Some(name) = arg.strip_prefix("--env=") {
                name.to_owned()
            } else {
                return Err(format!("unknown argument `{arg}`; {USAGE}"));
            };
            if name.trim().is_empty() {
                return Err(format!("`--env` needs an environment name; {USAGE}"));
            }
            options.environment = Some(name);
        }
        Ok(options)
    }
}

/// A setting of the production profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    /// A failure fails the check; a note does not.
    pub fail: bool,
    /// The key.
    pub key: &'static str,
    /// What is wrong and what to do.
    pub message: String,
}

/// The result of [`CheckReport::run`].
#[derive(Debug)]
pub struct CheckReport {
    /// How many registered sections were read.
    pub sections: usize,
    /// One error per section that does not read or validate.
    pub errors: Vec<ConfigError>,
    /// Keys and sections that nothing reads.
    pub unknown: Vec<UnusedKey>,
    /// Keys that `.env` and a config file both set with different values (a
    /// warning, not a failure).
    pub shadowed: Vec<Shadowed>,
    /// The environment the check ran for, when the sources or `--env` name one.
    pub environment: Option<Environment>,
    /// The production profile; empty unless the environment is production.
    pub profile: Vec<Finding>,
    /// What the web app's settings say that the configuration does not
    /// ([`with_web_app`](CheckReport::with_web_app); warnings, not failures).
    pub web: Vec<String>,
}

impl CheckReport {
    /// Loads `sources` (with `app.env` set to `options.environment` when
    /// given) and checks them against `registrations`.
    ///
    /// Only a source that cannot be loaded at all (a malformed file) is an
    /// `Err`; everything else is in the report.
    pub fn run(
        sources: &ConfigSources,
        registrations: &[SectionRegistration],
        options: &CheckOptions,
    ) -> Result<CheckReport, ConfigError> {
        Self::run_with_claims(sources, registrations, &[], options)
    }

    /// Like [`run`](CheckReport::run), with `claims`: keys, and the keys
    /// below them, that modules read by themselves (see
    /// [`ConfigClaim`](crate::ConfigClaim)) and that are not unknown.
    pub fn run_with_claims(
        sources: &ConfigSources,
        registrations: &[SectionRegistration],
        claims: &[String],
        options: &CheckOptions,
    ) -> Result<CheckReport, ConfigError> {
        let sources = match &options.environment {
            Some(name) => sources.clone().set("app.env", name.as_str()),
            None => sources.clone(),
        };
        let config = Config::load(&sources)?;
        let errors = problems(&config, registrations);
        let mut known: Vec<&str> = FRAMEWORK_SECTIONS.to_vec();
        known.extend(claims.iter().map(String::as_str));
        let unknown = config.unused_with(&known);
        let shadowed = config.shadowed().to_vec();
        let environment = config.environment();
        let profile = if environment
            .as_ref()
            .is_some_and(Environment::behaves_like_production)
        {
            production_profile(&config)
        } else {
            Vec::new()
        };
        Ok(CheckReport {
            sections: count_sections(registrations),
            errors,
            unknown,
            shadowed,
            environment,
            profile,
            web: Vec::new(),
        })
    }

    /// Adds what the web app's environment file at `path` (a kit's
    /// `web/.env.local`, [`WEB_APP_ENV`]) says that the configuration of
    /// `sources` does not: where the API listens, the real-time key and
    /// port, the origins it lets call it. Nothing without such a file. These
    /// are warnings: the check still passes.
    #[must_use]
    pub fn with_web_app(mut self, sources: &ConfigSources, path: &Path) -> CheckReport {
        if let Ok(config) = Config::load(sources) {
            self.web = web_app_mismatches(&config, path);
        }
        self
    }

    /// How many things fail the check: invalid sections, unknown keys and
    /// failing profile settings. Keys set in both `.env` and a config file,
    /// what the web app's settings disagree on, and notes, are warnings and
    /// do not count.
    pub fn problems(&self) -> usize {
        self.errors.len() + self.unknown.len() + self.profile.iter().filter(|f| f.fail).count()
    }

    /// Whether nothing fails the check.
    pub fn is_ok(&self) -> bool {
        self.problems() == 0
    }

    /// The process exit code: 0 when the check passes, 1 when it fails.
    pub fn exit_code(&self) -> i32 {
        i32::from(!self.is_ok())
    }

    /// The report as text. It names keys, sections and files, never a value.
    pub fn render(&self) -> String {
        let mut out = String::new();
        for error in &self.errors {
            // An `Invalid` error lists several sections; show one line each.
            match error {
                ConfigError::Invalid { problems } => {
                    for problem in problems {
                        let _ = writeln!(out, "error    {problem}");
                    }
                }
                other => {
                    let _ = writeln!(out, "error    {other}");
                }
            }
        }
        for unknown in &self.unknown {
            let _ = writeln!(out, "unknown  {}", unknown.message());
        }
        for shadow in &self.shadowed {
            let _ = writeln!(out, "warning  {}", shadow.message());
        }
        for mismatch in &self.web {
            let _ = writeln!(out, "warning  {mismatch}");
        }
        if let Some(environment) = &self.environment
            && environment.behaves_like_production()
        {
            let failures = self.profile.iter().filter(|f| f.fail).count();
            let _ = writeln!(
                out,
                "production profile ({environment}): {}",
                if failures == 0 { "ok" } else { "FAIL" }
            );
            for finding in &self.profile {
                let level = if finding.fail { "fail" } else { "note" };
                let _ = writeln!(out, "  {level}  {}: {}", finding.key, finding.message);
            }
        }
        let problems = self.problems();
        if problems == 0 {
            let _ = writeln!(out, "config:check: ok ({} sections read)", self.sections);
        } else {
            let plural = if problems == 1 { "problem" } else { "problems" };
            let _ = writeln!(out, "config:check: {problems} {plural}");
        }
        out
    }
}

/// How many distinct sections are registered.
fn count_sections(registrations: &[SectionRegistration]) -> usize {
    let mut ids: Vec<(&str, &str)> = registrations
        .iter()
        .map(|r| (r.key(), r.type_name()))
        .collect();
    ids.sort_unstable();
    ids.dedup();
    ids.len()
}

/// The settings that are unsafe in production. Failures are settings that
/// make the app wrong there; notes are settings worth a look.
pub fn production_profile(config: &Config) -> Vec<Finding> {
    let mut findings = Vec::new();
    let fail = |key, message: &str| Finding {
        fail: true,
        key,
        message: message.to_owned(),
    };
    let note = |key, message: &str| Finding {
        fail: false,
        key,
        message: message.to_owned(),
    };

    let has_key = config.app().is_ok_and(|app| app.key.is_some());
    if !has_key {
        findings.push(fail(
            "app.key",
            "is not set, so encryption and signed tokens stop working. Run `navree key:generate` and set APP_KEY in the environment.",
        ));
    }

    match config
        .get_string("database.url")
        .map(|url| url.trim().to_lowercase())
    {
        Some(url) if url.starts_with("sqlite:") => findings.push(fail(
            "database.url",
            "is a SQLite file. Production needs a database server such as PostgreSQL or MySQL.",
        )),
        Some(_) => {}
        None => findings.push(note(
            "database.url",
            "is not set, so a module that uses the database falls back to its default SQLite file.",
        )),
    }

    if config
        .get_string("mail.transport")
        .is_some_and(|transport| transport.trim().eq_ignore_ascii_case("log"))
    {
        findings.push(fail(
            "mail.transport",
            "is `log`: messages are written to the log and never sent. Use an smtp:// address or a mail service.",
        ));
    }

    match config.app_url() {
        None => findings.push(note(
            "app.url",
            "is not set, so links in mail and sign-in callbacks have no address.",
        )),
        Some(url) if !url.starts_with("https://") => {
            findings.push(note("app.url", "is not an https address."))
        }
        Some(_) => {}
    }

    // An app's development settings send the cookie over plain HTTP.
    if config.get_bool("session.secure") == Some(false) {
        findings.push(fail(
            "session.secure",
            "is false, so the session cookie also goes over plain HTTP. Production serves HTTPS: set APP_SESSION__SECURE=true.",
        ));
    }

    if config
        .get_string("log.level")
        .is_some_and(|level| matches!(level.trim().to_lowercase().as_str(), "debug" | "trace"))
    {
        findings.push(note(
            "log.level",
            "is `debug` or `trace`, which logs more than production usually needs.",
        ));
    }

    let exports_traces = config
        .get_string("observe.tracing.endpoint")
        .is_some_and(|endpoint| !endpoint.trim().is_empty());
    let traces_everything = config
        .get_string("observe.tracing.sample_ratio")
        .is_none_or(|ratio| ratio.trim().parse::<f64>().is_ok_and(|ratio| ratio >= 1.0));
    if exports_traces && traces_everything {
        findings.push(note(
            "observe.tracing.sample_ratio",
            "is 1 (the default) while traces are exported, so every request is traced. A smaller ratio such as 0.1 costs less.",
        ));
    }

    for key in ["api.docs_ui", "api.openapi_json"] {
        if config.get_bool(key) == Some(true) {
            findings.push(note(
                key,
                "is on, so the API reference or the OpenAPI document is public in production.",
            ));
            break;
        }
    }
    findings
}
