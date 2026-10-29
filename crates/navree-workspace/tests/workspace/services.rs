//! The `Workspaces` and `Members` services.

use navree_orm::Model as _;
use navree_workspace::{Member, MemberInfo, NewWorkspace, OWNER, WorkspaceError, WorkspaceRole};
use uuid::Uuid;

use crate::support::World;

#[tokio::test]
async fn creating_a_workspace_makes_its_creator_the_owner() {
    let world = World::new().await;
    let mut events = world
        .events(&["workspace.created", "workspace.member_added"])
        .await;
    let ada = world.user("Ada", "ada@example.com").await;

    let acme = world.workspace(ada, "Acme Inc.").await;

    assert_eq!(acme.name, "Acme Inc.");
    assert_eq!(acme.slug, "acme-inc");
    assert_eq!(acme.owner_id, ada);
    let membership = world
        .members()
        .await
        .find(acme.id, ada)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(membership.role, OWNER);
    let names = events.names();
    assert_eq!(names, ["workspace.created", "workspace.member_added"]);
}

#[tokio::test]
async fn slugs_are_unique() {
    let world = World::new().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let first = world.workspace(ada, "Acme").await;
    let second = world.workspace(ada, "Acme").await;
    assert_eq!(first.slug, "acme");
    assert_eq!(second.slug, "acme-2");

    let workspaces = world.workspaces().await;
    let taken = world
        .work(|uow| async move {
            workspaces
                .create(&uow, ada, NewWorkspace::named("Other").slug("acme"))
                .await
        })
        .await
        .unwrap_err();
    assert!(
        matches!(&taken, WorkspaceError::Invalid { field: "slug", message } if message == "slug has already been taken"),
        "{taken:?}"
    );

    let workspaces = world.workspaces().await;
    let malformed = world
        .work(|uow| async move {
            workspaces
                .create(&uow, ada, NewWorkspace::named("Other").slug("Not A Slug"))
                .await
        })
        .await
        .unwrap_err();
    assert!(matches!(
        malformed,
        WorkspaceError::Invalid { field: "slug", .. }
    ));

    let workspaces = world.workspaces().await;
    let unnamed = world
        .work(|uow| async move {
            workspaces
                .create(&uow, ada, NewWorkspace::named("  "))
                .await
        })
        .await
        .unwrap_err();
    assert!(matches!(
        unnamed,
        WorkspaceError::Invalid { field: "name", .. }
    ));
}

#[tokio::test]
async fn workspaces_are_renamed_listed_and_resolved() {
    let world = World::new().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let grace = world.user("Grace", "grace@example.com").await;
    let acme = world.workspace(ada, "Acme").await;
    let globex = world.workspace(grace, "Globex").await;
    world.join(globex.id, ada, "viewer").await;

    let workspaces = world.workspaces().await;
    let renamed = world
        .work(|uow| async move { workspaces.rename(&uow, acme.id, "Acme Corp").await })
        .await
        .unwrap();
    assert_eq!(renamed.name, "Acme Corp");
    assert_eq!(renamed.slug, "acme", "renaming keeps the slug");

    let workspaces = world.workspaces().await;
    let mine: Vec<(String, WorkspaceRole)> = workspaces
        .for_user(ada)
        .await
        .unwrap()
        .into_iter()
        .map(|m| (m.workspace.slug, m.role))
        .collect();
    assert_eq!(
        mine,
        [
            ("acme".to_owned(), WorkspaceRole::Owner),
            ("globex".to_owned(), WorkspaceRole::Viewer)
        ]
    );
    assert_eq!(workspaces.resolve("globex").await.unwrap().id, globex.id);
    assert_eq!(
        workspaces
            .resolve(&globex.id.to_string())
            .await
            .unwrap()
            .slug,
        "globex"
    );
    assert!(matches!(
        workspaces.resolve("initech").await,
        Err(WorkspaceError::WorkspaceNotFound(_))
    ));
}

#[tokio::test]
async fn ownership_moves_to_a_member() {
    let world = World::new().await;
    let mut events = world.events(&["workspace.ownership_transferred"]).await;
    let ada = world.user("Ada", "ada@example.com").await;
    let grace = world.user("Grace", "grace@example.com").await;
    let stranger = world.user("Eve", "eve@example.com").await;
    let acme = world.workspace(ada, "Acme").await;
    world.join(acme.id, grace, "member").await;

    let workspaces = world.workspaces().await;
    let refused = world
        .work(|uow| async move { workspaces.transfer_ownership(&uow, acme.id, stranger).await })
        .await
        .unwrap_err();
    assert!(matches!(
        refused,
        WorkspaceError::Invalid {
            field: "user_id",
            ..
        }
    ));

    let workspaces = world.workspaces().await;
    let moved = world
        .work(|uow| async move { workspaces.transfer_ownership(&uow, acme.id, grace).await })
        .await
        .unwrap();
    assert_eq!(moved.owner_id, grace);
    let members = world.members().await;
    assert_eq!(
        members.find(acme.id, grace).await.unwrap().unwrap().role,
        "owner"
    );
    assert_eq!(
        members.find(acme.id, ada).await.unwrap().unwrap().role,
        "admin"
    );
    let payloads = events.drain();
    assert_eq!(payloads.len(), 1);
    assert_eq!(payloads[0].1["from"], serde_json::json!(ada));
    assert_eq!(payloads[0].1["to"], serde_json::json!(grace));
}

#[tokio::test]
async fn deleting_a_workspace_removes_its_members_and_invitations() {
    let world = World::new().await;
    let mut events = world.events(&["workspace.deleted"]).await;
    let ada = world.user("Ada", "ada@example.com").await;
    let grace = world.user("Grace", "grace@example.com").await;
    let acme = world.workspace(ada, "Acme").await;
    world.join(acme.id, grace, "member").await;
    let invitations = world.invitations().await;
    world
        .work(|uow| async move {
            invitations
                .invite(
                    &uow,
                    acme.id,
                    "linus@example.com",
                    WorkspaceRole::Member,
                    Some(ada),
                )
                .await
        })
        .await
        .unwrap();

    let workspaces = world.workspaces().await;
    world
        .work(|uow| async move { workspaces.delete(&uow, acme.id).await })
        .await
        .unwrap();

    let db = world.db().await;
    let left = Member::query()
        .where_(Member::WORKSPACE_ID.eq(acme.id))
        .count(&*db)
        .await
        .unwrap();
    assert_eq!(left, 0);
    assert!(
        world
            .invitations()
            .await
            .pending(acme.id)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        world
            .workspaces()
            .await
            .find(acme.id)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(events.names(), ["workspace.deleted"]);
}

#[tokio::test]
async fn members_are_added_changed_listed_and_removed() {
    let world = World::new().await;
    let mut events = world
        .events(&[
            "workspace.member_added",
            "workspace.member_role_changed",
            "workspace.member_removed",
        ])
        .await;
    let ada = world.user("Ada", "ada@example.com").await;
    let grace = world.user("Grace", "grace@example.com").await;
    let acme = world.workspace(ada, "Acme").await;
    events.drain();

    world.join(acme.id, grace, "member").await;
    let members = world.members().await;
    let again = world
        .work(|uow| async move {
            members
                .add(&uow, acme.id, grace, WorkspaceRole::Viewer)
                .await
        })
        .await
        .unwrap_err();
    assert!(matches!(again, WorkspaceError::AlreadyMember));

    let members = world.members().await;
    let unknown_role = world
        .work(|uow| async move {
            members
                .add(
                    &uow,
                    acme.id,
                    Uuid::now_v7(),
                    WorkspaceRole::parse("auditor"),
                )
                .await
        })
        .await
        .unwrap_err();
    assert!(
        matches!(&unknown_role, WorkspaceError::Invalid { field: "role", message } if message.contains("admin, member, owner, viewer")),
        "{unknown_role:?}"
    );

    let members = world.members().await;
    let changed = world
        .work(|uow| async move {
            members
                .change_role(&uow, acme.id, grace, WorkspaceRole::Admin)
                .await
        })
        .await
        .unwrap();
    assert_eq!(changed.role, "admin");

    let listed: Vec<(String, String, Option<String>)> = world
        .members()
        .await
        .list(acme.id)
        .await
        .unwrap()
        .into_iter()
        .map(|MemberInfo { member, user }| {
            (
                member.role,
                user.as_ref().map(|u| u.name.clone()).unwrap_or_default(),
                user.map(|u| u.email),
            )
        })
        .collect();
    assert_eq!(
        listed,
        [
            (
                "owner".to_owned(),
                "Ada".to_owned(),
                Some("ada@example.com".to_owned())
            ),
            (
                "admin".to_owned(),
                "Grace".to_owned(),
                Some("grace@example.com".to_owned())
            ),
        ]
    );

    let members = world.members().await;
    world
        .work(|uow| async move { members.remove(&uow, acme.id, grace).await })
        .await
        .unwrap();
    assert!(
        world
            .members()
            .await
            .find(acme.id, grace)
            .await
            .unwrap()
            .is_none()
    );
    let members = world.members().await;
    let gone = world
        .work(|uow| async move { members.remove(&uow, acme.id, grace).await })
        .await
        .unwrap_err();
    assert!(matches!(gone, WorkspaceError::MemberNotFound));
    assert_eq!(
        events.names(),
        [
            "workspace.member_added",
            "workspace.member_role_changed",
            "workspace.member_removed"
        ]
    );
}

#[tokio::test]
async fn the_last_owner_cannot_be_removed_or_demoted() {
    let world = World::new().await;
    let ada = world.user("Ada", "ada@example.com").await;
    let grace = world.user("Grace", "grace@example.com").await;
    let acme = world.workspace(ada, "Acme").await;

    let members = world.members().await;
    let removed = world
        .work(|uow| async move { members.remove(&uow, acme.id, ada).await })
        .await
        .unwrap_err();
    assert!(matches!(removed, WorkspaceError::OwnerRequired));
    let members = world.members().await;
    let demoted = world
        .work(|uow| async move {
            members
                .change_role(&uow, acme.id, ada, WorkspaceRole::Admin)
                .await
        })
        .await
        .unwrap_err();
    assert!(matches!(demoted, WorkspaceError::OwnerRequired));

    // A second owner can go; the workspace keeps its owner.
    world.join(acme.id, grace, "owner").await;
    let members = world.members().await;
    world
        .work(|uow| async move {
            members
                .change_role(&uow, acme.id, grace, WorkspaceRole::Member)
                .await
        })
        .await
        .unwrap();
    let members = world.members().await;
    world
        .work(|uow| async move { members.remove(&uow, acme.id, grace).await })
        .await
        .unwrap();
}

#[tokio::test]
async fn rolled_back_changes_emit_no_events() {
    let world = World::new().await;
    let mut events = world.events(&["workspace.created"]).await;
    let ada = world.user("Ada", "ada@example.com").await;
    let workspaces = world.workspaces().await;
    let uow = world.db().await.uow(None, None).await;
    workspaces
        .create(&uow, ada, NewWorkspace::named("Acme"))
        .await
        .unwrap();
    uow.rollback().await.unwrap();
    assert!(events.names().is_empty());
    assert!(
        world
            .workspaces()
            .await
            .find_by_slug("acme")
            .await
            .unwrap()
            .is_none()
    );
}
