//! [`Telemetry`]: the registry of an app's sinks, held by every emitter.

use std::cell::Cell;
use std::fmt;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};

use chrono::Utc;
use navree_kernel::AppContext;

use crate::{
    EventContext, EventKind, ExceptionReported, Interest, TelemetryContext, TelemetryEvent,
    TelemetryRecord, TelemetrySink,
};

/// The app's telemetry registry: the sinks registered in the container and
/// the union of their [`Interest`]s. A cheap, cloneable handle.
///
/// Emitters (navree-http, navree-db, navree-queue, ...) resolve it once,
/// when they are built, with [`Telemetry::from_app`]. An app without a
/// sink has no registry, so the handle is [`disabled`](Telemetry::disabled)
/// and every check is a single branch:
///
/// ```
/// use std::time::Duration;
/// use navree_telemetry::{EventKind, QueryExecuted, Telemetry, TelemetryEvent};
///
/// fn report(telemetry: &Telemetry, sql: &str, took: Duration) {
///     // Builds nothing unless a sink wants queries.
///     telemetry.emit_with(EventKind::QueryExecuted, || {
///         TelemetryEvent::QueryExecuted(QueryExecuted {
///             sql: sql.to_owned(),
///             duration: took,
///             rows: None,
///             connection: "sqlite".to_owned(),
///             failed: false,
///         })
///     });
/// }
/// # report(&Telemetry::disabled(), "SELECT 1", Duration::ZERO);
/// ```
#[derive(Clone, Default)]
pub struct Telemetry {
    registry: Option<Arc<Registry>>,
}

/// The sinks of one app.
struct Registry {
    /// Set once: at construction ([`Telemetry::with_sinks`]) or when the
    /// app boots ([`TelemetryModule`](crate::TelemetryModule)).
    sinks: OnceLock<Box<[Entry]>>,
    /// The union of the sinks' interests while active; 0 before the app
    /// booted and after it shut down.
    interest: AtomicU32,
}

/// A sink and the interest it declared.
struct Entry {
    sink: Arc<dyn TelemetrySink>,
    interest: Interest,
}

thread_local! {
    /// Set while sinks receive a record on this thread: a sink that logs
    /// (or otherwise emits) from `record` must not loop back into itself.
    static DELIVERING: Cell<bool> = const { Cell::new(false) };
}

impl Telemetry {
    /// A handle that delivers nothing (an app without sinks).
    pub fn disabled() -> Self {
        Self::default()
    }

    /// A registry with these sinks, active at once (for code outside an
    /// app, and for tests of emitters).
    pub fn with_sinks(sinks: Vec<Arc<dyn TelemetrySink>>) -> Self {
        let telemetry = Self::pending();
        telemetry.activate(sinks);
        telemetry
    }

    /// The app's registry: the `Telemetry` provider of
    /// [`TelemetryModule`](crate::TelemetryModule), present once a module
    /// registers a sink; [`disabled`](Telemetry::disabled) otherwise.
    /// Resolve it in a provider's constructor and keep it.
    pub async fn from_app(ctx: &AppContext) -> Self {
        match ctx.try_get::<Telemetry>().await {
            Some(telemetry) => Telemetry::clone(&telemetry),
            None => Self::disabled(),
        }
    }

    /// The registry of the current [`TelemetryContext`] (the request or
    /// job this task works for); disabled outside one.
    pub fn current() -> Self {
        TelemetryContext::with_current(|context| context.telemetry().clone()).unwrap_or_default()
    }

    /// A registry with no sinks yet (activated when the app boots).
    pub(crate) fn pending() -> Self {
        Self {
            registry: Some(Arc::new(Registry {
                sinks: OnceLock::new(),
                interest: AtomicU32::new(0),
            })),
        }
    }

    /// Installs the sinks (once) and turns delivery on.
    pub(crate) fn activate(&self, sinks: Vec<Arc<dyn TelemetrySink>>) {
        let Some(registry) = &self.registry else {
            return;
        };
        let entries: Box<[Entry]> = sinks
            .into_iter()
            .map(|sink| Entry {
                interest: sink.interest(),
                sink,
            })
            .collect();
        let _ = registry.sinks.set(entries);
        let union = registry
            .sinks
            .get()
            .into_iter()
            .flatten()
            .fold(Interest::NONE, |all, entry| all.union(entry.interest));
        registry.interest.store(union.bits(), Ordering::Release);
    }

    /// Turns delivery off (the app shut down).
    pub(crate) fn deactivate(&self) {
        if let Some(registry) = &self.registry {
            registry.interest.store(0, Ordering::Release);
        }
    }

    /// Whether this handle has no registry ([`disabled`](Telemetry::disabled),
    /// an app without sinks): it never delivers anything. A handle with a
    /// registry may still be inactive (before the app booted).
    pub fn is_disabled(&self) -> bool {
        self.registry.is_none()
    }

    /// Whether both handles share one registry (two disabled handles do not).
    pub fn same_registry(&self, other: &Telemetry) -> bool {
        match (&self.registry, &other.registry) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }

    /// The kinds some sink wants right now.
    pub fn interest(&self) -> Interest {
        match &self.registry {
            Some(registry) => Interest::from_bits(registry.interest.load(Ordering::Acquire)),
            None => Interest::NONE,
        }
    }

    /// Whether any sink wants anything.
    pub fn is_active(&self) -> bool {
        !self.interest().is_empty()
    }

    /// Whether some sink wants events of `kind`. Check it before building
    /// an expensive event, or use [`emit_with`](Telemetry::emit_with).
    #[inline]
    pub fn enabled(&self, kind: EventKind) -> bool {
        match &self.registry {
            Some(registry) => registry.interest.load(Ordering::Relaxed) & kind.bit() != 0,
            None => false,
        }
    }

    /// Delivers `event` to every sink that wants its kind, with the
    /// current [`TelemetryContext`] and the current time.
    pub fn emit(&self, event: TelemetryEvent) {
        if !self.enabled(event.kind()) {
            return;
        }
        let context =
            TelemetryContext::with_current(TelemetryContext::event_context).unwrap_or_default();
        self.deliver(TelemetryRecord {
            at: Utc::now(),
            context,
            event,
        });
    }

    /// Builds the event with `build` and emits it, only when some sink
    /// wants `kind` (which must be the built event's kind).
    pub fn emit_with(&self, kind: EventKind, build: impl FnOnce() -> TelemetryEvent) {
        if self.enabled(kind) {
            self.emit(build());
        }
    }

    /// Delivers a record built by the caller (its own time and context).
    pub fn record(&self, record: TelemetryRecord) {
        if self.enabled(record.event.kind()) {
            self.deliver(record);
        }
    }

    /// Emits an [`ExceptionReported`] for `error` and its sources, located
    /// at the caller (`file:line:column`).
    #[track_caller]
    pub fn report_exception(&self, error: &(dyn std::error::Error + 'static)) {
        if !self.enabled(EventKind::ExceptionReported) {
            return;
        }
        let caller = std::panic::Location::caller();
        let mut exception = ExceptionReported::from_error(error);
        exception.location = Some(format!(
            "{}:{}:{}",
            caller.file(),
            caller.line(),
            caller.column()
        ));
        self.emit(TelemetryEvent::ExceptionReported(exception));
    }

    /// Hands `record` to the sinks that want it, unless a sink on this
    /// thread is already receiving one.
    fn deliver(&self, record: TelemetryRecord) {
        let Some(sinks) = self.registry.as_ref().and_then(|r| r.sinks.get()) else {
            return;
        };
        if DELIVERING.with(|delivering| delivering.replace(true)) {
            return;
        }
        let _reset = ResetDelivering;
        let kind = record.event.kind();
        for entry in sinks.iter() {
            if entry.interest.contains(kind) {
                entry.sink.record(&record);
            }
        }
    }

    /// The context a record emitted now would carry.
    pub fn current_context() -> EventContext {
        TelemetryContext::with_current(TelemetryContext::event_context).unwrap_or_default()
    }
}

/// Clears [`DELIVERING`] when delivery ends, also when a sink panics.
struct ResetDelivering;

impl Drop for ResetDelivering {
    fn drop(&mut self) {
        DELIVERING.with(|delivering| delivering.set(false));
    }
}

impl fmt::Debug for Telemetry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sinks = self
            .registry
            .as_ref()
            .and_then(|r| r.sinks.get())
            .map_or(0, |sinks| sinks.len());
        f.debug_struct("Telemetry")
            .field("sinks", &sinks)
            .field("interest", &self.interest())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use super::*;
    use crate::{LogLevel, LogRecord};

    /// A sink that emits from `record`, as a sink that logs would.
    struct Echo {
        telemetry: OnceLock<Telemetry>,
        seen: AtomicUsize,
    }

    impl TelemetrySink for Echo {
        fn record(&self, _record: &TelemetryRecord) {
            self.seen.fetch_add(1, Ordering::SeqCst);
            if let Some(telemetry) = self.telemetry.get() {
                telemetry.emit(log("from the sink"));
            }
        }
    }

    fn log(message: &str) -> TelemetryEvent {
        TelemetryEvent::Log(LogRecord {
            level: LogLevel::Info,
            target: "test".into(),
            message: message.into(),
            fields: Default::default(),
        })
    }

    #[test]
    fn a_sink_that_emits_does_not_loop() {
        let echo = Arc::new(Echo {
            telemetry: OnceLock::new(),
            seen: AtomicUsize::new(0),
        });
        let telemetry = Telemetry::with_sinks(vec![echo.clone()]);
        let _ = echo.telemetry.set(telemetry.clone());
        telemetry.emit(log("first"));
        assert_eq!(echo.seen.load(Ordering::SeqCst), 1);
        telemetry.emit(log("second"));
        assert_eq!(echo.seen.load(Ordering::SeqCst), 2, "delivery resets");
    }

    #[test]
    fn handles_compare_by_registry() {
        let a = Telemetry::pending();
        assert!(!a.is_disabled());
        assert!(Telemetry::disabled().is_disabled());
        assert!(a.same_registry(&a.clone()));
        assert!(!a.same_registry(&Telemetry::pending()));
        assert!(!Telemetry::disabled().same_registry(&Telemetry::disabled()));
    }

    #[test]
    fn activation_happens_once() {
        let telemetry = Telemetry::pending();
        assert!(!telemetry.is_active());
        telemetry.activate(Vec::new());
        assert!(!telemetry.is_active());
        telemetry.deactivate();
        assert!(!telemetry.is_active());
        assert!(format!("{telemetry:?}").contains("sinks: 0"));
    }
}
