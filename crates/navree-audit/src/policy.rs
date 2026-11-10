//! [`Policy`]: the compiled [`AuditOptions`] (what to skip, how to name).

use std::collections::{HashMap, HashSet};

use navree_contracts::Redactor;
use serde_json::{Map, Value};

use crate::naming::subject_type_of;
use crate::{AuditOptions, DEFAULT_SKIP_FIELDS};

/// The audit table (never audited itself).
pub const AUDIT_TABLE: &str = "audit_logs";

/// What gets audited and how it is named, compiled from [`AuditOptions`].
#[derive(Debug)]
pub(crate) struct Policy {
    pub(crate) enabled: bool,
    /// Lower-cased subject types (table names normalised with
    /// [`Policy::subject_type`]).
    skip_entities: HashSet<String>,
    /// Lower-cased field names skipped at any depth.
    skip_fields: HashSet<String>,
    /// Finds the fields that are secrets by name; they are skipped at any
    /// depth too.
    secrets: Redactor,
    /// Lower-cased (subject type, field) pairs.
    skip_owned_fields: HashSet<(String, String)>,
    /// Table → subject type overrides.
    subject_types: HashMap<String, String>,
}

impl Policy {
    pub(crate) fn new(options: &AuditOptions) -> Self {
        let mut policy = Self {
            enabled: options.enabled,
            skip_entities: HashSet::new(),
            skip_fields: HashSet::new(),
            secrets: Redactor::new(),
            skip_owned_fields: HashSet::new(),
            subject_types: options
                .subject_types
                .iter()
                .map(|(table, subject)| (table.trim().to_owned(), subject.trim().to_owned()))
                .collect(),
        };
        policy.skip_entities = options
            .skip_entities
            .iter()
            .map(|entity| policy.owner(entity))
            .collect();
        let configured = options.skip_fields.iter().map(String::as_str);
        for field in DEFAULT_SKIP_FIELDS.iter().copied().chain(configured) {
            match field.split_once('.') {
                Some((owner, field)) => {
                    let owned = (policy.owner(owner), field.trim().to_lowercase());
                    policy.skip_owned_fields.insert(owned);
                }
                None => {
                    policy.skip_fields.insert(field.trim().to_lowercase());
                }
            }
        }
        policy
    }

    /// The subject type recorded for `table`.
    pub(crate) fn subject_type(&self, table: &str) -> String {
        self.subject_types
            .get(table)
            .cloned()
            .unwrap_or_else(|| subject_type_of(table))
    }

    /// A table name or subject type, normalised to the lower-cased subject
    /// type (`users`, `User` → `user`).
    fn owner(&self, name: &str) -> String {
        self.subject_type(name.trim()).to_lowercase()
    }

    /// Whether changes of `table` are never audited.
    pub(crate) fn skips_entity(&self, table: &str) -> bool {
        table.eq_ignore_ascii_case(AUDIT_TABLE) || self.skip_entities.contains(&self.owner(table))
    }

    /// `value` without skipped fields: names skipped everywhere at any
    /// depth, `owner.field` pairs at the top level of the values of that
    /// owner (a table name or subject type, interchangeably).
    pub(crate) fn strip(&self, owner: Option<&str>, value: Value) -> Value {
        let value = match (owner, value) {
            (Some(owner), Value::Object(map)) => {
                let owner = self.owner(owner);
                Value::Object(
                    map.into_iter()
                        .filter(|(key, _)| {
                            !self
                                .skip_owned_fields
                                .contains(&(owner.clone(), key.to_lowercase()))
                        })
                        .collect(),
                )
            }
            (_, value) => value,
        };
        self.strip_names(value)
    }

    /// Whether a field called `name` is dropped wherever it is: it is in the
    /// skip list, or its name says it holds a secret.
    fn skips_name(&self, name: &str) -> bool {
        self.skip_fields.contains(&name.to_lowercase()) || self.secrets.is_secret(name)
    }

    /// `value` without the fields skipped everywhere, at any depth.
    fn strip_names(&self, value: Value) -> Value {
        match value {
            Value::Object(map) => Value::Object(
                map.into_iter()
                    .filter(|(key, _)| !self.skips_name(key))
                    .map(|(key, value)| (key, self.strip_names(value)))
                    .collect(),
            ),
            Value::Array(items) => {
                Value::Array(items.into_iter().map(|v| self.strip_names(v)).collect())
            }
            other => other,
        }
    }
}

/// The fields whose values differ between `old` and `new` (a key missing on
/// one side counts as `null`): (old values, new values). Values that are not
/// both objects are returned unchanged.
pub(crate) fn changed_fields(old: Value, new: Value) -> (Value, Value) {
    let (old, mut new) = match (old, new) {
        (Value::Object(old), Value::Object(new)) => (old, new),
        other => return other,
    };
    let mut before = Map::new();
    let mut after = Map::new();
    for (key, previous) in old {
        let current = new.remove(&key).unwrap_or(Value::Null);
        if previous != current {
            before.insert(key.clone(), previous);
            after.insert(key, current);
        }
    }
    for (key, current) in new {
        if !current.is_null() {
            before.insert(key.clone(), Value::Null);
            after.insert(key, current);
        }
    }
    (Value::Object(before), Value::Object(after))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn policy(options: AuditOptions) -> Policy {
        Policy::new(&options)
    }

    #[test]
    fn default_fields_are_stripped_at_any_depth() {
        let p = policy(AuditOptions::default());
        let stripped = p.strip(
            Some("users"),
            json!({
                "email": "a@example.com",
                "Password_Hash": "x",
                "remember_token": "y",
                "settings": {"theme": "dark", "token": "z", "list": [{"secret": 1, "ok": 2}]},
            }),
        );
        assert_eq!(
            stripped,
            json!({
                "email": "a@example.com",
                "settings": {"theme": "dark", "list": [{"ok": 2}]},
            })
        );
    }

    #[test]
    fn fields_named_like_secrets_are_stripped_too() {
        let p = policy(AuditOptions::default());
        let stripped = p.strip(
            Some("integrations"),
            json!({
                "name": "tally",
                "api_key": "k-1",
                "Client_Secret": "s-1",
                "db_password": "p-1",
                "config": {"webhook_token": "t-1", "region": "ap-south-1"},
            }),
        );
        assert_eq!(
            stripped,
            json!({"name": "tally", "config": {"region": "ap-south-1"}})
        );
    }

    #[test]
    fn configured_fields_extend_the_defaults() {
        let p = policy(AuditOptions {
            skip_fields: vec!["ssn".into(), "users.phone".into()],
            ..AuditOptions::default()
        });
        let value = json!({"ssn": 1, "phone": 2, "password": 3, "name": 4, "nested": {"phone": 5}});
        assert_eq!(
            p.strip(Some("users"), value.clone()),
            json!({"name": 4, "nested": {"phone": 5}}),
            "`users.phone` applies to the top level of users only"
        );
        assert_eq!(
            p.strip(Some("contacts"), value.clone()),
            json!({"phone": 2, "name": 4, "nested": {"phone": 5}})
        );
        assert_eq!(
            p.strip(None, value),
            json!({"phone": 2, "name": 4, "nested": {"phone": 5}})
        );
    }

    #[test]
    fn table_qualified_fields_match_the_subject_type_too() {
        let p = policy(AuditOptions {
            skip_fields: vec!["User.phone".into()],
            ..AuditOptions::default()
        });
        assert_eq!(
            p.strip(Some("users"), json!({"phone": 1, "name": 2})),
            json!({"name": 2})
        );
    }

    #[test]
    fn table_and_subject_type_qualifiers_are_interchangeable() {
        let p = policy(AuditOptions {
            skip_fields: vec!["users.phone".into(), "people.ssn".into()],
            subject_types: [("people".to_owned(), "Person".to_owned())].into(),
            ..AuditOptions::default()
        });
        let value = json!({"phone": 1, "ssn": 2, "name": 3});
        // Data rows are keyed by table, sink entries by subject type.
        for owner in ["users", "User"] {
            assert_eq!(
                p.strip(Some(owner), value.clone()),
                json!({"ssn": 2, "name": 3}),
                "{owner}"
            );
        }
        for owner in ["people", "Person"] {
            assert_eq!(
                p.strip(Some(owner), value.clone()),
                json!({"phone": 1, "name": 3}),
                "{owner}"
            );
        }
    }

    #[test]
    fn skipped_entities_match_table_or_subject_type() {
        let p = policy(AuditOptions {
            skip_entities: vec!["sessions".into(), "Job".into()],
            ..AuditOptions::default()
        });
        assert!(p.skips_entity("sessions"));
        assert!(p.skips_entity("Sessions"));
        assert!(p.skips_entity("jobs"));
        assert!(p.skips_entity("audit_logs"), "the audit table itself");
        assert!(!p.skips_entity("deals"));
    }

    #[test]
    fn subject_types_can_be_overridden() {
        let p = policy(AuditOptions {
            subject_types: [("people".to_owned(), "Person".to_owned())].into(),
            ..AuditOptions::default()
        });
        assert_eq!(p.subject_type("people"), "Person");
        assert_eq!(p.subject_type("deals"), "Deal");
    }

    #[test]
    fn changed_fields_keep_differences_only() {
        let (old, new) = changed_fields(
            json!({"id": 1, "name": "a", "stage": "lead", "gone": true}),
            json!({"id": 1, "name": "a", "stage": "won", "added": 2}),
        );
        assert_eq!(old, json!({"stage": "lead", "gone": true, "added": null}));
        assert_eq!(new, json!({"stage": "won", "gone": null, "added": 2}));
        let (old, new) = changed_fields(json!(1), json!(2));
        assert_eq!((old, new), (json!(1), json!(2)));
    }
}
