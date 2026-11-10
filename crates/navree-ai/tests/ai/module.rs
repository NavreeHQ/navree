//! `AiModule`: configuration from the `ai` section and the environment,
//! boot failures that name the key, the services it provides, and error
//! responses.

use std::sync::Arc;

use navree_ai::{
    Ai, AiError, AiModule, AiOptions, Conversations, Embeddings, FakeReply, ModelConfig,
    ProviderConfig, VectorStore, VectorStoreKind,
};
use navree_config::ConfigSources;
use navree_http::{AppError, Controller, ModuleBuilderHttpExt, RouteDef};
use navree_kernel::{Injectable, KernelError, Module, ModuleBuilder, Resolver};
use navree_testing::TestApp;
use serde_json::json;

#[tokio::test]
async fn models_and_providers_come_from_configuration_and_the_environment() {
    let sources = ConfigSources::new()
        .set("ai.default", json!("smart"))
        .set(
            "ai.models",
            json!({
                "smart": { "provider": "anthropic", "model": "claude-opus-5-5" },
                "fast": { "provider": "anthropic", "model": "claude-sonnet-5-5", "max_tokens": 1024 },
                "backup": { "provider": "openai", "model": "gpt-5" }
            }),
        )
        .env_vars(
            "APP_",
            [
                ("APP_AI__PROVIDERS__ANTHROPIC__API_KEY", "sk-ant-api03-test"),
                ("APP_AI__MODELS__SMART__FALLBACKS", "fast,backup"),
                ("APP_AI__TIMEOUT_SECS", "30"),
            ],
        );
    let app = TestApp::builder()
        .sources(sources)
        .module(AiModule::from_config())
        .build()
        .await;

    let options = app.get::<AiOptions>().await;
    assert_eq!(options.default, "smart");
    assert_eq!(options.timeout_secs, 30);
    assert_eq!(options.models["smart"].fallbacks, ["fast", "backup"]);
    assert_eq!(options.models["fast"].max_tokens, 1024);
    assert_eq!(
        options.providers["anthropic"]
            .api_key
            .as_ref()
            .map(|key| key.expose().as_str()),
        Some("sk-ant-api03-test")
    );
    assert!(app.get::<Ai>().await.fake().is_none());
}

#[tokio::test]
async fn invalid_configuration_fails_the_boot_naming_the_key() {
    let Err(error) = TestApp::builder()
        .config(
            "ai.models",
            json!({ "smart": { "provider": "anthropic", "model": "x", "fallbacks": ["ghost"] } }),
        )
        .module(AiModule::from_config())
        .try_build()
        .await
    else {
        panic!("the boot fails");
    };
    let message = format!("{error}");
    assert!(message.contains("ai.models.smart.fallbacks"), "{message}");

    let Err(error) = TestApp::builder()
        .config("ai.default", "nope")
        .config(
            "ai.models",
            json!({ "a": { "provider": "openai", "model": "x" } }),
        )
        .module(AiModule::from_config())
        .try_build()
        .await
    else {
        panic!("the boot fails");
    };
    assert!(format!("{error}").contains("ai.default"), "{error}");
}

#[tokio::test]
async fn for_root_takes_options_directly() {
    let app = TestApp::builder()
        .module(AiModule::for_root(AiOptions {
            default: "local".into(),
            providers: [(
                "ollama".to_owned(),
                ProviderConfig::ollama("http://127.0.0.1:11434"),
            )]
            .into(),
            models: [("local".to_owned(), ModelConfig::new("ollama", "llama3.3"))].into(),
            ..AiOptions::default()
        }))
        .build()
        .await;

    assert_eq!(app.get::<AiOptions>().await.default, "local");
    assert!(app.get::<Ai>().await.fake().is_none());
}

#[tokio::test]
async fn ai_fake_installs_the_fake_and_every_service() {
    let app = TestApp::builder()
        .config("ai.fake", true)
        .module(AiModule::from_config())
        .build()
        .await;

    let ai = app.get::<Ai>().await;
    let fake = ai.fake().expect("ai.fake = true installs AiFake");
    fake.respond(FakeReply::text("hello"));
    assert_eq!(ai.text("hi").await.unwrap().text, "hello");
    let _: Arc<Embeddings> = app.get::<Embeddings>().await;
    assert_eq!(
        app.get::<VectorStore>().await.kind(),
        VectorStoreKind::Memory
    );
    let _: Arc<Conversations> = app.get::<Conversations>().await;
}

#[tokio::test]
async fn an_installed_fake_replaces_the_service() {
    let fake = navree_ai::AiFake::new();
    let app = TestApp::builder()
        .module(AiModule::from_config())
        .override_value(Arc::new(fake.ai()))
        .build()
        .await;
    fake.respond(FakeReply::text("from my fake"));

    let answer = app.get::<Ai>().await.text("hi").await.unwrap();

    assert_eq!(answer.text, "from my fake");
}

/// Routes that fail with AI errors.
struct FailingController;

impl Injectable for FailingController {
    fn construct(_: Resolver) -> navree_kernel::BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Self) })
    }
}

impl Controller for FailingController {
    fn prefix(&self) -> &'static str {
        "/ai"
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(http::Method::GET, "/unavailable", || async {
                Err::<(), AppError>(AiError::unavailable("anthropic").into())
            })
            .public(),
            RouteDef::on(http::Method::GET, "/budget", || async {
                Err::<(), AppError>(
                    AiError::BudgetExceeded {
                        retry_after: std::time::Duration::from_secs(120),
                    }
                    .into(),
                )
            })
            .public(),
            RouteDef::on(http::Method::GET, "/config", || async {
                Err::<(), AppError>(AiError::config("no key").into())
            })
            .public(),
        ]
    }
}

struct FailingModule;

impl Module for FailingModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.controller::<FailingController>();
    }
}

#[tokio::test]
async fn ai_errors_returned_with_question_mark_get_their_status() {
    let app = TestApp::builder()
        .config("ai.fake", true)
        .module(AiModule::from_config())
        .module(FailingModule)
        .build()
        .await;

    app.client()
        .get("/api/ai/unavailable")
        .send()
        .await
        .assert_error(503, "The AI provider is unavailable");
    let response = app
        .client()
        .get("/api/ai/budget")
        .send()
        .await
        .assert_status(429);
    assert_eq!(response.header("retry-after"), Some("120"));
    app.client()
        .get("/api/ai/config")
        .send()
        .await
        .assert_error(500, "Internal server error");
}
