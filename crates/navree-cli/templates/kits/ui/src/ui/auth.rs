//! Who is signed in, and the demo account.
//!
//! Signing in is not done here. `AuthPages` (navree-auth, registered in
//! `src/app.rs`) draws the sign-in, two-factor, forgotten password and reset
//! pages and answers `POST /sign-out`, on the session cookie, with the
//! throttle, two-factor authentication, the sign-in policies and the audit
//! trail of `AuthModule`. To sign in some other way, register your own pages
//! and call `AuthService::sign_in_session` from them.
//!
//! What the pages of this app need is one thing, and this file has it:
//!
//! - [`CurrentAccount`]: who is signed in. As a handler argument it answers 401
//!   for a guest, which the error pages turn into a way to the sign-in page and
//!   back. `Option<CurrentAccount>` is `None` for a guest.

use std::convert::Infallible;

use http::request::Parts;
use navree::auth::{AuthService, CurrentUser};
use navree::http::AppError;
use navree::http::extract::{FromRequestParts, OptionalFromRequestParts};
use navree::kernel::{AppContext, BoxError};
use uuid::Uuid;

/// The name of the account development starts with.
pub const DEMO_NAME: &str = "Harpreet Singh";

/// The email of the account development starts with.
pub const DEMO_EMAIL: &str = "harpreet.singh@example.com";

/// Its password. It exists in development only: the seeder that makes the
/// account does not run when `env` is `production`.
pub const DEMO_PASSWORD: &str = "demo-password-2026";

/// The signed-in account of a request.
#[derive(Clone, Debug)]
pub struct CurrentAccount {
    /// The user's id.
    pub id: Uuid,
    /// The name to show.
    pub name: String,
    /// The email address.
    pub email: String,
}

impl FromRequestParts for CurrentAccount {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, AppError> {
        let CurrentUser(user) = CurrentUser::from_request_parts(parts).await?;
        Ok(CurrentAccount {
            id: user.id,
            name: user.name,
            email: user.email,
        })
    }
}

/// `Option<CurrentAccount>` is `None` for a guest.
impl OptionalFromRequestParts for CurrentAccount {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts) -> Result<Option<Self>, Infallible> {
        Ok(<Self as FromRequestParts>::from_request_parts(parts)
            .await
            .ok())
    }
}

/// Makes the demo account if it does not exist, and gives its id. Development
/// only: the caller checks the environment first.
pub async fn ensure_demo_account(ctx: &AppContext) -> Result<Uuid, BoxError> {
    let auth = ctx.get::<AuthService>().await?;
    let user = match auth.find_by_email(DEMO_EMAIL).await? {
        Some(user) => user,
        None => {
            auth.create_user(DEMO_NAME, DEMO_EMAIL, DEMO_PASSWORD)
                .await?
        }
    };
    Ok(user.id)
}
