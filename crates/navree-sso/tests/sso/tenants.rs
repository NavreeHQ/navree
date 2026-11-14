//! Connections owned by workspaces and organizations, and the sso.tenants section.

use navree_auth::{AuthService, ClientMeta};
use navree_config::check::{CheckOptions, CheckReport};
use navree_config::{ConfigSources, SectionRegistration};
use navree_contracts::{AuditEntry, AuditKind};
use navree_db::Db;
use navree_orm::Model as _;
use navree_sso::{
    Provisioning, RoleRule, SsoError, SsoFake, SsoOptions, SsoOwner, SsoProfile, SsoTenantOptions,
    sso_connection, sso_identity,
};
use navree_workspace::{Members, NewOrganization, NewWorkspace, Organizations, Workspaces};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::{self, World};

/// Provisioning that creates accounts outside the connection's verified
/// domains.
fn open() -> Provisioning {
    Provisioning {
        allow_unverified_emails: true,
        ..Provisioning::default()
    }
}

/// A workspace of the verified user Gurpreet Singh.
async fn workspace_of(world: &World) -> Uuid {
    let owner = world
        .create_verified_user("Gurpreet Singh", "gurpreet@ludhiana.test")
        .await;
    let workspaces = world.app.get::<Workspaces>().await;
    world
        .write(|uow| async move {
            workspaces
                .create(&uow, owner.id, NewWorkspace::named("Ludhiana Traders"))
                .await
        })
        .await
        .expect("the workspace is created")
        .id
}

/// An organization of the verified user Gurpreet Singh.
async fn organization_of(world: &World) -> Uuid {
    let owner = world
        .create_verified_user("Gurpreet Singh", "gurpreet@ludhiana.test")
        .await;
    let organizations = world.app.get::<Organizations>().await;
    world
        .write(|uow| async move {
            organizations
                .create(&uow, owner.id, NewOrganization::named("Ludhiana Group"))
                .await
        })
        .await
        .expect("the organization is created")
        .id
}

/// How many identities are linked, across every connection.
async fn identities(world: &World) -> usize {
    let db = world.app.get::<Db>().await;
    sso_identity::Model::query()
        .get(&db)
        .await
        .expect("the identities are read")
        .len()
}

/// The `sso.login_refused` audit entries written so far.
fn refusals(world: &World) -> Vec<AuditEntry> {
    let entries = world.audit.0.lock().unwrap().clone();
    entries
        .into_iter()
        .filter(|entry| entry.action == "sso.login_refused")
        .collect()
}

/// The role of `user` in `workspace`.
async fn role(world: &World, workspace: Uuid, user: Uuid) -> String {
    let members = world.app.get::<Members>().await;
    let member = members.find(workspace, user).await.unwrap();
    member.expect("a member").role
}

#[test]
fn config_check_knows_the_tenants_section() {
    let registrations = [
        SectionRegistration::of::<SsoOptions>(),
        SectionRegistration::of::<SsoTenantOptions>(),
    ];
    let set = ConfigSources::new().set("sso.tenants.unverified_emails", true);
    let report = CheckReport::run(&set, &registrations, &CheckOptions::default()).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert!(report.unknown.is_empty(), "{:?}", report.unknown);

    let unset = ConfigSources::new();
    let report = CheckReport::run(&unset, &registrations, &CheckOptions::default()).unwrap();
    assert!(report.is_ok());

    let misspelt = ConfigSources::new().set("sso.tenants.unverfied_emails", true);
    let report = CheckReport::run(&misspelt, &registrations, &CheckOptions::default()).unwrap();
    let keys: Vec<&str> = report.unknown.iter().map(|u| u.key.as_str()).collect();
    assert_eq!(keys, ["sso.tenants.unverfied_emails"]);
}

#[tokio::test]
async fn an_app_without_the_tenants_section_boots_with_the_flag_closed_to_customers() {
    let world = support::world().await;
    let sso = world.sso().await;
    assert_eq!(
        sso.options(),
        &SsoOptions {
            base_url: Some(support::BASE.to_owned()),
            ..SsoOptions::default()
        }
    );
    // What the defaults refuse is shown by the sign-in tests below.
}

#[tokio::test]
async fn a_workspace_connection_refuses_an_address_outside_its_domains() {
    let world = support::world().await;
    let fake = SsoFake::new();
    let workspace = workspace_of(&world).await;
    let connection = world
        .saml_connection(
            &fake,
            SsoOwner::Workspace(workspace),
            "ludhiana-okta",
            open(),
        )
        .await;
    world.verified_domain(&connection, "ludhiana.test").await;

    world
        .saml_sign_in(&fake, "ludhiana-okta", "simran.kaur@gmail.test", &[])
        .await
        .assert_error_code(403, "unverified_domain");

    let auth = world.app.get::<AuthService>().await;
    let found = auth.find_by_email("simran.kaur@gmail.test").await;
    assert!(found.unwrap().is_none());
    assert_eq!(identities(&world).await, 0);
    let refused = refusals(&world);
    assert_eq!(refused.len(), 1);
    assert_eq!(refused[0].kind, AuditKind::Security);
    assert_eq!(refused[0].subject_type.as_deref(), Some("SsoConnection"));
    assert_eq!(
        refused[0].subject_id.as_deref(),
        Some(connection.id.to_string().as_str())
    );
    assert_eq!(refused[0].metadata["connection"], "ludhiana-okta");
    assert_eq!(refused[0].metadata["owner"], "workspace");
    assert_eq!(refused[0].metadata["domain"], "gmail.test");
    assert_eq!(refused[0].metadata["reason"], "unverified_email");
}

#[tokio::test]
async fn an_organization_connection_refuses_an_address_outside_its_domains() {
    let world = support::world().await;
    let fake = SsoFake::new();
    let organization = organization_of(&world).await;
    let connection = world
        .saml_connection(
            &fake,
            SsoOwner::Organization(organization),
            "ludhiana-okta",
            open(),
        )
        .await;
    world.verified_domain(&connection, "ludhiana.test").await;

    world
        .saml_sign_in(&fake, "ludhiana-okta", "simran.kaur@gmail.test", &[])
        .await
        .assert_error_code(403, "unverified_domain");

    let auth = world.app.get::<AuthService>().await;
    let found = auth.find_by_email("simran.kaur@gmail.test").await;
    assert!(found.unwrap().is_none());
    assert_eq!(identities(&world).await, 0);
    let refused = refusals(&world);
    assert_eq!(refused.len(), 1);
    assert_eq!(refused[0].kind, AuditKind::Security);
    assert_eq!(
        refused[0].subject_id.as_deref(),
        Some(connection.id.to_string().as_str())
    );
    assert_eq!(refused[0].metadata["owner"], "organization");
    assert_eq!(refused[0].metadata["domain"], "gmail.test");
}

#[tokio::test]
async fn sign_in_profile_is_refused_the_same_way() {
    let world = support::world().await;
    let fake = SsoFake::new();
    let workspace = workspace_of(&world).await;
    let connection = world
        .saml_connection(
            &fake,
            SsoOwner::Workspace(workspace),
            "ludhiana-okta",
            open(),
        )
        .await;
    world.verified_domain(&connection, "ludhiana.test").await;
    let sso = world.sso().await;
    let profile = SsoProfile {
        subject: "simran-1".to_owned(),
        email: Some("simran.kaur@gmail.test".to_owned()),
        email_verified: None,
        name: Some("Simran Kaur".to_owned()),
        groups: Vec::new(),
        attributes: json!({}),
    };
    let result = sso
        .sign_in_profile("ludhiana-okta", profile, &ClientMeta::default())
        .await;
    assert!(matches!(result, Err(SsoError::UnverifiedDomain(domain)) if domain == "gmail.test"));
}

#[tokio::test]
async fn an_app_connection_with_the_flag_still_creates_the_account() {
    let world = support::world().await;
    let fake = SsoFake::new();
    world
        .saml_connection(&fake, SsoOwner::App, "staff-okta", open())
        .await;
    let body = world
        .saml_sign_in(&fake, "staff-okta", "simran.kaur@gmail.test", &[])
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(body["user"]["email_verified_at"], Value::Null);
    assert_eq!(identities(&world).await, 1);
    assert!(refusals(&world).is_empty());
}

#[tokio::test]
async fn the_tenants_key_lets_a_workspace_connection_create_the_account() {
    let builder = support::builder().config("sso.tenants.unverified_emails", true);
    let world = support::world_with(builder).await;
    let fake = SsoFake::new();
    let workspace = workspace_of(&world).await;
    world
        .saml_connection(
            &fake,
            SsoOwner::Workspace(workspace),
            "ludhiana-okta",
            open(),
        )
        .await;
    let body = world
        .saml_sign_in(&fake, "ludhiana-okta", "simran.kaur@gmail.test", &[])
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(body["user"]["email_verified_at"], Value::Null);
    assert_eq!(identities(&world).await, 1);
    assert!(refusals(&world).is_empty());
}

#[tokio::test]
async fn a_person_already_linked_through_a_customers_connection_still_signs_in() {
    let world = support::world().await;
    let fake = SsoFake::new();
    let workspace = workspace_of(&world).await;
    let provisioning = Provisioning {
        roles: vec![RoleRule {
            group: "Ludhiana Admins".to_owned(),
            role: "admin".to_owned(),
        }],
        ..open()
    };
    let connection = world
        .saml_connection(
            &fake,
            SsoOwner::Workspace(workspace),
            "ludhiana-okta",
            provisioning,
        )
        .await;
    // Gurpreet's colleague, linked to the connection while it still created
    // accounts outside its verified domains.
    let simran = world
        .create_user("Simran Kaur", "simran.kaur@gmail.test")
        .await;
    let now = chrono::Utc::now();
    let row = sso_identity::Model {
        id: Uuid::now_v7(),
        connection_id: connection.id,
        user_id: simran.id,
        subject: "simran.kaur@gmail.test".to_owned(),
        email: Some("simran.kaur@gmail.test".to_owned()),
        attributes: "{}".into(),
        last_login_at: now,
        created_at: now,
        updated_at: now,
    };
    world
        .write(|uow| async move { sso_identity::Model::insert(&uow, [row]).await })
        .await
        .expect("the identity is linked");

    world
        .saml_sign_in(
            &fake,
            "ludhiana-okta",
            "simran.kaur@gmail.test",
            &[("groups", &["Ludhiana Admins"])],
        )
        .await
        .assert_status(200)
        .assert_json_path("user.id", json!(simran.id));
    assert_eq!(role(&world, workspace, simran.id).await, "admin");
    world
        .saml_sign_in(
            &fake,
            "ludhiana-okta",
            "simran.kaur@gmail.test",
            &[("groups", &["Contractors"])],
        )
        .await
        .assert_status(200);
    // No rule matches her groups now: the default role.
    assert_eq!(role(&world, workspace, simran.id).await, "member");
    assert!(refusals(&world).is_empty());
}

#[tokio::test]
async fn a_verified_domain_still_creates_and_links_through_a_workspace_connection() {
    let world = support::world().await;
    let fake = SsoFake::new();
    let workspace = workspace_of(&world).await;
    let connection = world
        .saml_connection(
            &fake,
            SsoOwner::Workspace(workspace),
            "ludhiana-okta",
            open(),
        )
        .await;
    world.verified_domain(&connection, "ludhiana.test").await;
    let harpreet = world
        .create_verified_user("Harpreet Kaur", "harpreet@ludhiana.test")
        .await;

    let new = world
        .saml_sign_in(&fake, "ludhiana-okta", "simran@ludhiana.test", &[])
        .await
        .assert_status(200)
        .json_value();
    assert!(new["user"]["email_verified_at"].is_string());
    world
        .saml_sign_in(&fake, "ludhiana-okta", "harpreet@ludhiana.test", &[])
        .await
        .assert_status(200)
        .assert_json_path("user.id", json!(harpreet.id));
}

#[tokio::test]
async fn a_connection_whose_workspace_is_gone_is_still_a_customers() {
    let world = support::world().await;
    let fake = SsoFake::new();
    let workspace = workspace_of(&world).await;
    let connection = world
        .saml_connection(
            &fake,
            SsoOwner::Workspace(workspace),
            "ludhiana-okta",
            open(),
        )
        .await;
    // The connection's owner becomes a workspace that doesn't exist.
    let db = world.app.get::<Db>().await;
    let elsewhere = sso_connection::Model::OWNER_ID.set(Some(Uuid::now_v7()));
    sso_connection::Model::query()
        .where_key(connection.id)
        .update(&*db, [elsewhere])
        .await
        .unwrap();

    world
        .saml_sign_in(&fake, "ludhiana-okta", "simran.kaur@gmail.test", &[])
        .await
        .assert_error_code(403, "unverified_domain");
}

#[tokio::test]
async fn the_routes_store_the_flag_on_a_workspace_connection_as_before() {
    let world = support::world().await;
    let fake = SsoFake::new();
    let workspace = workspace_of(&world).await;
    let (_admin, token) = world.sso_admin().await;
    let input = json!({
        "owner": { "type": "workspace", "id": workspace },
        "name": "Ludhiana Okta",
        "settings": { "protocol": "saml", "metadata_xml": fake.metadata_xml() },
        "provisioning": { "allow_unverified_emails": true },
    });
    let created = world
        .app
        .client()
        .post("/api/sso/connections")
        .bearer(&token)
        .json(&input)
        .send()
        .await
        .assert_status(201)
        .assert_json_path("provisioning.allow_unverified_emails", json!(true))
        .json_value();
    let id = created["id"].as_str().unwrap();
    world
        .app
        .client()
        .patch(&format!("/api/sso/connections/{id}"))
        .bearer(&token)
        .json(&json!({ "provisioning": { "allow_unverified_emails": true, "jit": true } }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("provisioning.allow_unverified_emails", json!(true));
}

#[tokio::test]
async fn an_update_cannot_change_a_connections_owner() {
    let world = support::world().await;
    let fake = SsoFake::new();
    let workspace = workspace_of(&world).await;
    let (_admin, token) = world.sso_admin().await;
    let input = json!({
        "owner": { "type": "workspace", "id": workspace },
        "name": "Ludhiana Okta",
        "settings": { "protocol": "saml", "metadata_xml": fake.metadata_xml() },
        "provisioning": { "allow_unverified_emails": true },
    });
    let created = world
        .app
        .client()
        .post("/api/sso/connections")
        .bearer(&token)
        .json(&input)
        .send()
        .await
        .assert_status(201)
        .assert_json_path("slug", json!("ludhiana-okta"))
        .json_value();
    let id = created["id"].as_str().unwrap();
    let slug = created["slug"].as_str().unwrap();

    world
        .app
        .client()
        .patch(&format!("/api/sso/connections/{id}"))
        .bearer(&token)
        .json(&json!({ "owner": { "type": "app" }, "name": "Ludhiana Login" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("owner", json!({ "type": "workspace", "id": workspace }))
        .assert_json_path("name", json!("Ludhiana Login"));
    world
        .saml_sign_in(&fake, slug, "simran.kaur@gmail.test", &[])
        .await
        .assert_error_code(403, "unverified_domain");
}
