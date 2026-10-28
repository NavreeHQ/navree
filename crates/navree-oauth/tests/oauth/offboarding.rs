//! Offboarding: `AuthService::revoke_credentials_of_domain` ends every
//! credential of the users of a domain, whichever module issued it.

use navree_auth::AuthService;
use navree_oauth::testing::AuthorizeFlow;
use serde_json::json;

use crate::support::{PASSWORD, REDIRECT_URI, World, string};

/// Signs `email` in: the access token and the refresh token.
async fn sign_in(world: &World, email: &str) -> (String, String) {
    let body = world
        .client()
        .post("/api/auth/login")
        .json(&json!({ "email": email, "password": PASSWORD }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    (
        string(&body, "access_token"),
        string(&body, "refresh_token"),
    )
}

/// Every kind of credential a user holds.
struct Credentials {
    /// The sign-in session's refresh token (navree-auth).
    refresh: String,
    /// A personal access token.
    pat: String,
    /// An OAuth access token, and its refresh token.
    oauth_access: String,
    oauth_refresh: String,
    /// An authorization code that was approved and not yet exchanged.
    code: String,
    verifier: String,
}

/// What `client` was registered as in [`issue`].
struct App {
    id: String,
    secret: String,
}

async fn app(world: &World) -> App {
    let created = world
        .confidential_client(&["deals:read", "offline_access"])
        .await;
    App {
        id: created.client.id.to_string(),
        secret: created.secret.unwrap(),
    }
}

async fn issue(world: &World, app: &App, email: &str) -> Credentials {
    let (access, refresh) = sign_in(world, email).await;
    let pat = world
        .client()
        .post("/api/oauth/personal-access-tokens")
        .bearer(&access)
        .json(&json!({ "name": "report", "scopes": ["deals:read"] }))
        .send()
        .await
        .assert_status(201)
        .json_value();
    let tokens = AuthorizeFlow::new(&app.id)
        .secret(app.secret.clone())
        .run(&world.client(), &access, &["deals:read", "offline_access"])
        .await;
    let (verifier, challenge) = navree_oauth::testing::pkce_pair();
    let client_id = app.id.parse().unwrap();
    let request = world.request(client_id, "deals:read", &challenge).await;
    let code = world.approve(&request, &access).await;
    Credentials {
        refresh,
        pat: string(&pat, "token"),
        oauth_access: tokens.access_token,
        oauth_refresh: tokens.refresh_token.unwrap(),
        code,
        verifier,
    }
}

async fn whoami(world: &World, token: &str) -> u16 {
    world
        .client()
        .get("/api/probe/whoami")
        .bearer(token)
        .send()
        .await
        .status()
        .as_u16()
}

async fn refresh_session(world: &World, token: &str) -> u16 {
    world
        .client()
        .post("/api/auth/refresh")
        .json(&json!({ "refresh_token": token }))
        .send()
        .await
        .status()
        .as_u16()
}

async fn oauth_refresh(world: &World, app: &App, token: &str) -> u16 {
    world
        .post_form(
            "/oauth/token",
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", token),
                ("client_id", &app.id),
                ("client_secret", &app.secret),
            ],
        )
        .await
        .status()
        .as_u16()
}

async fn exchange_code(world: &World, app: &App, credentials: &Credentials) -> u16 {
    world
        .post_form(
            "/oauth/token",
            &[
                ("grant_type", "authorization_code"),
                ("code", &credentials.code),
                ("redirect_uri", REDIRECT_URI),
                ("code_verifier", &credentials.verifier),
                ("client_id", &app.id),
                ("client_secret", &app.secret),
            ],
        )
        .await
        .status()
        .as_u16()
}

async fn authorized_apps(world: &World, access: &str) -> usize {
    world
        .client()
        .get("/api/oauth/authorizations")
        .bearer(access)
        .send()
        .await
        .assert_status(200)
        .json_value()["data"]
        .as_array()
        .unwrap()
        .len()
}

#[tokio::test]
async fn revoking_a_domain_ends_every_kind_of_credential_of_its_users() {
    let world = World::new().await;
    world.user("ann@acme.test", &["deals.read"]).await;
    world.user("ned@Acme.Test", &["deals.read"]).await;
    world.user("bob@other.test", &["deals.read"]).await;
    let app = app(&world).await;
    let ann = issue(&world, &app, "ann@acme.test").await;
    let ned = issue(&world, &app, "ned@acme.test").await;
    let bob = issue(&world, &app, "bob@other.test").await;
    let (ann_jwt, _) = sign_in(&world, "ann@acme.test").await;
    assert_eq!(authorized_apps(&world, &ann_jwt).await, 1);

    // Everything works now.
    for who in [&ann, &ned, &bob] {
        assert_eq!(whoami(&world, &who.pat).await, 200);
        assert_eq!(whoami(&world, &who.oauth_access).await, 200);
    }

    let auth = world.app.get::<AuthService>().await;
    // The domain is matched as policies see it: any spelling.
    let report = auth
        .revoke_credentials_of_domain(" ACME.test. ")
        .await
        .unwrap();
    assert_eq!(report.users, 2);
    for kind in [
        "refresh_tokens",
        "personal_access_tokens",
        "oauth_access_tokens",
        "oauth_refresh_tokens",
        "oauth_codes",
        "oauth_authorizations",
    ] {
        assert!(report.revoked(kind) >= 2, "{kind}: {report:?}");
    }

    for who in [&ann, &ned] {
        assert_eq!(refresh_session(&world, &who.refresh).await, 401, "session");
        assert_eq!(whoami(&world, &who.pat).await, 401, "personal token");
        assert_eq!(whoami(&world, &who.oauth_access).await, 401, "oauth access");
        assert_eq!(
            oauth_refresh(&world, &app, &who.oauth_refresh).await,
            400,
            "oauth refresh"
        );
        assert_eq!(exchange_code(&world, &app, who).await, 400, "oauth code");
    }
    // Other domains are untouched.
    assert_eq!(whoami(&world, &bob.pat).await, 200);
    assert_eq!(whoami(&world, &bob.oauth_access).await, 200);
    assert_eq!(refresh_session(&world, &bob.refresh).await, 200);
    assert_eq!(exchange_code(&world, &app, &bob).await, 200);
    assert_eq!(oauth_refresh(&world, &app, &bob.oauth_refresh).await, 200);
    // The remembered consent of the revoked users is gone.
    let (bob_jwt, _) = sign_in(&world, "bob@other.test").await;
    assert_eq!(authorized_apps(&world, &bob_jwt).await, 1);

    let audited = world.audit.with_action("auth.credentials_revoked");
    assert_eq!(audited.len(), 1, "{audited:?}");
    assert_eq!(audited[0].metadata["domain"], "acme.test");
    assert_eq!(audited[0].metadata["users"], 2);
}

#[tokio::test]
async fn one_user_can_be_cut_off_and_unknown_domains_revoke_nothing() {
    let world = World::new().await;
    let (ann, _) = world.user("ann@acme.test", &["deals.read"]).await;
    let (ned, _) = world.user("ned@acme.test", &["deals.read"]).await;
    let app = app(&world).await;
    let (ann_creds, ned_creds) = (
        issue(&world, &app, "ann@acme.test").await,
        issue(&world, &app, "ned@acme.test").await,
    );
    let auth = world.app.get::<AuthService>().await;

    let none = auth
        .revoke_credentials_of_domain("nobody.test")
        .await
        .unwrap();
    assert_eq!(none.users, 0);
    assert_eq!(none.total(), 0);
    assert_eq!(whoami(&world, &ann_creds.pat).await, 200);

    let report = auth.revoke_all_credentials(ann.id).await.unwrap();
    assert_eq!(report.users, 1);
    assert_eq!(whoami(&world, &ann_creds.pat).await, 401);
    assert_eq!(refresh_session(&world, &ann_creds.refresh).await, 401);
    assert_eq!(whoami(&world, &ned_creds.pat).await, 200);
    let _ = ned;
}
