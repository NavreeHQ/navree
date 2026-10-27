//! [`BrowserError`]: what failed, where, and the files saved on failure.

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// An error from navree-browser: a failed step (action, assertion or wait),
/// a browser that could not be found or started, or a server or front end
/// that could not be served.
///
/// `Debug` prints the same text as `Display`, so a test returning
/// `Result<(), BrowserError>` fails with the readable message, the URL and
/// the paths of the failure files.
#[derive(thiserror::Error)]
#[non_exhaustive]
pub enum BrowserError {
    /// No Chrome or Chromium binary was found (or `NAVREE_BROWSER_CHROME`
    /// names a file that does not exist). The message says where navree-browser
    /// looked.
    #[error("{0}")]
    ChromeNotFound(String),
    /// Chrome was found but did not start or did not open its DevTools
    /// endpoint.
    #[error("Chrome did not start ({}): {reason}", path.display())]
    Launch {
        /// The binary that was started.
        path: PathBuf,
        /// What went wrong, with the end of Chrome's output.
        reason: String,
    },
    /// An action, assertion or wait failed; see [`StepFailure`].
    #[error("{0}")]
    Step(Box<StepFailure>),
    /// An invalid setting: an environment variable or a builder value.
    #[error("{0}")]
    Config(String),
    /// The DevTools connection to Chrome failed or closed.
    #[error("the browser connection failed: {0}")]
    Connection(String),
    /// A [`TestServer`](crate::TestServer) or [`FrontEnd`](crate::FrontEnd)
    /// could not be served.
    #[error("{0}")]
    Server(String),
    /// A file or process operation failed.
    #[error("{context}: {source}")]
    Io {
        /// What navree-browser was doing.
        context: String,
        /// The underlying error.
        source: std::io::Error,
    },
}

impl BrowserError {
    /// True when no browser was found, so a test can be skipped on a
    /// machine without Chrome (see [`Browser::find_chrome`](crate::Browser::find_chrome)).
    pub fn is_chrome_not_found(&self) -> bool {
        matches!(self, BrowserError::ChromeNotFound(_))
    }

    /// The failed step, for [`BrowserError::Step`].
    pub fn step(&self) -> Option<&StepFailure> {
        match self {
            BrowserError::Step(failure) => Some(failure),
            _ => None,
        }
    }

    /// The files saved when the step failed, if any.
    pub fn files(&self) -> Option<&FailureFiles> {
        self.step().and_then(|failure| failure.files.as_ref())
    }

    pub(crate) fn io(context: impl Into<String>, source: std::io::Error) -> BrowserError {
        BrowserError::Io {
            context: context.into(),
            source,
        }
    }
}

impl fmt::Debug for BrowserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

/// A failed step: what was attempted, why it failed, the page's URL and
/// the failure files.
///
/// Displays as:
///
/// ```text
/// assert_see("Deal saved") failed after 5s: the text is not visible on http://127.0.0.1:3100/deals
///   screenshot: target/navree-browser/a_rep_moves_a_deal/03-failure.png
///   html:       target/navree-browser/a_rep_moves_a_deal/03-failure.html
///   console:    target/navree-browser/a_rep_moves_a_deal/03-console.log
/// ```
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct StepFailure {
    /// The step as called, e.g. `assert_see("Deal saved")`.
    pub step: String,
    /// Why it failed, e.g. `the text is not visible`.
    pub reason: String,
    /// The page's URL when it failed.
    pub url: Option<String>,
    /// How long the step waited before giving up (`None` for checks that
    /// do not wait, such as `assert_no_console_errors`).
    pub waited: Option<Duration>,
    /// The screenshot, HTML and console log saved for the failure.
    pub files: Option<FailureFiles>,
}

impl fmt::Display for StepFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} failed", self.step)?;
        if let Some(waited) = self.waited {
            write!(f, " after {}", format_duration(waited))?;
        }
        write!(f, ": {}", self.reason)?;
        if let Some(url) = &self.url {
            write!(f, " on {url}")?;
        }
        if let Some(files) = &self.files {
            for (label, path) in [
                ("screenshot: ", &files.screenshot),
                ("html:       ", &files.html),
                ("console:    ", &files.console),
            ] {
                if let Some(path) = path {
                    write!(f, "\n  {label}{}", shown(path))?;
                }
            }
        }
        Ok(())
    }
}

/// The files saved when a step fails, under
/// `<artifacts>/<test name>/`: a PNG screenshot, the page's HTML and its
/// console log (console messages, uncaught exceptions, dialogs and failed
/// network requests). A file that could not be captured is `None`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct FailureFiles {
    /// `NN-failure.png`.
    pub screenshot: Option<PathBuf>,
    /// `NN-failure.html`.
    pub html: Option<PathBuf>,
    /// `NN-console.log`.
    pub console: Option<PathBuf>,
}

/// `5s`, `1.5s` or `250ms`.
pub(crate) fn format_duration(d: Duration) -> String {
    let millis = d.as_millis();
    if millis < 1000 {
        return format!("{millis}ms");
    }
    let tenths = (millis + 50) / 100;
    if tenths.is_multiple_of(10) {
        format!("{}s", tenths / 10)
    } else {
        format!("{}.{}s", tenths / 10, tenths % 10)
    }
}

/// `path` relative to the current directory when it is inside it.
pub(crate) fn shown(path: &Path) -> String {
    std::env::current_dir()
        .ok()
        .and_then(|cwd| path.strip_prefix(cwd).ok().map(Path::to_path_buf))
        .unwrap_or_else(|| path.to_path_buf())
        .display()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_read_like_the_docs() {
        assert_eq!(format_duration(Duration::from_secs(5)), "5s");
        assert_eq!(format_duration(Duration::from_millis(5004)), "5s");
        assert_eq!(format_duration(Duration::from_millis(1500)), "1.5s");
        assert_eq!(format_duration(Duration::from_millis(250)), "250ms");
        assert_eq!(format_duration(Duration::from_secs(20)), "20s");
    }

    #[test]
    fn a_step_failure_names_the_step_the_reason_the_url_and_the_files() {
        let cwd = std::env::current_dir().unwrap();
        let dir = cwd.join("target").join("navree-browser").join("a_test");
        let failure = StepFailure {
            step: "assert_see(\"Deal saved\")".into(),
            reason: "the text is not visible".into(),
            url: Some("http://127.0.0.1:3100/deals".into()),
            waited: Some(Duration::from_secs(5)),
            files: Some(FailureFiles {
                screenshot: Some(dir.join("03-failure.png")),
                html: Some(dir.join("03-failure.html")),
                console: Some(dir.join("03-console.log")),
            }),
        };
        let sep = std::path::MAIN_SEPARATOR;
        let expected = format!(
            "assert_see(\"Deal saved\") failed after 5s: the text is not visible on http://127.0.0.1:3100/deals\n  \
             screenshot: target{sep}navree-browser{sep}a_test{sep}03-failure.png\n  \
             html:       target{sep}navree-browser{sep}a_test{sep}03-failure.html\n  \
             console:    target{sep}navree-browser{sep}a_test{sep}03-console.log"
        );
        assert_eq!(failure.to_string(), expected);
        let error = BrowserError::Step(Box::new(failure));
        assert_eq!(format!("{error:?}"), expected, "Debug reads like Display");
        assert!(error.files().is_some());
    }

    #[test]
    fn a_check_without_waiting_has_no_duration() {
        let failure = StepFailure {
            step: "assert_no_console_errors()".into(),
            reason: "1 console error".into(),
            url: None,
            waited: None,
            files: None,
        };
        assert_eq!(
            failure.to_string(),
            "assert_no_console_errors() failed: 1 console error"
        );
    }
}
