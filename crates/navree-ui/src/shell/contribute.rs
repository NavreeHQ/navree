//! What a module adds to the shell document a native client reads.
//!
//! The shell document (`/native/shell` as the protocol names it) says where the app signs a
//! person in and where a device's push token is posted. Those routes belong to modules
//! that depend on this crate (authentication, notifications), not the other way round, so
//! a module says them here, as plain data, and the document is made of what every module
//! said. A module registers a [`NativeShellPart`] as a multi-binding:
//!
//! ```
//! use std::sync::Arc;
//! use navree_kernel::{Factory, ModuleBuilder};
//! use navree_ui::{NativeClient, NativeShellPart, NativeShellParts};
//!
//! struct Push;
//!
//! impl NativeShellPart for Push {
//!     fn contribute(&self, shell: &mut NativeShellParts, _: &NativeClient) {
//!         shell.push_register("/api/notifications/push-devices");
//!     }
//! }
//!
//! fn register(m: &mut ModuleBuilder) {
//!     m.multi(Factory::<dyn NativeShellPart>::new(|_| async {
//!         Ok(Arc::new(Push) as Arc<dyn NativeShellPart>)
//!     }));
//! }
//! # let _ = register;
//! ```
//!
//! Nothing here depends on the preview feature: a module says its routes the same whether
//! or not an app answers native clients, and the preview reads them.

/// The family of systems a native client is on, as an app's list of apps names them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NativeFamily {
    /// Apple's systems: an app is named by its bundle id.
    Apple,
    /// Android: an app is named by its package name.
    Android,
    /// Another system, or a client that did not say.
    Other,
}

/// The client the shell document is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct NativeClient {
    /// The family of systems it is on.
    pub family: NativeFamily,
}

impl NativeClient {
    /// A client of `family`.
    pub fn new(family: NativeFamily) -> Self {
        Self { family }
    }
}

/// Where an app signs a person in, as paths on its own origin (the callback an absolute
/// link): the routes of the hand-over a native app makes with the system's sign-in session.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct NativeSignIn {
    /// Where the system's sign-in session starts.
    pub start: String,
    /// Where the one-time code is exchanged for a session.
    pub exchange: String,
    /// The app's own link the sign-in session comes back on, when the server has one for
    /// the client's family.
    pub callback: Option<String>,
    /// The page that finishes a second step inside the app.
    pub second_step: Option<String>,
}

impl NativeSignIn {
    /// A hand-over that starts at `start` and is exchanged at `exchange`.
    pub fn new(start: impl Into<String>, exchange: impl Into<String>) -> Self {
        Self {
            start: start.into(),
            exchange: exchange.into(),
            callback: None,
            second_step: None,
        }
    }

    /// The app's own link the sign-in session comes back on.
    pub fn callback(mut self, callback: impl Into<String>) -> Self {
        self.callback = Some(callback.into());
        self
    }

    /// The page that finishes a second step.
    pub fn second_step(mut self, path: impl Into<String>) -> Self {
        self.second_step = Some(path.into());
        self
    }
}

/// What the modules of an app say of the shell document, for one client.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NativeShellParts {
    pub(crate) sign_in: Option<NativeSignIn>,
    pub(crate) push_register: Option<String>,
}

impl NativeShellParts {
    /// Says where the app signs a person in.
    pub fn sign_in(&mut self, routes: NativeSignIn) -> &mut Self {
        self.sign_in = Some(routes);
        self
    }

    /// Says where a device's push token is posted and deleted.
    pub fn push_register(&mut self, path: impl Into<String>) -> &mut Self {
        self.push_register = Some(path.into());
        self
    }
}

/// A module's share of the shell document: register it with
/// [`ModuleBuilder::multi`](navree_kernel::ModuleBuilder::multi).
pub trait NativeShellPart: Send + Sync {
    /// Adds what the module knows to `shell`, for `client`.
    fn contribute(&self, shell: &mut NativeShellParts, client: &NativeClient);
}
