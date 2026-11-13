//! Provider tokens: stored encrypted with `social.store_tokens`, returned
//! and refreshed by `Social::token`.

use std::sync::Arc;
use std::time::Duration;

use chrono::{TimeDelta, TimeZone, Utc};
use navree_contracts::Clock;
use navree_crypto::Encrypter;
use navree_db::Db;
use navree_orm::Model as _;
use navree_social::{ProviderToken, Social, SocialError, SocialFake, SocialUser, social_account};
use navree_testing::{FixedClock, TestApp, TestAppBuilder};
use serde_json::json;
use uuid::Uuid;

use crate::support::{APPLE_P8, fake_app, sign_in};

async fn account(app: &TestApp) -> social_account::Model {
    let db = app.get::<Db>().await;
    social_account::Model::query()
        .first(&*db)
        .await
        .unwrap()
        .expect("an account")
        .into_inner()
}

fn user_id(body: &serde_json::Value) -> Uuid {
    serde_json::from_value(body["user"]["id"].clone()).unwrap()
}

#[tokio::test]
async fn tokens_are_stored_encrypted_and_refreshed_when_expired() {
    let start = Utc.with_ymd_and_hms(2026, 10, 26, 8, 0, 0).unwrap();
    let clock = Arc::new(FixedClock::new(start));
    let (app, fake) = fake_app(|b| {
        b.config("social.store_tokens", true)
            .override_value::<dyn Clock>(clock.clone())
    })
    .await;
    fake.on_code(
        "github",
        "code",
        SocialUser::new("github", "1")
            .email(Some("anu@example.com"), true)
            .token(
                ProviderToken::new("gho_access")
                    .refresh_token("ghr_refresh")
                    .expires_at(start + TimeDelta::hours(1))
                    .scopes(["read:user", "repo"]),
            ),
    );
    let body = sign_in(&app, "github", "code")
        .await
        .assert_status(200)
        .json_value();
    let id = user_id(&body);

    let row = account(&app).await;
    let stored = row.access_token.clone().expect("stored");
    assert!(!stored.contains("gho_access"), "encrypted at rest");
    assert!(!row.refresh_token.clone().unwrap().contains("ghr_refresh"));
    assert_eq!(row.token_expires_at, Some(start + TimeDelta::hours(1)));
    assert_eq!(row.scopes.as_deref(), Some("read:user repo"));
    assert!(
        !format!("{row:?}").contains(&stored),
        "redacted in debug output"
    );

    let social = app.get::<Social>().await;
    assert_eq!(
        social.token(id, "github").await.unwrap().expose(),
        "gho_access"
    );

    // Expired, and the provider refuses the refresh: authorize again.
    clock.advance(Duration::from_secs(3600));
    assert!(matches!(
        social.token(id, "github").await,
        Err(SocialError::Reauthorize { .. })
    ));
    assert!(
        social.find_user("github", "1").await.unwrap().is_some(),
        "still linked"
    );

    // The provider refreshes: the new token is stored, the refresh token kept.
    fake.on_refresh(
        "github",
        ProviderToken::new("gho_new").expires_at(start + TimeDelta::hours(3)),
    );
    assert_eq!(
        social.token(id, "github").await.unwrap().expose(),
        "gho_new"
    );
    let row = account(&app).await;
    let encrypter = app.get::<Encrypter>().await;
    assert_eq!(
        encrypter
            .decrypt_string(row.access_token.as_deref().unwrap())
            .unwrap(),
        "gho_new"
    );
    assert_eq!(
        encrypter
            .decrypt_string(row.refresh_token.as_deref().unwrap())
            .unwrap(),
        "ghr_refresh"
    );
    assert_eq!(row.token_expires_at, Some(start + TimeDelta::hours(3)));
    // Fresh again: no refresh needed.
    assert_eq!(
        social.token(id, "github").await.unwrap().expose(),
        "gho_new"
    );
}

#[tokio::test]
async fn a_token_without_an_expiry_or_refresh_token() {
    let (app, fake) = fake_app(|b| b.config("social.store_tokens", true)).await;
    fake.on_code(
        "github",
        "code",
        SocialUser::new("github", "1")
            .email(Some("anu@example.com"), true)
            .token(ProviderToken::new("gho_forever")),
    );
    let body = sign_in(&app, "github", "code")
        .await
        .assert_status(200)
        .json_value();
    let social = app.get::<Social>().await;
    assert_eq!(
        social
            .token(user_id(&body), "github")
            .await
            .unwrap()
            .expose(),
        "gho_forever"
    );
}

#[tokio::test]
async fn without_store_tokens_nothing_is_kept() {
    let (app, fake) = fake_app(|b| b).await;
    fake.on_code(
        "github",
        "code",
        SocialUser::new("github", "1")
            .email(Some("anu@example.com"), true)
            .token(ProviderToken::new("gho_access")),
    );
    let body = sign_in(&app, "github", "code")
        .await
        .assert_status(200)
        .json_value();
    let row = account(&app).await;
    assert_eq!(row.access_token, None);
    assert_eq!(row.refresh_token, None);
    let social = app.get::<Social>().await;
    let id = user_id(&body);
    assert!(matches!(
        social.token(id, "github").await,
        Err(SocialError::NoToken { .. })
    ));
    assert!(matches!(
        social.token(Uuid::now_v7(), "github").await,
        Err(SocialError::NotLinked { .. })
    ));
}

/// The site's Apple settings, with the Services ID of the web flow.
fn apple(b: TestAppBuilder) -> TestAppBuilder {
    b.config(
        "social.providers.apple",
        json!({
            "client_id": "in.navree.dhaba.web",
            "team_id": "TEAM123456",
            "key_id": "KEY1234567",
            "private_key": APPLE_P8,
        }),
    )
}

/// The site's iPhone app, a native Apple app of `auth.apps.apple`.
fn with_apple_app(b: TestAppBuilder) -> TestAppBuilder {
    b.config(
        "auth.apps.apple.main",
        json!({ "team_id": "ABCDE12345", "bundle_id": "in.navree.dhaba" }),
    )
}

async fn sign_in_with_tokens(
    app: &TestApp,
    fake: &SocialFake,
    provider: &str,
) -> social_account::Model {
    fake.on_code(
        provider,
        "code",
        SocialUser::new(provider, "1")
            .email(Some("anu@example.com"), true)
            .token(ProviderToken::new("access").refresh_token("refresh")),
    );
    sign_in(app, provider, "code").await.assert_status(200);
    account(app).await
}

#[tokio::test]
async fn apples_tokens_are_kept_without_store_tokens_when_the_site_has_an_apple_app() {
    let (app, fake) = fake_app(|b| with_apple_app(apple(b))).await;
    let row = sign_in_with_tokens(&app, &fake, "apple").await;
    let encrypter = app.get::<Encrypter>().await;
    assert_eq!(
        encrypter
            .decrypt_string(row.refresh_token.as_deref().expect("kept"))
            .unwrap(),
        "refresh"
    );
    assert!(row.access_token.is_some());
}

#[tokio::test]
async fn another_providers_tokens_are_not_kept_because_the_site_has_an_apple_app() {
    let (app, fake) = fake_app(with_apple_app).await;
    let row = sign_in_with_tokens(&app, &fake, "github").await;
    assert_eq!(row.access_token, None);
    assert_eq!(row.refresh_token, None);
}

#[tokio::test]
async fn apples_tokens_are_not_kept_without_store_tokens_when_the_site_has_no_apple_app() {
    let (app, fake) = fake_app(apple).await;
    let row = sign_in_with_tokens(&app, &fake, "apple").await;
    assert_eq!(row.access_token, None);
    assert_eq!(row.refresh_token, None);
    assert_eq!(row.token_expires_at, None);
}

#[tokio::test]
async fn apples_tokens_are_kept_with_store_tokens_when_the_site_has_no_apple_app() {
    let (app, fake) = fake_app(|b| apple(b).config("social.store_tokens", true)).await;
    let row = sign_in_with_tokens(&app, &fake, "apple").await;
    assert!(row.access_token.is_some());
    assert!(row.refresh_token.is_some());
}
