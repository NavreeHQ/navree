//! What each user may do, checked by `#[can]` routes and `Accessible` list
//! filters and served to the front end at `GET /api/me/abilities`.

use navis::prelude::*;

use super::posts::Publish;

/// Everyone (guests included) reads published posts; signed-in users also
/// read, edit and publish their own, and write new ones. Later rules win.
pub fn define(ctx: &AbilityCtx, rules: &mut Rules) {
    ctx.grant_permissions(rules);
    rules.can(READ, "Post").when(field("published").eq(true));
    let Some(user) = &ctx.user else {
        return;
    };
    rules.can(READ, "Post").when(field("author_id").eq(user.id));
    rules.can(CREATE, "Post");
    rules
        .can(UPDATE, "Post")
        .when(field("author_id").eq(user.id));
    rules
        .can(Publish::NAME, "Post")
        .when(field("author_id").eq(user.id));
}
