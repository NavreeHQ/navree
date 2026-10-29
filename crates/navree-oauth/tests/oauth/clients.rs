//! Clients: the `OAuthClients` service and the commands.

use navree_oauth::testing::AuthorizeFlow;
use navree_oauth::{NewClient, OAuthClients, OAuthError};
use serde_json::json;

use crate::support::{REDIRECT_URI, World};

#[tokio::test]
async fn clients_are_created_with_a_secret_shown_once() {
    let world = World::new().await;
    let (user, _) = world.user("dev@acme.test", &[]).await;
    let created = world
        .register(
            NewClient::confidential("Ada's reporting script")
                .owner(user.id)
                .redirect_uri("https://reports.lovelace.test/callback")
                .scopes(["deals:read"]),
        )
        .await;
    let secret = created.secret.clone().unwrap();
    assert!(secret.starts_with("nvcs_"));
    assert!(!format!("{created:?}").contains(&secret));
    let client = &created.client;
    assert!(!client.is_public());
    assert_eq!(client.scope_list(), ["deals:read"]);
    assert_eq!(client.grant_list(), ["authorization_code"]);
    assert_ne!(client.secret_hash.as_deref(), Some(secret.as_str()));
    // The record never serializes its secret digest.
    assert!(
        serde_json::to_value(client)
            .unwrap()
            .get("secret_hash")
            .is_none()
    );

    let clients = world.app.get::<OAuthClients>().await;
    assert_eq!(clients.owned_by(user.id).await.unwrap().len(), 1);
    assert_eq!(
        clients.find(client.id).await.unwrap().unwrap().name,
        "Ada's reporting script"
    );

    let public = world.public_client(&["deals:read"]).await;
    assert!(public.secret.is_none());
    assert!(public.client.is_public());
    assert_eq!(clients.list().await.unwrap().len(), 2);
    let created = world.audit.with_action("oauth.client_created");
    assert_eq!(created.len(), 2);
    assert_eq!(created[0].actor_id, Some(user.id));
}

#[tokio::test]
async fn invalid_clients_name_the_field() {
    let world = World::new().await;
    let clients = world.app.get::<OAuthClients>().await;
    let cases = [
        (
            NewClient::confidential(" ").redirect_uri(REDIRECT_URI),
            "name",
        ),
        (NewClient::confidential("x"), "redirect_uris"),
        (
            NewClient::confidential("x").redirect_uri("http://evil.test/cb"),
            "redirect_uris",
        ),
        (
            NewClient::confidential("x")
                .redirect_uri(REDIRECT_URI)
                .scopes(["nope"]),
            "scopes",
        ),
        (NewClient::confidential("x").client_credentials(), "owner"),
        (
            NewClient::public("x")
                .client_credentials()
                .owner(uuid::Uuid::now_v7()),
            "grants",
        ),
        (NewClient::public("x").grant("password"), "grants"),
        (
            NewClient::public("x")
                .redirect_uri(REDIRECT_URI)
                .logo_url("not a url"),
            "logo_url",
        ),
    ];
    for (new, field) in cases {
        let db = world.db().await;
        let uow = db.uow(None, None).await;
        let error = clients.create(&uow, new.clone()).await.unwrap_err();
        uow.rollback().await.unwrap();
        match error {
            OAuthError::Invalid { field: got, .. } => assert_eq!(got, field, "{new:?}"),
            other => panic!("{new:?}: {other}"),
        }
    }
}

#[tokio::test]
async fn secrets_rotate() {
    let world = World::new().await;
    let (_, token) = world.user("rep@acme.test", &["deals.read"]).await;
    let created = world.confidential_client(&["deals:read"]).await;
    let clients = world.app.get::<OAuthClients>().await;
    let id = created.client.id;
    let new_secret = world
        .work(|uow| async move { Ok(clients.rotate_secret(&uow, id).await?) })
        .await;
    assert_ne!(Some(&new_secret), created.secret.as_ref());
    // The old secret fails; the new one works.
    let old = created.secret.clone().unwrap();
    let flow = AuthorizeFlow::new(id).secret(new_secret.clone());
    flow.run(&world.client(), &token, &["deals:read"]).await;
    let refused = world
        .post_form(
            "/oauth/token",
            &[
                ("grant_type", "authorization_code"),
                ("code", "nvc_x"),
                ("client_id", &id.to_string()),
                ("client_secret", &old),
            ],
        )
        .await
        .assert_status(401);
    assert_eq!(refused.json_value()["error"], "invalid_client");

    // A public client has no secret to rotate.
    let public = world.public_client(&["deals:read"]).await;
    let clients = world.app.get::<OAuthClients>().await;
    let db = world.db().await;
    let uow = db.uow(None, None).await;
    let error = clients
        .rotate_secret(&uow, public.client.id)
        .await
        .unwrap_err();
    uow.rollback().await.unwrap();
    assert!(matches!(error, OAuthError::Invalid { .. }));
}

#[tokio::test]
async fn the_commands_manage_clients() {
    let world = World::new().await;
    let (owner, _) = world.user("sync@acme.test", &["deals.read"]).await;
    let app = &world.app;
    app.run_command(
        "oauth:client",
        &[
            "Zapier",
            "--redirect-uri",
            REDIRECT_URI,
            "--scopes",
            "deals:read,contacts:read",
        ],
    )
    .await
    .unwrap();
    app.run_command(
        "oauth:client",
        &[
            "Acme mobile",
            "--public",
            "--redirect-uri",
            REDIRECT_URI,
            "--trusted",
        ],
    )
    .await
    .unwrap();
    app.run_command(
        "oauth:client",
        &[
            "Warehouse sync",
            "--grant",
            "client_credentials",
            "--owner",
            "sync@acme.test",
            "--scopes",
            "deals:read",
        ],
    )
    .await
    .unwrap();
    app.run_command(
        "oauth:client",
        &["Acme CLI", "--public", "--device", "--scopes", "deals:read"],
    )
    .await
    .unwrap();
    app.run_command("oauth:client", &["Billing service", "--introspect"])
        .await
        .unwrap();

    let clients = world.app.get::<OAuthClients>().await;
    let list = clients.list().await.unwrap();
    let by_name = |name: &str| list.iter().find(|c| c.name == name).unwrap().clone();
    let zapier = by_name("Zapier");
    assert!(!zapier.is_public());
    assert_eq!(zapier.scope_list(), ["deals:read", "contacts:read"]);
    assert_eq!(zapier.redirect_uri_list(), [REDIRECT_URI]);
    let mobile = by_name("Acme mobile");
    assert!(mobile.is_public() && mobile.trusted);
    let sync = by_name("Warehouse sync");
    assert_eq!(sync.grant_list(), ["client_credentials"]);
    assert_eq!(sync.owner_id, Some(owner.id));
    let cli = by_name("Acme CLI");
    assert_eq!(
        cli.grant_list(),
        ["urn:ietf:params:oauth:grant-type:device_code"]
    );
    let billing = by_name("Billing service");
    assert!(billing.introspect && billing.grant_list().is_empty());

    app.run_command("oauth:clients", &[]).await.unwrap();
    app.run_command("oauth:client:secret", &[&zapier.id.to_string()])
        .await
        .unwrap();
    assert_ne!(
        clients.find(zapier.id).await.unwrap().unwrap().secret_hash,
        zapier.secret_hash
    );
    app.run_command("oauth:client:revoke", &[&zapier.id.to_string()])
        .await
        .unwrap();
    assert!(clients.find(zapier.id).await.unwrap().unwrap().is_revoked());
    assert_eq!(world.audit.with_action("oauth.client_revoked").len(), 1);

    // Errors.
    assert!(app.run_command("oauth:client", &[]).await.is_err());
    assert!(
        app.run_command(
            "oauth:client",
            &[
                "X",
                "--owner",
                "nobody@acme.test",
                "--grant",
                "client_credentials"
            ]
        )
        .await
        .is_err()
    );
    assert!(
        app.run_command("oauth:client:revoke", &["nope"])
            .await
            .is_err()
    );
    assert!(
        app.run_command("oauth:client:secret", &[&uuid::Uuid::now_v7().to_string()])
            .await
            .is_err()
    );
    app.run_command("oauth:keys", &[]).await.unwrap();
}

#[tokio::test]
async fn prune_deletes_old_expired_records() {
    let world = World::new().await;
    let (_, token) = world.user("rep@acme.test", &["deals.read"]).await;
    let client = world.public_client(&["deals:read", "offline_access"]).await;
    let (_, challenge) = navree_oauth::testing::pkce_pair();
    world
        .request(client.client.id, "deals:read", &challenge)
        .await;
    let tokens = navree_oauth::testing::authorize(
        &world.client(),
        client.client.id,
        &token,
        &["deals:read", "offline_access"],
    )
    .await;
    world.app.run_command("oauth:prune", &[]).await.unwrap();
    // Nothing is old enough yet.
    world
        .client()
        .get("/api/probe/whoami")
        .bearer(&tokens.access_token)
        .send()
        .await
        .assert_status(200);
    world
        .clock
        .advance(std::time::Duration::from_secs(40 * 86_400));
    world
        .app
        .run_command("oauth:prune", &["--days", "1"])
        .await
        .unwrap();
    let db = world.db().await;
    use navree_oauth::entity::{access_token, authorization_request, code, refresh_token};
    use navree_orm::Model as _;
    for count in [
        access_token::Model::query().count(&db).await.unwrap(),
        refresh_token::Model::query().count(&db).await.unwrap(),
        code::Model::query().count(&db).await.unwrap(),
        authorization_request::Model::query()
            .count(&db)
            .await
            .unwrap(),
    ] {
        assert_eq!(count, 0);
    }
    assert!(
        world
            .app
            .run_command("oauth:prune", &["--days", "x"])
            .await
            .is_err()
    );
    let _ = json!(null);
}
