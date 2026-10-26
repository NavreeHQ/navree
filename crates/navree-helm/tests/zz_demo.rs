//! TEMPORARY (not committed): serves a seeded panel on 127.0.0.1:8765 for a
//! visual check. Run with `cargo test -p navree-helm --test zz_demo -- --ignored --nocapture`.

#[path = "helm/support.rs"]
mod support;

use std::sync::Arc;

use navree_access::AccessModule;
use navree_auth::{AuthModule, AuthService};
use navree_contracts::{Clock, FixedClock};
use navree_crypto::CryptoModule;
use navree_helm::HelmModule;
use navree_testing::TestApp;
use serde_json::Value;

use support::*;

#[tokio::test(flavor = "multi_thread")]
#[ignore = "manual"]
async fn serve_demo() {
    let clock = Arc::new(FixedClock::new(now()));
    let app = TestApp::builder()
        .temp_database()
        .config("app.key", navree_crypto::generate_key())
        .config("hashing.memory_kib", 1024)
        .config("hashing.iterations", 1)
        .config("http.per_ip_rate_limit", Value::Null)
        .config("helm.title", "Acme CRM")
        .override_value::<dyn Clock>(clock.clone())
        .module(CryptoModule::from_config())
        .module(navree_db::DatabaseModule)
        .module(AuthModule::from_config())
        .module(AccessModule::for_root(options()))
        .module(CrmModule)
        .module(HelmModule::from_config())
        .build()
        .await;
    let auth = app.get::<AuthService>().await;
    let manager = auth
        .create_user("Maya Manager", "maya@example.com", "correct horse battery")
        .await
        .unwrap();
    give_role(&app, manager.id, "manager", &["helm.view", "deals.*", "companies.*"]).await;
    let rep = auth
        .create_user("Priya Patel", "priya@example.com", "correct horse battery")
        .await
        .unwrap();
    give_role(&app, rep.id, "rep", &["helm.view"]).await;

    let globex = company(&app, "Globex", Some("Springfield")).await;
    let initech = company(&app, "Initech", Some("Austin")).await;
    company(&app, "Umbrella <script>alert(1)</script>", None).await;
    let stages = ["lead", "qualified", "won", "lost"];
    let titles = [
        "Globex renewal",
        "Initech upgrade",
        "Expansion seats",
        "Pilot program",
        "Annual contract",
        "Support add-on",
        "Training package",
        "Enterprise tier with a much longer name than usual",
    ];
    for (i, title) in titles.iter().enumerate() {
        deal(
            &app,
            DealSeed {
                title,
                amount_cents: 125_000 * (i as i64 + 1),
                stage: stages[i % 4],
                owner: Some(if i % 2 == 0 { rep.id } else { manager.id }),
                company: Some(if i % 3 == 0 { globex.id } else { initech.id }),
                created_at: now() - chrono::Duration::days(i as i64),
                expected_close_date: chrono::NaiveDate::from_ymd_opt(2026, 11, 1 + i as u32),
                closed: i % 4 >= 2,
            },
        )
        .await;
    }

    let router = navree_http::build_router(app.ctx()).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8765").await.unwrap();
    eprintln!("serving on http://127.0.0.1:8765/admin");
    let stop = std::path::PathBuf::from(std::env::var("DEMO_STOP_FILE").unwrap());
    let server = axum::serve(
        listener,
        router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        while !stop.exists() {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    });
    server.await.unwrap();
}
