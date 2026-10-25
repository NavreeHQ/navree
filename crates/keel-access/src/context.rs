//! [`AbilityCtx`]: what an abilities function knows about the principal.

use std::collections::BTreeMap;
use std::sync::Arc;

use uuid::Uuid;

use crate::engine::Rules;

/// The input of an abilities function ([`AccessOptions::abilities`](crate::AccessOptions::abilities)):
/// the signed-in user (if any) with their roles, the permissions granted to
/// them through roles and directly, and the current team.
///
/// ```
/// use keel_access::{AbilityCtx, MANAGE, ALL, READ, Rules, field};
///
/// fn define(ctx: &AbilityCtx, rules: &mut Rules) {
///     let Some(user) = &ctx.user else {
///         rules.can(READ, "Post").when(field("published").eq(true)); // guests
///         return;
///     };
///     if user.has_role("admin") {
///         rules.can(MANAGE, ALL);
///         return;
///     }
///     ctx.grant_permissions(rules); // e.g. posts.publish from database roles
///     rules.can(READ, "Post");
/// }
///
/// let user = uuid::Uuid::now_v7();
/// let mut rules = Rules::new();
/// define(&AbilityCtx::for_user(user, vec!["admin".into()], vec![]), &mut rules);
/// assert!(rules.build().can("delete", "Invoice", None));
/// ```
#[derive(Clone, Debug, Default)]
pub struct AbilityCtx {
    /// The signed-in user; `None` for guests.
    pub user: Option<AccessUser>,
    /// The user's permission strings (`posts.publish`), from their roles and
    /// direct grants, sorted.
    pub permissions: Vec<String>,
    /// The current team (Phase 2; always `None` for now).
    pub team_id: Option<Uuid>,
    /// Permission prefixes and the subject types they map to (`posts` →
    /// `Post`), from every registered subject; used by
    /// [`grant_permissions`](AbilityCtx::grant_permissions).
    pub permission_subjects: Arc<BTreeMap<String, String>>,
}

impl AbilityCtx {
    /// The context of a guest.
    pub fn guest() -> Self {
        Self::default()
    }

    /// The context of user `id` with `roles` and `permissions`.
    pub fn for_user(id: Uuid, roles: Vec<String>, permissions: Vec<String>) -> Self {
        Self {
            user: Some(AccessUser { id, roles }),
            permissions,
            ..Self::default()
        }
    }

    /// Sets the permission prefix → subject type map.
    pub fn with_subjects(mut self, subjects: Arc<BTreeMap<String, String>>) -> Self {
        self.permission_subjects = subjects;
        self
    }

    /// Adds an unconditional `can` rule per permission of the user
    /// ([`Rules::from_permissions`] with [`permission_subjects`](AbilityCtx::permission_subjects)).
    pub fn grant_permissions(&self, rules: &mut Rules) {
        rules.from_permissions(&self.permissions, &self.permission_subjects);
    }

    /// Whether there is a user and they have `role`.
    pub fn has_role(&self, role: &str) -> bool {
        self.user.as_ref().is_some_and(|user| user.has_role(role))
    }
}

/// The signed-in user of an [`AbilityCtx`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccessUser {
    /// The user's id.
    pub id: Uuid,
    /// The user's role names, sorted.
    pub roles: Vec<String>,
}

impl AccessUser {
    /// Whether the user has `role`.
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }

    /// Whether the user has any of `roles`.
    pub fn has_any_role<S: AsRef<str>>(&self, roles: &[S]) -> bool {
        roles.iter().any(|role| self.has_role(role.as_ref()))
    }
}
