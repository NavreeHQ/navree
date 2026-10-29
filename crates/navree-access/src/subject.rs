//! Subjects: the permission prefix map, record loaders for `can(.., from)`
//! checks, and the record form ([`row_json`]) that keeps record checks and
//! list filters in step.

use std::any::Any;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::Arc;

use navree_db::{Record, Uow};
use navree_kernel::{AppContext, BoxError, BoxFuture};
use serde_json::Value;

use crate::engine::{Subject, value_json};
use crate::error::AccessError;

/// A subject registered with the access module (a module contribution).
pub(crate) struct SubjectRegistration {
    pub(crate) subject: &'static str,
    pub(crate) prefix: &'static str,
    pub(crate) type_name: &'static str,
    pub(crate) loader: Option<Arc<dyn SubjectLoader>>,
}

impl SubjectRegistration {
    /// A subject without a record loader.
    pub(crate) fn of<S: Subject + 'static>() -> Self {
        Self {
            subject: S::SUBJECT,
            prefix: S::PERMISSION_PREFIX,
            type_name: std::any::type_name::<S>(),
            loader: None,
        }
    }

    /// A subject whose records `can(.., from = "id")` routes load through
    /// [`SubjectRecords::load`](crate::SubjectRecords::load).
    pub(crate) fn records<S: crate::SubjectRecords>() -> Self {
        Self {
            loader: Some(Arc::new(crate::records::RecordsLoader::<S>::new())),
            ..Self::of::<S>()
        }
    }
}

impl fmt::Debug for SubjectRegistration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SubjectRegistration")
            .field("subject", &self.subject)
            .field("prefix", &self.prefix)
            .field("type", &self.type_name)
            .field("loader", &self.loader.is_some())
            .finish()
    }
}

/// A loaded record: its JSON form (column names → values, as conditions see
/// it) and the record itself, for `Authorized<S, A>`.
pub(crate) struct LoadedRecord {
    pub(crate) json: Value,
    pub(crate) model: Arc<dyn Any + Send + Sync>,
}

/// Loads records of one subject by the id in a path parameter.
pub(crate) trait SubjectLoader: Send + Sync + 'static {
    /// The record with primary key `id` in `uow`'s transaction (`FOR UPDATE`
    /// where the backend supports it); `None` when there is none or `id`
    /// is not a valid key.
    fn load<'a>(
        &'a self,
        uow: &'a Uow,
        id: &'a str,
    ) -> BoxFuture<'a, Result<Option<LoadedRecord>, BoxError>>;
}

/// The JSON form of navree-db record `row` that ability conditions are
/// checked against: one member per column, named like the column, so a
/// record check sees exactly what the SQL list filter (for example
/// [`TypedColumns`](crate::TypedColumns)) sees.
///
/// UUIDs become hyphenated lower-case strings and instants RFC 3339 UTC
/// strings with microseconds (`2026-10-25T12:00:00.250000Z`), the forms
/// [`IntoCondValue`](crate::IntoCondValue) gives condition operands (see
/// [`value_json`](crate::engine::value_json)).
///
/// ```
/// use navree_access::row_json;
/// use navree_access::entity::RoleRow;
/// use serde_json::json;
///
/// let role = RoleRow {
///     id: uuid::Uuid::nil(),
///     name: "editor".into(),
///     description: None,
///     created_at: "2026-10-25T12:00:00.25Z".parse().unwrap(),
/// };
/// assert_eq!(
///     row_json(&role),
///     json!({
///         "id": "00000000-0000-0000-0000-000000000000",
///         "name": "editor",
///         "description": null,
///         "created_at": "2026-10-25T12:00:00.250000Z",
///     })
/// );
/// ```
pub fn row_json<R: Record>(row: &R) -> Value {
    Value::Object(
        R::COLUMNS
            .iter()
            .zip(row.values())
            .map(|(column, value)| ((*column).to_owned(), value_json(&value)))
            .collect(),
    )
}

/// Every registered subject: permission prefixes, record loaders and
/// policies.
#[derive(Default)]
pub(crate) struct Subjects {
    /// Permission prefix → subject type.
    pub(crate) prefixes: Arc<BTreeMap<String, String>>,
    loaders: HashMap<String, Arc<dyn SubjectLoader>>,
    /// The registered policies' rule definitions, in module order.
    pub(crate) policies: Vec<fn(&crate::AbilityCtx, &mut crate::Rules)>,
}

impl Subjects {
    /// Collects the subjects contributed by the modules of `ctx`.
    pub(crate) fn collect(ctx: &AppContext) -> Result<Subjects, AccessError> {
        let mut prefixes: BTreeMap<String, String> = BTreeMap::new();
        // Subject → its registered type.
        let mut owners: HashMap<&'static str, &'static str> = HashMap::new();
        let mut loaders = HashMap::new();
        for contributed in ctx.contributions::<SubjectRegistration>() {
            let registration = &contributed.value;
            match owners.get(registration.subject).copied() {
                Some(other) if other == registration.type_name => {}
                Some(other) => {
                    return Err(AccessError::Config(format!(
                        "subject `{}` is registered by both `{other}` and `{}`",
                        registration.subject, registration.type_name
                    )));
                }
                None => {}
            }
            owners.insert(registration.subject, registration.type_name);
            match prefixes.get(registration.prefix) {
                Some(subject) if subject != registration.subject => {
                    return Err(AccessError::Config(format!(
                        "permission prefix `{}` maps to both `{subject}` and `{}`",
                        registration.prefix, registration.subject
                    )));
                }
                _ => {
                    prefixes.insert(
                        registration.prefix.to_owned(),
                        registration.subject.to_owned(),
                    );
                }
            }
            if let Some(loader) = &registration.loader {
                loaders.insert(registration.subject.to_owned(), loader.clone());
            }
        }
        let mut policies: Vec<fn(&crate::AbilityCtx, &mut crate::Rules)> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for contributed in ctx.contributions::<crate::records::PolicyRegistration>() {
            // A policy registered by two modules applies once.
            if seen.insert(contributed.value.name) {
                policies.push(contributed.value.define);
            }
        }
        Ok(Subjects {
            prefixes: Arc::new(prefixes),
            loaders,
            policies,
        })
    }

    /// The record loader of `subject`.
    pub(crate) fn loader(&self, subject: &str) -> Option<&Arc<dyn SubjectLoader>> {
        self.loaders.get(subject)
    }

    /// The (action, subject type) a permission string checks: `posts.publish`
    /// → (`publish`, `Post`); `*` means `manage`. An unknown prefix stands
    /// for itself (`audit.read` → (`read`, `audit`)), as in
    /// [`Rules::from_permissions`](crate::Rules::from_permissions), so the
    /// permission itself (or `manage all`) grants it.
    pub(crate) fn permission_target<'a>(&'a self, permission: &'a str) -> (&'a str, &'a str) {
        let (prefix, action) = permission.rsplit_once('.').unwrap_or((permission, ""));
        let action = if action == "*" { crate::MANAGE } else { action };
        let subject = self
            .prefixes
            .get(prefix)
            .map(String::as_str)
            .unwrap_or(prefix);
        (action, subject)
    }
}
