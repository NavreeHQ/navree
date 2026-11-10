//! [`FakeExecutor`]: an [`Executor`] for tests. It records every command,
//! answers from a script and keeps a virtual clock, so no container
//! runtime is needed.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;
use std::time::Duration;

use crate::error::DockError;
use crate::exec::{Cmd, CmdOutput, Executor};

/// One scripted answer: commands whose printed line contains `pattern` get
/// `output` (once, or every time).
#[derive(Debug)]
struct Rule {
    pattern: String,
    output: Result<CmdOutput, u64>,
    once: bool,
}

#[derive(Debug, Default)]
struct State {
    commands: Vec<String>,
    rules: Vec<Rule>,
    http_failures: BTreeMap<u16, u32>,
    now: Duration,
}

/// A scripted [`Executor`].
///
/// By default: `docker` is on PATH, no environment variable is set, every
/// port is free, every HTTP probe answers 200, and every command succeeds
/// with empty output, except `redis-cli ping`, which answers `PONG` like a
/// ready Redis. [`commands`](FakeExecutor::commands) lists what ran, in
/// order; attached commands are prefixed with `[attached] `.
///
/// ```
/// use navree_dock::{Cmd, CmdOutput, Executor, FakeExecutor};
///
/// let exec = FakeExecutor::new().respond("docker ps", CmdOutput::ok("crm-api-redis|running|redis:7|\n"));
/// let output = exec.run(&Cmd::new("docker", ["ps", "--all"])).unwrap();
/// assert_eq!(output.stdout, "crm-api-redis|running|redis:7|\n");
/// assert_eq!(exec.commands(), ["docker ps --all"]);
/// ```
#[derive(Debug)]
pub struct FakeExecutor {
    state: Mutex<State>,
    programs: BTreeSet<String>,
    vars: BTreeMap<String, String>,
    taken: BTreeSet<u16>,
}

impl Default for FakeExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeExecutor {
    /// A fake with `docker` on PATH and everything answering.
    pub fn new() -> FakeExecutor {
        FakeExecutor {
            state: Mutex::new(State::default()),
            programs: BTreeSet::from(["docker".to_owned()]),
            vars: BTreeMap::new(),
            taken: BTreeSet::new(),
        }
    }

    /// Replaces the programs on PATH.
    pub fn with_programs<I>(mut self, programs: I) -> FakeExecutor
    where
        I: IntoIterator,
        I::Item: Into<String>,
    {
        self.programs = programs.into_iter().map(Into::into).collect();
        self
    }

    /// Sets the environment variable `name`.
    pub fn with_var(mut self, name: &str, value: &str) -> FakeExecutor {
        self.vars.insert(name.to_owned(), value.to_owned());
        self
    }

    /// Marks `port` on 127.0.0.1 as taken.
    pub fn take_port(mut self, port: u16) -> FakeExecutor {
        self.taken.insert(port);
        self
    }

    /// Commands whose line contains `pattern` answer `output`, every time.
    /// Rules added later win over earlier ones.
    pub fn respond(self, pattern: &str, output: CmdOutput) -> FakeExecutor {
        self.rule(pattern, Ok(output), false)
    }

    /// The next command whose line contains `pattern` answers `output`; the
    /// answer is used up. One-time answers win over [`respond`](Self::respond).
    pub fn respond_once(self, pattern: &str, output: CmdOutput) -> FakeExecutor {
        self.rule(pattern, Ok(output), true)
    }

    /// Commands whose line contains `pattern` hang until their timeout
    /// (which the virtual clock skips over), like a runtime whose daemon
    /// is not running.
    pub fn hang(self, pattern: &str) -> FakeExecutor {
        self.rule(pattern, Err(0), false)
    }

    /// HTTP probes of `port` fail `times` times before answering 200.
    pub fn http_ready_after(self, port: u16, times: u32) -> FakeExecutor {
        self.lock().http_failures.insert(port, times);
        self
    }

    /// The command lines run so far, in order.
    pub fn commands(&self) -> Vec<String> {
        self.lock().commands.clone()
    }

    /// The time the virtual clock has advanced.
    pub fn elapsed(&self) -> Duration {
        self.lock().now
    }

    fn rule(self, pattern: &str, output: Result<CmdOutput, u64>, once: bool) -> FakeExecutor {
        self.lock().rules.push(Rule {
            pattern: pattern.to_owned(),
            output,
            once,
        });
        self
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The scripted answer for `line`: one-time rules first, then the latest
    /// matching rule, then the default.
    fn answer(&self, line: &str) -> Result<CmdOutput, u64> {
        let mut state = self.lock();
        if let Some(index) = state
            .rules
            .iter()
            .position(|rule| rule.once && line.contains(&rule.pattern))
        {
            return state.rules.remove(index).output;
        }
        if let Some(rule) = state
            .rules
            .iter()
            .rev()
            .find(|rule| !rule.once && line.contains(&rule.pattern))
        {
            return rule.output.clone();
        }
        if line.contains("redis-cli ping") {
            return Ok(CmdOutput::ok("PONG\n"));
        }
        Ok(CmdOutput::ok(""))
    }
}

impl Executor for FakeExecutor {
    fn run(&self, cmd: &Cmd) -> Result<CmdOutput, DockError> {
        let line = cmd.to_string();
        self.lock().commands.push(line.clone());
        match self.answer(&line) {
            Ok(output) => Ok(output),
            Err(_) => {
                let limit = cmd.timeout.unwrap_or(Duration::from_secs(3600));
                self.sleep(limit);
                Err(DockError::Timeout {
                    command: line,
                    seconds: limit.as_secs(),
                })
            }
        }
    }

    fn run_attached(&self, cmd: &Cmd) -> Result<i32, DockError> {
        let line = cmd.to_string();
        self.lock().commands.push(format!("[attached] {line}"));
        match self.answer(&line) {
            Ok(output) => Ok(output.code),
            Err(_) => Err(DockError::Timeout {
                command: line,
                seconds: 0,
            }),
        }
    }

    fn has_program(&self, program: &str) -> bool {
        self.programs.contains(program)
    }

    fn var(&self, name: &str) -> Option<String> {
        self.vars.get(name).cloned()
    }

    fn port_taken(&self, port: u16) -> bool {
        self.taken.contains(&port)
    }

    fn http_status(&self, port: u16, _path: &str) -> Option<u16> {
        let mut state = self.lock();
        match state.http_failures.get_mut(&port) {
            Some(left) if *left > 0 => {
                *left -= 1;
                None
            }
            _ => Some(200),
        }
    }

    fn now(&self) -> Duration {
        self.lock().now
    }

    fn sleep(&self, duration: Duration) {
        self.lock().now += duration;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_commands_and_answers_from_the_script() {
        let exec = FakeExecutor::new()
            .respond("image inspect", CmdOutput::failed(1, "no such image"))
            .respond_once("image inspect redis", CmdOutput::ok("sha256:1"));
        let inspect = Cmd::new("docker", ["image", "inspect", "redis:7"]);
        assert_eq!(exec.run(&inspect).unwrap().stdout, "sha256:1");
        assert_eq!(exec.run(&inspect).unwrap().code, 1);
        assert_eq!(
            exec.run_attached(&Cmd::new("docker", ["pull", "redis:7"]))
                .unwrap(),
            0
        );
        assert_eq!(
            exec.commands(),
            [
                "docker image inspect redis:7",
                "docker image inspect redis:7",
                "[attached] docker pull redis:7"
            ]
        );
    }

    #[test]
    fn later_rules_win_and_redis_answers_pong() {
        let exec = FakeExecutor::new()
            .respond("ps", CmdOutput::ok("first"))
            .respond("ps", CmdOutput::ok("second"));
        assert_eq!(
            exec.run(&Cmd::new("docker", ["ps"])).unwrap().stdout,
            "second"
        );
        let ping = Cmd::new("docker", ["exec", "app-redis", "redis-cli", "ping"]);
        assert_eq!(exec.run(&ping).unwrap().stdout, "PONG\n");
    }

    #[test]
    fn hanging_commands_time_out_on_the_virtual_clock() {
        let exec = FakeExecutor::new().hang("docker ps");
        let cmd = Cmd::new("docker", ["ps"]).timeout(Duration::from_secs(30));
        let error = exec.run(&cmd).unwrap_err();
        assert!(
            matches!(error, DockError::Timeout { seconds: 30, .. }),
            "{error}"
        );
        assert_eq!(exec.elapsed(), Duration::from_secs(30));
    }

    #[test]
    fn ports_http_and_environment() {
        let exec = FakeExecutor::new()
            .take_port(5432)
            .http_ready_after(7700, 2)
            .with_var("NAVREE_DOCK_RUNTIME", "podman")
            .with_programs(["podman"]);
        assert!(exec.port_taken(5432));
        assert!(!exec.port_taken(6379));
        assert_eq!(exec.http_status(7700, "/health"), None);
        assert_eq!(exec.http_status(7700, "/health"), None);
        assert_eq!(exec.http_status(7700, "/health"), Some(200));
        assert_eq!(exec.var("NAVREE_DOCK_RUNTIME").as_deref(), Some("podman"));
        assert!(exec.has_program("podman") && !exec.has_program("docker"));
        exec.sleep(Duration::from_millis(500));
        assert_eq!(exec.now(), Duration::from_millis(500));
    }
}
