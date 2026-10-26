//! The migrations of the posts module.

use navis::prelude::*;

mod m20261025_000001_create_posts;

/// The module's migration set, after `auth` (posts reference `users`).
pub fn migrations() -> MigrationSet {
    MigrationSet::new("posts")
        .after(["auth"])
        .migration(m20261025_000001_create_posts::Migration)
}
