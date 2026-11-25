//! Sign-ins the system's own sheet does in a native app, which another module
//! adds to the sign-in page: Sign in with Apple on Apple's systems, Sign in
//! with Google on Android.
//!
//! The routes of such a sign-in belong to the module that checks its identity
//! token (navree-social), which depends on this crate and not the other way
//! round. So that module says them here, as plain data, with a
//! [`ProviderSheets`] registered as a multi-binding, and
//! [`AuthPages`](crate::AuthPages) offers each to a native client whose app
//! says it can open the sheet. A browser's sign-in page is not changed by it.
//!
//! ```
//! use std::sync::Arc;
//!
//! use navree_auth::{ProviderSheet, ProviderSheets, SheetProvider};
//! use navree_kernel::{Factory, ModuleBuilder};
//!
//! struct Sheets;
//!
//! impl ProviderSheets for Sheets {
//!     fn sheets(&self) -> Vec<ProviderSheet> {
//!         vec![ProviderSheet::new(
//!             SheetProvider::Apple,
//!             "/api/auth/social/apple/native/nonce",
//!             "/api/auth/social/apple/native/session",
//!         )]
//!     }
//! }
//!
//! fn register(m: &mut ModuleBuilder) {
//!     m.multi(Factory::<dyn ProviderSheets>::new(|_| async {
//!         Ok(Arc::new(Sheets) as Arc<dyn ProviderSheets>)
//!     }));
//! }
//! # let _ = register;
//! ```

/// Who a sign-in in the system's own sheet is with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SheetProvider {
    /// Sign in with Apple: offered to an app on one of Apple's systems.
    Apple,
    /// Sign in with Google: offered to an app on Android.
    Google,
}

impl SheetProvider {
    /// The words of the provider's button.
    pub fn label(self) -> &'static str {
        match self {
            SheetProvider::Apple => "Sign in with Apple",
            SheetProvider::Google => "Sign in with Google",
        }
    }
}

/// A sign-in the system's own sheet does in a native app, with the two routes
/// of this site it speaks to. The app posts to [`nonce`](Self::nonce) for a
/// one-time nonce and the sealed state of the attempt, opens the provider's
/// sheet with the nonce, and posts the identity token and the state to
/// [`sign_in`](Self::sign_in), which checks both and signs the app's session
/// in. The module that owns the routes does every check there: nothing a page
/// or a client says of a sheet is trusted.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ProviderSheet {
    /// Who the sign-in is with.
    pub provider: SheetProvider,
    /// The route that hands out a nonce and a state for one attempt (a path on
    /// this site): `/api/auth/social/apple/native/nonce`.
    pub nonce: String,
    /// The route the identity token is posted to, which signs the app's
    /// session in: `/api/auth/social/apple/native/session`.
    pub sign_in: String,
    /// The app's client id with the provider, where its sheet needs one:
    /// Google's server client id on Android. It is not a secret.
    pub client_id: Option<String>,
}

impl ProviderSheet {
    /// A sign-in with `provider` whose nonce comes from `nonce` and whose
    /// token goes to `sign_in`.
    pub fn new(
        provider: SheetProvider,
        nonce: impl Into<String>,
        sign_in: impl Into<String>,
    ) -> Self {
        Self {
            provider,
            nonce: nonce.into(),
            sign_in: sign_in.into(),
            client_id: None,
        }
    }

    /// The app's client id with the provider.
    #[must_use]
    pub fn client_id(mut self, id: impl Into<String>) -> Self {
        self.client_id = Some(id.into());
        self
    }
}

/// A module's sign-ins by the system's sheet: register it with
/// `ModuleBuilder::multi`. It is asked on every sign-in page a native client
/// that can open such a sheet is given, so it answers from what the module
/// already holds.
pub trait ProviderSheets: Send + Sync {
    /// The sign-ins the app is set up for now: none while the module has no
    /// provider, or no app of `auth.apps`, to take one.
    fn sheets(&self) -> Vec<ProviderSheet>;
}
