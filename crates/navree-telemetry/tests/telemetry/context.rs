//! `TelemetryContext`: the request or job a task works for.

use navree_telemetry::{Telemetry, TelemetryContext, TraceContext, current_trace_id};

#[tokio::test]
async fn there_is_no_context_outside_a_scope() {
    assert!(TelemetryContext::current().is_none());
    assert_eq!(current_trace_id(), None);
}

#[tokio::test]
async fn a_scope_makes_its_context_current() {
    let trace = TraceContext::root(true);
    let context = TelemetryContext::new(Telemetry::disabled())
        .with_request_id("req-1")
        .with_trace(trace.clone());
    let seen = context
        .scope(async {
            let current = TelemetryContext::current().expect("in scope");
            (
                current.request_id().map(str::to_owned),
                current.trace().map(|t| t.span_id),
                current_trace_id(),
            )
        })
        .await;
    assert_eq!(seen.0.as_deref(), Some("req-1"));
    assert_eq!(seen.1, Some(trace.span_id));
    assert_eq!(seen.2, Some(trace.trace_id));
    assert!(
        TelemetryContext::current().is_none(),
        "gone after the scope"
    );
}

#[tokio::test]
async fn scopes_nest() {
    let outer = TelemetryContext::new(Telemetry::disabled()).with_request_id("outer");
    let inner = TelemetryContext::new(Telemetry::disabled()).with_job_id("job-1");
    outer
        .scope(async {
            inner
                .scope(async {
                    let current = TelemetryContext::current().unwrap();
                    assert_eq!(current.job_id(), Some("job-1"));
                    assert_eq!(current.request_id(), None);
                })
                .await;
            assert_eq!(
                TelemetryContext::current().unwrap().request_id(),
                Some("outer")
            );
        })
        .await;
}

#[tokio::test]
async fn user_and_workspace_set_later_are_seen_by_every_clone() {
    let context = TelemetryContext::new(Telemetry::disabled());
    let clone = context.clone();
    context.set_user_id("u-1");
    context.set_workspace_id("w-1");
    // The first value wins: a request has one user and one workspace.
    context.set_user_id("u-2");
    assert_eq!(clone.user_id(), Some("u-1"));
    assert_eq!(clone.workspace_id(), Some("w-1"));
    let event = clone.event_context();
    assert_eq!(event.user_id.as_deref(), Some("u-1"));
    assert_eq!(event.workspace_id.as_deref(), Some("w-1"));
}

#[tokio::test]
async fn spawned_tasks_inherit_the_context_through_propagate() {
    let context = TelemetryContext::new(Telemetry::disabled()).with_request_id("req-9");
    let request_id = context
        .scope(async {
            let plain = tokio::spawn(async {
                TelemetryContext::current().and_then(|c| c.request_id().map(str::to_owned))
            })
            .await
            .unwrap();
            assert_eq!(plain, None, "tokio tasks do not inherit task-locals");
            tokio::spawn(TelemetryContext::propagate(async {
                TelemetryContext::current().and_then(|c| c.request_id().map(str::to_owned))
            }))
            .await
            .unwrap()
        })
        .await;
    assert_eq!(request_id.as_deref(), Some("req-9"));
    // Outside any context, propagate runs the future as it is.
    let none = TelemetryContext::propagate(async { TelemetryContext::current().is_none() }).await;
    assert!(none);
}

#[test]
fn sync_scopes_work_without_a_runtime() {
    let context = TelemetryContext::new(Telemetry::disabled()).with_request_id("sync");
    let id = context
        .sync_scope(|| TelemetryContext::current().and_then(|c| c.request_id().map(str::to_owned)));
    assert_eq!(id.as_deref(), Some("sync"));
}
