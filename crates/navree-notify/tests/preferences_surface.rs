//! Every public item `docs/digging-deeper/notification-preferences` names
//! (types, constants, methods, configuration keys and defaults, tables, the
//! command), so a rename fails the build or this test. The behavior is
//! exercised in `tests/notify`; the page's examples run in the facade's
//! `tests/notification_preferences_docs.rs`.

use navree_config::{Config, ConfigSection, ConfigSources};
use navree_notify::{
    CategoryPreferences, Channel, DEFERRALS_TABLE, DIGEST_TYPE, DeferralReason, DeferredOptions,
    Digest, DigestItem, DigestOptions, MailOverride, ModuleBuilderNotifyExt, NotificationCatalog,
    NotificationCategory, NotificationDeferral, NotificationKind, NotificationOverrides,
    NotificationPreference, NotificationPreferences, Notifier, NotifyError, NotifyModule,
    NotifyOptions, PREFERENCES_TABLE, PreferenceOptions, PreferenceOwner, Preferences, QuietHours,
    Released, ResolvedPreferences, TemplateOverrides, WORKSPACE_OWNER_TYPE, notification,
};

/// The configuration block of the page's "Configuration" section.
const DOCUMENTED: &str = r#"
[notify.preferences]
enabled = true
channels = ["mail", "database", "slack", "teams", "sms", "whatsapp", "web_push"]
required = ["security"]
workspace_permission = "workspace.update"

[notify.digest]
at = "09:00"
weekly_on = "monday"
subject = "Your notifications"
template = "notify/digest"

[notify.deferred]
channels = ["mail", "slack", "teams", "sms", "whatsapp", "web_push"]
batch = 500
max_attempts = 5
retry_secs = 300
schedule = true
"#;

#[test]
fn the_documented_configuration_is_the_default_plus_a_required_category() {
    let value: toml::Value = toml::from_str(DOCUMENTED).unwrap();
    let json = serde_json::to_value(&value["notify"]).unwrap();
    let config = Config::load(&ConfigSources::new().set("notify", json)).unwrap();
    let options = config.typed::<NotifyOptions>().unwrap();
    options.validate().unwrap();
    assert_eq!(
        options,
        NotifyOptions {
            preferences: PreferenceOptions {
                required: vec!["security".to_owned()],
                ..PreferenceOptions::default()
            },
            ..NotifyOptions::default()
        }
    );
}

#[test]
fn the_configuration_keys_have_their_documented_defaults() {
    let PreferenceOptions {
        enabled,
        channels,
        required,
        workspace_permission,
    } = PreferenceOptions::default();
    assert!(enabled && required.is_empty());
    assert_eq!(workspace_permission, "workspace.update");
    let names: Vec<&str> = channels.iter().map(|c| c.as_str()).collect();
    assert_eq!(
        names,
        [
            "mail", "database", "slack", "teams", "sms", "whatsapp", "web_push"
        ]
    );

    let DigestOptions {
        at,
        weekly_on,
        subject,
        template,
    } = DigestOptions::default();
    assert_eq!(
        (
            at.as_str(),
            weekly_on.as_str(),
            subject.as_str(),
            template.as_str()
        ),
        ("09:00", "monday", "Your notifications", "notify/digest")
    );

    let DeferredOptions {
        channels,
        batch,
        max_attempts,
        retry_secs,
        schedule,
    } = DeferredOptions::default();
    let names: Vec<&str> = channels.iter().map(|c| c.as_str()).collect();
    assert_eq!(
        names,
        ["mail", "slack", "teams", "sms", "whatsapp", "web_push"]
    );
    assert_eq!(
        (batch, max_attempts, retry_secs, schedule),
        (500, 5, 300, true)
    );
}

#[test]
fn lists_come_from_comma_separated_environment_variables() {
    let sources = ConfigSources::new().env_vars(
        "APP_",
        [
            ("APP_NOTIFY__PREFERENCES__REQUIRED", "security,billing"),
            ("APP_NOTIFY__DIGEST__AT", "08:30"),
            ("APP_NOTIFY__DEFERRED__CHANNELS", "sms,whatsapp"),
        ],
    );
    let options = Config::load(&sources)
        .unwrap()
        .typed::<NotifyOptions>()
        .unwrap();
    assert_eq!(options.preferences.required, ["security", "billing"]);
    assert_eq!(options.digest.at, "08:30");
    assert_eq!(options.deferred.channels, [Channel::Sms, Channel::WhatsApp]);
}

#[test]
fn invalid_values_are_refused_naming_the_key() {
    for (key, value, message) in [
        (
            "notify.digest.at",
            serde_json::json!("9am"),
            "notify.digest.at",
        ),
        (
            "notify.digest.weekly_on",
            serde_json::json!("someday"),
            "notify.digest.weekly_on",
        ),
        (
            "notify.deferred.batch",
            serde_json::json!(0),
            "notify.deferred.batch",
        ),
        (
            "notify.deferred.max_attempts",
            serde_json::json!(0),
            "notify.deferred.max_attempts",
        ),
        (
            "notify.preferences.channels",
            serde_json::json!(["broadcast"]),
            "notify.preferences.channels",
        ),
    ] {
        let error = Config::load(&ConfigSources::new().set(key, value))
            .unwrap()
            .typed::<NotifyOptions>()
            .unwrap_err()
            .to_string();
        assert!(error.contains(message), "{key}: {error}");
    }
}

#[test]
fn the_documented_items_exist() {
    // Tables and names.
    assert_eq!(PREFERENCES_TABLE, "notification_preferences");
    assert_eq!(DEFERRALS_TABLE, "notification_deferrals");
    assert_eq!(WORKSPACE_OWNER_TYPE, "workspace");
    assert_eq!(DIGEST_TYPE, "notify.digest");
    assert_eq!(
        [
            DeferralReason::Digest.as_str(),
            DeferralReason::QuietHours.as_str(),
            DeferralReason::SendWindow.as_str(),
        ],
        ["digest", "quiet_hours", "send_window"]
    );
    assert_eq!(
        Digest::ALL.map(Digest::as_str),
        ["off", "hourly", "daily", "weekly"]
    );

    // The services' documented methods.
    let _ = (
        NotificationPreferences::of,
        NotificationPreferences::update,
        NotificationPreferences::set,
        NotificationPreferences::forget,
        NotificationPreferences::resolve,
        NotificationPreferences::resolve_owner,
        ResolvedPreferences::allows,
        ResolvedPreferences::digest,
        ResolvedPreferences::quiet_hours,
        Notifier::release_due,
        NotificationCatalog::kinds,
        NotificationCatalog::kind,
        NotificationCatalog::categories,
        PreferenceOwner::of,
        PreferenceOwner::user,
        PreferenceOwner::workspace,
        NotificationDeferral::send_job,
        NotificationPreference::read,
        NotifyModule::for_feature::<Example>,
    );
    fn takes_ext<M: ModuleBuilderNotifyExt>(_: &M) {}
    let _ = takes_ext::<navree_kernel::ModuleBuilder>;
    fn takes_overrides(_: &dyn TemplateOverrides) {}
    let _ = takes_overrides;

    // The documented fields.
    let Preferences {
        channels: _,
        categories: _,
        digest: _,
        quiet_hours: _,
    } = Preferences::default();
    let CategoryPreferences {
        channels: _,
        digest: _,
    } = CategoryPreferences::default();
    let NotificationOverrides {
        style: _,
        templates: _,
    } = NotificationOverrides::default();
    let MailOverride {
        subject: _,
        html: _,
        text: _,
    } = MailOverride::default();
    let Released {
        sent: _,
        digests: _,
        batched: _,
        dropped: _,
        failed: _,
    } = Released::default();
    let _ = |kind: NotificationKind, category: NotificationCategory, item: DigestItem| {
        let NotificationKind {
            notification_type: _,
            category: _,
            digest: _,
            urgent: _,
        } = kind;
        let NotificationCategory {
            category: _,
            types: _,
            digest: _,
            required: _,
        } = category;
        let DigestItem {
            id: _,
            notification_type: _,
            category: _,
            mail: _,
            created_at: _,
        } = item;
    };
    let quiet = QuietHours::new("22:00", "07:00").unwrap();
    assert!(!quiet.is_empty());
    let error = NotifyError::InvalidPreferences(Default::default());
    assert!(
        error
            .to_string()
            .starts_with("invalid notification preferences")
    );
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Example;

#[notification(category = "examples", digest = "weekly")]
impl navree_notify::Notification for Example {
    const TYPE: &'static str = "example";

    fn via(&self, _n: &dyn navree_notify::Notifiable) -> Vec<Channel> {
        vec![Channel::Database]
    }
}

#[test]
fn the_attribute_is_re_exported() {
    use navree_notify::Notification as _;
    assert_eq!(Example::CATEGORY, "examples");
    assert_eq!(Example::DIGEST, Digest::Weekly);
}
