//! OpenID Connect: ID tokens, userinfo, the JWKS and discovery.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use navree_contracts::Secret;
use navree_oauth::testing::{AuthorizeFlow, authorize};
use navree_oauth::{OAuthOptions, OAuthServerModule, ScopeOptions, SigningKey25519};
use serde_json::{Value, json};

use crate::support::{ISSUER, World, builder};

/// Verifies `token` with the JWKS the server publishes: its claims.
async fn verify(world: &World, token: &str) -> Value {
    let jwks = world
        .client()
        .get("/oauth/jwks")
        .send()
        .await
        .assert_status(200)
        .json_value();
    let parts: Vec<&str> = token.split('.').collect();
    assert_eq!(parts.len(), 3);
    let header: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).unwrap()).unwrap();
    assert_eq!(header["alg"], "EdDSA");
    assert_eq!(header["typ"], "JWT");
    let key = jwks["keys"]
        .as_array()
        .unwrap()
        .iter()
        .find(|key| key["kid"] == header["kid"])
        .expect("the key is published");
    assert_eq!(key["kty"], "OKP");
    assert_eq!(key["crv"], "Ed25519");
    let x = URL_SAFE_NO_PAD.decode(key["x"].as_str().unwrap()).unwrap();
    assert_eq!(x.len(), 32);
    let signature = URL_SAFE_NO_PAD.decode(parts[2]).unwrap();
    // Checked with `ring` directly, apart from the code under test.
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, &x)
        .verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &signature)
        .expect("the signature verifies");
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap()
}

#[tokio::test]
async fn id_tokens_carry_the_claims_of_the_scopes() {
    let world = World::new().await;
    let (user, token) = world.user("ada@acme.test", &["deals.read"]).await;
    let client = world
        .public_client(&["openid", "profile", "email", "deals:read"])
        .await;
    let tokens = AuthorizeFlow::new(client.client.id)
        .nonce("n-0S6_WzA2Mj")
        .run(&world.client(), &token, &["openid", "profile", "email"])
        .await;
    let claims = verify(&world, tokens.id_token.as_deref().unwrap()).await;
    assert_eq!(claims["iss"], ISSUER);
    assert_eq!(claims["sub"], json!(user.id));
    assert_eq!(claims["aud"], json!(client.client.id));
    assert_eq!(claims["nonce"], "n-0S6_WzA2Mj");
    assert_eq!(claims["name"], "Ada Lovelace");
    assert_eq!(claims["email"], "ada@acme.test");
    assert_eq!(claims["email_verified"], false);
    let iat = claims["iat"].as_i64().unwrap();
    assert_eq!(claims["exp"].as_i64().unwrap() - iat, 3600);

    // openid alone: no profile or email claims, and no nonce unless sent.
    let bare = authorize(&world.client(), client.client.id, &token, &["openid"]).await;
    let claims = verify(&world, bare.id_token.as_deref().unwrap()).await;
    assert!(claims.get("name").is_none() && claims.get("email").is_none());
    assert!(claims.get("nonce").is_none());
    // No openid: no ID token.
    let plain = authorize(&world.client(), client.client.id, &token, &["deals:read"]).await;
    assert!(plain.id_token.is_none());
}

#[tokio::test]
async fn userinfo_returns_the_same_claims() {
    let world = World::new().await;
    let (user, token) = world.user("ada@acme.test", &["deals.read"]).await;
    let client = world
        .public_client(&["openid", "profile", "email", "deals:read"])
        .await;
    let tokens = authorize(
        &world.client(),
        client.client.id,
        &token,
        &["openid", "profile", "email"],
    )
    .await;
    for method in ["GET", "POST"] {
        let request = if method == "GET" {
            world.client().get("/oauth/userinfo")
        } else {
            world.client().post("/oauth/userinfo")
        };
        request
            .bearer(&tokens.access_token)
            .send()
            .await
            .assert_status(200)
            .assert_json_path("sub", json!(user.id))
            .assert_json_path("name", json!("Ada Lovelace"))
            .assert_json_path("email", json!("ada@acme.test"))
            .assert_json_path("email_verified", json!(false));
    }
    // Without openid: insufficient_scope. Without a token: 401.
    let plain = authorize(&world.client(), client.client.id, &token, &["deals:read"]).await;
    let refused = world
        .client()
        .get("/oauth/userinfo")
        .bearer(&plain.access_token)
        .send()
        .await
        .assert_status(403);
    assert_eq!(
        refused.header("www-authenticate"),
        Some(r#"Bearer error="insufficient_scope", scope="openid""#)
    );
    world
        .client()
        .get("/oauth/userinfo")
        .send()
        .await
        .assert_status(401);
}

#[tokio::test]
async fn discovery_documents_describe_the_server() {
    let world = World::new().await;
    for path in [
        "/.well-known/openid-configuration",
        "/.well-known/oauth-authorization-server",
    ] {
        world
            .client()
            .get(path)
            .send()
            .await
            .assert_status(200)
            .assert_json_path("issuer", json!(ISSUER))
            .assert_json_path(
                "authorization_endpoint",
                json!(format!("{ISSUER}/oauth/authorize")),
            )
            .assert_json_path("token_endpoint", json!(format!("{ISSUER}/oauth/token")))
            .assert_json_path(
                "device_authorization_endpoint",
                json!(format!("{ISSUER}/oauth/device_authorization")),
            )
            .assert_json_path(
                "revocation_endpoint",
                json!(format!("{ISSUER}/oauth/revoke")),
            )
            .assert_json_path(
                "introspection_endpoint",
                json!(format!("{ISSUER}/oauth/introspect")),
            )
            .assert_json_path(
                "userinfo_endpoint",
                json!(format!("{ISSUER}/oauth/userinfo")),
            )
            .assert_json_path("jwks_uri", json!(format!("{ISSUER}/oauth/jwks")))
            .assert_json_path("code_challenge_methods_supported", json!(["S256"]))
            .assert_json_path("id_token_signing_alg_values_supported", json!(["EdDSA"]))
            .assert_json_path("response_types_supported", json!(["code"]));
    }
    let metadata = world
        .client()
        .get("/.well-known/openid-configuration")
        .send()
        .await
        .json_value();
    let scopes = metadata["scopes_supported"].as_array().unwrap();
    for scope in ["openid", "profile", "email", "offline_access", "deals:read"] {
        assert!(scopes.contains(&json!(scope)), "{scope}");
    }
    assert!(metadata.get("registration_endpoint").is_none());
}

#[tokio::test]
async fn the_issuer_and_path_are_configurable() {
    let world = World::with(
        builder()
            .config("oauth.issuer", "https://auth.acme.test")
            .config("oauth.path", "/connect"),
    )
    .await;
    world
        .client()
        .get("/.well-known/openid-configuration")
        .send()
        .await
        .assert_json_path("issuer", json!("https://auth.acme.test"))
        .assert_json_path(
            "token_endpoint",
            json!("https://auth.acme.test/connect/token"),
        );
    world
        .client()
        .get("/connect/jwks")
        .send()
        .await
        .assert_status(200);
    world
        .client()
        .get("/oauth/jwks")
        .send()
        .await
        .assert_status(404);
}

#[tokio::test]
async fn rotated_keys_stay_in_the_jwks() {
    let (old, new) = (SigningKey25519::generate(), SigningKey25519::generate());
    let before = World::with(builder().config("oauth.signing_key", old.encode())).await;
    let (_, token) = before.user("ada@acme.test", &[]).await;
    let client = before.public_client(&["openid"]).await;
    let tokens = authorize(&before.client(), client.client.id, &token, &["openid"]).await;
    let id_token = tokens.id_token.unwrap();
    let jwks = before.client().get("/oauth/jwks").send().await.json_value();
    assert_eq!(jwks["keys"].as_array().unwrap().len(), 1);
    assert_eq!(jwks["keys"][0]["kid"], old.kid());

    let after = World::with(
        builder()
            .config("oauth.signing_key", new.encode())
            .config("oauth.previous_signing_keys", json!([old.encode()])),
    )
    .await;
    let jwks = after.client().get("/oauth/jwks").send().await.json_value();
    assert_eq!(jwks["keys"][0]["kid"], new.kid());
    assert_eq!(jwks["keys"][1]["kid"], old.kid());
    // The token signed before the rotation still verifies.
    verify(&after, &id_token).await;
}

#[tokio::test]
async fn options_in_code_work_like_configuration() {
    let key = SigningKey25519::generate();
    let world = World::with(
        crate::servers::database(navree_testing::TestApp::builder())
            .config("app.key", navree_crypto::generate_key())
            .config("hashing.memory_kib", 1024)
            .config("hashing.iterations", 1)
            .config("http.per_ip_rate_limit", Value::Null)
            .module(navree_crypto::CryptoModule::from_config())
            .module(navree_db::DatabaseModule::from_config())
            .module(navree_auth::AuthModule::from_config())
            .module(navree_access::AccessModule::for_root(
                navree_access::AccessOptions::default(),
            ))
            .module(crate::support::ProbeModule)
            .module(OAuthServerModule::for_root(
                OAuthOptions {
                    issuer: Some("https://code.acme.test".into()),
                    consent_url: Some("https://app.acme.test/consent".into()),
                    signing_key: Some(Secret::new(key.encode())),
                    ..OAuthOptions::default()
                }
                .scope(
                    "deals:read",
                    ScopeOptions::new("Read your deals", ["deals.read"]),
                ),
            )),
    )
    .await;
    world
        .client()
        .get("/oauth/jwks")
        .send()
        .await
        .assert_json_path("keys.0.kid", json!(key.kid()));
    let (_, token) = world.user("ada@acme.test", &["deals.read"]).await;
    let client = world.public_client(&["deals:read"]).await;
    let tokens = authorize(&world.client(), client.client.id, &token, &["deals:read"]).await;
    world
        .client()
        .get("/api/probe/deals")
        .bearer(&tokens.access_token)
        .send()
        .await
        .assert_status(200);
}

/// A 2048-bit RSA test key (PKCS#8 PEM).
const RSA_PEM: &str = include_str!("../fixtures/rsa-2048.pem");

/// Verifies an RS256 `token` with the JWKS the server publishes, with
/// ring's RSA verifier directly (apart from the code under test): its
/// claims.
async fn verify_rs256(world: &World, token: &str) -> Value {
    let jwks = world
        .client()
        .get("/oauth/jwks")
        .send()
        .await
        .assert_status(200)
        .json_value();
    let parts: Vec<&str> = token.split('.').collect();
    assert_eq!(parts.len(), 3);
    let header: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).unwrap()).unwrap();
    assert_eq!(header["alg"], "RS256");
    assert_eq!(header["typ"], "JWT");
    let key = jwks["keys"]
        .as_array()
        .unwrap()
        .iter()
        .find(|key| key["kid"] == header["kid"])
        .expect("the key is published");
    assert_eq!(key["kty"], "RSA");
    assert_eq!(key["alg"], "RS256");
    assert_eq!(key["use"], "sig");
    assert!(key.get("d").is_none(), "no private part in the JWKS");
    let n = URL_SAFE_NO_PAD.decode(key["n"].as_str().unwrap()).unwrap();
    let e = URL_SAFE_NO_PAD.decode(key["e"].as_str().unwrap()).unwrap();
    ring::signature::RsaPublicKeyComponents { n: &n, e: &e }
        .verify(
            &ring::signature::RSA_PKCS1_2048_8192_SHA256,
            format!("{}.{}", parts[0], parts[1]).as_bytes(),
            &URL_SAFE_NO_PAD.decode(parts[2]).unwrap(),
        )
        .expect("the RS256 signature verifies");
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap()
}

/// An app signing ID tokens with RS256.
fn rs256() -> navree_testing::TestAppBuilder {
    builder()
        .config("oauth.signing_key", RSA_PEM)
        .config("oauth.id_token_signing_alg", "RS256")
}

#[tokio::test]
async fn id_tokens_can_be_signed_with_rs256() {
    let world = World::with(rs256()).await;
    for path in [
        "/.well-known/openid-configuration",
        "/.well-known/oauth-authorization-server",
    ] {
        world
            .client()
            .get(path)
            .send()
            .await
            .assert_status(200)
            .assert_json_path("id_token_signing_alg_values_supported", json!(["RS256"]));
    }
    let jwks = world.client().get("/oauth/jwks").send().await.json_value();
    assert_eq!(jwks["keys"].as_array().unwrap().len(), 1);
    assert_eq!(jwks["keys"][0]["e"], "AQAB");

    let (user, token) = world.user("harpreet@acme.test", &["deals.read"]).await;
    let client = world.public_client(&["openid", "profile", "email"]).await;
    let tokens = AuthorizeFlow::new(client.client.id)
        .nonce("n-RS256-1")
        .run(&world.client(), &token, &["openid", "profile", "email"])
        .await;
    let claims = verify_rs256(&world, tokens.id_token.as_deref().unwrap()).await;
    assert_eq!(claims["iss"], ISSUER);
    assert_eq!(claims["sub"], json!(user.id));
    assert_eq!(claims["aud"], json!(client.client.id));
    assert_eq!(claims["nonce"], "n-RS256-1");
    assert_eq!(claims["email"], "harpreet@acme.test");
}

#[tokio::test]
async fn an_rsa_key_in_one_environment_line_signs_too() {
    let body: String = RSA_PEM
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .collect();
    // No algorithm set: it follows the key.
    let world = World::with(builder().config("oauth.signing_key", format!("rsa:{body}"))).await;
    world
        .client()
        .get("/.well-known/openid-configuration")
        .send()
        .await
        .assert_json_path("id_token_signing_alg_values_supported", json!(["RS256"]));
    let (_, token) = world.user("simran@acme.test", &[]).await;
    let client = world.public_client(&["openid"]).await;
    let tokens = authorize(&world.client(), client.client.id, &token, &["openid"]).await;
    verify_rs256(&world, tokens.id_token.as_deref().unwrap()).await;
}

#[tokio::test]
async fn moving_to_rs256_keeps_tokens_signed_with_the_old_key() {
    let old = SigningKey25519::generate();
    let before = World::with(builder().config("oauth.signing_key", old.encode())).await;
    let (_, token) = before.user("ada@acme.test", &[]).await;
    let client = before.public_client(&["openid"]).await;
    let tokens = authorize(&before.client(), client.client.id, &token, &["openid"]).await;
    let old_token = tokens.id_token.unwrap();

    let after =
        World::with(rs256().config("oauth.previous_signing_keys", json!([old.encode()]))).await;
    let jwks = after.client().get("/oauth/jwks").send().await.json_value();
    assert_eq!(jwks["keys"][0]["kty"], "RSA");
    assert_eq!(jwks["keys"][1]["kty"], "OKP");
    assert_eq!(jwks["keys"][1]["kid"], old.kid());
    // The EdDSA token signed before the move still verifies.
    verify(&after, &old_token).await;
}

#[tokio::test]
async fn rs256_settings_that_cannot_work_fail_the_boot() {
    for (builder, key) in [
        (
            builder().config("oauth.id_token_signing_alg", "RS256"),
            "oauth.signing_key",
        ),
        (
            builder()
                .config("oauth.signing_key", SigningKey25519::generate().encode())
                .config("oauth.id_token_signing_alg", "RS256"),
            "oauth.id_token_signing_alg",
        ),
        (
            builder().config(
                "oauth.signing_key",
                include_str!("../fixtures/rsa-1024.pem"),
            ),
            "oauth.signing_key",
        ),
    ] {
        let error = builder
            .try_build()
            .await
            .map(|_| ())
            .expect_err("the boot fails")
            .to_string();
        assert!(error.contains(key), "{key}: {error}");
        assert!(!error.contains("PRIVATE KEY"), "never the key: {error}");
    }
}

/// The ID token check by an independent JWT library: `jose` (panva), the
/// library the certified `openid-client` is built on, in Node.js. The app
/// signs RS256 and EdDSA tokens; `e2e/oidc-jose/verify.mjs` verifies each
/// against the published JWKS with the issuer, audience and algorithm
/// pinned, and prints the claims it read.
///
/// Needs Node.js 20 or later and the library installed (once):
///
/// ```sh
/// npm ci --prefix e2e/oidc-jose --ignore-scripts
/// cargo test -p navree-oauth --test oauth oidc::an_independent_jwt_library_verifies_the_id_tokens -- --ignored
/// ```
#[tokio::test]
#[ignore = "needs Node.js and `npm ci --prefix e2e/oidc-jose`"]
async fn an_independent_jwt_library_verifies_the_id_tokens() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../e2e/oidc-jose");
    assert!(
        root.join("node_modules/jose").exists(),
        "run `npm ci --prefix e2e/oidc-jose --ignore-scripts` first"
    );
    let mut cases = Vec::new();
    for (alg, world) in [
        ("RS256", World::with(rs256()).await),
        ("EdDSA", World::new().await),
    ] {
        let (user, token) = world.user("harpreet@acme.test", &[]).await;
        let client = world.public_client(&["openid", "email"]).await;
        let tokens = AuthorizeFlow::new(client.client.id)
            .nonce("n-jose-1")
            .run(&world.client(), &token, &["openid", "email"])
            .await;
        let jwks = world.client().get("/oauth/jwks").send().await.json_value();
        cases.push(json!({
            "alg": alg,
            "token": tokens.id_token.unwrap(),
            "jwks": jwks,
            "issuer": ISSUER,
            "audience": client.client.id.to_string(),
            "subject": user.id.to_string(),
            "nonce": "n-jose-1",
        }));
    }
    let input = std::env::temp_dir().join(format!(
        "navree-oidc-jose-{}.json",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::write(&input, serde_json::to_vec(&cases).unwrap()).unwrap();
    let output = std::process::Command::new("node")
        .arg(root.join("verify.mjs"))
        .arg(&input)
        .output()
        .expect("node runs");
    std::fs::remove_file(&input).ok();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "jose refused:\n{stdout}\n{stderr}");
    let verified: Value = serde_json::from_str(stdout.trim()).expect("JSON from verify.mjs");
    for (case, result) in cases.iter().zip(verified.as_array().unwrap()) {
        assert_eq!(result["alg"], case["alg"], "{result}");
        assert_eq!(result["sub"], case["subject"], "{result}");
        assert_eq!(result["email"], "harpreet@acme.test", "{result}");
    }
}
