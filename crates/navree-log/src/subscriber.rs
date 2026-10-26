//! Building the subscriber: a registry, the reloadable level filter and one
//! formatting layer per output.

use std::sync::Arc;

use tracing::Dispatch;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::fmt::format::DefaultFields;
use tracing_subscriber::layer::{Layered, SubscriberExt};
use tracing_subscriber::{EnvFilter, Layer, Registry, reload};

use crate::file::{FileSink, FileWriter};
use crate::filter::Levels;
use crate::json::{JsonEvents, JsonFields};
use crate::redact::{RedactingFields, Redactor};
use crate::telemetry::TelemetryLayer;
use crate::{LogControl, LogError, LogFormat, LogOptions};

/// The registry under the reloadable level filter.
type Filtered = Layered<reload::Layer<EnvFilter, Registry>, Registry>;

/// A boxed formatting layer over the filtered registry.
type BoxedLayer = Box<dyn Layer<Filtered> + Send + Sync>;

/// Marks the log file's field formatter, so the file keeps its own (never
/// coloured) formatted span fields apart from stdout's.
struct FileOutput;

/// Builds, without installing, the subscriber [`LoggerModule`](crate::LoggerModule)
/// would install for `options`, with `writer` in place of stdout (no ANSI
/// colours). The log file, if configured, is opened and written as usual.
///
/// Run it as a thread's default with `tracing::dispatcher::with_default`
/// (or `set_default`) to capture logs in tests; [`CaptureWriter`](crate::CaptureWriter)
/// collects them.
///
/// ```
/// use navree_log::{CaptureWriter, LogFormat, LogOptions, build_subscriber};
///
/// let writer = CaptureWriter::new();
/// let options = LogOptions { format: LogFormat::Json, ..LogOptions::default() };
/// let (dispatch, _control) = build_subscriber(&options, writer.clone())?;
/// tracing::dispatcher::with_default(&dispatch, || {
///     tracing::info!(password = "hunter2", "signed in");
/// });
/// assert!(writer.contents().contains(r#""password":"[redacted]""#));
/// # Ok::<(), navree_log::LogError>(())
/// ```
pub fn build_subscriber<W>(
    options: &LogOptions,
    writer: W,
) -> Result<(Dispatch, LogControl), LogError>
where
    W: for<'w> MakeWriter<'w> + Send + Sync + 'static,
{
    let levels = options.check()?;
    build(options, levels, writer, false)
}

/// Builds the subscriber: `levels` (already validated), `writer` as the main
/// output, plus the log file of `options`.
pub(crate) fn build<W>(
    options: &LogOptions,
    levels: Levels,
    writer: W,
    ansi: bool,
) -> Result<(Dispatch, LogControl), LogError>
where
    W: for<'w> MakeWriter<'w> + Send + Sync + 'static,
{
    let (filter, handle) = reload::Layer::new(levels.filter()?);
    let redactor = Redactor::new(&options.redact);
    let mut layers = vec![fmt_layer::<_, ()>(options.format, writer, &redactor, ansi)];
    let file = match &options.file {
        Some(file) => {
            let sink = Arc::new(FileSink::open(file)?);
            // Files never get colour codes.
            layers.push(fmt_layer::<_, FileOutput>(
                options.format,
                FileWriter(sink.clone()),
                &redactor,
                false,
            ));
            Some(sink)
        }
        None => None,
    };
    // Log lines also go to the telemetry sinks that want them.
    layers.push(TelemetryLayer::new(redactor.clone()).boxed());
    let subscriber = Registry::default().with(filter).with(layers);
    Ok((
        Dispatch::new(subscriber),
        LogControl::new(handle, levels, file),
    ))
}

/// The formatting layer for one output; `Output` keys its cache of
/// formatted span fields (see [`RedactingFields`]).
fn fmt_layer<W, Output: 'static>(
    format: LogFormat,
    writer: W,
    redactor: &Redactor,
    ansi: bool,
) -> BoxedLayer
where
    W: for<'w> MakeWriter<'w> + Send + Sync + 'static,
{
    let layer = tracing_subscriber::fmt::layer().with_writer(writer);
    match format {
        LogFormat::Pretty => layer
            .with_ansi(ansi)
            .fmt_fields(RedactingFields::<_, Output>::new(
                DefaultFields::new(),
                redactor.clone(),
            ))
            .boxed(),
        LogFormat::Json => layer
            .with_ansi(false)
            .fmt_fields(JsonFields::new(redactor.clone()))
            .event_format(JsonEvents::new(redactor.clone()))
            .boxed(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CaptureWriter, FileLog, Rotation};

    /// Coloured stdout and a plain file must not share formatted span
    /// fields, or colour codes end up in the file.
    #[test]
    fn coloured_stdout_does_not_leak_escapes_into_the_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("app.log");
        let options = LogOptions {
            format: LogFormat::Pretty,
            file: Some(FileLog {
                path: path.clone(),
                rotation: Rotation::Never,
                keep_days: 1,
            }),
            ..LogOptions::default()
        };
        let stdout = CaptureWriter::new();
        let levels = options.check().expect("valid options");
        let (dispatch, control) = build(&options, levels, stdout.clone(), true).expect("build");
        tracing::dispatcher::with_default(&dispatch, || {
            let span = tracing::info_span!("request", request_id = "r-9");
            let _entered = span.enter();
            tracing::info!(status = 200, "handled");
        });
        control.flush().expect("flush");
        assert!(stdout.contents().contains('\u{1b}'), "stdout is coloured");
        let written = std::fs::read_to_string(&path).expect("log file");
        assert!(written.contains("request_id=\"r-9\""), "{written}");
        assert!(!written.contains('\u{1b}'), "{written:?}");
    }
}
