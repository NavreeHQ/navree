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
//!
//! A module also adds links to the app's sidebar, as [`ShellNavItem`]s with `m.nav_item`
//! ([`ModuleBuilderShellExt`]): a shell built with `Shell::contributed` draws each one,
//! after its own groups, for the readers who may open it.

use std::borrow::Cow;

use navree_kernel::ModuleBuilder;

use crate::icons::Icon;

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
    /// Windows: an app is named by its package family name when it is installed from a
    /// package, and by the id it was built with when it is not.
    Windows,
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

/// A link a module adds to the app's sidebar: `m.nav_item(..)`.
///
/// A shell built with [`Shell::contributed`](crate::Shell::contributed) draws it through the
/// [`AppShell`](crate::AppShell) extractor, after the shell's own groups, for a reader who
/// holds its [`permission`](ShellNavItem::permission), decided on the server for every request.
///
/// A link without a permission is public: every reader sees it, a guest too, also through
/// `GET /_navree/ui/navigation`. Hiding a link is not access control: the page it leads to
/// declares its own access.
///
/// ```
/// use navree_kernel::{Module, ModuleBuilder};
/// use navree_ui::{ModuleBuilderShellExt, ShellNavItem, icons};
///
/// struct AnnouncementsModule;
///
/// impl Module for AnnouncementsModule {
///     fn register(&self, m: &mut ModuleBuilder) {
///         m.nav_item(
///             ShellNavItem::new("Announcements", "/announcements")
///                 .icon(icons::MEGAPHONE)
///                 .group("Workspace")
///                 .permission("announcements.read")
///                 .order(10),
///         );
///     }
/// }
/// ```
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ShellNavItem {
    pub(crate) label: Cow<'static, str>,
    pub(crate) href: &'static str,
    pub(crate) icon: Option<Icon>,
    pub(crate) group: Option<Cow<'static, str>>,
    pub(crate) permission: Option<&'static str>,
    pub(crate) order: i32,
}

impl ShellNavItem {
    /// A link labelled `label`. `href` is a path on this app (`/announcements`); checked at
    /// boot as [`LocalHref::parse`](crate::html::LocalHref::parse) does, and it starts with
    /// `/`.
    pub fn new(label: impl Into<Cow<'static, str>>, href: &'static str) -> ShellNavItem {
        ShellNavItem {
            label: label.into(),
            href,
            icon: None,
            group: None,
            permission: None,
            order: 0,
        }
    }

    /// Sets the icon: the folded sidebar shows only that.
    #[must_use]
    pub fn icon(mut self, icon: Icon) -> ShellNavItem {
        self.icon = Some(icon);
        self
    }

    /// The sidebar group's label (default: the plugin's title, else `More`).
    #[must_use]
    pub fn group(mut self, group: impl Into<Cow<'static, str>>) -> ShellNavItem {
        self.group = Some(group.into());
        self
    }

    /// Shown only to readers who hold `permission`, app-wide or in the request's workspace.
    #[must_use]
    pub fn permission(mut self, permission: &'static str) -> ShellNavItem {
        self.permission = Some(permission);
        self
    }

    /// Lower comes first in its group (default 0; ties keep module order).
    #[must_use]
    pub fn order(mut self, order: i32) -> ShellNavItem {
        self.order = order;
        self
    }
}

/// Sidebar links on a [`ModuleBuilder`].
pub trait ModuleBuilderShellExt {
    /// Adds `item` to the app's sidebar, drawn by a shell built with
    /// [`Shell::contributed`](crate::Shell::contributed). Its address is checked at boot;
    /// [`ShellNavItem`] shows a module calling it.
    fn nav_item(&mut self, item: ShellNavItem) -> &mut Self;
}

impl ModuleBuilderShellExt for ModuleBuilder {
    fn nav_item(&mut self, item: ShellNavItem) -> &mut Self {
        self.contribute(item)
    }
}
