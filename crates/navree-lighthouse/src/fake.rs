//! [`LighthouseFake`]: records reports instead of storing them.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use navree_telemetry::{EventContext, ExceptionReported};
use serde_json::Value;

use crate::alert::Alert;
use crate::model::Level;
use crate::report::Report;
use crate::service::Lighthouse;

/// One report a [`LighthouseFake`] recorded: from your code
/// ([`Lighthouse::report`], [`Lighthouse::capture`]) or captured by the
/// framework (a handler answering 500, a panic).
#[derive(Clone, Debug, PartialEq)]
pub struct Reported {
    /// The message (the error's, or the text).
    pub message: String,
    /// The error's message and every source's.
    pub chain: Vec<String>,
    /// The reported error's type name, when it was known statically
    /// (`report(&error)` with a concrete error type).
    pub error_type: Option<String>,
    /// The error's `Debug` form (`NotFound("inv_missing")`), for
    /// [`report`](Lighthouse::report) calls.
    pub debug: Option<String>,
    /// The level.
    pub level: Level,
    /// The fingerprint parts set with `.fingerprint([..])`.
    pub fingerprint: Option<Vec<String>>,
    /// The tags: the report's and the current request's
    /// ([`tag_current`](Lighthouse::tag_current)).
    pub tags: BTreeMap<String, String>,
    /// The context values (`.context(key, value)`).
    pub context: BTreeMap<String, Value>,
    /// The user set with `.user(id)`, else the request's.
    pub user: Option<String>,
    /// The request's route (`GET /api/v1/deals/{id}`), when the framework
    /// captured it.
    pub route: Option<String>,
    /// The request id, inside a request.
    pub request_id: Option<String>,
    /// Whether it was a panic.
    pub panic: bool,
}

/// What the fake holds.
#[derive(Debug, Default)]
pub(crate) struct FakeState {
    reports: Mutex<Vec<Reported>>,
    alerts: Mutex<Vec<Alert>>,
    tags: Mutex<BTreeMap<String, String>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl FakeState {
    pub(crate) fn record(&self, report: &Report, context: &EventContext) {
        let mut tags = lock(&self.tags).clone();
        tags.extend(report.tags.clone());
        lock(&self.reports).push(Reported {
            message: report.message.clone(),
            chain: report.chain.clone(),
            error_type: report.error_type.map(str::to_owned),
            debug: report.debug.clone(),
            level: report.level,
            fingerprint: report.fingerprint.clone(),
            tags,
            context: report.context.clone(),
            user: report.user.clone().or_else(|| context.user_id.clone()),
            route: None,
            request_id: context.request_id.clone(),
            panic: false,
        });
    }

    pub(crate) fn exception(&self, exception: &ExceptionReported, context: &EventContext) {
        let route = exception.request.as_ref().map(|request| {
            format!(
                "{} {}",
                request.method,
                request.route.as_deref().unwrap_or(&request.path)
            )
        });
        lock(&self.reports).push(Reported {
            message: exception.message.clone(),
            chain: exception.chain.clone(),
            error_type: None,
            debug: None,
            level: Level::Error,
            fingerprint: None,
            tags: lock(&self.tags).clone(),
            context: BTreeMap::new(),
            user: context.user_id.clone(),
            route,
            request_id: context.request_id.clone(),
            panic: exception.panic,
        });
    }

    pub(crate) fn tag(&self, key: String, value: String) {
        lock(&self.tags).insert(key, value);
    }

    pub(crate) fn alert(&self, alert: Alert) {
        lock(&self.alerts).push(alert);
    }
}

/// Records reports and alerts instead of storing and sending them, so a
/// test can check what your code reports. Install its
/// [`lighthouse`](LighthouseFake::lighthouse) in place of the app's:
///
/// ```ignore
/// let fake = LighthouseFake::new();
/// let app = TestApp::builder()
///     .module(LighthouseModule::from_config())
///     .override_value(Arc::new(fake.lighthouse()))
///     .build()
///     .await;
/// ```
///
/// The fake also records the exceptions the framework captures (a handler
/// answering 500 shows up in [`reports`](LighthouseFake::reports) with its
/// route), because the module's telemetry sink hands them to whichever
/// `Lighthouse` the app holds.
#[derive(Clone, Debug, Default)]
pub struct LighthouseFake {
    state: Arc<FakeState>,
}

impl LighthouseFake {
    /// A fake with nothing recorded.
    pub fn new() -> LighthouseFake {
        LighthouseFake::default()
    }

    /// A [`Lighthouse`] that records into this fake.
    pub fn lighthouse(&self) -> Lighthouse {
        Lighthouse::fake(self.state.clone())
    }

    /// Every recorded report, oldest first.
    pub fn reports(&self) -> Vec<Reported> {
        lock(&self.state.reports).clone()
    }

    /// The alerts raised through [`Lighthouse::raise_alert`], oldest first.
    pub fn alerts(&self) -> Vec<Alert> {
        lock(&self.state.alerts).clone()
    }

    /// Forgets every report, alert and tag.
    pub fn clear(&self) {
        lock(&self.state.reports).clear();
        lock(&self.state.alerts).clear();
        lock(&self.state.tags).clear();
    }

    /// Asserts a report of an error of type `E` (as passed to
    /// [`Lighthouse::report`]) for which `predicate` holds.
    ///
    /// # Panics
    ///
    /// When there is none; the message lists what was reported.
    #[track_caller]
    pub fn assert_reported<E: ?Sized + 'static>(&self, predicate: impl Fn(&Reported) -> bool) {
        let name = std::any::type_name::<E>();
        let reports = self.reports();
        assert!(
            reports
                .iter()
                .any(|report| report.error_type.as_deref() == Some(name) && predicate(report)),
            "expected a report of a `{name}` matching the predicate; reported: {:#?}",
            summary(&reports)
        );
    }

    /// Asserts a report whose message contains `text`.
    ///
    /// # Panics
    ///
    /// When there is none.
    #[track_caller]
    pub fn assert_reported_message(&self, text: &str) {
        let reports = self.reports();
        assert!(
            reports.iter().any(|report| report.message.contains(text)),
            "expected a report containing {text:?}; reported: {:#?}",
            summary(&reports)
        );
    }

    /// Asserts exactly `n` reports.
    ///
    /// # Panics
    ///
    /// When there are more or fewer.
    #[track_caller]
    pub fn assert_reported_times(&self, n: usize) {
        let reports = self.reports();
        assert_eq!(
            reports.len(),
            n,
            "expected {n} reports; reported: {:#?}",
            summary(&reports)
        );
    }

    /// Asserts there was no report.
    ///
    /// # Panics
    ///
    /// When something was reported.
    #[track_caller]
    pub fn assert_nothing_reported(&self) {
        self.assert_reported_times(0);
    }
}

fn summary(reports: &[Reported]) -> Vec<String> {
    reports
        .iter()
        .map(|report| match &report.error_type {
            Some(error_type) => format!("{error_type}: {}", report.message),
            None => report.message.clone(),
        })
        .collect()
}
