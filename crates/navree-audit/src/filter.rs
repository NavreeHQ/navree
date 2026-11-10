//! [`AuditFilter`]: which entries [`AuditLog::list`](crate::AuditLog::list)
//! returns; [`AuditList`]: the list query of `GET /api/audit`.

use navree_contracts::AuditKind;
use navree_orm::list::{Field, ListResource, ListSpec, Op};
use navree_orm::{Builder, Filter, Model as _};
use uuid::Uuid;

use crate::AuditRecord;
use crate::log::kind_str;

/// Filters for audit entries; every field is optional and they combine
/// with AND. Over HTTP the same filters are written in the list-query
/// grammar: see [`AuditList`].
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

/// The list query of `GET /api/audit`, in the list-query grammar that
/// every Navree list reads:
///
/// - `filter[actor_id]`, `filter[impersonator_id]`: a UUID; also `ne`,
///   `in`, `not_in` and `null` (`filter[actor_id][null]=true` keeps the
///   entries no signed-in user wrote).
/// - `filter[subject_type]`, `filter[subject_id]`: the record's type
///   (`Deal`) and id; also `ne`, `in`, `not_in` and `null`.
/// - `filter[action]`: exactly (`deal.updated`), or, without a `.`, any
///   action ending in `.<action>` too (`updated` matches `deal.updated`
///   and `company.updated`). `%` and `_` are plain characters.
/// - `filter[kind]`: `data`, `auth`, `access`, `security` or `custom`; also
///   `ne`, `in` and `not_in`.
/// - `filter[created_at][gte]`, `[gt]`, `[lte]`, `[lt]`: a date (the whole
///   day in UTC) or a date and time.
/// - `page` and `per_page` (default 15, at most 100).
///
/// Entries are always newest first. An unknown filter, a value of the wrong
/// kind or a `per_page` over 100 is a 400 that names the parameter.
///
/// ```
/// use navree_audit::AuditList;
/// use navree_http::list::ListResource;
///
/// let query = AuditList::SPEC
///     .parse("filter[kind][in]=auth,security&filter[created_at][gte]=2026-11-01")
///     .unwrap();
/// assert_eq!(query.filter.len(), 2);
/// assert!(AuditList::SPEC.parse("filter[actor_id]=nope").is_err());
/// ```
pub struct AuditList;

/// The operators of the audit list's text and UUID filters.
const SET_OPS: &[Op] = &[Op::Eq, Op::Ne, Op::In, Op::NotIn, Op::Null];

impl ListResource for AuditList {
    const SPEC: ListSpec = ListSpec::new(&[
        Field::uuid("actor_id"),
        Field::uuid("impersonator_id"),
        Field::text("subject_type").ops(SET_OPS),
        Field::text("subject_id").ops(SET_OPS),
        Field::text("action").ops(&[Op::Eq]).by_app(),
        Field::one_of("kind", &["data", "auth", "access", "security", "custom"]).ops(&[
            Op::Eq,
            Op::Ne,
            Op::In,
            Op::NotIn,
        ]),
        Field::datetime("created_at").ops(&[Op::Gt, Op::Gte, Op::Lt, Op::Lte]),
    ])
    .default_sort(&["-created_at"]);
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

#[cfg(test)]
mod tests {
    use navree_orm::list::List;

    use super::*;

    #[test]
    fn handlers_on_the_list_are_native() {
        async fn handler(_: List<AuditList>) {}
        let _ = navree_http::handler::Endpoint::new(handler);
    }

    #[test]
    fn the_list_reads_the_grammar_strictly() {
        let query = AuditList::SPEC
            .parse("filter[action]=updated&filter[kind][in]=auth,security&page=2&per_page=3")
            .unwrap();
        assert_eq!(query.eq("action"), Some("updated"));
        assert_eq!(query.page, Some(2));
        for (bad, param) in [
            ("filter[actor_id]=nope", "filter[actor_id]"),
            ("filter[kind]=other", "filter[kind]"),
            ("filter[action][contains]=deal", "filter[action][contains]"),
            ("filter[password]=x", "filter[password]"),
            ("sort=action", "sort"),
            ("per_page=101", "per_page"),
        ] {
            let error = AuditList::SPEC.parse(bad).unwrap_err();
            assert_eq!(error.problems[0].param, param, "{bad}");
        }
    }

    #[test]
    fn the_kind_filter_names_every_kind() {
        let field = AuditList::SPEC.field("kind").unwrap();
        for kind in [
            AuditKind::Data,
            AuditKind::Auth,
            AuditKind::Access,
            AuditKind::Security,
            AuditKind::Custom,
        ] {
            assert!(field.kind.check(kind_str(kind)).is_ok(), "{kind:?}");
        }
    }

    #[test]
    fn like_wildcards_are_escaped() {
        assert_eq!(escape_like("role_assigned"), "role!_assigned");
        assert_eq!(escape_like("100%!"), "100!%!!");
        assert_eq!(escape_like("a\\b"), "a\\b");
    }
}
