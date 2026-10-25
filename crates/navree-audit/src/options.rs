//! [`AuditOptions`]: the `audit` configuration section.

use std::collections::BTreeMap;

use navree_config::ConfigSection;
use serde::{Deserialize, Deserializer};

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
/// the default list.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
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
    #[serde(deserialize_with = "list_or_comma_separated")]
    pub skip_entities: Vec<String>,
    /// Fields dropped from old and new values and metadata: a name
    /// (`ssn`, matched case-insensitively at any depth) or `table.field`
    /// (`users.phone`, top-level fields of one table or subject type).
    /// Defaults to [`DEFAULT_SKIP_FIELDS`], which apply in any case.
    #[serde(deserialize_with = "list_or_comma_separated")]
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

impl ConfigSection for AuditOptions {
    const KEY: &'static str = "audit";

    fn validate(&self) -> Result<(), String> {
        self.check()
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

/// A list, or one comma-separated string (what an environment variable
/// holds); entries are trimmed and empty ones dropped.
fn list_or_comma_separated<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum List {
        Items(Vec<String>),
        Joined(String),
    }
    let items = match List::deserialize(deserializer)? {
        List::Items(items) => items,
        List::Joined(joined) => joined.split(',').map(str::to_owned).collect(),
    };
    Ok(items
        .into_iter()
        .map(|item| item.trim().to_owned())
        .filter(|item| !item.is_empty())
        .collect())
}
