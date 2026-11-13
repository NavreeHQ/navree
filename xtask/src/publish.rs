//! `release publish`: publishes every crate in dependency order, and can be
//! stopped and started again at any point.
//!
//! 1. The order comes from `cargo metadata` ([`crate::order`]).
//! 2. Navigator's files are staged ([`crate::stage`]).
//! 3. One `cargo publish --dry-run` of every crate builds the whole set as
//!    it will be published, on the exact commit; a marker under
//!    `target/release/` records it, so a resumed run does not build again.
//! 4. Each crate the registry's index does not list at the version yet is
//!    published with `--no-verify` (step 3 verified it), and the next one
//!    starts once the index lists it.
//!
//! When the registry answers 429 (crates.io allows 5 new crates at once,
//! then 1 every 10 minutes, and 30 new versions at once, then 1 a minute),
//! the publish waits until the time the answer gives and goes on. A network
//! error or a 5xx answer is tried again a few times. Anything else stops
//! the run with cargo's message; after the fix, the same command resumes
//! where it stopped.
//!
//! With `--trusted-publishing` (in GitHub Actions, for crates that exist on
//! crates.io already) every 25 crates get a fresh token from crates.io's
//! trusted publishing, which lasts 30 minutes, and each token is revoked
//! when its crates are done. A crate the registry has no version of cannot
//! be published that way: it is published with the API token in
//! `NAVREE_FIRST_PUBLISH_TOKEN`, and the run stops at it when there is none.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use crate::index::{Index, SparseIndex, env_name};
use crate::metadata::{Workspace, cargo, workspace_version};
use crate::order;
use crate::process::{Stdin, curl, git};
use crate::stage;
use crate::time::{format_utc, parse_http_date};

/// The environment variable that holds the API token for a crate's first
/// publish, read with `--trusted-publishing`.
pub const FIRST_PUBLISH_TOKEN: &str = "NAVREE_FIRST_PUBLISH_TOKEN";

/// The output of one `cargo publish`.
#[derive(Debug, Clone)]
pub struct Attempt {
    /// Whether cargo succeeded.
    pub success: bool,
    /// Its standard output and error.
    pub output: String,
}

/// Publishes one crate.
pub trait Publisher {
    /// Runs `cargo publish` for `name`, with `token` when one is given.
    fn publish(&mut self, name: &str, token: Option<&str>) -> Attempt;
}

/// The time, and waiting.
pub trait Clock {
    /// Now.
    fn now(&self) -> SystemTime;
    /// Waits `duration`.
    fn sleep(&mut self, duration: Duration);
}

/// Short-lived publish tokens.
pub trait Tokens {
    /// A fresh token.
    fn fresh(&mut self) -> Result<String, String>;
    /// Revokes `token` (best effort).
    fn revoke(&mut self, token: &str);
}

/// Why a `cargo publish` failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The registry has the version already (an earlier run uploaded it).
    AlreadyPublished,
    /// HTTP 429: try again after the time given, when there is one.
    RateLimited(Option<SystemTime>),
    /// HTTP 401 or 403.
    Unauthorized,
    /// A network error or a 5xx answer.
    Transient,
    /// Anything else: stop.
    Fatal,
}

/// Reads why a `cargo publish` failed from its output.
pub fn classify(output: &str) -> Failure {
    let lower = output.to_ascii_lowercase();
    if lower.contains("already exists on") || lower.contains("already uploaded") {
        return Failure::AlreadyPublished;
    }
    if lower.contains("status 429") || lower.contains("too many requests") {
        let until = output
            .find("try again after ")
            .map(|at| &output[at + "try again after ".len()..])
            .and_then(|rest| rest.find(" GMT").map(|end| &rest[..end + 4]))
            .and_then(parse_http_date);
        return Failure::RateLimited(until);
    }
    if lower.contains("status 401") || lower.contains("status 403") {
        return Failure::Unauthorized;
    }
    let server_error = lower.match_indices("status 5").any(|(at, _)| {
        lower[at + 7..]
            .chars()
            .take(3)
            .filter(char::is_ascii_digit)
            .count()
            == 3
    });
    let network = [
        "spurious network error",
        "failed to send request",
        "connection reset",
        "connection refused",
        "connection closed",
        "operation timed out",
        "timed out reading",
        "could not resolve host",
    ];
    if server_error || network.iter().any(|n| lower.contains(n)) {
        return Failure::Transient;
    }
    Failure::Fatal
}

/// How a publish run behaves.
#[derive(Debug, Clone)]
pub struct Options {
    /// The version being published.
    pub version: String,
    /// Crates per token, with a token provider.
    pub per_token: usize,
    /// Stop after publishing this many crates.
    pub limit: Option<usize>,
    /// How long a published crate may take to appear in the index.
    pub index_wait: Duration,
    /// How often the index is read while waiting.
    pub poll_every: Duration,
    /// How many times a network error or a 5xx answer is tried again.
    pub retries: u32,
    /// The wait after a 429 that gives no time.
    pub rate_limit_fallback: Duration,
    /// Added to the time a 429 gives, for clocks that differ.
    pub rate_limit_margin: Duration,
    /// With a token provider: the API token for a crate the registry has no
    /// version of. Trusted publishing only publishes crates that exist.
    pub first_publish_token: Option<String>,
}

impl Options {
    /// The defaults for `version`.
    pub fn new(version: &str) -> Self {
        Options {
            version: version.to_owned(),
            per_token: 25,
            limit: None,
            index_wait: Duration::from_secs(600),
            poll_every: Duration::from_secs(5),
            retries: 5,
            rate_limit_fallback: Duration::from_secs(600),
            rate_limit_margin: Duration::from_secs(10),
            first_publish_token: None,
        }
    }
}

/// What a run did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// The crates the registry had already.
    pub already: Vec<String>,
    /// The crates this run published.
    pub published: Vec<String>,
    /// The crates left when the run stopped (at its limit, or on an error).
    pub remaining: Vec<String>,
}

/// A run that stopped on an error.
#[derive(Debug, Clone)]
pub struct Stopped {
    /// What went wrong.
    pub message: String,
    /// What was done before.
    pub report: Report,
}

/// The tools a run uses.
pub struct Tools<'a> {
    /// The registry's index.
    pub index: &'a mut dyn Index,
    /// Runs `cargo publish`.
    pub publisher: &'a mut dyn Publisher,
    /// The time.
    pub clock: &'a mut dyn Clock,
    /// Short-lived tokens, when publishing with trusted publishing.
    pub tokens: Option<&'a mut dyn Tokens>,
    /// Where progress is written.
    pub log: &'a mut dyn FnMut(&str),
}

/// Publishes the crates of `order` that the index does not list at
/// `options.version`, in that order.
pub fn publish_all(
    order: &[String],
    options: &Options,
    tools: Tools<'_>,
) -> Result<Report, Stopped> {
    let Tools {
        index,
        publisher,
        clock,
        mut tokens,
        log,
    } = tools;
    let version = options.version.as_str();
    let mut report = Report::default();
    let mut token: Option<String> = None;
    let mut with_token = 0usize;

    for (at, name) in order.iter().enumerate() {
        match index.has(name, version) {
            Ok(true) => {
                log(&format!("{name} {version}: on the registry already"));
                report.already.push(name.clone());
                continue;
            }
            Ok(false) => {}
            Err(error) => {
                let message = format!("reading the index for {name}: {error}");
                return Err(stopped(
                    order,
                    at,
                    &report,
                    message,
                    &mut tokens,
                    &mut token,
                ));
            }
        }
        if options
            .limit
            .is_some_and(|limit| report.published.len() >= limit)
        {
            log(&format!(
                "stopping at the limit of {} crates",
                report.published.len()
            ));
            report.remaining = order[at..].to_vec();
            break;
        }
        // A crate the registry has never had cannot be published with a token from
        // trusted publishing: it takes the API token given for first publishes.
        let mut first: Option<&str> = None;
        if tokens.is_some() {
            match index.known(name) {
                Ok(true) => {}
                Ok(false) => match options.first_publish_token.as_deref() {
                    Some(own) => {
                        log(&format!(
                            "{name}: the registry has no version of it; publishing with the first-publish token"
                        ));
                        first = Some(own);
                    }
                    None => {
                        let message = format!(
                            "{name} has never been published, and trusted publishing only publishes a crate that exists: give an API token for its first publish in {FIRST_PUBLISH_TOKEN} (scopes publish-new and publish-update, this crate only), run the same command again, then remove the token and add the crate's trusted publisher"
                        );
                        return Err(stopped(
                            order,
                            at,
                            &report,
                            message,
                            &mut tokens,
                            &mut token,
                        ));
                    }
                },
                Err(error) => {
                    let message = format!("reading the index for {name}: {error}");
                    return Err(stopped(
                        order,
                        at,
                        &report,
                        message,
                        &mut tokens,
                        &mut token,
                    ));
                }
            }
        }
        if first.is_none()
            && tokens.is_some()
            && (token.is_none() || with_token >= options.per_token)
        {
            if let Err(message) = refresh(&mut tokens, &mut token) {
                return Err(stopped(
                    order,
                    at,
                    &report,
                    message,
                    &mut tokens,
                    &mut token,
                ));
            }
            with_token = 0;
        }

        let mut transient = 0u32;
        let mut refreshed = false;
        loop {
            log(&format!("publishing {name} {version}"));
            let attempt = publisher.publish(name, first.or(token.as_deref()));
            if attempt.success {
                break;
            }
            match classify(&attempt.output) {
                Failure::AlreadyPublished => {
                    log(&format!("{name} {version}: the registry has it already"));
                    break;
                }
                Failure::RateLimited(until) => {
                    let wait = match until {
                        Some(until) => {
                            until.duration_since(clock.now()).unwrap_or_default()
                                + options.rate_limit_margin
                        }
                        None => options.rate_limit_fallback,
                    };
                    log(&format!(
                        "{name}: the registry limits the rate of publishing; waiting {} minutes, until {}",
                        wait.as_secs().div_ceil(60),
                        format_utc(clock.now() + wait)
                    ));
                    clock.sleep(wait);
                    // A long wait outlives a short-lived token.
                    if tokens.is_some() && first.is_none() {
                        if let Err(message) = refresh(&mut tokens, &mut token) {
                            return Err(stopped(
                                order,
                                at,
                                &report,
                                message,
                                &mut tokens,
                                &mut token,
                            ));
                        }
                        with_token = 0;
                    }
                }
                Failure::Unauthorized if tokens.is_some() && first.is_none() && !refreshed => {
                    refreshed = true;
                    log(&format!(
                        "{name}: the token was refused; getting a fresh one"
                    ));
                    if let Err(message) = refresh(&mut tokens, &mut token) {
                        return Err(stopped(
                            order,
                            at,
                            &report,
                            message,
                            &mut tokens,
                            &mut token,
                        ));
                    }
                    with_token = 0;
                }
                Failure::Transient if transient < options.retries => {
                    transient += 1;
                    let wait = Duration::from_secs(30 * u64::from(transient));
                    log(&format!(
                        "{name}: a network or server error; trying again in {} seconds ({transient} of {})",
                        wait.as_secs(),
                        options.retries
                    ));
                    clock.sleep(wait);
                }
                _ => {
                    let message = format!(
                        "cargo publish failed for {name}:\n{}",
                        attempt.output.trim_end()
                    );
                    return Err(stopped(
                        order,
                        at,
                        &report,
                        message,
                        &mut tokens,
                        &mut token,
                    ));
                }
            }
        }

        // The next crate may depend on this one: wait until the index has it.
        let started = clock.now();
        while !index.has(name, version).unwrap_or(false) {
            let waited = clock.now().duration_since(started).unwrap_or_default();
            if waited >= options.index_wait {
                let message = format!(
                    "{name} {version} was published, but the index does not list it after {} seconds; run the same command again to go on",
                    waited.as_secs()
                );
                return Err(stopped(
                    order,
                    at,
                    &report,
                    message,
                    &mut tokens,
                    &mut token,
                ));
            }
            clock.sleep(options.poll_every);
        }
        log(&format!("{name} {version}: published"));
        report.published.push(name.clone());
        if first.is_none() {
            with_token += 1;
        }
    }

    if let (Some(provider), Some(old)) = (tokens.as_mut(), token.take()) {
        provider.revoke(&old);
    }
    Ok(report)
}

/// Revokes the current token, if any, and gets a fresh one.
fn refresh(tokens: &mut Option<&mut dyn Tokens>, token: &mut Option<String>) -> Result<(), String> {
    let Some(provider) = tokens.as_mut() else {
        return Ok(());
    };
    if let Some(old) = token.take() {
        provider.revoke(&old);
    }
    *token = Some(
        provider
            .fresh()
            .map_err(|e| format!("getting a publish token: {e}"))?,
    );
    Ok(())
}

/// The run stopped at `order[at]`: the token is revoked, and the report
/// lists what is left.
fn stopped(
    order: &[String],
    at: usize,
    report: &Report,
    message: String,
    tokens: &mut Option<&mut dyn Tokens>,
    token: &mut Option<String>,
) -> Stopped {
    if let (Some(provider), Some(old)) = (tokens.as_mut(), token.take()) {
        provider.revoke(&old);
    }
    let mut report = report.clone();
    report.remaining = order[at..].to_vec();
    Stopped { message, report }
}

// ---------------------------------------------------------------------------
// The real tools
// ---------------------------------------------------------------------------

/// `cargo publish --no-verify` of one crate.
///
/// Cargo is always given `--allow-dirty`: it counts the staged copies of
/// Navigator's skills and docs as changes although git ignores them, and
/// `run` has already asked git whether the checkout is clean (ignored files
/// apart) before it staged them.
pub struct CargoPublisher {
    /// The workspace root.
    pub root: PathBuf,
    /// The registry (`None` is crates.io).
    pub registry: Option<String>,
}

impl Publisher for CargoPublisher {
    fn publish(&mut self, name: &str, token: Option<&str>) -> Attempt {
        let mut command = Command::new(cargo());
        command
            .args([
                "publish",
                "--locked",
                "--no-verify",
                "--allow-dirty",
                "--package",
                name,
            ])
            .current_dir(&self.root);
        if let Some(registry) = &self.registry {
            command.args(["--registry", registry]);
        }
        if let Some(token) = token {
            match &self.registry {
                None => command.env("CARGO_REGISTRY_TOKEN", token),
                Some(registry) => command.env(env_name(registry, "TOKEN"), token),
            };
        }
        match command.output() {
            Ok(output) => {
                let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
                text.push_str(&String::from_utf8_lossy(&output.stderr));
                eprint!("{text}");
                Attempt {
                    success: output.status.success(),
                    output: text,
                }
            }
            Err(error) => Attempt {
                success: false,
                output: format!("cannot run cargo: {error}"),
            },
        }
    }
}

/// The system clock.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }

    fn sleep(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// Tokens from crates.io's trusted publishing, in GitHub Actions: the job's
/// OIDC token (audience `crates.io`) is exchanged for a token that lasts 30
/// minutes. The job needs `permissions: id-token: write`, and each crate a
/// trusted publisher for this repository and workflow on crates.io.
pub struct TrustedPublishing {
    /// `https://crates.io`.
    pub registry_url: String,
}

impl TrustedPublishing {
    fn endpoint(&self) -> String {
        format!(
            "{}/api/v1/trusted_publishing/tokens",
            self.registry_url.trim_end_matches('/')
        )
    }
}

impl Tokens for TrustedPublishing {
    fn fresh(&mut self) -> Result<String, String> {
        let request_url = std::env::var("ACTIONS_ID_TOKEN_REQUEST_URL")
            .map_err(|_| "ACTIONS_ID_TOKEN_REQUEST_URL is not set: trusted publishing needs GitHub Actions with `id-token: write`".to_owned())?;
        let request_token = std::env::var("ACTIONS_ID_TOKEN_REQUEST_TOKEN")
            .map_err(|_| "ACTIONS_ID_TOKEN_REQUEST_TOKEN is not set".to_owned())?;
        let audience = self
            .registry_url
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_end_matches('/');
        let url = format!("{request_url}&audience={audience}");
        let header = format!("Authorization: Bearer {request_token}\n");
        let response = curl("GET", &url, &[], Stdin::Headers(&header))?;
        if response.status != 200 {
            return Err(format!("GitHub's OIDC token: HTTP {}", response.status));
        }
        let jwt = serde_json::from_str::<serde_json::Value>(&response.body)
            .ok()
            .and_then(|json| json["value"].as_str().map(str::to_owned))
            .ok_or("GitHub's OIDC answer has no token")?;
        let body = serde_json::json!({ "jwt": jwt }).to_string();
        let response = curl(
            "POST",
            &self.endpoint(),
            &["Content-Type: application/json"],
            Stdin::Body(&body),
        )?;
        if response.status != 200 {
            return Err(format!(
                "crates.io's trusted publishing: HTTP {}: {}",
                response.status,
                response.body.trim()
            ));
        }
        let token = serde_json::from_str::<serde_json::Value>(&response.body)
            .ok()
            .and_then(|json| json["token"].as_str().map(str::to_owned))
            .ok_or("crates.io's answer has no token")?;
        if std::env::var_os("GITHUB_ACTIONS").is_some() {
            // Hidden from the job's log from here on.
            println!("::add-mask::{token}");
        }
        Ok(token)
    }

    fn revoke(&mut self, token: &str) {
        let header = format!("Authorization: Bearer {token}\n");
        match curl("DELETE", &self.endpoint(), &[], Stdin::Headers(&header)) {
            Ok(response) if response.status < 300 => {}
            Ok(response) => eprintln!("revoking the publish token: HTTP {}", response.status),
            Err(error) => eprintln!("revoking the publish token: {error}"),
        }
    }
}

// ---------------------------------------------------------------------------
// The command
// ---------------------------------------------------------------------------

/// The flags of `release publish`.
#[derive(Debug, Clone, Default)]
pub struct Flags {
    /// `--registry <name>`: a registry from Cargo's configuration; crates.io
    /// when absent.
    pub registry: Option<String>,
    /// `--index <url>`: the registry's sparse index, when Cargo's
    /// configuration does not name it.
    pub index: Option<String>,
    /// `--dry-run`: verify and show the plan, publish nothing.
    pub dry_run: bool,
    /// `--package <name>` (repeatable): only these crates. Workspace crates
    /// they depend on must be on the registry already.
    pub packages: Vec<String>,
    /// `--limit <n>`: stop after publishing n crates.
    pub limit: Option<usize>,
    /// `--per-token <n>`: crates per trusted-publishing token (25).
    pub per_token: Option<usize>,
    /// `--trusted-publishing`: get tokens from crates.io's trusted publishing.
    pub trusted_publishing: bool,
    /// `--skip-verify`: no `cargo publish --dry-run` first.
    pub skip_verify: bool,
    /// `--allow-dirty`: publish from a checkout with changes.
    pub allow_dirty: bool,
}

/// What the command ended with.
pub enum Outcome {
    /// Everything selected is on the registry.
    Done(Report),
    /// The run stopped at `--limit`; run again to go on.
    Paused(Report),
}

/// Runs `release publish`.
pub fn run(root: &Path, flags: &Flags) -> Result<Outcome, String> {
    let workspace = Workspace::load(root)?;
    let version = workspace_version(root)?;
    let registry = flags.registry.as_deref().filter(|r| *r != "crates-io");

    let selected: Vec<String> = if flags.packages.is_empty() {
        workspace
            .publishable()
            .filter(|p| p.publishable_to(registry))
            .map(|p| p.name.clone())
            .collect()
    } else {
        for name in &flags.packages {
            let package = workspace
                .member(name)
                .ok_or_else(|| format!("{name} is not a workspace member"))?;
            if !package.publishable_to(registry) {
                return Err(format!(
                    "{name} is not published to this registry (`publish` in its manifest)"
                ));
            }
        }
        flags.packages.clone()
    };
    let order = order::publish_order(&workspace, &selected)?;

    // The commit the verification marker names. With --allow-dirty there is
    // no clean commit to name, and git is not asked at all (so a checkout
    // that git cannot read from here, such as a Windows worktree seen from
    // WSL, still works).
    let commit = if flags.allow_dirty {
        None
    } else {
        let status = git(root, &["status", "--porcelain"])?;
        if !status.trim().is_empty() {
            return Err(format!(
                "the checkout has changes; publish from a clean checkout of the release commit:\n{status}"
            ));
        }
        Some(git(root, &["rev-parse", "HEAD"])?.trim().to_owned())
    };

    let staged = stage::stage(root)?;
    eprintln!(
        "staged Navigator's files: {} skill files, {} docs pages, the UI catalog",
        staged.skill_files, staged.doc_pages
    );

    let mut index = match &flags.index {
        Some(url) => SparseIndex::new(url)?,
        None => SparseIndex::for_registry(registry, root)?,
    };

    // A partial run's crates depend only on crates the registry has.
    let within: std::collections::BTreeSet<String> = selected.iter().cloned().collect();
    for name in &order {
        let package = workspace.member(name).expect("selected");
        for dep in workspace.internal_deps(package) {
            let waits = dep.kind != crate::metadata::DepKind::Dev || dep.is_versioned();
            if waits && !within.contains(&dep.name) && !index.has(&dep.name, &version)? {
                return Err(format!(
                    "{name} depends on {} {version}, which is neither selected nor on the registry",
                    dep.name
                ));
            }
        }
    }

    let marker_dir = root.join("target").join("release");
    let marker = commit.as_ref().map(|commit| {
        marker_dir.join(format!(
            "verified-{commit}-{}",
            registry.unwrap_or("crates-io")
        ))
    });
    if flags.skip_verify {
        eprintln!("skipping the verification build (--skip-verify)");
    } else if let Some(marker) = marker.as_ref().filter(|m| m.is_file()) {
        eprintln!(
            "this commit was verified for this registry already ({})",
            marker.display()
        );
    } else {
        eprintln!("verifying every crate as it will be published (cargo publish --dry-run)");
        let mut command = Command::new(cargo());
        command
            .args(["publish", "--dry-run", "--locked", "--allow-dirty"])
            .current_dir(root);
        for name in &order {
            command.args(["--package", name]);
        }
        if let Some(registry) = registry {
            command.args(["--registry", registry]);
        }
        let status = command
            .status()
            .map_err(|e| format!("cannot run cargo publish --dry-run: {e}"))?;
        if !status.success() {
            return Err("cargo publish --dry-run failed: nothing was published".to_owned());
        }
        if let Some(marker) = &marker {
            std::fs::create_dir_all(&marker_dir)
                .and_then(|()| std::fs::write(marker, format!("{version}\n")))
                .map_err(|e| format!("cannot write {}: {e}", marker.display()))?;
        }
    }

    if flags.dry_run {
        let mut report = Report::default();
        for name in &order {
            if index.has(name, &version)? {
                println!("{name} {version}: on the registry already");
                report.already.push(name.clone());
            } else {
                println!("{name} {version}: to publish");
                report.remaining.push(name.clone());
            }
        }
        return Ok(Outcome::Done(report));
    }

    let mut options = Options::new(&version);
    options.limit = flags.limit;
    if flags.trusted_publishing {
        options.first_publish_token = std::env::var(FIRST_PUBLISH_TOKEN)
            .ok()
            .filter(|token| !token.trim().is_empty());
    }
    if let Some(count) = flags.per_token {
        options.per_token = count.max(1);
    }
    let mut publisher = CargoPublisher {
        root: root.to_owned(),
        registry: registry.map(str::to_owned),
    };
    let mut clock = SystemClock;
    let mut trusted = TrustedPublishing {
        registry_url: "https://crates.io".to_owned(),
    };
    if flags.trusted_publishing && registry.is_some() {
        return Err("--trusted-publishing is for crates.io only".to_owned());
    }
    let log_file = marker_dir.join(format!("publish-{version}.log"));
    let _ = std::fs::create_dir_all(&marker_dir);
    let mut log = |line: &str| {
        let line = format!("{} {line}", format_utc(SystemTime::now()));
        eprintln!("{line}");
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_file)
        {
            use std::io::Write as _;
            let _ = writeln!(file, "{line}");
        }
    };
    let tools = Tools {
        index: &mut index,
        publisher: &mut publisher,
        clock: &mut clock,
        tokens: if flags.trusted_publishing {
            Some(&mut trusted as &mut dyn Tokens)
        } else {
            None
        },
        log: &mut log,
    };
    match publish_all(&order, &options, tools) {
        Ok(report) if report.remaining.is_empty() => Ok(Outcome::Done(report)),
        Ok(report) => Ok(Outcome::Paused(report)),
        Err(stopped) => Err(format!(
            "{}\n\npublished in this run: {}\nleft: {}\nfix the cause and run the same command again: crates on the registry are skipped",
            stopped.message,
            list(&stopped.report.published),
            list(&stopped.report.remaining)
        )),
    }
}

fn list(names: &[String]) -> String {
    if names.is_empty() {
        "none".to_owned()
    } else {
        names.join(", ")
    }
}
