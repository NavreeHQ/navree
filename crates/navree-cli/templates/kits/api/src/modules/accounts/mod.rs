//! Accounts: what AuthModule (`/api/auth/*`: sign-up, sign-in, two-factor,
//! email verification, password reset, profile and password changes) does
//! not cover: the user's active sessions, the sign-in options a front end
//! shows, and whether two-factor authentication is on (`two_factor_enabled`
//! on the user resource).
//!
//! ```text
//! accounts/
//! ├── mod.rs           # AccountsModule
//! ├── controller.rs    # /api/v1/account/sessions, /api/v1/account/providers
//! ├── service.rs       # SessionService: sessions from navree-auth's refresh tokens
//! ├── dto.rs           # response bodies
//! └── two_factor.rs    # TwoFactorStatus: `two_factor_enabled` on the user
//! (tests: tests/accounts.rs boots the whole app)
//! ```

mod controller;
pub mod dto;
mod service;
mod two_factor;

use navree::prelude::*;

pub use controller::AccountController;
pub use service::SessionService;
pub use two_factor::TwoFactorStatus;

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
    fn wire(&self, m: &mut ModuleBuilder) {
        m.user_resource_extension::<TwoFactorStatus>();
        // navree:wire
    }
}
