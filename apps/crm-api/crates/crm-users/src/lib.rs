//! # crm-users
//!
//! [`UsersModule`]: the CRM's user screens (spec §7).
//!
//! | Route | Access | Result |
//! |---|---|---|
//! | `GET /api/v1/me` | signed in | `{id, name, email, roles}` |
//! | `GET /api/v1/users?search=&page=&per_page=` | `read User` (everyone signed in) | page of [`UserView`] by name |
//! | `POST /api/v1/users` `{name, email, password, role}` | `create User` (admin) | 201 [`UserView`] |
//! | `PATCH /api/v1/users/{id}` `{name?, role?}` | `update User` (admin) | [`UserView`] |
//!
//! It also adds `roles` to navis-auth's user resource ([`UserRolesExtension`]),
//! so `GET /api/auth/me` and the login response's `user` carry them.
#![forbid(unsafe_code)]

mod controller;
mod dto;
mod resource;
mod service;

use crm_core::{CrmCoreModule, User};
use navis::prelude::*;

pub use controller::{MeController, UsersController};
pub use dto::{CreateUser, UpdateUser, UserFilter};
pub use resource::{Me, UserPage, UserPageMeta, UserRolesExtension, UserView, roles_by_user};
pub use service::UserService;

/// Users and their roles; see the [crate docs](crate).
#[module(
    imports = [CrmCoreModule, AccessModule::for_feature::<User>()],
    providers = [UserService],
    controllers = [MeController, UsersController],
    exports = [UserService],
    register = Self::wire,
)]
pub struct UsersModule;

impl UsersModule {
    /// The `roles` extension of the auth user resource.
    fn wire(&self, m: &mut ModuleBuilder) {
        m.user_resource_extension::<UserRolesExtension>();
    }
}
