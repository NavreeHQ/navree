//! Child processes: git, and HTTP requests through `curl` (on every CI
//! runner and in Windows since Windows 10), so this tool needs no HTTP
//! client of its own.

use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};

/// The `User-Agent` of every request (crates.io asks for one that names a
/// contact).
pub const USER_AGENT: &str = "navree-release (https://navree.com)";

/// An HTTP answer.
#[derive(Debug, Clone)]
pub struct Response {
    /// The status code (0 when there was no answer).
    pub status: u16,
    /// The body.
    pub body: String,
}

/// What goes on `curl`'s standard input: secrets never go on its command
/// line, where other processes could read them.
pub enum Stdin<'a> {
    /// Nothing.
    None,
    /// Header lines (`Authorization: ...`), read with `--header @-`.
    Headers(&'a str),
    /// The request body, read with `--data-binary @-`.
    Body(&'a str),
}

/// Sends one request with `curl`.
pub fn curl(
    method: &str,
    url: &str,
    headers: &[&str],
    stdin: Stdin<'_>,
) -> Result<Response, String> {
    let mut command = Command::new("curl");
    command.args([
        "--silent",
        "--show-error",
        "--location",
        "--max-time",
        "60",
        "--user-agent",
        USER_AGENT,
        "--request",
        method,
        "--write-out",
        "\n%{http_code}",
    ]);
    for header in headers {
        command.args(["--header", header]);
    }
    let input = match stdin {
        Stdin::None => None,
        Stdin::Headers(lines) => {
            command.args(["--header", "@-"]);
            Some(lines)
        }
        Stdin::Body(body) => {
            command.args(["--data-binary", "@-"]);
            Some(body)
        }
    };
    command.arg(url);
    command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|e| format!("cannot run curl (it must be on PATH): {e}"))?;
    if let Some(input) = input {
        let mut pipe = child.stdin.take().ok_or("curl has no stdin")?;
        pipe.write_all(input.as_bytes())
            .map_err(|e| format!("cannot write to curl: {e}"))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|e| format!("curl failed: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let (body, status) = stdout.rsplit_once('\n').unwrap_or(("", stdout.as_str()));
    let status: u16 = status.trim().parse().unwrap_or(0);
    if status == 0 {
        return Err(format!(
            "{method} {url}: no answer ({})",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(Response {
        status,
        body: body.to_owned(),
    })
}

/// Runs git in `root` and returns its standard output.
pub fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| format!("cannot run git: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
