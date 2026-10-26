//! `grpc.health.v1.Health`, answered from the app's readiness checks (every
//! `dyn HealthCheck` multi-binding, the checks behind `/health/ready`).

use std::collections::HashSet;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt as _;
use navree_contracts::{HealthCheck, HealthStatus};
use tokio_stream::Stream;
use tonic::{Request, Response, Status};
use tonic_health::pb::health_check_response::ServingStatus;
use tonic_health::pb::health_server::Health;
use tonic_health::pb::{HealthCheckRequest, HealthCheckResponse};

/// How long one readiness check may take before it counts as down.
const CHECK_TIMEOUT: Duration = Duration::from_secs(5);

/// How often `Watch` re-runs the checks.
const WATCH_INTERVAL: Duration = Duration::from_secs(5);

/// The health service: `SERVING` while every readiness check passes,
/// `NOT_SERVING` otherwise, for the server as a whole (`""`) and for each
/// registered service; `NOT_FOUND` (`SERVICE_UNKNOWN` in `Watch`) for any
/// other name.
pub(crate) struct ReadinessHealth {
    pub(crate) checks: Vec<Arc<dyn HealthCheck>>,
    pub(crate) services: HashSet<String>,
}

impl ReadinessHealth {
    /// The status of the app.
    async fn status(checks: &[Arc<dyn HealthCheck>]) -> ServingStatus {
        let runs = checks.iter().map(|check| async move {
            match tokio::time::timeout(CHECK_TIMEOUT, check.check()).await {
                Ok(HealthStatus::Up) => true,
                Ok(HealthStatus::Down(reason)) => {
                    tracing::warn!(target: "navree::grpc", check = check.name(), %reason, "readiness check failed");
                    false
                }
                Err(_) => {
                    tracing::warn!(target: "navree::grpc", check = check.name(), "readiness check timed out");
                    false
                }
            }
        });
        if futures_util::future::join_all(runs).await.into_iter().all(|up| up) {
            ServingStatus::Serving
        } else {
            ServingStatus::NotServing
        }
    }

    fn knows(&self, service: &str) -> bool {
        service.is_empty() || self.services.contains(service)
    }
}

fn response(status: ServingStatus) -> HealthCheckResponse {
    HealthCheckResponse {
        status: status as i32,
    }
}

type WatchStream = Pin<Box<dyn Stream<Item = Result<HealthCheckResponse, Status>> + Send>>;

#[tonic::async_trait]
impl Health for ReadinessHealth {
    async fn check(
        &self,
        request: Request<HealthCheckRequest>,
    ) -> Result<Response<HealthCheckResponse>, Status> {
        let service = request.into_inner().service;
        if !self.knows(&service) {
            return Err(Status::not_found(format!("unknown service `{service}`")));
        }
        Ok(Response::new(response(Self::status(&self.checks).await)))
    }

    type WatchStream = WatchStream;

    async fn watch(
        &self,
        request: Request<HealthCheckRequest>,
    ) -> Result<Response<Self::WatchStream>, Status> {
        let service = request.into_inner().service;
        if !self.knows(&service) {
            let unknown = futures_util::stream::once(async { Ok(response(ServingStatus::ServiceUnknown)) })
                .chain(futures_util::stream::pending());
            return Ok(Response::new(Box::pin(unknown)));
        }
        let checks = self.checks.clone();
        let updates = futures_util::stream::unfold(
            (checks, None::<ServingStatus>),
            |(checks, last)| async move {
                loop {
                    if last.is_some() {
                        tokio::time::sleep(WATCH_INTERVAL).await;
                    }
                    let status = ReadinessHealth::status(&checks).await;
                    if Some(status) != last {
                        return Some((Ok(response(status)), (checks, Some(status))));
                    }
                }
            },
        );
        Ok(Response::new(Box::pin(updates)))
    }
}

#[cfg(test)]
mod tests {
    use navree_kernel::BoxFuture;

    use super::*;

    struct Fixed(&'static str, bool);

    impl HealthCheck for Fixed {
        fn name(&self) -> &str {
            self.0
        }

        fn check(&self) -> BoxFuture<'_, HealthStatus> {
            let up = self.1;
            Box::pin(async move {
                if up {
                    HealthStatus::Up
                } else {
                    HealthStatus::Down("unreachable".into())
                }
            })
        }
    }

    fn health(checks: Vec<Arc<dyn HealthCheck>>) -> ReadinessHealth {
        ReadinessHealth {
            checks,
            services: HashSet::from(["crm.deals.v1.Deals".to_owned()]),
        }
    }

    fn ask(service: &str) -> Request<HealthCheckRequest> {
        Request::new(HealthCheckRequest {
            service: service.to_owned(),
        })
    }

    #[tokio::test]
    async fn serving_follows_the_readiness_checks() {
        let up = health(vec![Arc::new(Fixed("database", true))]);
        let status = up.check(ask("")).await.unwrap().into_inner().status;
        assert_eq!(status, ServingStatus::Serving as i32);
        let status = up.check(ask("crm.deals.v1.Deals")).await.unwrap().into_inner().status;
        assert_eq!(status, ServingStatus::Serving as i32);

        let down = health(vec![
            Arc::new(Fixed("database", true)),
            Arc::new(Fixed("storage", false)),
        ]);
        let status = down.check(ask("")).await.unwrap().into_inner().status;
        assert_eq!(status, ServingStatus::NotServing as i32);

        let unknown = down.check(ask("nope.Service")).await.unwrap_err();
        assert_eq!(unknown.code(), tonic::Code::NotFound);
    }

    #[tokio::test]
    async fn watch_sends_the_current_status_first() {
        let up = health(vec![]);
        let mut stream = up.watch(ask("")).await.unwrap().into_inner();
        let first = stream.next().await.unwrap().unwrap();
        assert_eq!(first.status, ServingStatus::Serving as i32);

        let mut unknown = up.watch(ask("nope.Service")).await.unwrap().into_inner();
        let first = unknown.next().await.unwrap().unwrap();
        assert_eq!(first.status, ServingStatus::ServiceUnknown as i32);
    }
}
