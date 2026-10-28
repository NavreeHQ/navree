//! What each user may do. Inside a workspace (behind the `InWorkspace`
//! guard) the user's permissions are those of their workspace role: the
//! built-in ones plus `workspace.roles.<role>.permissions` in
//! `config/app.toml`.

use navree::access::{AbilityCtx, READ, Rules, field};

/// The global role whose users pass every check (the tools' operators).
pub const SUPER_ADMIN: &str = "admin";

/// The rules of one user (or guest).
pub fn define(ctx: &AbilityCtx, rules: &mut Rules) {
    ctx.grant_permissions(rules);
    let Some(user) = &ctx.user else {
        return;
    };
    rules.can(READ, "User").when(field("id").eq(user.id));
}
