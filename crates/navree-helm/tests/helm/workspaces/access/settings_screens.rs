//! The Settings screens at the level of a workspace, an organization or a user:
//! the same rule as everywhere else in the panel.

use navree_settings::{ModuleBuilderSettingsExt, Scope, Settings, SettingsModule};
use navree_workspace::{NewOrganization, OrgMembers, OrgRole, Organizations};

use super::*;

/// What the member portal shows, at every level.
#[derive(
    navree_settings::Settings,
    Clone,
    Debug,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    navree_validation::Validate,
)]
#[settings(key = "levelled", scopes(global, organization, workspace, user))]
pub struct Levelled {
    #[validate(length(max = 40))]
    pub welcome_text: String,
}

impl Default for Levelled {
    fn default() -> Self {
        Levelled {
            welcome_text: "Welcome".to_owned(),
        }
    }
}

struct LevelledModule;

impl Module for LevelledModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.settings::<Levelled>();
    }
}

const SCREEN: &str = "/admin/settings/levelled";

fn at(level: &str, target: impl std::fmt::Display) -> String {
    format!("{SCREEN}?level={level}&target={target}")
}

async fn settings_world(extra: impl FnOnce(TestAppBuilder) -> TestAppBuilder) -> World {
    let builder = extra(
        crate::servers::database(TestApp::builder())
            .config("workspace.organizations.enabled", true)
            .config("audit.routes", false)
            .module(navree_audit::AuditModule::from_config())
            .module(LevelledModule)
            .module(SettingsModule::from_config()),
    );
    let world = world(with_members(builder).build().await).await;
    for user in [world.member_id, world.outsider_id] {
        give_role(
            &world.app,
            user,
            "settings-admin",
            &["helm.view", "settings.manage"],
        )
        .await;
    }
    world
}

async fn page(client: &TestClient, path: &str) -> TestResponse {
    client.get(path).header("accept", "text/html").send().await
}

async fn post(client: &TestClient, path: &str, form: &[(&str, &str)]) -> TestResponse {
    client
        .post(path)
        .header("accept", "text/html")
        .form(form)
        .send()
        .await
}

async fn stored(world: &World, scope: Scope) -> Option<Value> {
    let settings = world.app.get::<Settings>().await;
    settings
        .resolve_key("levelled", scope)
        .await
        .unwrap()
        .data
        .get("welcome_text")
        .cloned()
}

#[tokio::test]
async fn nobody_reaches_the_settings_of_a_workspace_they_arent_in() {
    let world = settings_world(|builder| builder).await;
    let gulati = world.gulati;
    for client in [&world.member, &world.outsider] {
        let response = page(client, &at("workspace", gulati))
            .await
            .assert_status(403);
        assert!(!response.text().contains("Gulati"), "{}", response.text());
        post(
            client,
            &at("workspace", gulati),
            &[("welcome_text", "Set by a stranger")],
        )
        .await
        .assert_status(403);
        post(
            client,
            &format!("{SCREEN}/inherit?level=workspace&target={gulati}"),
            &[("_inherit", "welcome_text")],
        )
        .await
        .assert_status(403);
    }
    assert_eq!(
        stored(&world, Scope::workspace(gulati)).await,
        Some(json!("Welcome"))
    );
}

#[tokio::test]
async fn a_slug_tells_nothing_a_user_may_not_know() {
    let world = settings_world(|builder| builder).await;
    let tenants = world.app.get::<Tenants>().await;
    let slug = tenants
        .resolve(&world.gulati.to_string())
        .await
        .unwrap()
        .slug;
    page(&world.outsider, &at("workspace", slug))
        .await
        .assert_status(403);
    page(&world.outsider, &at("workspace", "no-such-slug"))
        .await
        .assert_status(403);
    page(&world.outsider, &at("workspace", Uuid::now_v7()))
        .await
        .assert_status(403);
}

#[tokio::test]
async fn a_member_works_with_the_settings_of_their_workspace() {
    let world = settings_world(|builder| builder).await;
    let tenants = world.app.get::<Tenants>().await;
    let slug = tenants.resolve(&world.agra.to_string()).await.unwrap().slug;
    for target in [world.agra.to_string(), slug] {
        page(&world.member, &at("workspace", &target))
            .await
            .assert_status(200);
    }
    post(
        &world.member,
        &at("workspace", world.agra),
        &[("welcome_text", "Ours")],
    )
    .await
    .assert_status(303);
    assert_eq!(
        stored(&world, Scope::workspace(world.agra)).await,
        Some(json!("Ours"))
    );
    post(
        &world.member,
        &format!("{SCREEN}/inherit?level=workspace&target={}", world.agra),
        &[("_inherit", "welcome_text")],
    )
    .await
    .assert_status(303);
}

#[tokio::test]
async fn a_viewer_reads_the_settings_of_their_workspace() {
    let world = settings_world(|builder| builder).await;
    let id = account(&world.app, "settings-viewer").await;
    let members = world.app.get::<Members>().await;
    let db = world.app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    uow.scope(members.add(&uow, world.agra, id, WorkspaceRole::Viewer))
        .await
        .unwrap();
    uow.commit().await.unwrap();
    give_role(
        &world.app,
        id,
        "settings-admin",
        &["helm.view", "settings.manage"],
    )
    .await;
    let viewer = world.app.acting_as(navree_testing::principal(id));
    page(&viewer, &at("workspace", world.agra))
        .await
        .assert_status(200);
    post(
        &viewer,
        &at("workspace", world.agra),
        &[("welcome_text", "By a viewer")],
    )
    .await
    .assert_status(403);
}

#[tokio::test]
async fn staff_reach_settings_only_by_the_opt_in_and_reads_are_written_down() {
    let world = settings_world(|builder| builder.module(StaffNotesModule)).await;
    let staff_id = account(&world.app, "staff").await;
    give_role(
        &world.app,
        staff_id,
        "support",
        &["helm.view", "settings.manage", "support.enter"],
    )
    .await;
    let staff = world.app.acting_as(navree_testing::principal(staff_id));
    page(&staff, &at("workspace", world.gulati))
        .await
        .assert_status(200);
    assert_eq!(staff_entries(&world.app).await, [world.gulati.to_string()]);
    // A form that fails is written down as well.
    post(
        &staff,
        &at("workspace", world.gulati),
        &[("welcome_text", &"x".repeat(50))],
    )
    .await
    .assert_status(400);
    assert_eq!(staff_entries(&world.app).await.len(), 2);
    page(&staff, &at("workspace", Uuid::now_v7()))
        .await
        .assert_status(404);
    // The others still don't get in, and what a member does isn't staff's.
    page(&world.outsider, &at("workspace", world.gulati))
        .await
        .assert_status(403);
    page(&world.member, &at("workspace", world.agra))
        .await
        .assert_status(200);
    assert_eq!(staff_entries(&world.app).await.len(), 2);
}

#[tokio::test]
async fn a_user_works_with_their_own_settings_and_nobody_elses() {
    let world = settings_world(|builder| builder).await;
    post(
        &world.member,
        &at("user", world.outsider_id),
        &[("welcome_text", "Into a user's")],
    )
    .await
    .assert_status(403);
    page(&world.member, &at("user", world.outsider_id))
        .await
        .assert_status(403);
    assert_eq!(
        stored(&world, Scope::user(world.outsider_id)).await,
        Some(json!("Welcome"))
    );
    post(
        &world.member,
        &at("user", world.member_id),
        &[("welcome_text", "Mine")],
    )
    .await
    .assert_status(303);
    assert_eq!(
        stored(&world, Scope::user(world.member_id)).await,
        Some(json!("Mine"))
    );
}

/// An organization owned by `owner`.
async fn organization(world: &World, owner: Uuid, name: &str) -> Uuid {
    let organizations = world.app.get::<Organizations>().await;
    let db = world.app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    let made = uow
        .scope(organizations.create(&uow, owner, NewOrganization::named(name)))
        .await
        .unwrap();
    uow.commit().await.unwrap();
    made.id
}

#[tokio::test]
async fn nobody_reaches_the_settings_of_an_organization_they_arent_in() {
    let world = settings_world(|builder| builder).await;
    let ilaichi = organization(&world, world.member_id, "Ilaichi").await;
    let response = page(&world.outsider, &at("organization", ilaichi))
        .await
        .assert_status(403);
    assert!(!response.text().contains("Ilaichi"), "{}", response.text());
    post(
        &world.outsider,
        &at("organization", ilaichi),
        &[("welcome_text", "Set by a stranger")],
    )
    .await
    .assert_status(403);
    page(&world.outsider, &at("organization", "no-such-slug"))
        .await
        .assert_status(403);
    assert_eq!(
        stored(&world, Scope::organization(ilaichi)).await,
        Some(json!("Welcome"))
    );
    // Its owner does; a viewer of it only reads.
    post(
        &world.member,
        &at("organization", ilaichi),
        &[("welcome_text", "Ours")],
    )
    .await
    .assert_status(303);
    let id = account(&world.app, "org-viewer").await;
    let members = world.app.get::<OrgMembers>().await;
    let db = world.app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    uow.scope(members.add(&uow, ilaichi, id, OrgRole::Viewer, Default::default()))
        .await
        .unwrap();
    uow.commit().await.unwrap();
    give_role(
        &world.app,
        id,
        "settings-admin",
        &["helm.view", "settings.manage"],
    )
    .await;
    let viewer = world.app.acting_as(navree_testing::principal(id));
    page(&viewer, &at("organization", ilaichi))
        .await
        .assert_status(200);
    post(
        &viewer,
        &at("organization", ilaichi),
        &[("welcome_text", "By a viewer")],
    )
    .await
    .assert_status(403);
}

/// Staff for organizations are whoever holds the permission `support.enter`.
struct StaffOrganizationsModule;

impl Module for StaffOrganizationsModule {
    fn register(&self, m: &mut ModuleBuilder) {
        m.helm_staff_organizations(|entry| async move {
            Ok(entry.access.ability.can("enter", "support", None))
        });
    }
}

#[tokio::test]
async fn staff_reach_an_organization_only_by_its_own_opt_in_and_it_is_written_down() {
    let world = settings_world(|builder| builder.module(StaffOrganizationsModule)).await;
    let ilaichi = organization(&world, world.member_id, "Ilaichi").await;
    let staff_id = account(&world.app, "staff").await;
    give_role(
        &world.app,
        staff_id,
        "support",
        &["helm.view", "settings.manage", "support.enter"],
    )
    .await;
    let staff = world.app.acting_as(navree_testing::principal(staff_id));
    page(&staff, &at("organization", ilaichi))
        .await
        .assert_status(200);
    post(
        &staff,
        &at("organization", ilaichi),
        &[("welcome_text", "By staff")],
    )
    .await
    .assert_status(303);
    let log = world.app.get::<navree_audit::AuditLog>().await;
    log.flush().await;
    let entries = navree_audit::AuditLog::list(
        &log,
        &navree_audit::AuditFilter::default(),
        navree_db::PageQuery {
            page: 1,
            per_page: 100,
        },
    )
    .await
    .unwrap()
    .data
    .into_iter()
    .filter(|row| row.action == "helm.organization.staff")
    .count();
    assert_eq!(entries, 2);
    page(&staff, &at("organization", "no-such-slug"))
        .await
        .assert_status(404);
    // The others still don't get in.
    page(&world.outsider, &at("organization", ilaichi))
        .await
        .assert_status(403);
}

#[tokio::test]
async fn a_credential_limited_to_a_workspace_reaches_the_settings_of_that_one() {
    use super::pinned::{PinLayer, pinned};

    let world = settings_world(|builder| builder.module(PinLayer)).await;
    join(&world.app, world.gulati, world.member_id).await;
    let ilaichi = organization(&world, world.member_id, "Ilaichi").await;
    let html =
        |path: String| pinned(world.member.get(&path), world.agra).header("accept", "text/html");
    html(at("workspace", world.agra))
        .send()
        .await
        .assert_status(200);
    html(at("workspace", world.gulati))
        .send()
        .await
        .assert_status(403);
    html(at("organization", ilaichi))
        .send()
        .await
        .assert_status(403);
    html(format!("{SCREEN}?level=global"))
        .send()
        .await
        .assert_status(403);
    html(at("user", world.member_id))
        .send()
        .await
        .assert_status(200);
}
