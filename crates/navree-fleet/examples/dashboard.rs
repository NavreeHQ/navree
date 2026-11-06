//! A demo app for looking at the Fleet dashboard with live data:
//!
//! ```sh
//! cargo run -p navree-fleet --example dashboard
//! ```
//!
//! Then open <http://127.0.0.1:8090/_fleet> and sign in as
//! `ops@example.in` (may look and operate) or `viewer@example.in` (may only
//! look), with the password `correct horse battery`. A master runs
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

use navree_access::{AbilityCtx, AccessModule, AccessOptions, AccessService, Rules};
use navree_auth::{AuthModule, AuthPages, AuthService};
use navree_config::{ConfigModule, ConfigSources};
use navree_contracts::{Backoff, BoxError, Job, JobContext, JobOptions, ModuleBuilderJobsExt};
use navree_crypto::{CryptoModule, generate_key};
use navree_db::{DatabaseModule, Db};
use navree_fleet::{FleetModule, InProcessSpawner, Master, MasterSettings};
use navree_http::HttpModule;
use navree_kernel::{Module, ModuleBuilder, Navree};
use navree_queue::{Queue, QueueModule};
use navree_session::{SessionModule, SessionOptions};
use navree_ui::UiModule;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

/// The password of both demo accounts.
const PASSWORD: &str = "correct horse battery";

/// Only the permissions stored for a user count (granted at start).
fn abilities(ctx: &AbilityCtx, rules: &mut Rules) {
    ctx.grant_permissions(rules);
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
            .job::<ExportContacts>();
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
        .set("app.key", json!(generate_key()))
        .set("hashing.memory_kib", json!(1024))
        .set("hashing.iterations", json!(1))
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
        .module(CryptoModule)
        .module(DatabaseModule::from_config())
        .module(QueueModule::from_config())
        // The demo serves plain HTTP on this machine: no `Secure` cookie.
        .module(SessionModule::for_root(SessionOptions {
            secure: false,
            ..SessionOptions::default()
        }))
        .module(AuthModule::from_config())
        .module(AccessModule::for_root(AccessOptions {
            abilities,
            ..AccessOptions::default()
        }))
        .module(UiModule)
        .module(AuthPages::new().app_name("Agra Operations"))
        .module(FleetModule::from_config())
        .module(DemoModule)
        .boot()
        .await?;
    let ctx = app.ctx().clone();
    // Two people: one who may look and operate, one who may only look.
    let auth = ctx.get::<AuthService>().await?;
    let access = ctx.get::<AccessService>().await?;
    let db = ctx.get::<Db>().await?;
    let uow = db.uow(None, None).await;
    for (name, email, permissions) in [
        (
            "Harjinder Singh",
            "ops@example.in",
            &["fleet.view", "fleet.operate"][..],
        ),
        ("Gurpreet Kaur", "viewer@example.in", &["fleet.view"][..]),
    ] {
        let user = auth.create_user(name, email, PASSWORD).await?;
        for permission in permissions {
            access.grant_to_user(&uow, user.id, permission).await?;
        }
    }
    uow.commit().await?;
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
    println!(
        "Fleet dashboard: http://{addr}/_fleet (ops@example.in or viewer@example.in, password {PASSWORD})"
    );
    app.run_command("serve", Vec::new()).await?;
    Ok(())
}
