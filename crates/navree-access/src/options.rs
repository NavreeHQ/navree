//! [`AccessOptions`]: how the access module builds abilities.

use crate::context::AbilityCtx;
use crate::engine::Rules;

/// Defines the rules of one principal: called with the principal's
/// [`AbilityCtx`] whenever their abilities are (re)built.
pub type AbilitiesFn = fn(&AbilityCtx, &mut Rules);

/// The access module's settings, passed to
/// [`AccessModule::for_root`](crate::AccessModule::for_root).
///
/// ```
/// use navree_access::{AbilityCtx, AccessOptions, READ, Rules};
///
/// fn define(ctx: &AbilityCtx, rules: &mut Rules) {
///     ctx.grant_permissions(rules);
///     rules.can(READ, "Post");
/// }
///
/// let options = AccessOptions {
///     abilities: define,
///     super_admin_role: Some("admin".into()),
/// };
/// assert_eq!(options.super_admin_role.as_deref(), Some("admin"));
/// ```
#[derive(Clone, Debug)]
pub struct AccessOptions {
    /// Defines each principal's rules (default: [`default_abilities`], the
    /// user's permissions only).
    pub abilities: AbilitiesFn,
    /// Users with this role pass every check: a final `can manage all` rule
    /// is added after the rules `abilities` defines, and every role check
    /// passes (default `None`).
    pub super_admin_role: Option<String>,
}

impl Default for AccessOptions {
    fn default() -> Self {
        Self {
            abilities: default_abilities,
            super_admin_role: None,
        }
    }
}

/// The default abilities: one unconditional rule per permission the user
/// holds (see [`AbilityCtx::grant_permissions`]); guests get nothing.
pub fn default_abilities(ctx: &AbilityCtx, rules: &mut Rules) {
    ctx.grant_permissions(rules);
}
