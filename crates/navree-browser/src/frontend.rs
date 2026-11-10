//! [`FrontEnd`] and [`WebSession`]: a web front end started once and
//! pointed, through a small proxy, at the app of the test using it.

use std::collections::{HashMap, VecDeque};
use std::ffi::{OsStr, OsString};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, LazyLock, Mutex, RwLock, Weak};
use std::time::{Duration, Instant};

use http::{HeaderName, StatusCode, Uri, header};
use hyper_util::rt::TokioIo;
use navree_http::AppRouter;
use navree_http::body::Body;
use navree_http::extract::Request;
use navree_http::response::{IntoResponse, Response};
use navree_kernel::AppContext;
use serde_json::json;
use tokio::sync::OwnedMutexGuard;

use crate::server::join_url;
use crate::{BrowserError, TestServer, runtime};

/// The default time a front end may take to answer.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

/// A web front end (with a BFF) that browser tests run against: started on
/// the first [`serve`](FrontEnd::serve), given the address of a proxy in
/// its API environment variable, and the proxy is pointed at the app of
/// whichever test holds the [`WebSession`].
///
/// ```no_run
/// # async fn demo(ctx: &navree_kernel::AppContext) -> Result<(), navree_browser::BrowserError> {
/// use navree_browser::FrontEnd;
///
/// let web = FrontEnd::new("crm-web")
///     .command("pnpm", ["--dir", "../crm-web", "start", "--port", "3100"])
///     .url("http://127.0.0.1:3100")
///     .api_env("NAVREE_API_URL")
///     .serve(ctx)
///     .await?;
/// let login = web.url("/login");
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug)]
#[must_use = "call .serve(ctx) to start the front end"]
pub struct FrontEnd {
    name: String,
    program: Option<OsString>,
    args: Vec<OsString>,
    url: Option<String>,
    api_env: Option<String>,
    env: Vec<(OsString, OsString)>,
    timeout: Duration,
}

impl FrontEnd {
    /// A front end, identified by `name` within the test binary: every
    /// `serve` of that name shares one process (started with the settings
    /// of the first).
    pub fn new(name: impl Into<String>) -> FrontEnd {
        FrontEnd {
            name: name.into(),
            program: None,
            args: Vec::new(),
            url: None,
            api_env: None,
            env: Vec::new(),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// How to start it: a program on `PATH` (on Windows also `.cmd` and
    /// `.bat` shims such as `pnpm`) and its arguments, run in the current
    /// directory.
    pub fn command<I, S>(mut self, program: impl AsRef<OsStr>, args: I) -> FrontEnd
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.program = Some(program.as_ref().to_owned());
        self.args = args.into_iter().map(|a| a.as_ref().to_owned()).collect();
        self
    }

    /// Where it listens; `serve` checks that nothing listens on its port yet,
    /// then waits until this URL answers.
    pub fn url(mut self, base: impl Into<String>) -> FrontEnd {
        self.url = Some(base.into());
        self
    }

    /// The environment variable that tells it where the API is (it is set
    /// to the proxy's `http://127.0.0.1:<port>`).
    pub fn api_env(mut self, var: impl Into<String>) -> FrontEnd {
        self.api_env = Some(var.into());
        self
    }

    /// Any other environment variable.
    pub fn env(mut self, var: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> FrontEnd {
        self.env
            .push((var.as_ref().to_owned(), value.as_ref().to_owned()));
        self
    }

    /// How long it may take to start (default 60 s).
    pub fn timeout(mut self, timeout: Duration) -> FrontEnd {
        self.timeout = timeout;
        self
    }

    /// Starts the front end if it is not running, serves `ctx`'s app on a
    /// [`TestServer`], routes the front end's API calls to it, and returns
    /// the test's turn with the front end. Tests sharing a front end wait
    /// for each other's sessions to end.
    pub async fn serve(&self, ctx: &AppContext) -> Result<WebSession, BrowserError> {
        let base = self.check()?;
        let shared = Shared::get(self).await?;
        let turn = shared.turn.clone().lock_owned().await;
        shared.start().await?;
        let server = TestServer::start(ctx).await?;
        *write(&shared.proxy.upstream) = Some(server.addr());
        Ok(WebSession {
            base,
            server: Some(server),
            _turn: turn,
            shared,
        })
    }

    fn check(&self) -> Result<String, BrowserError> {
        let name = &self.name;
        if self.program.is_none() {
            return Err(BrowserError::Config(format!(
                "FrontEnd::new({name:?}) needs .command(program, args)"
            )));
        }
        let url = self.url.as_deref().ok_or_else(|| {
            BrowserError::Config(format!("FrontEnd::new({name:?}) needs .url(base)"))
        })?;
        let uri: Uri = url.parse().map_err(|_| {
            BrowserError::Config(format!("FrontEnd::new({name:?}).url({url:?}) is not a URL"))
        })?;
        if uri.scheme_str() != Some("http") || uri.host().is_none() {
            return Err(BrowserError::Config(format!(
                "FrontEnd::new({name:?}).url({url:?}) must be an http:// URL"
            )));
        }
        Ok(url.trim_end_matches('/').to_owned())
    }
}

/// A test's turn with a front end: the front end's URLs reach this test's
/// app until the session is dropped.
pub struct WebSession {
    base: String,
    server: Option<TestServer>,
    _turn: OwnedMutexGuard<()>,
    shared: Arc<Shared>,
}

impl WebSession {
    /// The front end's URL for `path`: `web.url("/login")`.
    pub fn url(&self, path: &str) -> String {
        join_url(&self.base, path)
    }

    /// The app's server, which the front end's API calls reach.
    pub fn server(&self) -> &TestServer {
        self.server.as_ref().expect("the session has its server")
    }
}

impl Drop for WebSession {
    fn drop(&mut self) {
        *write(&self.shared.proxy.upstream) = None;
        self.server.take();
    }
}

impl std::fmt::Debug for WebSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebSession")
            .field("front_end", &self.shared.front.name)
            .field("url", &self.base)
            .field("server", &self.server)
            .finish()
    }
}

/// The front ends of the binary that tests are using or waiting for.
static FRONT_ENDS: LazyLock<tokio::sync::Mutex<HashMap<String, Weak<Shared>>>> =
    LazyLock::new(Default::default);

/// One front end shared by the tests: its process and its proxy.
struct Shared {
    front: FrontEnd,
    /// Held by the [`WebSession`] of the test using the front end.
    turn: Arc<tokio::sync::Mutex<()>>,
    process: tokio::sync::Mutex<Option<Running>>,
    proxy: Proxy,
}

impl Shared {
    async fn get(front: &FrontEnd) -> Result<Arc<Shared>, BrowserError> {
        let mut all = FRONT_ENDS.lock().await;
        if let Some(shared) = all.get(&front.name).and_then(Weak::upgrade) {
            return Ok(shared);
        }
        let shared = Arc::new(Shared {
            front: front.clone(),
            turn: Arc::default(),
            process: tokio::sync::Mutex::new(None),
            proxy: Proxy::start().await?,
        });
        all.insert(front.name.clone(), Arc::downgrade(&shared));
        Ok(shared)
    }

    /// Starts the process unless it runs; fails if it has exited.
    async fn start(&self) -> Result<(), BrowserError> {
        let front = &self.front;
        let name = &front.name;
        let url = front.url.clone().unwrap_or_default();
        let mut process = self.process.lock().await;
        if let Some(running) = process.as_mut() {
            match running.child.try_wait() {
                Ok(None) => return Ok(()),
                status => {
                    let status = status.ok().flatten().map(|s| s.to_string());
                    let tail = crate::process::tail(&running.output);
                    *process = None;
                    return Err(BrowserError::Server(format!(
                        "the front end {name:?} exited ({}){tail}",
                        status.unwrap_or_else(|| "unknown status".to_owned())
                    )));
                }
            }
        }
        // A server already listening there may answer only later (a dev server
        // compiling its first page): the front end would then fail to listen, and
        // the test would talk to the other server.
        if let Some(taken) = port_taken(&url).await {
            return Err(BrowserError::Server(format!(
                "{taken} before the front end {name:?} started: stop whatever listens there, \
                 or give the front end another port"
            )));
        }
        if answers(&url).await {
            return Err(BrowserError::Server(format!(
                "{url} already answers before the front end {name:?} started: stop whatever \
                 listens there, or give the front end another port"
            )));
        }
        let program = front.program.clone().unwrap_or_default();
        let resolved = resolve_program(&program).ok_or_else(|| {
            BrowserError::Server(format!(
                "cannot start the front end {name:?}: {} was not found on PATH",
                program.to_string_lossy()
            ))
        })?;
        let mut command = Command::new(&resolved);
        command
            .args(&front.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(var) = &front.api_env {
            command.env(var, format!("http://{}", self.proxy.addr));
        }
        for (var, value) in &front.env {
            command.env(var, value);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command.spawn().map_err(|e| {
            BrowserError::Server(format!(
                "cannot start the front end {name:?} ({}): {e}",
                resolved.display()
            ))
        })?;
        let output = Arc::new(Mutex::new(VecDeque::new()));
        drain_into(child.stdout.take(), &output);
        drain_into(child.stderr.take(), &output);
        let mut running = Running { child, output };
        wait_for(&mut running, front, &url).await?;
        *process = Some(running);
        Ok(())
    }
}

/// Waits until the front end answers at `url`. An error when it exits first,
/// when `url` answers but the front end has exited (one that could not listen
/// there exits, and another server answered), or past the front end's timeout,
/// when it is stopped.
async fn wait_for(running: &mut Running, front: &FrontEnd, url: &str) -> Result<(), BrowserError> {
    let name = &front.name;
    let deadline = Instant::now() + front.timeout;
    loop {
        if answers(url).await {
            return match running.child.try_wait() {
                Ok(Some(status)) => Err(BrowserError::Server(format!(
                    "{url} answered, but the front end {name:?} had exited ({status}): another \
                     server answers there{}",
                    crate::process::tail(&running.output)
                ))),
                _ => Ok(()),
            };
        }
        if let Ok(Some(status)) = running.child.try_wait() {
            return Err(BrowserError::Server(format!(
                "the front end {name:?} exited ({status}) before {url} answered{}",
                crate::process::tail(&running.output)
            )));
        }
        if Instant::now() >= deadline {
            let tail = crate::process::tail(&running.output);
            running.stop();
            return Err(BrowserError::Server(format!(
                "the front end {name:?} did not answer at {url} within {}{tail}",
                crate::error::format_duration(front.timeout)
            )));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// What holds the port of `url`, when something listens there already (a
/// server that does not answer yet holds it too) or it cannot be listened on;
/// `None` when it is free, or when `url` is not on this machine. The port is
/// tried on the addresses `url` names and on every address: a server
/// listening on all of them holds it whichever one `url` names.
async fn port_taken(url: &str) -> Option<String> {
    let uri = url.parse::<Uri>().ok()?;
    let host = uri
        .host()?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_owned();
    let port = uri.port_u16().unwrap_or(80);
    let named: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), port))
        .await
        .map(Iterator::collect)
        .unwrap_or_default();
    let taken = |addr: SocketAddr| match std::net::TcpListener::bind(addr) {
        Ok(_) => Some(false),
        Err(error) if in_use(&error) => Some(true),
        // Not an address of this machine, or not one it has (IPv6 off).
        Err(_) => None,
    };
    let mut local = false;
    for addr in named {
        match taken(addr) {
            Some(true) => return Some(format!("{host}:{port} is already in use")),
            Some(false) => local = true,
            None => {}
        }
    }
    if !local {
        return None;
    }
    let every = [
        SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)),
        SocketAddr::from((Ipv6Addr::UNSPECIFIED, port)),
    ];
    every
        .into_iter()
        .any(|addr| taken(addr) == Some(true))
        .then(|| format!("{host}:{port} is already in use"))
}

/// Whether binding failed because the port is taken. Windows answers that
/// access is denied for a port another program holds for itself alone, or
/// that the system keeps.
fn in_use(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::AddrInUse
        || (cfg!(windows) && error.kind() == std::io::ErrorKind::PermissionDenied)
}

/// A started front-end process.
struct Running {
    child: Child,
    output: Arc<Mutex<VecDeque<String>>>,
}

impl Running {
    /// Stops the process and everything it started.
    fn stop(&mut self) {
        if let Ok(Some(_)) = self.child.try_wait() {
            return;
        }
        #[cfg(windows)]
        {
            let _ = Command::new("taskkill")
                .args(["/PID", &self.child.id().to_string(), "/T", "/F"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        #[cfg(unix)]
        signal_group(&self.child, rustix::process::Signal::TERM);
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        #[cfg(unix)]
        signal_group(&self.child, rustix::process::Signal::KILL);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Sends `signal` to the process group the front end leads (it starts with
/// `process_group(0)`, so the group's id is its pid), with the system call
/// itself. Never through a `kill` program: the one of procps 3.3 (Ubuntu
/// 22.04) reads `kill -TERM -<pid>` as `kill(0)` and signals the caller's
/// own group instead, which on a CI runner holds the test binary, cargo and
/// the runner itself.
#[cfg(unix)]
fn signal_group(child: &Child, signal: rustix::process::Signal) {
    let _ = rustix::process::kill_process_group(rustix::process::Pid::from_child(child), signal);
}

/// The proxy the front end's API calls go through, on the background
/// runtime.
struct Proxy {
    addr: SocketAddr,
    upstream: Arc<RwLock<Option<SocketAddr>>>,
    task: tokio::task::JoinHandle<()>,
}

type Upstream = Arc<RwLock<Option<SocketAddr>>>;

fn write(
    upstream: &RwLock<Option<SocketAddr>>,
) -> std::sync::RwLockWriteGuard<'_, Option<SocketAddr>> {
    upstream.write().unwrap_or_else(|e| e.into_inner())
}

impl Proxy {
    async fn start() -> Result<Proxy, BrowserError> {
        let upstream: Upstream = Arc::default();
        let (bound, address) = tokio::sync::oneshot::channel();
        let state = upstream.clone();
        let task = runtime::handle().spawn(async move {
            let listener = match tokio::net::TcpListener::bind("127.0.0.1:0").await {
                Ok(listener) => listener,
                Err(error) => {
                    let _ = bound.send(Err(error));
                    return;
                }
            };
            let _ = bound.send(listener.local_addr());
            let app =
                AppRouter::from_handler(move |request: Request| forward(state.clone(), request));
            let _ = navree_http::serve(listener, app).await;
        });
        let addr = address
            .await
            .map_err(|_| BrowserError::Server("the front-end proxy did not start".to_owned()))?
            .map_err(|e| BrowserError::io("cannot start the front-end proxy", e))?;
        Ok(Proxy {
            addr,
            upstream,
            task,
        })
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Headers that belong to one connection, not to the request.
const HOP_BY_HOP: [HeaderName; 6] = [
    header::CONNECTION,
    HeaderName::from_static("keep-alive"),
    HeaderName::from_static("proxy-connection"),
    header::TE,
    header::TRAILER,
    header::UPGRADE,
];

async fn forward(upstream: Upstream, request: Request) -> Response {
    let target = *upstream.read().unwrap_or_else(|e| e.into_inner());
    let Some(addr) = target else {
        return problem(
            StatusCode::SERVICE_UNAVAILABLE,
            "no browser test is using this front end now",
        );
    };
    match send(addr, request).await {
        Ok(response) => response,
        Err(error) => problem(
            StatusCode::BAD_GATEWAY,
            &format!("the test app at {addr} did not answer: {error}"),
        ),
    }
}

fn problem(status: StatusCode, message: &str) -> Response {
    let body = json!({
        "statusCode": status.as_u16(),
        "message": message,
        "error": status.canonical_reason().unwrap_or_default(),
    });
    (status, navree_http::Json(body)).into_response()
}

type BoxError = Box<dyn std::error::Error + Send + Sync>;

async fn send(addr: SocketAddr, request: Request) -> Result<Response, BoxError> {
    let stream = tokio::net::TcpStream::connect(addr).await?;
    let (mut sender, connection) =
        hyper::client::conn::http1::handshake(TokioIo::new(stream)).await?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let (mut parts, body) = request.into_parts();
    parts.uri = parts
        .uri
        .path_and_query()
        .map_or("/", |pq| pq.as_str())
        .parse()?;
    for name in &HOP_BY_HOP {
        parts.headers.remove(name);
    }
    let response = sender
        .send_request(http::Request::from_parts(parts, body))
        .await?;
    let (mut parts, body) = response.into_parts();
    for name in &HOP_BY_HOP {
        parts.headers.remove(name);
    }
    Ok(Response::from_parts(parts, Body::new(body)))
}

/// Whether anything answers HTTP at `url` (any status).
async fn answers(url: &str) -> bool {
    let Ok(uri) = url.parse::<Uri>() else {
        return false;
    };
    let Some(host) = uri.host() else {
        return false;
    };
    let port = uri.port_u16().unwrap_or(80);
    let host = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_owned();
    let path = uri
        .path_and_query()
        .map_or("/", |pq| pq.as_str())
        .to_owned();
    let attempt = async move {
        let stream = tokio::net::TcpStream::connect((host.as_str(), port))
            .await
            .ok()?;
        let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .ok()?;
        tokio::spawn(async move {
            let _ = connection.await;
        });
        let request = http::Request::get(path)
            .header(header::HOST, format!("{host}:{port}"))
            .body(http_body_util::Empty::<navree_http::body::Bytes>::new())
            .ok()?;
        sender.send_request(request).await.ok().map(drop)
    };
    matches!(
        tokio::time::timeout(Duration::from_secs(5), attempt).await,
        Ok(Some(()))
    )
}

/// Reads a child's output on a thread into `lines` (the last 40 kept).
fn drain_into(
    stream: Option<impl std::io::Read + Send + 'static>,
    lines: &Arc<Mutex<VecDeque<String>>>,
) {
    use std::io::BufRead;
    let Some(stream) = stream else {
        return;
    };
    let lines = lines.clone();
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(stream).lines() {
            let Ok(line) = line else { break };
            let mut lines = lines.lock().unwrap_or_else(|e| e.into_inner());
            if lines.len() >= 40 {
                lines.pop_front();
            }
            lines.push_back(line);
        }
    });
}

/// `program` as a path to run: as given when it names a file or has a
/// directory; otherwise found on `PATH` (on Windows trying `PATHEXT`, so
/// `pnpm` finds `pnpm.cmd`).
pub(crate) fn resolve_program(program: &OsStr) -> Option<PathBuf> {
    let path = Path::new(program);
    if path.components().count() > 1 || path.is_absolute() {
        return Some(path.to_path_buf());
    }
    let extensions: Vec<OsString> = if cfg!(windows) {
        let pathext = std::env::var_os("PATHEXT")
            .map(|v| v.to_string_lossy().into_owned())
            .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".to_owned());
        std::iter::once(OsString::new())
            .chain(
                pathext
                    .split(';')
                    .filter(|e| !e.is_empty())
                    .map(|e| OsString::from(e.to_ascii_lowercase())),
            )
            .collect()
    } else {
        vec![OsString::new()]
    };
    let dirs = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&dirs) {
        for ext in &extensions {
            let mut name = program.to_owned();
            name.push(ext);
            let candidate = dir.join(&name);
            if candidate.is_file()
                && (!cfg!(windows) || !ext.is_empty() || has_extension(&candidate))
            {
                return Some(candidate);
            }
        }
    }
    None
}

/// On Windows a file without an extension (a shell script) cannot be run.
fn has_extension(path: &Path) -> bool {
    path.extension().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_front_end_needs_a_command_and_an_http_url() {
        let error = FrontEnd::new("web")
            .url("http://127.0.0.1:3100")
            .check()
            .unwrap_err();
        assert!(error.to_string().contains(".command"), "{error}");
        let error = FrontEnd::new("web")
            .command("node", ["x"])
            .check()
            .unwrap_err();
        assert!(error.to_string().contains(".url"), "{error}");
        let error = FrontEnd::new("web")
            .command("node", ["x"])
            .url("https://example.test")
            .check()
            .unwrap_err();
        assert!(error.to_string().contains("http://"), "{error}");
        let base = FrontEnd::new("web")
            .command("node", ["x"])
            .url("http://127.0.0.1:3100/")
            .check()
            .unwrap();
        assert_eq!(base, "http://127.0.0.1:3100");
    }

    #[test]
    fn programs_are_found_on_the_path() {
        let found = resolve_program(OsStr::new(if cfg!(windows) { "cmd" } else { "sh" }));
        assert!(found.is_some_and(|p| p.is_file()));
        assert_eq!(
            resolve_program(OsStr::new("navree-no-such-program-here")),
            None
        );
        let explicit = Path::new("tools").join("run");
        assert_eq!(resolve_program(explicit.as_os_str()), Some(explicit));
    }

    /// A listener on a port below the range every system hands out when a
    /// program asks for any port (and uses for the source port of a
    /// connection). A port given back to that range can be handed out again
    /// at once, to another test of this binary or to any program on the
    /// machine, so a test that frees one and then expects it free is racing
    /// them; a port below the range is given only to a program that names it.
    fn listener_below_the_ephemeral_ports() -> (std::net::TcpListener, u16) {
        // Windows, macOS and BSD start their range at 49152, Linux at 32768.
        const LOW: u16 = 20_000;
        const HIGH: u16 = 29_999;
        let span = u32::from(HIGH - LOW) + 1;
        let first = std::process::id() % span;
        for step in 0..span {
            let port = LOW + ((first + step) % span) as u16;
            // Every address `port_taken` tries, so the port is free on all of them.
            let all = [
                SocketAddr::from((Ipv6Addr::UNSPECIFIED, port)),
                SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)),
            ];
            let free = all
                .iter()
                .all(|addr| match std::net::TcpListener::bind(addr) {
                    Ok(_) => true,
                    // No IPv6 on this machine.
                    Err(error) => !in_use(&error) && addr.is_ipv6(),
                });
            if !free {
                continue;
            }
            if let Ok(listener) = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port)) {
                return (listener, port);
            }
        }
        panic!("no port from {LOW} to {HIGH} is free");
    }

    /// Set in the process [`run_alone`] starts.
    const ALONE: &str = "NAVREE_BROWSER_TEST_ALONE";

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

    /// A listener that never answers, like a dev server compiling its first
    /// page, holds the port; once it is closed the port is free.
    ///
    /// Run alone, in a process of its own. On Unix, starting a process copies
    /// every socket of the process that starts it into the child, which holds
    /// them until it runs its program, and tests beside this one start processes.
    /// One started in the instant this test closes its listener would keep
    /// the port in use for the check that follows.
    #[tokio::test]
    async fn a_port_something_listens_on_is_taken_before_it_answers() {
        if std::env::var_os(ALONE).is_none() {
            return run_alone("a_port_something_listens_on_is_taken_before_it_answers");
        }
        let (listener, port) = listener_below_the_ephemeral_ports();
        let taken = port_taken(&format!("http://127.0.0.1:{port}/login"))
            .await
            .expect("the port is held");
        assert_eq!(taken, format!("127.0.0.1:{port} is already in use"));
        assert!(
            port_taken(&format!("http://localhost:{port}"))
                .await
                .is_some()
        );
        drop(listener);
        assert_eq!(port_taken(&format!("http://127.0.0.1:{port}")).await, None);
    }

    #[tokio::test]
    async fn a_server_on_every_address_holds_the_port_of_each() {
        let listener = std::net::TcpListener::bind("0.0.0.0:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(
            port_taken(&format!("http://127.0.0.1:{port}"))
                .await
                .is_some()
        );
        assert!(
            port_taken(&format!("http://localhost:{port}"))
                .await
                .is_some()
        );
    }

    #[tokio::test]
    async fn a_url_that_is_not_on_this_machine_is_not_checked() {
        // TEST-NET-1: never an address of this machine.
        assert_eq!(port_taken("http://192.0.2.1:3100").await, None);
        assert_eq!(port_taken("not a url").await, None);
    }

    /// One that could not listen exits; whatever answers then is another
    /// server, and the front end is not taken to be up.
    #[tokio::test]
    async fn an_answer_after_the_front_end_exited_is_refused() {
        let server = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", server.local_addr().unwrap());
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            while let Ok((mut stream, _)) = server.accept().await {
                tokio::spawn(async move {
                    let mut request = [0; 1024];
                    let _ = stream.read(&mut request).await;
                    let _ = stream
                        .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n")
                        .await;
                });
            }
        });
        let (program, args): (&str, Vec<&str>) = if cfg!(windows) {
            ("cmd", vec!["/C", "exit", "3"])
        } else {
            ("sh", vec!["-c", "exit 3"])
        };
        let mut child = Command::new(resolve_program(OsStr::new(program)).unwrap())
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        child.wait().unwrap();
        let mut running = Running {
            child,
            output: Arc::default(),
        };
        let front = FrontEnd::new("web")
            .command(program, args)
            .url(url.clone())
            .timeout(Duration::from_secs(5));
        let error = wait_for(&mut running, &front, &url).await.unwrap_err();
        let message = error.to_string();
        assert!(message.contains("had exited"), "{message}");
        assert!(
            message.contains("another server answers there"),
            "{message}"
        );
    }

    /// Stopping a front end ends every process of its group, a server it
    /// started included, and nothing of the caller's own group.
    #[cfg(unix)]
    #[test]
    fn stopping_ends_the_front_end_s_group_and_leaves_the_caller_s_group_alone() {
        use std::io::{BufRead, Read};
        use std::os::unix::process::CommandExt;

        // In the test binary's group, like cargo and the CI runner.
        let mut bystander = Command::new("sleep")
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        // A front end that starts a server, as pnpm starts `next start`. Test code with a fixed
        // script (the rule exempts tests; this one lives beside the code it tests).
        // nosemgrep: navree-shell-command-string
        let mut child = Command::new("sh")
            .args(["-c", "sleep 30 & echo started; wait"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .unwrap();
        let mut stdout = std::io::BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        assert_eq!(line.trim(), "started");
        // The pipe closes once every process holding it has ended.
        let (closed, until_closed) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = stdout.read_to_end(&mut Vec::new());
            let _ = closed.send(());
        });

        let mut running = Running {
            child,
            output: Arc::default(),
        };
        running.stop();
        assert!(matches!(running.child.try_wait(), Ok(Some(_))));
        until_closed
            .recv_timeout(Duration::from_secs(5))
            .expect("the server the front end started has ended too");
        std::thread::sleep(Duration::from_millis(200));
        assert!(
            matches!(bystander.try_wait(), Ok(None)),
            "a process of the caller's group was signalled"
        );
        let _ = bystander.kill();
        let _ = bystander.wait();
    }

    #[test]
    fn defaults_and_settings_are_kept() {
        let front = FrontEnd::new("crm-web")
            .command("pnpm", ["start"])
            .url("http://127.0.0.1:3100")
            .api_env("NAVREE_API_URL")
            .env("NAVREE_COOKIE_SECURE", "0")
            .timeout(Duration::from_secs(5));
        assert_eq!(front.timeout, Duration::from_secs(5));
        assert_eq!(front.args, [OsString::from("start")]);
        assert_eq!(front.api_env.as_deref(), Some("NAVREE_API_URL"));
        assert_eq!(FrontEnd::new("x").timeout, Duration::from_secs(60));
    }
}
