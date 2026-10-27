//! The module's `dyn PermissionCheck`: permission questions asked outside
//! a route (a channel authorizer, a service deciding who may see a record).

use navree_access::AbilityFactory;
use navree_contracts::{PermissionCheck, permitted};
use navree_testing::principal;
use uuid::Uuid;

use crate::support::{SUPER_ADMIN, app, give_role};

#[tokio::test]
async fn app_wide_roles_and_permissions_answer_permission_checks() {
    let (app, _audit) = app().await;
    let viewer = Uuid::now_v7();
    give_role(&app, viewer, "support", &["operations.view"]).await;
    let manager = Uuid::now_v7();
    give_role(&app, manager, "operations-manager", &["operations.*"]).await;
    let admin = Uuid::now_v7();
    give_role(&app, admin, SUPER_ADMIN, &[]).await;
    let nobody = Uuid::now_v7();
    let ctx = app.ctx();
    let workspace = Some(Uuid::now_v7());

    let allowed = |user: Uuid, permission: &'static str, workspace: Option<Uuid>| async move {
        permitted(ctx, &principal(user), permission, workspace)
            .await
            .unwrap()
    };
    assert!(allowed(viewer, "operations.view", None).await);
    // An app-wide grant holds in every workspace.
    assert!(allowed(viewer, "operations.view", workspace).await);
    assert!(!allowed(viewer, "operations.cancel", None).await);
    // `prefix.*` grants every action of the prefix.
    assert!(allowed(manager, "operations.cancel", workspace).await);
    assert!(allowed(admin, "operations.cancel", None).await);
    assert!(!allowed(nobody, "operations.view", None).await);
}

#[tokio::test]
async fn the_check_always_answers_and_follows_access_changes() {
    let (app, _audit) = app().await;
    let checks = app.ctx().get_all::<dyn PermissionCheck>().await.unwrap();
    assert_eq!(checks.len(), 1, "AccessModule registers one check");
    let user = Uuid::now_v7();
    let who = principal(user);
    assert_eq!(
        checks[0].check(&who, "reports.export", None).await.unwrap(),
        Some(false),
        "app-wide access always has an answer"
    );

    give_role(&app, user, "analyst", &["reports.export"]).await;
    assert_eq!(
        checks[0].check(&who, "reports.export", None).await.unwrap(),
        Some(true)
    );
    let factory = app.get::<AbilityFactory>().await;
    assert!(
        factory
            .has_permission(&who, "reports.export")
            .await
            .unwrap()
    );
    assert!(
        !factory
            .has_permission(&who, "reports.delete")
            .await
            .unwrap()
    );
}
