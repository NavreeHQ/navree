//! Personal access tokens: the routes, the service, the lifetime cap.

use std::time::Duration;

use navree_oauth::{OAuthError, PersonalAccessTokens};
use serde_json::json;

use crate::support::{World, builder, string};

#[tokio::test]
async fn users_create_list_and_revoke_tokens() {
    let world = World::new().await;
    let (user, token) = world
        .user("rep@acme.test", &["deals.read", "deals.create"])
        .await;

    let created = world
        .client()
        .post("/api/oauth/personal-access-tokens")
        .bearer(&token)
        .json(&json!({ "name": "Weekly pipeline report", "scopes": ["deals:read"], "expires_in_days": 90 }))
        .send()
        .await
        .assert_status(201)
        .assert_json_path("name", json!("Weekly pipeline report"))
        .assert_json_path("scopes", json!(["deals:read"]))
        .assert_json_path("expires_at", json!("2027-01-24T09:00:00Z"))
        .assert_json_path("last_used_at", json!(null))
        .json_value();
    let pat = string(&created, "token");
    assert!(pat.starts_with("nvp_"));
    let id = string(&created, "id");

    // It authenticates as the user, narrowed to its scopes.
    world
        .client()
        .get("/api/probe/whoami")
        .bearer(&pat)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("user_id", json!(user.id))
        .assert_json_path("token_abilities", json!(["deals.read"]))
        .assert_json_path("oauth.grant", json!("personal_access_token"))
        .assert_json_path("oauth.client_id", json!(null))
        .assert_json_path("oauth.client_name", json!("Weekly pipeline report"));
    world
        .client()
        .get("/api/probe/deals")
        .bearer(&pat)
        .send()
        .await
        .assert_status(200);
    world
        .client()
        .post("/api/probe/deals")
        .bearer(&pat)
        .send()
        .await
        .assert_status(403);

    // Listed without the secret, with its last use (recorded in the
    // background).
    let mut listed = serde_json::Value::Null;
    for _ in 0..100 {
        listed = world
            .client()
            .get("/api/oauth/personal-access-tokens")
            .bearer(&token)
            .send()
            .await
            .assert_status(200)
            .assert_json_path("data.0.id", json!(id))
            .json_value();
        if !listed["data"][0]["last_used_at"].is_null() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(listed["data"][0].get("token").is_none());
    assert_eq!(listed["data"][0]["last_used_at"], "2026-10-26T09:00:00Z");

    // A token can't manage tokens.
    world
        .client()
        .post("/api/oauth/personal-access-tokens")
        .bearer(&pat)
        .json(&json!({ "name": "escalate", "scopes": ["everything"] }))
        .send()
        .await
        .assert_status(403);

    world
        .client()
        .delete(&format!("/api/oauth/personal-access-tokens/{id}"))
        .bearer(&token)
        .send()
        .await
        .assert_status(204);
    world
        .client()
        .get("/api/probe/whoami")
        .bearer(&pat)
        .send()
        .await
        .assert_error(401, "Invalid token");
    world
        .client()
        .delete(&format!("/api/oauth/personal-access-tokens/{id}"))
        .bearer(&token)
        .send()
        .await
        .assert_status(404);
    assert_eq!(world.audit.with_action("oauth.pat_created").len(), 1);
    assert_eq!(world.audit.with_action("oauth.pat_revoked").len(), 1);
}

#[tokio::test]
async fn token_input_is_validated() {
    let world = World::new().await;
    let (_, token) = world.user("rep@acme.test", &["deals.read"]).await;
    let post = |body: serde_json::Value| {
        let (world, token) = (&world, token.clone());
        async move {
            world
                .client()
                .post("/api/oauth/personal-access-tokens")
                .bearer(&token)
                .json(&body)
                .send()
                .await
        }
    };
    post(json!({ "name": " ", "scopes": ["deals:read"] }))
        .await
        .assert_validation_error("name");
    post(json!({ "name": "x", "scopes": [] }))
        .await
        .assert_validation_error("scopes");
    post(json!({ "name": "x", "scopes": ["nope"] }))
        .await
        .assert_validation_error("scopes");
    post(json!({ "name": "x", "scopes": ["openid"] }))
        .await
        .assert_validation_error("scopes");
    post(json!({ "name": "x", "scopes": ["deals:read"], "expires_in_days": 366 }))
        .await
        .assert_validation_error("expires_in_days");
    post(json!({ "name": "x", "scopes": ["deals:read"], "expires_in_days": 0 }))
        .await
        .assert_validation_error("expires_in_days");
    // Without a lifetime: the longest allowed.
    post(json!({ "name": "x", "scopes": ["deals:read"] }))
        .await
        .assert_status(201)
        .assert_json_path("expires_at", json!("2027-10-26T09:00:00Z"));
}

#[tokio::test]
async fn max_days_zero_allows_tokens_that_never_expire() {
    let world = World::with(builder().config("oauth.personal_access_tokens.max_days", 0)).await;
    let (user, _) = world.user("rep@acme.test", &["deals.read"]).await;
    let pats = world.app.get::<PersonalAccessTokens>().await;
    let created = world
        .work(|uow| async move {
            Ok(pats
                .create(&uow, user.id, "forever", ["deals:read"], None)
                .await?)
        })
        .await;
    assert_eq!(created.record.expires_at, None);
    world.clock.advance(Duration::from_secs(10 * 365 * 86_400));
    world
        .client()
        .get("/api/probe/deals")
        .bearer(&created.token)
        .send()
        .await
        .assert_status(200);
}

#[tokio::test]
async fn the_service_creates_tokens_that_expire() {
    let world = World::new().await;
    let (user, _) = world.user("rep@acme.test", &["deals.read"]).await;
    let pats = world.app.get::<PersonalAccessTokens>().await;
    let thirty_days = Duration::from_secs(30 * 86_400);
    let service = pats.clone();
    let created = world
        .work(|uow| async move {
            Ok(service
                .create(
                    &uow,
                    user.id,
                    "CI deploy check",
                    ["deals:read"],
                    Some(thirty_days),
                )
                .await?)
        })
        .await;
    assert!(created.token.starts_with("nvp_"));
    assert!(!format!("{created:?}").contains(&created.token));
    assert_eq!(pats.list(user.id).await.unwrap().len(), 1);

    let too_long = Duration::from_secs(400 * 86_400);
    let db = world.db().await;
    let uow = db.uow(None, None).await;
    let error = pats
        .create(&uow, user.id, "x", ["deals:read"], Some(too_long))
        .await
        .unwrap_err();
    uow.rollback().await.unwrap();
    assert!(matches!(
        error,
        OAuthError::Invalid {
            field: "expires_in_days",
            ..
        }
    ));

    world.clock.advance(thirty_days + Duration::from_secs(1));
    world
        .client()
        .get("/api/probe/deals")
        .bearer(&created.token)
        .send()
        .await
        .assert_status(401);
}

#[tokio::test]
async fn the_routes_can_be_turned_off() {
    let world = World::with(builder().config("oauth.personal_access_tokens.enabled", false)).await;
    let (_, token) = world.user("rep@acme.test", &["deals.read"]).await;
    world
        .client()
        .get("/api/oauth/personal-access-tokens")
        .bearer(&token)
        .send()
        .await
        .assert_status(404);
}
