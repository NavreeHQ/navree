//! Saved locales: `PUT /api/me/locale`, the `user` and `workspace`
//! sources, and the `I18n` methods behind them.

use navree_i18n::{I18n, I18nError};
use serde_json::json;

use crate::support::World;

#[tokio::test]
async fn users_save_their_locale() {
    let world = World::new().await;
    let ada = world.user("Ada").await;

    // Nothing saved: the header decides.
    world
        .as_user(ada)
        .get("/api/deals/mine")
        .header("Accept-Language", "de")
        .send()
        .await
        .assert_status(200);
    assert_eq!(
        world
            .as_user(ada)
            .get("/api/deals/mine")
            .header("Accept-Language", "de")
            .send()
            .await
            .text(),
        "de"
    );

    world
        .as_user(ada)
        .put("/api/me/locale")
        .json(&json!({ "locale": "fr-FR" }))
        .send()
        .await
        .assert_status(204);
    // The user's saved locale comes before the header (stored as `fr`).
    let response = world
        .as_user(ada)
        .get("/api/deals/mine")
        .header("Accept-Language", "de")
        .send()
        .await;
    assert_eq!(response.text(), "fr");
    assert_eq!(response.header("content-language"), Some("fr"));
    let i18n = world.app.get::<I18n>().await;
    assert_eq!(i18n.user_locale(ada).await.unwrap().tag(), "fr");

    // Guests are not users.
    world
        .app
        .client()
        .put("/api/me/locale")
        .json(&json!({ "locale": "fr" }))
        .send()
        .await
        .assert_status(401);
    world.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn an_unsupported_locale_is_a_400_in_the_request_language() {
    let world = World::new().await;
    let ada = world.user("Ada").await;
    world
        .as_user(ada)
        .put("/api/me/locale")
        .json(&json!({ "locale": "ja" }))
        .send()
        .await
        .assert_validation_error("locale")
        .assert_json_path(
            "errors.locale.0",
            json!("locale must be one of the following values: en, fr, de, pt-BR"),
        );
    world
        .as_user(ada)
        .put("/api/me/locale")
        .header("Accept-Language", "fr")
        .json(&json!({ "locale": "not a tag" }))
        .send()
        .await
        .assert_validation_error("locale")
        .assert_json_path(
            "errors.locale.0",
            json!("locale doit être l'une des valeurs suivantes : en, fr, de, pt-BR"),
        );
    world.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn the_workspace_locale_applies_to_its_members() {
    let world = World::new().await;
    let owner = world.user("Owner").await;
    let outsider = world.user("Outsider").await;
    let acme = world.workspace(owner, "Acme").await;
    let i18n = world.app.get::<I18n>().await;
    let saved = world
        .work(|uow| {
            let i18n = i18n.clone();
            async move { i18n.set_workspace_locale(&uow, acme.id, "de").await }
        })
        .await
        .unwrap();
    assert_eq!(saved.tag(), "de");
    assert_eq!(i18n.workspace_locale(acme.id).await.unwrap().tag(), "de");

    let member = world
        .as_user(owner)
        .get("/api/deals/mine")
        .header("X-Workspace", acme.slug.as_str())
        .send()
        .await
        .text();
    assert_eq!(member, "de");
    // Not a member: the workspace does not count.
    let other = world
        .as_user(outsider)
        .get("/api/deals/mine")
        .header("X-Workspace", acme.slug.as_str())
        .header("Accept-Language", "fr")
        .send()
        .await
        .text();
    assert_eq!(other, "fr");
    // The user's own locale comes first.
    world
        .work(|uow| {
            let i18n = i18n.clone();
            async move { i18n.set_user_locale(&uow, owner, "pt-BR").await }
        })
        .await
        .unwrap();
    let own = world
        .as_user(owner)
        .get("/api/deals/mine")
        .header("X-Workspace", acme.slug.as_str())
        .send()
        .await
        .text();
    assert_eq!(own, "pt-BR");
    world.app.shutdown().await.unwrap();
}

#[tokio::test]
async fn saving_checks_the_locale() {
    let world = World::new().await;
    let ada = world.user("Ada").await;
    let i18n = world.app.get::<I18n>().await;
    let error = world
        .work(|uow| {
            let i18n = i18n.clone();
            async move { i18n.set_user_locale(&uow, ada, "ja").await }
        })
        .await
        .unwrap_err();
    assert!(
        matches!(&error, I18nError::UnsupportedLocale { locale, .. } if locale == "ja"),
        "{error}"
    );
    let error = world
        .work(|uow| {
            let i18n = i18n.clone();
            async move { i18n.set_user_locale(&uow, ada, "??").await }
        })
        .await
        .unwrap_err();
    assert!(matches!(error, I18nError::InvalidLocale(_)), "{error}");
    // Saving twice updates the row.
    for tag in ["fr", "de"] {
        world
            .work(|uow| {
                let i18n = i18n.clone();
                async move { i18n.set_user_locale(&uow, ada, tag).await }
            })
            .await
            .unwrap();
    }
    assert_eq!(i18n.user_locale(ada).await.unwrap().tag(), "de");
    world.app.shutdown().await.unwrap();
}
