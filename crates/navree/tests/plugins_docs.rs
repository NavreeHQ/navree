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
use std::sync::Mutex;
use std::time::Duration;

use http::Method;
use navree::config::ConfigSection;
use navree::contracts::permission_catalog;
use navree::crypto::generate_key;
use navree::db::Db;
use navree::http::extract::Request;
use navree::http::{AppRouter, Controller, FallibleLayer, GlobalLayer, LayerSlot, Mount, RouteDef};
use navree::kernel::ModuleId;
use navree::plugin::prelude::*;
use navree::plugin::{ClientModule, Permission, PluginInfo};
use navree::prelude::*;
use navree::testing::{TestApp, principal};
use navree::ui::{AppShell, Shell, ShellNavItem, Text};
use navree::workspace::{Members, NewWorkspace, WorkspaceRole, Workspaces};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// existing-crates: a ledger plugin that connects an existing crate
// ---------------------------------------------------------------------------

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
        .plugin(navree::plugin_info!(namespace = "khata"))
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
        m.plugin(navree::plugin_info!(namespace = "khata"));
        m.mount(
            Mount::new("/khata", AppRouter::from_handler(entries))
                .authenticated()
                .throttle(60, Duration::from_secs(60)),
        );
    }
}

/// A module of the app: it adds the ledger's timeout, and maps its error.
pub struct AppModule;

impl Module for AppModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.http_layer(khata_timeout())
            .map_error::<tower::timeout::error::Elapsed>(|_| {
                Some(AppError::service_unavailable("The khata took too long."))
            });
    }
}

#[tokio::test]
async fn the_existing_crates_page_examples_mount_inside_the_pipeline() {
    let app = TestApp::builder()
        .config("khata.base_url", "https://khata.example/ludhiana")
        .module(khata_client())
        .module(LedgerModule)
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

/// The announcements plugin, a community plugin of this test crate.
const ANNOUNCEMENTS: PluginInfo = PluginInfo::new(
    "navree-plugin-announcements",
    "plugins_docs",
    "announcements",
    "1.0.0",
)
.title("Announcements");

/// The announcements plugin's module: it describes `announcements.read`, adds a link that
/// needs it and a notice board that needs nothing, registers the app's shell (which takes
/// the links modules add) and serves `GET /notices-home`.
struct Announcements;

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
        m.value(Shell::new("Kesar").contributed()).exports::<Shell>();
        m.controller::<NoticesHome>();
    }
}

/// `GET /notices-home`: a page drawn from the `AppShell` extractor that says whether its
/// unit of work entered a workspace.
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
        ]
    }
}

/// The app: workspaces named by the `X-Workspace` header, access, the toolkit and the
/// announcements plugin; with `member_grant`, the app's configuration grants
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
        .module(Announcements)
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
    let client = app.acting_as(principal(user));
    let mut request = client.get("/notices-home").header("accept", "text/html");
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

/// A section of the database's, which the storage package's plugin reads below.
#[derive(Clone, Debug, Default, serde::Deserialize, ConfigSection)]
#[config(key = "database.extra")]
#[serde(default)]
struct DatabaseExtra {
    /// How many more connections the reports of the Amritsar office may open.
    connections: u32,
}

/// A digest of notices, which a community plugin reads below, outside its namespace.
#[derive(Clone, Debug, Default, serde::Deserialize, ConfigSection)]
#[config(key = "digest")]
#[serde(default)]
struct DigestOptions {
    /// How many notices a digest holds.
    size: u32,
}

/// `navree-storage` as the kernel names it: first-party, at the kernel's version.
fn storage_plugin() -> PluginInfo {
    PluginInfo::new(
        "navree-storage",
        "navree_storage",
        "storage",
        env!("CARGO_PKG_VERSION"),
    )
}

/// A module with the storage package's module id that names its plugin, then registers
/// what `adds` adds.
fn storage_probe(adds: fn(&mut ModuleBuilder)) -> Probe {
    Probe(
        ModuleId::of::<navree::storage::StorageModule>(),
        storage_plugin(),
        adds,
    )
}

fn reads_database_extra(m: &mut ModuleBuilder) {
    m.config_section::<DatabaseExtra>();
}

fn reads_digest(m: &mut ModuleBuilder) {
    m.config_section::<DigestOptions>();
}

fn describes_storage_audit(m: &mut ModuleBuilder) {
    m.permissions(&[Permission::new(
        "storage.audit",
        "Read the audit of the files stored for the Jalandhar branch",
    )]);
}

#[tokio::test(flavor = "current_thread")]
async fn a_first_party_plugin_reading_a_section_outside_its_namespace_does_not_warn() {
    let warnings = Warnings::default();
    let _guard = warnings.install();
    let builder = Navree::app()
        .module(ConfigModule::for_root(ConfigSources::default()))
        .module(storage_probe(reads_database_extra))
        .module(Probe(
            ModuleId::named::<Probe>("notices"),
            PluginInfo::new("navree-plugin-notices", "plugins_docs", "notices", "1.0.0"),
            reads_digest,
        ));
    let report = builder.plugin_report().expect("the graph is valid");
    let first_party = |name: &str| {
        report
            .plugins
            .iter()
            .find(|plugin| plugin.info.name == name)
            .map(|plugin| plugin.first_party)
    };
    assert_eq!(first_party("navree-storage"), Some(true));
    assert_eq!(first_party("navree-plugin-notices"), Some(false));
    let _app = builder.boot().await.expect("boots");
    let logged = warnings.all();
    // The positive control: the community plugin's section outside its namespace.
    let digest = "plugin `navree-plugin-notices` reads the configuration section `digest`, outside its namespace `notices`";
    assert!(logged.iter().any(|line| line == digest), "{logged:?}");
    assert!(
        !logged.iter().any(|line| line.contains("navree-storage")),
        "{logged:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_first_party_plugin_declaring_another_modules_permission_only_warns() {
    /// A module of the app that describes the storage package's audit permission too.
    struct Reports;

    impl Module for Reports {
        fn register(&self, m: &mut ModuleBuilder) {
            describes_storage_audit(m);
        }
    }

    let warnings = Warnings::default();
    let _guard = warnings.install();
    let app = TestApp::builder()
        .temp_database()
        .module(DatabaseModule::from_config())
        .module(AccessModule::for_root(AccessOptions::default()))
        .module(storage_probe(describes_storage_audit))
        .module(Reports)
        .build()
        .await;
    let described: Vec<(&str, &str)> = permission_catalog(app.ctx())
        .iter()
        .map(|(module, permission)| (module.name, permission.name))
        .collect();
    assert_eq!(described.len(), 2, "{described:?}");
    assert!(
        described.contains(&("StorageModule", "storage.audit")),
        "{described:?}"
    );
    assert!(
        described.contains(&("Reports", "storage.audit")),
        "{described:?}"
    );
    let logged = warnings.all();
    assert!(
        logged.iter().any(|line| {
            line.starts_with("permission `storage.audit` is declared by both")
                && line.ends_with("; this becomes an error in 2.0")
        }),
        "{logged:?}"
    );
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
    assert!(report.modules.iter().any(|module| module.name == "Everything"));
    assert_eq!(
        TypeId::of::<navree::plugin::Permission>(),
        TypeId::of::<navree::contracts::Permission>()
    );
}
