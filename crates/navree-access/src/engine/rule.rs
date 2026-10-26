//! Rules and the [`Rules`] builder.

use std::collections::BTreeMap;

use serde_json::Value;

use super::ability::Ability;
use super::condition::{Conditions, validate_field_name};
use super::{ALL, MANAGE};

/// Why a rule set was rejected (when building rules or parsing CASL JSON).
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RuleError {
    /// A condition operator outside `$eq`, `$ne`, `$lt`, `$lte`, `$gt`,
    /// `$gte`, `$in` and `$nin` (e.g. `$exists`, `$regex`, `$or`).
    #[error(
        "unsupported condition operator `{0}` (supported: $eq, $ne, $lt, $lte, $gt, $gte, $in, $nin)"
    )]
    UnsupportedOperator(String),
    /// A dotted field path; conditions and fields apply to top-level columns.
    #[error(
        "nested field `{0}` is not supported: conditions and fields apply to top-level columns"
    )]
    NestedField(String),
    /// A field name that is empty or not ASCII letters, digits and `_`.
    #[error("invalid field name `{0}`: use ASCII letters, digits and `_`")]
    InvalidField(String),
    /// An operand of the wrong type for its operator.
    #[error("invalid `{operator}` operand for `{field}`: {reason}")]
    InvalidValue {
        /// The field the condition is on.
        field: String,
        /// The operator, e.g. `"$lt"`.
        operator: &'static str,
        /// What is wrong with the operand.
        reason: String,
    },
    /// The same operator twice on one field (CASL's JSON form cannot
    /// express it).
    #[error("`{field}` has more than one `{operator}` condition")]
    DuplicateOperator {
        /// The field.
        field: String,
        /// The repeated operator.
        operator: &'static str,
    },
    /// A malformed rule (empty action or subject, wrong JSON shape).
    #[error("invalid rule: {0}")]
    InvalidRule(String),
}

/// One CASL rule: `action` on `subject`, optionally only where `conditions`
/// hold and only for `fields`; `inverted` rules deny.
///
/// Serializes to CASL's JSON rule format: `action` and `subject` are a string
/// when they hold one element and an array otherwise; `conditions`, `fields`,
/// `reason` and a false `inverted` are omitted when empty.
#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
    /// Actions this rule covers; `manage` covers every action.
    pub action: Vec<String>,
    /// Subject types this rule covers; `all` covers every subject type.
    pub subject: Vec<String>,
    /// Conditions on the record, if any.
    pub conditions: Option<Conditions>,
    /// Fields this rule is limited to, if any.
    pub fields: Option<Vec<String>>,
    /// Whether this is a `cannot` rule.
    pub inverted: bool,
    /// Why the rule exists (shown when it denies).
    pub reason: Option<String>,
}

impl Rule {
    /// An unconditional rule granting `action` on `subject`.
    pub fn allow(action: &str, subject: &str) -> Self {
        Rule {
            action: vec![action.to_owned()],
            subject: vec![subject.to_owned()],
            conditions: None,
            fields: None,
            inverted: false,
            reason: None,
        }
    }

    /// An unconditional rule denying `action` on `subject`.
    pub fn deny(action: &str, subject: &str) -> Self {
        Rule {
            inverted: true,
            ..Rule::allow(action, subject)
        }
    }

    /// Whether the rule covers `action` (directly or through `manage`).
    pub fn matches_action(&self, action: &str) -> bool {
        self.action.iter().any(|a| a == action || a == MANAGE)
    }

    /// Whether the rule covers `subject_type` (directly or through `all`).
    pub fn matches_subject(&self, subject_type: &str) -> bool {
        self.subject.iter().any(|s| s == subject_type || s == ALL)
    }

    /// Whether the rule applies to `field` (`None`: the whole subject).
    ///
    /// As in CASL, a field-limited rule applies to a whole-subject check
    /// only when it grants: `cannot update Post fields [author_id]` does not
    /// deny updating a post, only changing its author.
    pub fn matches_field(&self, field: Option<&str>) -> bool {
        match (&self.fields, field) {
            (None, _) => true,
            (Some(_), None) => !self.inverted,
            (Some(fields), Some(field)) => fields.iter().any(|f| f == field),
        }
    }

    /// Whether the rule's conditions hold for `record` (`None`: a
    /// type-level check, where a conditional rule matches unless inverted).
    pub fn matches_conditions(&self, record: Option<&Value>) -> bool {
        match (&self.conditions, record) {
            (None, _) => true,
            (Some(_), None) => !self.inverted,
            (Some(conditions), Some(record)) => conditions.matches(record),
        }
    }

    /// Checks the rule's shape, field names and condition operands.
    pub fn validate(&self) -> Result<(), RuleError> {
        let names = |what: &str, values: &[String]| {
            if values.is_empty() || values.iter().any(String::is_empty) {
                Err(RuleError::InvalidRule(format!(
                    "`{what}` must list at least one non-empty name"
                )))
            } else {
                Ok(())
            }
        };
        names("action", &self.action)?;
        names("subject", &self.subject)?;
        if let Some(conditions) = &self.conditions {
            conditions.validate()?;
        }
        for field in self.fields.iter().flatten() {
            validate_field_name(field)?;
        }
        Ok(())
    }
}

/// Rules collected in order; the last matching rule wins. Finish with
/// [`Rules::build`] (or [`Rules::try_build`]).
#[derive(Clone, Debug, Default)]
pub struct Rules {
    rules: Vec<Rule>,
}

impl Rules {
    /// An empty rule set (denies everything).
    pub fn new() -> Self {
        Self::default()
    }

    /// Grants `action` on `subject`; refine with the returned builder.
    pub fn can(&mut self, action: &str, subject: &str) -> RuleBuilder<'_> {
        self.push(Rule::allow(action, subject))
    }

    /// Denies `action` on `subject`; refine with the returned builder. Put
    /// `cannot` rules after the `can` rules they restrict.
    pub fn cannot(&mut self, action: &str, subject: &str) -> RuleBuilder<'_> {
        self.push(Rule::deny(action, subject))
    }

    /// Appends a ready-made rule.
    pub fn push(&mut self, rule: Rule) -> RuleBuilder<'_> {
        self.rules.push(rule);
        let index = self.rules.len() - 1;
        RuleBuilder {
            rule: &mut self.rules[index],
        }
    }

    /// Adds an unconditional `can` rule per permission string
    /// (`"<prefix>.<action>"`, e.g. `posts.publish` → can `publish` `Post`).
    /// `prefix_to_subject` maps permission prefixes to subject types; a
    /// prefix it does not know stands for itself (`audit.read` → can `read`
    /// `audit`), so a route requiring that permission is granted by it. An
    /// action of `*` means `manage`. Malformed strings are skipped.
    pub fn from_permissions(
        &mut self,
        perms: &[String],
        prefix_to_subject: &BTreeMap<String, String>,
    ) {
        for perm in perms {
            if let Some((action, subject)) = parse_permission(perm, prefix_to_subject) {
                self.can(action, subject);
            }
        }
    }

    /// The rules so far, in order.
    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    /// Validates the rules into an [`Ability`], or returns the first
    /// problem found.
    pub fn try_build(self) -> Result<Ability, RuleError> {
        Ability::from_rules(self.rules)
    }

    /// Validates the rules into an [`Ability`].
    ///
    /// # Panics
    ///
    /// If a rule is invalid (an unsupported operator, a nested field, an
    /// operand of the wrong type): rules are code, so this is a programming
    /// error. Use [`Rules::try_build`] to handle it instead.
    pub fn build(self) -> Ability {
        match self.try_build() {
            Ok(ability) => ability,
            Err(error) => panic!("invalid ability rule: {error}"),
        }
    }
}

/// Splits a permission string `"<prefix>.<action>"` into (action, subject);
/// `*` as the action means `manage`, and a prefix without a subject type
/// stands for itself. `None` for malformed strings.
pub(crate) fn parse_permission<'a>(
    perm: &'a str,
    prefix_to_subject: &'a BTreeMap<String, String>,
) -> Option<(&'a str, &'a str)> {
    let (prefix, action) = perm.rsplit_once('.')?;
    if prefix.is_empty() || action.is_empty() {
        return None;
    }
    let subject = prefix_to_subject.get(prefix).map_or(prefix, String::as_str);
    let action = if action == "*" { MANAGE } else { action };
    Some((action, subject))
}

/// Refines the rule just added by [`Rules::can`] or [`Rules::cannot`].
///
/// The rule is already in the set; dropping the builder keeps it as is.
#[derive(Debug)]
pub struct RuleBuilder<'a> {
    rule: &'a mut Rule,
}

impl RuleBuilder<'_> {
    /// Limits the rule to records matching `conditions` (combined with any
    /// conditions given before).
    pub fn when(self, conditions: Conditions) -> Self {
        let combined = match self.rule.conditions.take() {
            Some(existing) => existing.and(conditions),
            None => conditions,
        };
        self.rule.conditions = Some(combined);
        self
    }

    /// Limits the rule to these fields (wire names).
    pub fn fields<I, S>(self, fields: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.rule.fields = Some(fields.into_iter().map(Into::into).collect());
        self
    }

    /// Records why the rule exists (CASL `reason`).
    pub fn because(self, reason: &str) -> Self {
        self.rule.reason = Some(reason.to_owned());
        self
    }
}
