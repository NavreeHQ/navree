//! The OAuth tables as navree-orm models: clients and personal access
//! tokens written by the server read back as models, queried with typed
//! columns.

use navree_oauth::{Client, PersonalAccessToken};
use navree_orm::Model as _;
use serde_json::json;

use crate::support::{World, string};

#[tokio::test]
async fn clients_and_personal_access_tokens_are_orm_models() {
    let world = World::new().await;
    let created = world.confidential_client(&["deals:read"]).await;
    world.public_client(&["deals:read"]).await;
    let (user, token) = world.user("rep@acme.test", &["deals.read"]).await;
    let pat = world
        .client()
        .post("/api/oauth/personal-access-tokens")
        .bearer(&token)
        .json(&json!({ "name": "Weekly pipeline report", "scopes": ["deals:read"] }))
        .send()
        .await
        .assert_status(201)
        .json_value();
    let db = world.db().await;

    assert_eq!(Client::table_name(), "oauth_clients");
    let zapier = Client::find(&db, created.client.id)
        .await
        .unwrap()
        .expect("the client");
    assert_eq!(zapier.name, "Zapier");
    assert!(
        zapier.to_value().unwrap().get("secret_hash").is_none(),
        "secrets never serialize"
    );
    assert_eq!(
        Client::query()
            .where_null(Client::SECRET_HASH)
            .pluck(&db, Client::NAME)
            .await
            .unwrap(),
        ["Agra mobile"],
        "public clients have no secret"
    );

    let tokens = PersonalAccessToken::query()
        .where_(PersonalAccessToken::USER_ID.eq(user.id))
        .where_null(PersonalAccessToken::REVOKED_AT)
        .get(&db)
        .await
        .unwrap();
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].id.to_string(), string(&pat, "id"));
    assert_eq!(tokens[0].scopes, "deals:read");
}
