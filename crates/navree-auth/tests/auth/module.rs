//! `AuthModule` wiring, options and the service API used by seeders and
//! other modules.

use std::time::Duration;

use navree_auth::{
    AuthModule, AuthOptions, AuthService, JwtOptions, JwtService, TokenType, User, UserResource,
};
use navree_crypto::CryptoModule;
use navree_db::{DatabaseModule, Db};
use navree_orm::Model as _;
use navree_testing::TestApp;
use serde_json::{Value, json};

use crate::support::{self, PASSWORD, login, signed_in};

#[tokio::test]
async fn password_hash_never_serialized() {
    let app = support::app().await;
    let (user, access, _) = signed_in(&app, "anu@example.com").await;
    assert!(user.password_hash.starts_with("$argon2id$"));

    let value = serde_json::to_value(&user).unwrap();
    for hidden in [
        "password_hash",
        "two_factor_secret",
        "two_factor_recovery_codes",
        "two_factor_confirmed_at",
    ] {
        assert!(value.get(hidden).is_none(), "{hidden} in {value}");
    }
    assert_eq!(value["email"], "anu@example.com");
    let debug = format!("{user:?}");
    assert!(!debug.contains(&user.password_hash), "{debug}");

    let resource = serde_json::to_value(UserResource::from(&user)).unwrap();
    assert_eq!(
        resource,
        json!({
            "__type": "User",
            "id": user.id,
            "name": "Anu Lakhotia",
            "email": "anu@example.com",
            "email_verified_at": Value::Null,
            "created_at": user.created_at,
        })
    );

    for response in [
        login(&app.client(), "anu@example.com", PASSWORD).await,
        app.client()
            .get("/api/auth/me")
            .bearer(&access)
            .send()
            .await,
    ] {
        let text = response.text();
        assert!(!text.contains("password_hash"), "{text}");
        assert!(!text.contains("$argon2id$"), "{text}");
    }
}

#[tokio::test]
async fn routes_mount_under_the_configured_prefix() {
    let app = support::builder()
        .config("auth.routes_prefix", "/identity")
        .build()
        .await;
    support::create_user(&app, "anu@example.com").await;
    app.client()
        .post("/api/identity/login")
        .json(&json!({ "email": "anu@example.com", "password": PASSWORD }))
        .send()
        .await
        .assert_status(200);
    login(&app.client(), "anu@example.com", PASSWORD)
        .await
        .assert_error(404, None);
}

#[tokio::test]
async fn for_root_takes_options_and_a_configured_secret() {
    let secret = "a-very-long-jwt-secret-of-at-least-32-bytes";
    let options = AuthOptions {
        jwt: JwtOptions {
            secret: Some(secret.to_owned().into()),
            issuer: "crm".to_owned(),
            access_ttl_secs: 60,
            ..JwtOptions::default()
        },
        ..AuthOptions::default()
    };
    let app = crate::servers::database(TestApp::builder())
        .config("hashing.memory_kib", 1024)
        .config("hashing.iterations", 1)
        .module(CryptoModule::from_config())
        .module(DatabaseModule::from_config())
        .module(AuthModule::for_root(options.clone()))
        .build()
        .await;
    let (_, access, _) = signed_in(&app, "anu@example.com").await;
    // Anyone holding the configured secret can verify the tokens.
    let verifier = JwtService::new(secret.as_bytes(), &options.jwt);
    let claims = verifier.verify(&access, TokenType::Access).unwrap();
    assert_eq!(claims.iss, "crm");
    assert_eq!(claims.exp - claims.iat, 60);
    login(&app.client(), "anu@example.com", PASSWORD)
        .await
        .assert_json_path("expires_in", json!(60));
}

#[tokio::test]
async fn tokens_are_signed_with_a_key_derived_from_the_app_key() {
    let app = support::app().await;
    let (_, access, _) = signed_in(&app, "anu@example.com").await;
    let header = access.split('.').next().unwrap();
    assert_eq!(
        header, "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9",
        "HS256 JWT header"
    );
    // Not the raw app key: a token signed with it is rejected.
    let jwt = app.get::<JwtService>().await;
    let forged = JwtService::new(b"0123456789abcdef0123456789abcdef", &JwtOptions::default())
        .sign_access(uuid::Uuid::now_v7())
        .unwrap();
    assert!(jwt.verify(&forged, TokenType::Access).is_err());
}

#[tokio::test]
async fn boots_without_an_app_key() {
    let app = crate::servers::database(TestApp::builder())
        .config("hashing.memory_kib", 1024)
        .config("hashing.iterations", 1)
        .module(CryptoModule::from_config())
        .module(DatabaseModule::from_config())
        .module(AuthModule::from_config())
        .build()
        .await;
    let (_, access, _) = signed_in(&app, "anu@example.com").await;
    app.client()
        .get("/api/auth/me")
        .bearer(&access)
        .send()
        .await
        .assert_status(200);
}

#[tokio::test]
async fn module_graph_needs_crypto_and_database() {
    let error = TestApp::builder()
        .module(AuthModule::from_config())
        .try_build()
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("Db") || error.contains("Hasher") || error.contains("Encrypter"),
        "{error}"
    );
}

#[tokio::test]
async fn service_creates_finds_and_updates_users() {
    let app = support::app().await;
    let auth = app.get::<AuthService>().await;
    let created: User = auth
        .create_user("Anu Lakhotia", " ANU@Example.com ", PASSWORD)
        .await
        .unwrap();
    assert_eq!(created.email, "anu@example.com");
    assert_eq!(
        auth.find_user(created.id).await.unwrap(),
        Some(created.clone())
    );
    assert_eq!(
        auth.find_by_email("Anu@example.com")
            .await
            .unwrap()
            .map(|u| u.id),
        Some(created.id)
    );
    assert_eq!(auth.find_user(uuid::Uuid::now_v7()).await.unwrap(), None);
    assert!(
        auth.validate_user("anu@example.com", PASSWORD)
            .await
            .is_ok()
    );

    auth.update_password(created.id, "a brand new password")
        .await
        .unwrap();
    assert!(
        auth.validate_user("anu@example.com", PASSWORD)
            .await
            .is_err()
    );
    let user = auth
        .validate_user("anu@example.com", "a brand new password")
        .await
        .unwrap();
    assert_eq!(user.id, created.id);
    assert!(user.updated_at >= created.updated_at);

    let db = app.get::<Db>().await;
    assert_eq!(User::all(&*db).await.unwrap().len(), 1);
}

#[tokio::test]
async fn access_token_lifetime_matches_expires_in() {
    let app = support::app().await;
    let (_, access, _) = signed_in(&app, "anu@example.com").await;
    let claims = app
        .get::<JwtService>()
        .await
        .verify(&access, TokenType::Access)
        .unwrap();
    assert_eq!(
        Duration::from_secs((claims.exp - claims.iat) as u64),
        Duration::from_secs(900)
    );
}

/// Adds the roles a CRM keeps elsewhere to every user resource.
struct RolesExtension;

impl navree_kernel::Injectable for RolesExtension {
    fn construct(
        _r: navree_kernel::Resolver,
    ) -> navree_contracts::BoxFuture<'static, Result<Self, navree_kernel::KernelError>> {
        Box::pin(async { Ok(RolesExtension) })
    }
}

impl navree_auth::UserResourceExtension for RolesExtension {
    fn extend<'a>(
        &'a self,
        user: &'a User,
        fields: &'a mut serde_json::Map<String, Value>,
    ) -> navree_contracts::BoxFuture<'a, Result<(), navree_contracts::BoxError>> {
        Box::pin(async move {
            let roles = if user.email.starts_with("admin") {
                json!(["admin"])
            } else {
                json!([])
            };
            fields.insert("roles".to_owned(), roles);
            Ok(())
        })
    }
}

struct RolesModule;

impl navree_kernel::Module for RolesModule {
    fn register(&self, m: &mut navree_kernel::ModuleBuilder) {
        use navree_auth::ModuleBuilderAuthExt;
        m.user_resource_extension::<RolesExtension>();
    }
}

#[tokio::test]
async fn user_resource_extensions_add_fields() {
    let app = support::builder().module(RolesModule).build().await;
    let (_, access, _) = signed_in(&app, "admin@example.com").await;
    login(&app.client(), "admin@example.com", PASSWORD)
        .await
        .assert_status(200)
        .assert_json_path("user.roles", json!(["admin"]))
        .assert_json_path("user.__type", json!("User"));
    app.client()
        .get("/api/auth/me")
        .bearer(&access)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("roles", json!(["admin"]))
        .assert_json_path("email", json!("admin@example.com"));
    app.client()
        .post("/api/auth/register")
        .json(&json!({ "name": "Rep", "email": "rep@example.com", "password": PASSWORD }))
        .send()
        .await
        .assert_status(201)
        .assert_json_path("user.roles", json!([]));
    let auth = app.get::<AuthService>().await;
    let user = auth
        .find_by_email("rep@example.com")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(auth.present(&user).await.unwrap()["roles"], json!([]));
}

#[tokio::test]
async fn register_writes_in_the_given_unit_of_work() {
    let app = support::app().await;
    let auth = app.get::<AuthService>().await;
    let db = app.get::<Db>().await;
    let new = |email: &str| navree_auth::NewUser {
        name: "Anu".to_owned(),
        email: email.to_owned(),
        password: PASSWORD.to_owned(),
    };

    let uow = db.uow(None, None).await;
    auth.register(&uow, new("anu@example.com")).await.unwrap();
    uow.rollback().await.unwrap();
    assert_eq!(auth.find_by_email("anu@example.com").await.unwrap(), None);

    let uow = db.uow(None, None).await;
    let user = auth.register(&uow, new("anu@example.com")).await.unwrap();
    // A second registration in the same unit of work sees the first.
    assert!(matches!(
        auth.register(&uow, new("ANU@example.com")).await,
        Err(navree_auth::AuthError::EmailTaken)
    ));
    uow.commit().await.unwrap();
    assert_eq!(
        auth.find_by_email("anu@example.com")
            .await
            .unwrap()
            .map(|u| u.id),
        Some(user.id)
    );
    assert!(matches!(
        auth.update_password(uuid::Uuid::now_v7(), "a long enough password")
            .await,
        Err(navree_auth::AuthError::UserNotFound)
    ));
    assert!(matches!(
        auth.update_password(user.id, "short").await,
        Err(navree_auth::AuthError::Invalid {
            field: "password",
            ..
        })
    ));
}
