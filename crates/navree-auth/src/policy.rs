//! Who must use two-factor authentication: `auth.two_factor.required_for`
//! (roles) and the app's [`TwoFactorPolicy`]s (workspaces, organizations).

use std::sync::Arc;

use navree_contracts::{BoxError, BoxFuture};
use navree_kernel::WeakAppContext;
use tokio::sync::OnceCell;
use uuid::Uuid;

use crate::{AuthError, AuthOptions, AuthService, User};

/// A rule making users use two-factor authentication, beyond the roles of
/// `auth.two_factor.required_for`: a workspace or an organization that
/// requires it of its members, say. Register implementations as
/// `dyn TwoFactorPolicy` multi-bindings, most simply with
/// [`ModuleBuilderAuthExt::two_factor_policy`](crate::ModuleBuilderAuthExt::two_factor_policy).
///
/// A user any policy (or role) requires it of, who has not set it up,
/// signs in to a session limited to the setup routes: every other route
/// answers 403 `two_factor_required` until they confirm two-factor
/// authentication and refresh. Policies run when a session starts and at
/// each refresh, so a new requirement takes effect within one access-token
/// lifetime.
///
/// ```
/// use navree_auth::{TwoFactorPolicy, User};
/// use navree_contracts::{BoxError, BoxFuture};
///
/// /// Staff accounts must use two-factor authentication.
/// struct StaffPolicy;
///
/// impl TwoFactorPolicy for StaffPolicy {
///     fn requires_two_factor<'a>(&'a self, user: &'a User) -> BoxFuture<'a, Result<bool, BoxError>> {
///         Box::pin(async move { Ok(user.email.ends_with("@staff.example.com")) })
///     }
/// }
/// ```
pub trait TwoFactorPolicy: Send + Sync + 'static {
    /// Whether `user` must use two-factor authentication.
    fn requires_two_factor<'a>(&'a self, user: &'a User) -> BoxFuture<'a, Result<bool, BoxError>>;
}

/// The registered policies, resolved once.
#[derive(Default)]
pub(crate) struct Policies {
    resolved: OnceCell<Vec<Arc<dyn TwoFactorPolicy>>>,
}

impl Policies {
    /// The registered `dyn TwoFactorPolicy`s.
    pub(crate) async fn all(&self, app: &WeakAppContext) -> &[Arc<dyn TwoFactorPolicy>] {
        self.resolved
            .get_or_init(|| async {
                match app.upgrade() {
                    Some(app) => app
                        .get_all::<dyn TwoFactorPolicy>()
                        .await
                        .unwrap_or_default(),
                    None => Vec::new(),
                }
            })
            .await
    }

    /// Whether any rule exists (otherwise nothing is checked or loaded).
    pub(crate) async fn active(&self, options: &AuthOptions, app: &WeakAppContext) -> bool {
        !options.two_factor.required_for.is_empty() || !self.all(app).await.is_empty()
    }
}

impl AuthService {
    /// Whether `user` must use two-factor authentication: they hold a role
    /// of `auth.two_factor.required_for` (navree-access; the app must
    /// include `AccessModule` when it is set), or a registered
    /// [`TwoFactorPolicy`] says so.
    pub async fn two_factor_required(&self, user: &User) -> Result<bool, AuthError> {
        let required_for = &self.options.two_factor.required_for;
        if !required_for.is_empty() {
            let roles = self.roles_of(user.id).await?;
            if roles.iter().any(|role| required_for.contains(role)) {
                return Ok(true);
            }
        }
        for policy in self.policies.all(&self.app).await {
            if policy
                .requires_two_factor(user)
                .await
                .map_err(AuthError::Extension)?
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// The navree-access roles of `user_id` (`AccessModule` must be in the
    /// app).
    pub(crate) async fn roles_of(&self, user_id: Uuid) -> Result<Vec<String>, AuthError> {
        let app = self.app.upgrade().ok_or(AuthError::ShutDown)?;
        let access = app.get::<navree_access::AccessService>().await?;
        access
            .roles_of(user_id)
            .await
            .map_err(|error| AuthError::Extension(error.into()))
    }
}
