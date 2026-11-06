//! [`AuditOptions`]: the `audit` configuration section.

use std::collections::BTreeMap;

use navree_config::ConfigSection;
use serde::Deserialize;

/// Field names that never reach the audit trail, whatever
/// [`AuditOptions::skip_fields`] says.
pub const DEFAULT_SKIP_FIELDS: &[&str] = &[
    "password",
    "password_hash",
    "token",
    "token_hash",
    "secret",
    "two_factor_secret",
    "two_factor_recovery_codes",
    "remember_token",
];

/// The settings of `AuditModule`: the `audit` configuration section. Every
/// key is optional.
///
/// ```toml
/// [audit]
/// enabled = true
/// retention_days = 365                 # audit:prune deletes older entries; 0 keeps them forever
/// skip_entities = ["sessions"]         # tables (or subject types) never audited
/// skip_fields = ["ssn", "users.phone"] # fields never stored (`table.field` for one table)
/// subject_types = { people = "Person" } # subject type of a table (default: `deals` → `Deal`)
/// routes = true                        # GET /api/audit (permission `audit.read`)
/// ```
///
/// From the environment, the lists are comma-separated strings
/// (`APP_AUDIT__SKIP_FIELDS=ssn,users.phone`).
///
/// The names in [`DEFAULT_SKIP_FIELDS`] (passwords, tokens, secrets,
/// two-factor codes) are always skipped, even when `skip_fields` replaces
/// the default list, and so is any field whose name is a secret by name
/// (it contains one of the shared secret words, such as `api_key`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, ConfigSection)]
#[config(key = "audit", docs = "/docs/security/audit-log/", validate = Self::check)]
#[serde(default)]
pub struct AuditOptions {
    /// Record audit entries at all (default `true`). When off, nothing is
    /// written; existing entries can still be listed and pruned.
    pub enabled: bool,
    /// How many days `audit:prune` keeps entries (default 365; 0 keeps them
    /// forever).
    pub retention_days: u32,
    /// Tables whose changes are never audited, by table name (`sessions`)
    /// or subject type (`Session`), case-insensitively. The `audit_logs`
    /// table itself is never audited.
    #[serde(deserialize_with = "navree_config::comma_list")]
    pub skip_entities: Vec<String>,
    /// Fields dropped from old and new values and metadata: a name
    /// (`ssn`, matched case-insensitively at any depth) or `table.field`
    /// (`users.phone`, top-level fields of one table or subject type).
    /// Defaults to [`DEFAULT_SKIP_FIELDS`], which apply in any case.
    #[serde(deserialize_with = "navree_config::comma_list")]
    pub skip_fields: Vec<String>,
    /// The subject type recorded for a table, overriding the default (the
    /// table name singularised in PascalCase: `deals` → `Deal`,
    /// `companies` → `Company`, `deal_notes` → `DealNote`). Actions are
    /// `<subject type in snake_case>.<created|updated|deleted>`.
    pub subject_types: BTreeMap<String, String>,
    /// Mount `GET {api_prefix}/audit` (default `true`). It requires the
    /// `audit.read` permission, so the app needs a `dyn RouteAuthorizer`
    /// (navree-access); turn it off in apps without one.
    pub routes: bool,
}

impl Default for AuditOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            retention_days: 365,
            skip_entities: Vec::new(),
            skip_fields: DEFAULT_SKIP_FIELDS
                .iter()
                .map(|f| (*f).to_owned())
                .collect(),
            subject_types: BTreeMap::new(),
            routes: true,
        }
    }
}

impl AuditOptions {
    /// Checks the options.
    pub(crate) fn check(&self) -> Result<(), String> {
        let mut errors = Vec::new();
        for (table, subject) in &self.subject_types {
            if table.trim().is_empty() || subject.trim().is_empty() {
                errors.push(format!(
                    "audit.subject_types: `{table}` = `{subject}` must name a table and a subject type"
                ));
            }
        }
        if self
            .skip_fields
            .iter()
            .any(|f| f.split('.').any(|part| part.trim().is_empty()))
        {
            errors.push(
                "audit.skip_fields entries must be `field` or `table.field` names".to_owned(),
            );
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}

#[cfg(test)]
mod tests {
    use navree_config::FieldType;

    use super::*;

    #[test]
    fn the_section_describes_itself() {
        let schema = AuditOptions::schema().expect("schema");
        assert_eq!(schema.key, "audit");
        assert_eq!(schema.docs.as_deref(), Some("/docs/security/audit-log/"));
        assert!(
            schema.unmarked_secrets().is_empty(),
            "{:?}",
            schema.unmarked_secrets()
        );
        let field = |name: &str| {
            schema
                .fields
                .iter()
                .find(|f| f.name == name)
                .unwrap_or_else(|| panic!("{name}"))
        };
        assert_eq!(field("skip_fields").ty, FieldType::list(FieldType::String));
        assert_eq!(field("subject_types").ty, FieldType::map(FieldType::String));
        assert_eq!(field("retention_days").ty, FieldType::Integer);
        let defaults = AuditOptions::defaults().expect("defaults");
        assert_eq!(defaults["retention_days"], 365);
        assert_eq!(defaults["skip_fields"][0], "password");
    }

    #[test]
    fn lists_come_from_a_list_or_one_comma_separated_string() {
        let list: AuditOptions = serde_json::from_value(
            serde_json::json!({ "skip_fields": [" ssn ", "", "users.phone"] }),
        )
        .expect("list");
        assert_eq!(list.skip_fields, ["ssn", "users.phone"]);
        let joined: AuditOptions = serde_json::from_value(
            serde_json::json!({ "skip_entities": "sessions, jobs,,", "skip_fields": "" }),
        )
        .expect("joined");
        assert_eq!(joined.skip_entities, ["sessions", "jobs"]);
        assert!(joined.skip_fields.is_empty());
    }
}
