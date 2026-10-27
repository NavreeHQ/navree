//! Just-in-time provisioning into workspaces and organizations, with group
//! to role mapping, and its event.

use navree_events::EventBus;
use navree_sso::{Provisioning, RoleRule, SsoFake, SsoOwner, SsoUserProvisioned};
use navree_workspace::{
    Members, NewOrganization, NewWorkspace, OrgMembers, Organizations, Workspaces,
};

use crate::support;

fn rules() -> Provisioning {
    Provisioning {
        roles: vec![
            RoleRule {
                group: "Acme Admins".to_owned(),
                role: "admin".to_owned(),
            },
            RoleRule {
                group: "Contractors".to_owned(),
                role: "viewer".to_owned(),
            },
        ],
        ..Provisioning::default()
    }
}

#[tokio::test]
async fn workspace_members_get_the_role_of_their_groups() {
    let world = support::world().await;
    let fake = SsoFake::new();
    let owner = world
        .create_verified_user("Olive Owner", "olive@acme.test")
        .await;
    let workspaces = world.app.get::<Workspaces>().await;
    let workspace = world
        .write(|uow| async move {
            workspaces
                .create(&uow, owner.id, NewWorkspace::named("Acme"))
                .await
        })
        .await
        .unwrap();
    let connection = world
        .saml_connection(&fake, SsoOwner::Workspace(workspace.id), "acme", rules())
        .await;
    world.verified_domain(&connection, "acme.test").await;

    let mut provisioned = world
        .app
        .get::<EventBus>()
        .await
        .subscribe_json("sso.user_provisioned");

    let admin = world
        .saml_sign_in(
            &fake,
            "acme",
            "ada@acme.test",
            &[("groups", &["Engineering", "acme admins"])],
        )
        .await
        .assert_status(200)
        .json_value();
    let admin_id = uuid::Uuid::parse_str(admin["user"]["id"].as_str().unwrap()).unwrap();
    let viewer = world
        .saml_sign_in(
            &fake,
            "acme",
            "cy@acme.test",
            &[("groups", &["Contractors"])],
        )
        .await
        .assert_status(200)
        .json_value();
    let viewer_id = uuid::Uuid::parse_str(viewer["user"]["id"].as_str().unwrap()).unwrap();
    let member = world
        .saml_sign_in(&fake, "acme", "max@acme.test", &[])
        .await
        .assert_status(200)
        .json_value();
    let member_id = uuid::Uuid::parse_str(member["user"]["id"].as_str().unwrap()).unwrap();

    let members = world.app.get::<Members>().await;
    let role = |user| {
        let members = members.clone();
        async move {
            members
                .find(workspace.id, user)
                .await
                .unwrap()
                .unwrap()
                .role
        }
    };
    assert_eq!(role(admin_id).await, "admin");
    assert_eq!(role(viewer_id).await, "viewer");
    assert_eq!(role(member_id).await, "member");

    // Roles follow the groups at the next sign-in.
    world
        .saml_sign_in(
            &fake,
            "acme",
            "cy@acme.test",
            &[("groups", &["Acme Admins"])],
        )
        .await
        .assert_status(200);
    assert_eq!(role(viewer_id).await, "admin");
    // The owner signing in keeps the owner role.
    world
        .saml_sign_in(
            &fake,
            "acme",
            "olive@acme.test",
            &[("groups", &["Contractors"])],
        )
        .await
        .assert_status(200);
    assert_eq!(role(owner.id).await, "owner");

    let mut events: Vec<SsoUserProvisioned> = Vec::new();
    while let Ok(value) = provisioned.try_recv() {
        events.push(serde_json::from_value(value).unwrap());
    }
    assert_eq!(events.len(), 4, "{events:?}");
    assert!(events[0].created);
    assert_eq!(events[0].role.as_deref(), Some("admin"));
    assert_eq!(events[0].owner, SsoOwner::Workspace(workspace.id));
    assert!(!events[3].created);
}

#[tokio::test]
async fn without_a_default_role_unmapped_people_are_refused() {
    let world = support::world().await;
    let fake = SsoFake::new();
    let owner = world.create_user("Olive Owner", "olive@acme.test").await;
    let workspaces = world.app.get::<Workspaces>().await;
    let workspace = world
        .write(|uow| async move {
            workspaces
                .create(&uow, owner.id, NewWorkspace::named("Acme"))
                .await
        })
        .await
        .unwrap();
    let connection = world
        .saml_connection(
            &fake,
            SsoOwner::Workspace(workspace.id),
            "acme",
            Provisioning {
                default_role: None,
                ..rules()
            },
        )
        .await;
    world.verified_domain(&connection, "acme.test").await;
    world
        .saml_sign_in(&fake, "acme", "eve@acme.test", &[("groups", &["Sales"])])
        .await
        .assert_error_code(403, "sso_not_assigned");
    // The refused sign-in created nobody.
    let auth = world.app.get::<navree_auth::AuthService>().await;
    assert!(auth.find_by_email("eve@acme.test").await.unwrap().is_none());
}

#[tokio::test]
async fn organization_members_reach_its_workspaces() {
    let world = support::world().await;
    let fake = SsoFake::new();
    let owner = world.create_user("Olive Owner", "olive@acme.test").await;
    let organizations = world.app.get::<Organizations>().await;
    let organization = world
        .write(|uow| async move {
            organizations
                .create(&uow, owner.id, NewOrganization::named("Acme Group"))
                .await
        })
        .await
        .unwrap();
    let connection = world
        .saml_connection(
            &fake,
            SsoOwner::Organization(organization.id),
            "acme",
            rules(),
        )
        .await;
    world.verified_domain(&connection, "acme.test").await;
    let body = world
        .saml_sign_in(
            &fake,
            "acme",
            "ada@acme.test",
            &[("groups", &["Acme Admins"])],
        )
        .await
        .assert_status(200)
        .json_value();
    let user = uuid::Uuid::parse_str(body["user"]["id"].as_str().unwrap()).unwrap();
    let member = world
        .app
        .get::<OrgMembers>()
        .await
        .find(organization.id, user)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(member.role, "admin");
}
