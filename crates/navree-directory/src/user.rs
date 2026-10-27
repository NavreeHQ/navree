//! The normalized directory: [`DirectoryUser`], [`DirectoryGroup`] and
//! [`ContactValue`]. Every source (SCIM, Microsoft Graph, Google Workspace,
//! Okta, LDAP, your own) produces them, and your
//! [`DirectorySink`](crate::DirectorySink) maps them into your model.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use utoipa::ToSchema;

/// A person in a directory, the same shape whichever source it came from.
///
/// `external_id` is the person's id in the directory (the Entra object id,
/// the Google user id, the Okta id, the LDAP `objectGUID` or `entryUUID`;
/// for SCIM, the client's `externalId`, or the SCIM id when it sends
/// none). It is stable for the person, unlike `user_name` and the email,
/// which people change.
///
/// ```
/// use navree_directory::DirectoryUser;
///
/// let ada = DirectoryUser::new("00u1ada", "ada@acme.test")
///     .name("Ada", "Lovelace")
///     .email("ada@acme.test")
///     .title("Principal engineer")
///     .department("Engineering");
/// assert_eq!(ada.primary_email(), Some("ada@acme.test"));
/// assert_eq!(ada.full_name(), "Ada Lovelace");
/// assert!(ada.active);
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct DirectoryUser {
    /// The person's id in the directory.
    pub external_id: String,
    /// The sign-in name (user principal name, primary email, Okta login,
    /// `sAMAccountName` or `uid`, SCIM `userName`).
    pub user_name: String,
    /// Whether the account is enabled in the directory.
    pub active: bool,
    /// The given (first) name.
    pub given_name: Option<String>,
    /// The family (last) name.
    pub family_name: Option<String>,
    /// The name to show.
    pub display_name: Option<String>,
    /// Email addresses; the primary one is marked.
    pub emails: Vec<ContactValue>,
    /// Phone numbers (`work`, `mobile`, ...).
    pub phone_numbers: Vec<ContactValue>,
    /// The job title.
    pub title: Option<String>,
    /// The department.
    pub department: Option<String>,
    /// The organization or company name.
    pub organization: Option<String>,
    /// The employee number.
    pub employee_number: Option<String>,
    /// The manager's `external_id`, when the directory names one.
    pub manager_id: Option<String>,
    /// The preferred language or locale (`en-US`).
    pub locale: Option<String>,
    /// The time zone (`Europe/Paris`).
    pub time_zone: Option<String>,
    /// Anything else the source carries, by the source's attribute name
    /// (SCIM attributes Navree has no field for, LDAP attributes named in
    /// `settings.attributes`).
    #[schema(value_type = Object)]
    pub attributes: BTreeMap<String, Value>,
}

impl Default for DirectoryUser {
    fn default() -> Self {
        Self {
            external_id: String::new(),
            user_name: String::new(),
            active: true,
            given_name: None,
            family_name: None,
            display_name: None,
            emails: Vec::new(),
            phone_numbers: Vec::new(),
            title: None,
            department: None,
            organization: None,
            employee_number: None,
            manager_id: None,
            locale: None,
            time_zone: None,
            attributes: BTreeMap::new(),
        }
    }
}

impl DirectoryUser {
    /// An active user with an id and a sign-in name.
    pub fn new(external_id: impl Into<String>, user_name: impl Into<String>) -> Self {
        Self {
            external_id: external_id.into(),
            user_name: user_name.into(),
            ..Self::default()
        }
    }

    /// Sets the given and family names.
    #[must_use]
    pub fn name(mut self, given: impl Into<String>, family: impl Into<String>) -> Self {
        self.given_name = Some(given.into());
        self.family_name = Some(family.into());
        self
    }

    /// Sets the display name.
    #[must_use]
    pub fn display_name(mut self, name: impl Into<String>) -> Self {
        self.display_name = Some(name.into());
        self
    }

    /// Adds an email address; the first one added is the primary one.
    #[must_use]
    pub fn email(mut self, email: impl Into<String>) -> Self {
        let primary = self.emails.is_empty();
        self.emails.push(ContactValue {
            value: email.into(),
            kind: Some("work".to_owned()),
            primary,
        });
        self
    }

    /// Adds a phone number of `kind` (`work`, `mobile`).
    #[must_use]
    pub fn phone(mut self, kind: impl Into<String>, number: impl Into<String>) -> Self {
        let primary = self.phone_numbers.is_empty();
        self.phone_numbers.push(ContactValue {
            value: number.into(),
            kind: Some(kind.into()),
            primary,
        });
        self
    }

    /// Sets the job title.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Sets the department.
    #[must_use]
    pub fn department(mut self, department: impl Into<String>) -> Self {
        self.department = Some(department.into());
        self
    }

    /// Sets the employee number.
    #[must_use]
    pub fn employee_number(mut self, number: impl Into<String>) -> Self {
        self.employee_number = Some(number.into());
        self
    }

    /// Sets the manager's `external_id`.
    #[must_use]
    pub fn manager(mut self, manager_id: impl Into<String>) -> Self {
        self.manager_id = Some(manager_id.into());
        self
    }

    /// Sets an extra attribute.
    #[must_use]
    pub fn attribute(mut self, name: impl Into<String>, value: impl Into<Value>) -> Self {
        self.attributes.insert(name.into(), value.into());
        self
    }

    /// Marks the account as disabled.
    #[must_use]
    pub fn inactive(mut self) -> Self {
        self.active = false;
        self
    }

    /// The primary email: the one marked primary, else the first.
    pub fn primary_email(&self) -> Option<&str> {
        self.emails
            .iter()
            .find(|email| email.primary)
            .or_else(|| self.emails.first())
            .map(|email| email.value.as_str())
    }

    /// The primary phone number: the one marked primary, else the first.
    pub fn primary_phone(&self) -> Option<&str> {
        self.phone_numbers
            .iter()
            .find(|phone| phone.primary)
            .or_else(|| self.phone_numbers.first())
            .map(|phone| phone.value.as_str())
    }

    /// The name to show: the display name, else the given and family
    /// names, else the sign-in name.
    pub fn full_name(&self) -> String {
        if let Some(name) = self.display_name.as_deref().filter(|n| !n.trim().is_empty()) {
            return name.to_owned();
        }
        let parts: Vec<&str> = [self.given_name.as_deref(), self.family_name.as_deref()]
            .into_iter()
            .flatten()
            .filter(|part| !part.trim().is_empty())
            .collect();
        if parts.is_empty() {
            self.user_name.clone()
        } else {
            parts.join(" ")
        }
    }

    /// A SHA-256 of the user's canonical JSON: two users with the same
    /// fingerprint are the same version.
    pub fn fingerprint(&self) -> String {
        fingerprint(self)
    }

    /// The fields that differ from `other`, by their names in this struct
    /// (`title`, `emails`, `attributes`), sorted by name.
    pub fn changed_fields(&self, other: &DirectoryUser) -> Vec<String> {
        changed_fields(self, other)
    }
}

/// One email address or phone number.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ContactValue {
    /// The address or number.
    pub value: String,
    /// `work`, `home`, `mobile`, `other`...
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Whether it is the primary one.
    #[serde(default)]
    pub primary: bool,
}

impl ContactValue {
    /// A value of `kind`.
    pub fn new(value: impl Into<String>, kind: Option<&str>, primary: bool) -> Self {
        Self {
            value: value.into(),
            kind: kind.map(str::to_owned),
            primary,
        }
    }
}

/// A group in a directory, with its members' `external_id`s.
///
/// ```
/// use navree_directory::DirectoryGroup;
///
/// let engineering = DirectoryGroup::new("grp-eng", "Engineering")
///     .members(["00u1ada", "00u2grace"]);
/// assert_eq!(engineering.members.len(), 2);
/// ```
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct DirectoryGroup {
    /// The group's id in the directory.
    pub external_id: String,
    /// The group's name.
    pub display_name: String,
    /// What the group is for.
    pub description: Option<String>,
    /// The group's email address (mail-enabled groups).
    pub email: Option<String>,
    /// The members' `external_id`s (users only; nested groups are not
    /// expanded).
    pub members: Vec<String>,
    /// Anything else the source carries.
    #[schema(value_type = Object)]
    pub attributes: BTreeMap<String, Value>,
}

impl DirectoryGroup {
    /// A group without members.
    pub fn new(external_id: impl Into<String>, display_name: impl Into<String>) -> Self {
        Self {
            external_id: external_id.into(),
            display_name: display_name.into(),
            ..Self::default()
        }
    }

    /// Sets the description.
    #[must_use]
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Sets the email address.
    #[must_use]
    pub fn email(mut self, email: impl Into<String>) -> Self {
        self.email = Some(email.into());
        self
    }

    /// Adds members by `external_id`.
    #[must_use]
    pub fn members<I, S>(mut self, members: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.members.extend(members.into_iter().map(Into::into));
        self
    }

    /// The group without its members: what a fingerprint of the group's
    /// own attributes is taken of (memberships are compared on their own).
    pub(crate) fn without_members(&self) -> DirectoryGroup {
        DirectoryGroup {
            members: Vec::new(),
            ..self.clone()
        }
    }

    /// A SHA-256 of the group's attributes (members left out).
    pub fn fingerprint(&self) -> String {
        fingerprint(&self.without_members())
    }

    /// The attribute fields that differ from `other` (members left out).
    pub fn changed_fields(&self, other: &DirectoryGroup) -> Vec<String> {
        changed_fields(&self.without_members(), &other.without_members())
    }
}

fn fingerprint<T: Serialize>(value: &T) -> String {
    let json = serde_json::to_vec(value).unwrap_or_default();
    hex::encode(Sha256::digest(&json))
}

fn changed_fields<T: Serialize>(new: &T, old: &T) -> Vec<String> {
    let (Ok(Value::Object(new)), Ok(Value::Object(old))) =
        (serde_json::to_value(new), serde_json::to_value(old))
    else {
        return Vec::new();
    };
    let mut changed: Vec<String> = new
        .iter()
        .filter(|(key, value)| old.get(*key) != Some(*value))
        .map(|(key, _)| key.clone())
        .collect();
    changed.sort();
    changed
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_new_user_is_active_and_the_first_email_is_primary() {
        let user = DirectoryUser::new("u1", "ada@acme.test")
            .email("ada@acme.test")
            .email("ada.lovelace@acme.test");
        assert!(user.active);
        assert_eq!(user.primary_email(), Some("ada@acme.test"));
        assert!(user.emails[0].primary);
        assert!(!user.emails[1].primary);
        assert_eq!(user.primary_phone(), None);
    }

    #[test]
    fn the_primary_email_is_the_marked_one_else_the_first() {
        let mut user = DirectoryUser::new("u1", "ada");
        user.emails = vec![
            ContactValue::new("home@acme.test", Some("home"), false),
            ContactValue::new("work@acme.test", Some("work"), true),
        ];
        assert_eq!(user.primary_email(), Some("work@acme.test"));
        user.emails[1].primary = false;
        assert_eq!(user.primary_email(), Some("home@acme.test"));
    }

    #[test]
    fn the_full_name_falls_back_to_the_parts_then_the_sign_in_name() {
        let user = DirectoryUser::new("u1", "ada@acme.test");
        assert_eq!(user.full_name(), "ada@acme.test");
        let user = user.name("Ada", "Lovelace");
        assert_eq!(user.full_name(), "Ada Lovelace");
        let user = user.display_name("Countess of Lovelace");
        assert_eq!(user.full_name(), "Countess of Lovelace");
        let mut blank = DirectoryUser::new("u2", "grace");
        blank.display_name = Some("  ".into());
        blank.given_name = Some("Grace".into());
        assert_eq!(blank.full_name(), "Grace");
    }

    #[test]
    fn fingerprints_change_with_any_field_and_list_the_changed_ones() {
        let ada = DirectoryUser::new("u1", "ada@acme.test").title("Engineer");
        let same = ada.clone();
        assert_eq!(ada.fingerprint(), same.fingerprint());
        assert_eq!(ada.fingerprint().len(), 64);
        let promoted = ada.clone().title("Principal engineer").inactive();
        assert_ne!(ada.fingerprint(), promoted.fingerprint());
        assert_eq!(promoted.changed_fields(&ada), vec!["active", "title"]);
        let tagged = ada.clone().attribute("costCenter", "4130");
        assert_eq!(tagged.changed_fields(&ada), vec!["attributes"]);
    }

    #[test]
    fn group_fingerprints_leave_members_out() {
        let group = DirectoryGroup::new("g1", "Engineering").members(["u1"]);
        let more = group.clone().members(["u2"]);
        assert_eq!(group.fingerprint(), more.fingerprint());
        assert!(more.changed_fields(&group).is_empty());
        let renamed = DirectoryGroup {
            display_name: "R&D".into(),
            ..group.clone()
        };
        assert_eq!(renamed.changed_fields(&group), vec!["display_name"]);
    }

    #[test]
    fn users_round_trip_through_json_with_defaults() {
        let user: DirectoryUser =
            serde_json::from_value(json!({ "external_id": "u1", "user_name": "ada" })).unwrap();
        assert!(user.active);
        assert!(user.emails.is_empty());
        let email = ContactValue::new("ada@acme.test", Some("work"), true);
        assert_eq!(
            serde_json::to_value(&email).unwrap(),
            json!({ "value": "ada@acme.test", "type": "work", "primary": true })
        );
        let user = DirectoryUser::new("u1", "ada").phone("mobile", "+33 6 12 34 56 78");
        assert_eq!(user.primary_phone(), Some("+33 6 12 34 56 78"));
        let back: DirectoryUser =
            serde_json::from_value(serde_json::to_value(&user).unwrap()).unwrap();
        assert_eq!(back, user);
    }
}
