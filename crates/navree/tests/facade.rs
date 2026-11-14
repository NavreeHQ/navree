//! The facade's surface: one module per crate (each re-exporting the whole
//! crate), the macros and the most used kernel types at the root, the
//! prelude, `navree::Result`, and the Cargo features (one per module crate,
//! each turning on what its crate builds on).

use std::any::TypeId;
use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

use navree::prelude::*;
use navree::{KernelError, ModuleId};
use serde_json::Value;

/// `navree::<module>::<Item>` is the very item of the underlying crate.
fn same<A: ?Sized + 'static, B: ?Sized + 'static>() -> bool {
    TypeId::of::<A>() == TypeId::of::<B>()
}

#[test]
fn every_module_crate_is_reexported_whole() {
    // L0-L1
    assert!(same::<navree::kernel::Navree, navree::Navree>());
    assert!(same::<navree::kernel::ModuleBuilder, navree::ModuleBuilder>());
    assert!(same::<
        navree::bounded::Cache<String, u32>,
        navree_bounded::Cache<String, u32>,
    >());
    assert!(same::<navree::glob::Glob, navree_glob::Glob>());
    assert!(same::<
        navree::contracts::RequestId,
        navree::contracts::RequestId,
    >());
    assert!(same::<navree::config::ConfigModule, ConfigModule>());
    assert!(same::<
        navree::telemetry::TelemetryModule,
        navree::telemetry::TelemetryModule,
    >());
    // L2
    assert!(same::<navree::http::HttpModule, HttpModule>());
    assert!(same::<navree::log::LoggerModule, LoggerModule>());
    assert!(same::<navree::crypto::CryptoModule, CryptoModule>());
    assert!(same::<navree::events::EventsModule, EventsModule>());
    assert!(same::<navree::view::ViewModule, ViewModule>());
    assert!(same::<navree::cache::CacheModule, CacheModule>());
    assert!(same::<
        navree::http_client::HttpClientModule,
        HttpClientModule,
    >());
    assert!(same::<
        navree::schema::Blueprint,
        navree::db::navree_schema::Blueprint,
    >());
    // L3
    assert!(same::<navree::db::DatabaseModule, DatabaseModule>());
    assert!(same::<navree::testing::TestApp, navree::testing::TestApp>());
    assert!(same::<navree::observe::ObserveModule, ObserveModule>());
    assert!(same::<navree::api::ApiModule, ApiModule>());
    assert!(same::<navree::mail::MailModule, MailModule>());
    assert!(same::<navree::realtime::RealtimeModule, RealtimeModule>());
    assert!(same::<navree::storage::StorageModule, StorageModule>());
    assert!(same::<navree::time::TimeModule, TimeModule>());
    // L4
    assert!(same::<navree::session::SessionModule, SessionModule>());
    assert!(same::<navree::validation::ValidationModule, ValidationModule>());
    assert!(same::<navree::queue::QueueModule, QueueModule>());
    assert!(same::<navree::audit::AuditModule, AuditModule>());
    assert!(same::<navree::access::AccessModule, AccessModule>());
    assert!(same::<navree::schedule::ScheduleModule, ScheduleModule>());
    assert!(same::<navree::notify::NotifyModule, NotifyModule>());
    assert!(same::<navree::orm::Record<Thing>, Record<Thing>>());
    // L5
    assert!(same::<navree::auth::AuthModule, AuthModule>());
    assert!(same::<navree::workspace::WorkspaceModule, WorkspaceModule>());
    // L6
    assert!(same::<navree::webhooks::WebhooksModule, WebhooksModule>());
    assert!(same::<navree::settings::SettingsModule, SettingsModule>());
    assert!(same::<
        navree::settings::Setting<FacadeSettings>,
        Setting<FacadeSettings>,
    >());
    assert!(same::<navree::settings::Settings, navree::settings::Settings>());
    assert!(same::<navree::sso::SsoModule, SsoModule>());
    assert!(same::<navree::sheets::SheetsModule, SheetsModule>());
    assert!(same::<
        navree::integrations::IntegrationsModule,
        IntegrationsModule,
    >());
    assert!(same::<
        navree::integrations::Integrations,
        navree::integrations::Integrations,
    >());
    // L7
    assert!(same::<navree::lighthouse::LighthouseModule, LighthouseModule>());
    assert!(same::<
        navree::lighthouse::Lighthouse,
        navree::lighthouse::Lighthouse,
    >());
    assert!(same::<navree::logbook::LogbookModule, LogbookModule>());
    assert!(same::<navree::logbook::Logbook, navree::logbook::Logbook>());
    assert!(same::<navree::fleet::FleetModule, FleetModule>());
    assert!(same::<navree::workflows::WorkflowsModule, WorkflowsModule>());
    assert!(same::<
        navree::workflows::Workflows,
        navree::workflows::Workflows,
    >());
    assert!(same::<navree::directory::DirectoryModule, DirectoryModule>());
    assert!(same::<
        navree::directory::Directory,
        navree::directory::Directory,
    >());
    assert!(same::<navree::domains::DomainsModule, DomainsModule>());
    assert!(same::<navree::domains::CurrentDomain, CurrentDomain>());
    assert!(same::<navree::domains::Domains, navree::domains::Domains>());
    // L7 (opt-in)
    #[cfg(feature = "billing")]
    assert!(same::<navree::billing::BillingModule, BillingModule>());
    // L7
    assert!(same::<navree::helm::HelmModule, HelmModule>());
}

/// Opt-in ecosystem crates are re-exported whole behind their feature
/// (`cargo test -p navree --features oauth --test facade`).
#[cfg(feature = "oauth")]
#[test]
fn opt_in_crates_are_reexported_with_their_feature() {
    assert!(same::<navree::oauth::OAuthServerModule, OAuthServerModule>());
    assert!(same::<navree::oauth::OAuthToken, OAuthToken>());
    let (verifier, challenge) = navree::oauth::testing::pkce_pair();
    assert_eq!(navree::oauth::pkce_challenge(&verifier), challenge);
}

/// `navree::media` is navree-media re-exported whole behind `media`
/// (`cargo test -p navree --features media --test facade`).
#[cfg(feature = "media")]
#[test]
fn media_is_reexported_with_its_feature() {
    assert!(same::<navree::media::MediaModule, MediaModule>());
    assert!(same::<navree::media::Media, Media>());
    assert!(same::<navree::media::MediaFake, navree_media::MediaFake>());
}

/// `navree::ai` is navree-ai re-exported whole, and the `ai` feature puts
/// `AiModule` in the prelude (`cargo test -p navree --features ai --test facade`).
#[cfg(feature = "ai")]
#[test]
fn ai_is_reexported_with_its_feature() {
    assert!(same::<navree::ai::AiModule, AiModule>());
    assert!(same::<navree::ai::Ai, navree::ai::Ai>());
}

/// The kernel names an app's `main` and root module use most.
#[test]
fn kernel_essentials_sit_at_the_root() {
    assert!(same::<navree::Navree, navree::kernel::Navree>());
    assert!(same::<navree::AppBuilder, navree::kernel::AppBuilder>());
    assert!(same::<navree::AppContext, navree::kernel::AppContext>());
    assert!(same::<navree::BoxError, navree::kernel::BoxError>());
    assert!(same::<dyn navree::Module, dyn navree::kernel::Module>());
}

#[test]
fn prelude_brings_the_everyday_types() {
    // Kernel and HTTP.
    assert!(same::<Navree, navree::kernel::Navree>());
    assert!(same::<dyn Module, dyn navree::kernel::Module>());
    assert!(same::<Arc<u8>, std::sync::Arc<u8>>());
    assert!(same::<AppError, navree::http::AppError>());
    assert!(same::<Json<u8>, navree::http::Json<u8>>());
    assert!(same::<Created<u8>, navree::http::Created<u8>>());
    assert!(same::<NoContent, navree::http::NoContent>());
    assert!(same::<Scoped<u8>, navree::http::Scoped<u8>>());
    assert!(same::<Path<u8>, navree::http::Path<u8>>());
    assert!(same::<Query<u8>, navree::http::Query<u8>>());
    // Data, validation, identity and access.
    assert!(same::<Uow, navree::db::Uow>());
    assert!(same::<Page<u8>, navree::db::Page<u8>>());
    assert!(same::<PageQuery, navree::db::PageQuery>());
    assert!(same::<Valid<u8>, navree::validation::Valid<u8>>());
    assert!(same::<CurrentUser, navree::auth::CurrentUser>());
    assert!(same::<Abilities, navree::access::Abilities>());
    assert!(same::<Rules, navree::access::Rules>());
    assert!(same::<AbilityCtx, navree::access::AbilityCtx>());
    // Long work: batches and operations.
    assert!(same::<Batch, navree::queue::Batch>());
    assert!(same::<Operation, navree::queue::Operation>());
    assert!(same::<Operations, navree::queue::Operations>());
    // Workspaces.
    assert!(same::<CurrentWorkspace, navree::workspace::CurrentWorkspace>());
    assert!(same::<InWorkspace, navree::workspace::InWorkspace>());
    assert!(same::<WorkspaceUow, navree::workspace::WorkspaceUow>());
    assert!(same::<
        WorkspaceRepo<Thing>,
        navree::workspace::WorkspaceRepo<Thing>,
    >());
    // Organizations and partitions.
    assert!(same::<
        CurrentOrganization,
        navree::workspace::CurrentOrganization,
    >());
    assert!(same::<InOrganization, navree::workspace::InOrganization>());
    assert!(same::<CurrentPartition, navree::workspace::CurrentPartition>());
    assert!(same::<PartitionScope, navree::workspace::PartitionScope>());
    assert!(same::<OrgRepo<Thing>, navree::workspace::OrgRepo<Thing>>());
    // Shared libraries.
    assert!(same::<
        LibraryRepo<Thing>,
        navree::workspace::library::LibraryRepo<Thing>,
    >());
    assert!(same::<LibraryScope, navree::workspace::library::LibraryScope>());
    fn library<M: Library>() {}
    let _ = library::<LibraryItem>;
    // Time zones.
    assert!(same::<CurrentZone, navree::time::CurrentZone>());
    assert!(same::<Zone, navree::time::Zone>());
    assert!(same::<LocalDay, navree::time::LocalDay>());
    assert!(same::<DayRange, navree::time::DayRange>());
    assert!(same::<SendWindow, navree::time::SendWindow>());
    assert!(same::<WithZone<u8>, navree::time::WithZone<u8>>());
    for (marker, name) in [
        (TypeId::of::<Read>(), Read::NAME),
        (TypeId::of::<Create>(), Create::NAME),
        (TypeId::of::<Update>(), Update::NAME),
        (TypeId::of::<Delete>(), Delete::NAME),
        (TypeId::of::<Manage>(), Manage::NAME),
    ] {
        assert_ne!(marker, TypeId::of::<()>(), "{name}");
    }
    assert_eq!(
        (READ, CREATE, UPDATE, DELETE, MANAGE, ALL),
        ("read", "create", "update", "delete", "manage", "all")
    );
    let _: fn(&str) -> navree::access::FieldRef = field;
    // The two `Factory` names: the DI factory keeps its name, the ORM's
    // model factory trait is `ModelFactory`.
    assert!(same::<Factory<u8>, navree::kernel::Factory<u8>>());
    fn orm_factory<F: navree::orm::factory::ModelFactory>() {}
    orm_factory::<ThingFactory>();
}

/// A model factory written against the prelude's `ModelFactory`.
struct ThingFactory;

impl ModelFactory for ThingFactory {
    type Model = Thing;

    fn definition(&self, _ctx: &mut navree::orm::factory::FactoryContext<'_, Thing>) -> Thing {
        Thing {
            id: 0,
            name: "thing".to_owned(),
        }
    }
}

/// An ORM model declared with the prelude alone.
#[derive(Model, Clone, Debug, Default, serde::Serialize)]
#[model(table = "things")]
struct Thing {
    id: i64,
    name: String,
}

/// A library model declared with the prelude alone.
#[derive(Model, Clone, Debug, Default, serde::Serialize)]
#[model(table = "library_items")]
struct LibraryItem {
    id: uuid::Uuid,
    scope: LibraryScope,
    organization_id: Option<uuid::Uuid>,
    workspace_id: Option<uuid::Uuid>,
    cloned_from_id: Option<uuid::Uuid>,
}

impl Library for LibraryItem {}

/// Settings declared through the facade: the derive names
/// `::navree::settings`.
#[derive(
    navree::settings::Settings,
    Clone,
    Debug,
    Default,
    serde::Serialize,
    serde::Deserialize,
    navree::validation::Validate,
)]
#[settings(key = "facade", scopes(global, user))]
struct FacadeSettings {
    #[validate(skip)]
    compact: bool,
}

#[test]
fn the_settings_derive_works_through_the_facade() {
    use navree::settings::{ScopeKind, SettingsGroup};
    assert_eq!(FacadeSettings::KEY, "facade");
    assert_eq!(FacadeSettings::SCOPES, [ScopeKind::Global, ScopeKind::User]);
    assert_eq!(
        FacadeSettings::DESCRIPTION,
        "Settings declared through the facade: the derive names `::navree::settings`."
    );
}

#[test]
fn prelude_brings_the_orm() {
    assert!(same::<Collection<Thing>, navree::orm::Collection<Thing>>());
    assert!(same::<Builder<Thing>, navree::orm::Builder<Thing>>());
    assert!(same::<OrmModule, navree::orm::OrmModule>());
    assert_eq!(<Thing as navree::orm::Model>::meta().table(), "things");
    assert_eq!(Thing::NAME.name(), "name");
    // The lifecycle's model methods and the module registrations resolve
    // with the prelude alone.
    assert!(Thing::global_scope_names().is_empty());
    let _register = |m: &mut ModuleBuilder| {
        m.model::<Thing>();
    };
}

/// A subject and an action declared by an app, with the prelude only.
struct Invoice;

impl Subject for Invoice {
    const SUBJECT: &'static str = "Invoice";
    const PERMISSION_PREFIX: &'static str = "invoices";
}

struct Dispatch;

impl ActionMarker for Dispatch {
    const NAME: &'static str = "send";
}

#[test]
fn prelude_traits_are_implementable() {
    assert_eq!(<Invoice as Subject>::SUBJECT, "Invoice");
    assert_eq!(<Dispatch as ActionMarker>::NAME, "send");
}

#[test]
fn result_defaults_to_app_error() {
    fn find(id: u32) -> navree::Result<u32> {
        if id == 0 {
            return Err(AppError::not_found());
        }
        Ok(id)
    }
    assert_eq!(find(7).ok(), Some(7));
    assert_eq!(find(0).unwrap_err().status.as_u16(), 404);
    // The prelude's `Result` is the same alias.
    let other: Result<u8, std::fmt::Error> = Err(std::fmt::Error);
    assert!(other.is_err());
}

#[test]
fn module_builder_extensions_come_with_the_prelude() {
    // `m.migrations(..)`, `m.controller::<C>()`, `m.config::<T>()`, `m.job::<J>()`,
    // `m.templates(..)`, ... resolve with the prelude alone.
    struct Wired;

    impl Module for Wired {
        fn register(&self, m: &mut ModuleBuilder) {
            m.migrations(navree::db::MigrationSet::new("wired"));
            m.templates(&[("wired.txt", "hi")]);
        }
    }

    let report = Navree::app()
        .module(Wired)
        .validate()
        .expect("the graph is valid");
    assert!(report.modules.iter().any(|m| m.name == "Wired"));
}

/// `#[derive(Exportable)]` and `#[derive(Importable)]` expand through
/// `navree::sheets` in an app that depends on the facade only.
#[test]
fn sheets_derives_expand_through_the_facade() {
    use navree::sheets::{Cell, Exportable, ImportRow, Importable};

    #[derive(Exportable)]
    #[export(sheet = "Deals")]
    struct DealRow {
        #[export(header = "Title")]
        title: String,
        amount: i64,
    }

    #[derive(Importable)]
    #[allow(dead_code)]
    struct DealInput {
        #[import(aliases = ["Deal"])]
        title: String,
        amount: Option<i64>,
    }

    let row = DealRow {
        title: "Renewal".into(),
        amount: 1200,
    };
    assert_eq!(row.cells(), [Cell::Text("Renewal".into()), Cell::Int(1200)]);
    assert_eq!(DealRow::sheet_name(), "Deals");
    let columns = DealInput::columns();
    assert_eq!(columns[0].aliases, ["Deal"]);
    assert!(columns[0].required);
    assert!(!columns[1].required);
}

/// `#[workflow]`, `#[trigger]`, `#[condition]` and `#[action]` expand
/// through `navree::workflows` in an app that depends on the facade only.
#[test]
fn workflow_macros_expand_through_the_facade() {
    use navree::workflows::{
        Action, AutomationCtx, Condition, Trigger, Workflow, WorkflowCtx, action, condition,
        trigger, workflow,
    };

    #[derive(serde::Serialize, serde::Deserialize)]
    struct Welcome {
        user_id: u64,
    }

    /// Welcomes a user.
    #[workflow(version = 2)]
    async fn welcome(ctx: WorkflowCtx, input: Welcome) -> Result<u64, BoxError> {
        let _ = ctx;
        Ok(input.user_id)
    }

    #[derive(Clone, serde::Serialize, serde::Deserialize)]
    struct Signed {
        user_id: u64,
    }
    impl navree::events::Event for Signed {
        const NAME: &'static str = "user.signed";
    }

    /// A user signed.
    #[trigger(type = "user.signed", event = Signed, subject = "user_id")]
    #[derive(serde::Serialize, serde::Deserialize, navree::openapi::ToSchema)]
    struct SignedTrigger {}

    #[derive(serde::Serialize, serde::Deserialize, navree::openapi::ToSchema)]
    struct Always {}

    #[condition(type = "always")]
    fn always(_config: &Always, _ctx: &AutomationCtx) -> bool {
        true
    }

    #[derive(serde::Serialize, serde::Deserialize, navree::openapi::ToSchema)]
    struct Ping {}

    #[action(type = "ping", label = "Ping")]
    async fn ping(_config: Ping, _ctx: AutomationCtx) -> Result<String, BoxError> {
        Ok("pong".into())
    }

    assert_eq!(<Welcome as Workflow>::NAME, "welcome");
    assert_eq!(<Welcome as Workflow>::VERSION, 2);
    assert_eq!(<Welcome as Workflow>::DESCRIPTION, "Welcomes a user.");
    assert_eq!(<SignedTrigger as Trigger>::SUBJECT, Some("user_id"));
    assert_eq!(<Always as Condition>::TYPE, "always");
    assert_eq!(<Ping as Action>::LABEL, "Ping");
}

/// A migration file as `navree make:migration` writes it: the schema
/// builder through `navree::schema`, registered with `migration!`.
mod m20261027_120000_create_events_table {
    use navree::schema::prelude::*;

    pub struct Migration;

    impl navree::schema::Migration for Migration {
        async fn up(&self, schema: &Schema<'_>) -> SchemaResult {
            schema
                .create_hypertable("events", "occurred_at", |t| {
                    t.uuid_primary();
                    t.string("kind");
                    t.timestamp_tz("occurred_at");
                })
                .await
        }

        async fn down(&self, schema: &Schema<'_>) -> SchemaResult {
            schema.drop_if_exists("events").await
        }
    }
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn schema_migrations_run_through_the_facade() {
    struct Events;

    impl Module for Events {
        fn register(&self, m: &mut ModuleBuilder) {
            m.migrations(
                MigrationSet::new("events").migration(navree::schema::migration!(
                    m20261027_120000_create_events_table
                )),
            );
        }
    }

    let dir = tempfile::Builder::new()
        .prefix("navree-test-")
        .tempdir()
        .unwrap();
    let url = format!("sqlite://{}?mode=rwc", dir.path().join("app.db").display());
    let app = Navree::app()
        .module(DatabaseModule::for_root(navree::db::DbOptions {
            url,
            ..Default::default()
        }))
        .module(Events)
        .boot()
        .await
        .unwrap();
    let report = navree::db::Migrator::run(app.ctx(), &navree::db::MigrateOptions::default())
        .await
        .unwrap();
    assert_eq!(
        report.labels(),
        ["events/m20261027_120000_create_events_table"]
    );
    assert!(
        report.migrations[0].notices[1].starts_with("SQLite does not support TimescaleDB"),
        "{:?}",
        report.migrations[0].notices
    );
}

/// Runs `cargo metadata --no-deps` on the workspace.
fn workspace_metadata() -> Value {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
    let output = Command::new(cargo)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
        ])
        .arg("--manifest-path")
        .arg(&manifest)
        .output()
        .expect("failed to run `cargo metadata`");
    assert!(
        output.status.success(),
        "`cargo metadata` failed:
{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("`cargo metadata` printed invalid JSON")
}

/// The facade's features as read from its manifest.
struct Features {
    /// Feature name -> its list (features and `dep:` items).
    table: BTreeMap<String, Vec<String>>,
    /// Optional navree crate -> the feature that turns it on.
    feature_of: BTreeMap<String, String>,
    /// Workspace package name -> its normal (runtime) navree dependencies.
    runtime_deps: BTreeMap<String, Vec<String>>,
}

/// Features left out of `full` on purpose: development tools an app's
/// runtime never needs (`dock`, `harbor`, `navigator`), and ecosystem
/// libraries and packages an app turns on by name (`features = ["social"]`,
/// `features = ["oauth"]`, `features = ["flags"]`, `features = ["graphql"]`,
/// `features = ["grpc"]`, `features = ["documents"]`, `features = ["media"]`,
/// `features = ["billing"]`,
/// `features = ["search"]`, `features = ["mcp"]`, `features = ["ai"]`,
/// `features = ["redis"]`, `features = ["micro"]`, `features = ["i18n"]`,
/// `features = ["mongodb"]`), Helm's whole icon set (`helm-all-icons`), and test-only
/// crates (`features = ["browser"]` in `[dev-dependencies]`).
const OPT_IN: &[&str] = &[
    "dock",
    "harbor",
    "navigator",
    "helm-all-icons",
    "social",
    "oauth",
    "flags",
    "graphql",
    "grpc",
    "documents",
    "documents-native",
    "documents-chrome",
    "documents-docx",
    "pdf",
    "media",
    "billing",
    "search",
    "mcp",
    "ai",
    "redis",
    "micro",
    "i18n",
    "browser",
    "mongo",
    "mongodb",
];

/// Always compiled into the facade (not behind a feature).
const ALWAYS: [&str; 8] = [
    "navree-kernel",
    "navree-bounded",
    "navree-glob",
    "navree-contracts",
    "navree-config",
    "navree-macros",
    "navree-openapi",
    "navree-i18n-macros",
];

/// Proc-macro crates that a module crate re-exports, as (macro crate,
/// module crate): they come with the module crate's feature (`orm` brings
/// navree-orm-macros through navree-orm, `ui` navree-ui-macros through navree-ui),
/// so the facade does not name them.
const COMPANION_MACROS: [(&str, &str); 3] = [
    ("navree-orm-macros", "navree-orm"),
    ("navree-ui-macros", "navree-ui"),
    ("navree-validation-macros", "navree-validation"),
];

impl Features {
    fn read() -> Features {
        let metadata = workspace_metadata();
        let packages = metadata["packages"].as_array().expect("packages");
        let facade = packages
            .iter()
            .find(|p| p["name"] == "navree")
            .expect("the navree package");
        let table: BTreeMap<String, Vec<String>> = facade["features"]
            .as_object()
            .expect("features")
            .iter()
            .map(|(name, list)| {
                let items = list
                    .as_array()
                    .expect("a feature is a list")
                    .iter()
                    .map(|item| item.as_str().expect("a string").to_owned())
                    .collect();
                (name.clone(), items)
            })
            .collect();
        let feature_of = table
            .iter()
            .flat_map(|(name, items)| {
                items.iter().filter_map(move |item| {
                    item.strip_prefix("dep:")
                        .filter(|dep| dep.starts_with("navree-"))
                        .map(|dep| (dep.to_owned(), name.clone()))
                })
            })
            .collect();
        let runtime_deps = packages
            .iter()
            .map(|package| {
                // An optional dependency is part of the crate only when its
                // default features turn it on (navree-workspace's
                // `testing` is not; the facade's `testing` turns it on).
                let by_default = default_optional_deps(package);
                let deps = package["dependencies"]
                    .as_array()
                    .expect("dependencies")
                    .iter()
                    .filter(|dep| dep["kind"].is_null())
                    .filter(|dep| {
                        dep["optional"] != true
                            || dep["name"].as_str().is_some_and(|n| by_default.contains(n))
                    })
                    .filter_map(|dep| dep["name"].as_str())
                    .filter(|name| name.starts_with("navree-"))
                    .map(str::to_owned)
                    .collect();
                (package["name"].as_str().expect("name").to_owned(), deps)
            })
            .collect();
        Features {
            table,
            feature_of,
            runtime_deps,
        }
    }

    /// Every feature `feature` turns on, itself included (transitively).
    fn closure(&self, feature: &str) -> BTreeSet<String> {
        let mut seen = BTreeSet::new();
        let mut stack = vec![feature.to_owned()];
        while let Some(next) = stack.pop() {
            if !seen.insert(next.clone()) {
                continue;
            }
            for item in self.table.get(&next).into_iter().flatten() {
                if self.table.contains_key(item) {
                    stack.push(item.clone());
                }
            }
        }
        seen
    }
}

#[test]
fn every_navree_dependency_is_always_on_or_behind_its_own_feature() {
    let features = Features::read();
    let mut problems = Vec::new();
    for dep in &features.runtime_deps["navree"] {
        if !ALWAYS.contains(&dep.as_str()) && !features.feature_of.contains_key(dep) {
            problems.push(format!("{dep} is neither always on nor behind a feature"));
        }
    }
    for (krate, feature) in &features.feature_of {
        let expected = krate.trim_start_matches("navree-");
        if feature != expected {
            problems.push(format!(
                "{krate} is behind `{feature}`, expected `{expected}`"
            ));
        }
    }
    assert!(problems.is_empty(), "{problems:#?}");
}

/// The optional dependencies a package's `default` feature turns on
/// (transitively): `dep:name`, `name/feature` and implicit `name` features.
/// A weak `name?/feature` turns nothing on.
fn default_optional_deps(package: &Value) -> BTreeSet<String> {
    let table = package["features"].as_object();
    let mut seen = BTreeSet::new();
    let mut deps = BTreeSet::new();
    let mut stack = vec!["default".to_owned()];
    while let Some(feature) = stack.pop() {
        if !seen.insert(feature.clone()) {
            continue;
        }
        let items = table
            .and_then(|t| t.get(&feature))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str);
        for item in items {
            if let Some(dep) = item.strip_prefix("dep:") {
                deps.insert(dep.to_owned());
            } else if let Some((dep, _)) = item.split_once('/') {
                if !dep.ends_with('?') {
                    deps.insert(dep.to_owned());
                }
            } else {
                // Another feature, or the implicit feature of an optional
                // dependency of that name.
                deps.insert(item.to_owned());
                stack.push(item.to_owned());
            }
        }
    }
    deps
}

#[test]
fn optional_dependencies_count_when_default_features_turn_them_on() {
    let package = serde_json::json!({
        "features": {
            "default": ["testing", "postgres"],
            "testing": ["dep:navree-testing", "navree-view?/mail"],
            "postgres": ["navree-db/postgres"],
            "extra": ["dep:navree-cache"],
        }
    });
    let on = default_optional_deps(&package);
    assert!(on.contains("navree-testing"));
    assert!(on.contains("navree-db"));
    assert!(
        !on.contains("navree-view"),
        "a weak feature turns nothing on"
    );
    assert!(!on.contains("navree-cache"));
    assert!(default_optional_deps(&serde_json::json!({ "features": {} })).is_empty());
}

#[test]
fn module_features_turn_on_the_features_of_their_navree_dependencies() {
    let features = Features::read();
    let mut missing = Vec::new();
    for (krate, feature) in &features.feature_of {
        let enabled = features.closure(feature);
        for dep in &features.runtime_deps[krate] {
            if ALWAYS.contains(&dep.as_str())
                || COMPANION_MACROS.contains(&(dep.as_str(), krate.as_str()))
            {
                continue;
            }
            match features.feature_of.get(dep) {
                Some(needed) if enabled.contains(needed) => {}
                Some(needed) => missing.push(format!(
                    "`{feature}` must turn on `{needed}`: {krate} depends on {dep}"
                )),
                None => missing.push(format!(
                    "no facade feature turns on {dep} (used by {krate})"
                )),
            }
        }
    }
    assert!(missing.is_empty(), "{missing:#?}");
}

/// The other direction: a module feature turns on nothing its crate does
/// not build on, so `default-features = false, features = ["auth"]` pulls
/// in only what navree-auth needs.
#[test]
fn module_features_turn_on_only_the_features_of_their_navree_dependencies() {
    let features = Features::read();
    let mut extra = Vec::new();
    for (krate, feature) in &features.feature_of {
        let needed: BTreeSet<&str> = features.runtime_deps[krate]
            .iter()
            .filter_map(|dep| features.feature_of.get(dep))
            .map(String::as_str)
            .collect();
        for item in &features.table[feature] {
            if features.table.contains_key(item) && !needed.contains(item.as_str()) {
                extra.push(format!(
                    "`{feature}` turns on `{item}`, but {krate} does not depend on its crate"
                ));
            }
        }
    }
    assert!(extra.is_empty(), "{extra:#?}");
}

#[test]
fn full_is_the_default_and_turns_on_every_module_but_the_opt_in_ones() {
    let features = Features::read();
    assert_eq!(features.table["default"], ["full"]);
    let full = features.closure("full");
    let missing: Vec<&String> = features
        .feature_of
        .values()
        .filter(|feature| !full.contains(*feature) && !OPT_IN.contains(&feature.as_str()))
        .collect();
    assert!(missing.is_empty(), "`full` lacks {missing:?}");
    for feature in OPT_IN {
        assert!(
            features.table.contains_key(*feature),
            "opt-in feature `{feature}` is gone"
        );
        assert!(
            !full.contains(*feature),
            "`full` turns on opt-in `{feature}`"
        );
    }
}

/// The storage drivers are features of `storage`: S3 is in `full`, Google
/// Cloud Storage and Azure are opt-in, and each turns on navree-storage's
/// own feature.
#[test]
fn storage_drivers_are_features() {
    let features = Features::read();
    let full = features.closure("full");
    assert!(full.contains("storage-s3"), "`full` has S3 disks");
    for (feature, driver) in [
        ("storage-s3", "s3"),
        ("storage-gcs", "gcs"),
        ("storage-azure", "azure"),
    ] {
        let items = &features.table[feature];
        assert!(
            items.contains(&"storage".to_owned()),
            "{feature}: {items:?}"
        );
        assert!(
            items.contains(&format!("navree-storage/{driver}")),
            "{feature}: {items:?}"
        );
    }
    for opt_in in ["storage-gcs", "storage-azure"] {
        assert!(!full.contains(opt_in), "`full` turns on `{opt_in}`");
    }
}

/// With `full`, an s3 disk is available through the facade.
#[cfg(feature = "storage-s3")]
#[test]
fn s3_disks_are_built_through_the_facade() {
    use navree::storage::{DiskConfig, S3Config, Storage, StorageOptions};

    let options = StorageOptions {
        default: "uploads".into(),
        disks: [(
            "uploads".to_owned(),
            DiskConfig::from(
                S3Config::new("agra")
                    .endpoint("http://127.0.0.1:9")
                    .credentials("key", "secret"),
            ),
        )]
        .into(),
        ..StorageOptions::default()
    };
    let storage = Storage::new(&options, b"key").unwrap();
    assert_eq!(storage.default_disk().driver(), "s3");
}

/// `navree::dock` is navree-dock, with the opt-in `dock` feature
/// (`cargo test -p navree --features dock`).
#[cfg(feature = "dock")]
#[test]
fn dock_is_reexported_with_its_feature() {
    let file = navree::dock::DockFile::parse("[services.redis]\n", "shop").unwrap();
    assert_eq!(navree::dock::DOCK_FILE, "dock.toml");
    assert_eq!(file.env()[0].1, "redis://127.0.0.1:6379");
}

/// `validation-phone` and `validation-html` add navree-validation's
/// `Phone` and `SafeHtml`, and `full` turns both on.
#[test]
fn full_brings_the_international_validation_types() {
    let features = Features::read();
    let full = features.closure("full");
    for feature in ["validation-phone", "validation-html"] {
        assert!(full.contains(feature), "`full` lacks `{feature}`");
        assert!(
            features.closure(feature).contains("validation"),
            "`{feature}` must turn on `validation`"
        );
    }
}

/// The parts of `documents` are features of their own, outside `full`: each
/// turns on the module's support crates and one engine of navree-documents, and
/// `documents` is all three.
#[test]
fn documents_has_a_feature_for_each_of_its_parts() {
    let features = Features::read();
    let full = features.closure("full");
    for (part, engine) in [
        ("documents-native", "navree-documents/native"),
        ("documents-chrome", "navree-documents/chrome"),
        ("documents-docx", "navree-documents/docx"),
    ] {
        let list = features
            .table
            .get(part)
            .unwrap_or_else(|| panic!("the facade has no `{part}` feature"));
        assert!(
            list.iter().any(|item| item == engine),
            "`{part}` must turn on `{engine}`: {list:?}"
        );
        assert!(!full.contains(part), "`full` turns on `{part}`");
        assert!(
            features.closure(part).contains("pdf"),
            "`{part}` must turn on `pdf`"
        );
        assert!(
            features.table["documents"]
                .iter()
                .any(|item| item == engine),
            "`documents` must turn on `{engine}`"
        );
    }
}

#[cfg(all(feature = "validation-phone", feature = "validation-html"))]
#[test]
fn phone_and_safe_html_are_reexported() {
    let phone = navree::validation::Phone::parse("+44 7400 123456", None).unwrap();
    assert_eq!(phone.as_str(), "+447400123456");
    let html = navree::validation::SafeHtml::new("<b onclick=\"x()\">bold</b>");
    assert_eq!(html.as_str(), "<b>bold</b>");
}

/// `navree::harbor` is navree-harbor, with `--features harbor`.
#[cfg(feature = "harbor")]
#[test]
fn harbor_is_reexported_with_its_feature() {
    let config = navree::harbor::HarborConfig::parse(
        "app = \"shop\"\n\n[servers.app-1]\nhost = \"203.0.113.10\"\n",
        std::path::Path::new("harbor.toml"),
    );
    assert!(config.is_ok(), "{config:?}");
    assert!(same::<
        navree::harbor::cli::HarborCommand,
        navree::harbor::HarborCommand,
    >());
}

/// `navree::browser` is navree-browser, with the opt-in `browser` feature
/// (`cargo test -p navree --features browser --test facade`).
#[cfg(feature = "browser")]
#[test]
fn browser_is_reexported_with_its_feature() {
    let target = navree::browser::Target::from("@deal-card").nth(1);
    assert_eq!(target.to_string(), r#""@deal-card" nth 1"#);
    assert_eq!(navree::browser::Viewport::phone().width(), 390);
    let _launch = navree::browser::Browser::builder().window(1440, 900);
}

/// The opt-in ecosystem libraries are re-exported whole too.
#[cfg(feature = "social")]
#[test]
fn opt_in_libraries_are_reexported_whole() {
    assert!(same::<navree::social::SocialModule, SocialModule>());
    let (_social, _fake): (navree::social::Social, navree::social::SocialFake) =
        navree::social::Social::fake();
}

/// `navree::mongo` is navree-mongo re-exported whole, and the `mongo`
/// feature (also called `mongodb`) puts `MongoModule` and `MongoDb` in the
/// prelude (`cargo test -p navree --features mongodb --test facade`).
#[cfg(feature = "mongo")]
#[test]
fn mongo_is_reexported_with_its_feature() {
    assert!(same::<navree::mongo::MongoModule, MongoModule>());
    assert!(same::<navree::mongo::MongoDb, MongoDb>());
    assert_eq!(
        navree::mongo::MongoOptions::default().url,
        "mongodb://127.0.0.1:27017/navree"
    );
    assert!(!navree::mongo::ObjectId::new().is_nil());
}

/// `navree::redis` is navree-redis re-exported whole, and the `redis`
/// feature puts `RedisModule` in the prelude
/// (`cargo test -p navree --features redis --test facade`).
#[cfg(feature = "redis")]
#[test]
fn redis_is_reexported_with_its_feature() {
    assert!(same::<navree::redis::RedisModule, RedisModule>());
    assert_eq!(navree::redis::DRIVER, "redis");
    assert_eq!(navree::redis::RedisOptions::default().prefix, "navree:");
}

/// `navree::flags` is navree-flags re-exported whole, and the `flags`
/// feature puts its module and extractor in the prelude
/// (`cargo test -p navree --features flags`).
#[cfg(feature = "flags")]
#[test]
fn flags_is_reexported_with_its_feature() {
    assert!(same::<navree::flags::FlagsModule, FlagsModule>());
    assert!(same::<navree::flags::CurrentFlags, CurrentFlags>());
}

/// `navree::i18n` is navree-i18n re-exported whole, and the `i18n` feature
/// puts `I18nModule` and `Locale` in the prelude
/// (`cargo test -p navree --features i18n`).
#[cfg(feature = "i18n")]
#[test]
fn i18n_is_reexported_with_its_feature() {
    assert!(same::<navree::i18n::I18nModule, I18nModule>());
    assert!(same::<navree::i18n::Locale, Locale>());
    let args = navree::i18n::args!(count = 1);
    assert_eq!(args.len(), 1);
}

/// Every database driver is a feature of its own, outside `full`, that
/// turns on `db` and the matching feature of navree-db
/// (`features = ["sqlite"]`, `features = ["postgres"]`,
/// `features = ["mysql"]`): a build with one compiles none of the others
/// (`tests/dependency_graph.rs` checks the graph). There is no `mysql-rsa`:
/// the crypto policy keeps the `rsa` crate out.
#[test]
fn database_driver_features_turn_on_db_and_the_driver() {
    let features = Features::read();
    let full = features.closure("full");
    for (feature, driver) in [
        ("sqlite", "navree-db/sqlite"),
        ("postgres", "navree-db/postgres"),
        ("mysql", "navree-db/mysql"),
    ] {
        let list = features
            .table
            .get(feature)
            .unwrap_or_else(|| panic!("the facade has no `{feature}` feature"));
        let enabled = features.closure(feature);
        assert!(enabled.contains("db"), "`{feature}` must turn on `db`");
        assert!(
            list.iter().any(|item| item == driver),
            "`{feature}` must turn on `{driver}`: {list:?}"
        );
        assert!(!full.contains(feature), "`full` turns on `{feature}`");
    }
    assert!(
        !features.table.contains_key("mysql-rsa"),
        "`mysql-rsa` would pull in the `rsa` crate"
    );
}

/// `navree::micro` is navree-micro re-exported whole, and the `micro`
/// feature puts its module, client and payload in the prelude
/// (`cargo test -p navree --features micro --test facade`).
#[cfg(feature = "micro")]
#[test]
fn micro_is_reexported_with_its_feature() {
    assert!(same::<navree::micro::MicroModule, MicroModule>());
    assert!(same::<navree::micro::ServiceClient, ServiceClient>());
    assert!(same::<navree::micro::Payload<u8>, Payload<u8>>());
    let fake = navree::micro::MicroFake::new();
    assert_eq!(
        fake.transport().kind(),
        Some(navree::micro::TransportKind::Custom)
    );
}

/// `navree::documents` is navree-documents re-exported whole, and the
/// `documents` feature puts its module in the prelude
/// (`cargo test -p navree --features documents`).
#[cfg(feature = "documents")]
#[test]
fn documents_is_reexported_with_its_feature() {
    assert!(same::<navree::documents::DocumentsModule, DocumentsModule>());
    let pdf = navree::documents::Pdf::from_lines(["Invoice 42"]);
    assert_eq!(pdf.page_count().unwrap(), 1);
}

/// `UiModule` comes with the prelude when `ui` is on, and the country flags have a name
/// that cannot be taken for `navree::flags`, the feature flags
/// (`cargo test -p navree --test facade`).
#[cfg(feature = "ui")]
#[test]
fn ui_module_is_in_the_prelude_and_country_flags_have_their_own_name() {
    assert!(same::<UiModule, navree::ui::UiModule>());
    assert!(same::<
        navree::ui::country_flags::CountryCode,
        navree::ui::flags::CountryCode,
    >());
    assert_eq!(navree::ui::country_flags::IN.to_string(), "IN");
}

/// `navree::plugin` is the kernel's plugin types, and `navree::plugin_info!` describes the
/// crate that calls it.
#[test]
fn the_plugin_module_brings_the_kernels_plugin_types() {
    assert!(same::<navree::plugin::PluginInfo, navree::kernel::PluginInfo>());
    assert!(same::<
        navree::plugin::PluginsReport,
        navree::kernel::PluginsReport,
    >());
    assert!(same::<
        navree::plugin::PluginReport,
        navree::kernel::PluginReport,
    >());
    assert!(same::<
        navree::plugin::PluginModuleReport,
        navree::kernel::PluginModuleReport,
    >());
    assert!(same::<
        navree::plugin::Publishable,
        navree::kernel::Publishable,
    >());
    assert!(same::<navree::plugin::PublishTag, navree::kernel::PublishTag>());
    let info = navree::plugin_info!();
    assert_eq!(
        (info.name, info.crate_name, info.namespace),
        ("navree", "facade", "navree")
    );
}

/// `navree::embed_locales!` embeds a crate's Fluent files without the `i18n` feature. `full`,
/// the default this file needs, leaves `i18n` out (see
/// `full_is_the_default_and_turns_on_every_module_but_the_opt_in_ones`); with `i18n` turned on
/// the test would prove nothing, so it is left out.
#[test]
#[cfg_attr(
    feature = "i18n",
    ignore = "proves the facade without its i18n feature; run it with the default features"
)]
fn embed_locales_works_through_the_facade_without_i18n() {
    let files: &'static [(&str, &str, &str)] =
        navree::embed_locales!("tests/fixtures/plugin-locales");
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].0, "en");
    assert!(files[0].2.contains("₹1,00,000"));
}

/// The kernel's first-party list is the `kind = "module"` packages of the first-party index,
/// each behind a facade feature of its own name.
#[test]
fn the_kernels_first_party_list_matches_the_cli_index() {
    assert!(
        !std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../navree-cli/src/plugin/index.toml")
            .exists(),
        "index.toml exists: compare FIRST_PARTY and RESERVED_NAMESPACES with it instead of the pinned rows"
    );
    // The `kind = "module"` packages of the first-party index, pinned until
    // crates/navree-cli/src/plugin/index.toml exists.
    const INDEXED: [&str; 31] = [
        "ai",
        "audit",
        "billing",
        "directory",
        "documents",
        "domains",
        "flags",
        "fleet",
        "graphql",
        "grpc",
        "helm",
        "http-client",
        "i18n",
        "integrations",
        "mcp",
        "media",
        "micro",
        "mongo",
        "notify",
        "oauth",
        "realtime",
        "redis",
        "search",
        "settings",
        "sheets",
        "social",
        "sso",
        "storage",
        "time",
        "webhooks",
        "workflows",
    ];
    let listed: Vec<(String, String)> = navree::kernel::__private::FIRST_PARTY
        .iter()
        .map(|(package, krate)| ((*package).to_owned(), (*krate).to_owned()))
        .collect();
    let expected: Vec<(String, String)> = INDEXED
        .iter()
        .map(|name| {
            (
                format!("navree-{name}"),
                format!("navree_{}", name.replace('-', "_")),
            )
        })
        .collect();
    assert_eq!(listed, expected);
    let features = Features::read();
    for (package, _) in &listed {
        assert_eq!(
            features.feature_of.get(package).map(String::as_str),
            Some(package.trim_start_matches("navree-")),
            "{package} is behind its own facade feature"
        );
    }
}

/// The namespaces the kernel keeps for the framework are the first words of the framework's
/// configuration sections, a few words of its own, the folders the framework reads below the
/// plugins' folders and the Windows device names, and they hold every permission prefix a
/// framework crate declares.
#[test]
fn the_reserved_namespaces_match_the_framework_sections_and_permission_prefixes() {
    assert!(
        !std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../navree-cli/src/plugin/index.toml")
            .exists(),
        "index.toml exists: compare FIRST_PARTY and RESERVED_NAMESPACES with it instead of the pinned rows"
    );
    let reserved: BTreeSet<&str> = navree::kernel::__private::RESERVED_NAMESPACES
        .iter()
        .copied()
        .collect();
    let mut expected: BTreeSet<&str> = navree::config::FRAMEWORK_SECTIONS
        .iter()
        .map(|key| key.split('.').next().unwrap())
        .collect();
    expected.extend(["navree", "views", "roles", "env", "plugins"]);
    expected.extend([
        "all",
        "users",
        "organization",
        "subscriptions",
        "operations",
    ]);
    // Folders the framework reads below the plugins' folders: `layouts` for templates,
    // `fonts` for documents.
    expected.extend(["layouts", "fonts"]);
    // The Windows device names a namespace could spell: no folder may be called one.
    expected.extend(["con", "prn", "aux", "nul"]);
    expected.extend([
        "com0", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9",
    ]);
    expected.extend([
        "lpt0", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
    ]);
    assert_eq!(reserved, expected);
    // Every permission prefix a framework crate declares in its source is reserved.
    for (file, prefix) in permission_prefixes() {
        let word = prefix.split('.').next().unwrap();
        assert!(reserved.contains(word), "{file}: `{prefix}`");
    }
}

/// Every permission prefix declared in `crates/navree-*/src/**/*.rs`, but for navree-cli,
/// navree-macros and navree-orm-macros, with its file: the text after
/// `PERMISSION_PREFIX: &'static str = "` or after `permission_prefix = "`, up to the next `"`,
/// on a line that is not a comment. (Every first-party `#[model(subject ..)]` sets
/// `permission_prefix` today; one that relied on its table as the default would escape this
/// scan.)
fn permission_prefixes() -> Vec<(String, String)> {
    let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    for entry in std::fs::read_dir(&crates).expect("the crates folder") {
        let entry = entry.expect("a folder entry");
        let name = entry.file_name().to_string_lossy().into_owned();
        let left_out =
            ["navree-cli", "navree-macros", "navree-orm-macros"].contains(&name.as_str());
        let src = entry.path().join("src");
        if name.starts_with("navree-") && !left_out && src.is_dir() {
            rust_files(&src, &mut files);
        }
    }
    files.sort();
    let mut prefixes = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file).expect("a source file");
        let shown = file
            .strip_prefix(&crates)
            .unwrap_or(&file)
            .display()
            .to_string();
        for line in text
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
        {
            for needle in [
                "PERMISSION_PREFIX: &'static str = \"",
                "permission_prefix = \"",
            ] {
                for (at, _) in line.match_indices(needle) {
                    let rest = &line[at + needle.len()..];
                    let prefix = rest.split('"').next().unwrap_or_default();
                    prefixes.push((shown.clone(), prefix.to_owned()));
                }
            }
        }
    }
    prefixes
}

/// Every `.rs` file below `folder`, at any depth.
fn rust_files(folder: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(folder).expect("a source folder") {
        let path = entry.expect("a folder entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            out.push(path);
        }
    }
}

/// Whether `error` comes from the boot checks of plugins.
fn is_plugin_error(error: &KernelError) -> bool {
    matches!(
        error,
        KernelError::PluginTwice { .. }
            | KernelError::InvalidNamespace { .. }
            | KernelError::PluginVersions { .. }
            | KernelError::NamespaceTaken { .. }
            | KernelError::PublishableOutsidePlugin { .. }
            | KernelError::UnsafePublishPath { .. }
            | KernelError::NamespaceReserved { .. }
            | KernelError::PluginOfAnotherCrate { .. }
            | KernelError::PluginInfoDiffers { .. }
    )
}

/// Every module the facade's features bring, each in an app of its own, passes the boot
/// checks of plugins, and a plugin it names is first-party: a framework package that named
/// itself a plugin while off the kernel's first-party list, or from a module type of another
/// crate (a `ClientModule`, say), would fail every app's boot in its reserved namespace. A
/// module that needs other modules does not validate alone; it must not fail for a plugin.
#[test]
fn every_framework_module_passes_the_plugin_checks_and_names_only_first_party_plugins() {
    // The opt-in modules are added only with their features.
    #[allow(unused_mut)]
    let mut apps: Vec<(&str, AppBuilder)> = vec![
        (
            "ConfigModule",
            Navree::app().module(ConfigModule::for_root(ConfigSources::new())),
        ),
        (
            "LoggerModule",
            Navree::app().module(LoggerModule::from_config()),
        ),
        (
            "CryptoModule",
            Navree::app().module(CryptoModule::from_config()),
        ),
        ("HttpModule", Navree::app().module(HttpModule::from_config())),
        (
            "HttpClientModule",
            Navree::app().module(HttpClientModule::from_config()),
        ),
        (
            "DatabaseModule",
            Navree::app().module(DatabaseModule::from_config()),
        ),
        ("OrmModule", Navree::app().module(OrmModule::from_config())),
        (
            "CacheModule",
            Navree::app().module(CacheModule::from_config()),
        ),
        ("EventsModule", Navree::app().module(EventsModule::for_root())),
        ("ViewModule", Navree::app().module(ViewModule::from_config())),
        ("UiModule", Navree::app().module(UiModule::from_config())),
        ("TimeModule", Navree::app().module(TimeModule::from_config())),
        ("MailModule", Navree::app().module(MailModule::from_config())),
        (
            "QueueModule",
            Navree::app().module(QueueModule::from_config()),
        ),
        (
            "ScheduleModule",
            Navree::app().module(ScheduleModule::for_root()),
        ),
        (
            "ValidationModule",
            Navree::app().module(ValidationModule::from_config()),
        ),
        (
            "StorageModule",
            Navree::app().module(StorageModule::from_config()),
        ),
        ("AuthModule", Navree::app().module(AuthModule::from_config())),
        (
            "AccessModule",
            Navree::app().module(AccessModule::for_root(AccessOptions::default())),
        ),
        (
            "WorkspaceModule",
            Navree::app().module(WorkspaceModule::from_config()),
        ),
        (
            "SessionModule",
            Navree::app().module(SessionModule::from_config()),
        ),
        (
            "AuthPages",
            Navree::app().module(navree::auth::AuthPages::new()),
        ),
        (
            "AuditModule",
            Navree::app().module(AuditModule::from_config()),
        ),
        (
            "NotifyModule",
            Navree::app().module(NotifyModule::from_config()),
        ),
        (
            "RealtimeModule",
            Navree::app().module(RealtimeModule::from_config()),
        ),
        (
            "SettingsModule",
            Navree::app().module(SettingsModule::from_config()),
        ),
        (
            "WebhooksModule",
            Navree::app().module(WebhooksModule::from_config()),
        ),
        (
            "IntegrationsModule",
            Navree::app().module(IntegrationsModule::from_config()),
        ),
        (
            "DirectoryModule",
            Navree::app().module(DirectoryModule::from_config()),
        ),
        (
            "DomainsModule",
            Navree::app().module(DomainsModule::from_config()),
        ),
        ("SsoModule", Navree::app().module(SsoModule::from_config())),
        (
            "SheetsModule",
            Navree::app().module(SheetsModule::from_config()),
        ),
        (
            "WorkflowsModule",
            Navree::app().module(WorkflowsModule::from_config()),
        ),
        ("ApiModule", Navree::app().module(ApiModule::from_config())),
        (
            "ObserveModule",
            Navree::app().module(ObserveModule::from_config()),
        ),
        ("HelmModule", Navree::app().module(HelmModule::from_config())),
        (
            "LighthouseModule",
            Navree::app().module(LighthouseModule::from_config()),
        ),
        (
            "LogbookModule",
            Navree::app().module(LogbookModule::from_config()),
        ),
        (
            "FleetModule",
            Navree::app().module(FleetModule::from_config()),
        ),
    ];
    #[cfg(feature = "redis")]
    apps.push((
        "RedisModule",
        Navree::app().module(RedisModule::from_config()),
    ));
    #[cfg(feature = "mongo")]
    apps.push((
        "MongoModule",
        Navree::app().module(MongoModule::from_config()),
    ));
    #[cfg(feature = "search")]
    apps.push((
        "SearchModule",
        Navree::app().module(SearchModule::from_config()),
    ));
    #[cfg(feature = "billing")]
    apps.push((
        "BillingModule",
        Navree::app().module(BillingModule::from_config()),
    ));
    #[cfg(feature = "social")]
    apps.push((
        "SocialModule",
        Navree::app().module(SocialModule::from_config()),
    ));
    #[cfg(feature = "flags")]
    apps.push((
        "FlagsModule",
        Navree::app().module(FlagsModule::from_config()),
    ));
    #[cfg(feature = "oauth")]
    apps.push((
        "OAuthServerModule",
        Navree::app().module(OAuthServerModule::from_config()),
    ));
    #[cfg(feature = "documents")]
    apps.push((
        "DocumentsModule",
        Navree::app().module(DocumentsModule::from_config()),
    ));
    #[cfg(feature = "media")]
    apps.push((
        "MediaModule",
        Navree::app().module(MediaModule::from_config()),
    ));
    #[cfg(feature = "i18n")]
    apps.push((
        "I18nModule",
        Navree::app().module(I18nModule::from_config()),
    ));
    #[cfg(feature = "ai")]
    apps.push(("AiModule", Navree::app().module(AiModule::from_config())));
    #[cfg(feature = "graphql")]
    apps.push((
        "GraphqlModule",
        Navree::app().module(GraphqlModule::from_config()),
    ));
    #[cfg(feature = "grpc")]
    apps.push((
        "GrpcModule",
        Navree::app().module(GrpcModule::from_config()),
    ));
    #[cfg(feature = "micro")]
    apps.push((
        "MicroModule",
        Navree::app().module(MicroModule::from_config()),
    ));
    #[cfg(feature = "mcp")]
    apps.push(("McpModule", Navree::app().module(McpModule::from_config())));
    for (name, app) in apps {
        match app.plugin_report() {
            Ok(report) => {
                for plugin in &report.plugins {
                    assert!(
                        plugin.first_party,
                        "{name} names the plugin `{}`, which the kernel does not find first-party",
                        plugin.info.name
                    );
                }
            }
            Err(errors) => {
                for error in &errors.0 {
                    assert!(!is_plugin_error(error), "{name}: {error}");
                }
            }
        }
    }
}

/// Only the packages on the kernel's first-party list name themselves plugins, and each from
/// a module type of its own crate, never through a `ClientModule` (whose type is another
/// crate's, so the plugin would not be first-party). Read from the source of every
/// `crates/navree-*/src`, but for navree-kernel, which defines plugins, and navree-cli and
/// navree-macros, which write plugin code for apps.
#[test]
fn only_first_party_packages_name_themselves_plugins_and_from_their_own_module_types() {
    let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut problems = Vec::new();
    for entry in std::fs::read_dir(&crates).expect("the crates folder") {
        let entry = entry.expect("a folder entry");
        let package = entry.file_name().to_string_lossy().into_owned();
        let left_out = ["navree-kernel", "navree-cli", "navree-macros"].contains(&package.as_str());
        let src = entry.path().join("src");
        if !package.starts_with("navree-") || left_out || !src.is_dir() {
            continue;
        }
        let mut files = Vec::new();
        rust_files(&src, &mut files);
        for file in files {
            let text = std::fs::read_to_string(&file).expect("a source file");
            let names_a_plugin = text
                .lines()
                .filter(|line| !line.trim_start().starts_with("//"))
                .any(|line| line.contains("plugin_info!") || line.contains("PluginInfo::new("));
            if !names_a_plugin {
                continue;
            }
            let listed = navree::kernel::__private::FIRST_PARTY
                .iter()
                .any(|(name, _)| *name == package);
            if !listed {
                problems.push(format!(
                    "{}: {package} names itself a plugin but is not on the kernel's first-party list",
                    file.display()
                ));
            }
            if text.contains("ClientModule") {
                problems.push(format!(
                    "{}: a first-party package names itself a plugin from a module type of its own crate, not through ClientModule",
                    file.display()
                ));
            }
        }
    }
    assert!(problems.is_empty(), "{problems:#?}");
}

/// A module that takes a first-party module type's id and names a plugin by hand.
struct Probe(ModuleId, navree::plugin::PluginInfo);

impl Module for Probe {
    fn id(&self) -> ModuleId {
        self.0
    }

    fn register(&self, m: &mut ModuleBuilder) {
        m.plugin(self.1.clone());
    }
}

/// A module type whose path names a first-party crate only in a generic argument.
struct Holder<T>(std::marker::PhantomData<T>);

/// Whether an app of `probe` alone reports its plugin first-party, or the text of its boot
/// errors.
fn first_party(probe: Probe) -> Result<bool, String> {
    Navree::app()
        .module(probe)
        .plugin_report()
        .map(|report| report.plugins[0].first_party)
        .map_err(|errors| errors.to_string())
}

#[test]
fn only_the_indexed_crates_at_the_kernels_version_and_in_their_own_crate_are_first_party() {
    use navree::plugin::PluginInfo;
    let version = env!("CARGO_PKG_VERSION");
    let time = ModuleId::of::<navree::time::TimeModule>();
    assert_eq!(
        first_party(Probe(
            time,
            PluginInfo::new("navree-time", "navree_time", "time", version)
        )),
        Ok(true)
    );
    assert!(
        first_party(Probe(
            time,
            PluginInfo::new("navree-time", "navree_time", "time", "1.0.0")
        ))
        .unwrap_err()
        .contains("which the framework keeps for itself"),
        "another version"
    );
    assert_eq!(
        first_party(Probe(
            time,
            PluginInfo::new("navree-clock", "navree_time", "clock", version)
        )),
        Ok(false)
    );
    let wrapped = ModuleId::of::<Holder<navree::time::TimeModule>>();
    assert!(
        first_party(Probe(
            wrapped,
            PluginInfo::new("navree-time", "navree_time", "time", version)
        ))
        .unwrap_err()
        .contains("which the framework keeps for itself"),
        "generic arguments do not count"
    );
}

#[test]
fn a_first_party_plugin_keeps_its_reserved_namespace_and_a_community_plugin_does_not() {
    use navree::plugin::PluginInfo;
    let version = env!("CARGO_PKG_VERSION");
    // A first-party plugin keeps its section's namespace and is reported first-party, which is
    // what every crate's collision check reads to treat it as one of today's modules.
    let time = Probe(
        ModuleId::of::<navree::time::TimeModule>(),
        PluginInfo::new("navree-time", "navree_time", "time", version),
    );
    let report = Navree::app().module(time).plugin_report().expect("boots");
    assert!(report.plugins[0].first_party);
    // A community plugin in that namespace fails the boot.
    struct CommunityTime;
    impl Module for CommunityTime {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(PluginInfo::new(
                "navree-plugin-time",
                "facade",
                "time",
                "1.0.0",
            ));
        }
    }
    assert_eq!(
        Navree::app()
            .module(CommunityTime)
            .validate()
            .unwrap_err()
            .to_string(),
        "plugin `navree-plugin-time` uses the namespace `time`, which the framework keeps for itself; set another with plugin_info!(namespace = \"..\")"
    );
}
