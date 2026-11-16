//! What a plugin's module may not register, through the facade: an app with no plugin registers
//! through every route as before, and a plugin may not hook or scope the framework's rows.

use std::collections::HashMap;
use std::sync::Mutex;

use http::request::Parts;
use navree::BoxFuture;
use navree::access::Policy;
use navree::auth::Strategy;
use navree::contracts::{PermissionCheck, Principal};
use navree::http::{GlobalLayer, LayerSlot};
use navree::kernel::KernelError;
use navree::orm::ExecContext;
use navree::orm::hooks::ModelEvent;
use navree::prelude::*;
use navree::realtime::{ChannelDecision, RealtimeOptions};
use navree::testing::{TestApp, principal};
use navree::ui::Shell;
use navree::workspace::Member;
use serde_json::{Value, json};
use uuid::Uuid;

/// Every event at `WARN` or above logged on this thread while the guard lives, with its target
/// (the tests run on a current-thread runtime, so the boot logs on this thread).
#[derive(Clone, Default)]
struct Warnings(Arc<Mutex<Vec<(String, String)>>>);

impl Warnings {
    fn install(&self) -> tracing::subscriber::DefaultGuard {
        tracing::subscriber::set_default(self.clone())
    }

    fn all(&self) -> Vec<(String, String)> {
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
            let target = event.metadata().target().to_owned();
            self.0.lock().unwrap().push((target, message.0));
        }
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

// An app of today: its own module registers through every route a plugin's module is held to.

/// The app's abilities: a signed-in user reads the reports.
fn abilities(ctx: &AbilityCtx, rules: &mut Rules) {
    ctx.grant_permissions(rules);
    if ctx.user.is_some() {
        rules.can(READ, "reports");
    }
}

/// The app's policy: its staff manage everything.
struct StaffPolicy;

impl Policy for StaffPolicy {
    fn define(ctx: &AbilityCtx, rules: &mut Rules) {
        if ctx.has_role("staff") {
            rules.can(MANAGE, ALL);
        }
    }
}

/// The app's reports, a subject named in lower case.
struct Report;

impl Subject for Report {
    const SUBJECT: &'static str = "reports";
    const PERMISSION_PREFIX: &'static str = "reports";
}

/// The app's deals, scoped to workspaces.
#[derive(Clone, Debug, PartialEq, Eq, navree::orm::Model, serde::Serialize)]
#[model(table = "deals", subject = "Deal", permission_prefix = "deals")]
struct Deal {
    id: Uuid,
    workspace_id: Uuid,
    title: String,
}

impl WorkspaceScoped for Deal {}

/// The app's permission check: it answers no question, so the others decide.
struct Abstains;

impl PermissionCheck for Abstains {
    fn check<'a>(
        &'a self,
        _principal: &'a Principal,
        _permission: &'a str,
        _workspace: Option<Uuid>,
    ) -> BoxFuture<'a, Result<Option<bool>, BoxError>> {
        Box::pin(async { Ok(None) })
    }
}

/// The app's sign-in strategy: a request that names its user in `x-crm-user` is that user's.
struct HeaderStrategy;

impl Injectable for HeaderStrategy {
    fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async { Ok(HeaderStrategy) })
    }
}

impl Strategy for HeaderStrategy {
    fn name(&self) -> &'static str {
        "crm-header"
    }

    fn authenticate<'a>(
        &'a self,
        parts: &'a mut Parts,
    ) -> BoxFuture<'a, Result<Option<Principal>, AppError>> {
        let user = parts
            .headers
            .get("x-crm-user")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<Uuid>().ok());
        Box::pin(async move { Ok(user.map(principal)) })
    }
}

/// The app's own layer, around every route: it adds nothing.
fn crm_layer() -> GlobalLayer {
    GlobalLayer::new(LayerSlot::Context, "crm", |_ctx, router| Ok(router))
}

/// The app's channels `{scope}.{id}`: nobody listens to them here.
fn scoped_channel(
    _principal: &Principal,
    _params: &HashMap<String, String>,
    _ctx: &AppContext,
) -> BoxFuture<'static, Result<ChannelDecision, BoxError>> {
    Box::pin(async { Ok(ChannelDecision::Deny) })
}

/// A listener of the app that changes nothing.
fn unchanged<'a, M: Model>(
    _record: &'a mut Record<M>,
    _ctx: &'a ExecContext,
) -> BoxFuture<'a, navree::orm::Result<()>> {
    Box::pin(async { Ok(()) })
}

/// The app's own module.
struct CrmModule;

impl Module for CrmModule {
    fn register(&self, m: &mut ModuleBuilder) {
        // Access: a policy and a subject of its own.
        m.import(AccessModule::for_policy::<StaffPolicy>());
        m.import(AccessModule::for_subject::<Report>());
        // A configured module, imported by a module of the app rather than the root.
        m.import(RealtimeModule::for_root(RealtimeOptions::default()));
        // Who may do what: a permission check, a sign-in strategy and the app's shell.
        m.multi(Factory::<dyn PermissionCheck>::new(|_| async {
            Ok(Arc::new(Abstains) as Arc<dyn PermissionCheck>)
        }));
        m.strategy::<HeaderStrategy>();
        m.value(Shell::new("Kesar")).exports::<Shell>();
        // An app-wide layer, the panel's staff policy and a channel pattern.
        m.http_layer(crm_layer());
        m.helm_staff_workspaces(|_| async { Ok(false) });
        m.channel("{scope}.{id}", scoped_channel);
        // A subject scoped to workspaces, a global scope and model hooks, the workspace member
        // row's included.
        m.workspace_scoped::<Deal>();
        m.global_scope::<Deal>("open", |q| q);
        m.on_model::<Deal, _>(ModelEvent::Saving, unchanged::<Deal>);
        m.on_model::<Member, _>(ModelEvent::Saving, unchanged::<Member>);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn an_app_with_no_plugin_registers_through_every_route_as_before() {
    let warnings = Warnings::default();
    let _guard = warnings.install();
    let app = TestApp::builder()
        .temp_database()
        .config("app.key", navree::crypto::generate_key())
        .config("http.per_ip_rate_limit", Value::Null)
        .config("workspace.users_table", "")
        .config("workspace.roles.billing.permissions", json!(["deals.read"]))
        .fake_auth()
        .module(CryptoModule::from_config())
        .module(DatabaseModule::from_config())
        .module(OrmModule::from_config())
        .module(EventsModule::for_root())
        .module(ViewModule::from_config())
        .module(UiModule)
        .module(AuthModule::from_config())
        .module(AccessModule::for_root(AccessOptions {
            abilities,
            super_admin_role: Some("super-admin".into()),
        }))
        .module(WorkspaceModule::from_config())
        .module(RealtimeModule)
        .module(HelmModule::from_config())
        .module(CrmModule)
        .build()
        .await;

    // No module acts for a plugin, and nothing was refused or said about one.
    assert!(app.ctx().plugins().plugins.is_empty());
    for (target, message) in &warnings.all() {
        assert_ne!(target, "navree::security", "{message}");
        assert!(!message.contains("plugin `"), "{message}");
        assert!(!message.contains("left out a rule"), "{message}");
    }

    // Meera Sharma's abilities are the app's, and no rule names a plugin.
    let meera = principal(Uuid::now_v7());
    let body = app
        .acting_as(meera)
        .get("/api/me/abilities")
        .send()
        .await
        .assert_status(200)
        .json_value();
    let rules = body.as_array().expect("a rule array");
    for rule in rules {
        let reason = rule["reason"].as_str().unwrap_or_default();
        assert!(!reason.contains("of plugin"), "{rule}");
    }
    let reads_reports = json!({ "action": "read", "subject": "reports" });
    assert!(rules.contains(&reads_reports), "{body}");

    app.shutdown().await.unwrap();
}
