//! `TracingOptions`: the `observe.tracing` section, its defaults, the
//! `OTEL_*` fallbacks and validation.

use std::collections::BTreeMap;

use navree_config::{Config, ConfigSources};
use navree_observe::{OtlpProtocol, TracingOptions};
use serde_json::json;

fn config(sources: ConfigSources) -> Config {
    Config::load(&sources).expect("valid config")
}

fn no_env(_: &str) -> Option<String> {
    None
}

#[test]
fn defaults_record_spans_but_export_nothing() {
    let options = TracingOptions::from_config_with_env(&config(ConfigSources::new()), no_env)
        .expect("valid");
    assert!(options.enabled);
    assert_eq!(options.endpoint, None);
    assert_eq!(options.protocol, OtlpProtocol::Grpc);
    assert_eq!(options.sample_ratio, 1.0);
    assert!(options.trust_incoming);
    assert!(!options.stdout);
    assert_eq!(options.timeout_ms, 10_000);
    assert_eq!(options.max_queue_size, 2048);
    assert!(!options.exports());
}

#[test]
fn every_key_is_read_from_the_section() {
    let sources = ConfigSources::new().set(
        "observe.tracing",
        json!({
            "endpoint": "http://collector:4318",
            "protocol": "http",
            "headers": { "x-api-key": "k-1" },
            "service_name": "crm-api",
            "resource": { "deployment.environment": "production" },
            "sample_ratio": 0.2,
            "trust_incoming": false,
            "stdout": true,
            "timeout_ms": 5000,
            "max_queue_size": 64,
            "trace_url": "https://grafana.example.com/explore?traceId={trace_id}"
        }),
    );
    let options = TracingOptions::from_config_with_env(&config(sources), no_env).unwrap();
    assert_eq!(options.endpoint.as_deref(), Some("http://collector:4318"));
    assert_eq!(options.protocol, OtlpProtocol::Http);
    assert_eq!(
        options.traces_endpoint().as_deref(),
        Some("http://collector:4318/v1/traces"),
        "OTLP/HTTP posts to /v1/traces"
    );
    assert_eq!(options.headers["x-api-key"], "k-1");
    assert_eq!(options.service_name.as_deref(), Some("crm-api"));
    assert_eq!(options.resource["deployment.environment"], "production");
    assert_eq!(options.sample_ratio, 0.2);
    assert!(!options.trust_incoming);
    assert!(options.stdout);
    assert_eq!(options.timeout_ms, 5000);
    assert_eq!(options.max_queue_size, 64);
    assert!(options.exports());
    let id = "4bf92f3577b34da6a3ce929d0e0e4736".parse().unwrap();
    assert_eq!(
        options.trace_link(&id).as_deref(),
        Some("https://grafana.example.com/explore?traceId=4bf92f3577b34da6a3ce929d0e0e4736")
    );
    assert!(
        !format!("{options:?}").contains("k-1"),
        "header values stay out of Debug"
    );
}

#[test]
fn otel_variables_fill_what_the_section_leaves_out() {
    let env = |name: &str| {
        let value = match name {
            "OTEL_EXPORTER_OTLP_ENDPOINT" => "http://otel:4317",
            "OTEL_EXPORTER_OTLP_PROTOCOL" => "http/protobuf",
            "OTEL_EXPORTER_OTLP_HEADERS" => "x-api-key=abc,x-team=crm",
            "OTEL_SERVICE_NAME" => "from-env",
            "OTEL_TRACES_SAMPLER_ARG" => "0.5",
            "OTEL_RESOURCE_ATTRIBUTES" => "deployment.environment=staging,region=eu",
            _ => return None,
        };
        Some(value.to_owned())
    };
    let options = TracingOptions::from_config_with_env(&config(ConfigSources::new()), env).unwrap();
    assert_eq!(options.endpoint.as_deref(), Some("http://otel:4317"));
    assert_eq!(options.protocol, OtlpProtocol::Http);
    assert_eq!(
        options.headers,
        BTreeMap::from([
            ("x-api-key".to_owned(), "abc".to_owned()),
            ("x-team".to_owned(), "crm".to_owned())
        ])
    );
    assert_eq!(options.service_name.as_deref(), Some("from-env"));
    assert_eq!(options.sample_ratio, 0.5);
    assert_eq!(options.resource["region"], "eu");

    let sources = ConfigSources::new()
        .set("observe.tracing.endpoint", "http://section:4317")
        .set("observe.tracing.service_name", "from-section");
    let options = TracingOptions::from_config_with_env(&config(sources), env).unwrap();
    assert_eq!(options.endpoint.as_deref(), Some("http://section:4317"), "the section wins");
    assert_eq!(options.service_name.as_deref(), Some("from-section"));
}

#[test]
fn the_service_name_falls_back_to_the_app_name() {
    let sources = ConfigSources::new().set("app.name", "crm");
    let options = TracingOptions::from_config_with_env(&config(sources), no_env).unwrap();
    assert_eq!(options.service_name.as_deref(), Some("crm"));
    let sources = ConfigSources::new()
        .set("app.name", "crm")
        .set("micro.service", "deals");
    let options = TracingOptions::from_config_with_env(&config(sources), no_env).unwrap();
    assert_eq!(options.service_name.as_deref(), Some("deals"));
}

#[test]
fn invalid_values_name_their_key() {
    for (key, value, expected) in [
        ("observe.tracing.sample_ratio", json!(2.0), "observe.tracing.sample_ratio"),
        ("observe.tracing.protocol", json!("carrier-pigeon"), "observe.tracing"),
        ("observe.tracing.timeout_ms", json!(0), "observe.tracing.timeout_ms"),
        ("observe.tracing.max_queue_size", json!(0), "observe.tracing.max_queue_size"),
        ("observe.tracing.endpoint", json!("collector:4317"), "observe.tracing.endpoint"),
    ] {
        let sources = ConfigSources::new().set(key, value);
        let error = TracingOptions::from_config_with_env(&config(sources), no_env).unwrap_err();
        assert!(error.contains(expected), "{key}: {error}");
    }
    let env = |name: &str| (name == "OTEL_EXPORTER_OTLP_PROTOCOL").then(|| "http/json".to_owned());
    let error = TracingOptions::from_config_with_env(&config(ConfigSources::new()), env).unwrap_err();
    assert!(error.contains("OTEL_EXPORTER_OTLP_PROTOCOL"), "{error}");
}

#[cfg(not(feature = "otel"))]
#[tokio::test]
async fn an_endpoint_without_the_otel_feature_fails_the_boot() {
    use navree_config::ConfigModule;
    use navree_http::HttpModule;
    use navree_kernel::Navree;
    use navree_observe::ObserveModule;

    let result = Navree::app()
        .module(ConfigModule::for_root(
            ConfigSources::new()
                .set("observe.metrics", false)
                .set("observe.tracing.endpoint", "http://127.0.0.1:4317"),
        ))
        .module(HttpModule::from_config())
        .module(ObserveModule::from_config())
        .boot()
        .await;
    let error = result.expect_err("no exporter compiled in").to_string();
    assert!(error.contains("otel"), "{error}");
}
