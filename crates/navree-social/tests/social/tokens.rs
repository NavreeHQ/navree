//! Provider tokens: stored encrypted with `social.store_tokens`, returned
//! and refreshed by `Social::token`.

use std::sync::Arc;
use std::time::Duration;

use chrono::{TimeDelta, TimeZone, Utc};
use navree_contracts::Clock;
use navree_crypto::Encrypter;
use navree_db::Db;
use navree_orm::Model as _;
use navree_social::{ProviderToken, Social, SocialError, SocialUser, social_account};
use navree_testing::{FixedClock, TestApp};
use uuid::Uuid;

use crate::support::{fake_app, sign_in};

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
            .email(Some("ada@example.com"), true)
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
            .email(Some("ada@example.com"), true)
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
            .email(Some("ada@example.com"), true)
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
