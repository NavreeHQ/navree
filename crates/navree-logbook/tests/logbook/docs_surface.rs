//! Every item `tools/logbook.mdx` names: config keys and environment
//! variables, the routes, the command, the types and methods.

use navree_config::{Config, ConfigSources};
use navree_logbook::{
    Activation, Batch, CaptureOptions, IgnoreOptions, Logbook, LogbookModule, LogbookOptions,
    PRUNE_EVERY, REPLAY_TOKEN_TTL, StoreKind, VIEW_PERMISSION, Watchers,
};
use serde_json::json;

fn typed(sources: ConfigSources) -> LogbookOptions {
    Config::load(&sources)
        .unwrap()
        .typed::<LogbookOptions>()
        .unwrap()
}

#[test]
fn every_documented_key_has_its_default() {
    let options = typed(ConfigSources::new());
    assert_eq!(options.enabled, None);
    assert_eq!(options.path, "/_logbook");
    assert_eq!(options.store, StoreKind::File);
    assert_eq!(options.file, "storage/logbook.db");
    assert_eq!(options.max_entries, 10_000);
    assert_eq!(options.keep_hours, 24);
    assert!(!options.allow_remote);
    assert_eq!(options.log_level, "debug");
    assert_eq!(options.slow_query_ms, 100);
    assert_eq!(options.duplicate_queries, 5);
    assert_eq!(
        options.capture,
        CaptureOptions {
            request_body: true,
            response_body: true,
            max_body_kb: 64,
        }
    );
    assert!(options.replay);
    assert_eq!(
        options.ignore,
        IgnoreOptions {
            routes: vec!["/health/*".into(), "/metrics".into()],
        }
    );
    assert_eq!(options.watchers, Watchers::default());
    assert_eq!(
        options.activation() == Activation::On,
        cfg!(debug_assertions)
    );
}

#[test]
fn every_documented_key_can_be_set() {
    let options = typed(ConfigSources::new().set(
        "logbook",
        json!({
            "enabled": true,
            "path": "/_debug",
            "store": "memory",
            "file": "tmp/logbook.db",
            "max_entries": 500,
            "keep_hours": 2,
            "allow_remote": true,
            "log_level": "info",
            "slow_query_ms": 20,
            "duplicate_queries": 3,
            "capture": { "request_body": false, "response_body": false, "max_body_kb": 8 },
            "replay": false,
            "ignore": { "routes": ["/up"] },
            "watchers": {
                "requests": false, "queries": false, "jobs": false, "mail": false,
                "events": false, "cache": false, "logs": false, "exceptions": false,
                "http_client": false, "notifications": false, "models": false,
                "schedule": false
            }
        }),
    ));
    assert_eq!(options.enabled, Some(true));
    assert_eq!(options.path, "/_debug");
    assert_eq!(options.store, StoreKind::Memory);
    assert_eq!(options.max_entries, 500);
    assert!(!options.capture.request_body);
    assert_eq!(options.ignore.routes, ["/up"]);
    assert!(options.interest().is_empty(), "every watcher is off");
}

#[test]
fn environment_variables_reach_the_section() {
    let options = typed(ConfigSources::new().env_vars(
        "APP_",
        [
            ("APP_LOGBOOK__STORE", "memory"),
            ("APP_LOGBOOK__WATCHERS__CACHE", "false"),
            ("APP_LOGBOOK__IGNORE__ROUTES", "/health/*,/metrics,/up"),
            ("APP_LOGBOOK__ENABLED", "true"),
            ("APP_LOGBOOK__MAX_ENTRIES", "250"),
        ],
    ));
    assert_eq!(options.store, StoreKind::Memory);
    assert!(!options.watchers.cache);
    assert_eq!(options.ignore.routes, ["/health/*", "/metrics", "/up"]);
    assert_eq!(options.enabled, Some(true));
    assert_eq!(options.max_entries, 250);
}

#[test]
fn the_named_constants() {
    assert_eq!(VIEW_PERMISSION, "logbook.view");
    assert_eq!(REPLAY_TOKEN_TTL.as_secs(), 300);
    assert_eq!(PRUNE_EVERY.as_secs(), 300);
}

#[tokio::test]
async fn the_test_api_exists() {
    let logbook = Logbook::in_memory(crate::support::options());
    let last: Option<Batch> = logbook.last_request().await;
    assert!(last.is_none());
    assert!(logbook.last_job().await.is_none());
    assert!(logbook.batch("nope").await.is_none());
    logbook.clear().await.unwrap();
    let _ = LogbookModule::from_config();
    let _ = LogbookModule::for_root(LogbookOptions::default());
    fn batch_api(batch: &Batch) -> usize {
        batch.queries().len()
            + batch.duplicate_queries().len()
            + batch.events().len()
            + batch.jobs().len()
            + batch.mail().len()
            + batch.cache_calls().len()
            + batch.logs().len()
            + batch.entries().len()
    }
    let _ = batch_api;
}

#[tokio::test]
async fn the_documented_routes_and_command_exist() {
    let app = crate::app::app().await;
    let routes = navree_http::route_table(app.ctx()).await.unwrap();
    let logbook: Vec<String> = routes
        .iter()
        .filter(|r| r.full_path.starts_with("/_logbook"))
        .map(|r| format!("{} {}", r.method, r.full_path))
        .collect();
    for expected in [
        "GET /_logbook",
        "GET /_logbook/entries/{id}",
        "GET /_logbook/mail/{id}",
        "GET /_logbook/api/entries",
        "DELETE /_logbook/api/entries",
        "GET /_logbook/api/batches/{id}",
        "POST /_logbook/api/requests/{id}/replay",
        "GET /_logbook/api/requests/{id}/curl",
    ] {
        assert!(
            logbook.contains(&expected.to_owned()),
            "missing {expected}: {logbook:?}"
        );
    }
    assert!(
        app.ctx()
            .commands()
            .iter()
            .any(|command| command.name == "logbook:clear")
    );
}
