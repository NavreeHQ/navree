//! Settings over HTTP: `GET /api/settings`, `GET`, `PATCH` and `DELETE
//! /api/settings/{key}?scope=`, their permissions, the `Setting<T>`
//! extractor and the OpenAPI document.

use navree_api::{ApiDocs, ApiModule};
use navree_settings::{SETTINGS_MANAGE_PERMISSION, Scope, SettingsModule};
use navree_workspace::{OrgRole, WorkspaceRole};
use serde_json::{Value, json};

use crate::support::{NotificationSettings, PortalSettings, World, builder};

/// What only some users may read.
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
#[settings(
    key = "billing-contact",
    scopes(workspace),
    read_permission = "billing.read"
)]
struct BillingContact {
    #[garde(skip)]
    email: String,
}

/// Stores `patch` for `PortalSettings` at each scope, in order.
async fn store(world: &World, writes: Vec<(Scope, Value)>) {
    let settings = world.settings().await;
    for (scope, patch) in writes {
        let settings = settings.clone();
        world
            .work(|uow| async move { settings.update::<PortalSettings>(&uow, scope, patch).await })
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn guests_are_refused() {
    let world = World::new().await;
    world
        .app
        .client()
        .get("/api/settings")
        .send()
        .await
        .assert_error(401, None);
    world
        .app
        .client()
        .get("/api/settings/portal?scope=global")
        .send()
        .await
        .assert_error(401, None);
}

#[tokio::test]
async fn the_index_lists_every_registered_type() {
    let world = World::new().await;
    let ada = world.user("Ada").await;
    let body = world
        .as_user(ada)
        .get("/api/settings")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(
        body,
        json!({"data": [
            {
                "key": "notifications",
                "description": "How notification mail is sent.",
                "scopes": ["global", "workspace"],
                "fields": ["daily_digest", "sender_name"],
            },
            {
                "key": "portal",
                "description": "What the member portal shows.",
                "scopes": ["global", "organization", "workspace", "user"],
                "fields": ["page_size", "show_leaderboard", "welcome_text"],
            },
            {
                "key": "view-preferences",
                "description": "A user's view preferences.",
                "scopes": ["user"],
                "fields": ["compact", "theme"],
            },
        ]})
    );
}

#[tokio::test]
async fn reads_resolve_for_the_scope_with_sources_and_overrides() {
    let world = World::new().await;
    let ada = world.user("Ada").await;
    let holding = world.organization(ada, "Holding").await;
    let acme = world.workspace(ada, "Acme").await;
    world.attach(holding.id, acme.id).await;
    store(
        &world,
        vec![
            (Scope::global(), json!({"page_size": 50})),
            (
                Scope::organization(holding.id),
                json!({"show_leaderboard": true}),
            ),
            (
                Scope::workspace(acme.id),
                json!({"welcome_text": "Hi Acme"}),
            ),
            (Scope::user(ada), json!({"page_size": 10})),
        ],
    )
    .await;

    let body = world
        .as_user(ada)
        .get("/api/settings/portal?scope=workspace")
        .header("X-Workspace", "acme")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(
        body,
        json!({
            "data": {"show_leaderboard": true, "welcome_text": "Hi Acme", "page_size": 50},
            "sources": {
                "page_size": "global",
                "show_leaderboard": "organization",
                "welcome_text": "workspace",
            },
            "overridden": ["welcome_text"],
        })
    );

    world
        .as_user(ada)
        .get("/api/settings/portal?scope=organization")
        .header("X-Organization", "holding")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("overridden", json!(["show_leaderboard"]))
        .assert_json_path("sources.welcome_text", json!("default"));

    world
        .as_user(ada)
        .get("/api/settings/portal?scope=global")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.page_size", json!(50))
        .assert_json_path("overridden", json!(["page_size"]));

    // Without `scope`: the most specific level the type allows, here the
    // user's, over the request's workspace.
    world
        .as_user(ada)
        .get("/api/settings/portal")
        .header("X-Workspace", "acme")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.page_size", json!(10))
        .assert_json_path("sources.page_size", json!("user"))
        .assert_json_path("sources.welcome_text", json!("workspace"))
        .assert_json_path("overridden", json!(["page_size"]));
    // Without a workspace, the user's over the global values.
    world
        .as_user(ada)
        .get("/api/settings/portal")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("sources.welcome_text", json!("default"));
}

#[tokio::test]
async fn reads_need_what_the_scope_needs() {
    let world = World::new().await;
    let ada = world.user("Ada").await;
    let grace = world.user("Grace").await;
    world.workspace(ada, "Acme").await;
    world.organization(ada, "Holding").await;

    // Not a member of the workspace or the organization.
    world
        .as_user(grace)
        .get("/api/settings/portal?scope=workspace")
        .header("X-Workspace", "acme")
        .send()
        .await
        .assert_status(403);
    world
        .as_user(grace)
        .get("/api/settings/portal?scope=organization")
        .header("X-Organization", "holding")
        .send()
        .await
        .assert_status(403);
    // A workspace level without a workspace named.
    world
        .as_user(ada)
        .get("/api/settings/portal?scope=workspace")
        .send()
        .await
        .assert_status(400);
    // Unknown keys and levels, and levels the type does not allow.
    world
        .as_user(ada)
        .get("/api/settings/nope?scope=global")
        .send()
        .await
        .assert_error(404, Some("unknown settings `nope`"));
    world
        .as_user(ada)
        .get("/api/settings/portal?scope=team")
        .send()
        .await
        .assert_status(400)
        .assert_json_path(
            "errors.scope.0",
            json!("unknown scope `team`: expected `global`, `organization`, `workspace` or `user`"),
        );
    world
        .as_user(ada)
        .get("/api/settings/notifications?scope=user")
        .send()
        .await
        .assert_error(
            400,
            Some(
                "the `notifications` settings cannot be stored at the user level; they allow \
                 global, workspace",
            ),
        );
}

#[tokio::test]
async fn a_read_permission_guards_reads() {
    let world =
        World::with(builder().module(SettingsModule::for_feature::<BillingContact>())).await;
    let ada = world.user("Ada").await;
    let grace = world.user("Grace").await;
    let acme = world.workspace(ada, "Acme").await;
    world.member(acme.id, grace, WorkspaceRole::Member).await;

    world
        .as_user(grace)
        .get("/api/settings/billing-contact")
        .header("X-Workspace", "acme")
        .send()
        .await
        .assert_status(403);
    // The owner holds every permission in the workspace.
    world
        .as_user(ada)
        .get("/api/settings/billing-contact")
        .header("X-Workspace", "acme")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.email", json!(""));
}

#[tokio::test]
async fn writes_need_the_level_permission() {
    let world = World::new().await;
    let ada = world.user("Ada").await;
    let grace = world.user("Grace").await;
    let linus = world.user("Linus").await;
    let holding = world.organization(ada, "Holding").await;
    let acme = world.workspace(ada, "Acme").await;
    world.member(acme.id, grace, WorkspaceRole::Member).await;
    world.member(acme.id, linus, WorkspaceRole::Admin).await;
    world.org_member(holding.id, grace, OrgRole::Member).await;

    // Workspace: `workspace.update` (admins and owners).
    world
        .as_user(grace)
        .patch("/api/settings/portal?scope=workspace")
        .header("X-Workspace", "acme")
        .json(&json!({"welcome_text": "Hi"}))
        .send()
        .await
        .assert_status(403);
    world
        .as_user(linus)
        .patch("/api/settings/portal?scope=workspace")
        .header("X-Workspace", "acme")
        .json(&json!({"welcome_text": "Hi from Linus"}))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.welcome_text", json!("Hi from Linus"))
        .assert_json_path("sources.welcome_text", json!("workspace"))
        .assert_json_path("overridden", json!(["welcome_text"]));

    // Organization: `organization.update`.
    world
        .as_user(grace)
        .patch("/api/settings/portal?scope=organization")
        .header("X-Organization", "holding")
        .json(&json!({"show_leaderboard": true}))
        .send()
        .await
        .assert_status(403);
    world
        .as_user(ada)
        .patch("/api/settings/portal?scope=organization")
        .header("X-Organization", "holding")
        .json(&json!({"show_leaderboard": true}))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("overridden", json!(["show_leaderboard"]));

    // Global: `settings.manage`, app-wide.
    world
        .as_user(ada)
        .patch("/api/settings/portal?scope=global")
        .json(&json!({"page_size": 40}))
        .send()
        .await
        .assert_status(403);
    world.grant(grace, SETTINGS_MANAGE_PERMISSION).await;
    world
        .as_user(grace)
        .patch("/api/settings/portal?scope=global")
        .json(&json!({"page_size": 40}))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("sources.page_size", json!("global"));

    // User: anyone, for their own values.
    world
        .as_user(grace)
        .patch("/api/settings/portal?scope=user")
        .json(&json!({"page_size": 5}))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("sources.page_size", json!("user"));
    let settings = world.settings().await;
    assert_eq!(
        settings
            .get::<PortalSettings>(Scope::user(grace))
            .await
            .unwrap()
            .page_size,
        5
    );
    assert_eq!(
        settings
            .get::<PortalSettings>(Scope::user(ada))
            .await
            .unwrap()
            .page_size,
        40
    );
}

#[tokio::test]
async fn a_type_permission_replaces_the_level_permissions() {
    let world = World::new().await;
    let ada = world.user("Ada").await;
    let linus = world.user("Linus").await;
    let acme = world.workspace(ada, "Acme").await;
    world.member(acme.id, linus, WorkspaceRole::Admin).await;

    // `notifications.manage`: an admin has `workspace.update`, not this.
    world
        .as_user(linus)
        .patch("/api/settings/notifications?scope=workspace")
        .header("X-Workspace", "acme")
        .json(&json!({"daily_digest": true}))
        .send()
        .await
        .assert_status(403);
    // The owner holds every permission in the workspace.
    world
        .as_user(ada)
        .patch("/api/settings/notifications?scope=workspace")
        .header("X-Workspace", "acme")
        .json(&json!({"daily_digest": true}))
        .send()
        .await
        .assert_status(200);
    // Globally, it is granted app-wide.
    world
        .as_user(ada)
        .patch("/api/settings/notifications?scope=global")
        .json(&json!({"sender_name": "Acme"}))
        .send()
        .await
        .assert_status(403);
    world.grant(linus, "notifications.manage").await;
    world
        .as_user(linus)
        .patch("/api/settings/notifications?scope=global")
        .json(&json!({"sender_name": "Acme"}))
        .send()
        .await
        .assert_status(200);
    let settings = world.settings().await;
    let notifications = settings
        .resolve::<NotificationSettings>(Scope::workspace(acme.id))
        .await
        .unwrap();
    assert!(notifications.daily_digest);
    assert_eq!(notifications.sender_name, "Acme");
}

#[tokio::test]
async fn invalid_patches_are_400s_naming_the_field() {
    let world = World::new().await;
    let ada = world.user("Ada").await;
    world.workspace(ada, "Acme").await;
    let patch = |body: Value| {
        world
            .as_user(ada)
            .patch("/api/settings/portal?scope=workspace")
            .header("X-Workspace", "acme")
            .json(&body)
    };

    patch(json!({"welcome_text": "x".repeat(41)}))
        .send()
        .await
        .assert_status(400)
        .assert_json_path(
            "errors.welcome_text.0",
            json!("welcome_text must be shorter than or equal to 40 characters"),
        );
    patch(json!({"page_size": 0}))
        .send()
        .await
        .assert_status(400)
        .assert_json_path("statusCode", json!(400))
        .assert_json_path(
            "errors.page_size.0",
            json!("page_size must not be less than 1"),
        );
    patch(json!({"colour": "red"}))
        .send()
        .await
        .assert_status(400)
        .assert_json_path(
            "errors.colour.0",
            json!("colour is not a setting of portal"),
        );
    world
        .as_user(ada)
        .patch("/api/settings/nope?scope=global")
        .json(&json!({"x": 1}))
        .send()
        .await
        .assert_status(404);
}

#[tokio::test]
async fn delete_makes_fields_inherit_again() {
    let world = World::new().await;
    let ada = world.user("Ada").await;
    let acme = world.workspace(ada, "Acme").await;
    world
        .as_user(ada)
        .patch("/api/settings/portal?scope=workspace")
        .header("X-Workspace", "acme")
        .json(&json!({"welcome_text": "Hi", "page_size": 10}))
        .send()
        .await
        .assert_status(200)
        .assert_json_path("overridden", json!(["page_size", "welcome_text"]));

    world
        .as_user(ada)
        .delete("/api/settings/portal/welcome_text?scope=workspace")
        .header("X-Workspace", "acme")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("data.welcome_text", json!("Welcome"))
        .assert_json_path("sources.welcome_text", json!("default"))
        .assert_json_path("overridden", json!(["page_size"]));
    world
        .as_user(ada)
        .delete("/api/settings/portal/colour?scope=workspace")
        .header("X-Workspace", "acme")
        .send()
        .await
        .assert_status(400);
    world
        .as_user(ada)
        .delete("/api/settings/portal?scope=workspace")
        .header("X-Workspace", "acme")
        .send()
        .await
        .assert_status(200)
        .assert_json_path("overridden", json!([]));
    let settings = world.settings().await;
    assert!(
        settings
            .stored::<PortalSettings>(Scope::workspace(acme.id))
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn the_extractor_resolves_for_the_request() {
    let world = World::new().await;
    let ada = world.user("Ada").await;
    let grace = world.user("Grace").await;
    let holding = world.organization(ada, "Holding").await;
    let acme = world.workspace(ada, "Acme").await;
    world.attach(holding.id, acme.id).await;
    store(
        &world,
        vec![
            (Scope::global(), json!({"welcome_text": "Hello"})),
            (
                Scope::organization(holding.id),
                json!({"show_leaderboard": true}),
            ),
            (
                Scope::workspace(acme.id),
                json!({"welcome_text": "Hi Acme"}),
            ),
        ],
    )
    .await;

    let body = world
        .as_user(ada)
        .get("/api/portal")
        .header("X-Workspace", "acme")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(
        body,
        json!({
            "welcome": "Hi Acme",
            "leaderboard": true,
            "source": "workspace",
            "user": ada,
            "workspace": acme.id,
            "organization": holding.id,
        })
    );
    // No workspace named: the user's and the global values.
    let body = world
        .as_user(ada)
        .get("/api/portal")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(
        body,
        json!({
            "welcome": "Hello",
            "leaderboard": false,
            "source": "global",
            "user": ada,
            "workspace": null,
            "organization": null,
        })
    );
    // A workspace the user may not enter is refused.
    world
        .as_user(grace)
        .get("/api/portal")
        .header("X-Workspace", "acme")
        .send()
        .await
        .assert_status(403);
    // A guest gets the global values.
    let body = world
        .app
        .client()
        .get("/api/portal/public")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(body, json!({"welcome": "Hello", "source": "global"}));
}

#[tokio::test]
async fn routes_can_be_turned_off_and_are_documented() {
    let world = World::with(builder().config("settings.routes", false)).await;
    let ada = world.user("Ada").await;
    world
        .as_user(ada)
        .get("/api/settings")
        .send()
        .await
        .assert_status(404);

    let world = World::with(builder().module(ApiModule::from_config())).await;
    let docs = world.app.get::<ApiDocs>().await;
    let document = docs.document().await.expect("the document builds");
    let paths = &document["paths"];
    for (path, method) in [
        ("/api/settings", "get"),
        ("/api/settings/{key}", "get"),
        ("/api/settings/{key}", "patch"),
        ("/api/settings/{key}", "delete"),
        ("/api/settings/{key}/{field}", "delete"),
    ] {
        assert!(
            paths[path][method].is_object(),
            "{method} {path} is not documented: {paths:#}"
        );
        assert_eq!(
            paths[path][method]["tags"],
            json!(["Settings"]),
            "{method} {path}"
        );
    }
    assert!(
        document["components"]["schemas"]["ResolvedSettings"].is_object(),
        "{:#}",
        document["components"]["schemas"]
    );
}
