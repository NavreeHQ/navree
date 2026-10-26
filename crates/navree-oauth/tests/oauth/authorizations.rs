//! The user's authorized apps: listing and revoking them.

use navree_oauth::testing::authorize;
use serde_json::json;

use crate::support::World;

#[tokio::test]
async fn users_see_and_revoke_the_apps_they_authorized() {
    let world = World::new().await;
    let (user, token) = world.user("rep@acme.test", &["deals.read"]).await;
    let client = world
        .confidential_client(&["deals:read", "offline_access"])
        .await;
    let public = world.public_client(&["deals:read"]).await;
    let tokens = navree_oauth::testing::AuthorizeFlow::new(client.client.id)
        .secret(client.secret.clone().unwrap())
        .run(&world.client(), &token, &["deals:read", "offline_access"])
        .await;
    let other = authorize(&world.client(), public.client.id, &token, &["deals:read"]).await;

    let listed = world
        .client()
        .get("/api/oauth/authorizations")
        .bearer(&token)
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(listed["data"].as_array().unwrap().len(), 2);
    let zapier = listed["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["client"]["id"] == json!(client.client.id))
        .unwrap();
    assert_eq!(zapier["client"]["name"], "Zapier");
    assert_eq!(zapier["client"]["website"], "https://zapier.test");
    assert_eq!(zapier["scopes"][0]["name"], "deals:read");

    world
        .client()
        .delete(&format!("/api/oauth/authorizations/{}", client.client.id))
        .bearer(&token)
        .send()
        .await
        .assert_status(204);
    world
        .client()
        .get("/api/probe/whoami")
        .bearer(&tokens.access_token)
        .send()
        .await
        .assert_status(401);
    world
        .post_form(
            "/oauth/token",
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", tokens.refresh_token.as_deref().unwrap()),
                ("client_id", &client.client.id.to_string()),
                ("client_secret", client.secret.as_deref().unwrap()),
            ],
        )
        .await
        .assert_status(400);
    // The other app is untouched.
    world
        .client()
        .get("/api/probe/whoami")
        .bearer(&other.access_token)
        .send()
        .await
        .assert_status(200);
    world
        .client()
        .get("/api/oauth/authorizations")
        .bearer(&token)
        .send()
        .await
        .assert_json_path("data.0.client.id", json!(public.client.id));
    world
        .client()
        .delete(&format!("/api/oauth/authorizations/{}", client.client.id))
        .bearer(&token)
        .send()
        .await
        .assert_error(404, "Authorization not found.");
    let revoked = world.audit.with_action("oauth.revoked");
    assert_eq!(revoked.len(), 1);
    assert_eq!(revoked[0].actor_id, Some(user.id));

    // A token can't list or revoke authorizations.
    world
        .client()
        .get("/api/oauth/authorizations")
        .bearer(&other.access_token)
        .send()
        .await
        .assert_status(403);
}
