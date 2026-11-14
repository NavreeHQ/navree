//! [`PermissionCheck`]: permission questions asked outside a route's access
//! declaration (a channel authorizer, a service deciding who may see a
//! record); [`Permission`]: the permissions modules describe, which
//! [`permission_catalog`] lists and which describing grants to nobody.

use navree_kernel::{AppContext, ModuleBuilder, ModuleInfo};
use uuid::Uuid;

use crate::{BoxError, BoxFuture, Principal};

/// Answers "does this principal hold this permission?", app-wide or in one
/// workspace. Registered as multi-bindings of `dyn PermissionCheck`:
/// navree-access answers for the user's app-wide roles and permissions,
/// navree-workspace for their role in a workspace. Ask them all with
/// [`permitted`].
///
/// A check returns `Some(true)` or `Some(false)` when the question is its to
/// answer, and `None` when it is not (navree-workspace's check, asked about
/// no workspace).
pub trait PermissionCheck: Send + Sync + 'static {
    /// Whether `principal` holds `permission` (`deals.read`,
    /// `operations.view`), in `workspace` when one is given.
    fn check<'a>(
        &'a self,
        principal: &'a Principal,
        permission: &'a str,
        workspace: Option<Uuid>,
    ) -> BoxFuture<'a, Result<Option<bool>, BoxError>>;
}

/// Whether any registered [`PermissionCheck`] grants `principal` the
/// `permission`, app-wide or in `workspace`. False when no check is
/// registered or none grants it; a check that fails is an error, not a
/// denial.
///
/// ```no_run
/// # async fn example(ctx: &navree_kernel::AppContext, principal: &navree_contracts::Principal, workspace: uuid::Uuid) -> Result<(), navree_contracts::BoxError> {
/// if navree_contracts::permitted(ctx, principal, "operations.view", Some(workspace)).await? {
///     // show the operation
/// }
/// # Ok(())
/// # }
/// ```
pub async fn permitted(
    ctx: &AppContext,
    principal: &Principal,
    permission: &str,
    workspace: Option<Uuid>,
) -> Result<bool, BoxError> {
    for check in ctx.get_all::<dyn PermissionCheck>().await? {
        if check.check(principal, permission, workspace).await? == Some(true) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// A permission a module declares: its name and what it lets someone do.
///
/// A module describes its permissions with
/// [`m.permissions(..)`](ModuleBuilderPermissionsExt::permissions) and
/// [`permission_catalog`] lists them. Describing a permission grants it to
/// nobody: the app's own roles and configuration decide who holds it. The
/// roles that hold every permission hold it too, from the moment the module
/// is in the app: a workspace's `owner` (`all.*`) in that workspace, and the
/// app's super-admin role. So a plugin does not guard a staff-only action
/// with a permission a workspace's owner holds.
///
/// ```
/// use navree_contracts::{ModuleBuilderPermissionsExt, Permission};
/// use navree_kernel::{Module, ModuleBuilder};
///
/// struct AnnouncementsModule;
///
/// impl Module for AnnouncementsModule {
///     fn register(&self, m: &mut ModuleBuilder) {
///         m.permissions(&[
///             Permission::new("announcements.read", "Read the workspace's announcements"),
///             Permission::new("announcements.publish", "Publish an announcement to the workspace"),
///         ]);
///     }
/// }
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Permission {
    /// `announcements.publish`: two or more lower-case words joined by `.`.
    pub name: &'static str,
    /// One line: `Publish an announcement to the workspace`.
    pub description: &'static str,
}

impl Permission {
    /// The permission `name` (`announcements.publish`), which lets someone do
    /// what `description` says (`Publish an announcement to the workspace`).
    /// The name is checked at boot.
    pub const fn new(name: &'static str, description: &'static str) -> Permission {
        Permission { name, description }
    }
}

/// Permission descriptions on a [`ModuleBuilder`].
pub trait ModuleBuilderPermissionsExt {
    /// Describes permissions; it grants none. A route that requires one says
    /// so with `RouteDef::permissions`, and a person holds one only when the
    /// app grants it: a workspace role's list under `workspace.roles` in the
    /// app's configuration, or a role of navree-access. A workspace's owner
    /// and the app's super-admin role hold every permission.
    ///
    /// The list is copied. A name given twice keeps its first description,
    /// and an empty list adds nothing. [`permission_catalog`] lists what
    /// every module described; [`Permission`] shows a module calling it.
    fn permissions(&mut self, permissions: &[Permission]) -> &mut Self;
}

impl ModuleBuilderPermissionsExt for ModuleBuilder {
    fn permissions(&mut self, permissions: &[Permission]) -> &mut Self {
        let mut described: Vec<Permission> = Vec::with_capacity(permissions.len());
        for permission in permissions {
            if !described.iter().any(|kept| kept.name == permission.name) {
                described.push(permission.clone());
            }
        }
        if described.is_empty() {
            self
        } else {
            self.contribute(Permissions(described))
        }
    }
}

/// What one `m.permissions(..)` call contributes: the permissions it
/// describes, each name once. Read through [`permission_catalog`].
pub(crate) struct Permissions(pub(crate) Vec<Permission>);

/// Every declared permission and the module that declared it, in module
/// order (a module's own in the order it described them). A name one module
/// describes twice is listed once, with its first description; a name two
/// modules describe is listed for each (AccessModule checks the catalog at
/// boot; without it nothing checks the catalog and no route can require a
/// permission, while a workspace role still holds what `workspace.roles`
/// lists). The catalog grants nothing.
///
/// ```no_run
/// use navree_contracts::{ModuleBuilderPermissionsExt, Permission, permission_catalog};
/// use navree_kernel::{Module, ModuleBuilder, Navree};
///
/// struct AnnouncementsModule;
///
/// impl Module for AnnouncementsModule {
///     fn register(&self, m: &mut ModuleBuilder) {
///         m.permissions(&[Permission::new(
///             "announcements.read",
///             "Read the workspace's announcements",
///         )]);
///     }
/// }
///
/// #[tokio::main]
/// async fn main() -> Result<(), navree_kernel::BootErrors> {
///     let app = Navree::app().module(AnnouncementsModule).boot().await?;
///     for (module, permission) in permission_catalog(app.ctx()) {
///         println!("{} ({}): {}", permission.name, module.name, permission.description);
///     }
///     Ok(())
/// }
/// ```
pub fn permission_catalog(ctx: &AppContext) -> Vec<(ModuleInfo, Permission)> {
    let mut catalog: Vec<(ModuleInfo, Permission)> = Vec::new();
    for contributed in ctx.contributions::<Permissions>() {
        for permission in &contributed.value.0 {
            let listed = catalog.iter().any(|(module, kept)| {
                module.id == contributed.module.id && kept.name == permission.name
            });
            if !listed {
                catalog.push((contributed.module.clone(), permission.clone()));
            }
        }
    }
    catalog
}

#[cfg(test)]
mod tests {
    use navree_kernel::{Module, Navree};

    use super::*;
    use crate::AuthVia;

    #[tokio::test]
    async fn the_catalog_lists_declared_permissions_and_grants_nothing() {
        struct Announcements;
        impl Module for Announcements {
            fn register(&self, m: &mut ModuleBuilder) {
                m.permissions(&[
                    Permission::new("announcements.read", "Read the workspace's announcements"),
                    Permission::new(
                        "announcements.publish",
                        "Publish an announcement to the workspace",
                    ),
                    Permission::new("announcements.read", "Said a second time"),
                ]);
                m.permissions(&[]);
            }
        }
        struct Deals;
        impl Module for Deals {
            fn register(&self, m: &mut ModuleBuilder) {
                m.permissions(&[Permission::new(
                    "deals.export",
                    "Export the deals of Ludhiana as a spreadsheet",
                )]);
            }
        }
        let builder = Navree::app().module(Announcements).module(Deals);
        // Describing a permission registers nothing that could answer a check.
        let report = builder.validate().expect("valid");
        assert!(
            report
                .modules
                .iter()
                .all(|module| module.providers.is_empty())
        );
        let app = builder.boot().await.expect("boots");
        let ctx = app.ctx();
        assert_eq!(ctx.multi_count::<dyn PermissionCheck>(), 0);
        // An empty list contributes nothing.
        assert_eq!(ctx.contributions::<Permissions>().len(), 2);
        let catalog: Vec<(&str, &str, &str)> = permission_catalog(ctx)
            .iter()
            .map(|(module, permission)| (module.name, permission.name, permission.description))
            .collect();
        assert_eq!(
            catalog,
            [
                (
                    "Announcements",
                    "announcements.read",
                    "Read the workspace's announcements",
                ),
                (
                    "Announcements",
                    "announcements.publish",
                    "Publish an announcement to the workspace",
                ),
                (
                    "Deals",
                    "deals.export",
                    "Export the deals of Ludhiana as a spreadsheet",
                ),
            ]
        );
        let harpreet = Principal {
            user_id: Uuid::now_v7(),
            via: AuthVia::Test,
            token_abilities: None,
            session_id: None,
        };
        assert!(
            !permitted(ctx, &harpreet, "announcements.read", None)
                .await
                .unwrap()
        );
    }
}
