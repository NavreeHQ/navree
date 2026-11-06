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
    let ada = world.user("Anu", "anu@example.com").await;

    let agra = world.workspace(ada, "Agra Inc.").await;

    assert_eq!(agra.name, "Agra Inc.");
    assert_eq!(agra.slug, "agra-inc");
    assert_eq!(agra.owner_id, ada);
    let membership = world
        .members()
        .await
        .find(agra.id, ada)
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
    let ada = world.user("Anu", "anu@example.com").await;
    let first = world.workspace(ada, "Agra").await;
    let second = world.workspace(ada, "Agra").await;
    assert_eq!(first.slug, "agra");
    assert_eq!(second.slug, "agra-2");

    let workspaces = world.workspaces().await;
    let taken = world
        .work(|uow| async move {
            workspaces
                .create(&uow, ada, NewWorkspace::named("Other").slug("agra"))
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
    let ada = world.user("Anu", "anu@example.com").await;
    let grace = world.user("Gauri", "gauri@example.com").await;
    let agra = world.workspace(ada, "Agra").await;
    let gulati = world.workspace(grace, "Gulati").await;
    world.join(gulati.id, ada, "viewer").await;

    let workspaces = world.workspaces().await;
    let renamed = world
        .work(|uow| async move { workspaces.rename(&uow, agra.id, "Agra Corp").await })
        .await
        .unwrap();
    assert_eq!(renamed.name, "Agra Corp");
    assert_eq!(renamed.slug, "agra", "renaming keeps the slug");

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
            ("agra".to_owned(), WorkspaceRole::Owner),
            ("gulati".to_owned(), WorkspaceRole::Viewer)
        ]
    );
    assert_eq!(workspaces.resolve("gulati").await.unwrap().id, gulati.id);
    assert_eq!(
        workspaces
            .resolve(&gulati.id.to_string())
            .await
            .unwrap()
            .slug,
        "gulati"
    );
    assert!(matches!(
        workspaces.resolve("indrani").await,
        Err(WorkspaceError::WorkspaceNotFound(_))
    ));
}

#[tokio::test]
async fn ownership_moves_to_a_member() {
    let world = World::new().await;
    let mut events = world.events(&["workspace.ownership_transferred"]).await;
    let ada = world.user("Anu", "anu@example.com").await;
    let grace = world.user("Gauri", "gauri@example.com").await;
    let stranger = world.user("Eve", "eve@example.com").await;
    let agra = world.workspace(ada, "Agra").await;
    world.join(agra.id, grace, "member").await;

    let workspaces = world.workspaces().await;
    let refused = world
        .work(|uow| async move { workspaces.transfer_ownership(&uow, agra.id, stranger).await })
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
        .work(|uow| async move { workspaces.transfer_ownership(&uow, agra.id, grace).await })
        .await
        .unwrap();
    assert_eq!(moved.owner_id, grace);
    let members = world.members().await;
    assert_eq!(
        members.find(agra.id, grace).await.unwrap().unwrap().role,
        "owner"
    );
    assert_eq!(
        members.find(agra.id, ada).await.unwrap().unwrap().role,
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
    let ada = world.user("Anu", "anu@example.com").await;
    let grace = world.user("Gauri", "gauri@example.com").await;
    let agra = world.workspace(ada, "Agra").await;
    world.join(agra.id, grace, "member").await;
    let invitations = world.invitations().await;
    world
        .work(|uow| async move {
            invitations
                .invite(
                    &uow,
                    agra.id,
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
        .work(|uow| async move { workspaces.delete(&uow, agra.id).await })
        .await
        .unwrap();

    let db = world.db().await;
    let left = Member::query()
        .where_(Member::WORKSPACE_ID.eq(agra.id))
        .count(&*db)
        .await
        .unwrap();
    assert_eq!(left, 0);
    assert!(
        world
            .invitations()
            .await
            .pending(agra.id)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        world
            .workspaces()
            .await
            .find(agra.id)
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
    let ada = world.user("Anu", "anu@example.com").await;
    let grace = world.user("Gauri", "gauri@example.com").await;
    let agra = world.workspace(ada, "Agra").await;
    events.drain();

    world.join(agra.id, grace, "member").await;
    let members = world.members().await;
    let again = world
        .work(|uow| async move {
            members
                .add(&uow, agra.id, grace, WorkspaceRole::Viewer)
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
                    agra.id,
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
                .change_role(&uow, agra.id, grace, WorkspaceRole::Admin)
                .await
        })
        .await
        .unwrap();
    assert_eq!(changed.role, "admin");

    let listed: Vec<(String, String, Option<String>)> = world
        .members()
        .await
        .list(agra.id)
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
                "Anu".to_owned(),
                Some("anu@example.com".to_owned())
            ),
            (
                "admin".to_owned(),
                "Gauri".to_owned(),
                Some("gauri@example.com".to_owned())
            ),
        ]
    );

    let members = world.members().await;
    world
        .work(|uow| async move { members.remove(&uow, agra.id, grace).await })
        .await
        .unwrap();
    assert!(
        world
            .members()
            .await
            .find(agra.id, grace)
            .await
            .unwrap()
            .is_none()
    );
    let members = world.members().await;
    let gone = world
        .work(|uow| async move { members.remove(&uow, agra.id, grace).await })
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
    let ada = world.user("Anu", "anu@example.com").await;
    let grace = world.user("Gauri", "gauri@example.com").await;
    let agra = world.workspace(ada, "Agra").await;

    let members = world.members().await;
    let removed = world
        .work(|uow| async move { members.remove(&uow, agra.id, ada).await })
        .await
        .unwrap_err();
    assert!(matches!(removed, WorkspaceError::OwnerRequired));
    let members = world.members().await;
    let demoted = world
        .work(|uow| async move {
            members
                .change_role(&uow, agra.id, ada, WorkspaceRole::Admin)
                .await
        })
        .await
        .unwrap_err();
    assert!(matches!(demoted, WorkspaceError::OwnerRequired));

    // A second owner can go; the workspace keeps its owner.
    world.join(agra.id, grace, "owner").await;
    let members = world.members().await;
    world
        .work(|uow| async move {
            members
                .change_role(&uow, agra.id, grace, WorkspaceRole::Member)
                .await
        })
        .await
        .unwrap();
    let members = world.members().await;
    world
        .work(|uow| async move { members.remove(&uow, agra.id, grace).await })
        .await
        .unwrap();
}

#[tokio::test]
async fn rolled_back_changes_emit_no_events() {
    let world = World::new().await;
    let mut events = world.events(&["workspace.created"]).await;
    let ada = world.user("Anu", "anu@example.com").await;
    let workspaces = world.workspaces().await;
    let uow = world.db().await.uow(None, None).await;
    workspaces
        .create(&uow, ada, NewWorkspace::named("Agra"))
        .await
        .unwrap();
    uow.rollback().await.unwrap();
    assert!(events.names().is_empty());
    assert!(
        world
            .workspaces()
            .await
            .find_by_slug("agra")
            .await
            .unwrap()
            .is_none()
    );
}
