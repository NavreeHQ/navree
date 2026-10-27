//! `SettingsFake`: the real resolution in memory, recording changes, and
//! installed in a test app in place of the service.

use std::sync::Arc;

use navree_settings::{Scope, Settings, SettingsError, SettingsFake, Source};
use serde_json::json;
use uuid::Uuid;

use crate::support::{NotificationSettings, PortalSettings, World, builder};

#[tokio::test]
async fn presets_resolve_like_the_real_service_and_are_not_changes() {
    let (acme, holding, ada) = (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7());
    let fake = SettingsFake::new().attach(acme, holding);
    fake.set::<PortalSettings>(Scope::global(), json!({"page_size": 50}))
        .await
        .unwrap();
    fake.set::<PortalSettings>(
        Scope::organization(holding),
        json!({"show_leaderboard": true}),
    )
    .await
    .unwrap();
    fake.set::<PortalSettings>(Scope::user(ada), json!({"welcome_text": "Hi Ada"}))
        .await
        .unwrap();

    let settings = fake.settings();
    let portal = settings
        .resolve::<PortalSettings>(Scope::user(ada).in_workspace(acme))
        .await
        .unwrap();
    assert_eq!(portal.page_size, 50);
    assert_eq!(
        portal.source("show_leaderboard"),
        Some(Source::Organization)
    );
    assert_eq!(portal.source("welcome_text"), Some(Source::User));
    assert!(fake.changes().is_empty());
    fake.assert_not_changed::<PortalSettings>();

    // Presets are validated.
    let error = fake
        .set::<PortalSettings>(Scope::global(), json!({"page_size": 0}))
        .await
        .unwrap_err();
    assert!(matches!(error, SettingsError::Invalid(_)));
}

#[tokio::test]
async fn writes_through_the_service_are_recorded() {
    let fake = SettingsFake::new();
    let world = World::new().await;
    let db = world.db().await;
    let acme = Uuid::now_v7();
    let scope = Scope::workspace(acme).in_organization(None);
    let actor = Uuid::now_v7();
    let uow = db.uow(Some(actor), None).await;
    let settings = fake.settings();
    settings
        .update::<PortalSettings>(&uow, scope, json!({"welcome_text": "Hi"}))
        .await
        .unwrap();
    settings
        .inherit::<PortalSettings>(&uow, scope, "welcome_text")
        .await
        .unwrap();
    uow.commit().await.unwrap();

    fake.assert_changed::<PortalSettings>(scope);
    let changes = fake.changes_of::<PortalSettings>(scope);
    assert_eq!(changes.len(), 2);
    assert_eq!(changes[0].fields, ["welcome_text"]);
    assert_eq!(changes[0].new["welcome_text"], "Hi");
    assert_eq!(changes[0].actor_id, Some(actor));
    assert_eq!(changes[1].old["welcome_text"], "Hi");
    assert!(changes[1].new.is_empty());
    assert!(
        fake.changes_of::<PortalSettings>(Scope::global())
            .is_empty()
    );
    fake.assert_not_changed::<NotificationSettings>();

    fake.clear();
    assert!(fake.changes().is_empty());
}

#[tokio::test]
#[should_panic(expected = "expected a change of the `portal` settings at global")]
async fn assert_changed_names_what_is_missing() {
    SettingsFake::new().assert_changed::<PortalSettings>(Scope::global());
}

#[tokio::test]
async fn registered_types_are_known_by_key() {
    let fake = SettingsFake::new();
    assert!(matches!(
        fake.settings().resolve_key("portal", Scope::global()).await,
        Err(SettingsError::UnknownKey(_))
    ));
    let fake = fake.register::<PortalSettings>();
    let resolved = fake
        .settings()
        .resolve_key("portal", Scope::global())
        .await
        .unwrap();
    assert_eq!(resolved.data["page_size"], 25);
    assert_eq!(fake.settings().registry().len(), 1);
}

#[tokio::test]
async fn the_fake_replaces_the_service_in_a_test_app() {
    let acme_slug = "acme";
    let fake = SettingsFake::new().register::<PortalSettings>();
    fake.set::<PortalSettings>(Scope::global(), json!({"welcome_text": "From the fake"}))
        .await
        .unwrap();
    let world = World::with(builder().override_value(Arc::new(fake.settings()))).await;
    let ada = world.user("Ada").await;
    world.workspace(ada, "Acme").await;

    // The extractor and the routes read the fake.
    world
        .as_user(ada)
        .get("/api/portal")
        .header("X-Workspace", acme_slug)
        .send()
        .await
        .assert_status(200)
        .assert_json_path("welcome", json!("From the fake"))
        .assert_json_path("source", json!("global"));
    world
        .as_user(ada)
        .patch("/api/settings/portal?scope=workspace")
        .header("X-Workspace", acme_slug)
        .json(&json!({"page_size": 12}))
        .send()
        .await
        .assert_status(200);
    let workspace = world.app.get::<navree_workspace::Workspaces>().await;
    let acme = workspace.resolve(acme_slug).await.unwrap();
    fake.assert_changed::<PortalSettings>(Scope::workspace(acme.id));
    let service: Arc<Settings> = world.settings().await;
    assert_eq!(
        service
            .get::<PortalSettings>(Scope::workspace(acme.id).in_organization(None))
            .await
            .unwrap()
            .page_size,
        12
    );
}
