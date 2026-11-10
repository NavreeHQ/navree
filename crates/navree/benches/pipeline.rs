//! Micro-benchmarks of what Navree adds to every request.
//!
//! - `pipeline/json`: `GET /json` through the whole router: every global
//!   layer, the unit-of-work layer and the per-route pipeline, one request
//!   at a time. `bare/json` is the same handler on a bare `routing::Router`,
//!   the floor the pipeline is measured against.
//! - `pipeline/json_x64`: 64 `/json` requests at once on four worker
//!   threads, where process-wide locks show up.
//! - `pipeline/db`: `GET /db`, one `SELECT` by primary key through the pool
//!   (SQLite).
//! - `pipeline/insert`: `POST /deals`, a JSON body deserialized and
//!   validated by `Valid<Json<T>>`, then inserted with `Model::create` in
//!   the request's unit of work, which commits.
//! - `uow/begin_commit`: a unit of work that begins a SQLite transaction
//!   (with its first statement, `SELECT 1`) and commits it, outside HTTP.
//! - `kernel/get`: `AppContext::get::<Db>()`, the service locator.
//!
//! Run them with `cargo bench -p navree --features sqlite --bench pipeline` (add
//! `-- --quick` for a shorter run, or `-- --test` to run each once).
#![allow(missing_docs)]

use std::sync::atomic::{AtomicI32, Ordering};
use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};
use http::{Request, StatusCode};
use http_body_util::BodyExt;
use navree::App;
use navree::db::{Db, DbOptions, Executor as _, Statement};
use navree::http::body::Body;
use navree::http::{HttpOptions, build_router};
use navree::prelude::*;
use navree::validation::Validate;
use serde::{Deserialize, Serialize};
use tokio::runtime::Runtime;
use tower::ServiceExt;
use uuid::Uuid;

/// `world (id, randomnumber)`: the rows `/db` reads.
#[derive(Model, Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[model(table = "world")]
pub struct World {
    pub id: i32,
    #[model(column = "randomnumber")]
    #[serde(rename = "randomNumber")]
    pub random_number: i32,
}

/// `deals (id, title, amount_cents)`: the rows `/deals` inserts.
#[derive(Model, Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[model(table = "deals")]
pub struct Deal {
    pub id: Uuid,
    pub title: String,
    pub amount_cents: i64,
}

/// Rows in `world`.
const WORLD_ROWS: i32 = 1000;

/// The next `/db` id, cycling through `world`.
static NEXT_ID: AtomicI32 = AtomicI32::new(0);

#[derive(Serialize)]
pub struct Message {
    message: &'static str,
}

#[derive(Deserialize, Validate)]
pub struct CreateDeal {
    #[validate(length(min = 1, max = 200))]
    title: String,
    #[validate(range(min = 0, max = 1_000_000_000_000))]
    amount_cents: i64,
}

#[injectable]
pub struct BenchController {
    db: Arc<Db>,
}

#[controller("!/")]
impl BenchController {
    #[get("/json")]
    #[public]
    async fn json(&self) -> Json<Message> {
        Json(Message {
            message: "Hello, World!",
        })
    }

    #[get("/db")]
    #[public]
    async fn db(&self) -> Result<Json<Record<World>>> {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed) % WORLD_ROWS + 1;
        let row = World::find(&*self.db, id)
            .await?
            .ok_or_else(AppError::not_found)?;
        Ok(Json(row))
    }

    #[post("/deals")]
    #[public]
    async fn store(
        &self,
        uow: Uow,
        Valid(Json(input)): Valid<Json<CreateDeal>>,
    ) -> Result<Created<Record<Deal>>> {
        let deal = Deal::create(
            &uow,
            Deal {
                id: Uuid::now_v7(),
                title: input.title,
                amount_cents: input.amount_cents,
            },
        )
        .await?;
        Ok(Created(format!("/deals/{}", deal.id), deal))
    }
}

#[module(controllers = [BenchController])]
pub struct BenchModule;

/// The booted app and its router; the temporary directory holds the
/// SQLite file and the (absent) maintenance file.
struct Bench {
    app: App,
    router: navree::http::AppRouter,
    _dir: tempfile::TempDir,
}

async fn boot() -> Bench {
    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .expect("temp dir");
    let url = format!(
        "sqlite://{}?mode=rwc",
        dir.path().join("bench.db").display()
    );
    let http = HttpOptions {
        // The per-IP limit stays in the pipeline with a budget out of reach,
        // as in a load-test app.
        per_ip_rate_limit: Some(1_000_000_000),
        maintenance_file: dir.path().join("down"),
        ..HttpOptions::default()
    };
    let app = Navree::app()
        .module(HttpModule::for_root(http))
        .module(DatabaseModule::for_root(DbOptions {
            url,
            max_connections: 8,
            ..DbOptions::default()
        }))
        .module(ValidationModule::for_root())
        .module(BenchModule)
        .boot()
        .await
        .expect("the bench app boots");
    let db = app.ctx().get::<Db>().await.expect("Db");
    for sql in [
        "CREATE TABLE world (id INTEGER PRIMARY KEY, randomnumber INTEGER NOT NULL)".to_owned(),
        "CREATE TABLE deals (id BLOB PRIMARY KEY, title TEXT NOT NULL, amount_cents INTEGER NOT NULL)"
            .to_owned(),
        format!(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < {WORLD_ROWS}) \
             INSERT INTO world SELECT i, ((i * 7919) % 10000) + 1 FROM n"
        ),
    ] {
        db.pool().execute_script(&sql).await.expect("schema");
    }
    let router = build_router(app.ctx()).await.expect("router builds");
    Bench {
        app,
        router,
        _dir: dir,
    }
}

/// Sends `request` and reads the whole body; panics unless `expected`.
async fn send(router: navree::http::AppRouter, request: Request<Body>, expected: StatusCode) {
    let response = router.oneshot(request).await.expect("infallible");
    let status = response.status();
    let body = response.into_body().collect().await.expect("body");
    assert_eq!(status, expected, "{:?}", body.to_bytes());
}

fn get(path: &str) -> Request<Body> {
    Request::get(path).body(Body::empty()).expect("request")
}

fn post_deal() -> Request<Body> {
    Request::post("/deals")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"title":"Benchmark deal","amount_cents":4729000}"#,
        ))
        .expect("request")
}

fn runtime() -> Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("runtime")
}

fn pipeline(c: &mut Criterion) {
    let rt = runtime();
    let bench = rt.block_on(boot());
    let router = bench.router.clone();
    let bare = navree::http::AppRouter::new(navree::http::routing::Router::new().route(
        http::Method::GET,
        "/json",
        || async {
            navree::http::Json(Message {
                message: "Hello, World!",
            })
        },
    ));

    let mut group = c.benchmark_group("pipeline");
    group.measurement_time(Duration::from_secs(5));
    group.bench_function("json", |b| {
        b.to_async(&rt)
            .iter(|| send(router.clone(), get("/json"), StatusCode::OK));
    });
    group.bench_function("json_x64", |b| {
        b.to_async(&rt).iter(|| {
            let router = router.clone();
            async move {
                let mut requests = tokio::task::JoinSet::new();
                for _ in 0..64 {
                    requests.spawn(send(router.clone(), get("/json"), StatusCode::OK));
                }
                while let Some(done) = requests.join_next().await {
                    done.expect("request task");
                }
            }
        });
    });
    group.bench_function("db", |b| {
        b.to_async(&rt)
            .iter(|| send(router.clone(), get("/db"), StatusCode::OK));
    });
    group.bench_function("insert", |b| {
        b.to_async(&rt)
            .iter(|| send(router.clone(), post_deal(), StatusCode::CREATED));
    });
    group.finish();

    let mut group = c.benchmark_group("bare");
    group.measurement_time(Duration::from_secs(5));
    group.bench_function("json", |b| {
        b.to_async(&rt)
            .iter(|| send(bare.clone(), get("/json"), StatusCode::OK));
    });
    group.finish();

    let ctx = bench.app.ctx().clone();
    let db = rt.block_on(ctx.get::<Db>()).expect("Db");
    let mut group = c.benchmark_group("uow");
    group.bench_function("begin_commit", |b| {
        b.to_async(&rt).iter(|| {
            let db = db.clone();
            async move {
                let uow = db.uow(None, Some("bench".to_owned())).await;
                uow.fetch_all(&Statement::text("SELECT 1"))
                    .await
                    .expect("begin");
                uow.commit().await.expect("commit");
            }
        });
    });
    group.finish();

    let mut group = c.benchmark_group("kernel");
    group.bench_function("get", |b| {
        b.to_async(&rt)
            .iter(|| async { ctx.get::<Db>().await.expect("Db") });
    });
    group.finish();

    rt.block_on(bench.app.shutdown()).expect("shutdown");
}

criterion_group!(benches, pipeline);
criterion_main!(benches);
