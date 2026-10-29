//! Writes: validated patches naming the field, inherit and reset, units of
//! work, the audit trail, `settings.changed`, and cache tags per key and
//! level.

use navree_audit::{AuditFilter, AuditLog};
use navree_db::{Db, PageQuery};
use navree_settings::{Scope, Settings, SettingsError, Source};
use sea_orm::sea_query::{Expr, ExprTrait, Query};
use sea_orm::ConnectionTrait;
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::support::{PortalSettings, World};

/// Writes `patch` at `scope` in a unit of work of its own.
async fn update(
    world: &World,
    scope: Scope,
    patch: Value,
) -> Result<navree_settings::Resolved<PortalSettings>, SettingsError> {
    let settings = world.settings().await;
    world
        .work(|uow| async move { settings.update::<PortalSettings>(&uow, scope, patch).await })
        .await
}

/// Makes `field` inherit at `scope`, in a unit of work of its own.
async fn inherit(
    world: &World,
    scope: Scope,
    field: &'static str,
) -> Result<navree_settings::Resolved<PortalSettings>, SettingsError> {
    let settings = world.settings().await;
    world
        .work(|uow| async move { settings.inherit::<PortalSettings>(&uow, scope, field).await })
        .await
}

#[tokio::test]
async fn patches_are_validated_and_errors_name_the_field() {
    let world = World::new().await;
    let settings = world.settings().await;

    let error = update(
        &world,
        Scope::global(),
        json!({"welcome_text": "x".repeat(41)}),
    )
    .await
    .unwrap_err();
    let SettingsError::Invalid(errors) = &error else {
        panic!("expected Invalid, got {error:?}");
    };
    assert_eq!(
        errors.get("welcome_text").unwrap(),
        ["welcome_text must be shorter than or equal to 40 characters"]
    );
    let response = error.to_app_error().unwrap();
    assert_eq!(response.status.as_u16(), 400);
    assert_eq!(
        response.body()["errors"]["welcome_text"][0],
        "welcome_text must be shorter than or equal to 40 characters"
    );

    let error = update(&world, Scope::global(), json!({"page_size": 0}))
        .await
        .unwrap_err();
    let SettingsError::Invalid(errors) = &error else {
        panic!("expected Invalid, got {error:?}");
    };
    assert!(errors.get("page_size").is_some(), "{errors:?}");
    assert!(errors.get("welcome_text").is_none(), "{errors:?}");

    // A value of the wrong type is refused on its field too.
    let error = update(&world, Scope::global(), json!({"page_size": "ten"}))
        .await
        .unwrap_err();
    let SettingsError::Invalid(errors) = &error else {
        panic!("expected Invalid, got {error:?}");
    };
    assert!(errors.get("page_size").is_some(), "{errors:?}");

    let error = update(
        &world,
        Scope::global(),
        json!({"colour": "red", "page_size": 5}),
    )
    .await
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "the `portal` settings have no field `colour`"
    );

    let error = update(&world, Scope::global(), json!(["page_size"]))
        .await
        .unwrap_err();
    assert!(matches!(error, SettingsError::InvalidPatch(_)), "{error:?}");

    // Nothing was stored.
    assert!(
        settings
            .stored::<PortalSettings>(Scope::global())
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn the_inherited_values_are_validated_with_the_patch() {
    /// A rule across fields: the leaderboard needs a page size of at least
    /// ten.
    #[derive(
        navree_settings::Settings,
        Clone,
        Debug,
        Default,
        PartialEq,
        serde::Serialize,
        serde::Deserialize,
        garde::Validate,
    )]
    #[settings(key = "board")]
    struct BoardSettings {
        #[garde(skip)]
        leaderboard: bool,
        #[garde(custom(at_least_ten_with_leaderboard(&self.leaderboard)))]
        size: u32,
    }

    fn at_least_ten_with_leaderboard(
        leaderboard: &bool,
    ) -> impl FnOnce(&u32, &()) -> garde::Result + '_ {
        move |size, _| {
            if *leaderboard && *size < 10 {
                Err(garde::Error::new("the leaderboard needs at least 10 rows"))
            } else {
                Ok(())
            }
        }
    }

    let world = World::with(
        crate::support::builder()
            .module(navree_settings::SettingsModule::for_feature::<BoardSettings>()),
    )
    .await;
    let settings = world.settings().await;
    let acme = Uuid::now_v7();
    let scope = Scope::workspace(acme).in_organization(None);
    world
        .work(|uow| {
            let settings = settings.clone();
            async move {
                settings
                    .update::<BoardSettings>(&uow, Scope::global(), json!({"size": 5}))
                    .await
            }
        })
        .await
        .unwrap();
    // The workspace turns the leaderboard on; the global size it inherits
    // breaks the rule, which names the field.
    let error = world
        .work(|uow| {
            let settings = settings.clone();
            async move {
                settings
                    .update::<BoardSettings>(&uow, scope, json!({"leaderboard": true}))
                    .await
            }
        })
        .await
        .unwrap_err();
    let SettingsError::Invalid(errors) = error else {
        panic!("expected Invalid");
    };
    assert_eq!(
        errors.get("size").unwrap(),
        ["the leaderboard needs at least 10 rows"]
    );
    let board = world
        .work(|uow| {
            let settings = settings.clone();
            async move {
                settings
                    .update::<BoardSettings>(&uow, scope, json!({"leaderboard": true, "size": 12}))
                    .await
            }
        })
        .await
        .unwrap();
    assert!(board.leaderboard);
    assert_eq!(board.overridden, ["leaderboard", "size"]);
}

#[tokio::test]
async fn patches_take_any_serializable_value() {
    #[derive(Serialize)]
    struct Welcome<'a> {
        welcome_text: &'a str,
    }

    let world = World::new().await;
    let settings = world.settings().await;
    let portal = world
        .work(|uow| {
            let settings = settings.clone();
            async move {
                settings
                    .update::<PortalSettings>(
                        &uow,
                        Scope::global(),
                        Welcome {
                            welcome_text: "Hello",
                        },
                    )
                    .await
            }
        })
        .await
        .unwrap();
    assert_eq!(portal.welcome_text, "Hello");
    assert_eq!(portal.overridden, ["welcome_text"]);
    assert_eq!(
        Value::Object(
            settings
                .stored::<PortalSettings>(Scope::global())
                .await
                .unwrap()
        ),
        json!({"welcome_text": "Hello"})
    );
}

#[tokio::test]
async fn inherit_and_reset_make_fields_follow_the_level_above() {
    let world = World::new().await;
    let settings = world.settings().await;
    let acme = Scope::workspace(Uuid::now_v7()).in_organization(None);
    update(
        &world,
        Scope::global(),
        json!({"welcome_text": "Hello", "page_size": 50}),
    )
    .await
    .unwrap();
    let portal = update(
        &world,
        acme,
        json!({"welcome_text": "Hi Acme", "page_size": 10, "show_leaderboard": true}),
    )
    .await
    .unwrap();
    assert_eq!(
        portal.overridden,
        ["page_size", "show_leaderboard", "welcome_text"]
    );

    let portal = inherit(&world, acme, "welcome_text").await.unwrap();
    assert_eq!(portal.welcome_text, "Hello");
    assert_eq!(portal.source("welcome_text"), Some(Source::Global));
    assert_eq!(portal.overridden, ["page_size", "show_leaderboard"]);
    // Inheriting a field that already inherits changes nothing.
    let again = inherit(&world, acme, "welcome_text").await.unwrap();
    assert_eq!(again.overridden, ["page_size", "show_leaderboard"]);
    // An unknown field is an error naming it.
    assert!(matches!(
        inherit(&world, acme, "colour").await,
        Err(SettingsError::UnknownFields { fields, .. }) if fields == ["colour"]
    ));

    let portal = world
        .work(|uow| {
            let settings = settings.clone();
            async move { settings.reset::<PortalSettings>(&uow, acme).await }
        })
        .await
        .unwrap();
    assert!(portal.overridden.is_empty());
    assert_eq!(portal.page_size, 50);
    assert_eq!(portal.source("show_leaderboard"), Some(Source::Default));
    // The row is gone with its last field.
    assert!(
        settings
            .stored::<PortalSettings>(acme)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        settings
            .rows(Some("portal"))
            .await
            .unwrap()
            .iter()
            .all(|row| row.scope.kind != navree_settings::ScopeKind::Workspace)
    );
}

#[tokio::test]
async fn a_value_equal_to_the_inherited_one_is_still_an_override() {
    let world = World::new().await;
    let acme = Scope::workspace(Uuid::now_v7()).in_organization(None);
    update(&world, Scope::global(), json!({"page_size": 50}))
        .await
        .unwrap();
    let pinned = update(&world, acme, json!({"page_size": 50}))
        .await
        .unwrap();
    assert_eq!(pinned.source("page_size"), Some(Source::Workspace));
    assert_eq!(pinned.overridden, ["page_size"]);
    // The global value changes; the workspace keeps its own.
    update(&world, Scope::global(), json!({"page_size": 75}))
        .await
        .unwrap();
    let settings = world.settings().await;
    assert_eq!(
        settings
            .get::<PortalSettings>(acme)
            .await
            .unwrap()
            .page_size,
        50
    );
}

#[tokio::test]
async fn writes_commit_or_roll_back_with_their_unit_of_work() {
    let world = World::new().await;
    let settings = world.settings().await;
    let db = world.db().await;

    // Rolled back: nothing stored, nothing announced.
    let mut events = world.events(&["settings.changed"]).await;
    let uow = db.uow(None, None).await;
    let inside = uow
        .scope({
            let settings = settings.clone();
            let uow = uow.clone();
            async move {
                settings
                    .update::<PortalSettings>(&uow, Scope::global(), json!({"page_size": 40}))
                    .await
                    .unwrap();
                // Reads inside the unit of work see its uncommitted write.
                settings
                    .get::<PortalSettings>(Scope::global())
                    .await
                    .unwrap()
            }
        })
        .await;
    assert_eq!(inside.page_size, 40);
    uow.rollback().await.unwrap();
    assert_eq!(
        settings
            .get::<PortalSettings>(Scope::global())
            .await
            .unwrap()
            .page_size,
        25
    );
    assert!(events.drain().is_empty());

    // Committed: stored, announced once.
    update(&world, Scope::global(), json!({"page_size": 40}))
        .await
        .unwrap();
    assert_eq!(
        settings
            .get::<PortalSettings>(Scope::global())
            .await
            .unwrap()
            .page_size,
        40
    );
    let drained = events.drain();
    assert_eq!(drained.len(), 1, "{drained:?}");
}

#[tokio::test]
async fn changes_emit_settings_changed_after_the_commit() {
    let world = World::new().await;
    let actor = world.user("Ada").await;
    let settings = world.settings().await;
    let db = world.db().await;
    let acme = Uuid::now_v7();
    let mut events = world.events(&["settings.changed"]).await;

    let uow = db.uow(Some(actor), None).await;
    uow.scope({
        let settings = settings.clone();
        let uow = uow.clone();
        async move {
            settings
                .update::<PortalSettings>(
                    &uow,
                    Scope::workspace(acme).in_organization(None),
                    json!({"welcome_text": "Hi", "page_size": 30}),
                )
                .await
                .unwrap();
        }
    })
    .await;
    // Nothing before the commit.
    assert!(events.drain().is_empty());
    uow.commit().await.unwrap();
    let drained = events.drain();
    assert_eq!(drained.len(), 1);
    assert_eq!(
        drained[0].1,
        json!({
            "key": "portal",
            "scope_type": "workspace",
            "scope_id": acme,
            "fields": ["page_size", "welcome_text"],
            "old": {},
            "new": {"welcome_text": "Hi", "page_size": 30},
            "actor_id": actor,
        })
    );

    // A patch that changes nothing stored emits nothing.
    update(
        &world,
        Scope::workspace(acme).in_organization(None),
        json!({"page_size": 30}),
    )
    .await
    .unwrap();
    assert!(events.drain().is_empty());

    // Inheriting reports the old stored value; `new` lacks the field.
    world
        .work(|uow| {
            let settings = settings.clone();
            async move {
                settings
                    .inherit::<PortalSettings>(
                        &uow,
                        Scope::workspace(acme).in_organization(None),
                        "welcome_text",
                    )
                    .await
            }
        })
        .await
        .unwrap();
    let drained = events.drain();
    assert_eq!(drained.len(), 1);
    assert_eq!(drained[0].1["fields"], json!(["welcome_text"]));
    assert_eq!(drained[0].1["old"], json!({"welcome_text": "Hi"}));
    assert_eq!(drained[0].1["new"], json!({}));
}

#[tokio::test]
async fn every_change_is_audited_in_its_transaction() {
    let world = World::new().await;
    let actor = world.user("Ada").await;
    let settings = world.settings().await;
    let db = world.db().await;
    let acme = Uuid::now_v7();

    let uow = db.uow(Some(actor), Some("req-9".to_owned())).await;
    uow.scope({
        let settings = settings.clone();
        let uow = uow.clone();
        async move {
            settings
                .update::<PortalSettings>(&uow, Scope::global(), json!({"page_size": 30}))
                .await
                .unwrap();
            settings
                .update::<PortalSettings>(
                    &uow,
                    Scope::workspace(acme).in_organization(None),
                    json!({"show_leaderboard": true}),
                )
                .await
                .unwrap();
        }
    })
    .await;
    uow.commit().await.unwrap();
    // A rolled-back change leaves no entry.
    let uow = db.uow(Some(actor), None).await;
    uow.scope({
        let settings = settings.clone();
        let uow = uow.clone();
        async move {
            settings
                .update::<PortalSettings>(&uow, Scope::global(), json!({"page_size": 99}))
                .await
                .unwrap();
        }
    })
    .await;
    uow.rollback().await.unwrap();
    update(&world, Scope::global(), json!({"page_size": 35}))
        .await
        .unwrap();

    world.app.get::<AuditLog>().await.flush().await;
    let entries = {
        let mut page = world.app
            .get::<AuditLog>()
            .await
            .list(
                &AuditFilter {
                    action: Some("settings.updated".to_owned()),
                    ..AuditFilter::default()
                },
                PageQuery {
                    page: 1,
                    per_page: 100,
                },
            )
            .await
            .unwrap();
        page.data.reverse();
        page.data
    };
    assert_eq!(entries.len(), 3, "{entries:#?}");
    let global = &entries[0];
    assert_eq!(global.kind, "custom");
    assert_eq!(global.actor_id, Some(actor));
    assert_eq!(global.subject_type.as_deref(), Some("Settings"));
    assert_eq!(global.subject_id.as_deref(), Some("portal"));
    assert_eq!(global.request_id.as_deref(), Some("req-9"));
    assert_eq!(global.old_values, Some(json!({})));
    assert_eq!(global.new_values, Some(json!({"page_size": 30})));
    assert_eq!(global.metadata["scope_type"], "global");
    assert_eq!(global.metadata["scope_id"], json!(null));
    assert_eq!(global.metadata["fields"], json!(["page_size"]));
    let workspace = &entries[1];
    assert_eq!(workspace.metadata["scope_type"], "workspace");
    assert_eq!(workspace.metadata["scope_id"], json!(acme));
    let later = &entries[2];
    assert_eq!(later.old_values, Some(json!({"page_size": 30})));
    assert_eq!(later.new_values, Some(json!({"page_size": 35})));
    assert_eq!(later.actor_id, None);
}

/// Writes `value` to the stored `portal` row of `scope_type`/`scope_id`
/// straight to the table, as another app instance with its own cache
/// would.
async fn write_behind_the_cache(db: &Db, scope_type: &str, scope_id: Uuid, value: Value) {
    let update = Query::update()
        .table("settings")
        .value("value", value)
        .and_where(Expr::col("key").eq("portal"))
        .and_where(Expr::col("scope_type").eq(scope_type))
        .and_where(Expr::col("scope_id").eq(scope_id))
        .to_owned();
    db.conn().execute(&update).await.unwrap();
}

async fn page_size(settings: &Settings, scope: Scope) -> u32 {
    settings
        .get::<PortalSettings>(scope)
        .await
        .unwrap()
        .page_size
}

#[tokio::test]
async fn resolved_values_are_cached_and_writes_clear_the_levels_they_touch() {
    let world = World::new().await;
    let settings = world.settings().await;
    let db = world.db().await;
    let (acme, globex) = (Uuid::now_v7(), Uuid::now_v7());
    let acme_scope = Scope::workspace(acme).in_organization(None);
    let globex_scope = Scope::workspace(globex).in_organization(None);
    update(&world, acme_scope, json!({"page_size": 10}))
        .await
        .unwrap();
    update(&world, globex_scope, json!({"page_size": 20}))
        .await
        .unwrap();
    assert_eq!(page_size(&settings, acme_scope).await, 10);
    assert_eq!(page_size(&settings, globex_scope).await, 20);

    // Another instance changes both rows: the cached values stay.
    write_behind_the_cache(&db, "workspace", acme, json!({"page_size": 11})).await;
    write_behind_the_cache(&db, "workspace", globex, json!({"page_size": 21})).await;
    assert_eq!(page_size(&settings, acme_scope).await, 10);
    assert_eq!(page_size(&settings, globex_scope).await, 20);

    // A write at Acme's level clears Acme's entries only.
    update(&world, acme_scope, json!({"show_leaderboard": true}))
        .await
        .unwrap();
    assert_eq!(page_size(&settings, acme_scope).await, 11);
    assert_eq!(page_size(&settings, globex_scope).await, 20);

    // A global write clears every entry of the key.
    update(&world, Scope::global(), json!({"welcome_text": "Hello"}))
        .await
        .unwrap();
    assert_eq!(page_size(&settings, globex_scope).await, 21);

    // A user's entry under Acme is cleared by a write at Acme's level.
    let ada = Uuid::now_v7();
    let ada_scope = Scope::user(ada).in_workspace(acme).in_organization(None);
    assert_eq!(page_size(&settings, ada_scope).await, 11);
    write_behind_the_cache(&db, "workspace", acme, json!({"page_size": 12})).await;
    assert_eq!(page_size(&settings, ada_scope).await, 11);
    update(&world, acme_scope, json!({"show_leaderboard": false}))
        .await
        .unwrap();
    assert_eq!(page_size(&settings, ada_scope).await, 12);
}

#[tokio::test]
async fn without_a_cache_lifetime_every_read_hits_the_store() {
    let world = World::with(crate::support::builder().config("settings.cache_ttl_secs", 0)).await;
    let settings = world.settings().await;
    let db = world.db().await;
    let acme = Uuid::now_v7();
    let scope = Scope::workspace(acme).in_organization(None);
    update(&world, scope, json!({"page_size": 10}))
        .await
        .unwrap();
    assert_eq!(page_size(&settings, scope).await, 10);
    write_behind_the_cache(&db, "workspace", acme, json!({"page_size": 11})).await;
    assert_eq!(page_size(&settings, scope).await, 11);
}

#[tokio::test]
async fn stored_values_the_type_no_longer_accepts_are_skipped() {
    let world = World::with(crate::support::builder().config("settings.cache_ttl_secs", 0)).await;
    let settings = world.settings().await;
    let db = world.db().await;
    let acme = Uuid::now_v7();
    let scope = Scope::workspace(acme).in_organization(None);
    update(&world, Scope::global(), json!({"page_size": 50}))
        .await
        .unwrap();
    update(&world, scope, json!({"page_size": 10}))
        .await
        .unwrap();
    // The field's type changed since the value was stored, and a field was
    // removed from the type.
    write_behind_the_cache(
        &db,
        "workspace",
        acme,
        json!({"page_size": "ten", "removed": true, "welcome_text": "Hi"}),
    )
    .await;
    let portal = settings.resolve::<PortalSettings>(scope).await.unwrap();
    // The level below shows through the unreadable value.
    assert_eq!(portal.page_size, 50);
    assert_eq!(portal.source("page_size"), Some(Source::Global));
    assert_eq!(portal.welcome_text, "Hi");
    assert_eq!(portal.source("welcome_text"), Some(Source::Workspace));
    assert_eq!(portal.source("removed"), None);
    // `overridden` lists only the type's fields whose values are used.
    assert_eq!(portal.overridden, ["welcome_text"]);

    // Setting the field again replaces the unreadable value.
    let fixed = update(&world, scope, json!({"page_size": 15}))
        .await
        .unwrap();
    assert_eq!(fixed.page_size, 15);
    assert_eq!(fixed.overridden, ["page_size", "welcome_text"]);
}
