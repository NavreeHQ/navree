//! Running programs: `ssh`, `cargo`, `docker`, `git`. Every command Harbor
//! runs goes through an [`Executor`], so tests swap the system for a
//! [`FakeExecutor`] that records each command and answers from a script.

use std::fmt;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use crate::error::{HarborError, Result};

/// What a command reads on its standard input.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Input {
    /// Nothing (`/dev/null`), or the terminal for interactive commands.
    #[default]
    None,
    /// These bytes (a script, a file's contents).
    Bytes(Vec<u8>),
    /// A file's contents, streamed (a release archive).
    File(PathBuf),
}

/// A program, its arguments and its input.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cmd {
    /// The program, e.g. `ssh`.
    pub program: String,
    /// Its arguments.
    pub args: Vec<String>,
    /// Its standard input.
    pub stdin: Input,
    /// The working directory; the current one when `None`.
    pub cwd: Option<PathBuf>,
    /// Extra environment variables.
    pub env: Vec<(String, String)>,
    /// Write standard output to this file instead of capturing it.
    pub stdout_file: Option<PathBuf>,
}

impl Cmd {
    /// `program` with no arguments.
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            ..Self::default()
        }
    }

    /// Adds one argument.
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Adds arguments.
    pub fn args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Feeds `bytes` to standard input.
    pub fn stdin(mut self, bytes: impl Into<Vec<u8>>) -> Self {
        self.stdin = Input::Bytes(bytes.into());
        self
    }

    /// Streams `path` to standard input.
    pub fn stdin_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.stdin = Input::File(path.into());
        self
    }

    /// Runs in `dir`.
    pub fn cwd(mut self, dir: impl Into<PathBuf>) -> Self {
        self.cwd = Some(dir.into());
        self
    }

    /// Sets an environment variable.
    pub fn env(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((name.into(), value.into()));
        self
    }

    /// Writes standard output to `path`.
    pub fn stdout_to(mut self, path: impl Into<PathBuf>) -> Self {
        self.stdout_file = Some(path.into());
        self
    }

    /// The standard input as text (empty for none or a file).
    pub fn stdin_text(&self) -> String {
        match &self.stdin {
            Input::Bytes(bytes) => String::from_utf8_lossy(bytes).into_owned(),
            _ => String::new(),
        }
    }
}

impl fmt::Display for Cmd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.program)?;
        for arg in &self.args {
            if arg.is_empty() || arg.contains(|c: char| c.is_whitespace() || c == '"') {
                write!(f, " {arg:?}")?;
            } else {
                write!(f, " {arg}")?;
            }
        }
        Ok(())
    }
}

/// What a finished command returned.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Output {
    /// The exit code; `None` when a signal ended it.
    pub code: Option<i32>,
    /// Standard output (empty when it went to a file).
    pub stdout: String,
    /// Standard error.
    pub stderr: String,
}

impl Output {
    /// A successful run that printed `stdout`.
    pub fn ok(stdout: impl Into<String>) -> Self {
        Self {
            code: Some(0),
            stdout: stdout.into(),
            stderr: String::new(),
        }
    }

    /// A failed run with exit code `code` that printed `stderr`.
    pub fn failed(code: i32, stderr: impl Into<String>) -> Self {
        Self {
            code: Some(code),
            stdout: String::new(),
            stderr: stderr.into(),
        }
    }

    /// Whether it exited with 0.
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }

    /// Standard output and standard error together.
    pub fn combined(&self) -> String {
        match (self.stdout.trim().is_empty(), self.stderr.trim().is_empty()) {
            (true, _) => self.stderr.clone(),
            (false, true) => self.stdout.clone(),
            (false, false) => format!("{}\n{}", self.stdout.trim_end(), self.stderr),
        }
    }
}

/// Runs commands.
pub trait Executor: Send + Sync {
    /// Runs `cmd`, capturing its output.
    fn run(&self, cmd: &Cmd) -> Result<Output>;

    /// Runs `cmd` attached to the terminal (a shell, `--follow`, build
    /// output) and returns its exit code.
    fn interactive(&self, cmd: &Cmd) -> Result<Option<i32>>;
}

impl fmt::Debug for dyn Executor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Executor")
    }
}

/// Runs `cmd` and fails with [`HarborError::Local`] when it does not exit
/// with 0.
pub fn run_checked(exec: &dyn Executor, cmd: &Cmd) -> Result<Output> {
    let output = exec.run(cmd)?;
    if output.success() {
        Ok(output)
    } else {
        Err(HarborError::Local {
            command: cmd.to_string(),
            code: output.code,
            output: crate::error::tail(&output.combined(), 20),
        })
    }
}

/// Runs commands on this machine.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemExecutor;

impl SystemExecutor {
    fn command(cmd: &Cmd) -> Command {
        let mut command = Command::new(&cmd.program);
        command.args(&cmd.args);
        if let Some(dir) = &cmd.cwd {
            command.current_dir(dir);
        }
        for (name, value) in &cmd.env {
            command.env(name, value);
        }
        command
    }

    fn spawn_error(cmd: &Cmd, source: std::io::Error) -> HarborError {
        HarborError::Spawn {
            program: cmd.program.clone(),
            source,
        }
    }
}

impl Executor for SystemExecutor {
    fn run(&self, cmd: &Cmd) -> Result<Output> {
        let mut command = Self::command(cmd);
        command.stderr(Stdio::piped());
        command.stdin(match cmd.stdin {
            Input::None => Stdio::null(),
            _ => Stdio::piped(),
        });
        match &cmd.stdout_file {
            Some(path) => {
                let file = std::fs::File::create(path).map_err(|e| HarborError::io(path, e))?;
                command.stdout(file);
            }
            None => {
                command.stdout(Stdio::piped());
            }
        }
        let mut child = command.spawn().map_err(|e| Self::spawn_error(cmd, e))?;
        // Feed standard input from a thread so a chatty program can't
        // deadlock against a full pipe.
        let feeder = child.stdin.take().map(|mut stdin| {
            let input = cmd.stdin.clone();
            std::thread::spawn(move || -> std::io::Result<()> {
                match input {
                    Input::None => Ok(()),
                    Input::Bytes(bytes) => stdin.write_all(&bytes),
                    Input::File(path) => {
                        let mut file = std::fs::File::open(path)?;
                        std::io::copy(&mut file, &mut stdin).map(|_| ())
                    }
                }
            })
        });
        let mut stderr = String::new();
        let stderr_reader = child.stderr.take().map(|mut pipe| {
            std::thread::spawn(move || {
                let mut text = String::new();
                let _ = pipe.read_to_string(&mut text);
                text
            })
        });
        let mut stdout = String::new();
        if let Some(mut pipe) = child.stdout.take() {
            let _ = pipe.read_to_string(&mut stdout);
        }
        if let Some(reader) = stderr_reader {
            stderr = reader.join().unwrap_or_default();
        }
        let status = child.wait().map_err(|e| Self::spawn_error(cmd, e))?;
        if let Some(feeder) = feeder {
            // A program may exit without reading all of its input; its exit
            // code says whether that was a problem.
            let _ = feeder.join();
        }
        Ok(Output {
            code: status.code(),
            stdout,
            stderr,
        })
    }

    fn interactive(&self, cmd: &Cmd) -> Result<Option<i32>> {
        let mut command = Self::command(cmd);
        match &cmd.stdin {
            Input::None => {}
            Input::Bytes(_) | Input::File(_) => {
                command.stdin(Stdio::piped());
            }
        }
        let mut child = command.spawn().map_err(|e| Self::spawn_error(cmd, e))?;
        if let Some(mut stdin) = child.stdin.take() {
            match &cmd.stdin {
                Input::Bytes(bytes) => {
                    let _ = stdin.write_all(bytes);
                }
                Input::File(path) => {
                    let mut file =
                        std::fs::File::open(path).map_err(|e| HarborError::io(path, e))?;
                    let _ = std::io::copy(&mut file, &mut stdin);
                }
                Input::None => {}
            }
        }
        let status = child.wait().map_err(|e| Self::spawn_error(cmd, e))?;
        Ok(status.code())
    }
}

type Handler = dyn Fn(&Cmd) -> Output + Send + Sync;

/// An executor for tests: it records every command and answers with a
/// handler (success with no output by default).
#[derive(Clone)]
pub struct FakeExecutor {
    calls: Arc<Mutex<Vec<Cmd>>>,
    handler: Arc<Handler>,
}

impl fmt::Debug for FakeExecutor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FakeExecutor")
            .field("calls", &self.calls().len())
            .finish()
    }
}

impl Default for FakeExecutor {
    fn default() -> Self {
        Self::new(|_| Output::ok(""))
    }
}

impl FakeExecutor {
    /// An executor that answers every command with `handler`.
    pub fn new(handler: impl Fn(&Cmd) -> Output + Send + Sync + 'static) -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            handler: Arc::new(handler),
        }
    }

    /// Every command run so far, in order.
    pub fn calls(&self) -> Vec<Cmd> {
        self.calls.lock().expect("calls lock").clone()
    }

    /// The command lines run so far.
    pub fn command_lines(&self) -> Vec<String> {
        self.calls().iter().map(ToString::to_string).collect()
    }

    fn record(&self, cmd: &Cmd) -> Output {
        let mut recorded = cmd.clone();
        // Keep a streamed file's contents, so tests can look inside an
        // upload after its temporary file is gone.
        if let Input::File(path) = &cmd.stdin
            && let Ok(bytes) = std::fs::read(path)
        {
            recorded.stdin = Input::Bytes(bytes);
        }
        self.calls.lock().expect("calls lock").push(recorded);
        let output = (self.handler)(cmd);
        if let Some(path) = &cmd.stdout_file {
            let _ = std::fs::write(path, &output.stdout);
        }
        output
    }
}

impl Executor for FakeExecutor {
    fn run(&self, cmd: &Cmd) -> Result<Output> {
        Ok(self.record(cmd))
    }

    fn interactive(&self, cmd: &Cmd) -> Result<Option<i32>> {
        Ok(self.record(cmd).code)
    }
}

/// Whether `program` can be started (`<program> --version` succeeds).
pub fn available(exec: &dyn Executor, program: &str) -> bool {
    exec.run(&Cmd::new(program).arg("--version"))
        .map(|o| o.success())
        .unwrap_or(false)
}

/// `path` with `/` separators (for `ssh -o` values and Docker volumes).
pub fn slash_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_display_like_a_shell_line() {
        let cmd = Cmd::new("ssh").args(["-p", "22", "a b", ""]);
        assert_eq!(cmd.to_string(), r#"ssh -p 22 "a b" """#);
    }

    #[test]
    fn the_fake_records_and_answers() {
        let fake = FakeExecutor::new(|cmd| {
            if cmd.program == "git" {
                Output::ok("3f2a9c1\n")
            } else {
                Output::failed(1, "nope")
            }
        });
        let out = fake.run(&Cmd::new("git").arg("rev-parse")).unwrap();
        assert_eq!(out.stdout, "3f2a9c1\n");
        let out = fake.run(&Cmd::new("docker").stdin("script")).unwrap();
        assert!(!out.success());
        assert_eq!(fake.calls().len(), 2);
        assert_eq!(fake.calls()[1].stdin_text(), "script");
        assert_eq!(fake.command_lines(), ["git rev-parse", "docker"]);
    }

    #[test]
    fn run_checked_reports_the_command_and_output() {
        let fake = FakeExecutor::new(|_| Output::failed(101, "error: could not compile"));
        let error = run_checked(&fake, &Cmd::new("cargo").arg("build")).unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("`cargo build` failed with exit code 101"),
            "{message}"
        );
        assert!(message.contains("could not compile"), "{message}");
    }

    #[test]
    fn the_system_executor_captures_output_and_feeds_input() {
        let exec = SystemExecutor;
        let (program, args): (&str, &[&str]) = if cfg!(windows) {
            ("cmd", &["/C", "more"])
        } else {
            ("cat", &[])
        };
        let out = exec
            .run(
                &Cmd::new(program)
                    .args(args.iter().copied())
                    .stdin("hello\n"),
            )
            .unwrap();
        assert!(out.success(), "{out:?}");
        assert!(out.stdout.contains("hello"), "{out:?}");
        let error = exec
            .run(&Cmd::new("navree-harbor-no-such-program"))
            .unwrap_err();
        assert!(error.to_string().contains("is it installed"), "{error}");
    }

    #[test]
    fn combined_output() {
        assert_eq!(Output::ok("a\n").combined(), "a\n");
        assert_eq!(Output::failed(1, "b").combined(), "b");
        let both = Output {
            code: Some(1),
            stdout: "a\n".into(),
            stderr: "b".into(),
        };
        assert_eq!(both.combined(), "a\nb");
    }
}
