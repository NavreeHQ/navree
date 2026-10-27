//! Accounts: what AuthModule (`/api/auth/*`: sign-up, sign-in, two-factor,
//! email verification, password reset, profile and password changes) does
//! not cover: the user's active sessions, and the sign-in options a front
//! end shows.
//!
//! ```text
//! accounts/
//! ├── mod.rs           # AccountsModule
//! ├── controller.rs    # /api/v1/account/sessions, /api/v1/account/providers
//! ├── service.rs       # SessionService: sessions from navree-auth's refresh tokens
//! └── dto.rs           # response bodies
//! (tests: tests/accounts.rs boots the whole app)
//! ```

mod controller;
pub mod dto;
mod service;

use navree::prelude::*;

pub use controller::AccountController;
pub use service::SessionService;

/// Sessions and sign-in options.
#[module(
    providers = [SessionService],
    controllers = [AccountController],
    exports = [SessionService],
    register = Self::wire,
)]
pub struct AccountsModule;

impl AccountsModule {
    /// Registrations `#[module]` has no key for. `navree make:*` adds its
    /// lines above the `navree:wire` marker.
    fn wire(&self, _m: &mut ModuleBuilder) {
        // navree:wire
    }
}
