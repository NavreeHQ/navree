//! Plugins through the facade, as an app writes them: the examples of the Plugins section's
//! pages (`docs/plugins/*`), and what a plugin adds to an app (links, the admin panel,
//! permissions) with workspaces and access.
//!
//! `existing-crates` ("Connecting an existing crate"): a client crate as a
//! `ClientModule` of a plugin, a tower layer whose service can fail that the
//! plugin ships and the app adds, and a service mounted under `/khata` behind
//! the app's sign-in. The page's tonic example is the gRPC page's, proved in
//! `tests/grpc.rs`.

// The pages' examples are written as an app writes them, without doc comments.
#![allow(missing_docs)]

use std::any::TypeId;
use std::marker::PhantomData;
use std::sync::Mutex;

use http::Method;
use navree::config::ConfigSection;
use navree::contracts::{PermissionCheck, permitted};
use navree::crypto::generate_key;
use navree::db::Db;
use navree::http::{Controller, GlobalLayer, RouteDef};
use navree::kernel::ModuleId;
use navree::plugin::PluginInfo;
use navree::prelude::*;
use navree::testing::{TestApp, principal};
use navree::ui::{AppShell, Shell, Text};
use navree::workspace::{Members, NewWorkspace, WorkspaceRole, Workspaces};
use serde_json::{Value, json};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// existing-crates: a ledger plugin that connects an existing crate
// ---------------------------------------------------------------------------

/// The ledger plugin of the page, a community plugin whose crate is this module. A test
/// binary is one crate, so the plugin names this module as its crate where the page's plugin
/// writes `navree::plugin_info!(namespace = "khata")` in a crate of its own; the app's
/// `AppModule` below is in no crate the plugin names.
mod khata {
    use std::time::Duration;

    use navree::config::ConfigSection;
    use navree::http::extract::Request;
    use navree::http::{AppRouter, FallibleLayer, GlobalLayer, LayerSlot, Mount};
    use navree::plugin::prelude::*;
    use navree::plugin::{ClientModule, PluginInfo};
    use navree::prelude::*;
    use serde::Deserialize;

    /// The plugin's info: in the page, `navree::plugin_info!(namespace = "khata")` in the
    /// plugin's own crate.
    const KHATA: PluginInfo =
        PluginInfo::new("navree-plugin-khata", module_path!(), "khata", "1.0.0");

    /// The `khata` section.
    #[derive(Clone, Deserialize, ConfigSection)]
    #[config(key = "khata")]
    pub struct KhataOptions {
        /// Where the ledger service answers.
        pub base_url: String,
    }

    /// The client the ledger crate gives.
    pub struct KhataClient {
        pub base_url: String,
    }

    /// The ledger crate's client as a module of the plugin.
    pub fn khata_client() -> ClientModule<KhataClient, KhataOptions> {
        ClientModule::<KhataClient>::new("khata")
            .options::<KhataOptions>()
            .connect(|options, _resolver| async move {
                Ok(KhataClient {
                    base_url: options.base_url.clone(),
                })
            })
            .health(|client| async move {
                if client.base_url.starts_with("https://") {
                    Ok(())
                } else {
                    Err("the khata service is not on https".into())
                }
            })
            .plugin(KHATA)
    }

    /// The ledger's entries: the service mounted under `/khata`.
    async fn entries(request: Request) -> Result<String> {
        let ctx = request
            .extensions()
            .get::<AppContext>()
            .cloned()
            .ok_or_else(|| AppError::internal("the request has no app context"))?;
        let khata = ctx.get::<KhataClient>().await.map_err(AppError::internal)?;
        Ok(format!("{} {}", khata.base_url, request.uri().path()))
    }

    /// The ledger's timeout for every request, which the app adds.
    pub fn khata_timeout() -> GlobalLayer {
        GlobalLayer::new(LayerSlot::Context, "khata-timeout", |_ctx, router| {
            Ok(
                router.layer(FallibleLayer::new(tower::timeout::TimeoutLayer::new(
                    Duration::from_secs(10),
                ))),
            )
        })
    }

    /// The plugin's own module: the ledger under `/khata` for signed-in users.
    pub struct LedgerModule;

    impl Module for LedgerModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(KHATA);
            m.mount(
                Mount::new("/khata", AppRouter::from_handler(entries))
                    .authenticated()
                    .throttle(60, Duration::from_secs(60)),
            );
        }
    }
}

/// A module of the app: it adds the ledger's timeout, and maps its error.
pub struct AppModule;

impl Module for AppModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.http_layer(khata::khata_timeout())
            .map_error::<tower::timeout::error::Elapsed>(|_| {
                Some(AppError::service_unavailable("The khata took too long."))
            });
    }
}

#[tokio::test]
async fn the_existing_crates_page_examples_mount_inside_the_pipeline() {
    let app = TestApp::builder()
        .config("khata.base_url", "https://khata.example/ludhiana")
        .module(khata::khata_client())
        .module(khata::LedgerModule)
        .module(AppModule)
        .build()
        .await;
    app.client()
        .get("/khata/entries")
        .send()
        .await
        .assert_error(401, "Unauthorized");
    let harpreet = principal(Uuid::now_v7());
    let response = app
        .acting_as(harpreet)
        .get("/khata/entries")
        .send()
        .await
        .assert_status(200);
    assert_eq!(response.text(), "https://khata.example/ludhiana /entries");

    let report = app.ctx().plugins();
    let plugin = report
        .plugins
        .iter()
        .find(|plugin| plugin.info.namespace == "khata")
        .expect("the ledger plugin");
    assert!(!plugin.first_party);
    let modules: Vec<&str> = plugin
        .modules
        .iter()
        .map(|module| module.id.name())
        .collect();
    assert!(
        modules.contains(&"khata") && modules.contains(&"LedgerModule"),
        "{modules:?}"
    );
    assert!(
        !modules.contains(&"AppModule"),
        "the app's module is not the plugin's: {modules:?}"
    );
    // The plugin's module contributes no app-wide layer: the app adds it.
    let ledger = plugin
        .modules
        .iter()
        .find(|module| module.id.name() == "LedgerModule")
        .expect("the plugin's own module");
    assert!(
        ledger
            .contributions
            .iter()
            .all(|(name, _)| *name != std::any::type_name::<GlobalLayer>()),
        "{:?}",
        ledger.contributions
    );
    let layers: Vec<&str> = app
        .ctx()
        .contributions::<GlobalLayer>()
        .iter()
        .map(|layer| layer.module.name)
        .collect();
    assert!(
        layers.contains(&"AppModule") && !layers.contains(&"LedgerModule"),
        "the app's module adds the layer: {layers:?}"
    );

    app.shutdown().await.unwrap();
}

/// The warnings logged on this thread while the guard lives (the tests run on a
/// current-thread runtime, so the boot logs on this thread).
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
        if *event.metadata().level() == tracing::Level::WARN {
            let mut message = Message(String::new());
            event.record(&mut message);
            self.0.lock().unwrap().push(message.0);
        }
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

/// A module with the id `.0` that names the plugin `.1`, then registers what `.2` adds.
struct Probe(ModuleId, PluginInfo, fn(&mut ModuleBuilder));

impl Module for Probe {
    fn id(&self) -> ModuleId {
        self.0
    }

    fn register(&self, m: &mut ModuleBuilder) {
        m.plugin(self.1.clone());
        (self.2)(m);
    }
}

// Links, the admin panel and permissions.

/// The announcements plugin, a community plugin: its crate is this module, so the app's modules
/// at the top of the file are in no crate it names.
mod navree_plugin_announcements {
    use navree::plugin::prelude::*;
    use navree::plugin::{Permission, PluginInfo};
    use navree::prelude::*;
    use navree::ui::ShellNavItem;

    /// The plugin's info.
    const ANNOUNCEMENTS: PluginInfo = PluginInfo::new(
        "navree-plugin-announcements",
        module_path!(),
        "announcements",
        "1.0.0",
    )
    .title("Announcements");

    /// The announcements plugin's module: it describes `announcements.read`, adds a link that
    /// needs it and a notice board that needs nothing, and serves `GET /notices-home`.
    pub struct Announcements;

    impl Module for Announcements {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(ANNOUNCEMENTS)
                .permissions(&[Permission::new(
                    "announcements.read",
                    "Read the workspace's announcements",
                )])
                .nav_item(
                    ShellNavItem::new("Announcements", "/announcements")
                        .permission("announcements.read"),
                )
                .nav_item(ShellNavItem::new("Notice board", "/notice-board"));
            m.controller::<super::NoticesHome>();
        }
    }
}

/// A module of the app: the app's shell, which takes the links modules add, for every module.
struct Desk;

impl Module for Desk {
    fn register(&self, m: &mut ModuleBuilder) {
        m.global();
        m.value(Shell::new("Kesar").contributed())
            .exports::<Shell>();
    }
}

/// `GET /notices-home`: a page drawn from the `AppShell` extractor that says whether its
/// unit of work entered a workspace. `GET /notices-entered` is the same page after the
/// `CurrentWorkspace` extractor, which enters the workspace.
struct NoticesHome;

impl Injectable for NoticesHome {
    fn construct(
        _r: Resolver,
    ) -> navree::BoxFuture<'static, Result<Self, navree::kernel::KernelError>> {
        Box::pin(async { Ok(NoticesHome) })
    }
}

impl Controller for NoticesHome {
    fn prefix(&self) -> &'static str {
        ""
    }

    fn routes(self: Arc<Self>) -> Vec<RouteDef> {
        vec![
            RouteDef::on(
                Method::GET,
                "!/notices-home",
                |shell: AppShell, uow: Uow| async move {
                    shell
                        .page("Notices")
                        .body(Text::new(format!("entered: {:?}", uow.workspace())))
                },
            )
            .page()
            .authenticated(),
            RouteDef::on(
                Method::GET,
                "!/notices-entered",
                |_: CurrentWorkspace, shell: AppShell, uow: Uow| async move {
                    shell
                        .page("Notices")
                        .body(Text::new(format!("entered: {:?}", uow.workspace())))
                },
            )
            .page()
            .authenticated(),
        ]
    }
}

/// The app: workspaces named by the `X-Workspace` header, access, the toolkit, the app's shell
/// and the announcements plugin; with `member_grant`, the app's configuration grants
/// `announcements.read` to a workspace's members.
async fn app(member_grant: bool) -> TestApp {
    let mut builder = TestApp::builder()
        .temp_database()
        .config("app.key", generate_key())
        .config("http.per_ip_rate_limit", Value::Null)
        .config("workspace.users_table", "")
        .config("workspace.resolvers", json!(["header"]));
    if member_grant {
        builder = builder.config(
            "workspace.roles",
            json!({ "member": { "permissions": ["announcements.read"] } }),
        );
    }
    builder
        .module(CryptoModule)
        .module(DatabaseModule::from_config())
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(WorkspaceModule::from_config())
        .module(UiModule)
        .module(Desk)
        .module(navree_plugin_announcements::Announcements)
        .build()
        .await
}

/// Harpreet Sandhu owns the workspace "Ludhiana office"; Meera Sharma is its member and
/// Simran Kaur its viewer; Kuldeep Gill is in no workspace.
struct People {
    harpreet: Uuid,
    meera: Uuid,
    simran: Uuid,
    kuldeep: Uuid,
    office: Uuid,
}

async fn people(app: &TestApp) -> People {
    let (harpreet, meera, simran, kuldeep) = (
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
    );
    let db = app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    let office = app
        .get::<Workspaces>()
        .await
        .create(&uow, harpreet, NewWorkspace::named("Ludhiana office"))
        .await
        .unwrap();
    let members = app.get::<Members>().await;
    members
        .add(&uow, office.id, meera, WorkspaceRole::Member)
        .await
        .unwrap();
    members
        .add(&uow, office.id, simran, WorkspaceRole::Viewer)
        .await
        .unwrap();
    uow.commit().await.unwrap();
    People {
        harpreet,
        meera,
        simran,
        kuldeep,
        office: office.id,
    }
}

/// `GET /notices-home` as `user`, naming `workspace` in `X-Workspace` when there is one.
async fn notices_home(app: &TestApp, user: Uuid, workspace: Option<Uuid>) -> String {
    notices_page(app, "/notices-home", user, workspace).await
}

/// The page at `path` as `user`, naming `workspace` in `X-Workspace` when there is one.
async fn notices_page(app: &TestApp, path: &str, user: Uuid, workspace: Option<Uuid>) -> String {
    let client = app.acting_as(principal(user));
    let mut request = client.get(path).header("accept", "text/html");
    if let Some(workspace) = workspace {
        request = request.header("x-workspace", workspace.to_string());
    }
    request.send().await.assert_status(200).text()
}

/// The body of `GET /_navree/ui/navigation` for `reader` (a guest with `None`), naming
/// `workspace` in `X-Workspace` when there is one.
async fn navigation(app: &TestApp, reader: Option<Uuid>, workspace: Option<Uuid>) -> Vec<u8> {
    let client = match reader {
        Some(user) => app.acting_as(principal(user)),
        None => app.client(),
    };
    let mut request = client.get("/_navree/ui/navigation");
    if let Some(workspace) = workspace {
        request = request.header("x-workspace", workspace.to_string());
    }
    let response = request.send().await.assert_status(200);
    response.bytes().to_vec()
}

/// The link that needs `announcements.read`.
const READ_LINK: &str = r#"href="/announcements""#;

/// The link that needs nothing.
const BOARD_LINK: &str = r#"href="/notice-board""#;

#[tokio::test]
async fn a_contributed_link_shows_to_a_member_whose_workspace_role_grants_it() {
    // Describing the permission grants it to nobody.
    let described = app(false).await;
    let them = people(&described).await;
    let page = notices_home(&described, them.meera, Some(them.office)).await;
    assert!(!page.contains(READ_LINK), "{page}");
    assert!(page.contains(BOARD_LINK), "{page}");
    // The owner's built-in `all.*` holds every permission in the workspace, a plugin's too.
    let page = notices_home(&described, them.harpreet, Some(them.office)).await;
    assert!(page.contains(READ_LINK), "{page}");

    // The app's configuration grants it to a workspace's members.
    let granted = app(true).await;
    let them = people(&granted).await;
    let page = notices_home(&granted, them.meera, Some(them.office)).await;
    assert!(page.contains(READ_LINK), "{page}");
    // Without the header no workspace is named, and nothing grants it app-wide.
    let page = notices_home(&granted, them.meera, None).await;
    assert!(!page.contains(READ_LINK), "{page}");
    assert!(page.contains(BOARD_LINK), "{page}");
    // A viewer's role does not grant it; someone outside the workspace learns nothing by
    // naming it.
    for user in [them.simran, them.kuldeep] {
        let page = notices_home(&granted, user, Some(them.office)).await;
        assert!(!page.contains(READ_LINK), "{page}");
        assert!(page.contains(BOARD_LINK), "{page}");
    }
}

#[tokio::test]
async fn asking_for_links_does_not_enter_the_workspace_on_the_unit_of_work() {
    let granted = app(true).await;
    let them = people(&granted).await;
    let page = notices_home(&granted, them.meera, Some(them.office)).await;
    // The workspace was resolved: the member's link is drawn.
    assert!(page.contains(READ_LINK), "{page}");
    // And not entered: the page's unit of work is in no workspace.
    assert!(page.contains("entered: None"), "{page}");
    // The same page after `CurrentWorkspace` is in it: the unit of work reports an entered
    // workspace, so the line above can tell.
    let entered = notices_page(&granted, "/notices-entered", them.meera, Some(them.office)).await;
    let expected = format!("entered: {:?}", Some(them.office));
    assert!(entered.contains(&expected), "{entered}");
    assert!(entered.contains(READ_LINK), "{entered}");
}

#[tokio::test]
async fn the_navigation_endpoint_answers_the_permitted_items() {
    let granted = app(true).await;
    let them = people(&granted).await;
    let theirs = navigation(&granted, Some(them.meera), Some(them.office)).await;
    let theirs: Value = serde_json::from_slice(&theirs).unwrap();
    let labels: Vec<&str> = theirs["items"]
        .as_array()
        .expect("items")
        .iter()
        .filter_map(|item| item["label"].as_str())
        .collect();
    assert_eq!(labels, ["Announcements", "Notice board"]);

    let guest = navigation(&granted, None, None).await;
    let body: Value = serde_json::from_slice(&guest).unwrap();
    assert_eq!(
        body,
        json!({
            "items": [
                {
                    "label": "Notice board",
                    "href": "/notice-board",
                    "group": "Announcements",
                    "order": 0,
                    "icon": null,
                },
            ],
        })
    );
    // Whoever may not open the gated link gets the guest's bytes, and nothing more.
    for (reader, workspace) in [
        (them.kuldeep, Some(them.office)),
        (them.simran, Some(them.office)),
        (them.meera, None),
    ] {
        assert_eq!(navigation(&granted, Some(reader), workspace).await, guest);
    }
}

/// A digest of notices, which a community plugin reads below, outside its namespace.
#[derive(Clone, Debug, Default, serde::Deserialize, ConfigSection)]
#[config(key = "digest")]
#[serde(default)]
struct DigestOptions {
    /// How many notices a digest holds.
    size: u32,
}

fn reads_digest(m: &mut ModuleBuilder) {
    m.config_section::<DigestOptions>();
}

#[tokio::test(flavor = "current_thread")]
async fn a_community_plugin_reading_a_section_outside_its_namespace_is_warned_about() {
    let warnings = Warnings::default();
    let _guard = warnings.install();
    let builder = Navree::app()
        .module(ConfigModule::for_root(ConfigSources::default()))
        .module(Probe(
            ModuleId::named::<Probe>("notices"),
            PluginInfo::new("navree-plugin-notices", "plugins_docs", "notices", "1.0.0"),
            reads_digest,
        ));
    let report = builder.plugin_report().expect("the graph is valid");
    assert!(report.plugins.iter().all(|plugin| !plugin.first_party));
    let _app = builder.boot().await.expect("boots");
    let logged = warnings.all();
    let digest = "plugin `navree-plugin-notices` reads the configuration section `digest`, outside its namespace `notices`";
    assert!(logged.iter().any(|line| line == digest), "{logged:?}");
}

/// A module written with the two preludes and nothing else imported.
mod with_the_preludes {
    use navree::plugin::prelude::*;
    use navree::prelude::*;

    /// A module that describes a permission and adds a sidebar link, a panel link and a
    /// dashboard card.
    pub struct Everything;

    impl Module for Everything {
        fn register(&self, m: &mut ModuleBuilder) {
            m.permissions(&[navree::plugin::Permission::new(
                "everything.read",
                "Read everything the Ludhiana office keeps",
            )])
            .nav_item(navree::ui::ShellNavItem::new("Everything", "/everything"))
            .helm_nav(navree::helm::HelmLink::new("Everything", "/everything"))
            .helm_widget(
                navree::helm::HelmWidget::new("everything", "Everything")
                    .template("admin/widgets/everything"),
            );
        }
    }
}

#[test]
fn the_plugin_prelude_brings_the_link_card_and_permission_methods() {
    let report = Navree::app()
        .module(with_the_preludes::Everything)
        .validate()
        .expect("the graph is valid");
    assert!(
        report
            .modules
            .iter()
            .any(|module| module.name == "Everything")
    );
    assert_eq!(
        TypeId::of::<navree::plugin::Permission>(),
        TypeId::of::<navree::contracts::Permission>()
    );
}

// What a plugin's module may not register.

/// The roster plugin: its crate is this module, so the app's modules at the top of the file are
/// in no crate it names.
mod navree_plugin_roster {
    use navree::plugin::PluginInfo;
    use navree::prelude::*;

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
}

/// The refusal of the roster plugin's module configuring `module`.
fn roster_configures(module: &str) -> String {
    format!(
        "plugin `navree-plugin-roster` configures the module `{module}` in `RosterModule` (for_root or from_config); only a module of the app configures a module of another crate: remove the plugin or update it to a version that imports `{module}` without configuring it, and configure `{module}` in a module of the app"
    )
}

#[test]
fn a_plugin_that_configures_access_or_workspaces_fails_the_boot() {
    use navree_plugin_roster::RosterModule;
    // Every user may manage everything, set through the app's access module.
    let access = Navree::app()
        .module(AccessModule)
        .module(RosterModule(|m| {
            m.import(AccessModule::for_root(AccessOptions {
                abilities: |_, rules| {
                    rules.can(MANAGE, ALL);
                },
                super_admin_role: None,
            }));
        }))
        .validate()
        .expect_err("the plugin configures access")
        .to_string();
    assert!(
        access
            .lines()
            .any(|line| line == roster_configures("AccessModule")),
        "{access}"
    );
    // The app configures workspaces too: the plugin's refusal, alone.
    let workspaces = Navree::app()
        .module(WorkspaceModule::from_config())
        .module(RosterModule(|m| {
            m.import(WorkspaceModule::from_config());
        }))
        .validate()
        .expect_err("the plugin configures workspaces")
        .to_string();
    assert_eq!(workspaces, roster_configures("WorkspaceModule"));
}

#[test]
fn a_plugin_that_configures_a_module_only_it_imports_is_refused_once() {
    use navree_plugin_roster::RosterModule;
    // No module of the app imports access, so its configured instance acts for the plugin:
    // what it registers is the configured module's, which the one refusal names.
    let text = Navree::app()
        .module(RosterModule(|m| {
            m.import(AccessModule::for_root(AccessOptions::default()));
        }))
        .validate()
        .expect_err("the plugin configures access")
        .to_string();
    let roster: Vec<&str> = text
        .lines()
        .filter(|line| line.contains("navree-plugin-roster"))
        .collect();
    let expected = roster_configures("AccessModule");
    assert_eq!(roster, [expected.as_str()], "{text}");
}

/// The guard plugin: its crate is this module, so the app's modules at the top of the file are in
/// no crate it names.
mod navree_plugin_guard {
    use chrono::{DateTime, Utc};
    use http::request::Parts;
    use navree::BoxFuture;
    use navree::auth::{AuthError, SignInAttempt, SignInPolicy, Strategy};
    use navree::contracts::{Clock, FixedClock, PermissionCheck, Principal};
    use navree::db::{ChangeEvent, ChangeObserver, DbError, Uow};
    use navree::kernel::KernelError;
    use navree::plugin::PluginInfo;
    use navree::prelude::*;
    use navree::testing::principal;
    use navree::ui::{NativeClient, NativeShellPart, NativeShellParts, NativeSignIn};
    use uuid::Uuid;

    /// The plugin's info.
    const GUARD: PluginInfo =
        PluginInfo::new("navree-plugin-guard", module_path!(), "guard", "1.0.0");

    /// The plugin's module: it names the plugin, then registers what its function adds.
    pub struct Provides(pub fn(&mut ModuleBuilder));

    impl Module for Provides {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(GUARD);
            (self.0)(m);
        }
    }

    /// A permission check of the plugin: `Some(true)` for the permission `.0` names, or for
    /// every permission when it names none; no answer for any other.
    pub struct Grants(pub Option<&'static str>);

    impl PermissionCheck for Grants {
        fn check<'a>(
            &'a self,
            _principal: &'a Principal,
            permission: &'a str,
            _workspace: Option<Uuid>,
        ) -> BoxFuture<'a, Result<Option<bool>, BoxError>> {
            let granted = self.0.is_none_or(|only| only == permission);
            Box::pin(async move { Ok(granted.then_some(true)) })
        }
    }

    /// The plugin's permission check, which grants `guard.unlock` to everyone, for a module of
    /// the app to register.
    pub fn permission_check() -> Factory<dyn PermissionCheck> {
        Factory::new(|_| async {
            Ok(Arc::new(Grants(Some("guard.unlock"))) as Arc<dyn PermissionCheck>)
        })
    }

    /// A sign-in strategy of the plugin: it takes every request for the nil user's.
    pub struct TrustEveryone;

    impl Injectable for TrustEveryone {
        fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
            Box::pin(async { Ok(TrustEveryone) })
        }
    }

    impl Strategy for TrustEveryone {
        fn name(&self) -> &'static str {
            "trust-everyone"
        }

        fn authenticate<'a>(
            &'a self,
            _parts: &'a mut Parts,
        ) -> BoxFuture<'a, Result<Option<Principal>, AppError>> {
            Box::pin(async { Ok(Some(principal(Uuid::nil()))) })
        }
    }

    /// A sign-in policy of the plugin: a policy may only refuse a sign-in, and this one lets
    /// every attempt go on.
    pub struct ChapterHours;

    impl Injectable for ChapterHours {
        fn construct(_: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
            Box::pin(async { Ok(ChapterHours) })
        }
    }

    impl SignInPolicy for ChapterHours {
        fn check<'a>(&'a self, _: &'a SignInAttempt) -> BoxFuture<'a, Result<(), AuthError>> {
            Box::pin(async { Ok(()) })
        }
    }

    /// Where the plugin says the app's native clients sign in: routes of its own.
    pub struct SignInHere;

    impl NativeShellPart for SignInHere {
        fn contribute(&self, shell: &mut NativeShellParts, _: &NativeClient) {
            shell.sign_in(NativeSignIn::new("/guard/start", "/guard/exchange"));
        }
    }

    /// The plugin's part of the native clients' shell document, for a module of the app to
    /// register.
    pub fn native_sign_in() -> Factory<dyn NativeShellPart> {
        Factory::new(|_| async { Ok(Arc::new(SignInHere) as Arc<dyn NativeShellPart>) })
    }

    /// The plugin's clock, stopped at the start of 1970, for a module of the app to register.
    pub fn clock() -> Factory<dyn Clock> {
        Factory::new(|_| async {
            Ok(Arc::new(FixedClock::new(DateTime::<Utc>::UNIX_EPOCH)) as Arc<dyn Clock>)
        })
    }

    /// An observer of every commit: it sees the writes, and here changes nothing.
    pub struct Watches;

    impl ChangeObserver for Watches {
        fn observe<'a>(
            &'a self,
            _uow: &'a Uow,
            _changes: &'a [ChangeEvent],
        ) -> BoxFuture<'a, Result<(), DbError>> {
            Box::pin(async { Ok(()) })
        }
    }

    /// The plugin's observer of every commit, for a module of the app to register.
    pub fn change_observer() -> Factory<dyn ChangeObserver> {
        Factory::new(|_| async { Ok(Arc::new(Watches) as Arc<dyn ChangeObserver>) })
    }

    /// A module of the plugin's crate that names no plugin, and registers what `.0` adds.
    pub struct Helper(pub fn(&mut ModuleBuilder));

    impl Module for Helper {
        fn register(&self, m: &mut ModuleBuilder) {
            (self.0)(m);
        }
    }
}

/// The refusal of the guard plugin's `module` (its name, then why it acts for the plugin when
/// it does not name it) providing `provider`, which does what `role` says.
fn guard_provides(module: &str, provider: &str, role: &str) -> String {
    format!(
        "plugin `navree-plugin-guard` provides `{provider}` in {module}, {role}; only a module of the app registers it: remove the plugin or update it to a version that leaves it to the app, and to use the plugin's, register it from a module of the app"
    )
}

/// The refusal of the guard plugin's module hooking `model`.
fn guard_hooks(model: &str) -> String {
    format!(
        "plugin `navree-plugin-guard` hooks the model `{model}` in `Provides`, a model of another crate; only a module of the app hooks it: remove the plugin or update it to a version that leaves the hook to the app, and to use the plugin's hook, register it from a module of the app"
    )
}

/// A generic module of the app's crate that registers a permission check granting every
/// permission: imported only by a module of the guard plugin, it acts for that plugin.
struct GrantsAll<T>(PhantomData<fn() -> T>);

impl<T: 'static> Module for GrantsAll<T> {
    fn register(&self, m: &mut ModuleBuilder) {
        m.multi(Factory::<dyn PermissionCheck>::new(|_| async {
            Ok(Arc::new(navree_plugin_guard::Grants(None)) as Arc<dyn PermissionCheck>)
        }));
    }
}

/// A provider of `T` that is never built: the plugin checks run before anything is built.
fn never_built<T: ?Sized + Send + Sync + 'static>() -> Factory<T> {
    Factory::new(|_| async {
        Err::<Arc<T>, BoxError>("the boot refuses it before anything is built".into())
    })
}

#[test]
fn a_plugin_that_provides_an_access_provider_fails_the_boot() {
    use std::any::type_name;

    use navree::access::{AbilityFactory, WorkspaceGate};
    use navree::auth::{AuthOptions, Strategy};
    use navree::contracts::Clock;
    use navree::db::ChangeObserver;
    use navree::helm::HelmOptions;
    use navree::http::{Authenticator, RouteAuthorizer};
    use navree::ui::NativeShellPart;
    use navree::workspace::WorkspaceConfig;
    use navree_plugin_guard::{ChapterHours, Grants, Provides, TrustEveryone};

    // One app per provider, each registered by the plugin's module, with what the refusal
    // says the provider does.
    let cases = [
        // A permission check that grants every permission to everyone.
        (
            Provides(|m| {
                m.multi(Factory::<dyn PermissionCheck>::new(|_| async {
                    Ok(Arc::new(Grants(None)) as Arc<dyn PermissionCheck>)
                }));
            }),
            type_name::<dyn PermissionCheck>(),
            "which answers the app's permission checks",
        ),
        (
            Provides(|m| {
                m.factory(never_built::<dyn RouteAuthorizer>());
            }),
            type_name::<dyn RouteAuthorizer>(),
            "which decides who may open the app's routes",
        ),
        (
            Provides(|m| {
                m.factory(never_built::<dyn Authenticator>());
            }),
            type_name::<dyn Authenticator>(),
            "which decides who a request is signed in as",
        ),
        (
            Provides(|m| {
                m.strategy::<TrustEveryone>();
            }),
            type_name::<dyn Strategy>(),
            "which signs requests in",
        ),
        (
            Provides(|m| {
                m.factory(never_built::<dyn WorkspaceGate>());
            }),
            type_name::<dyn WorkspaceGate>(),
            "which decides who enters the app's workspaces",
        ),
        (
            Provides(|m| {
                m.value(AccessOptions::default());
            }),
            type_name::<AccessOptions>(),
            "which sets the options of the app's access rules",
        ),
        (
            Provides(|m| {
                m.factory(never_built::<AbilityFactory>());
            }),
            type_name::<AbilityFactory>(),
            "which builds every user's abilities",
        ),
        (
            Provides(|m| {
                m.value(WorkspaceConfig::default());
            }),
            type_name::<WorkspaceConfig>(),
            "which sets the options of the app's workspaces",
        ),
        (
            Provides(|m| {
                m.value(AuthOptions::default());
            }),
            type_name::<AuthOptions>(),
            "which sets the options of the app's sign-in",
        ),
        (
            Provides(|m| {
                m.value(HelmOptions::default());
            }),
            type_name::<HelmOptions>(),
            "which sets the options of the app's admin panel",
        ),
        (
            Provides(|m| {
                m.value(Shell::new("Khata")).exports::<Shell>();
            }),
            type_name::<Shell>(),
            "which is the app's shell",
        ),
        // Where the app's native clients sign in: the plugin's own routes.
        (
            Provides(|m| {
                m.multi(navree_plugin_guard::native_sign_in());
            }),
            type_name::<dyn NativeShellPart>(),
            "which says where the app's native clients sign in",
        ),
        // The time the app's sessions, signed links and invitations expire by.
        (
            Provides(|m| {
                m.factory(navree_plugin_guard::clock());
            }),
            type_name::<dyn Clock>(),
            "which sets the time the app's sessions, signed links and invitations expire by",
        ),
        // An observer that sees, and could change, every write the app commits.
        (
            Provides(|m| {
                m.multi(navree_plugin_guard::change_observer());
            }),
            type_name::<dyn ChangeObserver>(),
            "which sees, and can change, every write the app commits",
        ),
    ];
    for (provides, provider, role) in cases {
        let refused = Navree::app()
            .module(provides)
            .validate()
            .expect_err(provider)
            .to_string();
        assert_eq!(refused, guard_provides("`Provides`", provider, role));
    }
    // A sign-in policy may only refuse a sign-in: a plugin registers its own.
    Navree::app()
        .module(Provides(|m| {
            m.sign_in_policy::<ChapterHours>();
        }))
        .validate()
        .expect("a plugin's sign-in policy");
}

#[test]
fn a_refusal_says_why_a_module_acts_for_the_plugin() {
    use std::any::type_name;

    use navree_plugin_guard::{Helper, Provides};

    let provider = type_name::<dyn PermissionCheck>();
    let role = "which answers the app's permission checks";
    // A module of the plugin's crate that does not name the plugin.
    let in_crate = Navree::app()
        .module(Provides(|_| {}))
        .module(Helper(|m| {
            m.multi(navree_plugin_guard::permission_check());
        }))
        .validate()
        .expect_err("a module of the plugin's crate")
        .to_string();
    assert_eq!(
        in_crate,
        guard_provides(
            "`Helper` (a module of the plugin's crate `plugins_docs::navree_plugin_guard`)",
            provider,
            role,
        )
    );
    // A generic module of the app's crate that only the plugin's module imports.
    let imported = Navree::app()
        .module(Provides(|m| {
            m.import(GrantsAll::<u8>(PhantomData));
        }))
        .validate()
        .expect_err("a module only the plugin imports")
        .to_string();
    assert_eq!(
        imported,
        guard_provides(
            "`GrantsAll<u8>` (only the plugin's modules import it)",
            provider,
            role,
        )
    );
}

/// A module of the app: it registers the guard plugin's permission check.
struct GuardChecks;

impl Module for GuardChecks {
    fn register(&self, m: &mut ModuleBuilder) {
        m.multi(navree_plugin_guard::permission_check());
    }
}

#[tokio::test]
async fn the_app_may_register_a_plugins_permission_check() {
    let app = TestApp::builder()
        .module(navree_plugin_guard::Provides(|_| {}))
        .module(GuardChecks)
        .build()
        .await;
    let harpreet = principal(Uuid::now_v7());
    let unlock = permitted(app.ctx(), &harpreet, "guard.unlock", None)
        .await
        .unwrap();
    assert!(unlock, "the app's module registered the plugin's check");
    // The check answers for its own permission only.
    let other = permitted(app.ctx(), &harpreet, "deals.read", None)
        .await
        .unwrap();
    assert!(!other);
    // The app's module acts for no plugin.
    let report = app.ctx().plugins();
    let guard = report
        .plugins
        .iter()
        .find(|plugin| plugin.info.namespace == "guard")
        .expect("the guard plugin");
    let modules: Vec<&str> = guard
        .modules
        .iter()
        .map(|module| module.id.name())
        .collect();
    assert_eq!(modules, ["Provides"]);

    app.shutdown().await.unwrap();
}

/// A module of the app: it registers the guard plugin's sign-in strategy.
struct GuardSignIn;

impl Module for GuardSignIn {
    fn register(&self, m: &mut ModuleBuilder) {
        m.strategy::<navree_plugin_guard::TrustEveryone>();
    }
}

#[tokio::test]
async fn the_app_may_register_a_plugins_sign_in_strategy() {
    use navree::auth::Strategy;

    let app = TestApp::builder()
        .temp_database()
        .config("app.key", generate_key())
        .config("http.per_ip_rate_limit", Value::Null)
        .module(CryptoModule::from_config())
        .module(DatabaseModule::from_config())
        .module(AuthModule::from_config())
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(navree_plugin_guard::Provides(|_| {}))
        .module(GuardSignIn)
        .build()
        .await;
    let strategies = app.ctx().get_all::<dyn Strategy>().await.unwrap();
    assert!(
        strategies
            .iter()
            .any(|strategy| strategy.name() == "trust-everyone"),
        "the app's module registered the plugin's strategy"
    );

    app.shutdown().await.unwrap();
}

/// A module of the app: it registers where the guard plugin says native clients sign in.
struct GuardNativeSignIn;

impl Module for GuardNativeSignIn {
    fn register(&self, m: &mut ModuleBuilder) {
        m.multi(navree_plugin_guard::native_sign_in());
    }
}

#[tokio::test]
async fn the_app_may_register_where_a_plugin_says_native_clients_sign_in() {
    use navree::ui::{NativeClient, NativeFamily, NativeShellPart, NativeShellParts, NativeSignIn};

    let app = TestApp::builder()
        .module(navree_plugin_guard::Provides(|_| {}))
        .module(GuardNativeSignIn)
        .build()
        .await;
    let parts = app.ctx().get_all::<dyn NativeShellPart>().await.unwrap();
    assert_eq!(parts.len(), 1);
    let mut said = NativeShellParts::default();
    parts[0].contribute(&mut said, &NativeClient::new(NativeFamily::Apple));
    let mut expected = NativeShellParts::default();
    expected.sign_in(NativeSignIn::new("/guard/start", "/guard/exchange"));
    assert_eq!(said, expected);

    app.shutdown().await.unwrap();
}

/// A module of the app: it registers the guard plugin's clock as the app's.
struct GuardClock;

impl Module for GuardClock {
    fn register(&self, m: &mut ModuleBuilder) {
        m.factory(navree_plugin_guard::clock());
    }
}

#[tokio::test]
async fn the_app_may_register_a_plugins_clock() {
    use chrono::{DateTime, Utc};
    use navree::contracts::Clock;

    let app = TestApp::builder()
        .module(navree_plugin_guard::Provides(|_| {}))
        .module(GuardClock)
        .build()
        .await;
    let clock = app.get::<dyn Clock>().await;
    assert_eq!(clock.now(), DateTime::<Utc>::UNIX_EPOCH);

    app.shutdown().await.unwrap();
}

/// A module of the app: it registers the guard plugin's observer of every commit.
struct GuardObserver;

impl Module for GuardObserver {
    fn register(&self, m: &mut ModuleBuilder) {
        m.multi(navree_plugin_guard::change_observer());
    }
}

#[tokio::test]
async fn the_app_may_register_a_plugins_change_observer() {
    use navree::db::ChangeObserver;

    let app = TestApp::builder()
        .module(navree_plugin_guard::Provides(|_| {}))
        .module(GuardObserver)
        .build()
        .await;
    let observers = app.ctx().get_all::<dyn ChangeObserver>().await.unwrap();
    assert_eq!(observers.len(), 1);

    app.shutdown().await.unwrap();
}

#[test]
fn a_plugin_that_hooks_another_crates_model_fails_validate() {
    use std::any::type_name;
    use std::sync::atomic::{AtomicBool, Ordering};

    use navree::orm::hooks::{ModelEvent, ModelHook};
    use navree::workspace::Member;
    use navree_plugin_guard::Provides;

    /// Whether the plugin's hook was built.
    static BUILT: AtomicBool = AtomicBool::new(false);

    let refused = guard_hooks(type_name::<Member>());
    // A listener that would make every member of a workspace its owner.
    let listener = Navree::app()
        .module(Provides(|m| {
            m.on_model::<Member, _>(ModelEvent::Saving, |member, _ctx| {
                Box::pin(async move {
                    member.role = "owner".to_owned();
                    Ok(())
                })
            });
        }))
        .validate()
        .expect_err("the plugin hooks the member row")
        .to_string();
    assert_eq!(listener, refused);
    // The same through the hook's own key.
    let multi = Navree::app()
        .module(Provides(|m| {
            m.multi(Factory::<dyn ModelHook<Member>>::new(|_| async {
                BUILT.store(true, Ordering::SeqCst);
                Err::<Arc<dyn ModelHook<Member>>, BoxError>("never built".into())
            }));
        }))
        .validate()
        .expect_err("the plugin hooks the member row")
        .to_string();
    assert_eq!(multi, refused);
    assert!(
        !BUILT.load(Ordering::SeqCst),
        "the hook's factory never ran"
    );
}
