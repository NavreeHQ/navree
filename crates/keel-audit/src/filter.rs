//! [`AuditFilter`]: which entries `GET /api/audit` and [`AuditLog::list`](crate::AuditLog::list) return.

use std::collections::BTreeMap;

use axum::extract::{FromRequestParts, Query};
use axum::http::request::Parts;
use keel_contracts::AuditKind;
use keel_http::AppError;
use sea_orm::sea_query::LikeExpr;
use sea_orm::{ColumnTrait, Condition, QueryFilter, Select};
use serde::Deserialize;
use uuid::Uuid;

use crate::audit_log::{self, Column};
use crate::log::kind_str;

/// Filters for audit entries; every field is optional and they combine
/// with AND.
///
/// As an extractor it reads `?actor_id=&subject_type=&subject_id=&action=&kind=`
/// (other parameters are ignored) and rejects a malformed `actor_id` or an
/// unknown `kind` with a 422.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AuditFilter {
    /// Entries by this user.
    pub actor_id: Option<Uuid>,
    /// Entries about this record type (`Deal`).
    pub subject_type: Option<String>,
    /// Entries about this record id.
    pub subject_id: Option<String>,
    /// Entries with this action: exactly (`deal.updated`), or, without a
    /// `.`, any action ending in `.<action>` too (`updated` matches
    /// `deal.updated` and `company.updated`).
    pub action: Option<String>,
    /// Entries of this kind.
    pub kind: Option<AuditKind>,
}

impl AuditFilter {
    /// `select` restricted to the matching entries.
    pub fn apply(&self, select: Select<audit_log::Entity>) -> Select<audit_log::Entity> {
        let mut condition = Condition::all();
        if let Some(actor_id) = self.actor_id {
            condition = condition.add(Column::ActorId.eq(actor_id));
        }
        if let Some(subject_type) = &self.subject_type {
            condition = condition.add(Column::SubjectType.eq(subject_type.as_str()));
        }
        if let Some(subject_id) = &self.subject_id {
            condition = condition.add(Column::SubjectId.eq(subject_id.as_str()));
        }
        if let Some(action) = &self.action {
            condition = condition.add(if action.contains('.') {
                Condition::all().add(Column::Action.eq(action.as_str()))
            } else {
                Condition::any()
                    .add(Column::Action.eq(action.as_str()))
                    .add(
                        Column::Action
                            .like(LikeExpr::new(format!("%.{}", escape_like(action))).escape('\\')),
                    )
            });
        }
        if let Some(kind) = self.kind {
            condition = condition.add(Column::Kind.eq(kind_str(kind)));
        }
        select.filter(condition)
    }
}

/// `value` with the LIKE wildcards `%` and `_` (and the escape `\`) escaped.
fn escape_like(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for c in value.chars() {
        if matches!(c, '%' | '_' | '\\') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

/// The raw query parameters.
#[derive(Deserialize)]
struct RawFilter {
    actor_id: Option<String>,
    subject_type: Option<String>,
    subject_id: Option<String>,
    action: Option<String>,
    kind: Option<String>,
}

impl<S: Send + Sync> FromRequestParts<S> for AuditFilter {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let Query(raw) = Query::<RawFilter>::try_from_uri(&parts.uri)
            .map_err(|rejection| AppError::bad_request(rejection.body_text()))?;
        let mut errors = BTreeMap::new();
        let present = |v: Option<String>| v.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty());
        let actor_id = present(raw.actor_id).and_then(|id| match Uuid::parse_str(&id) {
            Ok(id) => Some(id),
            Err(_) => {
                errors.insert(
                    "actor_id".to_owned(),
                    vec!["The actor id must be a valid UUID.".to_owned()],
                );
                None
            }
        });
        let kind = present(raw.kind).and_then(|kind| match parse_kind(&kind) {
            Some(kind) => Some(kind),
            None => {
                errors.insert(
                    "kind".to_owned(),
                    vec![
                        "The kind must be one of: data, auth, access, security, custom.".to_owned(),
                    ],
                );
                None
            }
        });
        if !errors.is_empty() {
            return Err(AppError::validation(errors));
        }
        Ok(AuditFilter {
            actor_id,
            subject_type: present(raw.subject_type),
            subject_id: present(raw.subject_id),
            action: present(raw.action),
            kind,
        })
    }
}

/// The kind named `name` (`data`, `auth`, `access`, `security`, `custom`).
fn parse_kind(name: &str) -> Option<AuditKind> {
    [
        AuditKind::Data,
        AuditKind::Auth,
        AuditKind::Access,
        AuditKind::Security,
        AuditKind::Custom,
    ]
    .into_iter()
    .find(|kind| kind_str(*kind).eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn like_wildcards_are_escaped() {
        assert_eq!(escape_like("role_assigned"), "role\\_assigned");
        assert_eq!(escape_like("100%\\"), "100\\%\\\\");
    }

    #[test]
    fn kinds_parse_case_insensitively() {
        assert_eq!(parse_kind("security"), Some(AuditKind::Security));
        assert_eq!(parse_kind("DATA"), Some(AuditKind::Data));
        assert_eq!(parse_kind("other"), None);
    }
}
