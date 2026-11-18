//! Subjects: the permission prefix map, record loaders for `can(.., from)`
//! checks, and the record form ([`row_json`]) that keeps record checks and
//! list filters in step.

use std::any::Any;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use navree_db::{Record, Uow};
use navree_kernel::{AppContext, BoxError, BoxFuture, PluginInfo};
use serde_json::Value;

use crate::catalog::{community_plugin, is_under};
use crate::engine::{ALL, Rule, Subject, value_json};
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
    /// The registered policies, in module order, each once.
    pub(crate) policies: Vec<RegisteredPolicy>,
}

/// A registered policy, and the plugin it acts for when the kernel does not
/// find that plugin first-party.
pub(crate) struct RegisteredPolicy {
    /// Adds the policy's rules ([`Policy::define`](crate::Policy::define)).
    pub(crate) define: fn(&crate::AbilityCtx, &mut crate::Rules),
    /// The plugin, when every module that registers the policy acts for a
    /// community plugin; `None` for a policy of the app.
    pub(crate) plugin: Option<PluginPolicy>,
}

/// A policy of a community plugin: what its rules may name.
pub(crate) struct PluginPolicy {
    /// The plugin's name.
    pub(crate) plugin: &'static str,
    /// The policy's type name.
    pub(crate) policy: &'static str,
    /// The first module that registers it.
    pub(crate) module: &'static str,
    /// Why `module` acts for `plugin` when it does not name it itself.
    pub(crate) module_reason: String,
    /// The subjects its rules may name.
    pub(crate) own: OwnSubjects,
    /// Attribution added to each kept rule's `reason`: the policy and its
    /// plugin.
    reason: String,
    /// The subjects of the rules left out so far, each logged once.
    logged: Mutex<BTreeSet<String>>,
}

impl PluginPolicy {
    /// The policy `policy` that `module`, a module acting for `plugin`,
    /// registers.
    fn new(
        plugin: &PluginInfo,
        policy: &'static str,
        module: &'static str,
        module_reason: String,
    ) -> Self {
        PluginPolicy {
            plugin: plugin.name,
            policy,
            module,
            module_reason,
            own: OwnSubjects {
                namespace: plugin.namespace,
            },
            reason: format!("policy `{policy}` of plugin `{}`", plugin.name),
            logged: Mutex::default(),
        }
    }

    /// Calls `define` into rules of its own and adds to `rules`, in order,
    /// each rule whose every subject is one of the plugin's own, naming
    /// this policy and its plugin in `reason` (beside the policy's own
    /// `because` words when it gave any). Any other rule is left out, and
    /// each subject of it that is not the plugin's own is logged once, at
    /// target `navree::security`.
    pub(crate) fn copy_own_rules(
        &self,
        define: fn(&crate::AbilityCtx, &mut crate::Rules),
        ctx: &crate::AbilityCtx,
        rules: &mut crate::Rules,
    ) {
        let mut written = crate::Rules::new();
        define(ctx, &mut written);
        for mut rule in written.into_rules() {
            if self.outside(&rule).is_none() {
                rule.reason = Some(
                    match rule.reason.as_deref().filter(|words| !words.is_empty()) {
                        Some(words) => format!("{words} ({})", self.reason),
                        None => self.reason.clone(),
                    },
                );
                rules.push(rule);
                continue;
            }
            for subject in &rule.subject {
                if !self.own.contains(subject) {
                    self.log_left_out(subject);
                }
            }
        }
    }

    /// Logs that a rule on `subject` was left out, the first time only.
    fn log_left_out(&self, subject: &str) {
        let first = {
            let mut seen = self.logged.lock().unwrap_or_else(PoisonError::into_inner);
            !seen.contains(subject) && seen.insert(subject.to_owned())
        };
        if first {
            tracing::warn!(
                target: "navree::security",
                "left out a rule of plugin `{}`'s policy `{}` on `{subject}`, not one of its own subjects",
                self.plugin,
                self.policy
            );
        }
    }

    /// The first subject of `rule` that is not one of the plugin's own
    /// (`all` included), if any.
    pub(crate) fn outside<'r>(&self, rule: &'r Rule) -> Option<&'r str> {
        rule.subject
            .iter()
            .map(String::as_str)
            .find(|subject| !self.own.contains(subject))
    }
}

/// The subjects a plugin's policy may write rules on: the names under the
/// plugin's namespace (the namespace, or the namespace, `.` and a name),
/// never `all`.
pub(crate) struct OwnSubjects {
    namespace: &'static str,
}

impl OwnSubjects {
    /// Whether `subject` is one of the plugin's own.
    pub(crate) fn contains(&self, subject: &str) -> bool {
        subject != ALL && is_under(subject, self.namespace)
    }
}

impl Subjects {
    /// Collects the subjects and policies contributed by the modules of `ctx`.
    ///
    /// A policy name registered by more than one module is added once, in the
    /// order it is first seen. When any module that registers it acts for no
    /// community plugin, it is the app's policy and every rule of it applies;
    /// when every module that registers it acts for a community plugin, the
    /// first plugin's own subjects apply, even when a module of another
    /// community plugin registers the same policy name later.
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
        let registrations = ctx.contributions::<crate::records::PolicyRegistration>();
        // The plugins listing, read only when a policy is registered.
        let plugins = if registrations.is_empty() {
            Vec::new()
        } else {
            ctx.plugins().plugins
        };
        let mut policies: Vec<RegisteredPolicy> = Vec::new();
        // Policy name → its index in `policies`.
        let mut seen: HashMap<&'static str, usize> = HashMap::new();
        for contributed in &registrations {
            let registration = &contributed.value;
            let plugin = community_plugin(&plugins, contributed.module.id);
            match seen.get(registration.name) {
                // A policy registered by two modules applies once: it becomes
                // the app's as soon as one of them acts for no community
                // plugin, and between two community plugins keeps the first
                // plugin's subjects.
                Some(&index) => {
                    if plugin.is_none() {
                        policies[index].plugin = None;
                    }
                }
                None => {
                    seen.insert(registration.name, policies.len());
                    policies.push(RegisteredPolicy {
                        define: registration.define,
                        plugin: plugin.map(|plugin| {
                            PluginPolicy::new(
                                &plugin.info,
                                registration.name,
                                contributed.module.name,
                                navree_kernel::__private::acts_for_reason(
                                    ctx,
                                    contributed.module.id,
                                ),
                            )
                        }),
                    });
                }
            }
        }
        Ok(Subjects {
            prefixes: Arc::new(prefixes),
            loaders,
            policies,
        })
    }

    /// Whether a policy acts for a community plugin.
    pub(crate) fn has_plugin_policies(&self) -> bool {
        self.policies.iter().any(|policy| policy.plugin.is_some())
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
