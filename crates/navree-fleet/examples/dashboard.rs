//! A demo app for looking at the Fleet dashboard with live data:
//!
//! ```sh
//! cargo run -p navree-fleet --example dashboard
//! ```
//!
//! Then open <http://127.0.0.1:8090/_fleet> and use the access token `demo`
//! (`viewer` may only look; `nobody` has no access). A master runs
//! in-process workers while a feeder queues jobs (some slow, some failing).
//!
//! Settings, from the environment:
//!
//! - `DEMO_ADDR`: where to listen (default `127.0.0.1:8090`).
//! - `DEMO_BACKLOG`: queue this many `mail.send` jobs, like a newsletter
//!   (default none), `DEMO_BACKLOG_AT` seconds after the start (default 20).
//! - `DEMO_MAIL_MS`: how long a `mail.send` takes (default 400).
//! - `DEMO_QUIET=1`: no background feeder, only the backlog.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use http::request::Parts;
use navree_access::{AbilityCtx, AccessModule, AccessOptions, Rules};
use navree_config::{ConfigModule, ConfigSources};
use navree_contracts::{
    AuthVia, Backoff, BoxError, Job, JobContext, JobOptions, ModuleBuilderJobsExt, Principal,
};
use navree_db::DatabaseModule;
use navree_fleet::{FleetModule, InProcessSpawner, Master, MasterSettings};
use navree_http::{AppError, Authenticator, HttpModule};
use navree_kernel::{BoxFuture, Factory, Module, ModuleBuilder, Navree};
use navree_queue::{Queue, QueueModule};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

/// Accepts `Authorization: Bearer demo` (may operate), `viewer` (may look)
/// and `nobody` (may do nothing).
struct DemoAuth;

impl Authenticator for DemoAuth {
    fn authenticate<'a>(
        &'a self,
        parts: &'a mut Parts,
    ) -> BoxFuture<'a, Result<Option<Principal>, AppError>> {
        let user = match parts
            .headers
            .get(http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
        {
            Some("Bearer demo") => Some(1),
            Some("Bearer viewer") => Some(2),
            Some("Bearer nobody") => Some(3),
            _ => None,
        };
        Box::pin(async move {
            Ok(user.map(|id| Principal {
                user_id: Uuid::from_u128(id),
                via: AuthVia::Token,
                token_abilities: None,
                session_id: None,
            }))
        })
    }
}

fn abilities(ctx: &AbilityCtx, rules: &mut Rules) {
    match ctx.user.as_ref().map(|user| user.id) {
        Some(id) if id == Uuid::from_u128(1) => {
            rules.can("view", "Fleet");
            rules.can("operate", "Fleet");
        }
        Some(id) if id == Uuid::from_u128(2) => {
            rules.can("view", "Fleet");
        }
        _ => {}
    }
}

/// Takes `ms` milliseconds.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Report {
    ms: u64,
}

impl Job for Report {
    const NAME: &'static str = "reports.build";

    async fn handle(self, _ctx: JobContext) -> Result<(), BoxError> {
        tokio::time::sleep(Duration::from_millis(self.ms)).await;
        Ok(())
    }
}

/// How long a mail takes, in milliseconds.
static MAIL_MS: AtomicU64 = AtomicU64::new(400);

/// A mail; on queue `mail`. One in a hundred cannot reach the server.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct SendMail {
    user_id: Uuid,
    api_token: String,
}

impl Job for SendMail {
    const NAME: &'static str = "mail.send";

    fn options() -> JobOptions {
        JobOptions {
            queue: "mail".into(),
            max_attempts: 5,
            backoff: Backoff::Fixed { secs: 1 },
            ..JobOptions::default()
        }
    }

    async fn handle(self, _ctx: JobContext) -> Result<(), BoxError> {
        tokio::time::sleep(Duration::from_millis(MAIL_MS.load(Ordering::Relaxed))).await;
        if self.user_id.as_u128().is_multiple_of(100) {
            return Err("connection refused: smtp.internal:587".into());
        }
        Ok(())
    }
}

/// A push; on queue `notifications`.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct PushNotification {
    user_id: Uuid,
}

impl Job for PushNotification {
    const NAME: &'static str = "notifications.push";

    fn options() -> JobOptions {
        JobOptions {
            queue: "notifications".into(),
            ..JobOptions::default()
        }
    }

    async fn handle(self, _ctx: JobContext) -> Result<(), BoxError> {
        tokio::time::sleep(Duration::from_millis(80)).await;
        Ok(())
    }
}

/// A listener with a long name, as the events module names its jobs.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct SyncSeats {
    workspace: Uuid,
}

impl Job for SyncSeats {
    const NAME: &'static str =
        "events.listener.workspace.member_added.navree_billing::seats::SyncSeatsAfterMemberAdded";

    async fn handle(self, _ctx: JobContext) -> Result<(), BoxError> {
        tokio::time::sleep(Duration::from_millis(120)).await;
        Ok(())
    }
}

/// A webhook to an endpoint that is gone; on queue `default`.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct DeliverWebhook {
    endpoint: String,
}

impl Job for DeliverWebhook {
    const NAME: &'static str = "webhooks.deliver";

    fn options() -> JobOptions {
        JobOptions {
            max_attempts: 8,
            backoff: Backoff::Fixed { secs: 1 },
            ..JobOptions::default()
        }
    }

    async fn handle(self, _ctx: JobContext) -> Result<(), BoxError> {
        Err("endpoint answered 410 Gone".into())
    }
}

/// Fails; on queue `exports`.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct ExportContacts {
    contact: String,
    password: String,
}

impl Job for ExportContacts {
    const NAME: &'static str = "contacts.export";

    fn options() -> JobOptions {
        JobOptions {
            queue: "exports".into(),
            max_attempts: 3,
            backoff: Backoff::Fixed { secs: 1 },
            ..JobOptions::default()
        }
    }

    async fn handle(self, _ctx: JobContext) -> Result<(), BoxError> {
        Err("timed out after 120 s".into())
    }
}

struct DemoModule;

impl Module for DemoModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.job::<Report>()
            .job::<SendMail>()
            .job::<PushNotification>()
            .job::<DeliverWebhook>()
            .job::<SyncSeats>()
            .job::<ExportContacts>()
            .global()
            .factory(Factory::<dyn Authenticator>::new(|_| async {
                Ok(Arc::new(DemoAuth) as Arc<dyn Authenticator>)
            }))
            .exports::<dyn Authenticator>();
    }
}

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    let addr = std::env::var("DEMO_ADDR").unwrap_or_else(|_| "127.0.0.1:8090".into());
    let backlog: u64 = std::env::var("DEMO_BACKLOG")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let backlog_at: u64 = std::env::var("DEMO_BACKLOG_AT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);
    let quiet = std::env::var("DEMO_QUIET").as_deref() == Ok("1");
    if let Some(ms) = std::env::var("DEMO_MAIL_MS")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        MAIL_MS.store(ms, Ordering::Relaxed);
    }
    let dir = tempfile::tempdir()?;
    let url = format!(
        "sqlite://{}?mode=rwc",
        dir.path()
            .join("fleet.db")
            .display()
            .to_string()
            .replace('\\', "/")
    );
    let sources = ConfigSources::new()
        .set("database.url", Value::String(url))
        .set("database.migrate_on_boot", json!(true))
        .set("http.addr", json!(addr))
        .set("http.per_ip_rate_limit", Value::Null)
        .set("queue.poll_interval_ms", json!(100))
        .set("fleet.snapshot_secs", json!(5))
        .set("fleet.long_wait", json!({ "mail": 20 }))
        .set(
            "fleet.supervisors",
            json!({
                "default": { "queues": ["default", "mail", "notifications"], "max_processes": 8, "concurrency": 4 },
                "exports": { "queues": ["exports"], "balance": "off", "max_processes": 2 }
            }),
        );
    let app = Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(HttpModule::from_config())
        .module(DatabaseModule::from_config())
        .module(QueueModule::from_config())
        .module(AccessModule::for_root(AccessOptions {
            abilities,
            ..AccessOptions::default()
        }))
        .module(FleetModule::from_config())
        .module(DemoModule)
        .boot()
        .await?;
    let ctx = app.ctx().clone();
    let master = Master::from_app(&ctx)
        .await?
        .with_spawner(Arc::new(InProcessSpawner::new(ctx.clone())))
        .with_settings(MasterSettings {
            name: "demo-1".into(),
            ..MasterSettings::default()
        });
    tokio::spawn(master.run(ctx.shutdown_token()));
    let queue = ctx.get::<Queue>().await?;
    if backlog > 0 {
        let queue = queue.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(backlog_at)).await;
            for _ in 0..backlog {
                let _ = queue
                    .dispatch(SendMail {
                        user_id: Uuid::now_v7(),
                        api_token: "tok_live_123".into(),
                    })
                    .await;
            }
            println!("queued {backlog} mails");
        });
    }
    if !quiet {
        tokio::spawn(async move {
            let mut n: u64 = 0;
            loop {
                n += 1;
                let _ = queue
                    .dispatch(Report {
                        ms: 50 + (n * 37) % 400,
                    })
                    .await;
                let _ = queue
                    .dispatch(PushNotification {
                        user_id: Uuid::now_v7(),
                    })
                    .await;
                if n.is_multiple_of(4) {
                    let _ = queue
                        .dispatch(SendMail {
                            user_id: Uuid::now_v7(),
                            api_token: "tok_live_123".into(),
                        })
                        .await;
                }
                if n.is_multiple_of(10) {
                    let _ = queue
                        .dispatch(SyncSeats {
                            workspace: Uuid::now_v7(),
                        })
                        .await;
                }
                if n.is_multiple_of(40) {
                    let _ = queue
                        .dispatch(ExportContacts {
                            contact: format!("contact-{n}"),
                            password: "hunter2".into(),
                        })
                        .await;
                    let _ = queue
                        .dispatch(DeliverWebhook {
                            endpoint: format!("https://hooks.example.test/{n}"),
                        })
                        .await;
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        });
    }
    println!("Fleet dashboard: http://{addr}/_fleet (tokens: demo, viewer, nobody)");
    app.run_command("serve", Vec::new()).await?;
    Ok(())
}
