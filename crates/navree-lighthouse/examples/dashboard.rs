//! A demo app for looking at the Lighthouse dashboard with live data:
//!
//! ```sh
//! cargo run -p navree-lighthouse --example dashboard
//! ```
//!
//! Then open <http://127.0.0.1:8099/_lighthouse>: it sends you to the app's
//! sign-in page. Sign in as `ops@demo.test` (may view, resolve and
//! configure), `viewer@demo.test` (may only view) or `nobody@demo.test` (may
//! not open it); the password is `demo-password-1`.
//!
//! The app is a small CRM API with a few routes that fail, panic, run slow
//! or call an upstream that fails, and a queue worker with jobs that
//! succeed or fail. A traffic task calls the routes through real HTTP, so
//! every figure, issue and trace the dashboard shows was recorded, not
//! made up. Alerts go to an in-memory mailbox and to a webhook that is down.
//!
//! The 24-hour and 7-day charts need hours of traffic: after the first
//! minute the example copies the hour it recorded into the hours before it,
//! scaled by a working-day shape with noise, so the charts have a shape
//! (the same way the UI run does it). Those back-dated
//! aggregates are not traffic; pass `--no-history` to leave them out, or
//! `--empty` to start with no traffic at all, and `--no-release` to run
//! without a release label.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Timelike as _, Utc};
use http::Method;
use navree_access::{AbilityCtx, AccessModule, AccessOptions, AccessService, Rules};
use navree_auth::{AuthModule, AuthPages, AuthService};
use navree_config::{ConfigModule, ConfigSources};
use navree_contracts::{
    AuditEntry, AuditKind, AuditSink, Backoff, BoxError, Job, JobContext, JobOptions,
    ModuleBuilderJobsExt,
};
use navree_crypto::CryptoModule;
use navree_db::{DatabaseModule, Db, Executor, Statement};
use navree_http::{AppError, Controller, HttpModule, ModuleBuilderHttpExt, Path, RouteDef};
use navree_http_client::{HttpClient, HttpClientModule, HttpClientOptions};
use navree_kernel::{
    BoxFuture, Factory, Injectable, KernelError, Module, ModuleBuilder, Navree, ProviderKey,
    Resolver,
};
use navree_lighthouse::entity::aggregate;
use navree_lighthouse::{
    Alert, AlertChannel, Lighthouse, LighthouseModule, ModuleBuilderLighthouseExt, ReleaseReport,
};
use navree_orm::Model as _;
use navree_queue::{Queue, QueueModule, WorkOptions};
use navree_session::{SessionModule, SessionOptions};
use navree_ui::UiModule;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

const PORT: u16 = 8099;
const PASSWORD: &str = "demo-password-1";

/// An error with a source, for the source chain.
#[derive(Debug, thiserror::Error)]
enum InvoiceError {
    #[error("cannot charge invoice: card declined")]
    Declined(#[source] std::io::Error),
    #[error("unique violation on companies")]
    Duplicate,
}

/// The CRM routes the traffic task calls.
struct CrmController {
    db: Arc<Db>,
    upstream: Arc<HttpClient>,
}

impl Injectable for CrmController {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<Db>(), ProviderKey::of::<HttpClient>()]
    }

    fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(CrmController {
                db: r.get::<Db>().await?,
                upstream: r.get::<HttpClient>().await?,
            })
        })
    }
}

/// A pseudo-random number from the request's id, so routes vary.
fn spread(n: u64) -> u64 {
    let mut x = n.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x ^= x >> 29;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^ (x >> 32)
}

impl Controller for CrmController {
    fn prefix(&self) -> &'static str {
        "/v1"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        let (board, deal, upstream) = (self.clone(), self.clone(), self.clone());
        vec![
            RouteDef::on(Method::GET, "/deals/board", move || {
                let this = board.clone();
                async move {
                    let n = Uuid::now_v7().as_u128() as u64;
                    // One request in twenty is slow.
                    let wait = if spread(n).is_multiple_of(20) {
                        1_100 + spread(n + 1) % 500
                    } else {
                        40 + spread(n) % 160
                    };
                    tokio::time::sleep(Duration::from_millis(wait)).await;
                    for _ in 0..3 {
                        this.db
                            .fetch_all(&Statement::text("SELECT 1 AS deal WHERE 1 = 1"))
                            .await
                            .map_err(AppError::internal)?;
                    }
                    Ok::<_, AppError>("board")
                }
            })
            .public(),
            RouteDef::on(Method::GET, "/deals/{id}", move |Path(id): Path<u64>| {
                let this = deal.clone();
                async move {
                    this.db
                        .fetch_all(&Statement::portable("SELECT ? AS id", [(id as i64).into()]))
                        .await
                        .map_err(AppError::internal)?;
                    tokio::time::sleep(Duration::from_millis(8 + spread(id) % 40)).await;
                    Ok::<_, AppError>("deal")
                }
            })
            .public(),
            RouteDef::on(Method::GET, "/contacts", || async {
                tokio::time::sleep(Duration::from_millis(15)).await;
                "contacts"
            })
            .public(),
            RouteDef::on(Method::GET, "/health/live", || async { "ok" }).public(),
            RouteDef::on(
                Method::POST,
                "/invoices/{id}/charge",
                |Path(id): Path<u64>| async move {
                    Err::<&'static str, AppError>(AppError::internal(InvoiceError::Declined(
                        std::io::Error::other(format!("provider declined card for invoice {id}")),
                    )))
                },
            )
            .public(),
            RouteDef::on(Method::POST, "/companies", || async {
                Err::<&'static str, AppError>(AppError::internal(InvoiceError::Duplicate))
            })
            .public(),
            RouteDef::on(
                Method::GET,
                "/reports/stage/{id}",
                |Path(id): Path<u64>| async move {
                    if id > 0 {
                        panic!("stage probability out of range: {id}");
                    }
                    "never"
                },
            )
            .public(),
            RouteDef::on(Method::GET, "/upstream/rates", || async {
                Err::<&'static str, AppError>(AppError::service_unavailable("rates are down"))
            })
            .public(),
            RouteDef::on(Method::POST, "/contacts/import", move || {
                let this = upstream.clone();
                async move {
                    // A real call to an upstream that answers 503.
                    let _ = this
                        .upstream
                        .get(format!("http://127.0.0.1:{PORT}/api/v1/upstream/rates"))
                        .send()
                        .await;
                    Ok::<_, AppError>("imported")
                }
            })
            .public(),
        ]
    }
}

/// Succeeds.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct SendDigest {
    user: Uuid,
}

impl Job for SendDigest {
    const NAME: &'static str = "digest.send";

    async fn handle(self, _ctx: JobContext) -> Result<(), BoxError> {
        tokio::time::sleep(Duration::from_millis(60)).await;
        Ok(())
    }
}

/// Fails every attempt.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct ExportContacts {
    n: u64,
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
        Err("timed out after 120 s".into())
    }
}

/// A channel that is always down, to show a failed delivery.
struct Pager;

impl Injectable for Pager {
    fn dependencies() -> Vec<ProviderKey> {
        Vec::new()
    }

    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Pager) })
    }
}

impl AlertChannel for Pager {
    fn name(&self) -> &str {
        "pager"
    }

    fn wants(&self, alert: &Alert) -> bool {
        alert.kind.as_str() == "failed_job"
    }

    fn send<'a>(&'a self, _alert: &'a Alert) -> BoxFuture<'a, Result<(), BoxError>> {
        Box::pin(async { Err("pager.example.test: 503 service unavailable".into()) })
    }
}

/// Prints what the dashboard audits.
struct PrintAudit;

impl AuditSink for PrintAudit {
    fn record(&self, entry: AuditEntry) -> BoxFuture<'_, Result<(), BoxError>> {
        println!(
            "audit: {} by {:?} {} -> {}",
            entry.action,
            entry.actor_id,
            entry.old_values.clone().unwrap_or(Value::Null),
            entry.new_values.clone().unwrap_or(Value::Null)
        );
        let _ = AuditKind::Custom;
        Box::pin(async { Ok(()) })
    }
}

struct DemoModule;

impl Module for DemoModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<CrmController>()
            .job::<SendDigest>()
            .job::<ExportContacts>()
            .alert_channel::<Pager>()
            .global()
            .factory(Factory::<dyn AuditSink>::new(|_| async {
                Ok(Arc::new(PrintAudit) as Arc<dyn AuditSink>)
            }))
            .exports::<dyn AuditSink>();
    }
}

fn abilities(ctx: &AbilityCtx, rules: &mut Rules) {
    ctx.grant_permissions(rules);
}

/// Creates a user with `permissions` through a role of their own.
async fn user(
    ctx: &navree_kernel::AppContext,
    email: &str,
    role: &str,
    permissions: &[&str],
) -> Result<(), BoxError> {
    let auth = ctx.get::<AuthService>().await?;
    let user = auth.create_user("Priya Verma", email, PASSWORD).await?;
    if permissions.is_empty() {
        return Ok(());
    }
    let db = ctx.get::<Db>().await?;
    let service = ctx.get::<AccessService>().await?;
    let uow = db.uow(None, None).await;
    if service.role(role).await?.is_none() {
        service.create_role(&uow, role, None).await?;
    }
    for permission in permissions {
        service.grant(&uow, role, permission).await?;
    }
    service.assign_role(&uow, user.id, role).await?;
    uow.commit().await?;
    Ok(())
}

/// Copies the aggregates of the hour recorded so far into the 167 hours
/// before it, scaled by a working-day shape with noise.
async fn back_date(db: &Db) -> Result<usize, BoxError> {
    let now = Utc::now();
    let this_hour: DateTime<Utc> = now
        .with_minute(0)
        .and_then(|t| t.with_second(0))
        .and_then(|t| t.with_nanosecond(0))
        .ok_or("no hour")?;
    let rows = aggregate::Model::query()
        .where_(aggregate::Model::RESOLUTION.eq(3600))
        .where_(aggregate::Model::BUCKET.gte(this_hour))
        .get(db)
        .await?
        .into_models();
    let mut copies = Vec::new();
    for ago in 1..168i64 {
        let bucket = this_hour - TimeDelta::hours(ago);
        let hour = bucket.hour() as f64;
        let weekend = matches!(bucket.format("%a").to_string().as_str(), "Sat" | "Sun");
        // A working day peaks in the afternoon; the night is quiet.
        let shape = 0.18 + 0.82 * (-((hour - 14.0).powi(2)) / 30.0).exp();
        let shape = if weekend { shape * 0.45 } else { shape };
        for row in &rows {
            let noise =
                0.85 + (spread((ago as u64) * 7919 + row.count as u64) % 300) as f64 / 1000.0;
            // The recorded hour is a couple of minutes of traffic; a busy hour
            // is several times that.
            let k = shape * noise * 6.0;
            let scale = |n: i64| ((n as f64) * k).round() as i64;
            let mut copy = row.clone();
            copy.id = Uuid::now_v7();
            copy.bucket = bucket;
            copy.count = scale(row.count);
            copy.errors = scale(row.errors);
            copy.extra = scale(row.extra);
            copy.total_ms = row.total_ms * k;
            if copy.count > 0 {
                copies.push(copy);
            }
        }
    }
    let n = copies.len();
    for chunk in copies.chunks(200) {
        aggregate::Model::query().insert(db, chunk.to_vec()).await?;
    }
    Ok(n)
}

/// The traffic: requests through real HTTP, jobs, now and then a deploy.
async fn traffic(ctx: navree_kernel::AppContext, history: bool) -> Result<(), BoxError> {
    let client = HttpClient::new(HttpClientOptions {
        ssrf: navree_http_client::SsrfPolicy {
            block_private: false,
            ..Default::default()
        },
        ..HttpClientOptions::default()
    })?;
    let base = format!("http://127.0.0.1:{PORT}/api/v1");
    for _ in 0..50 {
        if client
            .get(format!("{base}/health/live"))
            .send()
            .await
            .is_ok()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let queue = ctx.get::<Queue>().await?;
    let lighthouse = ctx.get::<Lighthouse>().await?;
    let db = ctx.get::<Db>().await?;
    let worker_queue = queue.clone();
    tokio::spawn(async move {
        let _ = worker_queue
            .work(WorkOptions {
                queues: vec!["default".into(), "exports".into()],
                ..WorkOptions::default()
            })
            .await;
    });
    // A deploy a while ago, as a deploy would report it.
    lighthouse
        .report_release(
            ReleaseReport::new("3f2a9c1")
                .commit("3f2a9c1")
                .url("https://git.example.com/crm/commit/3f2a9c1")
                .notes("Stage probability is validated before saving")
                .deployed_at(Utc::now() - TimeDelta::minutes(18)),
        )
        .await?;
    let started = std::time::Instant::now();
    let mut backdated = !history;
    let mut n: u64 = 0;
    loop {
        n += 1;
        let r = spread(n);
        let path = match r % 100 {
            0..=34 => format!("GET {base}/deals/board"),
            35..=62 => format!("GET {base}/deals/{}", r % 5000),
            63..=78 => format!("GET {base}/contacts"),
            79..=82 => format!("POST {base}/invoices/{}/charge", r % 900),
            83..=84 => format!("POST {base}/companies"),
            85 => format!("GET {base}/reports/stage/{}", 1 + r % 9),
            86..=88 => format!("POST {base}/contacts/import"),
            _ => format!("GET {base}/contacts"),
        };
        let (method, url) = path.split_once(' ').unwrap_or(("GET", &path));
        let request = if method == "POST" {
            client.post(url)
        } else {
            client.get(url)
        };
        let _ = request.send().await;
        if n.is_multiple_of(40) {
            let _ = queue
                .dispatch(SendDigest {
                    user: Uuid::now_v7(),
                })
                .await;
        }
        if n.is_multiple_of(150) {
            let _ = queue.dispatch(ExportContacts { n }).await;
        }
        if !backdated && started.elapsed() > Duration::from_secs(60) {
            lighthouse.flush().await;
            match back_date(&db).await {
                Ok(rows) => println!("back-dated {rows} aggregate rows over the 167 hours before"),
                Err(error) => println!("could not back-date: {error}"),
            }
            backdated = true;
        }
        tokio::time::sleep(Duration::from_millis(60 + r % 140)).await;
    }
}

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    let history = !std::env::args().any(|arg| arg == "--no-history");
    let dir = tempfile::tempdir()?;
    let url = format!(
        "sqlite://{}?mode=rwc",
        dir.path()
            .join("lighthouse.db")
            .display()
            .to_string()
            .replace('\\', "/")
    );
    let sources = ConfigSources::new()
        .set("app.key", Value::String(navree_crypto::generate_key()))
        .set("app.url", json!(format!("http://127.0.0.1:{PORT}")))
        .set("app.name", json!("crm-api"))
        .set("database.url", Value::String(url))
        .set("database.migrate_on_boot", json!(true))
        .set("http.addr", json!(format!("127.0.0.1:{PORT}")))
        .set("http.per_ip_rate_limit", Value::Null)
        .set("hashing.memory_kib", json!(1024))
        .set("hashing.iterations", json!(1))
        .set("queue.poll_interval_ms", json!(100))
        .set("http_client.ssrf.block_private", json!(false))
        .set(
            "mail",
            json!({ "from": "Lighthouse <ops@demo.test>", "transport": "array" }),
        )
        .set(
            "lighthouse.release",
            json!(if std::env::args().any(|arg| arg == "--no-release") {
                ""
            } else {
                "3f2a9c1"
            }),
        )
        .set("lighthouse.environment", json!("production"))
        .set("lighthouse.thresholds.slow_query_ms", json!(60))
        .set("lighthouse.sample.requests", json!(0.2))
        .set("lighthouse.alerts.cooldown_minutes", json!(5))
        .set("lighthouse.alerts.email.to", json!(["oncall@demo.test"]))
        .set(
            "lighthouse.alerts.webhook",
            json!({
                "url": "http://127.0.0.1:9/hooks/lighthouse",
                "secret": "a-demo-webhook-secret-of-at-least-32-bytes!"
            }),
        );
    let app = Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(CryptoModule::from_config())
        .module(HttpModule::from_config())
        .module(DatabaseModule::from_config())
        .module(HttpClientModule::from_config())
        // The demo serves plain HTTP on this machine: no `Secure` cookie.
        .module(SessionModule::for_root(SessionOptions {
            secure: false,
            ..SessionOptions::default()
        }))
        .module(AuthModule::from_config())
        .module(UiModule)
        .module(AuthPages::new().app_name("crm-api"))
        .module(navree_view::ViewModule::from_config())
        .module(navree_mail::MailModule::from_config())
        .module(QueueModule::from_config())
        .module(AccessModule::for_root(AccessOptions {
            abilities,
            super_admin_role: None,
        }))
        .module(DemoModule)
        .module(LighthouseModule::from_config())
        .boot()
        .await?;
    let ctx = app.ctx().clone();
    user(
        &ctx,
        "ops@demo.test",
        "ops",
        &[
            "lighthouse.view",
            "lighthouse.resolve",
            "lighthouse.configure",
        ],
    )
    .await?;
    user(&ctx, "viewer@demo.test", "viewer", &["lighthouse.view"]).await?;
    user(&ctx, "nobody@demo.test", "nobody", &[]).await?;
    if !std::env::args().any(|arg| arg == "--empty") {
        tokio::spawn(traffic(ctx.clone(), history));
    }
    println!(
        "Lighthouse dashboard: http://127.0.0.1:{PORT}/_lighthouse (ops@demo.test, viewer@demo.test, nobody@demo.test; password {PASSWORD})"
    );
    app.run_command("serve", Vec::new()).await?;
    Ok(())
}
