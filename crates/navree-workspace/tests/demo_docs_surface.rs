//! Every public item `content/docs/workspaces/demo-workspaces.mdx` names,
//! so a rename fails the build: types, methods, config keys, constants,
//! commands and the event.

use navree_config::{Config, ConfigSources};
use navree_events::Event;
use navree_kernel::BoxFuture;
use navree_workspace::events::DemoWorkspaceSeeded;
use navree_workspace::{
    CurrentWorkspace, DEMO_HEADER, DEMO_READ_ONLY, DemoIntent, DemoMode, DemoOptions, DemoReset,
    DemoSeed, DemoSeeder, DemoWorkspaces, ModuleBuilderWorkspaceExt, Workspace, WorkspaceConfig,
    WorkspaceError, WorkspaceKind,
};

/// The trait's documented methods, called through a seeder.
#[allow(dead_code)]
fn seeder_surface<'a>(seeder: &'a dyn DemoSeeder, demo: &'a DemoSeed) {
    let _: &'static str = seeder.name();
    let _: &'static [&'static str] = seeder.tables();
    let _: BoxFuture<'a, _> = seeder.seed(demo);
    let _: BoxFuture<'a, _> = seeder.clear(demo);
}

/// `DemoSeed`'s documented methods.
#[allow(dead_code)]
async fn seed_surface(demo: &DemoSeed) {
    let _ = (
        demo.app(),
        demo.workspace(),
        demo.id(),
        demo.uow(),
        demo.now(),
    );
    let _ = (demo.isolation(), demo.is_reset());
    let _: Result<Vec<String>, _> = demo.fixture("crm/companies");
    let _: Result<Vec<String>, _> = demo.parse_fixture("[]");
    let _ = demo.clear_table("companies").await;
}

/// `DemoWorkspaces`' documented methods.
#[allow(dead_code)]
async fn service_surface(demos: &DemoWorkspaces, uow: &navree_db::Uow) {
    let id = uuid::Uuid::nil();
    let _ = demos
        .create(uow, id, navree_workspace::NewWorkspace::named("Demo"))
        .await;
    let _: Result<DemoReset, WorkspaceError> = demos.reset(id).await;
    let _: Result<Vec<DemoReset>, WorkspaceError> = demos.reset_all().await;
    let _: Result<Vec<Workspace>, WorkspaceError> = demos.all().await;
    let _: Result<Vec<&'static str>, WorkspaceError> = demos.seeders().await;
    let _: (DemoMode, &DemoOptions) = (demos.mode(), demos.options());
}

/// `CurrentWorkspace` and `Workspace`.
#[allow(dead_code)]
fn workspace_surface(current: &CurrentWorkspace, workspace: &Workspace) {
    let _: (bool, Option<DemoMode>, bool) =
        (current.is_demo(), current.demo_mode(), current.is_guest());
    let _: (WorkspaceKind, bool) = (workspace.kind(), workspace.is_demo());
}

/// The builder extension.
#[allow(dead_code)]
fn builder_surface<S: DemoSeeder + navree_kernel::Injectable>(
    m: &mut navree_kernel::ModuleBuilder,
) {
    m.demo_seeder::<S>();
}

#[test]
fn constants_variants_and_the_event_have_their_documented_values() {
    assert_eq!(DEMO_HEADER, "x-navree-demo");
    assert_eq!(DEMO_READ_ONLY, "demo_read_only");
    assert_eq!(DemoWorkspaceSeeded::NAME, "workspace.demo_seeded");
    assert_eq!(
        [WorkspaceKind::Live.as_str(), WorkspaceKind::Demo.as_str()],
        ["live", "demo"]
    );
    assert_eq!(
        [DemoMode::ReadOnly.as_str(), DemoMode::Reset.as_str()],
        ["read_only", "reset"]
    );
    assert_ne!(DemoIntent::Read, DemoIntent::Write);
    assert!(matches!(
        WorkspaceError::NotDemo("acme".into()),
        WorkspaceError::NotDemo(_)
    ));
}

#[test]
fn every_documented_config_key_is_read() {
    let config = Config::load(
        &ConfigSources::new()
            .set("workspace.demo.mode", "reset")
            .set("workspace.demo.reset_at", "04:30")
            .set("workspace.demo.guest_role", "viewer")
            .set("workspace.demo.editor_roles", serde_json::json!(["owner"]))
            .set(
                "workspace.demo.allow",
                serde_json::json!(["reports.preview"]),
            )
            .set("workspace.demo.fixtures", "storage/demo"),
    )
    .unwrap()
    .typed::<WorkspaceConfig>()
    .unwrap();
    let DemoOptions {
        mode,
        reset_at,
        guest_role,
        editor_roles,
        allow,
        fixtures,
    } = config.demo;
    assert_eq!(mode, DemoMode::Reset);
    assert_eq!(reset_at, "04:30");
    assert_eq!(guest_role.as_deref(), Some("viewer"));
    assert_eq!(editor_roles, ["owner"]);
    assert_eq!(allow, ["reports.preview"]);
    assert_eq!(fixtures, "storage/demo");
}
