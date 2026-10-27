//! Installing the process-wide subscriber, once.

use std::io::IsTerminal;
use std::sync::{Mutex, MutexGuard, PoisonError};

use tracing::Dispatch;
use tracing_subscriber::fmt::TestWriter;

use crate::filter::Levels;
use crate::subscriber::build;
use crate::{LogControl, LogError, LogFormat, LogOptions};

/// What is installed in this process. A `Mutex<Option<_>>` rather than a
/// `OnceLock` because building can fail, and a failed boot must not prevent
/// a later one from installing.
static INSTALLED: Mutex<Option<Installed>> = Mutex::new(None);

struct Installed {
    control: LogControl,
    origin: Origin,
}

enum Origin {
    /// Installed by a `LoggerModule` boot with these options.
    Module(LogOptions),
    /// Installed by [`init_for_tests`].
    Tests,
    /// Another global subscriber was installed first.
    Foreign,
}

fn installed() -> MutexGuard<'static, Option<Installed>> {
    INSTALLED.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Where console lines go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Console {
    /// Standard output (the default).
    Stdout,
    /// Standard error: the app runs a command that speaks a protocol on
    /// stdout (`CommandSpec::stdio`, such as `mcp:serve`).
    Stderr,
}

impl Console {
    /// Stderr when the running command owns stdout, else stdout.
    pub(crate) fn for_command(command: Option<&navree_kernel::CommandSpec>) -> Self {
        if command.is_some_and(|command| command.stdio) {
            Console::Stderr
        } else {
            Console::Stdout
        }
    }
}

/// Installs the subscriber for `options`, writing console lines to
/// `console`, on first use; later calls reuse it and apply their levels
/// (unless it was installed by [`init_for_tests`] or is foreign). Blocks
/// briefly (opens the log file).
pub(crate) fn install(options: &LogOptions, console: Console) -> Result<LogControl, LogError> {
    let levels = options.check()?;
    let mut slot = installed();
    if let Some(installed) = slot.as_ref() {
        if let Origin::Module(first) = &installed.origin {
            installed.control.replace_levels(levels)?;
            if (&first.format, &first.file, &first.redact)
                != (&options.format, &options.file, &options.redact)
            {
                tracing::warn!(
                    "navree-log is already installed in this process: this boot's log levels apply, its format, file and redact settings do not"
                );
            }
        }
        return Ok(installed.control.clone());
    }
    install_into(
        &mut slot,
        Origin::Module(options.clone()),
        || match console {
            Console::Stdout => build(options, levels, std::io::stdout, stdout_supports_color()),
            Console::Stderr => build(options, levels, std::io::stderr, stderr_supports_color()),
        },
    )
}

/// Installs a subscriber for tests, once per process: human-readable lines
/// written through the test harness's output capture (shown only for
/// failing tests), no ANSI colours, the default redaction, and the level
/// filter from `RUST_LOG` (`EnvFilter` syntax), `info` when unset or invalid.
///
/// Call it at the start of any test; repeated calls do nothing. A
/// `LoggerModule` booted later in the process reuses this subscriber and
/// keeps the `RUST_LOG` levels. If another global subscriber is already
/// installed, it is left in place.
pub fn init_for_tests() {
    let mut slot = installed();
    if slot.is_some() {
        return;
    }
    let levels = std::env::var("RUST_LOG")
        .ok()
        .filter(|directives| !directives.trim().is_empty())
        .map(Levels::raw)
        .filter(|levels| levels.filter().is_ok())
        .unwrap_or_else(|| Levels::raw("info"));
    let options = LogOptions {
        format: LogFormat::Pretty,
        ..LogOptions::default()
    };
    // Building without a file cannot fail once the levels compile.
    let _ = install_into(&mut slot, Origin::Tests, || {
        build(&options, levels, TestWriter::new(), false)
    });
}

/// Builds and installs a subscriber as the global default, or records a
/// foreign one if another global subscriber is already set.
fn install_into(
    slot: &mut Option<Installed>,
    origin: Origin,
    build: impl FnOnce() -> Result<(Dispatch, LogControl), LogError>,
) -> Result<LogControl, LogError> {
    let installed = if tracing::dispatcher::has_been_set() {
        foreign()
    } else {
        let (dispatch, control) = build()?;
        if tracing::dispatcher::set_global_default(dispatch).is_ok() {
            // Route `log` records into tracing too; a `log` logger installed
            // by someone else stays in place.
            let _ = tracing_log::LogTracer::init();
            Installed { control, origin }
        } else {
            foreign()
        }
    };
    let control = installed.control.clone();
    *slot = Some(installed);
    Ok(control)
}

fn foreign() -> Installed {
    tracing::warn!(
        "a global tracing subscriber was installed before navree-log; it stays in place and LoggerModule settings do not apply"
    );
    Installed {
        control: LogControl::detached(),
        origin: Origin::Foreign,
    }
}

/// Colours only on a terminal, and never when `NO_COLOR` is set.
fn stdout_supports_color() -> bool {
    std::io::stdout().is_terminal()
        && std::env::var_os("NO_COLOR").is_none_or(|value| value.is_empty())
}

/// Colours only on a terminal, and never when `NO_COLOR` is set.
fn stderr_supports_color() -> bool {
    std::io::stderr().is_terminal()
        && std::env::var_os("NO_COLOR").is_none_or(|value| value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_for_tests_is_idempotent_and_wins_over_later_boots() {
        init_for_tests();
        init_for_tests();
        assert!(tracing::dispatcher::has_been_set());
        tracing::info!("logged through the test subscriber");

        let before = installed()
            .as_ref()
            .map(|installed| installed.control.directives())
            .expect("installed");
        let control = install(
            &LogOptions {
                level: "error".to_owned(),
                ..LogOptions::default()
            },
            Console::Stdout,
        )
        .expect("a boot reuses the test subscriber");
        assert_eq!(control.directives(), before, "test levels are kept");
    }

    #[test]
    fn commands_that_own_stdout_log_to_stderr() {
        let plain = navree_kernel::CommandSpec::new("serve", "", |_, _| async { Ok(()) });
        let protocol = plain.clone().stdio();
        assert_eq!(Console::for_command(None), Console::Stdout);
        assert_eq!(Console::for_command(Some(&plain)), Console::Stdout);
        assert_eq!(Console::for_command(Some(&protocol)), Console::Stderr);
    }
}
