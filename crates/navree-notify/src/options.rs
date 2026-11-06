//! [`NotifyOptions`]: configuration section `notify`.

use chrono::{NaiveTime, Weekday};
use navree_config::{ConfigSection, ConfigTable};
use navree_contracts::Secret;
use serde::{Deserialize, Serialize};

use crate::Channel;
use crate::timing::{parse_hh_mm, parse_weekday};

/// How [`NotifyModule`](crate::NotifyModule) sends notifications
/// (configuration section `notify`, e.g. `APP_NOTIFY__QUEUE=true`).
///
/// Build it with `..NotifyOptions::default()` so new fields keep compiling.
/// Secrets (tokens, passwords, private keys) are read from the
/// configuration but never serialized.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ConfigSection)]
#[config(key = "notify", docs = "/docs/digging-deeper/notifications/", validate = Self::check)]
#[serde(default)]
pub struct NotifyOptions {
    /// Queue notifications: [`Notifier::send`](crate::Notifier::send)
    /// renders the notification, then dispatches one job `notify.send` per
    /// channel through the app's `dyn Dispatcher`, so a queue worker
    /// delivers (and retries) each channel on its own. Without a
    /// dispatcher in the app, notifications are delivered at once. Default:
    /// `false` (deliver at once).
    pub queue: bool,
    /// Serve the signed-in user's notifications under
    /// `{api_prefix}/notifications` (authenticated routes, so the app needs
    /// a `dyn Authenticator`), and their push subscriptions. Default: `true`.
    pub routes: bool,
    /// Record each delivery over the Slack, Teams, SMS, WhatsApp and web
    /// push channels in `notification_deliveries` (with `DatabaseModule`).
    /// Default: `true`.
    pub deliveries: bool,
    /// The Slack channel.
    #[config(table)]
    pub slack: SlackOptions,
    /// The Microsoft Teams channel.
    #[config(table)]
    pub teams: TeamsOptions,
    /// The SMS channel.
    #[config(table)]
    pub sms: SmsOptions,
    /// The WhatsApp channel.
    #[config(table)]
    pub whatsapp: WhatsAppOptions,
    /// The web push channel.
    #[config(table)]
    pub web_push: WebPushOptions,
    /// Twilio credentials, for `sms.driver` and `whatsapp.driver` `twilio`.
    #[config(table)]
    pub twilio: TwilioOptions,
    /// Vonage credentials, for `sms.driver = "vonage"`.
    #[config(table)]
    pub vonage: VonageOptions,
    /// Amazon SNS credentials, for `sms.driver = "sns"`.
    #[config(table)]
    pub sns: SnsOptions,
    /// WhatsApp Cloud API credentials, for `whatsapp.driver = "meta"`.
    #[config(table)]
    pub meta: MetaOptions,
    /// What people and workspaces choose to receive.
    #[config(table)]
    pub preferences: PreferenceOptions,
    /// The digest mail of batched notifications.
    #[config(table)]
    pub digest: DigestOptions,
    /// Deliveries held for quiet hours, send windows and digests.
    #[config(table)]
    pub deferred: DeferredOptions,
}

impl Default for NotifyOptions {
    fn default() -> Self {
        Self {
            queue: false,
            routes: true,
            deliveries: true,
            slack: SlackOptions::default(),
            teams: TeamsOptions::default(),
            sms: SmsOptions::default(),
            whatsapp: WhatsAppOptions::default(),
            web_push: WebPushOptions::default(),
            twilio: TwilioOptions::default(),
            vonage: VonageOptions::default(),
            sns: SnsOptions::default(),
            meta: MetaOptions::default(),
            preferences: PreferenceOptions::default(),
            digest: DigestOptions::default(),
            deferred: DeferredOptions::default(),
        }
    }
}

/// `notify.preferences`: the channels people switch per category.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ConfigTable)]
#[serde(default)]
pub struct PreferenceOptions {
    /// Apply the stored preferences when sending (with `DatabaseModule`).
    /// Default: `true`.
    pub enabled: bool,
    /// The channels people may switch, in the order the preference routes
    /// list them: a list, or a comma-separated string from an environment
    /// variable. Default: every channel but `broadcast`, which follows the
    /// `database` choice (the real-time event of an in-app notification).
    #[serde(deserialize_with = "navree_config::comma_list_of")]
    #[config(default = "mail, database, slack, teams, sms, whatsapp, web_push")]
    pub channels: Vec<Channel>,
    /// Categories that cannot be turned off (`security`): their
    /// notifications go out over every channel they name, and a change
    /// that turns one of their channels off is refused. Default: none.
    #[serde(deserialize_with = "navree_config::comma_list")]
    pub required: Vec<String>,
    /// The permission that changes a workspace's defaults
    /// (`PATCH /api/notifications/preferences/workspace`), checked in the
    /// request's workspace. Default: `workspace.update`.
    pub workspace_permission: String,
}

impl Default for PreferenceOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            channels: Channel::ALL
                .into_iter()
                .filter(|channel| *channel != Channel::Broadcast)
                .collect(),
            required: Vec::new(),
            workspace_permission: "workspace.update".to_owned(),
        }
    }
}

impl PreferenceOptions {
    /// Whether `category` cannot be turned off.
    pub fn is_required(&self, category: &str) -> bool {
        self.required.iter().any(|required| required == category)
    }
}

/// `notify.digest`: when digests go out and how they look.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ConfigTable)]
#[serde(default)]
pub struct DigestOptions {
    /// The local time daily and weekly digests go out, `HH:MM` on the
    /// recipient's clock. Default: `09:00`.
    pub at: String,
    /// The day weekly digests go out (`monday` to `sunday`). Default:
    /// `monday`.
    pub weekly_on: String,
    /// The digest mail's subject, a template with `count` and `digest`
    /// (`daily`). Default: `Your notifications`.
    pub subject: String,
    /// The digest mail's template (`{template}.html` and `.txt`), with
    /// `items`, `count`, `digest` and `style`. Default: `notify/digest`,
    /// built in; a file of that name replaces it.
    pub template: String,
}

impl Default for DigestOptions {
    fn default() -> Self {
        Self {
            at: "09:00".to_owned(),
            weekly_on: "monday".to_owned(),
            subject: "Your notifications".to_owned(),
            template: "notify/digest".to_owned(),
        }
    }
}

impl DigestOptions {
    /// [`at`](DigestOptions::at), parsed (09:00 when invalid; the options
    /// refuse to load with such a value).
    pub fn time(&self) -> NaiveTime {
        parse_hh_mm(&self.at)
            .unwrap_or_else(|_| NaiveTime::from_hms_opt(9, 0, 0).unwrap_or_default())
    }

    /// [`weekly_on`](DigestOptions::weekly_on), parsed (Monday when
    /// invalid).
    pub fn weekday(&self) -> Weekday {
        parse_weekday(&self.weekly_on).unwrap_or(Weekday::Mon)
    }
}

/// `notify.deferred`: deliveries held for quiet hours, send windows and
/// digests, released by `notify:release`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ConfigTable)]
#[serde(default)]
pub struct DeferredOptions {
    /// The channels quiet hours and send windows hold. Default: `mail`,
    /// `slack`, `teams`, `sms`, `whatsapp` and `web_push` (the database
    /// and broadcast channels show in the app at once).
    #[serde(deserialize_with = "navree_config::comma_list_of")]
    #[config(default = "mail, slack, teams, sms, whatsapp, web_push")]
    pub channels: Vec<Channel>,
    /// How many held deliveries one `notify:release` run releases at most.
    /// Default: 500.
    pub batch: u64,
    /// Attempts before a held delivery that keeps failing is dropped
    /// (logged). Default: 5.
    pub max_attempts: u32,
    /// Seconds before a release that failed is tried again. Default: 300.
    pub retry_secs: u64,
    /// Run `notify:release` every minute on `ScheduleModule`'s scheduler.
    /// Default: `true`.
    pub schedule: bool,
}

impl Default for DeferredOptions {
    fn default() -> Self {
        Self {
            channels: vec![
                Channel::Mail,
                Channel::Slack,
                Channel::MicrosoftTeams,
                Channel::Sms,
                Channel::WhatsApp,
                Channel::WebPush,
            ],
            batch: 500,
            max_attempts: 5,
            retry_secs: 300,
            schedule: true,
        }
    }
}

impl DeferredOptions {
    /// Whether quiet hours and send windows hold `channel`.
    pub fn holds(&self, channel: Channel) -> bool {
        self.channels.contains(&channel)
    }
}

impl NotifyOptions {
    /// Checks the values (the boot calls this).
    fn check(&self) -> Result<(), String> {
        parse_hh_mm(&self.digest.at).map_err(|error| format!("notify.digest.at: {error}"))?;
        parse_weekday(&self.digest.weekly_on)
            .map_err(|error| format!("notify.digest.weekly_on: {error}"))?;
        if self.digest.template.trim().is_empty() {
            return Err("notify.digest.template must name a template".into());
        }
        if self.deferred.batch == 0 {
            return Err("notify.deferred.batch must be at least 1".into());
        }
        if self.deferred.max_attempts == 0 {
            return Err("notify.deferred.max_attempts must be at least 1".into());
        }
        if self.preferences.channels.contains(&Channel::Broadcast) {
            return Err(
                "notify.preferences.channels cannot list `broadcast`: it follows `database`".into(),
            );
        }
        if let Some(driver) = self.sms.driver {
            match driver {
                SmsDriverKind::Twilio => self.twilio.check("sms")?,
                SmsDriverKind::Vonage => self.vonage.check()?,
                SmsDriverKind::Sns => self.sns.check()?,
                SmsDriverKind::Log => {}
            }
        }
        if let Some(region) = &self.sms.default_region {
            navree_validation::Country::parse(region).map_err(|_| {
                format!("notify.sms.default_region `{region}` is not an ISO 3166-1 alpha-2 code")
            })?;
        }
        if let Some(driver) = self.whatsapp.driver {
            match driver {
                WhatsAppDriverKind::Twilio => self.twilio.check("whatsapp")?,
                WhatsAppDriverKind::Meta => self.meta.check()?,
                WhatsAppDriverKind::Log => {}
            }
        }
        self.web_push.check()?;
        if self.teams.app_id.is_some() && self.teams.app_password.expose().is_empty() {
            return Err("notify.teams.app_password is required with notify.teams.app_id".into());
        }
        for (key, url) in [
            ("notify.slack.endpoint", &self.slack.endpoint),
            ("notify.teams.token_url", &self.teams.token_url),
            ("notify.twilio.endpoint", &self.twilio.endpoint),
            ("notify.vonage.endpoint", &self.vonage.endpoint),
            ("notify.sns.endpoint", &self.sns.endpoint),
            ("notify.meta.endpoint", &self.meta.endpoint),
            ("notify.sms.status_callback", &self.sms.status_callback),
            (
                "notify.whatsapp.status_callback",
                &self.whatsapp.status_callback,
            ),
        ] {
            if let Some(url) = url.as_deref().filter(|url| !url.trim().is_empty()) {
                check_url(key, url)?;
            }
        }
        Ok(())
    }
}

/// An absolute http(s) URL, or an error naming `key`.
fn check_url(key: &str, url: &str) -> Result<(), String> {
    let parsed =
        url::Url::parse(url.trim()).map_err(|error| format!("{key} is not a URL: {error}"))?;
    if !matches!(parsed.scheme(), "http" | "https") || !parsed.has_host() {
        return Err(format!("{key} must be an absolute http(s) URL"));
    }
    Ok(())
}

/// `notify.slack`: the bot token of bot routes without a connection.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ConfigTable)]
#[serde(default)]
pub struct SlackOptions {
    /// The bot token (`xoxb-...`) for [`SlackRoute::channel`](crate::SlackRoute::channel).
    #[serde(skip_serializing)]
    pub bot_token: Secret<String>,
    /// The Web API's base URL (default `https://slack.com/api`).
    pub endpoint: Option<String>,
}

/// `notify.teams`: the bot that sends proactive messages to conversations
/// without a connection (an Azure Bot's app registration).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ConfigTable)]
#[serde(default)]
pub struct TeamsOptions {
    /// The bot's Microsoft app id.
    pub app_id: Option<String>,
    /// The bot's client secret.
    #[serde(skip_serializing)]
    pub app_password: Secret<String>,
    /// The tenant that issues the bot's tokens: `botframework.com` for a
    /// multi-tenant bot (default), the tenant id for a single-tenant one.
    pub tenant: String,
    /// The token endpoint (default
    /// `https://login.microsoftonline.com/{tenant}/oauth2/v2.0/token`).
    #[config(public)]
    pub token_url: Option<String>,
}

impl Default for TeamsOptions {
    fn default() -> Self {
        Self {
            app_id: None,
            app_password: Secret::default(),
            tenant: "botframework.com".to_owned(),
            token_url: None,
        }
    }
}

/// An SMS driver.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SmsDriverKind {
    /// Twilio's Messages API (`notify.twilio`).
    Twilio,
    /// Vonage's Messages API (`notify.vonage`).
    Vonage,
    /// Amazon SNS `Publish` (`notify.sns`); `aws_sns` is accepted too.
    #[serde(alias = "aws_sns")]
    Sns,
    /// Writes each message to the log (development).
    Log,
}

/// `notify.sms`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ConfigTable)]
#[serde(default)]
pub struct SmsOptions {
    /// The driver; without one the SMS channel is unavailable.
    #[config(one_of = "twilio, vonage, sns, log")]
    pub driver: Option<SmsDriverKind>,
    /// The default sender: a number in E.164, an alphanumeric sender id, or
    /// a Twilio messaging service (`MG...`).
    pub from: Option<String>,
    /// The region numbers without a country code are read in (`US`,
    /// `GB`); without it such numbers are invalid.
    pub default_region: Option<String>,
    /// The URL the provider reports delivery statuses to (Twilio's
    /// `StatusCallback`, Vonage's `webhook_url`), e.g.
    /// `https://api.acme.test/api/webhooks/notify/twilio`.
    pub status_callback: Option<String>,
}

/// A WhatsApp driver.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WhatsAppDriverKind {
    /// Twilio's Messages API with `whatsapp:` addresses (`notify.twilio`).
    Twilio,
    /// The WhatsApp Cloud API (`notify.meta`).
    Meta,
    /// Writes each message to the log (development).
    Log,
}

/// `notify.whatsapp`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ConfigTable)]
#[serde(default)]
pub struct WhatsAppOptions {
    /// The driver; without one the WhatsApp channel is unavailable.
    #[config(one_of = "twilio, meta, log")]
    pub driver: Option<WhatsAppDriverKind>,
    /// The sender's number in E.164 (Twilio).
    pub from: Option<String>,
    /// The URL Twilio reports delivery statuses to.
    pub status_callback: Option<String>,
}

/// `notify.web_push`: the VAPID key pair (see `notify:vapid`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ConfigTable)]
#[serde(default)]
pub struct WebPushOptions {
    /// The application server key: the public key, base64url (65 bytes).
    pub public_key: Option<String>,
    /// The private key, base64url (32 bytes).
    #[serde(skip_serializing)]
    pub private_key: Secret<String>,
    /// The contact push services reach you at: `mailto:` or `https:`.
    pub subject: String,
    /// How long push services keep a message for an offline device, in
    /// seconds (default 86400, one day).
    pub ttl_secs: u32,
}

impl Default for WebPushOptions {
    fn default() -> Self {
        Self {
            public_key: None,
            private_key: Secret::default(),
            subject: String::new(),
            ttl_secs: 86_400,
        }
    }
}

impl WebPushOptions {
    /// Whether a key pair is configured.
    pub fn is_configured(&self) -> bool {
        self.public_key.as_deref().is_some_and(|k| !k.is_empty())
    }

    fn check(&self) -> Result<(), String> {
        if !self.is_configured() {
            return Ok(());
        }
        if self.private_key.expose().is_empty() {
            return Err(
                "notify.web_push.private_key is required with notify.web_push.public_key".into(),
            );
        }
        if !(self.subject.starts_with("mailto:") || self.subject.starts_with("https://")) {
            return Err("notify.web_push.subject must be a `mailto:` or `https://` contact".into());
        }
        Ok(())
    }
}

/// `notify.twilio`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ConfigTable)]
#[serde(default)]
pub struct TwilioOptions {
    /// The account SID (`AC...`).
    pub account_sid: String,
    /// The auth token, or the secret of `api_key`.
    #[serde(skip_serializing)]
    pub auth_token: Secret<String>,
    /// An API key SID (`SK...`) to authenticate with instead of the
    /// account SID (its secret goes in `auth_token`).
    #[config(public)]
    pub api_key: Option<String>,
    /// The API's base URL (default `https://api.twilio.com`).
    pub endpoint: Option<String>,
}

impl TwilioOptions {
    fn check(&self, channel: &str) -> Result<(), String> {
        if self.account_sid.is_empty() || self.auth_token.expose().is_empty() {
            return Err(format!(
                "notify.{channel}.driver `twilio` needs notify.twilio.account_sid and notify.twilio.auth_token"
            ));
        }
        Ok(())
    }
}

/// `notify.vonage`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ConfigTable)]
#[serde(default)]
pub struct VonageOptions {
    /// The API key, which names the account (the secret is `api_secret`).
    #[config(public)]
    pub api_key: String,
    /// The API secret.
    #[serde(skip_serializing)]
    pub api_secret: Secret<String>,
    /// The API's base URL (default `https://api.nexmo.com`).
    pub endpoint: Option<String>,
}

impl VonageOptions {
    fn check(&self) -> Result<(), String> {
        if self.api_key.is_empty() || self.api_secret.expose().is_empty() {
            return Err(
                "notify.sms.driver `vonage` needs notify.vonage.api_key and notify.vonage.api_secret".into(),
            );
        }
        Ok(())
    }
}

/// `notify.sns`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ConfigTable)]
#[serde(default)]
pub struct SnsOptions {
    /// The AWS region (`us-east-1`).
    pub region: String,
    /// The access key id.
    pub access_key_id: String,
    /// The secret access key.
    #[serde(skip_serializing)]
    pub secret_access_key: Secret<String>,
    /// A session token, for temporary credentials.
    #[serde(skip_serializing)]
    pub session_token: Option<Secret<String>>,
    /// `Transactional` (default) or `Promotional`.
    pub sms_type: String,
    /// The API's base URL (default `https://sns.{region}.amazonaws.com`).
    pub endpoint: Option<String>,
}

impl Default for SnsOptions {
    fn default() -> Self {
        Self {
            region: String::new(),
            access_key_id: String::new(),
            secret_access_key: Secret::default(),
            session_token: None,
            sms_type: "Transactional".to_owned(),
            endpoint: None,
        }
    }
}

impl SnsOptions {
    fn check(&self) -> Result<(), String> {
        if self.region.is_empty()
            || self.access_key_id.is_empty()
            || self.secret_access_key.expose().is_empty()
        {
            return Err("notify.sms.driver `sns` needs notify.sns.region, notify.sns.access_key_id and notify.sns.secret_access_key".into());
        }
        if !matches!(self.sms_type.as_str(), "Transactional" | "Promotional") {
            return Err("notify.sns.sms_type must be `Transactional` or `Promotional`".into());
        }
        Ok(())
    }
}

/// `notify.meta`: the WhatsApp Cloud API.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ConfigTable)]
#[serde(default)]
pub struct MetaOptions {
    /// The sending phone number's id.
    pub phone_number_id: String,
    /// A system user access token.
    #[serde(skip_serializing)]
    pub access_token: Secret<String>,
    /// The Graph API version (default `v21.0`).
    pub api_version: String,
    /// The Graph API's base URL (default `https://graph.facebook.com`).
    pub endpoint: Option<String>,
}

impl Default for MetaOptions {
    fn default() -> Self {
        Self {
            phone_number_id: String::new(),
            access_token: Secret::default(),
            api_version: "v21.0".to_owned(),
            endpoint: None,
        }
    }
}

impl MetaOptions {
    fn check(&self) -> Result<(), String> {
        if self.phone_number_id.is_empty() || self.access_token.expose().is_empty() {
            return Err("notify.whatsapp.driver `meta` needs notify.meta.phone_number_id and notify.meta.access_token".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use navree_config::{Config, ConfigSources, FieldType};

    use super::*;

    #[test]
    fn a_missing_section_uses_the_defaults() {
        let config = Config::load(&ConfigSources::new()).unwrap();
        assert_eq!(
            config.typed::<NotifyOptions>().unwrap(),
            NotifyOptions::default()
        );
        let options = NotifyOptions::default();
        assert!(options.routes);
        assert!(options.deliveries);
        assert_eq!(options.teams.tenant, "botframework.com");
        assert_eq!(options.web_push.ttl_secs, 86_400);
        assert_eq!(options.meta.api_version, "v21.0");
        assert_eq!(options.sns.sms_type, "Transactional");
    }

    #[test]
    fn environment_strings_set_the_flags() {
        let sources = ConfigSources::new().env_vars(
            "APP_",
            [
                ("APP_NOTIFY__QUEUE", "true"),
                ("APP_NOTIFY__ROUTES", "false"),
                ("APP_NOTIFY__DELIVERIES", "false"),
            ],
        );
        let options = Config::load(&sources)
            .unwrap()
            .typed::<NotifyOptions>()
            .unwrap();
        assert_eq!(
            options,
            NotifyOptions {
                queue: true,
                routes: false,
                deliveries: false,
                ..NotifyOptions::default()
            }
        );
    }

    #[test]
    fn drivers_and_credentials_come_from_the_environment() {
        let sources = ConfigSources::new().env_vars(
            "APP_",
            [
                ("APP_NOTIFY__SMS__DRIVER", "twilio"),
                ("APP_NOTIFY__SMS__FROM", "+15005550006"),
                ("APP_NOTIFY__SMS__DEFAULT_REGION", "US"),
                ("APP_NOTIFY__TWILIO__ACCOUNT_SID", "AC123"),
                ("APP_NOTIFY__TWILIO__AUTH_TOKEN", "secret-token"),
                ("APP_NOTIFY__WHATSAPP__DRIVER", "meta"),
                ("APP_NOTIFY__META__PHONE_NUMBER_ID", "1055"),
                ("APP_NOTIFY__META__ACCESS_TOKEN", "EAAG"),
            ],
        );
        let options = Config::load(&sources)
            .unwrap()
            .typed::<NotifyOptions>()
            .unwrap();
        assert_eq!(options.sms.driver, Some(SmsDriverKind::Twilio));
        assert_eq!(options.whatsapp.driver, Some(WhatsAppDriverKind::Meta));
        assert_eq!(options.twilio.auth_token.expose(), "secret-token");
        assert_eq!(options.sms.default_region.as_deref(), Some("US"));
    }

    #[test]
    fn a_driver_without_its_credentials_is_refused_naming_the_keys() {
        let options = NotifyOptions {
            sms: SmsOptions {
                driver: Some(SmsDriverKind::Twilio),
                ..SmsOptions::default()
            },
            ..NotifyOptions::default()
        };
        let error = options.validate().unwrap_err();
        assert!(error.contains("notify.twilio.account_sid"), "{error}");

        let options = NotifyOptions {
            whatsapp: WhatsAppOptions {
                driver: Some(WhatsAppDriverKind::Meta),
                ..WhatsAppOptions::default()
            },
            ..NotifyOptions::default()
        };
        assert!(options.validate().unwrap_err().contains("notify.meta"));

        let options = NotifyOptions {
            sms: SmsOptions {
                driver: Some(SmsDriverKind::Sns),
                ..SmsOptions::default()
            },
            ..NotifyOptions::default()
        };
        assert!(
            options
                .validate()
                .unwrap_err()
                .contains("notify.sns.region")
        );

        let options = NotifyOptions {
            sms: SmsOptions {
                driver: Some(SmsDriverKind::Vonage),
                ..SmsOptions::default()
            },
            ..NotifyOptions::default()
        };
        assert!(options.validate().unwrap_err().contains("notify.vonage"));
    }

    #[test]
    fn web_push_needs_both_halves_and_a_contact() {
        let mut options = NotifyOptions::default();
        options.web_push.public_key = Some("BPub".to_owned());
        assert!(options.validate().unwrap_err().contains("private_key"));
        options.web_push.private_key = Secret::new("priv".to_owned());
        assert!(options.validate().unwrap_err().contains("subject"));
        options.web_push.subject = "mailto:ops@acme.test".to_owned();
        assert!(options.validate().is_ok());
    }

    #[test]
    fn the_sns_driver_is_also_spelled_aws_sns() {
        let sources =
            ConfigSources::new().env_vars("APP_", [("APP_NOTIFY__SMS__DRIVER", "aws_sns")]);
        let options = Config::load(&sources).unwrap().typed::<NotifyOptions>();
        // The driver parses; its credentials are missing.
        let error = options.unwrap_err().to_string();
        assert!(error.contains("notify.sns.region"), "{error}");
    }

    #[test]
    fn endpoints_and_callbacks_must_be_http_urls() {
        let mut options = NotifyOptions::default();
        options.twilio.endpoint = Some("not a url".to_owned());
        assert!(
            options
                .validate()
                .unwrap_err()
                .contains("notify.twilio.endpoint")
        );
        options.twilio.endpoint = Some("http://127.0.0.1:9000".to_owned());
        options.sms.status_callback = Some("ftp://x.test/cb".to_owned());
        assert!(
            options
                .validate()
                .unwrap_err()
                .contains("notify.sms.status_callback")
        );
        options.sms.status_callback =
            Some("https://api.acme.test/api/webhooks/notify/twilio".to_owned());
        assert!(options.validate().is_ok());
    }

    #[test]
    fn an_unknown_default_region_is_refused() {
        let options = NotifyOptions {
            sms: SmsOptions {
                default_region: Some("XX".to_owned()),
                ..SmsOptions::default()
            },
            ..NotifyOptions::default()
        };
        assert!(options.validate().unwrap_err().contains("default_region"));
    }

    #[test]
    fn secrets_are_never_serialized() {
        let mut options = NotifyOptions::default();
        options.twilio.auth_token = Secret::new("tw-secret".to_owned());
        options.slack.bot_token = Secret::new("xoxb-secret".to_owned());
        let json = serde_json::to_string(&options).unwrap();
        assert!(!json.contains("tw-secret"));
        assert!(!json.contains("xoxb-secret"));
    }

    #[test]
    fn secrets_are_never_printed() {
        let sources = ConfigSources::new().env_vars(
            "APP_",
            [
                ("APP_NOTIFY__SLACK__BOT_TOKEN", "xoxb-bot-value"),
                ("APP_NOTIFY__TEAMS__APP_PASSWORD", "teams-password-value"),
                ("APP_NOTIFY__WEB_PUSH__PRIVATE_KEY", "vapid-private-value"),
                ("APP_NOTIFY__TWILIO__AUTH_TOKEN", "twilio-token-value"),
                ("APP_NOTIFY__VONAGE__API_SECRET", "vonage-secret-value"),
                ("APP_NOTIFY__SNS__SECRET_ACCESS_KEY", "sns-secret-value"),
                ("APP_NOTIFY__SNS__SESSION_TOKEN", "sns-session-value"),
                ("APP_NOTIFY__META__ACCESS_TOKEN", "meta-token-value"),
            ],
        );
        let options = Config::load(&sources)
            .unwrap()
            .typed::<NotifyOptions>()
            .unwrap();
        let text = format!("{options:?}");
        for value in [
            "xoxb-bot-value",
            "teams-password-value",
            "vapid-private-value",
            "twilio-token-value",
            "vonage-secret-value",
            "sns-secret-value",
            "sns-session-value",
            "meta-token-value",
        ] {
            assert!(!text.contains(value), "{text}");
        }
    }

    #[test]
    fn a_channel_list_names_only_known_channels() {
        let sources = ConfigSources::new()
            .env_vars("APP_", [("APP_NOTIFY__DEFERRED__CHANNELS", "sms, carrier")]);
        assert!(
            Config::load(&sources)
                .unwrap()
                .typed::<NotifyOptions>()
                .is_err()
        );
    }

    #[test]
    fn the_section_describes_itself() {
        let schema = NotifyOptions::schema().expect("schema");
        assert_eq!(schema.key, "notify");
        assert_eq!(
            schema.docs.as_deref(),
            Some("/docs/digging-deeper/notifications/")
        );
        assert!(
            schema.unmarked_secrets().is_empty(),
            "{:?}",
            schema.unmarked_secrets()
        );
        // Every credential is a secret, and nothing else is.
        let mut secrets = schema.secret_paths();
        secrets.sort();
        assert_eq!(
            secrets,
            [
                "notify.meta.access_token",
                "notify.slack.bot_token",
                "notify.sns.secret_access_key",
                "notify.sns.session_token",
                "notify.teams.app_password",
                "notify.twilio.auth_token",
                "notify.vonage.api_secret",
                "notify.web_push.private_key",
            ]
        );
        let table = |name: &str| {
            schema
                .fields
                .iter()
                .find(|field| field.name == name)
                .unwrap_or_else(|| panic!("no table {name}"))
        };
        assert_eq!(table("queue").ty, FieldType::Bool);
        for name in [
            "slack",
            "teams",
            "sms",
            "whatsapp",
            "web_push",
            "twilio",
            "vonage",
            "sns",
            "meta",
            "preferences",
            "digest",
            "deferred",
        ] {
            assert_eq!(table(name).ty, FieldType::Table, "{name}");
        }
        let field = |table_name: &str, name: &str| {
            table(table_name)
                .fields
                .iter()
                .find(|field| field.name == name)
                .unwrap_or_else(|| panic!("no field {table_name}.{name}"))
        };
        assert_eq!(
            field("sms", "driver").ty,
            FieldType::choice(&["twilio", "vonage", "sns", "log"])
        );
        assert_eq!(
            field("whatsapp", "driver").ty,
            FieldType::choice(&["twilio", "meta", "log"])
        );
        assert_eq!(
            field("preferences", "channels").ty,
            FieldType::list(FieldType::other("Channel"))
        );
        // Identifiers that are named like credentials are not secrets.
        for (table_name, name) in [
            ("twilio", "api_key"),
            ("vonage", "api_key"),
            ("teams", "token_url"),
        ] {
            let field = field(table_name, name);
            assert!(field.public && !field.secret, "{table_name}.{name}");
        }
        let defaults = NotifyOptions::defaults().expect("defaults");
        assert_eq!(defaults["queue"], false);
        assert_eq!(defaults["routes"], true);
        assert_eq!(defaults["teams"]["tenant"], "botframework.com");
        assert_eq!(defaults["web_push"]["ttl_secs"], 86_400);
        assert!(defaults["twilio"]["auth_token"].is_null());
        assert!(defaults["sms"]["driver"].is_null());
        assert_eq!(
            defaults["deferred"]["channels"],
            "mail, slack, teams, sms, whatsapp, web_push"
        );
    }
}
