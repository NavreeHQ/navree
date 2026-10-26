//! [`TelemetrySink`] and [`Interest`].

use crate::{EventKind, TelemetryRecord};

/// A set of [`EventKind`]s: what a sink wants to receive.
///
/// ```
/// use navree_telemetry::{EventKind, Interest};
///
/// let jobs = Interest::only([EventKind::JobProcessed, EventKind::JobFailed]);
/// assert!(jobs.contains(EventKind::JobFailed));
/// assert!(!jobs.contains(EventKind::Log));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Interest(u32);

impl Interest {
    /// Every kind.
    pub const ALL: Interest = Interest(u32::MAX);
    /// No kind.
    pub const NONE: Interest = Interest(0);

    /// Exactly these kinds.
    pub fn only(kinds: impl IntoIterator<Item = EventKind>) -> Interest {
        kinds.into_iter().fold(Interest::NONE, Interest::with)
    }

    /// This set plus `kind`.
    #[must_use]
    pub fn with(self, kind: EventKind) -> Interest {
        Interest(self.0 | kind.bit())
    }

    /// This set without `kind`.
    #[must_use]
    pub fn without(self, kind: EventKind) -> Interest {
        Interest(self.0 & !kind.bit())
    }

    /// Both sets together.
    #[must_use]
    pub fn union(self, other: Interest) -> Interest {
        Interest(self.0 | other.0)
    }

    /// Whether `kind` is in the set.
    pub fn contains(self, kind: EventKind) -> bool {
        self.0 & kind.bit() != 0
    }

    /// Whether the set has no kind.
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub(crate) fn bits(self) -> u32 {
        self.0
    }

    pub(crate) fn from_bits(bits: u32) -> Interest {
        Interest(bits)
    }
}

/// Receives telemetry records. Register one with
/// [`ModuleBuilderTelemetryExt`](crate::ModuleBuilderTelemetryExt)
/// (`m.telemetry_sink::<MySink>()`); the app's emitters then deliver every
/// record whose kind is in the sink's [`interest`](TelemetrySink::interest).
///
/// `record` runs on the emitting task, in the middle of a request, a query
/// or a log call: keep it short and never block. A sink that stores or
/// sends records hands them to a channel and does the work in a
/// background task. A sink must not emit telemetry from `record`.
pub trait TelemetrySink: Send + Sync + 'static {
    /// The kinds this sink wants (all of them by default). Read once, when
    /// the app boots; emitters skip building events no sink wants.
    fn interest(&self) -> Interest {
        Interest::ALL
    }

    /// Receives one record.
    fn record(&self, record: &TelemetryRecord);
}
