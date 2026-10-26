//! [`TelemetryContext`]: the request or job the current task works for.

use std::fmt;
use std::future::Future;
use std::sync::{Arc, Once, OnceLock};

use crate::{EventContext, Telemetry, TraceContext, TraceId};

tokio::task_local! {
    static CURRENT: TelemetryContext;
}

/// The unit of work the current task belongs to: an HTTP request (its
/// request id) or a job run (its job id), its [`TraceContext`], the user
/// and workspace once known, and the app's [`Telemetry`].
///
/// navree-http makes one current for every request and navree-queue for
/// every job attempt, with [`scope`](TelemetryContext::scope); events
/// emitted inside carry it as their [`EventContext`], and
/// navree-http-client sends its trace in `traceparent`. It is a task-local
/// value: a spawned task does not inherit it unless the future is wrapped
/// with [`propagate`](TelemetryContext::propagate).
///
/// ```
/// use navree_telemetry::{Telemetry, TelemetryContext, TraceContext};
///
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() {
/// let context = TelemetryContext::new(Telemetry::disabled())
///     .with_job_id("0199c1d2")
///     .with_trace(TraceContext::root(true));
/// context
///     .scope(async {
///         let current = TelemetryContext::current().expect("in scope");
///         assert_eq!(current.job_id(), Some("0199c1d2"));
///     })
///     .await;
/// # }
/// ```
#[derive(Clone)]
pub struct TelemetryContext {
    inner: Arc<Inner>,
}

#[derive(Clone)]
struct Inner {
    telemetry: Telemetry,
    request_id: Option<String>,
    job_id: Option<String>,
    trace: Option<TraceContext>,
    user_id: OnceLock<String>,
    workspace_id: OnceLock<String>,
    panic_location: OnceLock<String>,
}

impl TelemetryContext {
    /// A context reporting to `telemetry`, with nothing else set.
    pub fn new(telemetry: Telemetry) -> Self {
        Self {
            inner: Arc::new(Inner {
                telemetry,
                request_id: None,
                job_id: None,
                trace: None,
                user_id: OnceLock::new(),
                workspace_id: OnceLock::new(),
                panic_location: OnceLock::new(),
            }),
        }
    }

    /// With the HTTP request id.
    #[must_use]
    pub fn with_request_id(mut self, request_id: impl Into<String>) -> Self {
        Arc::make_mut(&mut self.inner).request_id = Some(request_id.into());
        self
    }

    /// With the job id.
    #[must_use]
    pub fn with_job_id(mut self, job_id: impl Into<String>) -> Self {
        Arc::make_mut(&mut self.inner).job_id = Some(job_id.into());
        self
    }

    /// With the trace and this unit of work's span.
    #[must_use]
    pub fn with_trace(mut self, trace: TraceContext) -> Self {
        Arc::make_mut(&mut self.inner).trace = Some(trace);
        self
    }

    /// The current task's context, if it runs inside a
    /// [`scope`](TelemetryContext::scope).
    pub fn current() -> Option<TelemetryContext> {
        CURRENT.try_with(Clone::clone).ok()
    }

    /// Calls `f` with the current context without cloning it; `None`
    /// outside a scope.
    pub fn with_current<R>(f: impl FnOnce(&TelemetryContext) -> R) -> Option<R> {
        CURRENT.try_with(f).ok()
    }

    /// Runs `future` with this context as the current one.
    pub async fn scope<F: Future>(self, future: F) -> F::Output {
        CURRENT.scope(self, future).await
    }

    /// Runs `f` with this context as the current one (no runtime needed).
    pub fn sync_scope<R>(self, f: impl FnOnce() -> R) -> R {
        CURRENT.sync_scope(self, f)
    }

    /// `future`, running in the context current *now* (when `propagate`
    /// is called), for `tokio::spawn` and task trackers; as it is when
    /// there is none.
    pub fn propagate<F: Future>(future: F) -> impl Future<Output = F::Output> {
        let current = Self::current();
        async move {
            match current {
                Some(context) => context.scope(future).await,
                None => future.await,
            }
        }
    }

    /// The app's telemetry.
    pub fn telemetry(&self) -> &Telemetry {
        &self.inner.telemetry
    }

    /// The HTTP request id.
    pub fn request_id(&self) -> Option<&str> {
        self.inner.request_id.as_deref()
    }

    /// The job id.
    pub fn job_id(&self) -> Option<&str> {
        self.inner.job_id.as_deref()
    }

    /// The trace and this unit of work's span.
    pub fn trace(&self) -> Option<&TraceContext> {
        self.inner.trace.as_ref()
    }

    /// Records the authenticated user (navree-http does, once it
    /// authenticated the request). The first value stays.
    pub fn set_user_id(&self, user_id: impl ToString) {
        let _ = self.inner.user_id.set(user_id.to_string());
    }

    /// The authenticated user, once recorded.
    pub fn user_id(&self) -> Option<&str> {
        self.inner.user_id.get().map(String::as_str)
    }

    /// Records the workspace the request works in (navree-workspace does,
    /// once it resolved it). The first value stays.
    pub fn set_workspace_id(&self, workspace_id: impl ToString) {
        let _ = self.inner.workspace_id.set(workspace_id.to_string());
    }

    /// The workspace, once recorded.
    pub fn workspace_id(&self) -> Option<&str> {
        self.inner.workspace_id.get().map(String::as_str)
    }

    /// Where the first panic in this unit of work happened
    /// (`file:line:column`), once [`capture_panic_locations`] is installed.
    pub fn panic_location(&self) -> Option<&str> {
        self.inner.panic_location.get().map(String::as_str)
    }

    /// What an event emitted now carries.
    pub fn event_context(&self) -> EventContext {
        EventContext {
            request_id: self.inner.request_id.clone(),
            job_id: self.inner.job_id.clone(),
            trace: self.inner.trace.clone(),
            user_id: self.inner.user_id.get().cloned(),
            workspace_id: self.inner.workspace_id.get().cloned(),
        }
    }
}

impl fmt::Debug for TelemetryContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TelemetryContext")
            .field("request_id", &self.inner.request_id)
            .field("job_id", &self.inner.job_id)
            .field("trace", &self.inner.trace)
            .field("user_id", &self.inner.user_id.get())
            .field("workspace_id", &self.inner.workspace_id.get())
            .finish()
    }
}

/// Installs, once per process, a panic hook that records where a panic
/// happened in the current [`TelemetryContext`]
/// ([`panic_location`](TelemetryContext::panic_location)), then runs the
/// hook that was installed before (which prints the panic as usual).
/// navree-http installs it when a sink wants
/// [`ExceptionReported`](crate::ExceptionReported) events, so a panicking
/// handler is reported with its location.
pub fn capture_panic_locations() {
    static INSTALLED: Once = Once::new();
    INSTALLED.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if let Some(location) = info.location() {
                let _ = CURRENT.try_with(|context| {
                    let _ = context.inner.panic_location.set(format!(
                        "{}:{}:{}",
                        location.file(),
                        location.line(),
                        location.column()
                    ));
                });
            }
            previous(info);
        }));
    });
}

/// The trace id of the current request or job, if it has one: a reference
/// to show users (or to search logs and traces with).
///
/// ```
/// let reference = navree_telemetry::current_trace_id()
///     .map(|id| id.to_string())
///     .unwrap_or_default();
/// # assert!(reference.is_empty());
/// ```
pub fn current_trace_id() -> Option<TraceId> {
    TelemetryContext::with_current(|context| context.trace().map(|trace| trace.trace_id)).flatten()
}
