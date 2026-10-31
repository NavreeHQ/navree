//! [`AuditFilter`]: which entries `GET /api/audit` and [`AuditLog::list`](crate::AuditLog::list) return.

use std::collections::BTreeMap;

use http::request::Parts;
use navree_contracts::AuditKind;
use navree_http::extract::FromRequestParts;
use navree_http::{AppError, Query};
use navree_orm::{Builder, Filter, Model as _};
use serde::Deserialize;
use uuid::Uuid;

use crate::AuditRecord;
use crate::log::kind_str;

/// Filters for audit entries; every field is optional and they combine
/// with AND.
///
/// As an extractor it reads
/// `?actor_id=&impersonator_id=&subject_type=&subject_id=&action=&kind=`
/// (other parameters are ignored) and rejects a malformed `actor_id` or
/// `impersonator_id` or an unknown `kind` with a 400.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AuditFilter {
    /// Entries by this user.
    pub actor_id: Option<Uuid>,
    /// Entries written while this user impersonated the actor.
    pub impersonator_id: Option<Uuid>,
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
    /// The matching entries as an ORM query, unordered: what
    /// [`AuditLog::list`](crate::AuditLog::list) pages through, newest
    /// first. Chain anything else on it: an order, a limit, a stream.
    ///
    /// ```no_run
    /// use navree_audit::{AuditFilter, AuditRecord};
    /// use navree_db::Db;
    /// # async fn demo(db: Db, user: uuid::Uuid) -> navree_orm::Result<()> {
    /// let filter = AuditFilter { actor_id: Some(user), ..AuditFilter::default() };
    /// let changes = filter.query().count(&db).await?;
    /// let latest = filter.query().latest_by(AuditRecord::CREATED_AT).first(&db).await?;
    /// # Ok(()) }
    /// ```
    pub fn query(&self) -> Builder<AuditRecord> {
        AuditRecord::query().where_(self.condition())
    }

    /// The condition of [`query`](AuditFilter::query). An action without a
    /// `.` also matches the actions ending in `.<action>`: a `LIKE` with
    /// the database's own case rules (as an exact action has), the
    /// wildcards in the action escaped with `!` (an escape character every
    /// database reads the same in a string literal).
    fn condition(&self) -> Filter {
        let mut filters = Vec::new();
        if let Some(actor_id) = self.actor_id {
            filters.push(AuditRecord::ACTOR_ID.eq(actor_id));
        }
        if let Some(impersonator_id) = self.impersonator_id {
            filters.push(AuditRecord::IMPERSONATOR_ID.eq(impersonator_id));
        }
        if let Some(subject_type) = &self.subject_type {
            filters.push(AuditRecord::SUBJECT_TYPE.eq(subject_type.as_str()));
        }
        if let Some(subject_id) = &self.subject_id {
            filters.push(AuditRecord::SUBJECT_ID.eq(subject_id.as_str()));
        }
        if let Some(action) = &self.action {
            let exact = AuditRecord::ACTION.eq(action.as_str());
            filters.push(if action.contains('.') {
                exact
            } else {
                exact.or(Filter::raw(
                    "action LIKE ? ESCAPE '!'",
                    [format!("%.{}", escape_like(action))],
                ))
            });
        }
        if let Some(kind) = self.kind {
            filters.push(AuditRecord::KIND.eq(kind_str(kind)));
        }
        Filter::all(filters)
    }
}

/// `value` with the LIKE wildcards `%` and `_` (and the escape `!`) escaped.
fn escape_like(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for c in value.chars() {
        if matches!(c, '%' | '_' | '!') {
            escaped.push('!');
        }
        escaped.push(c);
    }
    escaped
}

/// The raw query parameters.
#[derive(Deserialize)]
struct RawFilter {
    actor_id: Option<String>,
    impersonator_id: Option<String>,
    subject_type: Option<String>,
    subject_id: Option<String>,
    action: Option<String>,
    kind: Option<String>,
}

impl FromRequestParts for AuditFilter {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        let Query(raw) = Query::<RawFilter>::try_from_uri(&parts.uri)
            .map_err(|rejection| AppError::bad_request(rejection.body_text()))?;
        let mut errors = BTreeMap::new();
        let present = |v: Option<String>| v.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty());
        let mut uuid = |field: &str, value: Option<String>| {
            present(value).and_then(|id| match Uuid::parse_str(&id) {
                Ok(id) => Some(id),
                Err(_) => {
                    errors.insert(field.to_owned(), vec![format!("{field} must be a UUID")]);
                    None
                }
            })
        };
        let actor_id = uuid("actor_id", raw.actor_id);
        let impersonator_id = uuid("impersonator_id", raw.impersonator_id);
        let kind = present(raw.kind).and_then(|kind| match parse_kind(&kind) {
            Some(kind) => Some(kind),
            None => {
                errors.insert(
                    "kind".to_owned(),
                    vec![
                        "kind must be one of the following values: data, auth, access, security, custom"
                            .to_owned(),
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
            impersonator_id,
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
    #[test]
    fn handlers_on_the_filter_are_native() {
        async fn handler(_: super::AuditFilter) {}
        assert!(navree_http::handler::Endpoint::new(handler).is_native());
    }

    use super::*;

    #[test]
    fn like_wildcards_are_escaped() {
        assert_eq!(escape_like("role_assigned"), "role!_assigned");
        assert_eq!(escape_like("100%!"), "100!%!!");
        assert_eq!(escape_like("a\\b"), "a\\b");
    }

    #[test]
    fn kinds_parse_case_insensitively() {
        assert_eq!(parse_kind("security"), Some(AuditKind::Security));
        assert_eq!(parse_kind("DATA"), Some(AuditKind::Data));
        assert_eq!(parse_kind("other"), None);
    }
}
