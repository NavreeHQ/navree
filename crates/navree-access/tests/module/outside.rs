//! Access checks outside an HTTP request (MCP tools, jobs): declarations
//! with `AbilityFactory::allows`, record checks with
//! `AbilityFactory::authorize_record` and `Authorized::load`.

use std::sync::Arc;

use http::StatusCode;
use navree_access::{AbilityFactory, Authorized, Read, Update};
use navree_db::Db;
use navree_http::{AccessSpec, CanSpec};
use navree_testing::principal;
use uuid::Uuid;

use crate::support::{Post, SUPER_ADMIN, app, give_role, insert_post};

fn spec() -> AccessSpec {
    AccessSpec::default()
}

fn can(action: &str, subject: &str) -> AccessSpec {
    AccessSpec {
        can: Some(CanSpec {
            action: action.to_owned(),
            subject: subject.to_owned(),
            from: None,
        }),
        ..AccessSpec::default()
    }
}

#[tokio::test]
async fn allows_checks_roles_permissions_and_type_level_abilities() {
    let (app, _audit) = app().await;
    let factory = app.get::<AbilityFactory>().await;
    let writer = Uuid::now_v7();
    give_role(&app, writer, "writer", &["posts.publish", "reports.view"]).await;
    let access = factory.access_for(Some(&principal(writer))).await.unwrap();

    assert!(factory.allows(&access, &spec()));
    let roles = |roles: &[&str]| AccessSpec {
        roles: roles.iter().map(|r| (*r).to_owned()).collect(),
        ..AccessSpec::default()
    };
    assert!(factory.allows(&access, &roles(&["admin", "writer"])));
    assert!(!factory.allows(&access, &roles(&["admin"])));

    let permissions = |permissions: &[&str]| AccessSpec {
        permissions: permissions.iter().map(|p| (*p).to_owned()).collect(),
        ..AccessSpec::default()
    };
    assert!(factory.allows(&access, &permissions(&["posts.publish", "reports.view"])));
    assert!(!factory.allows(&access, &permissions(&["posts.delete"])));

    // Users may update their own posts: the type-level check passes.
    assert!(factory.allows(&access, &can("update", "Post")));
    assert!(!factory.allows(&access, &can("archive", "Post")));

    // A guest has no roles and only conditional rules.
    let guest = factory.access_for(None).await.unwrap();
    assert!(!factory.allows(&guest, &roles(&["writer"])));
    assert!(factory.allows(&guest, &can("read", "Post")));
    assert!(!factory.allows(&guest, &can("update", "Post")));
}

#[tokio::test]
async fn the_super_admin_role_passes_role_checks() {
    let (app, _audit) = app().await;
    let factory = app.get::<AbilityFactory>().await;
    let boss = Uuid::now_v7();
    give_role(&app, boss, SUPER_ADMIN, &[]).await;
    let access = factory.access_for(Some(&principal(boss))).await.unwrap();
    let spec = AccessSpec {
        roles: vec!["editor".to_owned()],
        permissions: vec!["posts.delete".to_owned()],
        ..can("archive", "Post")
    };
    assert!(factory.allows(&access, &spec));
}

#[tokio::test]
async fn permission_targets_follow_the_registered_prefixes() {
    let (app, _audit) = app().await;
    let factory = app.get::<AbilityFactory>().await;
    assert_eq!(
        factory.permission_target("posts.publish"),
        ("publish", "Post")
    );
    assert_eq!(factory.permission_target("posts.*"), ("manage", "Post"));
    assert_eq!(
        factory.permission_target("reports.view"),
        ("view", "reports")
    );
}

#[tokio::test]
async fn authorize_record_loads_by_subject_and_checks_the_record() {
    let (app, _audit) = app().await;
    let factory = app.get::<AbilityFactory>().await;
    let db = app.get::<Db>().await;
    let author = Uuid::now_v7();
    let own = insert_post(&app, "Mine", Some(author), "draft", false).await;
    let other = insert_post(&app, "Theirs", Some(Uuid::now_v7()), "draft", false).await;
    let access = factory.access_for(Some(&principal(author))).await.unwrap();
    let uow = db.uow(Some(author), None).await;

    let record = factory
        .authorize_record(&access, &uow, "update", "Post", &own.id.to_string())
        .await
        .expect("own post");
    assert_eq!(record.subject(), "Post");
    assert_eq!(record.action(), "update");
    assert_eq!(record.record()["title"], "Mine");
    assert!(
        record.clone().into_authorized::<Post, Read>().is_none(),
        "the action must match"
    );
    let typed: Authorized<Post, Update> = record.into_authorized().expect("typed");
    assert_eq!(typed.model().id, own.id);

    let denied = factory
        .authorize_record(&access, &uow, "update", "Post", &other.id.to_string())
        .await
        .expect_err("someone else's post");
    assert_eq!(denied.status, StatusCode::FORBIDDEN);

    let missing = factory
        .authorize_record(&access, &uow, "update", "Post", &Uuid::now_v7().to_string())
        .await
        .expect_err("no such post");
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    let malformed = factory
        .authorize_record(&access, &uow, "update", "Post", "not-a-uuid")
        .await
        .expect_err("not an id");
    assert_eq!(malformed.status, StatusCode::NOT_FOUND);

    let unknown = factory
        .authorize_record(&access, &uow, "read", "Invoice", "1")
        .await
        .expect_err("no loader");
    assert_eq!(unknown.status, StatusCode::INTERNAL_SERVER_ERROR);
    uow.rollback().await.unwrap();
}

#[tokio::test]
async fn authorized_load_checks_a_typed_record() {
    let (app, _audit) = app().await;
    let factory = app.get::<AbilityFactory>().await;
    let db = app.get::<Db>().await;
    let author = Uuid::now_v7();
    let own = insert_post(&app, "Mine", Some(author), "draft", false).await;
    let published = insert_post(&app, "Public", Some(Uuid::now_v7()), "draft", true).await;
    let hidden = insert_post(&app, "Hidden", Some(Uuid::now_v7()), "draft", false).await;
    let ability = factory
        .for_principal(&Some(principal(author)))
        .await
        .unwrap();
    let uow = db.uow(Some(author), None).await;

    let post = Authorized::<Post, Update>::load(&uow, Arc::clone(&ability), &own.id.to_string())
        .await
        .expect("own post");
    assert_eq!(post.into_inner().title, "Mine");
    let post =
        Authorized::<Post, Read>::load(&uow, Arc::clone(&ability), &published.id.to_string())
            .await
            .expect("published post");
    assert_eq!(post.title, "Public");

    let denied = Authorized::<Post, Read>::load(&uow, Arc::clone(&ability), &hidden.id.to_string())
        .await
        .expect_err("hidden post");
    assert_eq!(denied.status, StatusCode::FORBIDDEN);
    let missing = Authorized::<Post, Read>::load(&uow, ability, &Uuid::now_v7().to_string())
        .await
        .expect_err("missing post");
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    uow.rollback().await.unwrap();
}
