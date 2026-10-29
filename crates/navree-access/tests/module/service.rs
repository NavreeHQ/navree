//! `AccessService`, `AbilityFactory` caching and the audit trail.

use std::sync::Arc;

use navree_access::{AbilityFactory, AccessError, AccessService};
use navree_contracts::AuditKind;
use navree_db::Db;
use navree_testing::principal;
use serde_json::json;
use uuid::Uuid;

use crate::support::{app, give_role, in_uow, insert_post};

#[tokio::test]
async fn version_bump_invalidates_cached_ability() {
    let (app, _audit) = app().await;
    let factory = app.get::<AbilityFactory>().await;
    let service = app.get::<AccessService>().await;
    let user = Some(principal(Uuid::now_v7()));
    give_role(&app, user.as_ref().unwrap().user_id, "writer", &[]).await;

    let before = factory.for_principal(&user).await.unwrap();
    assert!(!before.has_unconditional("publish", "Post"));
    // Unchanged access version: the cached ability is reused.
    let again = factory.for_principal(&user).await.unwrap();
    assert!(Arc::ptr_eq(&before, &again));

    let version = service.version().await.unwrap();
    in_uow(&app, |service, uow| async move {
        service.grant(&uow, "writer", "posts.publish").await
    })
    .await;
    assert_eq!(service.version().await.unwrap(), version + 1);

    let after = factory.for_principal(&user).await.unwrap();
    assert!(after.has_unconditional("publish", "Post"));
    assert!(!Arc::ptr_eq(&before, &after));
}

#[tokio::test]
async fn unchanged_writes_do_not_bump_the_version() {
    let (app, audit) = app().await;
    let service = app.get::<AccessService>().await;
    let user = Uuid::now_v7();
    give_role(&app, user, "writer", &["posts.publish"]).await;
    let version = service.version().await.unwrap();
    let entries = audit.entries().len();

    let changed = in_uow(&app, |service, uow| async move {
        Ok([
            service.grant(&uow, "writer", "posts.publish").await?,
            service.assign_role(&uow, user, "writer").await?,
            service.revoke(&uow, "writer", "posts.delete").await?,
            service.remove_role(&uow, Uuid::now_v7(), "writer").await?,
        ])
    })
    .await;
    assert_eq!(changed, [false; 4]);
    assert_eq!(service.version().await.unwrap(), version);
    assert_eq!(audit.entries().len(), entries);
}

#[tokio::test]
async fn role_changes_write_access_audit_entries_after_commit() {
    let (app, audit) = app().await;
    let user = Uuid::now_v7();
    give_role(&app, user, "editor", &["posts.update"]).await;

    let access: Vec<_> = audit
        .entries()
        .into_iter()
        .filter(|e| e.kind == AuditKind::Access)
        .collect();
    let actions: Vec<&str> = access.iter().map(|e| e.action.as_str()).collect();
    assert_eq!(
        actions,
        [
            "access.role_created",
            "access.permission_granted",
            "access.role_assigned"
        ]
    );
    assert_eq!(access[1].subject_type.as_deref(), Some("role"));
    assert_eq!(access[1].subject_id.as_deref(), Some("editor"));
    assert_eq!(
        access[1].new_values,
        Some(json!({ "permission": "posts.update" }))
    );
    assert_eq!(access[2].subject_id, Some(user.to_string()));

    // A rolled-back change writes nothing and leaves the version alone.
    let db = app.get::<Db>().await;
    let service = app.get::<AccessService>().await;
    let version = service.version().await.unwrap();
    let uow = db.uow(Some(user), Some("req-1".into())).await;
    service.create_role(&uow, "temporary", None).await.unwrap();
    uow.rollback().await.unwrap();
    assert_eq!(audit.entries().len(), 3);
    assert_eq!(service.version().await.unwrap(), version);
    assert!(service.role("temporary").await.unwrap().is_none());

    // The actor and request id of the unit of work are recorded.
    let uow = db.uow(Some(user), Some("req-2".into())).await;
    service
        .create_role(&uow, "kept", Some("Kept"))
        .await
        .unwrap();
    uow.commit().await.unwrap();
    let last = audit.entries().pop().unwrap();
    assert_eq!(last.action, "access.role_created");
    assert_eq!(last.actor_id, Some(user));
    assert_eq!(last.request_id.as_deref(), Some("req-2"));
}

#[tokio::test]
async fn denial_writes_security_audit_entry() {
    let (app, audit) = app().await;
    let (owner, other) = (Uuid::now_v7(), Uuid::now_v7());
    let post = insert_post(&app, "Mine", Some(owner), "draft", false).await;

    app.acting_as(principal(other))
        .patch(&format!("/api/posts/{}", post.id))
        .json(&json!({ "title": "Stolen" }))
        .header("user-agent", "probe/1.0")
        .send()
        .await
        .assert_error(403, None);
    app.acting_as(principal(other))
        .get("/admin/dashboard")
        .send()
        .await
        .assert_status(403);

    let denials: Vec<_> = audit
        .entries()
        .into_iter()
        .filter(|e| e.kind == AuditKind::Security)
        .collect();
    assert_eq!(denials.len(), 2);
    let record = &denials[0];
    assert_eq!(record.action, "access.denied");
    assert_eq!(record.actor_id, Some(other));
    assert_eq!(record.subject_type.as_deref(), Some("Post"));
    assert_eq!(record.subject_id, Some(post.id.to_string()));
    assert_eq!(record.ip.as_deref(), Some("127.0.0.1"));
    assert_eq!(record.user_agent.as_deref(), Some("probe/1.0"));
    assert!(record.request_id.is_some());
    assert_eq!(record.metadata["check"], json!("can"));
    assert_eq!(record.metadata["action"], json!("update"));
    assert_eq!(record.metadata["route"], json!("/api/posts/{id}"));
    assert_eq!(record.metadata["method"], json!("PATCH"));
    assert_eq!(denials[1].metadata["check"], json!("roles"));
    assert_eq!(denials[1].metadata["roles"], json!(["admin", "owner"]));
}

#[tokio::test]
async fn sync_methods_replace_permissions_and_roles() {
    let (app, _audit) = app().await;
    let user = Uuid::now_v7();
    give_role(&app, user, "editor", &["posts.update", "posts.delete"]).await;
    give_role(&app, Uuid::now_v7(), "viewer", &[]).await;

    let role = in_uow(&app, |service, uow| async move {
        service
            .sync_role_permissions(&uow, "editor", ["posts.read", "posts.update"])
            .await
    })
    .await;
    assert_eq!(role.permissions, ["posts.read", "posts.update"]);
    let roles = in_uow(&app, |service, uow| async move {
        service.sync_user_roles(&uow, user, ["viewer"]).await
    })
    .await;
    assert_eq!(roles, ["viewer"]);

    let service = app.get::<AccessService>().await;
    assert_eq!(service.roles_of(user).await.unwrap(), ["viewer"]);
    assert!(service.permissions_of(user).await.unwrap().is_empty());
    let names: Vec<String> = service
        .roles()
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.name)
        .collect();
    assert_eq!(names, ["editor", "viewer"]);

    let db = app.get::<Db>().await;
    let uow = db.uow(None, None).await;
    let error = service
        .sync_user_roles(&uow, user, ["viewer", "nope"])
        .await
        .unwrap_err();
    assert!(matches!(error, AccessError::UnknownRoles(ref r) if r == &["nope"]));
    let error = service.grant(&uow, "nope", "posts.read").await.unwrap_err();
    assert!(matches!(error, AccessError::RoleNotFound(_)));
    let error = service.grant(&uow, "viewer", "nodot").await.unwrap_err();
    assert!(matches!(error, AccessError::InvalidName { .. }));
    uow.rollback().await.unwrap();
}

#[tokio::test]
async fn direct_permissions_count_and_deleting_a_role_removes_its_grants() {
    let (app, _audit) = app().await;
    let user = Uuid::now_v7();
    give_role(&app, user, "editor", &["posts.update"]).await;
    in_uow(&app, |service, uow| async move {
        service.grant_to_user(&uow, user, "posts.publish").await
    })
    .await;
    let service = app.get::<AccessService>().await;
    assert_eq!(
        service.permissions_of(user).await.unwrap(),
        ["posts.publish", "posts.update"]
    );

    in_uow(&app, |service, uow| async move {
        service.delete_role(&uow, "editor").await
    })
    .await;
    assert!(service.roles_of(user).await.unwrap().is_empty());
    assert_eq!(
        service.permissions_of(user).await.unwrap(),
        ["posts.publish"]
    );

    in_uow(&app, |service, uow| async move {
        service.revoke_from_user(&uow, user, "posts.publish").await
    })
    .await;
    assert!(service.permissions_of(user).await.unwrap().is_empty());
}

/// An `AuditSink` that writes rows on its own pool connection, like
/// navree-audit does.
#[derive(Clone, Default)]
struct DbAudit(Arc<std::sync::OnceLock<Db>>);

impl navree_contracts::AuditSink for DbAudit {
    fn record(
        &self,
        entry: navree_contracts::AuditEntry,
    ) -> navree_contracts::BoxFuture<'_, Result<(), navree_contracts::BoxError>> {
        Box::pin(async move {
            use navree_db::{Executor, Statement};
            let db = self.0.get().expect("database set");
            let pool = db.pool();
            pool.execute(&Statement::portable(
                "INSERT INTO denials (action) VALUES (?)",
                [entry.action.into()],
            ))
            .await?;
            Ok(())
        })
    }
}

#[tokio::test]
async fn denial_audit_does_not_wait_for_the_request_transaction() {
    use navree_db::{Executor, ExecutorExt, Statement};

    let sink = DbAudit::default();
    let app = crate::support::builder(&crate::support::RecordingAudit::default())
        .override_value::<dyn navree_contracts::AuditSink>(Arc::new(sink.clone()))
        .build()
        .await;
    let db = app.get::<Db>().await;
    db.execute_script("CREATE TABLE denials (action TEXT NOT NULL)")
        .await
        .unwrap();
    sink.0.set((*db).clone()).ok();
    let post = insert_post(&app, "Theirs", Some(Uuid::now_v7()), "draft", false).await;

    let started = std::time::Instant::now();
    app.acting_as(principal(Uuid::now_v7()))
        .patch(&format!("/api/posts/{}", post.id))
        .json(&json!({ "title": "Stolen" }))
        .send()
        .await
        .assert_error(403, None);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(2),
        "the denial took {:?}",
        started.elapsed()
    );
    let denied: i64 = db
        .fetch_scalar(&Statement::text(
            "SELECT count(*) AS n FROM denials WHERE action = 'access.denied'",
        ))
        .await
        .unwrap();
    assert_eq!(denied, 1);
}

/// Records `end_user_subscriptions` calls.
#[derive(Default)]
struct RecordingBroadcaster(std::sync::Mutex<Vec<Uuid>>);

impl RecordingBroadcaster {
    fn take(&self) -> Vec<Uuid> {
        let mut ended = std::mem::take(&mut *self.0.lock().unwrap());
        ended.sort();
        ended
    }
}

impl navree_contracts::Broadcaster for RecordingBroadcaster {
    fn broadcast(
        &self,
        _channels: Vec<String>,
        _event: String,
        _data: serde_json::Value,
    ) -> navree_contracts::BoxFuture<'_, Result<(), navree_contracts::BoxError>> {
        Box::pin(async { Ok(()) })
    }

    fn end_user_subscriptions(
        &self,
        user_id: Uuid,
    ) -> navree_contracts::BoxFuture<'_, Result<(), navree_contracts::BoxError>> {
        self.0.lock().unwrap().push(user_id);
        Box::pin(async { Ok(()) })
    }
}

/// PRD "Where rules are enforced" #5: a user whose access changes, or the
/// members of a role that loses permissions, get their real-time
/// subscriptions ended once the change commits (never for a rolled-back
/// change, and not for changes that only widen a role).
#[tokio::test]
async fn access_changes_end_the_affected_users_subscriptions() {
    let audit = crate::support::RecordingAudit::default();
    let broadcaster = Arc::new(RecordingBroadcaster::default());
    let app = crate::support::builder(&audit)
        .override_value::<dyn navree_contracts::Broadcaster>(broadcaster.clone())
        .build()
        .await;
    let (ada, bob, cy) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    give_role(&app, ada, "writer", &["posts.create"]).await;
    give_role(&app, bob, "writer", &[]).await;
    give_role(&app, cy, "editor", &["posts.publish"]).await;
    let mut assigned = vec![ada, bob, cy];
    assigned.sort();
    assert_eq!(broadcaster.take(), assigned, "role assignments");

    // Widening a role ends nothing.
    in_uow(&app, |service, uow| async move {
        service.grant(&uow, "writer", "posts.update").await
    })
    .await;
    assert!(broadcaster.take().is_empty());

    // Narrowing it ends its members' subscriptions.
    in_uow(&app, |service, uow| async move {
        service.revoke(&uow, "writer", "posts.create").await
    })
    .await;
    let mut writers = vec![ada, bob];
    writers.sort();
    assert_eq!(broadcaster.take(), writers);
    in_uow(&app, |service, uow| async move {
        service
            .sync_role_permissions(&uow, "writer", ["posts.update"])
            .await
    })
    .await;
    assert!(broadcaster.take().is_empty(), "nothing changed");

    // User-level changes.
    in_uow(&app, |service, uow| async move {
        service.sync_user_roles(&uow, ada, ["editor"]).await
    })
    .await;
    assert_eq!(broadcaster.take(), [ada]);

    // A rolled-back change ends nothing.
    let db = app.get::<Db>().await;
    let service = app.get::<AccessService>().await;
    let uow = db.uow(None, None).await;
    service.remove_role(&uow, cy, "editor").await.unwrap();
    uow.rollback().await.unwrap();
    assert!(broadcaster.take().is_empty());

    // Deleting a role ends every member's subscriptions.
    in_uow(&app, |service, uow| async move {
        service.delete_role(&uow, "editor").await
    })
    .await;
    let mut editors = vec![ada, cy];
    editors.sort();
    assert_eq!(broadcaster.take(), editors);
}
