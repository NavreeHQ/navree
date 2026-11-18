//! [`HelmModule`] and [`ModuleBuilderHelmExt`].

use std::future::Future;
use std::sync::Arc;

use navree_access::{AccessModule, Subject};
use navree_config::ModuleBuilderConfigExt;
use navree_contracts::ModuleBuilderJobsExt;
use navree_http::{AppError, ModuleBuilderHttpExt};
use navree_kernel::{
    AppContext, Factory, Injectable, Module, ModuleBuilder, ProviderKey, Resolver,
};

use crate::controller::HelmController;
use crate::core::Core;
use crate::page::PageRegistration;
use crate::registry::ResourceRegistration;
use crate::workspace::{StaffEntry, StaffOrganizationEntry, StaffOrganizationPolicy, StaffPolicy};
use crate::{HelmActionJob, HelmOptions, HelmPage, Resource, session};

/// The subject that guards the panel: `view` on `Helm` (permission
/// `helm.view`) lets a user in.
#[derive(Clone, Copy, Debug, Default)]
pub struct Helm;

impl Subject for Helm {
    const SUBJECT: &'static str = "Helm";
    const PERMISSION_PREFIX: &'static str = "helm";
}

/// The admin panel, served at `helm.path` (`/admin`).
///
/// Global; provides and exports [`HelmOptions`], registers the subject
/// [`Helm`] (prefix `helm`), the panel's routes, its request layer (the
/// CSRF token of the browser's session, error pages and a strict Content
/// Security Policy) and the `helm.action` job of queued actions. It has no
/// tables.
///
/// The app needs `DatabaseModule`, `AccessModule` (every check) and
/// `CryptoModule`. People sign in to the panel the way they sign in to the
/// rest of the app, on the app's browser session: `SessionModule`,
/// `AuthModule`, `UiModule` and `AuthPages` (the sign-in pages). A guest is
/// sent to the sign-in page, and comes back to the page they asked for.
/// Resources and pages are registered by feature modules with
/// [`ModuleBuilderHelmExt`].
///
/// ```no_run
/// use navree_access::{AccessModule, AccessOptions};
/// use navree_config::{ConfigModule, ConfigSources};
/// use navree_db::DatabaseModule;
/// use navree_helm::HelmModule;
/// use navree_http::HttpModule;
/// use navree_kernel::Navree;
///
/// # fn main() -> Result<(), navree_kernel::BoxError> {
/// Navree::app()
///     .module(ConfigModule::for_root(ConfigSources::defaults()))
///     .module(HttpModule::from_config())
///     .module(DatabaseModule::from_config())
///     .module(AccessModule::for_root(AccessOptions::default()))
///     // CryptoModule, SessionModule, AuthModule, UiModule, AuthPages,
///     // your feature modules ...
///     .module(HelmModule::from_config()) // section `helm`
///     .run()
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct HelmModule;

impl HelmModule {
    /// The module with explicit options (invalid options fail the boot).
    pub fn for_root(options: HelmOptions) -> impl Module {
        ConfiguredHelmModule::for_root(options)
    }

    /// The module configured from the `helm` section of `Config` at boot.
    pub fn from_config() -> impl Module {
        ConfiguredHelmModule::from_config()
    }
}

impl Module for HelmModule {
    fn register(&self, m: &mut ModuleBuilder) {
        register(m, None);
    }
}

navree_kernel::configured_module!(ConfiguredHelmModule, HelmModule, HelmOptions, register);

fn register(m: &mut ModuleBuilder, options: Option<HelmOptions>) {
    m.plugin(navree_kernel::plugin_info!());
    m.global();
    match options {
        Some(options) => m.factory(Factory::<HelmOptions>::new(move |_| {
            let options = options.clone();
            async move {
                options
                    .check()
                    .map_err(|errors| format!("invalid helm options: {}", errors.join("; ")))?;
                Ok(Arc::new(options))
            }
        })),
        None => m.config::<HelmOptions>(),
    };
    m.exports::<HelmOptions>()
        .imports(AccessModule::for_subject::<Helm>())
        // The panel's error pages are the toolkit's, and the sign-in page's
        // address (`ui.sign_in`) is its setting.
        .import(navree_ui::UiOnDemand)
        .contribute(navree_ui::UiNeed::when(
            |_ctx: AppContext| async move { true },
        ))
        .contribute(navree_http::__private::AccessDeclarations::new(
            crate::plugin::declared_access,
        ))
        .factory(
            Factory::<Core>::new(|r: Resolver| async move {
                let options = r.get::<HelmOptions>().await?;
                Ok(Arc::new(Core::new(r.app(), options).await?))
            })
            .deps(vec![ProviderKey::of::<HelmOptions>()]),
        )
        .controller::<HelmController>()
        .http_layer(session::layer())
        .job::<HelmActionJob>();
}

/// Helm registrations on a [`ModuleBuilder`].
pub trait ModuleBuilderHelmExt {
    /// Registers resource `R` as a provider of this module and adds it to
    /// the panel. Its model's subject is registered with navree-access
    /// (`AccessModule::for_feature`), so permissions on its prefix map to
    /// it.
    fn resource<R: Resource + Injectable>(&mut self) -> &mut Self;

    /// Adds a custom page to the panel.
    fn helm_page(&mut self, page: HelmPage) -> &mut Self;

    /// Lets the users `may_enter` says work in workspaces they are not members
    /// of: support staff, an operator. Without this nobody does; a panel user
    /// works only in the workspaces they are a member of. Each request made
    /// that way is audited. Register it once.
    ///
    /// Inside a workspace entered this way a user keeps their app-wide
    /// abilities. In an app without `WorkspaceModule` this is the only way a
    /// request can name a workspace, and nothing checks that the workspace
    /// exists unless `may_enter` does.
    ///
    /// Only the app decides who works as staff: the panel stops the boot when
    /// a module of a plugin that is not one of the framework's own packages
    /// calls this.
    ///
    /// ```no_run
    /// # use navree_helm::ModuleBuilderHelmExt;
    /// # fn register(m: &mut navree_kernel::ModuleBuilder) {
    /// m.helm_staff_workspaces(|entry| async move {
    ///     Ok(entry.access.ability.can("support", "Workspace", None))
    /// });
    /// # }
    /// ```
    fn helm_staff_workspaces<F, Fut>(&mut self, may_enter: F) -> &mut Self
    where
        F: Fn(StaffEntry) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<bool, AppError>> + Send + 'static;

    /// Lets the users `may_enter` says work with the settings of organizations
    /// they are not members of, on the Settings screens. Without this nobody does.
    /// Each request made that way writes a `helm.organization.staff` audit entry,
    /// reads included. Register it once.
    ///
    /// Only the app decides who works as staff: the panel stops the boot when a
    /// module of a plugin that is not one of the framework's own packages calls
    /// this.
    fn helm_staff_organizations<F, Fut>(&mut self, may_enter: F) -> &mut Self
    where
        F: Fn(StaffOrganizationEntry) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<bool, AppError>> + Send + 'static;
}

impl ModuleBuilderHelmExt for ModuleBuilder {
    fn resource<R: Resource + Injectable>(&mut self) -> &mut Self {
        let module = self.module_id();
        self.provider::<R>()
            .imports(AccessModule::for_feature::<R::Model>())
            .contribute(ResourceRegistration::of::<R>(module))
    }

    fn helm_page(&mut self, page: HelmPage) -> &mut Self {
        let module = self.module_id();
        self.contribute(PageRegistration { page, module })
    }

    fn helm_staff_workspaces<F, Fut>(&mut self, may_enter: F) -> &mut Self
    where
        F: Fn(StaffEntry) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<bool, AppError>> + Send + 'static,
    {
        self.contribute(StaffPolicy::new(may_enter))
    }

    fn helm_staff_organizations<F, Fut>(&mut self, may_enter: F) -> &mut Self
    where
        F: Fn(StaffOrganizationEntry) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<bool, AppError>> + Send + 'static,
    {
        self.contribute(StaffOrganizationPolicy::new(may_enter))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use http::Method;
    use navree_access::{AccessModule, AccessOptions, AccessService};
    use navree_contracts::{ModuleBuilderPermissionsExt, Permission, permission_catalog};
    use navree_db::{DatabaseModule, Db};
    use navree_http::{Controller, ModuleBuilderHttpExt, RouteDef};
    use navree_kernel::__private::is_app_module;
    use navree_kernel::{
        BoxFuture, ConfiguredModule, Injectable, KernelError, Module, ModuleBuilder, ModuleId,
        ProviderKey, Resolver,
    };
    use navree_testing::{TestApp, principal};
    use navree_ui::{AppSheet, UiModule, UiOptions};
    use serde_json::Value;
    use uuid::Uuid;

    /// Warnings logged on this thread while the guard lives, with each event's target.
    #[derive(Clone, Default)]
    struct Warnings(Arc<Mutex<Vec<(String, String)>>>);

    impl Warnings {
        fn install(&self) -> tracing::subscriber::DefaultGuard {
            tracing::subscriber::set_default(self.clone())
        }

        fn at(&self, target: &str) -> Vec<String> {
            self.0
                .lock()
                .unwrap()
                .iter()
                .filter(|(at, _)| at == target)
                .map(|(_, message)| message.clone())
                .collect()
        }
    }

    struct Message(String);

    impl tracing::field::Visit for Message {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            if field.name() == "message" {
                self.0 = format!("{value:?}");
            }
        }
    }

    impl tracing::Subscriber for Warnings {
        fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
            *metadata.level() <= tracing::Level::WARN
        }

        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }

        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

        fn event(&self, event: &tracing::Event<'_>) {
            if *event.metadata().level() == tracing::Level::WARN {
                let mut message = Message(String::new());
                event.record(&mut message);
                let target = event.metadata().target().to_owned();
                self.0.lock().unwrap().push((target, message.0));
            }
        }

        fn enter(&self, _: &tracing::span::Id) {}

        fn exit(&self, _: &tracing::span::Id) {}
    }

    static HELM_BRAND: AppSheet = AppSheet::new("brand", b".helm-hero{color:var(--ink-1)}", 1024);
    static APP_BRAND: AppSheet = AppSheet::new("brand", b".app-hero{color:var(--ink-2)}", 1024);

    /// A stand-in module inside `navree-helm` contributing a static sheet.
    struct HelmSheetProbe;

    impl Module for HelmSheetProbe {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(navree_kernel::plugin_info!())
                .contribute(HELM_BRAND);
        }
    }

    fn register_app_sheet(m: &mut ModuleBuilder, _: Option<()>) {
        m.contribute(APP_BRAND);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_first_party_plugins_static_file_beside_an_apps_only_warns() {
        let warnings = Warnings::default();
        let _guard = warnings.install();

        let app_module = ConfiguredModule::<UiOptions, ()>::from_config(register_app_sheet);
        let app = TestApp::builder()
            .config("http.per_ip_rate_limit", Value::Null)
            .module(UiModule)
            .module(HelmSheetProbe)
            .module(app_module)
            .build()
            .await;

        assert!(is_app_module(app.ctx(), ModuleId::of::<UiOptions>()));
        let helm = app
            .ctx()
            .plugins()
            .plugins
            .into_iter()
            .find(|plugin| plugin.info.name == "navree-helm")
            .expect("navree-helm listed");
        assert!(helm.first_party, "{helm:?}");

        let ui_warnings = warnings.at("navree::ui");
        assert!(
            ui_warnings.iter().any(|warning| warning.contains(
                "static file `brand.css` is contributed by both `HelmSheetProbe` and `UiOptions`; `HelmSheetProbe`'s is served, and this becomes an error in 2.0"
            )),
            "{ui_warnings:?}"
        );

        let reply = app
            .client()
            .get(&HELM_BRAND.url())
            .send()
            .await
            .assert_status(200);
        assert!(reply.text().contains(".helm-hero"), "{}", reply.text());

        app.shutdown().await.unwrap();
    }

    /// A stand-in module inside `navree-helm` describing a permission another module describes.
    struct HelmPermissionProbe;

    impl Module for HelmPermissionProbe {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(navree_kernel::plugin_info!())
                .permissions(&[Permission::new(
                    "ledger.audit",
                    "Audit the Ludhiana ledger from the panel",
                )]);
        }
    }

    fn register_app_permission(m: &mut ModuleBuilder, _: Option<()>) {
        m.permissions(&[Permission::new(
            "ledger.audit",
            "Audit the Ludhiana ledger in the app",
        )]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_first_party_plugin_describing_another_modules_permission_only_warns() {
        let warnings = Warnings::default();
        let _guard = warnings.install();

        let app_module =
            ConfiguredModule::<AccessOptions, ()>::from_config(register_app_permission);
        let app = TestApp::builder()
            .temp_database()
            .config("http.per_ip_rate_limit", Value::Null)
            .module(DatabaseModule::from_config())
            .module(AccessModule::for_root(AccessOptions::default()))
            .module(HelmPermissionProbe)
            .module(app_module)
            .build()
            .await;

        assert!(is_app_module(app.ctx(), ModuleId::of::<AccessOptions>()));
        let helm = app
            .ctx()
            .plugins()
            .plugins
            .into_iter()
            .find(|plugin| plugin.info.name == "navree-helm")
            .expect("navree-helm listed");
        assert!(helm.first_party);

        let access_warnings = warnings.at("navree::access");
        assert!(
            access_warnings.iter().any(|warning| warning.contains(
                "permission `ledger.audit` is declared by both `HelmPermissionProbe` and `AccessOptions`; this becomes an error in 2.0"
            )),
            "{access_warnings:?}"
        );
        assert_eq!(
            permission_catalog(app.ctx())
                .iter()
                .filter(|(_, permission)| permission.name == "ledger.audit")
                .count(),
            2
        );

        app.shutdown().await.unwrap();
    }

    /// A community plugin whose namespace is `announcements`.
    mod navree_plugin_announcements {
        use navree_contracts::{ModuleBuilderPermissionsExt, Permission};
        use navree_kernel::{Module, ModuleBuilder, PluginInfo};

        pub struct AnnouncementsPlugin(pub &'static [Permission]);

        impl Module for AnnouncementsPlugin {
            fn register(&self, m: &mut ModuleBuilder) {
                m.plugin(PluginInfo::new(
                    "navree-plugin-announcements",
                    module_path!(),
                    "announcements",
                    "0.1.0",
                ))
                .permissions(self.0);
            }
        }
    }

    struct RequiredPermission(&'static [&'static str]);

    struct HelmProbeController(Arc<RequiredPermission>);

    impl Injectable for HelmProbeController {
        fn dependencies() -> Vec<ProviderKey> {
            vec![ProviderKey::of::<RequiredPermission>()]
        }

        fn construct(r: Resolver) -> BoxFuture<'static, Result<Self, KernelError>> {
            Box::pin(async move { Ok(HelmProbeController(r.get::<RequiredPermission>().await?)) })
        }
    }

    impl Controller for HelmProbeController {
        fn prefix(&self) -> &'static str {
            "!/helm-probe"
        }

        fn routes(self: Arc<Self>) -> Vec<RouteDef> {
            let permissions = self.0.0;
            vec![
                RouteDef::on(Method::GET, "/board", || async { "Ludhiana board" })
                    .permissions(permissions),
            ]
        }
    }

    /// A stand-in module inside `navree-helm` whose route requires `.0`.
    struct HelmRouteProbe(&'static [&'static str]);

    impl Module for HelmRouteProbe {
        fn register(&self, m: &mut ModuleBuilder) {
            m.plugin(navree_kernel::plugin_info!())
                .value(RequiredPermission(self.0))
                .controller::<HelmProbeController>();
        }
    }

    const ANNOUNCEMENTS_READ: &[Permission] = &[Permission::new(
        "announcements.read",
        "Read Ludhiana notices",
    )];
    const ANNOUNCEMENTS_READ_AND_ARCHIVE: &[Permission] = &[
        Permission::new("announcements.read", "Read Ludhiana notices"),
        Permission::new("announcements.archive", "Archive Ludhiana notices"),
    ];

    #[tokio::test(flavor = "current_thread")]
    async fn a_handler_of_a_first_party_plugin_that_requires_a_community_plugins_undescribed_permission_is_refused()
     {
        let warnings = Warnings::default();
        let _guard = warnings.install();

        // `announcements.archive` is under the community plugin's namespace and undescribed by it:
        // even a handler of a first-party plugin is refused, since the community plugin's policies
        // decide the names under `announcements`.
        let Err(error) = TestApp::builder()
            .temp_database()
            .config("http.per_ip_rate_limit", Value::Null)
            .module(DatabaseModule::from_config())
            .module(AccessModule::for_root(AccessOptions::default()))
            .module(navree_plugin_announcements::AnnouncementsPlugin(
                ANNOUNCEMENTS_READ,
            ))
            .module(HelmRouteProbe(&["announcements.archive"]))
            .try_build()
            .await
        else {
            panic!("first-party handler requiring undescribed community permission must fail")
        };
        let text = error.to_string();
        assert!(
            text.contains(
                "plugin `navree-plugin-announcements` uses the namespace `announcements`, the first word of the permission `announcements.archive` that the route `GET /helm-probe/board` of `HelmRouteProbe` requires, and the plugin describes no permission of that name; a plugin's policies decide the names under its namespace, so they would decide who may use it: require a permission the plugin describes, or name the app's permission under another word"
            ),
            "{text}"
        );

        // Once the community plugin describes `announcements.archive`, the app boots and the
        // app's grants decide who passes the route.
        let app = TestApp::builder()
            .temp_database()
            .config("http.per_ip_rate_limit", Value::Null)
            .module(DatabaseModule::from_config())
            .module(AccessModule::for_root(AccessOptions::default()))
            .module(navree_plugin_announcements::AnnouncementsPlugin(
                ANNOUNCEMENTS_READ_AND_ARCHIVE,
            ))
            .module(HelmRouteProbe(&["announcements.archive"]))
            .build()
            .await;

        let report = app.ctx().plugins();
        let helm = report
            .plugins
            .iter()
            .find(|plugin| plugin.info.name == "navree-helm")
            .expect("navree-helm listed");
        assert!(helm.first_party);
        let announcements = report
            .plugins
            .iter()
            .find(|plugin| plugin.info.name == "navree-plugin-announcements")
            .expect("navree-plugin-announcements listed");
        assert!(!announcements.first_party);

        let db = app.get::<Db>().await;
        let access = app.get::<AccessService>().await;
        let (harpreet, meera) = (Uuid::now_v7(), Uuid::now_v7());
        let uow = db.uow(None, None).await;
        access.create_role(&uow, "munshi", None).await.unwrap();
        access
            .grant(&uow, "munshi", "announcements.archive")
            .await
            .unwrap();
        access.assign_role(&uow, harpreet, "munshi").await.unwrap();
        uow.commit().await.unwrap();

        app.acting_as(principal(harpreet))
            .get("/helm-probe/board")
            .send()
            .await
            .assert_status(200);
        app.acting_as(principal(meera))
            .get("/helm-probe/board")
            .send()
            .await
            .assert_status(403);

        app.shutdown().await.unwrap();
    }
}
