//! Route authorization: roles, permissions and `can` declarations.

use navree_contracts::{AuthVia, Principal};
use navree_testing::principal;
use serde_json::json;
use uuid::Uuid;

use crate::support::{SUPER_ADMIN, app, give_role, insert_post};

#[tokio::test]
async fn role_route_allows_matching_role_and_denies_other() {
    let (app, _audit) = app().await;
    let (owner, editor) = (Uuid::now_v7(), Uuid::now_v7());
    give_role(&app, owner, "owner", &[]).await;
    give_role(&app, editor, "editor", &[]).await;

    app.acting_as(principal(owner))
        .get("/admin/dashboard")
        .send()
        .await
        .assert_status(200);
    app.acting_as(principal(editor))
        .get("/admin/dashboard")
        .send()
        .await
        .assert_problem(403, "forbidden");
    app.client()
        .get("/admin/dashboard")
        .send()
        .await
        .assert_problem(401, "unauthenticated");
}

#[tokio::test]
async fn permission_route_requires_unconditional_rule() {
    let (app, _audit) = app().await;
    let (author, publisher) = (Uuid::now_v7(), Uuid::now_v7());
    give_role(&app, publisher, "publisher", &["posts.publish"]).await;
    let post = insert_post(&app, "Draft", Some(author), "draft", false).await;
    let path = format!("/api/posts/{}/publish", post.id);

    // The author may publish their own post (a conditional rule), which is
    // not enough for a permission check.
    app.acting_as(principal(author))
        .post(&path)
        .send()
        .await
        .assert_problem(403, "forbidden");
    app.acting_as(principal(publisher))
        .post(&path)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("published", json!(post.id));
}

#[tokio::test]
async fn can_from_loads_record_and_checks_conditions() {
    let (app, _audit) = app().await;
    let (owner, other) = (Uuid::now_v7(), Uuid::now_v7());
    let post = insert_post(&app, "Mine", Some(owner), "draft", false).await;
    let path = format!("/api/posts/{}", post.id);

    app.acting_as(principal(owner))
        .patch(&path)
        .json(&json!({ "title": "Renamed" }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("title", json!("Renamed"));
    app.acting_as(principal(other))
        .patch(&path)
        .json(&json!({ "title": "Stolen" }))
        .send()
        .await
        .assert_problem(403, "forbidden");

    // The handler receives the record the authorizer loaded.
    app.acting_as(principal(owner))
        .get(&path)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("title", json!("Renamed"))
        .assert_json_path("author_id", json!(owner));
    // An unpublished post of someone else is not readable.
    app.acting_as(principal(other))
        .get(&path)
        .send()
        .await
        .assert_problem(403, "forbidden");
}

#[tokio::test]
async fn can_from_missing_record_is_404() {
    let (app, _audit) = app().await;
    let user = Uuid::now_v7();
    for id in [Uuid::now_v7().to_string(), "not-a-uuid".to_owned()] {
        app.acting_as(principal(user))
            .get(&format!("/api/posts/{id}"))
            .send()
            .await
            .assert_problem(404, "not_found");
    }
}

#[tokio::test]
async fn type_level_denial_comes_before_loading_the_record() {
    let (app, _audit) = app().await;
    let user = Uuid::now_v7();
    // No rule grants `archive`, so the record is never looked up: a missing
    // post is a 403, not a 404 (no existence leak).
    app.acting_as(principal(user))
        .post(&format!("/api/posts/{}/archive", Uuid::now_v7()))
        .send()
        .await
        .assert_problem(403, "forbidden");
}

#[tokio::test]
async fn check_fields_rejects_fields_the_rule_does_not_cover() {
    let (app, _audit) = app().await;
    let owner = Uuid::now_v7();
    let post = insert_post(&app, "Mine", Some(owner), "draft", false).await;

    app.acting_as(principal(owner))
        .patch(&format!("/api/posts/{}", post.id))
        .json(&json!({ "title": "Moved", "author_id": Uuid::now_v7() }))
        .send()
        .await
        .assert_problem(403, "forbidden")
        .assert_json_path(
            "detail",
            json!("You may not change these fields: author_id."),
        );
}

#[tokio::test]
async fn accessible_filters_list_to_permitted_rows() {
    let (app, _audit) = app().await;
    let (alice, bob) = (Uuid::now_v7(), Uuid::now_v7());
    insert_post(&app, "a published", Some(bob), "open", true).await;
    insert_post(&app, "b alice draft", Some(alice), "draft", false).await;
    insert_post(&app, "c bob draft", Some(bob), "draft", false).await;
    insert_post(&app, "d orphan draft", None, "draft", false).await;

    app.acting_as(principal(alice))
        .get("/api/posts")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("titles", json!(["a published", "b alice draft"]));
    app.acting_as(principal(bob))
        .get("/api/posts")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("titles", json!(["a published", "c bob draft"]));
}

fn token(user: Uuid, abilities: &[&str]) -> Principal {
    Principal {
        user_id: user,
        via: AuthVia::Token,
        token_abilities: Some(abilities.iter().map(|a| (*a).to_owned()).collect()),
        session_id: None,
    }
}

#[tokio::test]
async fn token_principal_restricted_to_token_abilities() {
    let (app, _audit) = app().await;
    let user = Uuid::now_v7();
    give_role(&app, user, "publisher", &["posts.publish", "posts.read"]).await;
    let post = insert_post(&app, "Theirs", Some(Uuid::now_v7()), "draft", false).await;
    let publish = format!("/api/posts/{}/publish", post.id);

    // A token without `posts.publish` cannot publish, though the user can.
    app.acting_as(token(user, &["posts.read"]))
        .post(&publish)
        .send()
        .await
        .assert_problem(403, "forbidden");
    app.acting_as(token(user, &["posts.read"]))
        .get(&format!("/api/posts/{}", post.id))
        .send()
        .await
        .assert_status(200);
    app.acting_as(token(user, &["posts.publish"]))
        .post(&publish)
        .send()
        .await
        .assert_status(200);
    // `*` keeps every ability of the user.
    app.acting_as(token(user, &["*"]))
        .post(&publish)
        .send()
        .await
        .assert_status(200);
    // Roles are the user's, whatever the token.
    give_role(&app, user, "admin", &[]).await;
    app.acting_as(token(user, &[]))
        .get("/admin/dashboard")
        .send()
        .await
        .assert_status(200);
}

/// A permission whose prefix is no registered subject (`reports.view`,
/// `audit.read`) is granted by holding exactly that permission, through a
/// role or a token, like any other.
#[tokio::test]
async fn permission_of_an_unregistered_prefix_is_granted_by_holding_it() {
    let (app, _audit) = app().await;
    let (analyst, author) = (Uuid::now_v7(), Uuid::now_v7());
    give_role(&app, analyst, "analyst", &["reports.view"]).await;
    give_role(&app, author, "author", &["posts.publish", "reports.export"]).await;

    app.acting_as(principal(analyst))
        .get("/reports/sales")
        .send()
        .await
        .assert_status(200);
    app.acting_as(principal(author))
        .get("/reports/sales")
        .send()
        .await
        .assert_problem(403, "forbidden");
    app.acting_as(token(analyst, &["reports.view"]))
        .get("/reports/sales")
        .send()
        .await
        .assert_status(200);
    app.acting_as(token(analyst, &["reports.*"]))
        .get("/reports/sales")
        .send()
        .await
        .assert_status(200);
    app.acting_as(token(analyst, &["posts.read"]))
        .get("/reports/sales")
        .send()
        .await
        .assert_problem(403, "forbidden");
}

#[tokio::test]
async fn super_admin_role_passes_everything() {
    let (app, _audit) = app().await;
    let root = Uuid::now_v7();
    give_role(&app, root, SUPER_ADMIN, &[]).await;
    let locked = insert_post(&app, "Locked", Some(Uuid::now_v7()), "locked", false).await;
    let path = format!("/api/posts/{}", locked.id);
    let client = app.acting_as(principal(root));

    client
        .get("/admin/dashboard")
        .send()
        .await
        .assert_status(200);
    client
        .post(&format!("{path}/publish"))
        .send()
        .await
        .assert_status(200);
    client
        .post(&format!("{path}/archive"))
        .send()
        .await
        .assert_status(200);
    client
        .patch(&path)
        .json(&json!({ "author_id": root }))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("author_id", json!(root));
    // Even past a `cannot` rule (locked posts cannot be deleted).
    client.delete(&path).send().await.assert_status(204);
    client
        .get("/api/posts")
        .send()
        .await
        .assert_json_path("titles", json!(["Locked"]));
}
