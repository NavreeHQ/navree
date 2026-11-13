//! Preferences: channels people and workspaces turn off per category,
//! required categories, the `/api/notifications/preferences` routes and
//! a workspace's template overrides.

use std::sync::Arc;

use navree_contracts::{BoxError, BoxFuture, PermissionCheck, Principal};
use navree_db::Uow;
use navree_http::extract::Request;
use navree_http::middleware::Next;
use navree_http::{GlobalLayer, LayerSlot, ModuleBuilderHttpExt, Response};
use navree_kernel::{Factory, Module, ModuleBuilder};
use navree_notify::{
    Channel, MailOverride, NotificationOverrides, NotificationPreferences, Notifier, NotifyModule,
    NotifyOptions, PreferenceOptions, PreferenceOwner, Preferences, TemplateOverrides,
};
use navree_testing::principal;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::people::{
    DealUpdate, Mention, Person, SignInCode, TrainingDue, prefer, subjects, timed,
};
use crate::support;

/// Options with `security` required.
fn options() -> NotifyOptions {
    NotifyOptions {
        preferences: PreferenceOptions {
            required: vec!["security".to_owned()],
            ..PreferenceOptions::default()
        },
        ..NotifyOptions::default()
    }
}

#[tokio::test]
async fn channels_turned_off_are_skipped_and_the_workspace_gives_the_defaults() {
    let (builder, _clock, fakes) = timed(options(), "2026-10-28T12:00:00Z");
    let app = builder.build().await;
    let notifier = app.get::<Notifier>().await;
    let agra = Uuid::now_v7();
    prefer(
        &app,
        &PreferenceOwner::workspace(agra),
        json!({"channels": {"sms": false}}),
    )
    .await;
    let quiet = Person::in_zone(Some("UTC")).of(agra);
    prefer(
        &app,
        &quiet.owner(),
        json!({"categories": {"mentions": {"channels": {"mail": false}}}}),
    )
    .await;
    let texter = Person::in_zone(Some("UTC"))
        .of(agra)
        .phone("+44 7400 123456");
    prefer(&app, &texter.owner(), json!({"channels": {"sms": true}})).await;

    notifier
        .send(&quiet, Mention { by: "Anu".into() })
        .await
        .unwrap();
    notifier
        .send(&texter, Mention { by: "Bal".into() })
        .await
        .unwrap();

    // Both in-app notifications are stored.
    assert_eq!(support::rows(&app).await.len(), 2);
    // `quiet` turned mail off for mentions, the workspace turned SMS off.
    assert_eq!(subjects(&app).await, ["Bal mentioned you"]);
    assert!(fakes.sms.sent_to("+14155550100").is_empty());
    // `texter` turned SMS back on.
    assert_eq!(fakes.sms.sent_to("+447400123456").len(), 1);

    // A required category goes out whatever the switches say.
    notifier
        .send(
            &quiet,
            SignInCode {
                code: "123456".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(fakes.sms.sent_to("+14155550100").len(), 1);
}

#[tokio::test]
async fn turning_off_database_also_stops_the_broadcast() {
    let (builder, _clock, _fakes) = timed(NotifyOptions::default(), "2026-10-28T12:00:00Z");
    let broadcaster = Arc::new(support::RecordingBroadcaster::default());
    let app = builder
        .override_value::<dyn navree_contracts::Broadcaster>(broadcaster.clone())
        .build()
        .await;
    let ada = Person::in_zone(Some("UTC"));
    prefer(&app, &ada.owner(), json!({"channels": {"database": false}})).await;
    let mut update = DealUpdate::new("Agra");
    update.channels = vec![Channel::Database, Channel::Broadcast];
    app.get::<Notifier>()
        .await
        .send(&ada, update)
        .await
        .unwrap();
    assert!(support::rows(&app).await.is_empty());
    assert!(broadcaster.sent().is_empty());
}

#[tokio::test]
async fn preferences_are_disabled_with_notify_preferences_enabled_false() {
    let (builder, _clock, fakes) = timed(
        NotifyOptions {
            preferences: PreferenceOptions {
                enabled: false,
                ..PreferenceOptions::default()
            },
            ..NotifyOptions::default()
        },
        "2026-10-28T12:00:00Z",
    );
    let app = builder.build().await;
    let ada = Person::in_zone(Some("UTC"));
    prefer(&app, &ada.owner(), json!({"channels": {"sms": false}})).await;
    app.get::<Notifier>()
        .await
        .send(&ada, Mention { by: "Bal".into() })
        .await
        .unwrap();
    assert_eq!(fakes.sms.sent().len(), 1);
}

#[tokio::test]
async fn the_service_reads_merges_and_forgets() {
    let (builder, _clock, _fakes) = timed(options(), "2026-10-28T12:00:00Z");
    let app = builder.build().await;
    let preferences = app.get::<NotificationPreferences>().await;
    let ada = Person::in_zone(Some("UTC"));
    assert_eq!(
        preferences.of(&ada.owner()).await.unwrap(),
        Preferences::default()
    );

    let stored = preferences
        .update(
            &ada.owner(),
            &json!({"channels": {"sms": false}, "digest": "daily"}),
        )
        .await
        .unwrap();
    let again = preferences
        .update(
            &ada.owner(),
            &json!({"digest": null, "categories": {"deals": {"channels": {"mail": false}}}}),
        )
        .await
        .unwrap();
    assert_eq!(stored.channels.get(&Channel::Sms), Some(&false));
    assert_eq!(again.digest, None);
    assert_eq!(preferences.of(&ada.owner()).await.unwrap(), again);
    let resolved = preferences.resolve(&ada, None).await.unwrap();
    assert!(!resolved.allows("deals", Channel::Mail));
    assert!(!resolved.allows("billing", Channel::Sms));
    assert!(resolved.allows("security", Channel::Sms));

    let error = preferences
        .update(
            &ada.owner(),
            &json!({"categories": {"security": {"channels": {"sms": false}}}}),
        )
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("security notifications cannot be turned off"),
        "{error}"
    );

    let replaced = preferences
        .set(&ada.owner(), Preferences::default())
        .await
        .unwrap();
    assert!(replaced.is_empty());
    assert!(
        !preferences.forget(&ada.owner()).await.unwrap(),
        "the empty set was deleted"
    );
}

/// Registers the test notification types.
struct Catalog;

impl Module for Catalog {
    fn register(&self, m: &mut ModuleBuilder) {
        m.import(NotifyModule::for_feature::<DealUpdate>())
            .import(NotifyModule::for_feature::<SignInCode>())
            .import(NotifyModule::for_feature::<TrainingDue>());
    }
}

#[tokio::test]
async fn the_routes_show_and_change_the_signed_in_users_preferences() {
    let (builder, _clock, _fakes) = timed(options(), "2026-10-28T12:00:00Z");
    let app = builder.module(Catalog).build().await;
    let user = Uuid::now_v7();
    let as_user = app.acting_as(principal(user));

    let body = as_user
        .get("/api/notifications/preferences")
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(body["data"], json!({}));
    assert_eq!(body["workspace"], Value::Null);
    assert_eq!(body["time_zone"], "UTC");
    // No push service here: no switch for native app push.
    assert_eq!(
        body["channels"],
        json!(["mail", "database", "slack", "teams", "sms", "whatsapp", "web_push"])
    );
    let categories: Vec<&str> = body["categories"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["category"].as_str().unwrap())
        .collect();
    assert_eq!(categories, ["deals", "security", "training"]);
    assert_eq!(body["categories"][0]["types"], json!(["deal_update"]));
    assert_eq!(body["categories"][0]["digest"], "daily");
    assert_eq!(body["categories"][1]["required"], true);
    assert_eq!(body["categories"][0]["channels"]["sms"], true);

    let body = as_user
        .patch("/api/notifications/preferences")
        .json(&json!({
            "channels": {"sms": false},
            "categories": {"deals": {"digest": "off", "channels": {"mail": false}}},
            "quiet_hours": {"start": "22:00", "end": "07:00"}
        }))
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(
        body["data"],
        json!({
            "channels": {"sms": false},
            "categories": {"deals": {"channels": {"mail": false}, "digest": "off"}},
            "quiet_hours": {"start": "22:00", "end": "07:00"}
        })
    );
    let deals = &body["categories"][0];
    assert_eq!(deals["digest"], "off");
    assert_eq!(deals["channels"]["mail"], false);
    assert_eq!(deals["channels"]["sms"], false);
    assert_eq!(deals["channels"]["database"], true);
    // Security is required: on, whatever the switches.
    assert_eq!(body["categories"][1]["channels"]["sms"], true);

    // Stored for the user, and applied when sending.
    let stored = app
        .get::<NotificationPreferences>()
        .await
        .of(&PreferenceOwner::user(user))
        .await
        .unwrap();
    assert_eq!(stored.channel("deals", Channel::Mail), Some(false));

    // Refused changes name the field (400 validation_failed).
    let refused = as_user
        .patch("/api/notifications/preferences")
        .json(&json!({"categories": {"security": {"channels": {"sms": false}}, "billing": {"digest": "daily"}}}))
        .send()
        .await
        .assert_status(400)
        .json_value();
    assert_eq!(refused["code"], "validation_failed");
    assert_eq!(
        refused["errors"]["categories.security.channels.sms"],
        json!(["security notifications cannot be turned off"])
    );
    assert_eq!(
        refused["errors"]["categories.billing"],
        json!(["`billing` is not a notification category"])
    );
    let refused = as_user
        .patch("/api/notifications/preferences")
        .json(&json!({"quiet_hours": {"start": "late", "end": "07:00"}}))
        .send()
        .await
        .assert_status(400)
        .json_value();
    assert!(
        refused["errors"]["preferences"][0]
            .as_str()
            .unwrap()
            .contains("late")
    );

    // Guests get 401.
    app.client()
        .get("/api/notifications/preferences")
        .send()
        .await
        .assert_status(401);
}

/// Puts each request with an `X-Workspace` header in that workspace (what
/// navree-workspace does for workspace requests).
struct EnterWorkspace;

impl Module for EnterWorkspace {
    fn register(&self, m: &mut ModuleBuilder) {
        m.http_layer(GlobalLayer::new(
            LayerSlot::Context,
            "test-workspace",
            |_ctx, router| Ok(router.layer(navree_http::middleware::from_fn(enter))),
        ));
    }
}

async fn enter(request: Request, next: Next) -> Response {
    let workspace = request
        .headers()
        .get("x-workspace")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| Uuid::parse_str(value).ok());
    if let (Some(workspace), Some(uow)) = (workspace, request.extensions().get::<Uow>()) {
        uow.set_workspace(Some(workspace));
    }
    next.run(request).await
}

/// Grants `workspace.update` to one user.
struct Admins(Uuid);

impl PermissionCheck for Admins {
    fn check<'a>(
        &'a self,
        principal: &'a Principal,
        permission: &'a str,
        workspace: Option<Uuid>,
    ) -> BoxFuture<'a, Result<Option<bool>, BoxError>> {
        let granted = principal.user_id == self.0 && permission == "workspace.update";
        Box::pin(async move { Ok(workspace.map(|_| granted)) })
    }
}

impl Module for Admins {
    fn register(&self, m: &mut ModuleBuilder) {
        let check: Arc<dyn PermissionCheck> = Arc::new(Admins(self.0));
        m.multi(Factory::<dyn PermissionCheck>::new(move |_| {
            let check = check.clone();
            async move { Ok(check) }
        }));
    }
}

#[tokio::test]
async fn a_workspace_gives_its_members_defaults_that_admins_change() {
    let admin = Uuid::now_v7();
    let member = Uuid::now_v7();
    let agra = Uuid::now_v7().to_string();
    let (builder, _clock, _fakes) = timed(options(), "2026-10-28T12:00:00Z");
    let app = builder
        .module(Catalog)
        .module(EnterWorkspace)
        .module(Admins(admin))
        .build()
        .await;
    let as_admin = app.acting_as(principal(admin));
    let as_member = app.acting_as(principal(member));

    // Outside a workspace there is none.
    as_member
        .get("/api/notifications/preferences/workspace")
        .send()
        .await
        .assert_status(404);

    let defaults =
        json!({"channels": {"sms": false}, "categories": {"deals": {"digest": "weekly"}}});
    as_member
        .patch("/api/notifications/preferences/workspace")
        .header("x-workspace", agra.as_str())
        .json(&defaults)
        .send()
        .await
        .assert_status(403);
    let body = as_admin
        .patch("/api/notifications/preferences/workspace")
        .header("x-workspace", agra.as_str())
        .json(&defaults)
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(body["data"], defaults);
    assert_eq!(body["workspace"], Value::Null);
    assert_eq!(body["categories"][0]["digest"], "weekly");

    // Members see the defaults under their own choices.
    as_member
        .patch("/api/notifications/preferences")
        .header("x-workspace", agra.as_str())
        .json(&json!({"categories": {"deals": {"channels": {"sms": true}}}}))
        .send()
        .await
        .assert_status(200);
    let body = as_member
        .get("/api/notifications/preferences")
        .header("x-workspace", agra.as_str())
        .send()
        .await
        .assert_status(200)
        .json_value();
    assert_eq!(body["workspace"], defaults);
    let deals = &body["categories"][0];
    assert_eq!(deals["channels"]["sms"], true, "the member's own choice");
    assert_eq!(deals["digest"], "weekly", "the workspace's default");
    assert_eq!(body["categories"][2]["channels"]["sms"], false);

    // Any member reads the workspace's defaults.
    as_member
        .get("/api/notifications/preferences/workspace")
        .header("x-workspace", agra.as_str())
        .send()
        .await
        .assert_status(200);
}

/// Overrides for workspace `agra` and app-wide.
struct Branded {
    agra: Uuid,
}

impl TemplateOverrides for Branded {
    fn overrides(
        &self,
        workspace: Option<Uuid>,
    ) -> BoxFuture<'_, Result<NotificationOverrides, BoxError>> {
        let agra = self.agra;
        Box::pin(async move {
            let mut overrides = NotificationOverrides::default();
            if workspace == Some(agra) {
                overrides
                    .style
                    .insert("brand_color".into(), json!("#0F766E"));
                overrides.templates.insert(
                    "mentions".into(),
                    MailOverride {
                        subject: Some("Agra: {{ title }} mentioned you".into()),
                        html: Some(
                            "<p style=\"color: {{ style.brand_color }}\">{{ title }}</p>".into(),
                        ),
                        text: None,
                    },
                );
            } else if workspace.is_none() {
                overrides.templates.insert(
                    "mentions".into(),
                    MailOverride {
                        subject: Some("Mention from {{ title }}".into()),
                        ..MailOverride::default()
                    },
                );
            } else {
                return Err("unknown workspace".into());
            }
            Ok(overrides)
        })
    }
}

#[tokio::test]
async fn a_workspaces_overrides_restyle_its_mail() {
    let agra = Uuid::now_v7();
    let (builder, _clock, _fakes) = timed(NotifyOptions::default(), "2026-10-28T12:00:00Z");
    let app = builder
        .override_value::<dyn TemplateOverrides>(Arc::new(Branded { agra }))
        .build()
        .await;
    let notifier = app.get::<Notifier>().await;

    notifier
        .send(
            &Person::in_zone(Some("UTC")).of(agra),
            Mention { by: "Anu".into() },
        )
        .await
        .unwrap();
    notifier
        .send(&Person::in_zone(Some("UTC")), Mention { by: "Bal".into() })
        .await
        .unwrap();
    // A failing source sends the default mail.
    notifier
        .send(
            &Person::in_zone(Some("UTC")).of(Uuid::now_v7()),
            Mention { by: "Cy".into() },
        )
        .await
        .unwrap();

    let mail = support::sent_mail(&app).await;
    assert_eq!(mail[0].subject, "Agra: Anu mentioned you");
    assert_eq!(
        mail[0].html.as_deref(),
        Some("<p style=\"color: #0F766E\">Anu</p>")
    );
    assert_eq!(mail[0].text.as_deref(), Some("Anu was won."));
    assert_eq!(mail[1].subject, "Mention from Bal");
    assert_eq!(mail[2].subject, "Cy mentioned you");
}
