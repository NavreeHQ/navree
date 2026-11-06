//! The consent page the module draws with Navree UI: what the page shows for a request, the
//! decisions it posts, and who may use it.
#![cfg(feature = "ui")]

use navree_oauth::testing::pkce_pair;

use crate::support::{REDIRECT_URI, World, param};

#[tokio::test]
async fn the_page_shows_the_request_and_the_decisions_go_back_to_the_client() {
    let world = World::new().await;
    let (_, token) = world.user("rep@acme.test", &["deals.read"]).await;
    let client = world.public_client(&["deals:read", "offline_access"]).await;
    let (_, challenge) = pkce_pair();
    let request = world
        .request(client.client.id, "deals:read offline_access", &challenge)
        .await;

    let page = world
        .client()
        .get(&format!("/oauth/consent?request={request}"))
        .bearer(&token)
        .send()
        .await
        .assert_status(200)
        .text();
    assert!(page.contains("Authorize Acme mobile"), "{page}");
    assert!(
        page.contains("deals:read") && page.contains("Read your deals"),
        "{page}"
    );
    assert!(
        page.contains(&format!("/oauth/consent/{request}/approve"))
            && page.contains(&format!("/oauth/consent/{request}/deny")),
        "{page}"
    );

    // Allow: the browser goes back with a code and the state, as the JSON route's would.
    let approved = world
        .client()
        .post(&format!("/oauth/consent/{request}/approve"))
        .bearer(&token)
        .send()
        .await;
    assert!(approved.status().is_redirection(), "{}", approved.text());
    let location = approved.header("location").expect("a location").to_owned();
    assert!(
        location.starts_with(&format!("{REDIRECT_URI}?code=nvc_")),
        "{location}"
    );
    assert_eq!(param(&location, "state").as_deref(), Some("af0ifjsldkj"));

    // The request was decided once: the page no longer finds it.
    world
        .client()
        .get(&format!("/oauth/consent?request={request}"))
        .bearer(&token)
        .send()
        .await
        .assert_error(404, None);
}

#[tokio::test]
async fn deny_sends_the_client_access_denied() {
    let world = World::new().await;
    let (_, token) = world.user("rep@acme.test", &["deals.read"]).await;
    let client = world.public_client(&["deals:read"]).await;
    let (_, challenge) = pkce_pair();
    let request = world
        .request(client.client.id, "deals:read", &challenge)
        .await;
    let denied = world
        .client()
        .post(&format!("/oauth/consent/{request}/deny"))
        .bearer(&token)
        .send()
        .await;
    assert!(denied.status().is_redirection(), "{}", denied.text());
    let location = denied.header("location").expect("a location").to_owned();
    assert_eq!(param(&location, "error").as_deref(), Some("access_denied"));
    assert_eq!(param(&location, "state").as_deref(), Some("af0ifjsldkj"));
}

#[tokio::test]
async fn a_guest_and_a_token_are_refused() {
    let world = World::new().await;
    let (_, token) = world.user("rep@acme.test", &["deals.read"]).await;
    let client = world.public_client(&["deals:read"]).await;
    let (_, challenge) = pkce_pair();
    let request = world
        .request(client.client.id, "deals:read", &challenge)
        .await;
    let path = format!("/oauth/consent?request={request}");

    // Nobody signed in.
    world.client().get(&path).send().await.assert_status(401);

    // An OAuth token may not decide for a person: get one, then try.
    let tokens = world
        .code_flow(client.client.id, &token, "deals:read")
        .await;
    let access = tokens["access_token"].as_str().expect("a token");
    let other = world
        .request(client.client.id, "deals:read", &pkce_pair().1)
        .await;
    world
        .client()
        .post(&format!("/oauth/consent/{other}/approve"))
        .bearer(access)
        .send()
        .await
        .assert_status(403);
}
