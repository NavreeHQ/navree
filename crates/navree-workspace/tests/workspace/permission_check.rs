//! The module's `dyn PermissionCheck`: a member's role in a workspace
//! answers permission questions about that workspace, outside any route.

use navree_contracts::{PermissionCheck, permitted};
use navree_workspace::{Members, WorkspaceSelector};
use serde_json::json;
use uuid::Uuid;

use crate::support::{World, builder, org_builder, signed_in};

#[tokio::test]
async fn the_members_role_answers_for_its_workspace() {
    let world = World::with(builder().config(
        "workspace.roles.support.permissions",
        json!(["operations.view"]),
    ))
    .await;
    let owner = world.user("Ojas", "ojas@example.test").await;
    let support = world.user("Sahil", "sahil@example.test").await;
    let member = world.user("Mia", "mia@example.test").await;
    let outsider = world.user("Otto", "otto@example.test").await;
    let agra = world.workspace(owner, "Agra").await;
    let gulati = world.workspace(outsider, "Gulati").await;
    world.join(agra.id, support, "support").await;
    world.join(agra.id, member, "member").await;
    let ctx = world.app.ctx();
    let allowed = |user: Uuid, permission: &'static str, workspace: Option<Uuid>| async move {
        permitted(ctx, &signed_in(user), permission, workspace)
            .await
            .unwrap()
    };

    // The configured role grants it in its workspace.
    assert!(allowed(support, "operations.view", Some(agra.id)).await);
    assert!(!allowed(support, "operations.cancel", Some(agra.id)).await);
    // The owner's `all.*` covers every permission.
    assert!(allowed(owner, "operations.cancel", Some(agra.id)).await);
    // `member` was given `deals.*`, not operations.
    assert!(!allowed(member, "operations.view", Some(agra.id)).await);
    assert!(allowed(member, "deals.update", Some(agra.id)).await);
    // Not a member: no grant, and a role elsewhere does not carry over.
    assert!(!allowed(outsider, "operations.view", Some(agra.id)).await);
    assert!(!allowed(support, "operations.view", Some(gulati.id)).await);
    assert!(!allowed(owner, "operations.view", Some(Uuid::now_v7())).await);
    // Workspace roles say nothing app-wide.
    assert!(!allowed(owner, "operations.view", None).await);

    // A role change applies at once.
    let members = world.app.get::<Members>().await;
    world
        .work(|uow| async move {
            members
                .change_role(&uow, agra.id, member, "support".into())
                .await
        })
        .await
        .unwrap();
    assert!(allowed(member, "operations.view", Some(agra.id)).await);
}

#[tokio::test]
async fn the_check_answers_only_about_a_workspace() {
    let world = World::new().await;
    let owner = world.user("Ojas", "ojas@example.test").await;
    let agra = world.workspace(owner, "Agra").await;
    let checks = world
        .app
        .ctx()
        .get_all::<dyn PermissionCheck>()
        .await
        .unwrap();
    // AccessModule's app-wide check, then the workspace module's.
    assert_eq!(checks.len(), 2);
    let who = signed_in(owner);
    let answers: Vec<Option<bool>> = {
        let mut answers = Vec::new();
        for check in &checks {
            answers.push(check.check(&who, "deals.read", None).await.unwrap());
        }
        answers
    };
    assert_eq!(
        answers,
        [Some(false), None],
        "no workspace: not its question"
    );
    let workspace = checks[1]
        .check(&who, "deals.read", Some(agra.id))
        .await
        .unwrap();
    assert_eq!(workspace, Some(true));
}

#[tokio::test]
async fn an_organization_role_answers_as_it_does_behind_the_guard() {
    let world = World::with(org_builder()).await;
    let owner = world.user("Ojas", "ojas@example.test").await;
    let ken = world.user("Ken", "ken@example.test").await;
    let linus = world.user("Laksh", "laksh@example.test").await;
    let agra = world.organization(owner, "Agra").await;
    let uk = world.workspace(owner, "UK").await;
    let de = world.workspace(owner, "DE").await;
    world.attach(agra.id, uk.id).await;
    world.attach(agra.id, de.id).await;
    // `partner` maps to the workspace role `viewer` (`deals.read`).
    world
        .grant(agra.id, ken, "partner", WorkspaceSelector::All)
        .await;
    world
        .grant(agra.id, linus, "member", WorkspaceSelector::only([uk.id]))
        .await;
    let ctx = world.app.ctx();
    let allowed = |user: Uuid, permission: &'static str, workspace: Uuid| async move {
        permitted(ctx, &signed_in(user), permission, Some(workspace))
            .await
            .unwrap()
    };

    // No membership of the workspace: the mapped role answers.
    assert!(allowed(ken, "deals.read", uk.id).await);
    assert!(allowed(ken, "deals.read", de.id).await);
    assert!(!allowed(ken, "deals.update", uk.id).await);
    assert!(allowed(linus, "deals.update", uk.id).await);
    // Outside the workspaces the grant reaches, nothing.
    assert!(!allowed(linus, "deals.read", de.id).await);
}
