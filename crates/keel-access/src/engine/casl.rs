//! CASL JSON: the rule format `createMongoAbility` in `@casl/ability`
//! accepts, for [`Rule`], [`Conditions`] and [`Ability`].
//!
//! A rule is `{"action", "subject", "conditions"?, "fields"?, "inverted"?,
//! "reason"?}`. `action` and `subject` are a string or an array; `fields` is
//! written as an array and read as a string or an array; `conditions` is a
//! MongoDB-style query object, `{"field": value | {"$op": value, ...}}`.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

use super::ability::Ability;
use super::condition::{Cond, Conditions};
use super::rule::{Rule, RuleError};

impl Ability {
    /// The rules as CASL's JSON rule array, in order.
    pub fn to_casl_json(&self) -> Value {
        Value::Array(self.rules().iter().map(Rule::to_casl_json).collect())
    }

    /// Parses and validates CASL's JSON rule array.
    ///
    /// Only the operators Keel can also compile to SQL are accepted; `$exists`,
    /// `$regex`, `$or`, nested fields and the like are rejected.
    pub fn from_casl_json(json: &Value) -> Result<Ability, RuleError> {
        let Value::Array(items) = json else {
            return Err(RuleError::InvalidRule(
                "CASL rules must be a JSON array".to_owned(),
            ));
        };
        let rules = items
            .iter()
            .map(Rule::from_casl_json)
            .collect::<Result<Vec<_>, _>>()?;
        Ability::from_rules(rules)
    }
}

impl Rule {
    /// The rule as one CASL JSON rule object.
    pub fn to_casl_json(&self) -> Value {
        let mut rule = Map::new();
        rule.insert("action".to_owned(), one_or_many(&self.action));
        rule.insert("subject".to_owned(), one_or_many(&self.subject));
        if let Some(conditions) = &self.conditions {
            rule.insert("conditions".to_owned(), conditions.to_json());
        }
        if let Some(fields) = &self.fields {
            rule.insert("fields".to_owned(), Value::from(fields.clone()));
        }
        if self.inverted {
            rule.insert("inverted".to_owned(), Value::Bool(true));
        }
        if let Some(reason) = &self.reason {
            rule.insert("reason".to_owned(), Value::String(reason.clone()));
        }
        Value::Object(rule)
    }

    /// Parses and validates one CASL JSON rule object.
    pub fn from_casl_json(json: &Value) -> Result<Rule, RuleError> {
        let raw = RawRule::deserialize(json).map_err(|e| RuleError::InvalidRule(e.to_string()))?;
        let rule = Rule {
            action: raw.action.into_vec(),
            subject: raw.subject.into_vec(),
            conditions: raw
                .conditions
                .as_ref()
                .map(Conditions::from_json)
                .transpose()?,
            fields: raw.fields.map(OneOrMany::into_vec),
            inverted: raw.inverted,
            reason: raw.reason,
        };
        rule.validate()?;
        Ok(rule)
    }
}

fn one_or_many(values: &[String]) -> Value {
    match values {
        [one] => Value::String(one.clone()),
        many => Value::from(many.to_vec()),
    }
}

/// A CASL rule as it appears on the wire, before validation.
#[derive(Deserialize)]
struct RawRule {
    action: OneOrMany,
    subject: OneOrMany,
    #[serde(default)]
    conditions: Option<Value>,
    #[serde(default)]
    fields: Option<OneOrMany>,
    #[serde(default)]
    inverted: bool,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

impl OneOrMany {
    fn into_vec(self) -> Vec<String> {
        match self {
            OneOrMany::One(one) => vec![one],
            OneOrMany::Many(many) => many,
        }
    }
}

impl Conditions {
    /// The conditions as a CASL (MongoDB-style) query object. A lone `$eq`
    /// is written in its short form, `{"field": value}`.
    pub fn to_json(&self) -> Value {
        let query = self
            .iter()
            .map(|(field, conds)| {
                let value = match conds {
                    [Cond::Eq(value)] if !matches!(value, Value::Array(_) | Value::Object(_)) => {
                        value.clone()
                    }
                    conds => Value::Object(
                        conds
                            .iter()
                            .map(|cond| (cond.operator().to_owned(), cond.operand()))
                            .collect(),
                    ),
                };
                (field.to_owned(), value)
            })
            .collect();
        Value::Object(query)
    }

    /// Parses and validates a CASL (MongoDB-style) query object.
    pub fn from_json(query: &Value) -> Result<Conditions, RuleError> {
        let Value::Object(query) = query else {
            return Err(RuleError::InvalidRule(
                "conditions must be a JSON object".to_owned(),
            ));
        };
        let mut conditions = Conditions::new();
        for (field, value) in query {
            if field.starts_with('$') {
                return Err(RuleError::UnsupportedOperator(field.clone()));
            }
            match value {
                Value::Object(ops) if ops.is_empty() => {
                    return Err(RuleError::InvalidValue {
                        field: field.clone(),
                        operator: "$eq",
                        reason: "an empty operator object".to_owned(),
                    });
                }
                Value::Object(ops) if ops.keys().all(|op| op.starts_with('$')) => {
                    for (operator, operand) in ops {
                        let cond = Cond::from_operator(field, operator, operand.clone())?;
                        conditions = conditions.with(field, cond);
                    }
                }
                // Shorthand equality (an object here is a nested document,
                // which validation rejects as a non-scalar operand).
                value => conditions = conditions.with(field, Cond::Eq(value.clone())),
            }
        }
        conditions.validate()?;
        Ok(conditions)
    }
}

impl Serialize for Conditions {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_json().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Conditions {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let query = Value::deserialize(deserializer)?;
        Conditions::from_json(&query).map_err(serde::de::Error::custom)
    }
}

impl Serialize for Rule {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_casl_json().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Rule {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let json = Value::deserialize(deserializer)?;
        Rule::from_casl_json(&json).map_err(serde::de::Error::custom)
    }
}

/// Serializes as CASL's JSON rule array ([`Ability::to_casl_json`]).
impl Serialize for Ability {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_casl_json().serialize(serializer)
    }
}
