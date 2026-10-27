//! Resolution: the inheritance matrix, field-by-field sources, the levels a
//! type allows, and the organization of a workspace.

use std::collections::BTreeMap;

use navree_settings::{Scope, ScopeKind, SettingsError, Source};
use navree_workspace::Organizations;
use serde_json::json;

use crate::support::{
    ForgottenSettings, NotificationSettings, PortalSettings, ViewPreferences, World,
};

/// Stores `patch` for `PortalSettings` at `scope`.
async fn set(world: &World, scope: Scope, patch: serde_json::Value) {
    let settings = world.settings().await;
    world
        .work(|uow| async move { settings.update::<PortalSettings>(&uow, scope, patch).await })
        .await
        .unwrap();
}

#[tokio::test]
async fn nothing_stored_resolves_to_the_defaults() {
    let world = World::new().await;
    let settings = world.settings().await;
    let ada = world.user("Ada").await;
    let acme = world.workspace(ada, "Acme").await;

    let portal = settings
        .resolve::<PortalSettings>(Scope::user(ada).in_workspace(acme.id))
        .await
        .unwrap();
    assert_eq!(portal.value, PortalSettings::default());
    assert_eq!(
        portal.sources,
        BTreeMap::from([
            ("page_size".to_owned(), Source::Default),
            ("show_leaderboard".to_owned(), Source::Default),
            ("welcome_text".to_owned(), Source::Default),
        ])
    );
    assert!(portal.overridden.is_empty());
    assert_eq!(
        settings
            .get::<PortalSettings>(Scope::global())
            .await
            .unwrap(),
        PortalSettings::default()
    );
}

/// Every combination of the four levels storing `welcome_text`: the most
/// specific level that stores it wins, and reports itself as the source.
#[tokio::test]
async fn the_inheritance_matrix() {
    let world = World::new().await;
    let settings = world.settings().await;
    let ada = world.user("Ada").await;
    let holding = world.organization(ada, "Holding").await;
    let acme = world.workspace(ada, "Acme").await;
    world.attach(holding.id, acme.id).await;

    let levels = [
        (Scope::global(), Source::Global),
        (Scope::organization(holding.id), Source::Organization),
        (Scope::workspace(acme.id), Source::Workspace),
        (Scope::user(ada).in_workspace(acme.id), Source::User),
    ];
    // Resolved for the user in the workspace (the full chain), and for each
    // level on its own.
    let user_scope = Scope::user(ada).in_workspace(acme.id);
    for mask in 0u8..16 {
        for (bit, (scope, source)) in levels.iter().enumerate() {
            if mask & (1 << bit) != 0 {
                let text = format!("from {source}");
                world
                    .work(|uow| {
                        let settings = settings.clone();
                        let scope = *scope;
                        async move {
                            settings
                                .update::<PortalSettings>(
                                    &uow,
                                    scope,
                                    json!({"welcome_text": text}),
                                )
                                .await
                        }
                    })
                    .await
                    .unwrap();
            }
        }

        let expected = (0..4)
            .rev()
            .find(|bit| mask & (1 << bit) != 0)
            .map(|bit| levels[bit].1)
            .unwrap_or(Source::Default);
        let resolved = settings
            .resolve::<PortalSettings>(user_scope)
            .await
            .unwrap();
        assert_eq!(
            resolved.source("welcome_text"),
            Some(expected),
            "mask {mask:04b}"
        );
        let text = match expected {
            Source::Default => "Welcome".to_owned(),
            source => format!("from {source}"),
        };
        assert_eq!(resolved.welcome_text, text, "mask {mask:04b}");
        // Only the user's own level counts as overridden for the user.
        assert_eq!(
            resolved.is_overridden("welcome_text"),
            mask & 0b1000 != 0,
            "mask {mask:04b}"
        );
        // The other fields were never set.
        assert_eq!(resolved.source("page_size"), Some(Source::Default));

        // The workspace on its own skips the user's level.
        let workspace = settings
            .resolve::<PortalSettings>(Scope::workspace(acme.id))
            .await
            .unwrap();
        let expected_for_workspace = (0..3)
            .rev()
            .find(|bit| mask & (1 << bit) != 0)
            .map(|bit| levels[bit].1)
            .unwrap_or(Source::Default);
        assert_eq!(
            workspace.source("welcome_text"),
            Some(expected_for_workspace),
            "mask {mask:04b}"
        );

        // Back to nothing stored.
        for (scope, _) in &levels {
            world
                .work(|uow| {
                    let settings = settings.clone();
                    let scope = *scope;
                    async move { settings.reset::<PortalSettings>(&uow, scope).await }
                })
                .await
                .unwrap();
        }
    }
}

#[tokio::test]
async fn every_field_reports_its_own_source() {
    let world = World::new().await;
    let settings = world.settings().await;
    let ada = world.user("Ada").await;
    let holding = world.organization(ada, "Holding").await;
    let acme = world.workspace(ada, "Acme").await;
    // The workspace as attached: it carries its organization.
    let acme = world.attach(holding.id, acme.id).await;

    set(
        &world,
        Scope::global(),
        json!({"page_size": 50, "welcome_text": "Hello"}),
    )
    .await;
    set(
        &world,
        Scope::organization(holding.id),
        json!({"show_leaderboard": true}),
    )
    .await;
    set(
        &world,
        Scope::workspace(acme.id),
        json!({"welcome_text": "Welcome to Acme"}),
    )
    .await;
    set(
        &world,
        Scope::user(ada).in_workspace(acme.id),
        json!({"page_size": 10}),
    )
    .await;

    let portal = settings
        .resolve::<PortalSettings>(Scope::user(ada).in_workspace(acme.id))
        .await
        .unwrap();
    assert_eq!(
        portal.value,
        PortalSettings {
            show_leaderboard: true,
            welcome_text: "Welcome to Acme".to_owned(),
            page_size: 10,
        }
    );
    assert_eq!(
        portal.sources,
        BTreeMap::from([
            ("page_size".to_owned(), Source::User),
            ("show_leaderboard".to_owned(), Source::Organization),
            ("welcome_text".to_owned(), Source::Workspace),
        ])
    );
    assert_eq!(portal.overridden, ["page_size"]);
    // Resolved derefs to the value and serializes as {data, sources,
    // overridden}.
    assert!(portal.show_leaderboard);
    assert_eq!(
        serde_json::to_value(&portal).unwrap(),
        json!({
            "data": {"show_leaderboard": true, "welcome_text": "Welcome to Acme", "page_size": 10},
            "sources": {"page_size": "user", "show_leaderboard": "organization", "welcome_text": "workspace"},
            "overridden": ["page_size"],
        })
    );

    // The workspace's own view: its override, its organization's, the
    // global value.
    let workspace = settings.resolve::<PortalSettings>(&acme).await.unwrap();
    assert_eq!(workspace.overridden, ["welcome_text"]);
    assert_eq!(workspace.source("page_size"), Some(Source::Global));
    assert_eq!(
        workspace.source("show_leaderboard"),
        Some(Source::Organization)
    );
    assert_eq!(workspace.page_size, 50);

    // By key, as JSON (what the routes and commands use).
    let by_key = settings
        .resolve_key("portal", Scope::workspace(acme.id))
        .await
        .unwrap();
    assert_eq!(
        by_key.source("show_leaderboard"),
        Some(Source::Organization)
    );
    assert_eq!(by_key.data["welcome_text"], "Welcome to Acme");
    assert!(matches!(
        settings.resolve_key("nope", Scope::global()).await,
        Err(SettingsError::UnknownKey(key)) if key == "nope"
    ));
}

#[tokio::test]
async fn levels_a_type_does_not_allow_are_skipped_and_refused() {
    let world = World::new().await;
    let settings = world.settings().await;
    let ada = world.user("Ada").await;
    let holding = world.organization(ada, "Holding").await;
    let acme = world.workspace(ada, "Acme").await;
    world.attach(holding.id, acme.id).await;

    // Notifications: global and workspace only.
    let error = world
        .work(|uow| {
            let settings = settings.clone();
            async move {
                settings
                    .update::<NotificationSettings>(
                        &uow,
                        Scope::organization(holding.id),
                        json!({"daily_digest": true}),
                    )
                    .await
            }
        })
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "the `notifications` settings cannot be stored at the organization level; they allow \
         global, workspace"
    );
    world
        .work(|uow| {
            let settings = settings.clone();
            async move {
                settings
                    .update::<NotificationSettings>(
                        &uow,
                        Scope::global(),
                        json!({"sender_name": "Navree"}),
                    )
                    .await?;
                settings
                    .update::<NotificationSettings>(
                        &uow,
                        Scope::workspace(acme.id),
                        json!({"daily_digest": true}),
                    )
                    .await
            }
        })
        .await
        .unwrap();
    // A user in the workspace reads the workspace's values: the user level
    // is skipped.
    let notifications = settings
        .resolve::<NotificationSettings>(Scope::user(ada).in_workspace(acme.id))
        .await
        .unwrap();
    assert!(notifications.daily_digest);
    assert_eq!(
        notifications.source("daily_digest"),
        Some(Source::Workspace)
    );
    assert_eq!(notifications.source("sender_name"), Some(Source::Global));
    // The user level is not the target's level: nothing is overridden there.
    assert!(notifications.overridden.is_empty());

    // View preferences: the user's level only.
    world
        .work(|uow| {
            let settings = settings.clone();
            async move {
                settings
                    .update::<ViewPreferences>(&uow, Scope::user(ada), json!({"theme": "dark"}))
                    .await
            }
        })
        .await
        .unwrap();
    let mine = settings
        .resolve::<ViewPreferences>(Scope::user(ada).in_workspace(acme.id))
        .await
        .unwrap();
    assert_eq!(mine.theme, "dark");
    assert_eq!(mine.source("theme"), Some(Source::User));
    let workspace = settings
        .resolve::<ViewPreferences>(Scope::workspace(acme.id))
        .await
        .unwrap();
    assert_eq!(workspace.theme, "");
    assert_eq!(workspace.source("theme"), Some(Source::Default));
    let error = world
        .work(|uow| {
            let settings = settings.clone();
            async move {
                settings
                    .update::<ViewPreferences>(&uow, Scope::global(), json!({"theme": "light"}))
                    .await
            }
        })
        .await
        .unwrap_err();
    assert!(
        matches!(
            error,
            SettingsError::ScopeNotAllowed {
                scope: ScopeKind::Global,
                ..
            }
        ),
        "{error}"
    );
}

#[tokio::test]
async fn a_workspace_scope_finds_its_organization() {
    let world = World::new().await;
    let settings = world.settings().await;
    let ada = world.user("Ada").await;
    let holding = world.organization(ada, "Holding").await;
    let acme = world.workspace(ada, "Acme").await;
    // An `&Organization` is a scope.
    let holding_scope = Scope::from(&holding);
    set(
        &world,
        holding_scope,
        json!({"welcome_text": "Hello from the holding"}),
    )
    .await;

    // Not attached yet: the organization's values do not apply.
    let before = settings
        .resolve::<PortalSettings>(Scope::workspace(acme.id))
        .await
        .unwrap();
    assert_eq!(before.source("welcome_text"), Some(Source::Default));
    assert_eq!(before.scope.organization_id(), None);

    // Attached: the lookup (cached) is forgotten and the organization's
    // values apply.
    world.attach(holding.id, acme.id).await;
    let attached = settings
        .resolve::<PortalSettings>(Scope::workspace(acme.id))
        .await
        .unwrap();
    assert_eq!(attached.welcome_text, "Hello from the holding");
    assert_eq!(attached.source("welcome_text"), Some(Source::Organization));
    assert_eq!(attached.scope.organization_id(), Some(holding.id));
    assert_eq!(
        settings.complete(Scope::workspace(acme.id)).await.unwrap(),
        Scope::workspace(acme.id).in_organization(holding.id)
    );

    // Detached: back to the global values.
    let organizations = world.app.get::<Organizations>().await;
    world
        .work(|uow| async move { organizations.detach(&uow, holding.id, acme.id).await })
        .await
        .unwrap();
    let detached = settings
        .resolve::<PortalSettings>(Scope::workspace(acme.id))
        .await
        .unwrap();
    assert_eq!(detached.source("welcome_text"), Some(Source::Default));

    // A named organization is used as given, without a lookup.
    let named = settings
        .resolve::<PortalSettings>(Scope::workspace(acme.id).in_organization(holding.id))
        .await
        .unwrap();
    assert_eq!(named.source("welcome_text"), Some(Source::Organization));
}

#[tokio::test]
async fn unregistered_types_name_the_registration() {
    let world = World::new().await;
    let settings = world.settings().await;
    let error = settings
        .resolve::<ForgottenSettings>(Scope::global())
        .await
        .unwrap_err();
    assert!(matches!(error, SettingsError::Unregistered { .. }));
    assert!(
        error
            .to_string()
            .contains("import SettingsModule::for_feature::<ForgottenSettings>()"),
        "{error}"
    );
}
