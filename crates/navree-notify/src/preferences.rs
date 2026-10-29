//! Notification preferences: what a person, or a workspace for its
//! members, chooses to receive per category and channel; the
//! [`NotificationPreference`] rows and the [`NotificationPreferences`]
//! service.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use chrono::{DateTime, Utc};
use navree_kernel::WeakAppContext;
use navree_orm::{Model, Record};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::store::{Exec, on};
use crate::{Channel, Digest, Notifiable, NotifyError, PreferenceOptions, QuietHours};

/// The owner type of a workspace's preferences: the defaults of its
/// members.
pub const WORKSPACE_OWNER_TYPE: &str = "workspace";

/// The longest category name.
pub const MAX_CATEGORY_LEN: usize = 100;

/// The most categories one owner's preferences name.
pub const MAX_CATEGORIES: usize = 200;

/// What one owner (a person, or a workspace for its members) chooses to
/// receive. Every field is optional: what an owner leaves out comes from
/// the level below (a person's workspace, then the notification's own
/// defaults).
///
/// ```
/// use navree_notify::{Channel, Digest, Preferences};
/// use serde_json::json;
///
/// let preferences: Preferences = serde_json::from_value(json!({
///     "channels": { "sms": false },
///     "categories": {
///         "deals": { "channels": { "mail": false }, "digest": "daily" }
///     },
///     "quiet_hours": { "start": "22:00", "end": "07:00" }
/// })).unwrap();
/// assert_eq!(preferences.channel("deals", Channel::Mail), Some(false));
/// assert_eq!(preferences.channel("deals", Channel::Sms), Some(false));
/// assert_eq!(preferences.channel("billing", Channel::Mail), None);
/// assert_eq!(preferences.digest("deals"), Some(Digest::Daily));
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Preferences {
    /// Channel switches for every category.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub channels: BTreeMap<Channel, bool>,
    /// Per category: channel switches and a digest.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub categories: BTreeMap<String, CategoryPreferences>,
    /// The digest of every category's mail.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub digest: Option<Digest>,
    /// When nothing that interrupts should arrive, on the owner's clock.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quiet_hours: Option<QuietHours>,
}

/// The choices of one category.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CategoryPreferences {
    /// Channel switches.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub channels: BTreeMap<Channel, bool>,
    /// The digest of its mail.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub digest: Option<Digest>,
}

impl CategoryPreferences {
    fn is_empty(&self) -> bool {
        self.channels.is_empty() && self.digest.is_none()
    }
}

impl Preferences {
    /// Whether nothing is chosen.
    pub fn is_empty(&self) -> bool {
        self.channels.is_empty()
            && self.categories.values().all(CategoryPreferences::is_empty)
            && self.digest.is_none()
            && self.quiet_hours.is_none()
    }

    /// This owner's switch for `channel` in `category`: the category's,
    /// else the one for every category; `None` when neither is set. The
    /// broadcast channel follows the database channel.
    pub fn channel(&self, category: &str, channel: Channel) -> Option<bool> {
        let channel = preference_channel(channel);
        self.categories
            .get(category)
            .and_then(|category| category.channels.get(&channel))
            .or_else(|| self.channels.get(&channel))
            .copied()
    }

    /// This owner's digest for `category`: the category's, else the one
    /// for every category.
    pub fn digest(&self, category: &str) -> Option<Digest> {
        self.categories
            .get(category)
            .and_then(|category| category.digest)
            .or(self.digest)
    }

    /// These preferences with `patch` applied as a JSON merge patch
    /// (RFC 7386): objects merge, `null` removes a choice (it is inherited
    /// again), other values replace. The result is checked like
    /// [`check`](Preferences::check); errors name the field.
    pub fn merged(
        &self,
        patch: &Value,
        options: &PreferenceOptions,
        categories: &BTreeSet<String>,
    ) -> Result<Preferences, BTreeMap<String, Vec<String>>> {
        if !patch.is_object() {
            return Err(field_error(
                "preferences",
                "the preferences must be a JSON object",
            ));
        }
        let mut merged = serde_json::to_value(self).unwrap_or_else(|_| Value::Object(Map::new()));
        merge_patch(&mut merged, patch);
        let mut preferences: Preferences = serde_json::from_value(merged)
            .map_err(|error| field_error("preferences", &error.to_string()))?;
        preferences.compact();
        preferences.check(options, categories)?;
        Ok(preferences)
    }

    /// Checks the choices: category names (one of `categories` when that
    /// is not empty), channels people may switch
    /// (`notify.preferences.channels`), and no channel of a required
    /// category turned off. Errors name the field
    /// (`categories.deals.channels.sms`).
    pub fn check(
        &self,
        options: &PreferenceOptions,
        categories: &BTreeSet<String>,
    ) -> Result<(), BTreeMap<String, Vec<String>>> {
        let mut errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
        if self.categories.len() > MAX_CATEGORIES {
            errors.insert(
                "categories".to_owned(),
                vec![format!(
                    "at most {MAX_CATEGORIES} categories can have choices of their own"
                )],
            );
        }
        let mut channel_errors = |prefix: &str, channels: &BTreeMap<Channel, bool>| {
            for channel in channels.keys() {
                if !options.channels.contains(channel) {
                    errors
                        .entry(format!("{prefix}channels.{channel}"))
                        .or_default()
                        .push(format!("the {channel} channel cannot be switched"));
                }
            }
        };
        channel_errors("", &self.channels);
        let mut checked = Vec::new();
        for (category, choices) in &self.categories {
            let prefix = format!("categories.{category}.");
            channel_errors(&prefix, &choices.channels);
            checked.push((category, prefix, choices));
        }
        for (category, prefix, choices) in checked {
            if let Err(message) = check_category(category) {
                errors
                    .entry(format!("categories.{category}"))
                    .or_default()
                    .push(message);
            } else if !categories.is_empty() && !categories.contains(category) {
                errors
                    .entry(format!("categories.{category}"))
                    .or_default()
                    .push(format!("`{category}` is not a notification category"));
            }
            if options.is_required(category) {
                for (channel, on) in &choices.channels {
                    if !on {
                        errors
                            .entry(format!("{prefix}channels.{channel}"))
                            .or_default()
                            .push(format!("{category} notifications cannot be turned off"));
                    }
                }
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Drops categories with no choices left.
    fn compact(&mut self) {
        self.categories.retain(|_, choices| !choices.is_empty());
    }
}

/// The channel whose switch applies to `channel`: broadcast follows
/// database.
fn preference_channel(channel: Channel) -> Channel {
    match channel {
        Channel::Broadcast => Channel::Database,
        other => other,
    }
}

/// Checks a category name: 1 to [`MAX_CATEGORY_LEN`] ASCII letters,
/// digits, `_`, `-`, `.` or `:`.
pub fn check_category(category: &str) -> Result<(), String> {
    let valid = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':');
    if category.is_empty() || category.len() > MAX_CATEGORY_LEN || !category.chars().all(valid) {
        return Err(format!(
            "`{category}` is not a category name (1 to {MAX_CATEGORY_LEN} letters, digits, `_`, `-`, `.` or `:`)"
        ));
    }
    Ok(())
}

/// RFC 7386: `patch` merged into `target`.
fn merge_patch(target: &mut Value, patch: &Value) {
    let Value::Object(patch) = patch else {
        *target = patch.clone();
        return;
    };
    if !target.is_object() {
        *target = Value::Object(Map::new());
    }
    let Value::Object(target) = target else {
        return;
    };
    for (key, value) in patch {
        if value.is_null() {
            target.remove(key);
        } else {
            merge_patch(target.entry(key.clone()).or_insert(Value::Null), value);
        }
    }
}

fn field_error(field: &str, message: &str) -> BTreeMap<String, Vec<String>> {
    BTreeMap::from([(field.to_owned(), vec![message.to_owned()])])
}

/// Whose preferences: a notifiable (`user` and the user's id) or a
/// workspace (the defaults of its members).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PreferenceOwner {
    /// [`Notifiable::notifiable_type`], or [`WORKSPACE_OWNER_TYPE`].
    pub owner_type: String,
    /// [`Notifiable::notifiable_id`], or the workspace id.
    pub owner_id: String,
}

impl PreferenceOwner {
    /// The preferences of `notifiable`.
    pub fn of(notifiable: &dyn Notifiable) -> PreferenceOwner {
        PreferenceOwner {
            owner_type: notifiable.notifiable_type().to_owned(),
            owner_id: notifiable.notifiable_id(),
        }
    }

    /// The preferences of user `id` (notifiable type
    /// [`USER_NOTIFIABLE_TYPE`](crate::USER_NOTIFIABLE_TYPE)).
    pub fn user(id: Uuid) -> PreferenceOwner {
        PreferenceOwner {
            owner_type: crate::USER_NOTIFIABLE_TYPE.to_owned(),
            owner_id: id.to_string(),
        }
    }

    /// The defaults of workspace `id`'s members.
    pub fn workspace(id: Uuid) -> PreferenceOwner {
        PreferenceOwner {
            owner_type: WORKSPACE_OWNER_TYPE.to_owned(),
            owner_id: id.to_string(),
        }
    }

    /// Whether this is a workspace.
    pub fn is_workspace(&self) -> bool {
        self.owner_type == WORKSPACE_OWNER_TYPE
    }
}

impl fmt::Display for PreferenceOwner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.owner_type, self.owner_id)
    }
}

/// A row of `notification_preferences`: one owner's [`Preferences`] as
/// JSON. Unique per owner.
#[derive(Model, Clone, Debug, Default, PartialEq, Serialize)]
#[model(table = "notification_preferences")]
pub struct NotificationPreference {
    /// The id (a UUID v7).
    pub id: Uuid,
    /// [`PreferenceOwner::owner_type`].
    pub owner_type: String,
    /// [`PreferenceOwner::owner_id`].
    pub owner_id: String,
    /// The [`Preferences`], as JSON.
    pub preferences: Value,
    /// When the owner first chose.
    pub created_at: DateTime<Utc>,
    /// When the owner last changed a choice.
    pub updated_at: DateTime<Utc>,
}

impl NotificationPreference {
    /// The stored preferences; a row that no longer reads (written by
    /// hand) counts as none.
    pub fn read(&self) -> Preferences {
        serde_json::from_value(self.preferences.clone()).unwrap_or_else(|error| {
            tracing::warn!(
                target: "navree::notify",
                owner = %format!("{}:{}", self.owner_type, self.owner_id),
                %error,
                "ignoring stored notification preferences that do not read"
            );
            Preferences::default()
        })
    }
}

/// The preferences that decide one delivery: the recipient's own over
/// their workspace's, over the notification's defaults. Required
/// categories (`notify.preferences.required`) always deliver.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResolvedPreferences {
    /// The recipient's own.
    pub own: Preferences,
    /// Their workspace's, if any.
    pub workspace: Option<Preferences>,
    /// The required categories.
    pub required: Vec<String>,
}

impl ResolvedPreferences {
    /// Whether `category` notifications go out over `channel`: the
    /// recipient's switch, else the workspace's, else on.
    pub fn allows(&self, category: &str, channel: Channel) -> bool {
        if self.required.iter().any(|required| required == category) {
            return true;
        }
        self.own
            .channel(category, channel)
            .or_else(|| {
                self.workspace
                    .as_ref()
                    .and_then(|workspace| workspace.channel(category, channel))
            })
            .unwrap_or(true)
    }

    /// The digest of `category`'s mail: the recipient's, else the
    /// workspace's, else `default` (the notification's).
    pub fn digest(&self, category: &str, default: Digest) -> Digest {
        self.own
            .digest(category)
            .or_else(|| {
                self.workspace
                    .as_ref()
                    .and_then(|workspace| workspace.digest(category))
            })
            .unwrap_or(default)
    }

    /// The quiet hours: the recipient's, else the workspace's.
    pub fn quiet_hours(&self) -> Option<QuietHours> {
        self.own
            .quiet_hours
            .or_else(|| self.workspace.as_ref().and_then(|w| w.quiet_hours))
            .filter(|quiet| !quiet.is_empty())
    }
}

/// Stored notification preferences: read, change and resolve the choices
/// of people and workspaces. The [`Notifier`](crate::Notifier) applies
/// them to every notification it sends (with `notify.preferences.enabled`
/// and `DatabaseModule`); the `/api/notifications/preferences` routes
/// serve them to the signed-in user.
///
/// Provided by [`NotifyModule`](crate::NotifyModule): inject
/// `Arc<NotificationPreferences>`. Reads and writes join the current unit
/// of work when there is one.
pub struct NotificationPreferences {
    app: WeakAppContext,
    options: PreferenceOptions,
}

impl NotificationPreferences {
    pub(crate) fn new(app: WeakAppContext, options: PreferenceOptions) -> Self {
        Self { app, options }
    }

    /// The options (`notify.preferences`).
    pub fn options(&self) -> &PreferenceOptions {
        &self.options
    }

    /// `owner`'s preferences (empty when none are stored).
    pub async fn of(&self, owner: &PreferenceOwner) -> Result<Preferences, NotifyError> {
        let exec = Exec::current(&self.app).await?;
        let row = on!(&exec, e => NotificationPreference::query()
            .where_(NotificationPreference::OWNER_TYPE.eq(owner.owner_type.as_str()))
            .where_(NotificationPreference::OWNER_ID.eq(owner.owner_id.as_str()))
            .first(e)
            .await)?;
        Ok(row.map(|row| row.into_inner().read()).unwrap_or_default())
    }

    /// Applies `patch` to `owner`'s preferences as a JSON merge patch
    /// (`{"categories": {"deals": {"channels": {"sms": false}}}}`; `null`
    /// removes a choice) and stores the result, which it returns.
    /// [`NotifyError::InvalidPreferences`] names the fields it refuses
    /// (see [`Preferences::check`]).
    pub async fn update(
        &self,
        owner: &PreferenceOwner,
        patch: &Value,
    ) -> Result<Preferences, NotifyError> {
        let current = self.of(owner).await?;
        let categories = self.known_categories().await;
        let preferences = current
            .merged(patch, &self.options, &categories)
            .map_err(NotifyError::InvalidPreferences)?;
        self.store(owner, &preferences).await?;
        Ok(preferences)
    }

    /// Replaces `owner`'s preferences with `preferences` (checked like
    /// [`update`](NotificationPreferences::update)).
    pub async fn set(
        &self,
        owner: &PreferenceOwner,
        preferences: Preferences,
    ) -> Result<Preferences, NotifyError> {
        let mut preferences = preferences;
        preferences.compact();
        preferences
            .check(&self.options, &self.known_categories().await)
            .map_err(NotifyError::InvalidPreferences)?;
        self.store(owner, &preferences).await?;
        Ok(preferences)
    }

    /// Deletes `owner`'s preferences: everything is inherited again.
    /// `false` when none were stored.
    pub async fn forget(&self, owner: &PreferenceOwner) -> Result<bool, NotifyError> {
        let exec = Exec::current(&self.app).await?;
        let deleted = on!(&exec, e => NotificationPreference::query()
            .where_(NotificationPreference::OWNER_TYPE.eq(owner.owner_type.as_str()))
            .where_(NotificationPreference::OWNER_ID.eq(owner.owner_id.as_str()))
            .delete(e)
            .await)?;
        exec.wrote();
        Ok(deleted > 0)
    }

    /// The preferences that decide what `notifiable` receives: their own
    /// and, with `workspace`, the workspace's.
    pub async fn resolve(
        &self,
        notifiable: &dyn Notifiable,
        workspace: Option<Uuid>,
    ) -> Result<ResolvedPreferences, NotifyError> {
        self.resolve_owner(&PreferenceOwner::of(notifiable), workspace)
            .await
    }

    /// [`resolve`](NotificationPreferences::resolve) for an owner: one
    /// query for both levels.
    pub async fn resolve_owner(
        &self,
        owner: &PreferenceOwner,
        workspace: Option<Uuid>,
    ) -> Result<ResolvedPreferences, NotifyError> {
        let workspace_owner = workspace.map(PreferenceOwner::workspace);
        let mut ids = vec![owner.owner_id.clone()];
        if let Some(workspace) = &workspace_owner {
            ids.push(workspace.owner_id.clone());
        }
        let exec = Exec::current(&self.app).await?;
        let rows = on!(&exec, e => NotificationPreference::query()
            .where_(NotificationPreference::OWNER_ID.is_in(ids.iter().map(String::as_str)))
            .get(e)
            .await)?;
        let mut resolved = ResolvedPreferences {
            required: self.options.required.clone(),
            ..ResolvedPreferences::default()
        };
        for row in rows.into_iter().map(Record::into_inner) {
            let stored = PreferenceOwner {
                owner_type: row.owner_type.clone(),
                owner_id: row.owner_id.clone(),
            };
            if stored == *owner {
                resolved.own = row.read();
            } else if workspace_owner.as_ref() == Some(&stored) {
                resolved.workspace = Some(row.read());
            }
        }
        if workspace_owner.is_some() && resolved.workspace.is_none() {
            resolved.workspace = Some(Preferences::default());
        }
        Ok(resolved)
    }

    /// Writes `preferences` for `owner` (one row per owner; an empty set
    /// deletes it).
    async fn store(
        &self,
        owner: &PreferenceOwner,
        preferences: &Preferences,
    ) -> Result<(), NotifyError> {
        if preferences.is_empty() {
            self.forget(owner).await?;
            return Ok(());
        }
        let now = crate::notifier::now_in(&self.app).await;
        let row = NotificationPreference {
            id: Uuid::now_v7(),
            owner_type: owner.owner_type.clone(),
            owner_id: owner.owner_id.clone(),
            preferences: serde_json::to_value(preferences).map_err(|error| {
                NotifyError::InvalidPreferences(field_error("preferences", &error.to_string()))
            })?,
            created_at: now,
            updated_at: now,
        };
        let exec = Exec::current(&self.app).await?;
        on!(&exec, e => NotificationPreference::query()
            .upsert(
                e,
                [row],
                [NotificationPreference::OWNER_TYPE, NotificationPreference::OWNER_ID],
                [NotificationPreference::PREFERENCES],
            )
            .await)?;
        exec.wrote();
        Ok(())
    }

    /// The categories of the registered notification types (empty when
    /// none are registered: any category name is accepted then).
    async fn known_categories(&self) -> BTreeSet<String> {
        match self.app.upgrade() {
            Some(ctx) => match ctx.try_get::<crate::NotificationCatalog>().await {
                Some(catalog) => catalog.category_names(),
                None => BTreeSet::new(),
            },
            None => BTreeSet::new(),
        }
    }
}

impl fmt::Debug for NotificationPreferences {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NotificationPreferences")
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn prefs(value: Value) -> Preferences {
        serde_json::from_value(value).unwrap()
    }

    fn options() -> PreferenceOptions {
        PreferenceOptions {
            required: vec!["security".to_owned()],
            ..PreferenceOptions::default()
        }
    }

    #[test]
    fn a_category_choice_beats_the_one_for_every_category() {
        let own = prefs(json!({
            "channels": {"sms": false, "mail": true},
            "categories": {"deals": {"channels": {"sms": true}}}
        }));
        assert_eq!(own.channel("deals", Channel::Sms), Some(true));
        assert_eq!(own.channel("billing", Channel::Sms), Some(false));
        assert_eq!(own.channel("billing", Channel::Slack), None);
        // Broadcast follows database.
        let own = prefs(json!({"channels": {"database": false}}));
        assert_eq!(own.channel("deals", Channel::Broadcast), Some(false));
    }

    #[test]
    fn the_person_beats_the_workspace_which_beats_the_default() {
        let resolved = ResolvedPreferences {
            own: prefs(json!({"categories": {"deals": {"channels": {"mail": true}}}})),
            workspace: Some(prefs(json!({
                "channels": {"mail": false, "sms": false},
                "digest": "weekly",
                "quiet_hours": {"start": "20:00", "end": "08:00"}
            }))),
            required: vec!["security".to_owned()],
        };
        assert!(resolved.allows("deals", Channel::Mail));
        assert!(!resolved.allows("billing", Channel::Mail));
        assert!(!resolved.allows("deals", Channel::Sms));
        assert!(resolved.allows("deals", Channel::Slack));
        // Required categories always deliver.
        assert!(resolved.allows("security", Channel::Sms));
        assert_eq!(resolved.digest("deals", Digest::Off), Digest::Weekly);
        assert_eq!(
            resolved.quiet_hours(),
            Some(QuietHours::new("20:00", "08:00").unwrap())
        );

        let own_quiet = ResolvedPreferences {
            own: prefs(json!({"quiet_hours": {"start": "00:00", "end": "00:00"}, "digest": "off"})),
            ..resolved.clone()
        };
        // An empty period of one's own turns the workspace's off.
        assert_eq!(own_quiet.quiet_hours(), None);
        assert_eq!(own_quiet.digest("deals", Digest::Daily), Digest::Off);

        let nothing = ResolvedPreferences::default();
        assert!(nothing.allows("deals", Channel::Sms));
        assert_eq!(nothing.digest("deals", Digest::Daily), Digest::Daily);
        assert_eq!(nothing.quiet_hours(), None);
    }

    #[test]
    fn merge_patches_add_replace_and_remove_choices() {
        let none = BTreeSet::new();
        let current = prefs(json!({
            "channels": {"sms": false},
            "categories": {"deals": {"channels": {"mail": false}, "digest": "daily"}},
            "quiet_hours": {"start": "22:00", "end": "07:00"}
        }));
        let merged = current
            .merged(
                &json!({
                    "channels": {"sms": null, "slack": false},
                    "categories": {"deals": {"digest": null}, "billing": {"channels": {"mail": false}}},
                    "quiet_hours": null
                }),
                &options(),
                &none,
            )
            .unwrap();
        assert_eq!(
            serde_json::to_value(&merged).unwrap(),
            json!({
                "channels": {"slack": false},
                "categories": {
                    "billing": {"channels": {"mail": false}},
                    "deals": {"channels": {"mail": false}}
                }
            })
        );
        // Removing a category's last choice drops the category.
        let merged = merged
            .merged(
                &json!({"categories": {"deals": {"channels": {"mail": null}}}}),
                &options(),
                &none,
            )
            .unwrap();
        assert!(!merged.categories.contains_key("deals"));
        let emptied = merged
            .merged(
                &json!({"channels": null, "categories": null}),
                &options(),
                &none,
            )
            .unwrap();
        assert!(emptied.is_empty());
    }

    #[test]
    fn bad_changes_are_refused_naming_the_field() {
        let none = BTreeSet::new();
        let current = Preferences::default();
        let errors = current
            .merged(
                &json!({"categories": {"security": {"channels": {"mail": false}}}}),
                &options(),
                &none,
            )
            .unwrap_err();
        assert_eq!(
            errors["categories.security.channels.mail"],
            ["security notifications cannot be turned off"]
        );
        // Turning a required category's channel on is fine.
        assert!(
            current
                .merged(
                    &json!({"categories": {"security": {"channels": {"mail": true}}}}),
                    &options(),
                    &none
                )
                .is_ok()
        );

        let errors = current
            .merged(
                &json!({"channels": {"broadcast": false}}),
                &options(),
                &none,
            )
            .unwrap_err();
        assert_eq!(
            errors["channels.broadcast"],
            ["the broadcast channel cannot be switched"]
        );

        let errors = current
            .merged(&json!({"channels": {"pigeon": false}}), &options(), &none)
            .unwrap_err();
        assert!(errors.contains_key("preferences"), "{errors:?}");

        let errors = current
            .merged(&json!({"digest": "monthly"}), &options(), &none)
            .unwrap_err();
        assert!(errors.contains_key("preferences"), "{errors:?}");

        let errors = current
            .merged(
                &json!({"quiet_hours": {"start": "25:00", "end": "07:00"}}),
                &options(),
                &none,
            )
            .unwrap_err();
        assert!(errors["preferences"][0].contains("25:00"), "{errors:?}");

        let errors = current
            .merged(
                &json!({"categories": {"bad name": {"digest": "daily"}}}),
                &options(),
                &none,
            )
            .unwrap_err();
        assert!(errors.contains_key("categories.bad name"), "{errors:?}");

        let known = BTreeSet::from(["deals".to_owned()]);
        let errors = current
            .merged(
                &json!({"categories": {"billing": {"digest": "daily"}}}),
                &options(),
                &known,
            )
            .unwrap_err();
        assert_eq!(
            errors["categories.billing"],
            ["`billing` is not a notification category"]
        );

        let errors = current.merged(&json!([1]), &options(), &none).unwrap_err();
        assert!(errors.contains_key("preferences"));
    }

    #[test]
    fn owners_name_people_and_workspaces() {
        let id = Uuid::now_v7();
        assert_eq!(PreferenceOwner::user(id).to_string(), format!("user:{id}"));
        assert!(PreferenceOwner::workspace(id).is_workspace());
        assert!(!PreferenceOwner::user(id).is_workspace());
        assert!(check_category("deals.won:v2").is_ok());
        assert!(check_category("").is_err());
        assert!(check_category(&"a".repeat(MAX_CATEGORY_LEN + 1)).is_err());
    }

    #[test]
    fn the_number_of_categories_is_capped() {
        let none = BTreeSet::new();
        let many: serde_json::Map<String, Value> = (0..=MAX_CATEGORIES)
            .map(|i| (format!("c{i}"), json!({"digest": "daily"})))
            .collect();
        let errors = Preferences::default()
            .merged(&json!({"categories": many}), &options(), &none)
            .unwrap_err();
        assert!(errors.contains_key("categories"), "{errors:?}");
    }
}
