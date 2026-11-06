//! The users module: each signed-in user's profile at `/api/v1/profile`.
//!
//! Accounts themselves (registration, login, tokens, the `users` table) come
//! from navree's `AuthModule` (`/api/auth/*`); this module shows how a feature
//! module builds on them.
//!
//! ```text
//! users/
//! ├── mod.rs           # UsersModule: what the module provides
//! ├── controller.rs    # ProfileController: GET and PUT /api/v1/profile
//! ├── service.rs       # ProfileService: reads and writes profiles
//! ├── entity.rs        # the `user_profiles` table (an ORM model)
//! ├── dto.rs           # request and response bodies
//! ├── migrations/      # migrations owned by this module
//! └── tests/           # tests on a fresh database
//! ```

mod controller;
pub mod dto;
pub mod entity;
mod migrations;
mod service;
#[cfg(test)]
mod tests;

use navree::db::ModuleBuilderDbExt;
use navree::kernel::ModuleBuilder;
use navree::module;
use navree::orm::ModuleBuilderOrmExt;
use navree::workspace::ModuleBuilderWorkspaceExt;

pub use controller::ProfileController;
pub use entity::Profile;
pub use service::ProfileService;

/// User profiles.
#[module(
    providers = [ProfileService],
    controllers = [ProfileController],
    exports = [ProfileService],
    register = Self::wire,
)]
pub struct UsersModule;

impl UsersModule {
    /// Registrations `#[module]` has no key for (migrations, models, jobs,
    /// listeners, commands, templates). `navree make:*` adds its lines above
    /// the `navree:wire` marker.
    fn wire(&self, m: &mut ModuleBuilder) {
        m.migrations(migrations::migrations());
        m.model::<Profile>();
        // A user has one profile, the same in every workspace.
        m.global_entity::<Profile>();
        // navree:wire
    }
}
