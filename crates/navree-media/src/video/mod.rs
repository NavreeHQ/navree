//! Video and audio through an FFmpeg sidecar (the `ffmpeg` feature):
//! `ffprobe` for metadata, `ffmpeg` for poster frames and transcodes.
//!
//! The tools run as child processes ([`ProcessRunner`]) with a timeout;
//! they must be installed on the server (`ffmpeg` and `ffprobe` on the
//! PATH, or configured in `media.ffmpeg`). Inputs are opened with
//! `-protocol_whitelist file`, so a crafted file (a playlist inside a
//! container) cannot make FFmpeg fetch URLs. Tests use [`FfmpegFake`].
//!
//! ```no_run
//! use std::path::Path;
//! use navree_media::video::{Ffmpeg, FfmpegOptions, Transcode};
//!
//! # async fn run() -> Result<(), navree_media::MediaError> {
//! let ffmpeg = Ffmpeg::new(FfmpegOptions::default());
//! let probe = ffmpeg.probe(Path::new("intro.mov")).await?;
//! println!("{:?} seconds, {:?}", probe.duration, probe.dimensions());
//! ffmpeg.frame(Path::new("intro.mov"), 1.0, Path::new("poster.png")).await?;
//! ffmpeg
//!     .transcode(Path::new("intro.mov"), Path::new("intro.mp4"), &Transcode::mp4().height(720))
//!     .await?;
//! # Ok(())
//! # }
//! ```

mod fake;
mod probe;
mod transcode;

use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};

pub use fake::{FfmpegCall, FfmpegFake};
pub use probe::{Probe, ProbeStream};
pub use transcode::Transcode;

use crate::MediaError;

/// Where the tools are and how long they may run (configuration section
/// `media.ffmpeg`).
///
/// ```toml
/// [media.ffmpeg]
/// ffmpeg = "/usr/bin/ffmpeg"
/// ffprobe = "/usr/bin/ffprobe"
/// timeout_secs = 600
/// threads = 2
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FfmpegOptions {
    /// The `ffmpeg` executable: a name on the PATH or a path (default
    /// `ffmpeg`).
    pub ffmpeg: String,
    /// The `ffprobe` executable (default `ffprobe`).
    pub ffprobe: String,
    /// The longest a single run may take before it is killed (default
    /// 600 s).
    pub timeout_secs: u64,
    /// Encoder threads per transcode; `0` lets FFmpeg choose (default 0).
    pub threads: u32,
}

impl Default for FfmpegOptions {
    fn default() -> Self {
        Self {
            ffmpeg: "ffmpeg".to_owned(),
            ffprobe: "ffprobe".to_owned(),
            timeout_secs: 600,
            threads: 0,
        }
    }
}

/// What a tool printed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FfmpegOutput {
    /// Standard output (ffprobe's JSON).
    pub stdout: Vec<u8>,
    /// Standard error (FFmpeg's messages).
    pub stderr: Vec<u8>,
}

/// Runs `ffmpeg` and `ffprobe`: [`ProcessRunner`] for the real tools,
/// [`FfmpegFake`] in tests.
pub trait FfmpegRunner: Send + Sync + 'static {
    /// Runs `program` with `args` and waits for it, at most `timeout`.
    /// A non-zero exit, a missing program and a timeout are
    /// [`MediaError::Ffmpeg`].
    fn run<'a>(
        &'a self,
        program: &'a str,
        args: &'a [String],
        timeout: Duration,
    ) -> BoxFuture<'a, Result<FfmpegOutput, MediaError>>;
}

/// The only variables a tool inherits: where to find programs (`PATH`),
/// where it puts temporary files (`TMPDIR`; `TEMP` and `TMP` on Windows,
/// where a tool without them uses the Windows directory) and, on Windows,
/// the system directory its libraries load from (`SystemRoot`). Nothing
/// else of the server's environment (its keys, database URLs, tokens)
/// reaches a process that reads files an upload chose. Neither tool needs
/// a home directory, a locale or a library path to probe, take a frame or
/// transcode.
#[cfg(not(windows))]
const INHERITED_ENV: &[&str] = &["PATH", "TMPDIR"];
#[cfg(windows)]
const INHERITED_ENV: &[&str] = &["PATH", "SystemRoot", "TEMP", "TMP"];

/// The variables of `vars` that a tool inherits ([`INHERITED_ENV`]; Windows
/// does not tell `Path` from `PATH`).
fn inherited_env(
    vars: impl IntoIterator<Item = (OsString, OsString)>,
) -> Vec<(OsString, OsString)> {
    vars.into_iter()
        .filter(|(name, _)| {
            INHERITED_ENV.iter().any(|allowed| {
                if cfg!(windows) {
                    name.eq_ignore_ascii_case(allowed)
                } else {
                    name == OsStr::new(allowed)
                }
            })
        })
        .collect()
}

/// Runs the tools as child processes: no shell, no standard input, an
/// environment cleared down to [`INHERITED_ENV`] (`PATH`, the temporary
/// directory, and `SystemRoot` on Windows), killed when the timeout passes
/// (or the future is dropped).
#[derive(Clone, Copy, Debug, Default)]
pub struct ProcessRunner;

impl FfmpegRunner for ProcessRunner {
    fn run<'a>(
        &'a self,
        program: &'a str,
        args: &'a [String],
        timeout: Duration,
    ) -> BoxFuture<'a, Result<FfmpegOutput, MediaError>> {
        Box::pin(async move {
            let child = tokio::process::Command::new(program)
                .args(args)
                .env_clear()
                .envs(inherited_env(std::env::vars_os()))
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true)
                .spawn()
                .map_err(|error| MediaError::Ffmpeg {
                    message: if error.kind() == std::io::ErrorKind::NotFound {
                        format!("{program} was not found; install FFmpeg or set media.ffmpeg")
                    } else {
                        format!("cannot start {program}: {error}")
                    },
                })?;
            let output = match tokio::time::timeout(timeout, child.wait_with_output()).await {
                Ok(output) => output.map_err(|error| MediaError::Ffmpeg {
                    message: format!("{program}: {error}"),
                })?,
                // Dropping the future dropped the child, which kills it.
                Err(_) => {
                    return Err(MediaError::Ffmpeg {
                        message: format!(
                            "{program} did not finish within {} s",
                            timeout.as_secs_f64()
                        ),
                    });
                }
            };
            if !output.status.success() {
                return Err(MediaError::Ffmpeg {
                    message: format!(
                        "{program} failed ({}): {}",
                        output.status,
                        last_lines(&output.stderr)
                    ),
                });
            }
            Ok(FfmpegOutput {
                stdout: output.stdout,
                stderr: output.stderr,
            })
        })
    }
}

/// The last lines of a tool's error output, at most 600 characters.
pub(crate) fn last_lines(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let text = text.trim();
    let start = text
        .char_indices()
        .rev()
        .nth(599)
        .map_or(0, |(index, _)| index);
    text[start..].trim().to_owned()
}

/// FFmpeg and ffprobe, run through a [`FfmpegRunner`].
#[derive(Clone)]
pub struct Ffmpeg {
    runner: Arc<dyn FfmpegRunner>,
    options: FfmpegOptions,
}

impl std::fmt::Debug for Ffmpeg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ffmpeg")
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

impl Ffmpeg {
    /// The tools as child processes ([`ProcessRunner`]).
    pub fn new(options: FfmpegOptions) -> Self {
        Self::with_runner(options, Arc::new(ProcessRunner))
    }

    /// The tools through `runner` (an [`FfmpegFake`] in tests).
    pub fn with_runner(options: FfmpegOptions, runner: Arc<dyn FfmpegRunner>) -> Self {
        Self { runner, options }
    }

    /// The options.
    pub fn options(&self) -> &FfmpegOptions {
        &self.options
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(self.options.timeout_secs.max(1))
    }

    /// The first line of `ffmpeg -version` (fails when FFmpeg is missing).
    pub async fn version(&self) -> Result<String, MediaError> {
        let args = vec!["-hide_banner".to_owned(), "-version".to_owned()];
        let output = self
            .runner
            .run(&self.options.ffmpeg, &args, Duration::from_secs(30))
            .await?;
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .unwrap_or_default()
            .to_owned())
    }

    /// The container and streams of a local file.
    pub async fn probe(&self, input: &Path) -> Result<Probe, MediaError> {
        let args: Vec<String> = [
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
            "-protocol_whitelist",
            "file",
        ]
        .iter()
        .map(|arg| (*arg).to_owned())
        .chain([input.display().to_string()])
        .collect();
        let output = self
            .runner
            .run(&self.options.ffprobe, &args, self.timeout())
            .await?;
        Probe::from_json(&output.stdout)
    }

    /// Writes the frame at `at_secs` as an image (`.png` or `.jpg`, after
    /// `output`'s extension), turned upright.
    pub async fn frame(&self, input: &Path, at_secs: f64, output: &Path) -> Result<(), MediaError> {
        let args: Vec<String> = vec![
            "-y".into(),
            "-hide_banner".into(),
            "-loglevel".into(),
            "error".into(),
            "-ss".into(),
            format_secs(at_secs),
            "-protocol_whitelist".into(),
            "file".into(),
            "-i".into(),
            input.display().to_string(),
            "-frames:v".into(),
            "1".into(),
            "-an".into(),
            output.display().to_string(),
        ];
        self.runner
            .run(&self.options.ffmpeg, &args, self.timeout())
            .await?;
        Ok(())
    }

    /// Re-encodes `input` into `output` as `transcode` says.
    pub async fn transcode(
        &self,
        input: &Path,
        output: &Path,
        transcode: &Transcode,
    ) -> Result<(), MediaError> {
        let args = transcode.args(input, output, self.options.threads);
        self.runner
            .run(&self.options.ffmpeg, &args, self.timeout())
            .await?;
        Ok(())
    }
}

/// Seconds as FFmpeg reads them (`1.5`, `0`).
fn format_secs(secs: f64) -> String {
    let secs = secs.max(0.0);
    let text = format!("{secs:.3}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seconds_are_short() {
        assert_eq!(format_secs(1.5), "1.5");
        assert_eq!(format_secs(0.0), "0");
        assert_eq!(format_secs(12.0), "12");
        assert_eq!(format_secs(-3.0), "0");
    }

    fn env(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        pairs
            .iter()
            .map(|(name, value)| (OsString::from(name), OsString::from(value)))
            .collect()
    }

    #[test]
    fn a_tool_inherits_the_path_and_the_temporary_directory_and_no_secret() {
        let server = env(&[
            ("APP_KEY", "base64:not-for-a-tool"),
            ("DATABASE_URL", "postgres://user:password@db/app"),
            ("AWS_SECRET_ACCESS_KEY", "secret"),
            ("HOME", "/home/server"),
            ("LD_LIBRARY_PATH", "/opt/lib"),
            ("TMPDIR_URL", "https://example.test"),
            ("PATHS", "no"),
            ("PATH", "/usr/bin"),
            ("TMPDIR", "/var/tmp/app"),
            ("TEMP", r"C:\Temp"),
            ("TMP", r"C:\Temp"),
            ("SystemRoot", r"C:\Windows"),
        ]);
        let mut names: Vec<String> = inherited_env(server)
            .into_iter()
            .map(|(name, _)| name.to_string_lossy().into_owned())
            .collect();
        names.sort();
        let expected: &[&str] = if cfg!(windows) {
            &["PATH", "SystemRoot", "TEMP", "TMP"]
        } else {
            &["PATH", "TMPDIR"]
        };
        assert_eq!(names, expected);
    }

    #[test]
    fn a_tool_gets_the_values_the_server_has() {
        let inherited = inherited_env(env(&[("PATH", "/usr/bin:/bin"), ("KEY", "secret")]));
        assert_eq!(inherited, env(&[("PATH", "/usr/bin:/bin")]));
    }

    #[cfg(windows)]
    #[test]
    fn windows_variable_names_are_matched_whatever_their_case() {
        let inherited = inherited_env(env(&[
            ("Path", r"C:\bin"),
            ("temp", r"C:\Temp"),
            ("Tmp", r"C:\Temp"),
        ]));
        assert_eq!(inherited.len(), 3);
    }

    #[test]
    fn last_lines_are_capped() {
        let long = "x".repeat(2000);
        assert_eq!(last_lines(long.as_bytes()).len(), 600);
        assert_eq!(last_lines(b"  short \n"), "short");
    }
}
