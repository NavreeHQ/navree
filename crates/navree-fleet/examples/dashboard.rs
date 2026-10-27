//! A demo app for looking at the Fleet dashboard with live data:
//!
//! ```sh
//! cargo run -p navree-fleet --example dashboard
//! ```
//!
//! Then open <http://127.0.0.1:8090/_fleet> and use the access token `demo`.
//! A master runs in-process workers while a feeder queues jobs (some slow,
//! some failing) and a backlog builds up on `mail`.

use std::sync::Arc;
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

/// Accepts `Authorization: Bearer demo`.
struct DemoAuth;

impl Authenticator for DemoAuth {
    fn authenticate<'a>(
        &'a self,
        parts: &'a mut Parts,
    ) -> BoxFuture<'a, Result<Option<Principal>, AppError>> {
        let ok = parts
            .headers
            .get(http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            == Some("Bearer demo");
        Box::pin(async move {
            Ok(ok.then(|| Principal {
                user_id: Uuid::from_u128(1),
                via: AuthVia::Token,
                token_abilities: None,
                session_id: None,
            }))
        })
    }
}

fn abilities(ctx: &AbilityCtx, rules: &mut Rules) {
    if ctx.user.is_some() {
        rules.can("view", "Fleet");
        rules.can("operate", "Fleet");
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

/// A mail; on queue `mail`.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct SendDigest {
    user_id: Uuid,
    api_token: String,
}

impl Job for SendDigest {
    const NAME: &'static str = "mail.send_digest";

    fn options() -> JobOptions {
        JobOptions {
            queue: "mail".into(),
            ..JobOptions::default()
        }
    }

    async fn handle(self, _ctx: JobContext) -> Result<(), BoxError> {
        tokio::time::sleep(Duration::from_millis(400)).await;
        Ok(())
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
            max_attempts: 2,
            backoff: Backoff::Fixed { secs: 1 },
            ..JobOptions::default()
        }
    }

    async fn handle(self, _ctx: JobContext) -> Result<(), BoxError> {
        Err("smtp.mail.local:587: connection refused (os error 111)".into())
    }
}

struct DemoModule;

impl Module for DemoModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.job::<Report>()
            .job::<SendDigest>()
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
        .set("http.addr", json!("127.0.0.1:8090"))
        .set("http.per_ip_rate_limit", Value::Null)
        .set("queue.poll_interval_ms", json!(100))
        .set("fleet.snapshot_secs", json!(5))
        .set("fleet.long_wait", json!({ "mail": 20 }))
        .set(
            "fleet.supervisors",
            json!({
                "default": { "queues": ["default", "mail"], "max_processes": 4, "concurrency": 2, "max_jobs": 40 },
                "exports": { "queues": ["exports"], "balance": "off", "max_processes": 1 }
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
    tokio::spawn(async move {
        let mut n: u64 = 0;
        loop {
            n += 1;
            let _ = queue
                .dispatch(Report {
                    ms: 50 + (n * 37) % 400,
                })
                .await;
            if n % 3 == 0 {
                for _ in 0..6 {
                    let _ = queue
                        .dispatch(SendDigest {
                            user_id: Uuid::now_v7(),
                            api_token: "tok_live_123".into(),
                        })
                        .await;
                }
            }
            if n % 20 == 0 {
                let _ = queue
                    .dispatch(ExportContacts {
                        contact: format!("contact-{n}"),
                        password: "hunter2".into(),
                    })
                    .await;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    });
    println!("Fleet dashboard: http://127.0.0.1:8090/_fleet (token: demo)");
    app.run_command("serve", Vec::new()).await?;
    Ok(())
}
