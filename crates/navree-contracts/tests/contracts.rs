//! Behaviour of the cross-cutting contracts.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{TimeZone, Utc};
use navree_contracts::{
    AuditEntry, AuditKind, AuthVia, Backoff, BoxError, BoxFuture, Clock, Dispatcher, FixedClock,
    Job, JobContext, JobEnvelope, JobOptions, JobRegistration, ModuleBuilderJobsExt, Principal,
    RequestId, Secret, SessionRecord,
};
use navree_kernel::{AppContext, Module, ModuleBuilder, Navree};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Secret
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct DbConfig {
    #[allow(dead_code)]
    url: String,
    password: Secret<String>,
}

#[test]
fn secret_debug_is_redacted() {
    let secret = Secret::new(String::from("hunter2"));
    assert_eq!(format!("{secret:?}"), "[redacted]");
    assert_eq!(secret.to_string(), "[redacted]");

    let config = DbConfig {
        url: "postgres://db".into(),
        password: Secret::new("hunter2".into()),
    };
    assert!(!format!("{config:?}").contains("hunter2"));
    assert_eq!(config.password.expose(), "hunter2");
}

#[test]
fn secret_deserializes_transparently() {
    let config: DbConfig =
        serde_json::from_value(json!({"url": "postgres://db", "password": "hunter2"})).unwrap();
    assert_eq!(config.password.expose(), "hunter2");
    assert_eq!(config.password.clone().into_inner(), "hunter2");
    assert_eq!(config.password, Secret::new(String::from("hunter2")));
}

// ---------------------------------------------------------------------------
// RequestId
// ---------------------------------------------------------------------------

#[test]
fn request_id_rejects_long_or_bad_chars() {
    let max = "a".repeat(64);
    assert_eq!(
        RequestId::parse("abc-123_XYZ").unwrap().as_str(),
        "abc-123_XYZ"
    );
    assert_eq!(RequestId::parse(&max).unwrap().as_str(), max);

    for bad in [
        "",
        "has space",
        "dots.are.bad",
        "slash/",
        "üñí",
        "new\nline",
        &"a".repeat(65),
    ] {
        assert!(RequestId::parse(bad).is_none(), "{bad:?} must be rejected");
    }
}

#[test]
fn generated_request_ids_are_valid_and_unique() {
    let a = RequestId::generate();
    let b = RequestId::generate();
    assert_ne!(a, b);
    assert_eq!(a.as_str().len(), 32, "uuid v7 in simple (hex) form");
    assert_eq!(RequestId::parse(a.as_str()).as_ref(), Some(&a));
    assert_eq!(a.to_string(), a.as_str());
}

#[test]
fn generated_request_ids_are_uuid_v7_in_time_order_without_a_shared_lock() {
    let ids: Vec<RequestId> = (0..1000).map(|_| RequestId::generate()).collect();
    for id in &ids {
        let uuid = uuid::Uuid::parse_str(id.as_str()).expect("a uuid in simple form");
        assert_eq!(uuid.get_version_num(), 7, "{id}");
    }
    assert!(
        ids.windows(2)
            .all(|pair| pair[0].as_str() < pair[1].as_str()),
        "one thread's ids sort in the order they were made"
    );
    // Threads generate on their own, and never collide.
    let mut all: Vec<String> = std::thread::scope(|scope| {
        let threads: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    (0..1000)
                        .map(|_| RequestId::generate().as_str().to_owned())
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        threads
            .into_iter()
            .flat_map(|thread| thread.join().expect("thread"))
            .collect()
    });
    let count = all.len();
    all.sort();
    all.dedup();
    assert_eq!(all.len(), count, "no duplicates across threads");
}

#[test]
fn request_id_clones_share_the_text() {
    let id = RequestId::generate();
    let clone = id.clone();
    assert_eq!(clone, id);
    assert!(
        std::ptr::eq(id.as_str(), clone.as_str()),
        "a clone is a reference count, not a new string"
    );
}

#[test]
fn parse_or_generate_keeps_valid_ids_and_replaces_forged_ones() {
    assert_eq!(
        RequestId::parse_or_generate(Some("trace-1")).as_str(),
        "trace-1"
    );
    let replaced = RequestId::parse_or_generate(Some("<script>"));
    assert_ne!(replaced.as_str(), "<script>");
    assert!(RequestId::parse(replaced.as_str()).is_some());
    assert_eq!(RequestId::parse_or_generate(None).as_str().len(), 32);
}

// ---------------------------------------------------------------------------
// Audit entries
// ---------------------------------------------------------------------------

#[test]
fn audit_entry_builder_sets_fields_and_kind_is_snake_case() {
    let actor = Uuid::now_v7();
    let entry = AuditEntry::new(AuditKind::Security, "auth.login_failed")
        .actor(actor)
        .subject("user", "42")
        .old_values(json!({"name": "a"}))
        .new_values(json!({"name": "b"}))
        .request_id("req-1")
        .ip("10.0.0.1")
        .user_agent("curl")
        .metadata(json!({"attempts": 3}));
    let value = serde_json::to_value(&entry).unwrap();
    assert_eq!(
        value,
        json!({
            "kind": "security",
            "action": "auth.login_failed",
            "actor_id": actor,
            "subject_type": "user",
            "subject_id": "42",
            "old_values": {"name": "a"},
            "new_values": {"name": "b"},
            "request_id": "req-1",
            "ip": "10.0.0.1",
            "user_agent": "curl",
            "metadata": {"attempts": 3},
        })
    );
    assert_eq!(AuditEntry::new(AuditKind::Data, "x").metadata, json!({}));
}

// ---------------------------------------------------------------------------
// Clock
// ---------------------------------------------------------------------------

#[test]
fn fixed_clock_can_be_set_and_advanced() {
    let start = Utc.with_ymd_and_hms(2026, 10, 25, 9, 0, 0).unwrap();
    let clock = FixedClock::new(start);
    assert_eq!(clock.now(), start);
    clock.advance(Duration::from_secs(90));
    assert_eq!(
        clock.now(),
        Utc.with_ymd_and_hms(2026, 10, 25, 9, 1, 30).unwrap()
    );
    let later = Utc.with_ymd_and_hms(2027, 1, 1, 0, 0, 0).unwrap();
    clock.set(later);
    assert_eq!(clock.now(), later);
}

// ---------------------------------------------------------------------------
// Jobs
// ---------------------------------------------------------------------------

#[test]
fn backoff_exponential_doubles_and_caps() {
    let backoff = Backoff::Exponential {
        base_secs: 1,
        max_secs: 300,
    };
    let delays: Vec<u64> = [1, 2, 3, 4, 9, 10, 64, u32::MAX]
        .into_iter()
        .map(|attempt| backoff.delay_for(attempt).as_secs())
        .collect();
    assert_eq!(delays, [1, 2, 4, 8, 256, 300, 300, 300]);
    assert_eq!(
        Backoff::Fixed { secs: 5 }.delay_for(7),
        Duration::from_secs(5)
    );
    assert_eq!(
        Backoff::default(),
        Backoff::Exponential {
            base_secs: 1,
            max_secs: 300
        }
    );
}

#[test]
fn job_options_fill_missing_fields_with_defaults() {
    let options: JobOptions = serde_json::from_value(json!({"queue": "mail"})).unwrap();
    assert_eq!(
        options,
        JobOptions {
            queue: "mail".into(),
            max_attempts: 3,
            backoff: Backoff::Exponential {
                base_secs: 1,
                max_secs: 300
            },
            timeout_secs: 120,
            unique_for_secs: None,
        }
    );
}

/// Records what jobs did.
#[derive(Default)]
struct Outbox(Mutex<Vec<String>>);

struct OutboxModule(Arc<Outbox>);

impl Module for OutboxModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.value_arc(self.0.clone())
            .exports::<Outbox>()
            .job::<SendWelcome>();
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct SendWelcome {
    user: String,
}

impl Job for SendWelcome {
    const NAME: &'static str = "mail.welcome";

    fn options() -> JobOptions {
        JobOptions {
            queue: "mail".into(),
            ..JobOptions::default()
        }
    }

    async fn handle(self, ctx: JobContext) -> Result<(), BoxError> {
        let outbox = ctx.app.get::<Outbox>().await?;
        outbox.0.lock().unwrap().push(format!(
            "welcome {} ({} attempt {})",
            self.user, ctx.job_id, ctx.attempt
        ));
        Ok(())
    }
}

async fn app_with_outbox() -> (navree_kernel::App, Arc<Outbox>) {
    let outbox = Arc::new(Outbox::default());
    let app = Navree::app()
        .module(OutboxModule(outbox.clone()))
        .boot()
        .await
        .expect("boot");
    (app, outbox)
}

#[tokio::test]
async fn job_registration_runs_typed_handler() {
    let (app, outbox) = app_with_outbox().await;
    let registration = JobRegistration::of::<SendWelcome>();
    assert_eq!(registration.name, "mail.welcome");
    assert_eq!(registration.options.queue, "mail");

    let ctx = JobContext {
        app: app.ctx().clone(),
        job_id: "job-1".into(),
        attempt: 2,
    };
    (registration.run)(json!({"user": "ada"}), ctx.clone())
        .await
        .expect("handler");
    assert_eq!(*outbox.0.lock().unwrap(), ["welcome ada (job-1 attempt 2)"]);

    let error = (registration.run)(json!({"nobody": true}), ctx)
        .await
        .expect_err("bad payload");
    assert!(error.to_string().contains("user"), "{error}");
}

#[tokio::test]
async fn module_job_ext_contributes_registration() {
    let (app, _) = app_with_outbox().await;
    let names: Vec<&str> = app
        .ctx()
        .contributions::<JobRegistration>()
        .iter()
        .map(|c| c.value.name)
        .collect();
    assert_eq!(names, ["mail.welcome"]);
}

/// A dispatcher that captures envelopes instead of queueing them.
#[derive(Default)]
struct Capture(Mutex<Vec<JobEnvelope>>);

impl Dispatcher for Capture {
    fn dispatch_envelope(&self, env: JobEnvelope) -> BoxFuture<'_, Result<String, BoxError>> {
        Box::pin(async move {
            let mut captured = self.0.lock().unwrap();
            captured.push(env);
            Ok(format!("job-{}", captured.len()))
        })
    }
}

#[tokio::test]
async fn dispatch_serializes_job_into_envelope() {
    let capture = Arc::new(Capture::default());
    let dispatcher: Arc<dyn Dispatcher> = capture.clone();

    let first = dispatcher
        .dispatch(SendWelcome { user: "ada".into() })
        .await
        .unwrap();
    let second = dispatcher
        .dispatch_later(SendWelcome { user: "bob".into() }, Duration::from_secs(90))
        .await
        .unwrap();
    assert_eq!((first.as_str(), second.as_str()), ("job-1", "job-2"));

    let envelopes = capture.0.lock().unwrap();
    assert_eq!(envelopes[0].job_type, "mail.welcome");
    assert_eq!(envelopes[0].payload, json!({"user": "ada"}));
    assert_eq!(envelopes[0].options, SendWelcome::options());
    assert_eq!(envelopes[0].delay_secs, None);
    assert_eq!(envelopes[0].unique_key, None);
    assert_eq!(envelopes[1].payload, json!({"user": "bob"}));
    assert_eq!(envelopes[1].delay_secs, Some(90));
}

#[tokio::test]
async fn dispatch_later_rounds_delay_up_to_whole_seconds() {
    let capture = Arc::new(Capture::default());
    let dispatcher: Arc<dyn Dispatcher> = capture.clone();
    dispatcher
        .dispatch_later(
            SendWelcome { user: "ada".into() },
            Duration::from_millis(1500),
        )
        .await
        .unwrap();
    assert_eq!(capture.0.lock().unwrap()[0].delay_secs, Some(2));
}

// ---------------------------------------------------------------------------
// Values that must not leak into logs
// ---------------------------------------------------------------------------

#[test]
fn principal_debug_redacts_session_id() {
    let principal = Principal {
        user_id: Uuid::now_v7(),
        via: AuthVia::Session,
        token_abilities: None,
        session_id: Some("s3cr3t-session".into()),
    };
    let debug = format!("{principal:?}");
    assert!(!debug.contains("s3cr3t-session"), "{debug}");
    assert!(debug.contains(&principal.user_id.to_string()), "{debug}");
}

#[test]
fn session_record_debug_lists_keys_only() {
    let mut record = SessionRecord::default();
    record.data.insert("_csrf".into(), json!("t0ken-value"));
    let debug = format!("{record:?}");
    assert!(debug.contains("_csrf"), "{debug}");
    assert!(!debug.contains("t0ken-value"), "{debug}");
}

/// Keeps the `AppContext` import used even if helpers change.
#[allow(dead_code)]
fn _uses(_: AppContext) {}
