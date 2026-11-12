//! Native app push devices: [`PushDevice`] rows, the [`PushDevices`]
//! service, and [`LiveSessions`], which tells the push channel which
//! signed-in sessions still own their devices.

use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use navree_contracts::{BoxError, BoxFuture, CredentialRevoker, RevokedByKind};
use navree_kernel::{Injectable, KernelError, ProviderKey, Resolver, WeakAppContext};
use navree_orm::{Model, Record};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::options::is_app_id;
use crate::store::{Exec, on};
use crate::{Notifiable, NotifyError, USER_NOTIFIABLE_TYPE};

/// The longest token accepted (Firebase tokens run to a few hundred
/// characters; Apple's are 64 hex digits today).
const MAX_TOKEN: usize = 4096;

/// The system a native app runs on, which says which service reaches it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PushPlatform {
    /// iOS and iPadOS (and every Apple system): Apple's push service.
    Ios,
    /// Android: Firebase Cloud Messaging.
    Android,
}

impl PushPlatform {
    /// `ios` or `android`.
    pub fn as_str(self) -> &'static str {
        match self {
            PushPlatform::Ios => "ios",
            PushPlatform::Android => "android",
        }
    }
}

impl fmt::Display for PushPlatform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for PushPlatform {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ios" => Ok(PushPlatform::Ios),
            "android" => Ok(PushPlatform::Android),
            other => Err(format!("unknown push platform `{other}`")),
        }
    }
}

/// Which of Apple's environments issued a device token: a development
/// build's tokens work only in the sandbox. Android devices are always
/// `production`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PushEnvironment {
    /// Apple's sandbox (development builds).
    Sandbox,
    /// Production (App Store, TestFlight and every Android build).
    #[default]
    Production,
}

impl PushEnvironment {
    /// `sandbox` or `production`.
    pub fn as_str(self) -> &'static str {
        match self {
            PushEnvironment::Sandbox => "sandbox",
            PushEnvironment::Production => "production",
        }
    }
}

impl FromStr for PushEnvironment {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "sandbox" => Ok(PushEnvironment::Sandbox),
            "production" => Ok(PushEnvironment::Production),
            other => Err(format!("unknown push environment `{other}`")),
        }
    }
}

/// A row of `push_devices`: one install of a native app that receives the
/// push notifications of a notifiable, for as long as the signed-in session
/// that registered it lives. The token is never serialized.
#[derive(Model, Clone, Default, PartialEq, Eq, Serialize)]
#[model(table = "push_devices")]
pub struct PushDevice {
    /// The id (a UUID v7).
    pub id: Uuid,
    /// The notifiable's type (`user`).
    pub notifiable_type: String,
    /// The notifiable's id.
    pub notifiable_id: String,
    /// The signed-in session that registered it: ending the session ends
    /// the device.
    #[serde(skip_serializing)]
    pub session_id: String,
    /// `ios` or `android` ([`PushPlatform`]).
    pub platform: String,
    /// The device token Apple or Firebase gave the app.
    #[serde(skip_serializing)]
    #[model(hidden)]
    pub token: String,
    /// The SHA-256 of `token` (hex): one row per token.
    #[serde(skip_serializing)]
    pub token_hash: String,
    /// The app's bundle id (iOS) or package name (Android).
    pub app_id: String,
    /// `sandbox` or `production` ([`PushEnvironment`]).
    pub environment: String,
    /// When it first registered.
    pub created_at: DateTime<Utc>,
    /// When the app last registered it (each launch does).
    pub last_seen_at: DateTime<Utc>,
    /// When a notification was last pushed to it.
    pub last_used_at: Option<DateTime<Utc>>,
}

impl PushDevice {
    /// Where the driver sends; `None` for a row whose platform or
    /// environment this version does not know.
    pub fn target(&self) -> Option<DeviceTarget> {
        Some(DeviceTarget {
            platform: self.platform.parse().ok()?,
            token: self.token.clone(),
            app_id: self.app_id.clone(),
            environment: self.environment.parse().ok()?,
        })
    }
}

/// The token and session are redacted.
impl fmt::Debug for PushDevice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PushDevice")
            .field("id", &self.id)
            .field("notifiable_type", &self.notifiable_type)
            .field("notifiable_id", &self.notifiable_id)
            .field("platform", &self.platform)
            .field("app_id", &self.app_id)
            .field("environment", &self.environment)
            .field("created_at", &self.created_at)
            .field("last_seen_at", &self.last_seen_at)
            .field("last_used_at", &self.last_used_at)
            .finish_non_exhaustive()
    }
}

/// One device, as a push driver needs it.
#[derive(Clone, PartialEq, Eq)]
pub struct DeviceTarget {
    /// Which service reaches it.
    pub platform: PushPlatform,
    /// The device token.
    pub token: String,
    /// The app's bundle id (Apple's topic) or package name.
    pub app_id: String,
    /// Apple's sandbox or production.
    pub environment: PushEnvironment,
}

impl DeviceTarget {
    /// A production device.
    pub fn new(
        platform: PushPlatform,
        token: impl Into<String>,
        app_id: impl Into<String>,
    ) -> Self {
        Self {
            platform,
            token: token.into(),
            app_id: app_id.into(),
            environment: PushEnvironment::Production,
        }
    }

    /// In `environment`.
    #[must_use]
    pub fn environment(mut self, environment: PushEnvironment) -> Self {
        self.environment = environment;
        self
    }
}

/// The token is redacted.
impl fmt::Debug for DeviceTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceTarget")
            .field("platform", &self.platform)
            .field("app_id", &self.app_id)
            .field("environment", &self.environment)
            .finish_non_exhaustive()
    }
}

/// A device as the native app registers it (`POST
/// /api/notifications/push-devices`).
///
/// ```
/// use navree_notify::NewPushDevice;
///
/// let device: NewPushDevice = serde_json::from_str(r#"{
///     "platform": "ios",
///     "token": "740f4707bebcf74f9b7c25d48e3358945f6aa01da5ddb387462c7eaf61bb78ad",
///     "app_id": "com.acme.crm",
///     "environment": "sandbox"
/// }"#).unwrap();
/// assert!(device.check().is_ok());
/// ```
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewPushDevice {
    /// `ios` or `android`.
    pub platform: PushPlatform,
    /// The token the system gave the app (Apple's device token as hex,
    /// Firebase's registration token).
    pub token: String,
    /// The app's bundle id or package name.
    pub app_id: String,
    /// `sandbox` for a development build on iOS; default `production`.
    #[serde(default)]
    pub environment: PushEnvironment,
}

/// The token is redacted.
impl fmt::Debug for NewPushDevice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NewPushDevice")
            .field("platform", &self.platform)
            .field("app_id", &self.app_id)
            .field("environment", &self.environment)
            .finish_non_exhaustive()
    }
}

impl NewPushDevice {
    /// Checks the device: Apple's token is hexadecimal (16 to 200 digits),
    /// Firebase's is 16 to 4096 characters of `A-Z a-z 0-9 _ - : .`; the
    /// app id is a bundle id or package name; only iOS has a sandbox. The
    /// error names the field.
    pub fn check(&self) -> Result<(), (&'static str, String)> {
        let token = self.token.trim();
        let token_ok = match self.platform {
            PushPlatform::Ios => {
                (16..=200).contains(&token.len())
                    && token.len().is_multiple_of(2)
                    && token.chars().all(|c| c.is_ascii_hexdigit())
            }
            PushPlatform::Android => {
                (16..=MAX_TOKEN).contains(&token.len())
                    && token
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ':' | '.'))
            }
        };
        if !token_ok {
            return Err((
                "token",
                format!(
                    "token must be the device token {} gave the app",
                    match self.platform {
                        PushPlatform::Ios => "Apple",
                        PushPlatform::Android => "Firebase",
                    }
                ),
            ));
        }
        if !is_app_id(self.app_id.trim()) {
            return Err((
                "app_id",
                "app_id must be the app's bundle id or package name".to_owned(),
            ));
        }
        if self.platform == PushPlatform::Android && self.environment == PushEnvironment::Sandbox {
            return Err((
                "environment",
                "only iOS devices have a sandbox environment".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Which signed-in sessions are still live. The push channel asks the
/// app's `dyn LiveSessions` before it sends: a device whose session has
/// ended (signed out, ended from another device, revoked by a password
/// change or offboarding) gets nothing and is deleted. Without one, every
/// device is sent to (logged once), so an app that signs people in
/// registers one answering from its sessions: with navree-auth, the ids
/// `AuthService::sessions` lists.
///
/// ```
/// use std::collections::HashSet;
/// use std::sync::{Arc, Mutex};
///
/// use navree_contracts::{BoxError, BoxFuture};
/// use navree_notify::LiveSessions;
/// use uuid::Uuid;
///
/// /// The sessions the app's sign-in keeps (here, in memory).
/// struct SignedIn(Arc<Mutex<HashSet<String>>>);
///
/// impl LiveSessions for SignedIn {
///     fn live<'a>(
///         &'a self,
///         _user_id: Uuid,
///         sessions: &'a [String],
///     ) -> BoxFuture<'a, Result<Vec<String>, BoxError>> {
///         let open = self.0.lock().unwrap().clone();
///         let live = sessions.iter().filter(|s| open.contains(*s)).cloned().collect();
///         Box::pin(async move { Ok(live) })
///     }
/// }
/// // Registered in a module with
/// // `m.value_arc::<dyn LiveSessions>(Arc::new(SignedIn(..)))`.
/// ```
pub trait LiveSessions: Send + Sync + 'static {
    /// The ones of `sessions` (session ids of user `user_id`) that are
    /// still live.
    fn live<'a>(
        &'a self,
        user_id: Uuid,
        sessions: &'a [String],
    ) -> BoxFuture<'a, Result<Vec<String>, BoxError>>;
}

/// The apps devices may register for, by platform; an empty list takes
/// any app id.
#[derive(Clone, Debug, Default)]
pub(crate) struct RegisteredApps {
    pub(crate) ios: Vec<String>,
    pub(crate) android: Vec<String>,
}

impl RegisteredApps {
    fn allows(&self, platform: PushPlatform, app_id: &str) -> bool {
        let apps = match platform {
            PushPlatform::Ios => &self.ios,
            PushPlatform::Android => &self.android,
        };
        apps.is_empty() || apps.iter().any(|app| app == app_id)
    }
}

/// The push devices of notifiables: the push channel sends to each device
/// of the recipient whose session is live, and deletes the ones Apple or
/// Firebase report gone.
///
/// The signed-in user registers and removes their own with `POST` and
/// `DELETE /api/notifications/push-devices`. Provided by
/// [`NotifyModule`](crate::NotifyModule): inject `Arc<PushDevices>`. Needs
/// `DatabaseModule`; inside an unfinished unit of work its writes join the
/// transaction.
pub struct PushDevices {
    app: WeakAppContext,
    apps: RegisteredApps,
}

impl PushDevices {
    pub(crate) fn new(app: WeakAppContext, apps: RegisteredApps) -> Self {
        Self { app, apps }
    }

    /// Saves `device` for `notifiable`, owned by the signed-in session
    /// `session_id` ([`check`](NewPushDevice::check)ed first, and its app
    /// id one of `notify.push`'s apps). A token already saved moves to
    /// `notifiable` and this session: a device belongs to whoever signed in
    /// on it last. A session keeps one device per app, so a token the
    /// system replaced is deleted.
    pub async fn register(
        &self,
        notifiable: &dyn Notifiable,
        session_id: &str,
        device: NewPushDevice,
    ) -> Result<PushDevice, NotifyError> {
        device
            .check()
            .map_err(|(_, message)| NotifyError::InvalidRecipient(message))?;
        let app_id = device.app_id.trim().to_owned();
        if !self.allows(device.platform, &app_id) {
            return Err(NotifyError::InvalidRecipient(not_an_app()));
        }
        if session_id.trim().is_empty() {
            return Err(NotifyError::InvalidRecipient(
                "a push device needs a signed-in session".to_owned(),
            ));
        }
        let token = device.token.trim().to_owned();
        let hash = navree_crypto::sha256_hex(token.as_bytes());
        let now = crate::notifier::now_in(&self.app).await;
        let exec = Exec::current(&self.app).await?;
        // The session's earlier token for this app, if the system gave it a
        // new one.
        on!(&exec, e => PushDevice::query()
            .where_(PushDevice::SESSION_ID.eq(session_id))
            .where_(PushDevice::PLATFORM.eq(device.platform.as_str()))
            .where_(PushDevice::APP_ID.eq(app_id.as_str()))
            .where_(PushDevice::TOKEN_HASH.ne(hash.as_str()))
            .delete(e)
            .await)?;
        let existing = on!(&exec, e => PushDevice::query()
            .where_(PushDevice::TOKEN_HASH.eq(hash.as_str()))
            .first(e)
            .await)?;
        let mut record = existing.unwrap_or_else(|| {
            Record::new(PushDevice {
                id: Uuid::now_v7(),
                token: token.clone(),
                token_hash: hash.clone(),
                created_at: now,
                ..PushDevice::default()
            })
        });
        record.notifiable_type = notifiable.notifiable_type().to_owned();
        record.notifiable_id = notifiable.notifiable_id();
        record.session_id = session_id.to_owned();
        record.platform = device.platform.as_str().to_owned();
        record.app_id = app_id;
        record.environment = device.environment.as_str().to_owned();
        record.last_seen_at = now;
        on!(&exec, e => record.save(e).await)?;
        exec.wrote();
        Ok(record.into_inner())
    }

    /// Whether a device of `platform` may register for `app_id`.
    pub(crate) fn allows(&self, platform: PushPlatform, app_id: &str) -> bool {
        self.apps.allows(platform, app_id)
    }

    /// Deletes `notifiable`'s device with `token`; `false` when it has none
    /// with it.
    pub async fn unregister(
        &self,
        notifiable: &dyn Notifiable,
        token: &str,
    ) -> Result<bool, NotifyError> {
        let hash = navree_crypto::sha256_hex(token.trim().as_bytes());
        let exec = Exec::current(&self.app).await?;
        let deleted = on!(&exec, e => PushDevice::query()
            .where_(PushDevice::TOKEN_HASH.eq(hash.as_str()))
            .where_(PushDevice::NOTIFIABLE_TYPE.eq(notifiable.notifiable_type()))
            .where_(PushDevice::NOTIFIABLE_ID.eq(notifiable.notifiable_id()))
            .delete(e)
            .await)?;
        exec.wrote();
        Ok(deleted > 0)
    }

    /// Every device of the notifiable `notifiable_type` / `notifiable_id`,
    /// oldest first.
    pub async fn of(
        &self,
        notifiable_type: &str,
        notifiable_id: &str,
    ) -> Result<Vec<PushDevice>, NotifyError> {
        let exec = Exec::current(&self.app).await?;
        let rows = on!(&exec, e => PushDevice::query()
            .where_(PushDevice::NOTIFIABLE_TYPE.eq(notifiable_type))
            .where_(PushDevice::NOTIFIABLE_ID.eq(notifiable_id))
            .oldest_by(PushDevice::CREATED_AT)
            .get(e)
            .await)?;
        Ok(rows.into_iter().map(Record::into_inner).collect())
    }

    /// Deletes device `id`; `false` when it does not exist.
    pub async fn delete(&self, id: Uuid) -> Result<bool, NotifyError> {
        let exec = Exec::current(&self.app).await?;
        let deleted = on!(&exec, e => PushDevice::query()
            .where_(PushDevice::ID.eq(id))
            .delete(e)
            .await)?;
        exec.wrote();
        Ok(deleted > 0)
    }

    /// Deletes the devices the signed-in session `session_id` registered:
    /// what signing out, or ending the session from elsewhere, calls. How
    /// many were deleted.
    pub async fn end_session(&self, session_id: &str) -> Result<u64, NotifyError> {
        let exec = Exec::current(&self.app).await?;
        let deleted = on!(&exec, e => PushDevice::query()
            .where_(PushDevice::SESSION_ID.eq(session_id))
            .delete(e)
            .await)?;
        exec.wrote();
        Ok(deleted)
    }

    /// Deletes every device of the notifiables `notifiable_type` / each of
    /// `notifiable_ids` (offboarding). How many were deleted.
    pub async fn end_all(
        &self,
        notifiable_type: &str,
        notifiable_ids: &[String],
    ) -> Result<u64, NotifyError> {
        if notifiable_ids.is_empty() {
            return Ok(0);
        }
        let exec = Exec::current(&self.app).await?;
        let deleted = on!(&exec, e => PushDevice::query()
            .where_(PushDevice::NOTIFIABLE_TYPE.eq(notifiable_type))
            .where_(PushDevice::NOTIFIABLE_ID.is_in(notifiable_ids.iter().map(String::as_str)))
            .delete(e)
            .await)?;
        exec.wrote();
        Ok(deleted)
    }

    /// Notes a push to device `id` at `at` (pool, outside any unit of
    /// work).
    pub(crate) async fn touch(&self, id: Uuid, at: DateTime<Utc>) -> Result<(), NotifyError> {
        let exec = Exec::pool(&self.app).await?;
        on!(&exec, e => PushDevice::query()
            .where_(PushDevice::ID.eq(id))
            .update(e, [PushDevice::LAST_USED_AT.set(Some(at))])
            .await)?;
        Ok(())
    }

    /// [`delete`](PushDevices::delete) on the pool, outside any unit of
    /// work (the channel runs after commit).
    pub(crate) async fn forget(&self, id: Uuid) -> Result<(), NotifyError> {
        let exec = Exec::pool(&self.app).await?;
        on!(&exec, e => PushDevice::query()
            .where_(PushDevice::ID.eq(id))
            .delete(e)
            .await)?;
        Ok(())
    }
}

impl fmt::Debug for PushDevices {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PushDevices").finish_non_exhaustive()
    }
}

/// The message of an app id that is not one of `notify.push`'s apps.
pub(crate) fn not_an_app() -> String {
    "app_id is not an app this server sends notifications to".to_owned()
}

/// Ends the push devices of offboarded users: registered as a
/// `dyn CredentialRevoker`, asked by navree-auth when it revokes every
/// credential of a user.
pub(crate) struct DeviceRevoker {
    devices: std::sync::Arc<PushDevices>,
}

impl Injectable for DeviceRevoker {
    fn dependencies() -> Vec<ProviderKey> {
        vec![ProviderKey::of::<PushDevices>()]
    }

    fn construct(r: Resolver) -> navree_kernel::BoxFuture<'static, Result<Self, KernelError>> {
        Box::pin(async move {
            Ok(DeviceRevoker {
                devices: r.get::<PushDevices>().await?,
            })
        })
    }
}

impl CredentialRevoker for DeviceRevoker {
    fn name(&self) -> &'static str {
        "notify"
    }

    fn revoke_for_users<'a>(
        &'a self,
        users: &'a [Uuid],
    ) -> BoxFuture<'a, Result<RevokedByKind, BoxError>> {
        Box::pin(async move {
            let ids: Vec<String> = users.iter().map(Uuid::to_string).collect();
            let deleted = self.devices.end_all(USER_NOTIFIABLE_TYPE, &ids).await?;
            let mut revoked = RevokedByKind::new();
            if deleted > 0 {
                revoked.insert("push_devices", deleted);
            }
            Ok(revoked)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const APPLE_TOKEN: &str = "740f4707bebcf74f9b7c25d48e3358945f6aa01da5ddb387462c7eaf61bb78ad";
    const FIREBASE_TOKEN: &str = "dQw4w9WgXcQ:APA91bHun4MxP5egoKMwt2KZFBaFUH-1RYqx";

    fn device(platform: PushPlatform, token: &str, app_id: &str) -> NewPushDevice {
        NewPushDevice {
            platform,
            token: token.to_owned(),
            app_id: app_id.to_owned(),
            environment: PushEnvironment::Production,
        }
    }

    #[test]
    fn devices_need_a_token_of_their_platform_and_an_app_id() {
        assert!(
            device(PushPlatform::Ios, APPLE_TOKEN, "com.acme.crm")
                .check()
                .is_ok()
        );
        assert!(
            device(PushPlatform::Android, FIREBASE_TOKEN, "com.acme.crm")
                .check()
                .is_ok()
        );
        for (platform, token) in [
            (PushPlatform::Ios, FIREBASE_TOKEN),
            (PushPlatform::Ios, "abc"),
            (PushPlatform::Ios, "740f4707bebcf74f9b7c25d48e335894f"),
            (PushPlatform::Android, "short"),
            (PushPlatform::Android, "has spaces in it and more"),
            (PushPlatform::Android, "<script>alert(1)</script>"),
        ] {
            assert_eq!(
                device(platform, token, "com.acme.crm")
                    .check()
                    .unwrap_err()
                    .0,
                "token",
                "{token}"
            );
        }
        for app in ["", "com..acme", "com/acme", "a b"] {
            assert_eq!(
                device(PushPlatform::Ios, APPLE_TOKEN, app)
                    .check()
                    .unwrap_err()
                    .0,
                "app_id",
                "{app}"
            );
        }
        let mut sandboxed = device(PushPlatform::Android, FIREBASE_TOKEN, "com.acme.crm");
        sandboxed.environment = PushEnvironment::Sandbox;
        assert_eq!(sandboxed.check().unwrap_err().0, "environment");
    }

    #[test]
    fn tokens_never_show_in_debug_output() {
        let new = device(PushPlatform::Ios, APPLE_TOKEN, "com.acme.crm");
        assert!(!format!("{new:?}").contains(APPLE_TOKEN));
        let row = PushDevice {
            token: APPLE_TOKEN.to_owned(),
            session_id: "session-value".to_owned(),
            platform: "ios".to_owned(),
            environment: "production".to_owned(),
            ..PushDevice::default()
        };
        let printed = format!("{row:?}");
        assert!(!printed.contains(APPLE_TOKEN) && !printed.contains("session-value"));
        let target = row.target().unwrap();
        assert!(!format!("{target:?}").contains(APPLE_TOKEN));
        let json = serde_json::to_string(&row).unwrap();
        assert!(
            !json.contains(APPLE_TOKEN) && !json.contains("session-value"),
            "{json}"
        );
    }

    #[test]
    fn registered_apps_limit_app_ids_when_listed() {
        let apps = RegisteredApps {
            ios: vec!["com.acme.crm".to_owned()],
            android: Vec::new(),
        };
        assert!(apps.allows(PushPlatform::Ios, "com.acme.crm"));
        assert!(!apps.allows(PushPlatform::Ios, "com.evil.app"));
        assert!(apps.allows(PushPlatform::Android, "com.anything"));
    }
}
