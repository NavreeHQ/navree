//! `HttpClientModule`: `for_root`, `from_config`, globality and fakes in apps.

use std::sync::Arc;

use http::Method;
use navree_config::{ConfigModule, ConfigSources};
use navree_http_client::{
    FakeResponse, HttpClient, HttpClientModule, HttpClientOptions, SsrfPolicy,
};
use navree_kernel::{Factory, Module, ModuleBuilder, ModuleId, Navree, ProviderKey};
use serde_json::json;

/// A module that needs the client without importing `HttpClientModule`.
struct BillingModule;

/// What `BillingModule` builds from the client.
struct BillingGateway {
    client: Arc<HttpClient>,
}

impl Module for BillingModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.factory(
            Factory::<BillingGateway>::new(|r| async move {
                Ok(Arc::new(BillingGateway {
                    client: r.get::<HttpClient>().await?,
                }))
            })
            .deps(vec![ProviderKey::of::<HttpClient>()]),
        );
    }
}

#[tokio::test]
async fn for_root_exports_a_global_client() {
    let options = HttpClientOptions {
        timeout_secs: 9,
        retries: 0,
        ..HttpClientOptions::default()
    };
    let app = Navree::app()
        .module(ConfigModule::for_root(ConfigSources::new()))
        .module(HttpClientModule::for_root(options.clone()))
        .module(BillingModule)
        .boot()
        .await
        .unwrap();

    let gateway = app
        .ctx()
        .resolver_for(ModuleId::of::<BillingModule>())
        .get::<BillingGateway>()
        .await
        .unwrap();

    assert_eq!(gateway.client.options(), &options);
    let shared = app.ctx().get::<HttpClient>().await.unwrap();
    assert!(
        Arc::ptr_eq(&shared, &gateway.client),
        "one shared singleton"
    );
}

#[tokio::test]
async fn from_config_reads_the_http_client_section() {
    let sources = ConfigSources::new().set(
        "http_client",
        json!({
            "timeout_secs": 5,
            "retries": 0,
            "ssrf": { "allow_hosts": ["api.internal", "10.0.0.0/8"] }
        }),
    );
    let app = Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(HttpClientModule::from_config())
        .boot()
        .await
        .unwrap();

    let client = app.ctx().get::<HttpClient>().await.unwrap();

    assert_eq!(
        client.options(),
        &HttpClientOptions {
            timeout_secs: 5,
            retries: 0,
            ssrf: SsrfPolicy {
                block_private: true,
                allow_hosts: vec!["api.internal".into(), "10.0.0.0/8".into()],
            },
            ..HttpClientOptions::default()
        }
    );
}

#[tokio::test]
async fn plain_module_reads_config_from_the_environment() {
    let sources = ConfigSources::new().env_vars(
        "APP_",
        [
            ("APP_HTTP_CLIENT__TIMEOUT_SECS", "7"),
            ("APP_HTTP_CLIENT__SSRF__BLOCK_PRIVATE", "false"),
            (
                "APP_HTTP_CLIENT__SSRF__ALLOW_HOSTS",
                "a.internal, 10.0.0.0/8",
            ),
        ],
    );
    let app = Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(HttpClientModule)
        .boot()
        .await
        .unwrap();

    let options = app
        .ctx()
        .get::<HttpClient>()
        .await
        .unwrap()
        .options()
        .clone();

    assert_eq!(options.timeout_secs, 7);
    assert!(!options.ssrf.block_private);
    assert_eq!(options.ssrf.allow_hosts, vec!["a.internal", "10.0.0.0/8"]);
    assert_eq!(
        options.connect_timeout_secs, 5,
        "unset keys keep their defaults"
    );
}

#[tokio::test]
async fn invalid_config_fails_the_boot_naming_the_section() {
    let sources = ConfigSources::new().set("http_client", json!({ "timeout_secs": 0 }));

    let error = Navree::app()
        .module(ConfigModule::for_root(sources))
        .module(HttpClientModule::from_config())
        .boot()
        .await
        .unwrap_err();

    let message = error.to_string();
    assert!(message.contains("http_client"), "{message}");
    assert!(message.contains("timeout_secs"), "{message}");
}

#[test]
fn invalid_options_are_rejected_by_new() {
    let error = HttpClient::new(HttpClientOptions {
        connect_timeout_secs: 0,
        ..HttpClientOptions::default()
    })
    .unwrap_err();

    assert!(
        error.to_string().contains("connect_timeout_secs"),
        "{error}"
    );
}

#[tokio::test]
async fn a_fake_client_can_replace_the_real_one_in_an_app() {
    let (fake, server) = HttpClient::fake();
    server.on(
        Method::GET,
        "https://billing.example.com/*",
        FakeResponse::new(200),
    );
    let app = Navree::app()
        .module(ConfigModule::for_root(ConfigSources::new()))
        .module(HttpClientModule::from_config())
        .module(BillingModule)
        .override_value(Arc::new(fake))
        .boot()
        .await
        .unwrap();

    let gateway = app
        .ctx()
        .resolver_for(ModuleId::of::<BillingModule>())
        .get::<BillingGateway>()
        .await
        .unwrap();
    gateway
        .client
        .get("https://billing.example.com/invoices")
        .send()
        .await
        .unwrap();

    server.assert_sent(|r| r.url().path() == "/invoices");
}
