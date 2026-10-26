//! The process-wide Prometheus recorder, its upkeep task and the `/metrics`
//! bearer-token guard.

use std::fmt;
use std::sync::OnceLock;
use std::time::Duration;

use axum::http::HeaderValue;
use axum::http::header::{AUTHORIZATION, WWW_AUTHENTICATE};
use axum::http::request::Parts;
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};
use navis_contracts::Secret;
use navis_http::{AppError, ClientIp, Guard};
use navis_kernel::{
    AppContext, BoxError, BoxFuture, Injectable, KernelError, Lifecycle, ProviderKey, Resolver,
};
use subtle::ConstantTimeEq;
use tokio::time::MissedTickBehavior;

use crate::ObserveOptions;

/// The Prometheus recorder, installed at most once per process: the `metrics`
/// facade has one global recorder, while apps (and test apps) may boot many
/// times. `Err` holds why installation failed, reported by every boot.
static RECORDER: OnceLock<Result<PrometheusHandle, String>> = OnceLock::new();

/// Histogram buckets (seconds) for every metric whose name ends in `_seconds`.
const SECONDS_BUCKETS: &[f64] = &[
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

/// How often histogram samples are folded into their buckets when nobody
/// scrapes `/metrics`, so memory stays bounded.
const UPKEEP_INTERVAL: Duration = Duration::from_secs(5);

/// The process-wide Prometheus handle, installing the recorder on first use.
fn prometheus() -> Result<PrometheusHandle, String> {
    RECORDER.get_or_init(install).clone()
}

fn install() -> Result<PrometheusHandle, String> {
    let handle = PrometheusBuilder::new()
        .set_buckets_for_metric(Matcher::Suffix("_seconds".to_owned()), SECONDS_BUCKETS)
        .map_err(|error| error.to_string())?
        .install_recorder()
        .map_err(|error| {
            format!(
                "cannot install the Prometheus metrics recorder ({error}); another `metrics` \
                 recorder is installed in this process: set `observe.metrics = false` to keep it"
            )
        })?;
    metrics::describe_counter!(
        "http_requests_total",
        "HTTP requests by method, route template and status."
    );
    metrics::describe_histogram!(
        "http_request_duration_seconds",
        metrics::Unit::Seconds,
        "HTTP request latency by method, route template and status."
    );
    tracing::debug!(target: "navis::observe", "Prometheus metrics recorder installed");
    Ok(handle)
}

/// The Prometheus handle when metrics are enabled; runs the upkeep task.
pub(crate) struct MetricsRecorder {
    handle: Option<PrometheusHandle>,
}

impl MetricsRecorder {
    /// The handle rendering the exposition, when metrics are enabled.
    pub(crate) fn handle(&self) -> Option<&PrometheusHandle> {
        self.handle.as_ref()
    }
}

impl Injectable for MetricsRecorder {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<ObserveOptions>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            let options = r.get::<ObserveOptions>().await?;
            let handle = if options.metrics {
                Some(prometheus().map_err(|error| KernelError::Construction {
                    provider: std::any::type_name::<Self>().to_owned(),
                    source: error.into(),
                })?)
            } else {
                None
            };
            Ok(Self { handle })
        })
    }
}

/// Runs the recorder's upkeep until the app shuts down.
impl Lifecycle for MetricsRecorder {
    fn on_bootstrap(&self, ctx: AppContext) -> BoxFuture<'_, Result<(), BoxError>> {
        Box::pin(async move {
            if let Some(handle) = self.handle.clone() {
                let token = ctx.shutdown_token();
                ctx.tracker().spawn(async move {
                    let mut ticks = tokio::time::interval(UPKEEP_INTERVAL);
                    ticks.set_missed_tick_behavior(MissedTickBehavior::Delay);
                    loop {
                        tokio::select! {
                            () = token.cancelled() => break,
                            _ = ticks.tick() => handle.run_upkeep(),
                        }
                    }
                });
            }
            Ok(())
        })
    }
}

impl fmt::Debug for MetricsRecorder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MetricsRecorder")
            .field("enabled", &self.handle.is_some())
            .finish()
    }
}

/// Requires `Authorization: Bearer <token>` (compared in constant time).
#[derive(Debug)]
pub(crate) struct BearerToken(pub(crate) Secret<String>);

impl Guard for BearerToken {
    fn check<'a>(&'a self, parts: &'a mut Parts) -> BoxFuture<'a, Result<(), AppError>> {
        let presented = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(bearer);
        let valid = presented
            .is_some_and(|token| bool::from(token.as_bytes().ct_eq(self.0.expose().as_bytes())));
        if !valid {
            tracing::warn!(
                target: "navis::security",
                path = %parts.uri.path(),
                ip = parts.extensions.get::<ClientIp>().map(|ip| tracing::field::display(ip.0)),
                reason = if presented.is_some() { "invalid token" } else { "missing bearer token" },
                "metrics request rejected"
            );
        }
        Box::pin(async move {
            if valid {
                Ok(())
            } else {
                Err(AppError::unauthenticated()
                    .with_header(WWW_AUTHENTICATE, HeaderValue::from_static("Bearer")))
            }
        })
    }
}

/// The token of a `Bearer <token>` authorization value (scheme case-insensitive).
fn bearer(value: &str) -> Option<&str> {
    let (scheme, token) = value.trim().split_once(' ')?;
    let token = token.trim();
    (scheme.eq_ignore_ascii_case("bearer") && !token.is_empty()).then_some(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_accepts_any_scheme_case_and_trims() {
        assert_eq!(bearer("Bearer abc"), Some("abc"));
        assert_eq!(bearer("bearer   abc  "), Some("abc"));
        assert_eq!(bearer("BEARER abc"), Some("abc"));
        assert_eq!(bearer("Basic abc"), None);
        assert_eq!(bearer("Bearer"), None);
        assert_eq!(bearer("Bearer   "), None);
        assert_eq!(bearer("abc"), None);
    }
}
