//! `SocialModule` as a first-party plugin.

use std::sync::{Arc, Mutex};

use navree_kernel::__private::{acts_for, is_app_module};
use navree_kernel::{
    BoxFuture, Injectable, KernelError, Module, ModuleBuilder, ModuleId, Resolver,
};
use navree_social::{
    AuthorizeUrl, ProviderConfig, ProviderToken, SocialError, SocialModule, SocialProvider,
    SocialUser,
};
use serde_json::json;

use crate::support::{builder, redirect};

/// Every warning or error logged on this thread while the guard lives (the
/// test runs on a current-thread runtime, so the boot logs on this thread).
#[derive(Clone, Default)]
struct Warnings(Arc<Mutex<Vec<String>>>);

impl Warnings {
    fn install(&self) -> tracing::subscriber::DefaultGuard {
        tracing::subscriber::set_default(self.clone())
    }

    fn all(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

struct Message(String);

impl tracing::field::Visit for Message {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}");
        }
    }
}

impl tracing::Subscriber for Warnings {
    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        *metadata.level() <= tracing::Level::WARN
    }

    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        if *event.metadata().level() <= tracing::Level::WARN {
            let mut message = Message(String::new());
            event.record(&mut message);
            self.0.lock().unwrap().push(message.0);
        }
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

#[tokio::test]
async fn social_names_itself_a_first_party_plugin_with_the_namespace_social() {
    let app = builder().build().await;
    let report = app.ctx().plugins();
    assert!(
        report.plugins.iter().all(|plugin| plugin.first_party),
        "{report:?}"
    );
    let matching: Vec<_> = report
        .plugins
        .iter()
        .filter(|plugin| plugin.info.name == "navree-social")
        .collect();
    let [plugin] = matching.as_slice() else {
        panic!("expected one navree-social plugin: {report:?}")
    };
    assert_eq!(plugin.info.namespace, "social");
    assert_eq!(plugin.info.version, env!("CARGO_PKG_VERSION"));
    assert!(plugin.first_party);

    let social_module = plugin
        .modules
        .iter()
        .find(|module| module.id.type_name() == "navree_social::module::SocialModule")
        .expect("SocialModule is listed under navree-social");
    assert!(social_module.commands.is_empty());
    assert!(social_module.publishable.is_empty());

    // The documented redirect route answers as before.
    let body = redirect(&app, "github", "", None)
        .await
        .assert_status(200)
        .json_value();
    assert!(body["url"].is_string());
    assert!(body["state"].is_string());
}

/// A custom identity provider registered by an app module.
struct Provider;

impl Injectable for Provider {
    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(Provider) })
    }
}

impl SocialProvider for Provider {
    fn key(&self) -> &'static str {
        "zoho"
    }

    fn authorize_url(&self, _config: &ProviderConfig, url: AuthorizeUrl) -> AuthorizeUrl {
        url.endpoint("https://accounts.zoho.in/oauth/v2/auth")
            .scopes(["AaaServer.profile.Read"])
    }

    fn user<'a>(
        &'a self,
        _config: &'a ProviderConfig,
        token: ProviderToken,
    ) -> BoxFuture<'a, Result<SocialUser, SocialError>> {
        Box::pin(async move {
            Ok(SocialUser::new("zoho", "harpreet-1")
                .name("Harpreet Sandhu")
                .email(Some("harpreet@ludhiana.example"), true)
                .token(token))
        })
    }
}

/// An app module that imports a custom provider registration.
struct Accounts;

impl Module for Accounts {
    fn register(&self, m: &mut ModuleBuilder) {
        m.import(SocialModule::for_feature::<Provider>());
    }
}

#[tokio::test]
async fn a_module_that_imports_for_feature_stays_the_apps_own() {
    let app = builder()
        .config(
            "social.providers.zoho",
            json!({
                "type": "custom",
                "name": "Zoho",
                "client_id": "1000.ABC",
                "token_url": "https://accounts.zoho.in/oauth/v2/token",
            }),
        )
        .module(Accounts)
        .build()
        .await;
    let ctx = app.ctx();
    let accounts = ModuleId::of::<Accounts>();
    let feature = SocialModule::for_feature::<Provider>().id();

    assert!(is_app_module(ctx, accounts));
    assert_eq!(
        acts_for(ctx, feature).map(|info| info.name),
        Some("navree-social")
    );
    assert!(acts_for(ctx, accounts).is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn the_social_module_names_itself_and_boots_with_no_warning() {
    let warnings = Warnings::default();
    let _guard = warnings.install();
    let _app = builder().build().await;
    let logged = warnings.all();
    assert!(logged.is_empty(), "{logged:?}");
}
