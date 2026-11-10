//! Everything Dock does outside its own memory goes through an [`Executor`]:
//! commands, the environment, ports, HTTP readiness probes and the clock.
//! [`ProcessExecutor`] does it for real; [`FakeExecutor`](crate::FakeExecutor)
//! records commands and answers from a script, so Dock is tested without
//! Docker.

use std::fmt;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::error::DockError;

/// A command line to run: a program, its arguments and extra environment
/// variables. Dock runs every command in the app's directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cmd {
    /// The program, e.g. `docker`.
    pub program: String,
    /// Its arguments.
    pub args: Vec<String>,
    /// How long a captured run may take before it is stopped.
    pub timeout: Option<Duration>,
}

impl Cmd {
    /// `program` with `args`, without a time limit.
    pub fn new(
        program: impl Into<String>,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> Cmd {
        Cmd {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
            timeout: None,
        }
    }

    /// The same command, stopped after `timeout` when run captured.
    pub fn timeout(mut self, timeout: Duration) -> Cmd {
        self.timeout = Some(timeout);
        self
    }
}

impl fmt::Display for Cmd {
    /// The command as you would type it in a POSIX shell.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&shell_word(&self.program))?;
        for arg in &self.args {
            write!(f, " {}", shell_word(arg))?;
        }
        Ok(())
    }
}

/// `word` quoted for a POSIX shell when it needs it.
pub(crate) fn shell_word(word: &str) -> String {
    let plain = !word.is_empty()
        && word.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(c, '_' | '-' | '.' | '/' | ':' | '@' | '%' | '+' | '=' | ',')
        });
    if plain {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}

/// What a captured command did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CmdOutput {
    /// The exit code; -1 when it was killed by a signal.
    pub code: i32,
    /// Its standard output.
    pub stdout: String,
    /// Its standard error.
    pub stderr: String,
}

impl CmdOutput {
    /// Exit code 0 with `stdout`.
    pub fn ok(stdout: impl Into<String>) -> CmdOutput {
        CmdOutput {
            code: 0,
            stdout: stdout.into(),
            stderr: String::new(),
        }
    }

    /// Exit code `code` with `stderr`.
    pub fn failed(code: i32, stderr: impl Into<String>) -> CmdOutput {
        CmdOutput {
            code,
            stdout: String::new(),
            stderr: stderr.into(),
        }
    }

    /// Whether it exited with 0.
    pub fn success(&self) -> bool {
        self.code == 0
    }

    /// The error for `command` when it failed, with what it printed.
    pub(crate) fn check(self, command: &Cmd) -> Result<CmdOutput, DockError> {
        if self.success() {
            return Ok(self);
        }
        let output = if self.stderr.trim().is_empty() {
            self.stdout.clone()
        } else {
            self.stderr.clone()
        };
        Err(DockError::CommandFailed {
            command: command.to_string(),
            code: self.code,
            output,
        })
    }
}

/// The outside world, as Dock sees it.
pub trait Executor {
    /// Runs `cmd` in the app's directory with its output captured, and stops
    /// it after its timeout ([`DockError::Timeout`]).
    fn run(&self, cmd: &Cmd) -> Result<CmdOutput, DockError>;

    /// Runs `cmd` attached to the terminal (pulls, logs, shells) and returns
    /// its exit code.
    fn run_attached(&self, cmd: &Cmd) -> Result<i32, DockError>;

    /// Whether `program` is on PATH.
    fn has_program(&self, program: &str) -> bool;

    /// The environment variable `name`, when set.
    fn var(&self, name: &str) -> Option<String>;

    /// Whether something already listens on, or holds, `127.0.0.1:port`.
    fn port_taken(&self, port: u16) -> bool;

    /// The HTTP status of `GET http://127.0.0.1:{port}{path}`, or `None`
    /// when nothing answers.
    fn http_status(&self, port: u16, path: &str) -> Option<u16>;

    /// Time elapsed on a monotonic clock.
    fn now(&self) -> Duration;

    /// Waits for `duration`.
    fn sleep(&self, duration: Duration);
}

/// The real [`Executor`]: processes, sockets and the system clock.
#[derive(Debug)]
pub struct ProcessExecutor {
    dir: PathBuf,
    start: Instant,
}

impl ProcessExecutor {
    /// Runs commands in `dir` (the app's directory).
    pub fn new(dir: impl Into<PathBuf>) -> ProcessExecutor {
        ProcessExecutor {
            dir: dir.into(),
            start: Instant::now(),
        }
    }

    fn command(&self, cmd: &Cmd) -> Command {
        let mut command = Command::new(&cmd.program);
        command.args(&cmd.args).current_dir(&self.dir);
        command
    }
}

impl Executor for ProcessExecutor {
    fn run(&self, cmd: &Cmd) -> Result<CmdOutput, DockError> {
        let spawn_error = |source| DockError::Spawn {
            command: cmd.to_string(),
            source,
        };
        let mut child = self
            .command(cmd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(spawn_error)?;
        let stdout = read_in_background(child.stdout.take());
        let stderr = read_in_background(child.stderr.take());
        let started = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().map_err(spawn_error)? {
                break status;
            }
            if let Some(limit) = cmd.timeout
                && started.elapsed() >= limit
            {
                // Already exited is fine; nothing else to do on failure.
                let _ = child.kill();
                let _ = child.wait();
                return Err(DockError::Timeout {
                    command: cmd.to_string(),
                    seconds: limit.as_secs(),
                });
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        Ok(CmdOutput {
            code: status.code().unwrap_or(-1),
            stdout: stdout.join().unwrap_or_default(),
            stderr: stderr.join().unwrap_or_default(),
        })
    }

    fn run_attached(&self, cmd: &Cmd) -> Result<i32, DockError> {
        let status = self
            .command(cmd)
            .status()
            .map_err(|source| DockError::Spawn {
                command: cmd.to_string(),
                source,
            })?;
        Ok(status.code().unwrap_or(-1))
    }

    fn has_program(&self, program: &str) -> bool {
        find_on_path(
            program,
            std::env::var_os("PATH"),
            std::env::var_os("PATHEXT"),
        )
        .is_some()
    }

    fn var(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }

    fn port_taken(&self, port: u16) -> bool {
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        if TcpListener::bind(address).is_err() {
            return true;
        }
        // A program listening on 0.0.0.0 may let the bind above succeed (on
        // Windows), but it answers a connection.
        TcpStream::connect_timeout(&address, Duration::from_millis(200)).is_ok()
    }

    fn http_status(&self, port: u16, path: &str) -> Option<u16> {
        http_get_status(port, path, Duration::from_secs(2))
    }

    fn now(&self) -> Duration {
        self.start.elapsed()
    }

    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// Reads a pipe to its end on another thread, so a chatty child never
/// blocks on a full pipe while it is waited for.
fn read_in_background<R: Read + Send + 'static>(
    pipe: Option<R>,
) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            // A read error leaves what was read so far.
            let _ = pipe.read_to_end(&mut bytes);
        }
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

/// `GET path` on 127.0.0.1:port over HTTP/1.1; the status code of the
/// answer, or `None` when nothing answers in time.
pub(crate) fn http_get_status(port: u16, path: &str, timeout: Duration) -> Option<u16> {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&address, timeout).ok()?;
    stream.set_read_timeout(Some(timeout)).ok()?;
    stream.set_write_timeout(Some(timeout)).ok()?;
    let request =
        format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).ok()?;
    let mut head = Vec::new();
    let mut byte = [0_u8; 1];
    while head.len() < 256 {
        match stream.read(&mut byte) {
            Ok(1) if byte[0] == b'\n' => break,
            Ok(1) => head.push(byte[0]),
            _ => break,
        }
    }
    parse_status_line(&String::from_utf8_lossy(&head))
}

/// The status code of an HTTP status line (`HTTP/1.1 200 OK`).
fn parse_status_line(line: &str) -> Option<u16> {
    let mut parts = line.split_whitespace();
    parts
        .next()
        .filter(|version| version.starts_with("HTTP/"))?;
    parts.next()?.parse().ok()
}

/// The path of `program` in the directories of `path`, trying the
/// extensions of `pathext` (Windows) too.
pub(crate) fn find_on_path(
    program: &str,
    path: Option<std::ffi::OsString>,
    pathext: Option<std::ffi::OsString>,
) -> Option<PathBuf> {
    let extensions: Vec<String> = pathext
        .map(|exts| {
            exts.to_string_lossy()
                .split(';')
                .filter(|e| !e.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    std::env::split_paths(&path?).find_map(|dir| {
        let plain = dir.join(program);
        if is_file(&plain) && (cfg!(not(windows)) || extensions.is_empty()) {
            return Some(plain);
        }
        extensions
            .iter()
            .flat_map(|ext| [ext.to_lowercase(), ext.to_uppercase()])
            .map(|ext| dir.join(format!("{program}{ext}")))
            .find(|candidate| is_file(candidate))
    })
}

fn is_file(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_print_like_a_shell_line() {
        let cmd = Cmd::new(
            "docker",
            ["ps", "--format", "{{.Names}}|{{.State}}", "it's", ""],
        );
        assert_eq!(
            cmd.to_string(),
            r"docker ps --format '{{.Names}}|{{.State}}' 'it'\''s' ''"
        );
    }

    #[test]
    fn status_lines_parse() {
        assert_eq!(parse_status_line("HTTP/1.1 200 OK\r"), Some(200));
        assert_eq!(
            parse_status_line("HTTP/1.0 503 Service Unavailable"),
            Some(503)
        );
        assert_eq!(parse_status_line("SSH-2.0-OpenSSH"), None);
        assert_eq!(parse_status_line(""), None);
    }

    #[test]
    fn http_probe_reads_the_status_of_a_local_server() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 512];
            let read = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..read]).into_owned();
            stream
                .write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n")
                .unwrap();
            request
        });
        assert_eq!(
            http_get_status(port, "/health", Duration::from_secs(2)),
            Some(204)
        );
        let request = server.join().unwrap();
        assert!(request.starts_with("GET /health HTTP/1.1\r\n"), "{request}");
    }

    #[test]
    fn http_probe_of_a_closed_port_is_none() {
        let port = {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            listener.local_addr().unwrap().port()
        };
        assert_eq!(http_get_status(port, "/", Duration::from_millis(500)), None);
    }

    /// Set in the process [`run_alone`] starts.
    const ALONE: &str = "NAVREE_DOCK_TEST_ALONE";

    /// Runs the test `name` of this module again in a new process of this
    /// test binary, with no other test beside it, and fails if it fails.
    fn run_alone(name: &str) {
        let module = module_path!()
            .split_once("::")
            .map_or(module_path!(), |(_, path)| path);
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                format!("{module}::{name}").as_str(),
                "--exact",
                "--test-threads=1",
                "--nocapture",
            ])
            .env(ALONE, "1")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.status.success(), "{text}");
        assert!(text.contains(" 1 passed;"), "the test ran: {text}");
    }

    /// Run alone, in a process of its own. On Unix, starting a process copies
    /// every socket of the process that starts it into the child, which holds
    /// them until it runs its program, and tests beside this one start
    /// processes (one looks for a program on every directory of `PATH`). One
    /// started in the instant this test closes its listener would keep the
    /// port in use for the check that follows.
    #[test]
    fn a_listening_port_is_taken_and_a_closed_one_is_not() {
        if std::env::var_os(ALONE).is_none() {
            return run_alone("a_listening_port_is_taken_and_a_closed_one_is_not");
        }
        let exec = ProcessExecutor::new(".");
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(exec.port_taken(port));
        drop(listener);
        assert!(!exec.port_taken(port));
    }

    #[test]
    fn captured_runs_return_output_and_exit_code() {
        let exec = ProcessExecutor::new(std::env::temp_dir());
        let output = exec
            .run(&Cmd::new("cargo", ["--version"]).timeout(Duration::from_secs(60)))
            .unwrap();
        assert!(output.success(), "{output:?}");
        assert!(output.stdout.starts_with("cargo "), "{output:?}");
        let failed = exec
            .run(&Cmd::new("cargo", ["no-such-subcommand-for-dock"]))
            .unwrap();
        assert!(!failed.success());
        let missing = exec.run(&Cmd::new("no-such-program-for-dock", ["x"]));
        assert!(
            matches!(missing, Err(DockError::Spawn { .. })),
            "{missing:?}"
        );
    }

    #[test]
    fn programs_are_found_on_path_with_windows_extensions() {
        let dir = tempfile::Builder::new()
            .prefix("navree-test-")
            .tempdir()
            .unwrap();
        std::fs::write(dir.path().join("podman.exe"), "").unwrap();
        std::fs::write(dir.path().join("docker"), "").unwrap();
        let path = Some(dir.path().as_os_str().to_owned());
        let found = find_on_path("podman", path.clone(), Some(".COM;.EXE".into()));
        assert_eq!(
            found.map(|p| p.file_name().unwrap().to_ascii_lowercase()),
            Some("podman.exe".into())
        );
        assert!(find_on_path("docker", path.clone(), None).is_some());
        assert!(find_on_path("nerdctl", path, Some(".EXE".into())).is_none());
    }

    #[test]
    fn failed_output_becomes_an_error_with_stderr_or_stdout() {
        let cmd = Cmd::new("docker", ["compose", "up"]);
        let error = CmdOutput::failed(2, "boom").check(&cmd).unwrap_err();
        assert_eq!(
            error.to_string(),
            "`docker compose up` failed (exit code 2):\nboom"
        );
        let stdout_only = CmdOutput {
            code: 1,
            stdout: "from stdout".into(),
            stderr: " ".into(),
        };
        assert!(
            stdout_only
                .check(&cmd)
                .unwrap_err()
                .to_string()
                .ends_with("from stdout")
        );
        assert!(CmdOutput::ok("fine").check(&cmd).is_ok());
    }
}
