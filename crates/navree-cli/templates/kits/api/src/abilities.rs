//! What each user may do: ability rules, checked by `#[can]` routes,
//! `Accessible` list filters and Helm, and served to the front end at
//! `GET /api/me/abilities` so its buttons follow the same rules.
//!
//! Inside a workspace (behind the `InWorkspace` guard) the user's
//! permissions are those of their workspace role: the built-in ones plus
//! `workspace.roles.<role>.permissions` in `config/app.toml`.

use navree::access::{AbilityCtx, DELETE, READ, Rules, UPDATE, field};

/// The global role whose users pass every check: the super admins who use
/// the admin panel at `/admin`.
pub const SUPER_ADMIN: &str = "admin";

/// The workspace role that changes only the projects it created.
const MEMBER: &str = "member";

/// The rules of one user (or guest), rebuilt when their roles change.
///
/// The permissions of the user's roles come first (`projects.update`
/// becomes `can update Project`). Add conditional rules below; later rules
/// win.
pub fn define(ctx: &AbilityCtx, rules: &mut Rules) {
    ctx.grant_permissions(rules);
    // Module policies (`navree make:policy` adds them above the marker).
    // navree:policies
    let Some(user) = &ctx.user else {
        return;
    };
    // Signed-in users may read their own account.
    rules.can(READ, "User").when(field("id").eq(user.id));
    // In a workspace, members change and delete the projects they created
    // (admins and owners may change any: `projects.*`).
    let in_workspace = ctx.team_id.is_some();
    if in_workspace && user.roles.iter().any(|role| role == MEMBER) {
        rules
            .can(UPDATE, "Project")
            .when(field("created_by").eq(user.id));
        rules
            .can(DELETE, "Project")
            .when(field("created_by").eq(user.id));
    }
}
