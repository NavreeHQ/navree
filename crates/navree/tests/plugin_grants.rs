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
use navree::testing::{TestApp, TestAppBuilder, principal};
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

// A plugin may not hook or scope the framework's rows.

/// The roster plugin: its crate is this module, so the app's modules at the top of the file are
/// in no crate it names.
mod navree_plugin_roster {
    use navree::BoxFuture;
    use navree::orm::ExecContext;
    use navree::plugin::PluginInfo;
    use navree::prelude::*;
    use navree::workspace::Member;

    /// The plugin's info.
    const ROSTER: PluginInfo =
        PluginInfo::new("navree-plugin-roster", module_path!(), "roster", "1.0.0");

    /// The plugin's module: it names the plugin, then registers what its function adds.
    pub struct RosterModule(pub fn(&mut ModuleBuilder));

    impl Module for RosterModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(ROSTER);
            (self.0)(m);
        }
    }

    /// The plugin's listener on the workspace member row: it rewrites the role of every member
    /// it saves (here it only tidies the role's spelling).
    pub fn tidy_role<'a>(
        member: &'a mut Record<Member>,
        _ctx: &'a ExecContext,
    ) -> BoxFuture<'a, navree::orm::Result<()>> {
        member.role = member.role.trim().to_lowercase();
        Box::pin(async { Ok(()) })
    }

    /// The plugin's scope on the workspace member row: it shows every row, the ones the row's
    /// other scopes leave out included.
    pub fn every_member(query: Builder<Member>) -> Builder<Member> {
        query.without_global_scopes()
    }
}

/// The roster plugin's module hooking the workspace member row.
fn hooks_members(m: &mut ModuleBuilder) {
    m.on_model::<Member, _>(ModelEvent::Saving, navree_plugin_roster::tidy_role);
}

/// The roster plugin's module registering a global scope on the workspace member row.
fn scopes_members(m: &mut ModuleBuilder) {
    m.global_scope::<Member>("all-members", navree_plugin_roster::every_member);
}

/// The refusal of the roster plugin's listener on the workspace member row.
fn roster_hooks() -> String {
    let member = std::any::type_name::<Member>();
    format!(
        "plugin `navree-plugin-roster` hooks the model `{member}` in `RosterModule`, a model of another crate; only a module of the app hooks it: remove the plugin or update it to a version that leaves the hook to the app, and to use the plugin's hook, register it from a module of the app"
    )
}

/// The refusal of the roster plugin's scope on the workspace member row.
fn roster_scopes() -> String {
    let member = std::any::type_name::<Member>();
    format!(
        "plugin `navree-plugin-roster` registers the global scope `all-members` on `{member}` in `RosterModule`, a model of another crate; only a module of the app registers a global scope on it: remove the plugin or update it to a version that leaves the scope to the app, and to use the plugin's scope, register it from a module of the app"
    )
}

/// An app with workspaces on a temporary database, and no module of its own.
fn workspace_app() -> TestAppBuilder {
    TestApp::builder()
        .temp_database()
        .config("workspace.routes", false)
        .config("workspace.users_table", "")
        .module(DatabaseModule::from_config())
        .module(WorkspaceModule::from_config())
}

/// The boot error of an app that must not boot.
async fn boot_error(builder: TestAppBuilder) -> String {
    match builder.try_build().await {
        Ok(_) => panic!("the app booted"),
        Err(error) => error.to_string(),
    }
}

/// A module of the app: the roster plugin's listener on the member row, and a scope of its own
/// that changes nothing.
struct MemberRows;

impl Module for MemberRows {
    fn register(&self, m: &mut ModuleBuilder) {
        m.global_scope::<Member>("noop", |q| q)
            .on_model::<Member, _>(ModelEvent::Saving, navree_plugin_roster::tidy_role);
    }
}

#[tokio::test]
async fn a_plugin_may_not_hook_or_scope_the_workspace_member_row() {
    use navree_plugin_roster::RosterModule;

    // The listener: refused with the ORM's module in the app and without it (the member row's
    // hooks are found by their key alone), at the boot and by validate.
    let hooks = roster_hooks();
    let with_orm = workspace_app()
        .module(OrmModule::from_config())
        .module(RosterModule(hooks_members));
    let error = boot_error(with_orm).await;
    assert!(error.contains(&hooks), "expected `{hooks}` in:\n{error}");
    let without_orm = workspace_app().module(RosterModule(hooks_members));
    let error = boot_error(without_orm).await;
    assert!(error.contains(&hooks), "expected `{hooks}` in:\n{error}");
    let validated = Navree::app()
        .module(WorkspaceModule::from_config())
        .module(RosterModule(hooks_members))
        .validate()
        .expect_err("the plugin hooks the member row")
        .to_string();
    assert!(validated.lines().any(|line| line == hooks), "{validated}");

    // The scope: refused before any scope is installed, and the scopes are the process's.
    let scoped = workspace_app().module(RosterModule(scopes_members));
    let error = boot_error(scoped).await;
    let scopes = roster_scopes();
    assert!(error.contains(&scopes), "expected `{scopes}` in:\n{error}");
    assert!(!Member::has_global_scope("all-members"));

    // A module of the app registers the plugin's listener and a scope of its own, beside the
    // plugin: it boots.
    let app = workspace_app()
        .module(RosterModule(|_| {}))
        .module(MemberRows)
        .build()
        .await;
    assert_eq!(app.ctx().plugins().plugins.len(), 1);
    assert!(Member::has_global_scope("noop"));
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_frameworks_audit_module_keeps_the_apps_audit_sink_beside_a_plugin() {
    use navree::contracts::AuditSink;
    use navree_plugin_roster::RosterModule;

    // navree-audit's module acts for no plugin: beside a community plugin it registers the
    // app's audit sink as before. Its route asks for `audit.read`, which needs the access
    // module, so this app, which has none, leaves the route out.
    let app = workspace_app()
        .config("audit.routes", false)
        .module(navree::audit::AuditModule::from_config())
        .module(RosterModule(|_| {}))
        .build()
        .await;
    assert_eq!(app.ctx().plugins().plugins.len(), 1);
    assert!(app.ctx().try_get::<dyn AuditSink>().await.is_some());
    app.shutdown().await.unwrap();
}
