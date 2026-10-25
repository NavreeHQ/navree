//! [`Ability`]: a validated rule set and its checks.

use std::collections::BTreeMap;

use serde_json::Value;

use super::rule::{Rule, RuleError, parse_permission};
use super::{ALL, MANAGE};

/// A validated, ordered rule set; share it as `Arc<Ability>`.
///
/// Checks follow CASL: rules are scanned from last to first and the first
/// one that matches the action, subject type, field and record decides;
/// no matching rule means deny. The default ability has no rules.
#[derive(Clone, Debug, Default)]
pub struct Ability {
    rules: Vec<Rule>,
}

impl Ability {
    /// Validates `rules` (in order) into an ability.
    pub fn from_rules(rules: Vec<Rule>) -> Result<Self, RuleError> {
        for rule in &rules {
            rule.validate()?;
        }
        Ok(Ability { rules })
    }

    /// The rules, in definition order.
    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    /// Whether `action` is allowed on `subject_type`.
    ///
    /// With `record: None` this is a type-level check (CASL semantics): a
    /// conditional `can` rule counts as matching ("some records may be
    /// allowed") and a conditional `cannot` rule is ignored. With a record
    /// (a JSON object of wire field names), conditions are evaluated.
    pub fn can(&self, action: &str, subject_type: &str, record: Option<&Value>) -> bool {
        self.relevant_rule(action, subject_type, record, None)
            .is_some_and(|rule| !rule.inverted)
    }

    /// `!can(...)`.
    pub fn cannot(&self, action: &str, subject_type: &str, record: Option<&Value>) -> bool {
        !self.can(action, subject_type, record)
    }

    /// Whether `action` is allowed on `field` of `subject_type` (or of
    /// `record`): rules limited to other fields do not apply.
    pub fn can_field(
        &self,
        action: &str,
        subject_type: &str,
        record: Option<&Value>,
        field: &str,
    ) -> bool {
        self.relevant_rule(action, subject_type, record, Some(field))
            .is_some_and(|rule| !rule.inverted)
    }

    /// The rule that decides the check, if any (CASL's `relevantRuleFor`):
    /// [`Ability::can`] is "a rule decides and it is not inverted". Use it to
    /// explain a decision, e.g. with the rule's `reason`.
    pub fn relevant_rule(
        &self,
        action: &str,
        subject_type: &str,
        record: Option<&Value>,
        field: Option<&str>,
    ) -> Option<&Rule> {
        self.rules.iter().rev().find(|rule| {
            rule.matches_action(action)
                && rule.matches_subject(subject_type)
                && rule.matches_field(field)
                && rule.matches_conditions(record)
        })
    }

    /// The fields of `all_fields` that `action` is allowed on, in
    /// `all_fields` order.
    pub fn permitted_fields(
        &self,
        action: &str,
        subject_type: &str,
        record: Option<&Value>,
        all_fields: &[&str],
    ) -> Vec<String> {
        all_fields
            .iter()
            .filter(|field| self.can_field(action, subject_type, record, field))
            .map(|field| (*field).to_owned())
            .collect()
    }

    /// Whether `action` is allowed on every `subject_type` record, i.e. an
    /// unconditional `can` rule grants it and no later `cannot` rule takes
    /// any record back. Used for permission (`#[permissions]`) checks.
    pub fn has_unconditional(&self, action: &str, subject_type: &str) -> bool {
        let relevant = self
            .rules
            .iter()
            .rev()
            .filter(|rule| rule.matches_action(action) && rule.matches_subject(subject_type));
        for rule in relevant {
            let conditional = rule.conditions.as_ref().is_some_and(|c| !c.is_empty());
            match (rule.inverted, rule.fields.is_some(), conditional) {
                // Field-limited denials restrict fields, not records.
                (true, true, _) => continue,
                (true, false, _) => return false,
                (false, _, false) => return true,
                // A conditional grant covers only some records; look further.
                (false, _, true) => continue,
            }
        }
        false
    }

    /// This ability narrowed to what an API token allows: each token
    /// ability is a permission string (`"<prefix>.<action>"`, see
    /// [`Rules::from_permissions`](super::Rules::from_permissions)) and `"*"`
    /// keeps everything. Unknown or malformed token abilities grant nothing.
    pub fn restrict_to(
        &self,
        token_abilities: &[String],
        prefix_to_subject: &BTreeMap<String, String>,
    ) -> Ability {
        if token_abilities.iter().any(|ability| ability == "*") {
            return self.clone();
        }
        let pairs: Vec<(&str, &str)> = token_abilities
            .iter()
            .filter_map(|ability| parse_permission(ability, prefix_to_subject))
            .collect();
        let mut rules = Vec::new();
        for rule in &self.rules {
            // One narrowed copy per distinct (actions, subjects) overlap with
            // the token; copies sit where the original was, so order (and so
            // last-match-wins) is preserved.
            let mut narrowed: Vec<(Vec<String>, Vec<String>)> = Vec::new();
            for (action, subject) in &pairs {
                let (Some(actions), Some(subjects)) = (
                    overlap(&rule.action, action, MANAGE),
                    overlap(&rule.subject, subject, ALL),
                ) else {
                    continue;
                };
                if !narrowed.contains(&(actions.clone(), subjects.clone())) {
                    narrowed.push((actions, subjects));
                }
            }
            rules.extend(narrowed.into_iter().map(|(action, subject)| Rule {
                action,
                subject,
                ..rule.clone()
            }));
        }
        Ability { rules }
    }
}

/// The part of a rule's `values` (actions or subjects) that a token's
/// `wanted` value covers, where `any` (`manage` / `all`) covers everything.
fn overlap(values: &[String], wanted: &str, any: &str) -> Option<Vec<String>> {
    if wanted == any {
        Some(values.to_vec())
    } else if values.iter().any(|v| v == wanted || v == any) {
        Some(vec![wanted.to_owned()])
    } else {
        None
    }
}
