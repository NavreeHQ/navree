//! [`AbilityFactory`]: builds (and caches) each principal's abilities.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use navree_bounded::Cache;
use navree_contracts::Principal;
use navree_db::{Db, Uow};
use navree_http::{AccessSpec, AppError};
use uuid::Uuid;

use crate::authorizer::AuthorizedRecord;
use crate::context::{AbilityCtx, AccessUser};
use crate::engine::{ALL, Ability, MANAGE, Rules};
use crate::error::AccessError;
use crate::options::AccessOptions;
use crate::store;
use crate::subject::Subjects;

/// What one principal may do: their roles, permissions and compiled
/// [`Ability`], at access version `version`.
#[derive(Clone, Debug)]
pub struct PrincipalAccess {
    /// The user; `None` for guests.
    pub user: Option<Uuid>,
    /// The user's role names, sorted.
    pub roles: Vec<String>,
    /// The user's permissions (through roles and directly), sorted.
    pub permissions: Vec<String>,
    /// The access version the abilities were built at.
    pub version: i64,
    /// Whether the user has the super-admin role.
    pub super_admin: bool,
    /// The compiled rules (restricted to the token's abilities for token
    /// requests).
    pub ability: Arc<Ability>,
}

impl PrincipalAccess {
    /// Whether the user has `role`.
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }
}

/// How many principals' abilities are kept in memory.
const CACHE_CAPACITY: usize = 10_000;

/// How long an unused cached ability is kept.
const CACHE_IDLE: Duration = Duration::from_secs(10 * 60);

/// The cache key: user, team and access version. Every role or permission
/// change bumps the version, so stale entries are never read again (and
/// age out); no invalidation message is needed between nodes.
type CacheKey = (Uuid, Option<Uuid>, i64);

/// Builds each principal's [`Ability`] from the abilities function, their
/// roles and permissions. Provided and exported by the access module.
///
/// Each lookup reads the access version (one small query); the user's
/// roles, permissions and compiled rules are cached in memory under (user,
/// team, version), so they are rebuilt only after a change. Token requests
/// get the cached ability restricted to the token's abilities.
pub struct AbilityFactory {
    db: Arc<Db>,
    options: Arc<AccessOptions>,
    subjects: Arc<Subjects>,
    cache: Cache<CacheKey, Arc<PrincipalAccess>>,
}

impl AbilityFactory {
    pub(crate) fn new(db: Arc<Db>, options: Arc<AccessOptions>, subjects: Arc<Subjects>) -> Self {
        Self {
            db,
            options,
            subjects,
            cache: Cache::builder(CACHE_CAPACITY)
                .time_to_idle(CACHE_IDLE)
                .build(),
        }
    }

    /// The abilities of `principal` (`None`: a guest).
    pub async fn for_principal(
        &self,
        principal: &Option<Principal>,
    ) -> Result<Arc<Ability>, AccessError> {
        Ok(self.access_for(principal.as_ref()).await?.ability.clone())
    }

    /// The roles, permissions and abilities of `principal` (`None`: a guest).
    pub async fn access_for(
        &self,
        principal: Option<&Principal>,
    ) -> Result<Arc<PrincipalAccess>, AccessError> {
        let Some(principal) = principal else {
            return self.build(None, Vec::new(), Vec::new(), 0).map(Arc::new);
        };
        let user = principal.user_id;
        let version = store::version(self.db.conn()).await?;
        let access = self
            .cache
            .try_get_with((user, None, version), self.load(user, version))
            .await
            .map_err(|error| {
                Arc::try_unwrap(error)
                    .unwrap_or_else(|shared| AccessError::Config(shared.to_string()))
            })?;
        Ok(match &principal.token_abilities {
            Some(abilities) => Arc::new(PrincipalAccess {
                ability: Arc::new(
                    access
                        .ability
                        .restrict_to(abilities, &self.subjects.prefixes),
                ),
                ..(*access).clone()
            }),
            None => access,
        })
    }

    /// Whether `principal` holds `permission` (`<prefix>.<action>`) app-wide,
    /// checked the way a route's `permissions` declaration checks it: an
    /// unconditional rule grants it (`prefix.*` and the super-admin role
    /// grant every action).
    pub async fn has_permission(
        &self,
        principal: &Principal,
        permission: &str,
    ) -> Result<bool, AccessError> {
        let access = self.access_for(Some(principal)).await?;
        let (action, subject) = self.subjects.permission_target(permission);
        Ok(access.ability.has_unconditional(action, subject))
    }

    /// Forgets every cached ability (they are rebuilt on next use). Changes
    /// made through [`AccessService`](crate::AccessService) never need this.
    pub fn clear_cache(&self) {
        self.cache.invalidate_all();
    }

    /// Reads `user`'s roles and permissions and builds their abilities.
    async fn load(&self, user: Uuid, version: i64) -> Result<Arc<PrincipalAccess>, AccessError> {
        let conn = self.db.conn();
        let roles = store::roles_of(conn, user).await?;
        let permissions = store::permissions_of(conn, user).await?;
        Ok(Arc::new(self.build(
            Some(user),
            roles,
            permissions,
            version,
        )?))
    }

    /// Permission prefix → subject type, from every registered subject.
    pub fn permission_subjects(&self) -> &Arc<BTreeMap<String, String>> {
        &self.subjects.prefixes
    }

    /// The (action, subject type) a permission string checks:
    /// `posts.publish` → (`publish`, `Post`), `posts.*` → (`manage`,
    /// `Post`); an unregistered prefix stands for itself (`reports.view` →
    /// (`view`, `reports`)).
    pub fn permission_target<'a>(&'a self, permission: &'a str) -> (&'a str, &'a str) {
        self.subjects.permission_target(permission)
    }

    /// Whether `access` passes the roles, permissions and type-level `can`
    /// of an access declaration, as the route authorizer checks them before
    /// it loads a record: any of the roles (the super-admin role passes),
    /// every permission as an unconditional rule, and the ability on the
    /// subject type. `public` and `authenticated` are the caller's to
    /// check; a `can` with `from` needs
    /// [`authorize_record`](AbilityFactory::authorize_record) as well. For
    /// checks outside an HTTP request (MCP tools, agent tools, jobs).
    pub fn allows(&self, access: &PrincipalAccess, spec: &AccessSpec) -> bool {
        if !spec.roles.is_empty()
            && !access.super_admin
            && !spec.roles.iter().any(|role| access.has_role(role))
        {
            return false;
        }
        let permissions = spec.permissions.iter().all(|permission| {
            let (action, subject) = self.permission_target(permission);
            access.ability.has_unconditional(action, subject)
        });
        permissions
            && spec
                .can
                .as_ref()
                .is_none_or(|can| access.ability.can(&can.action, &can.subject, None))
    }

    /// Loads the record of `subject` whose id is `id` in `uow` (`FOR UPDATE`
    /// where supported), with the loader registered by
    /// [`AccessModule::for_feature`](crate::AccessModule::for_feature), and
    /// checks `action` on it: what a `can(action, subject, from = "id")`
    /// route does, outside an HTTP request. A 404 when there is no such
    /// record, a 403 when `access` does not allow `action` on it, a 500 when
    /// the subject has no loader.
    pub async fn authorize_record(
        &self,
        access: &PrincipalAccess,
        uow: &Uow,
        action: &str,
        subject: &str,
        id: &str,
    ) -> Result<AuthorizedRecord, AppError> {
        let loader = self.subjects.loader(subject).ok_or_else(|| {
            AppError::internal(format!(
                "no record loader for subject `{subject}`: import `AccessModule::for_feature::<E>()` for its entity"
            ))
        })?;
        let record = loader
            .load(uow, id)
            .await
            .map_err(AppError::internal)?
            .ok_or_else(AppError::not_found)?;
        if !access.ability.can(action, subject, Some(&record.json)) {
            return Err(AppError::forbidden());
        }
        Ok(AuthorizedRecord {
            subject: subject.to_owned(),
            action: action.to_owned(),
            json: Arc::new(record.json),
            model: record.model,
            ability: access.ability.clone(),
        })
    }

    pub(crate) fn subjects(&self) -> &Subjects {
        &self.subjects
    }

    /// Adds the rules of the principal of `ctx`: the registered policies
    /// ([`AccessModule::for_policy`](crate::AccessModule::for_policy), and
    /// the policies of subjects registered with `for_feature`), then the
    /// app's abilities function (which can override them). Code building
    /// abilities for another scope (navree-workspace's workspace roles) calls
    /// this, so policies apply there too.
    pub fn define_rules(&self, ctx: &AbilityCtx, rules: &mut Rules) {
        for define in &self.subjects.policies {
            define(ctx, rules);
        }
        (self.options.abilities)(ctx, rules);
    }

    fn build(
        &self,
        user: Option<Uuid>,
        roles: Vec<String>,
        permissions: Vec<String>,
        version: i64,
    ) -> Result<PrincipalAccess, AccessError> {
        let ctx = AbilityCtx {
            user: user.map(|id| AccessUser {
                id,
                roles: roles.clone(),
            }),
            permissions: permissions.clone(),
            team_id: None,
            organization_id: None,
            permission_subjects: self.subjects.prefixes.clone(),
        };
        let super_admin = self
            .options
            .super_admin_role
            .as_ref()
            .is_some_and(|role| ctx.has_role(role));
        let mut rules = Rules::new();
        self.define_rules(&ctx, &mut rules);
        if super_admin {
            // Last, so it wins over every `cannot` rule.
            rules.can(MANAGE, ALL).because("super admin");
        }
        let ability = rules.try_build()?;
        Ok(PrincipalAccess {
            user,
            roles,
            permissions,
            version,
            super_admin,
            ability: Arc::new(ability),
        })
    }
}

impl fmt::Debug for AbilityFactory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AbilityFactory")
            .field("options", &self.options)
            .field("subjects", &self.subjects.prefixes)
            .finish_non_exhaustive()
    }
}
