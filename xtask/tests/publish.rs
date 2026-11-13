//! The publish loop against a registry, a cargo and a clock that live in
//! memory: resuming, rate limits, retries, tokens and the index wait.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use xtask::index::Index;
use xtask::publish::{
    Attempt, Clock, Failure, Options, Publisher, Report, Tokens, Tools, classify, publish_all,
};
use xtask::time::{days_from_civil, parse_http_date};

/// The crates a registry has (at the version being published).
type Registry = Rc<RefCell<BTreeSet<String>>>;

struct FakeIndex {
    registry: Registry,
    /// Reads of a crate's file that still miss it after it was uploaded.
    lag: BTreeMap<String, usize>,
    /// Crates the registry has no version of at all, until they are uploaded.
    never_published: BTreeSet<String>,
}

impl Index for FakeIndex {
    fn known(&mut self, name: &str) -> Result<bool, String> {
        Ok(!self.never_published.contains(name) || self.registry.borrow().contains(name))
    }

    fn has(&mut self, name: &str, _version: &str) -> Result<bool, String> {
        if !self.registry.borrow().contains(name) {
            return Ok(false);
        }
        match self.lag.get_mut(name) {
            Some(left) if *left > 0 => {
                *left -= 1;
                Ok(false)
            }
            _ => Ok(true),
        }
    }
}

/// A cargo that uploads unless an answer is scripted for the crate.
struct FakeCargo {
    registry: Registry,
    scripted: BTreeMap<String, VecDeque<Attempt>>,
    /// (crate, token) for every call.
    calls: Vec<(String, Option<String>)>,
    /// Uploads that answer with an error anyway (the upload went through,
    /// the answer was lost).
    upload_then_fail: BTreeMap<String, String>,
}

impl FakeCargo {
    fn new(registry: &Registry) -> Self {
        FakeCargo {
            registry: registry.clone(),
            scripted: BTreeMap::new(),
            calls: Vec::new(),
            upload_then_fail: BTreeMap::new(),
        }
    }

    fn script(&mut self, name: &str, output: &str) {
        self.scripted
            .entry(name.to_owned())
            .or_default()
            .push_back(Attempt {
                success: false,
                output: output.to_owned(),
            });
    }

    fn attempts(&self, name: &str) -> usize {
        self.calls.iter().filter(|(n, _)| n == name).count()
    }
}

impl Publisher for FakeCargo {
    fn publish(&mut self, name: &str, token: Option<&str>) -> Attempt {
        self.calls.push((name.to_owned(), token.map(str::to_owned)));
        if let Some(answer) = self.scripted.get_mut(name).and_then(VecDeque::pop_front) {
            return answer;
        }
        if self.registry.borrow().contains(name) {
            return Attempt {
                success: false,
                output: format!("error: crate {name}@1.0.0 already exists on crates.io index"),
            };
        }
        self.registry.borrow_mut().insert(name.to_owned());
        if let Some(output) = self.upload_then_fail.get(name) {
            return Attempt {
                success: false,
                output: output.clone(),
            };
        }
        Attempt {
            success: true,
            output: format!("Uploaded {name} v1.0.0 to registry `crates-io`"),
        }
    }
}

struct FakeClock {
    now: SystemTime,
    sleeps: Vec<Duration>,
}

impl FakeClock {
    fn at(text: &str) -> Self {
        FakeClock {
            now: parse_http_date(text).expect("a date"),
            sleeps: Vec::new(),
        }
    }
}

impl Clock for FakeClock {
    fn now(&self) -> SystemTime {
        self.now
    }

    fn sleep(&mut self, duration: Duration) {
        self.now += duration;
        self.sleeps.push(duration);
    }
}

#[derive(Default)]
struct FakeTokens {
    issued: usize,
    revoked: Vec<String>,
}

impl Tokens for FakeTokens {
    fn fresh(&mut self) -> Result<String, String> {
        self.issued += 1;
        Ok(format!("token-{}", self.issued))
    }

    fn revoke(&mut self, token: &str) {
        self.revoked.push(token.to_owned());
    }
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

fn registry(with: &[&str]) -> Registry {
    Rc::new(RefCell::new(with.iter().map(|s| (*s).to_owned()).collect()))
}

const NOW: &str = "Tue, 10 Nov 2026 01:00:00 GMT";

/// Runs the loop and returns its result with the log.
fn run(
    order: &[String],
    options: &Options,
    index: &mut FakeIndex,
    cargo: &mut FakeCargo,
    clock: &mut FakeClock,
    tokens: Option<&mut FakeTokens>,
) -> (Result<Report, xtask::publish::Stopped>, Vec<String>) {
    let mut lines = Vec::new();
    let mut log = |line: &str| lines.push(line.to_owned());
    let result = publish_all(
        order,
        options,
        Tools {
            index,
            publisher: cargo,
            clock,
            tokens: tokens.map(|t| t as &mut dyn Tokens),
            log: &mut log,
        },
    );
    (result, lines)
}

fn index_of(registry: &Registry) -> FakeIndex {
    FakeIndex {
        registry: registry.clone(),
        lag: BTreeMap::new(),
        never_published: BTreeSet::new(),
    }
}

#[test]
fn a_crate_never_published_takes_the_first_publish_token_and_the_rest_trusted_publishing() {
    let reg = registry(&[]);
    let mut cargo = FakeCargo::new(&reg);
    let mut tokens = FakeTokens::default();
    let mut index = index_of(&reg);
    index.never_published.insert("b".to_owned());
    let mut options = Options::new("1.0.0");
    options.first_publish_token = Some("first".to_owned());
    let order = names(&["a", "b", "c"]);
    let (result, log) = run(
        &order,
        &options,
        &mut index,
        &mut cargo,
        &mut FakeClock::at(NOW),
        Some(&mut tokens),
    );
    assert_eq!(result.unwrap().published, order);
    assert_eq!(
        cargo.calls,
        [
            ("a".to_owned(), Some("token-1".to_owned())),
            ("b".to_owned(), Some("first".to_owned())),
            ("c".to_owned(), Some("token-1".to_owned())),
        ]
    );
    // The first-publish token is the maintainer's to remove: it is never revoked here.
    assert_eq!(tokens.revoked, ["token-1"]);
    assert!(
        log.iter()
            .any(|l| l.contains("b: the registry has no version of it")),
        "{log:?}"
    );
}

#[test]
fn a_crate_never_published_stops_the_run_when_no_token_is_given_for_it() {
    let reg = registry(&[]);
    let mut cargo = FakeCargo::new(&reg);
    let mut tokens = FakeTokens::default();
    let mut index = index_of(&reg);
    index.never_published.insert("b".to_owned());
    let order = names(&["a", "b", "c"]);
    let (result, _) = run(
        &order,
        &Options::new("1.0.0"),
        &mut index,
        &mut cargo,
        &mut FakeClock::at(NOW),
        Some(&mut tokens),
    );
    let stopped = result.unwrap_err();
    assert!(
        stopped.message.contains("b has never been published")
            && stopped.message.contains("NAVREE_FIRST_PUBLISH_TOKEN"),
        "{}",
        stopped.message
    );
    assert_eq!(stopped.report.published, names(&["a"]));
    assert_eq!(stopped.report.remaining, names(&["b", "c"]));
    assert_eq!(cargo.attempts("b"), 0, "nothing was tried without a token");
    assert_eq!(
        tokens.revoked,
        ["token-1"],
        "the short-lived token is not left alive"
    );
}

#[test]
fn without_trusted_publishing_a_new_crate_is_published_as_any_other() {
    let reg = registry(&[]);
    let mut cargo = FakeCargo::new(&reg);
    let mut index = index_of(&reg);
    index.never_published.insert("a".to_owned());
    let (result, _) = run(
        &names(&["a"]),
        &Options::new("1.0.0"),
        &mut index,
        &mut cargo,
        &mut FakeClock::at(NOW),
        None,
    );
    assert_eq!(result.unwrap().published, names(&["a"]));
    assert_eq!(cargo.calls, [("a".to_owned(), None)]);
}

#[test]
fn crates_the_registry_has_are_skipped() {
    let reg = registry(&["a", "b"]);
    let mut cargo = FakeCargo::new(&reg);
    let order = names(&["a", "b", "c", "d"]);
    let (result, _) = run(
        &order,
        &Options::new("1.0.0"),
        &mut index_of(&reg),
        &mut cargo,
        &mut FakeClock::at(NOW),
        None,
    );
    let report = result.unwrap();
    assert_eq!(report.already, names(&["a", "b"]));
    assert_eq!(report.published, names(&["c", "d"]));
    assert!(report.remaining.is_empty());
    assert_eq!(
        cargo
            .calls
            .iter()
            .map(|(n, _)| n.as_str())
            .collect::<Vec<_>>(),
        ["c", "d"]
    );
}

#[test]
fn a_run_stopped_part_way_resumes_where_it_stopped() {
    let reg = registry(&[]);
    let order = names(&["a", "b", "c", "d", "e"]);
    let mut options = Options::new("1.0.0");
    options.limit = Some(2);

    let mut first = FakeCargo::new(&reg);
    let (result, log) = run(
        &order,
        &options,
        &mut index_of(&reg),
        &mut first,
        &mut FakeClock::at(NOW),
        None,
    );
    let report = result.unwrap();
    assert_eq!(report.published, names(&["a", "b"]));
    assert_eq!(report.remaining, names(&["c", "d", "e"]));
    assert!(log.iter().any(|l| l.contains("limit")), "{log:?}");

    options.limit = None;
    let mut second = FakeCargo::new(&reg);
    let (result, _) = run(
        &order,
        &options,
        &mut index_of(&reg),
        &mut second,
        &mut FakeClock::at(NOW),
        None,
    );
    let report = result.unwrap();
    assert_eq!(report.already, names(&["a", "b"]));
    assert_eq!(report.published, names(&["c", "d", "e"]));
    assert_eq!(
        second.attempts("a") + second.attempts("b"),
        0,
        "nothing is published twice"
    );
}

#[test]
fn an_upload_whose_answer_was_lost_counts_as_published() {
    // The first run was interrupted after the upload and before the index
    // listed it; the next attempt finds the version there.
    let reg = registry(&[]);
    let mut cargo = FakeCargo::new(&reg);
    cargo.upload_then_fail.insert(
        "b".to_owned(),
        "error: failed to publish to registry at https://crates.io\n\nCaused by:\n  the remote server responded with an error (status 503 Service Unavailable)".to_owned(),
    );
    let order = names(&["a", "b", "c"]);
    let mut clock = FakeClock::at(NOW);
    let (result, log) = run(
        &order,
        &Options::new("1.0.0"),
        &mut index_of(&reg),
        &mut cargo,
        &mut clock,
        None,
    );
    let report = result.unwrap();
    assert_eq!(report.published, names(&["a", "b", "c"]));
    assert_eq!(cargo.attempts("b"), 2);
    assert!(
        log.iter()
            .any(|l| l.contains("b 1.0.0: the registry has it already")),
        "{log:?}"
    );
}

#[test]
fn a_fatal_error_stops_the_run_and_names_what_is_left() {
    let reg = registry(&[]);
    let mut cargo = FakeCargo::new(&reg);
    cargo.script(
        "c",
        "error: failed to publish to registry at https://crates.io\n\nCaused by:\n  the remote server responded with an error (status 400 Bad Request): invalid upload request: missing field `description`",
    );
    let order = names(&["a", "b", "c", "d"]);
    let (result, _) = run(
        &order,
        &Options::new("1.0.0"),
        &mut index_of(&reg),
        &mut cargo,
        &mut FakeClock::at(NOW),
        None,
    );
    let stopped = result.unwrap_err();
    assert!(
        stopped.message.contains("cargo publish failed for c"),
        "{}",
        stopped.message
    );
    assert!(
        stopped.message.contains("missing field `description`"),
        "{}",
        stopped.message
    );
    assert_eq!(stopped.report.published, names(&["a", "b"]));
    assert_eq!(stopped.report.remaining, names(&["c", "d"]));
    assert_eq!(cargo.attempts("d"), 0, "nothing after the failure is tried");
    assert_eq!(cargo.attempts("c"), 1, "a fatal error is not tried again");
}

#[test]
fn a_rate_limit_waits_until_the_time_the_registry_gives() {
    let reg = registry(&[]);
    let mut cargo = FakeCargo::new(&reg);
    cargo.script(
        "f",
        "error: failed to publish f v1.0.0 to registry at https://crates.io\n\nCaused by:\n  the remote server responded with an error (status 429 Too Many Requests): You have published too many new crates in a short period of time. Please try again after Tue, 10 Nov 2026 01:20:00 GMT and see https://crates.io/docs/rate-limits for more details.",
    );
    let order = names(&["e", "f", "g"]);
    let mut clock = FakeClock::at(NOW);
    let (result, log) = run(
        &order,
        &Options::new("1.0.0"),
        &mut index_of(&reg),
        &mut cargo,
        &mut clock,
        None,
    );
    let report = result.unwrap();
    assert_eq!(report.published, names(&["e", "f", "g"]));
    assert_eq!(cargo.attempts("f"), 2);
    // Twenty minutes, plus the margin for clocks that differ.
    assert_eq!(clock.sleeps, vec![Duration::from_secs(20 * 60 + 10)]);
    assert!(
        log.iter()
            .any(|l| l.contains("waiting 21 minutes, until 2026-11-10 01:20:10 UTC")),
        "{log:?}"
    );
}

#[test]
fn a_rate_limit_without_a_time_waits_the_fallback_and_several_in_a_row_are_waited_out() {
    let reg = registry(&[]);
    let mut cargo = FakeCargo::new(&reg);
    for _ in 0..3 {
        cargo.script(
            "a",
            "the remote server responded with an error (status 429 Too Many Requests)",
        );
    }
    let mut clock = FakeClock::at(NOW);
    let (result, _) = run(
        &names(&["a"]),
        &Options::new("1.0.0"),
        &mut index_of(&reg),
        &mut cargo,
        &mut clock,
        None,
    );
    assert_eq!(result.unwrap().published, names(&["a"]));
    assert_eq!(clock.sleeps, vec![Duration::from_secs(600); 3]);
    assert_eq!(cargo.attempts("a"), 4);
}

#[test]
fn a_time_already_past_waits_only_the_margin() {
    let reg = registry(&[]);
    let mut cargo = FakeCargo::new(&reg);
    cargo.script("a", "(status 429 Too Many Requests): Please try again after Tue, 10 Nov 2026 00:59:00 GMT and see");
    let mut clock = FakeClock::at(NOW);
    let (result, _) = run(
        &names(&["a"]),
        &Options::new("1.0.0"),
        &mut index_of(&reg),
        &mut cargo,
        &mut clock,
        None,
    );
    result.unwrap();
    assert_eq!(clock.sleeps, vec![Duration::from_secs(10)]);
}

#[test]
fn network_errors_are_tried_again_a_few_times_then_stop_the_run() {
    let reg = registry(&[]);
    let mut cargo = FakeCargo::new(&reg);
    cargo.script(
        "a",
        "warning: spurious network error (3 tries remaining): [7] Couldn't connect to server",
    );
    cargo.script(
        "a",
        "the remote server responded with an error (status 502 Bad Gateway)",
    );
    let mut clock = FakeClock::at(NOW);
    let (result, _) = run(
        &names(&["a"]),
        &Options::new("1.0.0"),
        &mut index_of(&reg),
        &mut cargo,
        &mut clock,
        None,
    );
    result.unwrap();
    assert_eq!(
        clock.sleeps,
        vec![Duration::from_secs(30), Duration::from_secs(60)]
    );

    let reg = registry(&[]);
    let mut cargo = FakeCargo::new(&reg);
    for _ in 0..10 {
        cargo.script("a", "(status 500 Internal Server Error)");
    }
    let mut options = Options::new("1.0.0");
    options.retries = 2;
    let (result, _) = run(
        &names(&["a", "b"]),
        &options,
        &mut index_of(&reg),
        &mut cargo,
        &mut FakeClock::at(NOW),
        None,
    );
    let stopped = result.unwrap_err();
    assert_eq!(cargo.attempts("a"), 3);
    assert_eq!(stopped.report.remaining, names(&["a", "b"]));
}

#[test]
fn the_next_crate_waits_until_the_index_lists_the_last_one() {
    let reg = registry(&[]);
    let mut cargo = FakeCargo::new(&reg);
    let mut index = index_of(&reg);
    index.lag.insert("a".to_owned(), 3);
    let mut clock = FakeClock::at(NOW);
    let (result, _) = run(
        &names(&["a", "b"]),
        &Options::new("1.0.0"),
        &mut index,
        &mut cargo,
        &mut clock,
        None,
    );
    assert_eq!(result.unwrap().published, names(&["a", "b"]));
    assert_eq!(clock.sleeps, vec![Duration::from_secs(5); 3]);
}

#[test]
fn a_crate_the_index_never_lists_stops_the_run() {
    let reg = registry(&[]);
    let mut cargo = FakeCargo::new(&reg);
    let mut index = index_of(&reg);
    index.lag.insert("a".to_owned(), usize::MAX);
    let mut options = Options::new("1.0.0");
    options.index_wait = Duration::from_secs(60);
    let (result, _) = run(
        &names(&["a", "b"]),
        &options,
        &mut index,
        &mut cargo,
        &mut FakeClock::at(NOW),
        None,
    );
    let stopped = result.unwrap_err();
    assert!(
        stopped
            .message
            .contains("does not list it after 60 seconds"),
        "{}",
        stopped.message
    );
    assert_eq!(stopped.report.remaining, names(&["a", "b"]));
    assert_eq!(cargo.attempts("b"), 0);
}

#[test]
fn a_fresh_token_comes_every_few_crates_and_every_token_is_revoked() {
    let reg = registry(&["a"]);
    let mut cargo = FakeCargo::new(&reg);
    let mut tokens = FakeTokens::default();
    let mut options = Options::new("1.0.0");
    options.per_token = 2;
    let order = names(&["a", "b", "c", "d", "e", "f"]);
    let (result, _) = run(
        &order,
        &options,
        &mut index_of(&reg),
        &mut cargo,
        &mut FakeClock::at(NOW),
        Some(&mut tokens),
    );
    assert_eq!(result.unwrap().published, names(&["b", "c", "d", "e", "f"]));
    let used: Vec<(&str, &str)> = cargo
        .calls
        .iter()
        .map(|(n, t)| (n.as_str(), t.as_deref().unwrap()))
        .collect();
    assert_eq!(
        used,
        [
            ("b", "token-1"),
            ("c", "token-1"),
            ("d", "token-2"),
            ("e", "token-2"),
            ("f", "token-3")
        ]
    );
    assert_eq!(tokens.revoked, names(&["token-1", "token-2", "token-3"]));
}

#[test]
fn a_rate_limit_wait_and_a_refused_token_get_a_fresh_token() {
    let reg = registry(&[]);
    let mut cargo = FakeCargo::new(&reg);
    cargo.script("a", "(status 429 Too Many Requests): Please try again after Tue, 10 Nov 2026 01:45:00 GMT and see");
    cargo.script(
        "b",
        "the remote server responded with an error (status 403 Forbidden): authentication failed",
    );
    let mut tokens = FakeTokens::default();
    let (result, _) = run(
        &names(&["a", "b"]),
        &Options::new("1.0.0"),
        &mut index_of(&reg),
        &mut cargo,
        &mut FakeClock::at(NOW),
        Some(&mut tokens),
    );
    result.unwrap();
    let used: Vec<&str> = cargo
        .calls
        .iter()
        .map(|(_, t)| t.as_deref().unwrap())
        .collect();
    assert_eq!(used, ["token-1", "token-2", "token-2", "token-3"]);
    assert_eq!(tokens.revoked, names(&["token-1", "token-2", "token-3"]));
}

#[test]
fn a_refused_token_without_trusted_publishing_stops_the_run() {
    let reg = registry(&[]);
    let mut cargo = FakeCargo::new(&reg);
    cargo.script("a", "the remote server responded with an error (status 403 Forbidden): this token does not have the required permissions");
    let (result, _) = run(
        &names(&["a"]),
        &Options::new("1.0.0"),
        &mut index_of(&reg),
        &mut cargo,
        &mut FakeClock::at(NOW),
        None,
    );
    assert!(result.unwrap_err().message.contains("required permissions"));
    assert_eq!(cargo.attempts("a"), 1);
}

#[test]
fn cargo_messages_are_read() {
    assert_eq!(
        classify("error: crate navree-kernel@1.0.0 already exists on crates.io index"),
        Failure::AlreadyPublished
    );
    assert_eq!(
        classify(
            "Caused by:\n  the remote server responded with an error (status 429 Too Many Requests): You have published too many new crates in a short period of time. Please try again after Wed, 11 Nov 2026 09:05:07 GMT and see https://crates.io/docs/rate-limits for more details."
        ),
        Failure::RateLimited(Some(
            UNIX_EPOCH
                + Duration::from_secs(
                    days_from_civil(2026, 11, 11) as u64 * 86_400 + 9 * 3600 + 5 * 60 + 7
                )
        ))
    );
    assert_eq!(
        classify("(status 429 Too Many Requests)"),
        Failure::RateLimited(None)
    );
    assert_eq!(
        classify("(status 401 Unauthorized): invalid token"),
        Failure::Unauthorized
    );
    assert_eq!(
        classify("(status 503 Service Unavailable)"),
        Failure::Transient
    );
    assert_eq!(
        classify("error: failed to send request: operation timed out"),
        Failure::Transient
    );
    assert_eq!(
        classify("(status 400 Bad Request): invalid version"),
        Failure::Fatal
    );
    // A 5 that is not a status code is not a server error.
    assert_eq!(
        classify("the manifest status 5 fields are invalid"),
        Failure::Fatal
    );
    assert_eq!(
        classify("error: failed to verify package tarball"),
        Failure::Fatal
    );
}
