//! The migrations of the posts module.

use navree::prelude::*;
use navree::schema::migration;

mod m20261025_000001_create_posts;

/// The module's migration set, after `auth` (posts reference `users`).
pub fn migrations() -> MigrationSet {
    MigrationSet::new("posts")
        .after(["auth"])
        .migration(migration!(m20261025_000001_create_posts))
}
