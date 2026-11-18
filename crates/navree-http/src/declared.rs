//! [`AccessDeclarations`] and [`DeclaredAccess`]: how a crate whose
//! handlers declare who may call them (a route, a mount, a message pattern, a
//! gRPC method, an MCP item, a GraphQL field), or whose values carry a
//! permission or an ability check (a link, a card, a tool, a settings group,
//! an import), shows those declarations to a boot check, whichever modules the
//! app has.

use std::fmt;
use std::sync::Arc;

use navree_kernel::{AppContext, BoxError, BoxFuture, ModuleId};

use crate::mount::collect_mounts;
use crate::router::routes_under;
use crate::{AccessSpec, CanSpec, HttpServer};

/// What one handler or value declares about who may call or see it, as a boot
/// check reads it.
#[derive(Clone, Debug)]
pub struct DeclaredAccess {
    /// What declares it, as a boot text names it: "the route", "the mount",
    /// "the message pattern" or "the sidebar link", then its name between
    /// backticks.
    pub what: String,
    /// The module that registered the handler or value, when it is known.
    pub module: Option<ModuleId>,
    /// That module's name.
    pub module_name: &'static str,
    /// The declaration: the roles, the permissions and the ability it
    /// requires.
    pub access: AccessSpec,
}

impl DeclaredAccess {
    /// Formats the boot-check label of a value of `kind` named `owner`, such
    /// as `the sidebar link` or `the agent tool` followed by `owner` between
    /// backticks.
    pub fn value_label(kind: &str, owner: &str) -> String {
        let kind = kind.strip_prefix("the ").unwrap_or(kind);
        format!("the {kind} `{owner}`")
    }

    /// An access declaration carried as a value (a link's, a card's, a tool's,
    /// a settings group's, an import's or export's, an option's), labelled
    /// with `kind` and `owner`.
    ///
    /// Contributed directly on a `ModuleBuilder` (`m.contribute(..)`), it
    /// receives that module's id and name when [`declared_access`] collects it.
    /// Returned from an [`AccessDeclarations`] source instead, attach the
    /// registering module with [`in_module`](Self::in_module), or a source
    /// name with [`in_named`](Self::in_named).
    pub fn value(kind: &str, owner: impl AsRef<str>, access: AccessSpec) -> Self {
        Self {
            what: Self::value_label(kind, owner.as_ref()),
            module: None,
            module_name: "",
            access,
        }
    }

    /// One permission carried as a value of `kind` named `owner`.
    pub fn value_permission(
        kind: &str,
        owner: impl AsRef<str>,
        permission: impl Into<String>,
    ) -> Self {
        Self::value(
            kind,
            owner,
            AccessSpec {
                permissions: vec![permission.into()],
                ..AccessSpec::default()
            },
        )
    }

    /// Several permissions carried as a value of `kind` named `owner`.
    pub fn value_permissions<I, S>(kind: &str, owner: impl AsRef<str>, permissions: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::value(
            kind,
            owner,
            AccessSpec {
                permissions: permissions.into_iter().map(Into::into).collect(),
                ..AccessSpec::default()
            },
        )
    }

    /// A type-level `can` check (`action` on `subject`) carried as a value of
    /// `kind` named `owner` (such as a Helm link, card or page, or an agent
    /// tool's `can` attribute).
    pub fn value_can(
        kind: &str,
        owner: impl AsRef<str>,
        action: impl Into<String>,
        subject: impl Into<String>,
    ) -> Self {
        Self::value(
            kind,
            owner,
            AccessSpec {
                can: Some(CanSpec {
                    action: action.into(),
                    subject: subject.into(),
                    from: None,
                }),
                ..AccessSpec::default()
            },
        )
    }

    /// Sets the module (`id` and `name`) that registered this value, for use
    /// inside an [`AccessDeclarations`] source.
    #[must_use]
    pub fn in_module(mut self, id: ModuleId, name: &'static str) -> Self {
        self.module = Some(id);
        self.module_name = name;
        self
    }

    /// Sets `name` as the owner's module or source name when no [`ModuleId`]
    /// applies (such as a configuration key read at boot).
    #[must_use]
    pub fn in_named(mut self, name: &'static str) -> Self {
        self.module = None;
        self.module_name = name;
        self
    }
}

/// The function behind an [`AccessDeclarations`].
type ReadDeclared = Arc<
    dyn Fn(AppContext) -> BoxFuture<'static, Result<Vec<DeclaredAccess>, BoxError>> + Send + Sync,
>;

/// Reads the access declaration of every handler of one kind: a contribution
/// of the module that serves that kind (`HttpModule` for routes and mounts),
/// so an app reads the kinds it has, with or without `HttpModule`.
/// [`declared_access`] calls every one the app's modules contribute.
#[derive(Clone)]
pub struct AccessDeclarations(ReadDeclared);

impl AccessDeclarations {
    /// The declarations that `read` finds in the app it is given. It runs at
    /// boot, once every singleton is built, and may run more than once.
    pub fn new<F, Fut>(read: F) -> Self
    where
        F: Fn(AppContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Vec<DeclaredAccess>, BoxError>> + Send + 'static,
    {
        Self(Arc::new(move |ctx| Box::pin(read(ctx))))
    }
}

impl fmt::Debug for AccessDeclarations {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AccessDeclarations").finish_non_exhaustive()
    }
}

/// The access declarations of every handler and value of the app: every kind
/// whose module contributes an [`AccessDeclarations`] and every
/// [`DeclaredAccess`] contributed directly on a `ModuleBuilder`, modules in
/// dependency order. navree-access reads it at boot, to hold what the app
/// requires against the namespaces of the app's plugins.
pub async fn declared_access(ctx: &AppContext) -> Result<Vec<DeclaredAccess>, BoxError> {
    let mut declared = Vec::new();
    for source in ctx.contributions::<AccessDeclarations>() {
        for mut entry in (source.value.0)(ctx.clone()).await? {
            if entry.module_name.is_empty() {
                entry.module_name = "?";
            }
            declared.push(entry);
        }
    }
    for contributed in ctx.contributions::<DeclaredAccess>() {
        let mut entry = (*contributed.value).clone();
        if entry.module.is_none() && entry.module_name.is_empty() {
            entry.module = Some(contributed.module.id);
            entry.module_name = contributed.module.name;
        }
        if entry.module_name.is_empty() {
            entry.module_name = "?";
        }
        declared.push(entry);
    }
    Ok(declared)
}

/// What the app's routes and mounts declare: what `HttpModule` contributes
/// as its [`AccessDeclarations`].
///
/// The routes are read with the options of the app's [`HttpServer`], the ones
/// the router is built with, and not with a lookup of `HttpOptions`: a second
/// provider of the options in the app (a module that registers and exports
/// its own) makes that lookup ambiguous, and the routes are read all the
/// same. An app whose `HttpServer` cannot be resolved is refused.
pub(crate) async fn routes_and_mounts(ctx: AppContext) -> Result<Vec<DeclaredAccess>, BoxError> {
    let server = ctx
        .get::<HttpServer>()
        .await
        .map_err(|error| format!("the app's routes cannot be read without HttpServer: {error}"))?;
    let mut declared = Vec::new();
    for route in routes_under(&ctx, server.options()).await? {
        declared.push(DeclaredAccess {
            what: format!("the route `{} {}`", route.method, route.full_path),
            module: route.module_id,
            module_name: route.module.unwrap_or("?"),
            access: route.access,
        });
    }
    for mount in collect_mounts(&ctx) {
        declared.push(DeclaredAccess {
            what: format!("the mount `{}`", mount.prefix),
            module: mount.meta.module_id,
            module_name: mount.module,
            access: mount.meta.access,
        });
    }
    Ok(declared)
}

#[cfg(test)]
mod tests {
    use navree_kernel::{Module, ModuleBuilder, ModuleId, Navree};

    use super::{AccessDeclarations, DeclaredAccess, declared_access};

    /// Contributes one declaration of each value-carrying kind directly on its
    /// `ModuleBuilder`.
    struct PortalModule;

    impl Module for PortalModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.contribute(DeclaredAccess::value_permission(
                "sidebar link",
                "/announcements",
                "announcements.read",
            ))
            .contribute(DeclaredAccess::value_can(
                "Helm card",
                "open-deals",
                "read",
                "Deal",
            ))
            .contribute(DeclaredAccess::value_permissions(
                "agent tool",
                "close_deal",
                ["deals.update", "deals.close"],
            ))
            .contribute(DeclaredAccess::value_permissions(
                "settings group",
                "portal",
                ["portal.manage", "portal.read"],
            ))
            .contribute(DeclaredAccess::value_permission(
                "the sheet import",
                "deals",
                "deals.import",
            ));
        }
    }

    /// Contributes value declarations through an [`AccessDeclarations`] source.
    struct ExportsModule;

    impl Module for ExportsModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.contribute(AccessDeclarations::new(|_ctx| async move {
                Ok(vec![
                    DeclaredAccess::value_permission("sheet export", "deals", "deals.export")
                        .in_module(ModuleId::of::<PortalModule>(), "PortalModule"),
                    DeclaredAccess::value_permission(
                        "option",
                        "auth.impersonation.permission",
                        "users.impersonate",
                    )
                    .in_named("configuration"),
                ])
            }));
        }
    }

    #[tokio::test]
    async fn a_permission_carried_as_a_value_is_declared_with_its_kind_and_its_owner() {
        let app = Navree::app()
            .module(PortalModule)
            .module(ExportsModule)
            .boot()
            .await
            .expect("boots");
        let declared = declared_access(app.ctx()).await.expect("declarations");
        let portal = Some(ModuleId::of::<PortalModule>());
        let summary: Vec<(
            &str,
            Option<ModuleId>,
            &str,
            Vec<&str>,
            Option<(&str, &str)>,
        )> = declared
            .iter()
            .map(|entry| {
                let permissions = entry
                    .access
                    .permissions
                    .iter()
                    .map(String::as_str)
                    .collect();
                let can = entry
                    .access
                    .can
                    .as_ref()
                    .map(|can| (can.action.as_str(), can.subject.as_str()));
                (
                    entry.what.as_str(),
                    entry.module,
                    entry.module_name,
                    permissions,
                    can,
                )
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                (
                    "the sheet export `deals`",
                    portal,
                    "PortalModule",
                    vec!["deals.export"],
                    None,
                ),
                (
                    "the option `auth.impersonation.permission`",
                    None,
                    "configuration",
                    vec!["users.impersonate"],
                    None,
                ),
                (
                    "the sidebar link `/announcements`",
                    portal,
                    "PortalModule",
                    vec!["announcements.read"],
                    None,
                ),
                (
                    "the Helm card `open-deals`",
                    portal,
                    "PortalModule",
                    Vec::new(),
                    Some(("read", "Deal")),
                ),
                (
                    "the agent tool `close_deal`",
                    portal,
                    "PortalModule",
                    vec!["deals.update", "deals.close"],
                    None,
                ),
                (
                    "the settings group `portal`",
                    portal,
                    "PortalModule",
                    vec!["portal.manage", "portal.read"],
                    None,
                ),
                (
                    "the sheet import `deals`",
                    portal,
                    "PortalModule",
                    vec!["deals.import"],
                    None,
                ),
            ]
        );
    }
}
