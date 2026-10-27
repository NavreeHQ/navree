//! The module's `dyn PermissionCheck`: a member's role in a workspace
//! answers permission questions about that workspace, outside any route.

use navree_contracts::{PermissionCheck, permitted};
use navree_workspace::Members;
use serde_json::json;
use uuid::Uuid;

use crate::support::{World, builder, signed_in};

#[tokio::test]
async fn the_members_role_answers_for_its_workspace() {
    let world = World::with(builder().config(
        "workspace.roles.support.permissions",
        json!(["operations.view"]),
    ))
    .await;
    let owner = world.user("Olive", "olive@example.test").await;
    let support = world.user("Sam", "sam@example.test").await;
    let member = world.user("Mia", "mia@example.test").await;
    let outsider = world.user("Otto", "otto@example.test").await;
    let acme = world.workspace(owner, "Acme").await;
    let globex = world.workspace(outsider, "Globex").await;
    world.join(acme.id, support, "support").await;
    world.join(acme.id, member, "member").await;
    let ctx = world.app.ctx();
    let allowed = |user: Uuid, permission: &'static str, workspace: Option<Uuid>| async move {
        permitted(ctx, &signed_in(user), permission, workspace)
            .await
            .unwrap()
    };

    // The configured role grants it in its workspace.
    assert!(allowed(support, "operations.view", Some(acme.id)).await);
    assert!(!allowed(support, "operations.cancel", Some(acme.id)).await);
    // The owner's `all.*` covers every permission.
    assert!(allowed(owner, "operations.cancel", Some(acme.id)).await);
    // `member` was given `deals.*`, not operations.
    assert!(!allowed(member, "operations.view", Some(acme.id)).await);
    assert!(allowed(member, "deals.update", Some(acme.id)).await);
    // Not a member: no grant, and a role elsewhere does not carry over.
    assert!(!allowed(outsider, "operations.view", Some(acme.id)).await);
    assert!(!allowed(support, "operations.view", Some(globex.id)).await);
    assert!(!allowed(owner, "operations.view", Some(Uuid::now_v7())).await);
    // Workspace roles say nothing app-wide.
    assert!(!allowed(owner, "operations.view", None).await);

    // A role change applies at once.
    let members = world.app.get::<Members>().await;
    world
        .work(|uow| async move {
            members
                .change_role(&uow, acme.id, member, "support".into())
                .await
        })
        .await
        .unwrap();
    assert!(allowed(member, "operations.view", Some(acme.id)).await);
}

#[tokio::test]
async fn the_check_answers_only_about_a_workspace() {
    let world = World::new().await;
    let owner = world.user("Olive", "olive@example.test").await;
    let acme = world.workspace(owner, "Acme").await;
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
        .check(&who, "deals.read", Some(acme.id))
        .await
        .unwrap();
    assert_eq!(workspace, Some(true));
}
