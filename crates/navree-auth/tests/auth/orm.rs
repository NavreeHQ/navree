//! The auth tables as navree-orm models: users, sessions (refresh token
//! families), pending password resets and sign-in tokens are queried with
//! the ORM, and what the module writes reads back as models.

use navree_auth::{User, login_token, password_reset_token, refresh_token};
use navree_db::Db;
use navree_orm::Model as _;
use serde_json::json;

use crate::support::{self, PASSWORD, login, signed_in, tokens};

type RefreshToken = refresh_token::Model;

#[tokio::test]
async fn users_are_orm_models() {
    let app = support::app().await;
    let ada = support::create_user(&app, "anu@navree.test").await;
    support::create_user(&app, "gauri@navree.test").await;
    let db = app.get::<Db>().await;

    assert_eq!(User::table_name(), "users");
    let found = User::query()
        .where_(User::EMAIL.eq("anu@navree.test"))
        .first(&db)
        .await
        .unwrap()
        .expect("anu");
    assert_eq!(found.model(), &ada);
    assert_eq!(User::query().count(&db).await.unwrap(), 2);
    assert_eq!(
        User::query()
            .where_null(User::EMAIL_VERIFIED_AT)
            .pluck(&db, User::EMAIL)
            .await
            .unwrap()
            .len(),
        2
    );

    let json = found.to_value().unwrap();
    assert_eq!(json["email"], json!("anu@navree.test"));
    for secret in [
        "password_hash",
        "two_factor_secret",
        "two_factor_recovery_codes",
        "two_factor_confirmed_at",
    ] {
        assert!(json.get(secret).is_none(), "{secret} is never serialized");
    }
}

#[tokio::test]
async fn sessions_are_refresh_token_models() {
    let app = support::app().await;
    let (user, access, first) = signed_in(&app, "anu@navree.test").await;
    let client = app.client();
    let second = tokens(
        &login(&client, "anu@navree.test", PASSWORD)
            .await
            .assert_status(200),
    )
    .1;
    let db = app.get::<Db>().await;
    let live = || {
        RefreshToken::query()
            .where_(RefreshToken::USER_ID.eq(user.id))
            .where_null(RefreshToken::REVOKED_AT)
    };
    assert_eq!(live().count(&db).await.unwrap(), 2);

    client
        .post("/api/auth/logout")
        .bearer(&access)
        .json(&json!({ "refresh_token": first }))
        .send()
        .await
        .assert_status(204);

    let left = live().get(&db).await.unwrap();
    assert_eq!(left.len(), 1);
    assert!(second.starts_with(&left[0].id.to_string()));
    assert!(
        left[0].to_value().unwrap().get("token_hash").is_none(),
        "token digests are never serialized"
    );
}

#[tokio::test]
async fn pending_resets_and_sign_in_tokens_are_orm_models() {
    let app = support::mail_builder()
        .config("auth.magic_link.enabled", true)
        .build()
        .await;
    let ada = support::create_user(&app, "anu@navree.test").await;
    let client = app.client();
    client
        .post("/api/auth/forgot-password")
        .json(&json!({ "email": "anu@navree.test" }))
        .send()
        .await
        .assert_status(202);
    client
        .post("/api/auth/magic-link")
        .json(&json!({ "email": "anu@navree.test" }))
        .send()
        .await
        .assert_status(202);
    let db = app.get::<Db>().await;

    let reset = password_reset_token::Model::find(&db, "anu@navree.test".to_owned())
        .await
        .unwrap()
        .expect("a pending reset");
    assert_eq!(reset.token_hash.len(), 64);
    let links = login_token::Model::query()
        .where_(login_token::Model::USER_ID.eq(ada.id))
        .where_(login_token::Model::KIND.eq("magic_link"))
        .get(&db)
        .await
        .unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].attempts, 0);
    assert_eq!(links[0].used_at, None);
}
