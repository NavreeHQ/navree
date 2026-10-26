//! `#[module]`: generates `impl Module` from imports, providers, exports and `global`.

use std::sync::Arc;

use navis_kernel::{GraphReport, KernelError, ModuleReport, Navis};
use navis_macros::{injectable, module};

/// The report of module `name`.
fn module_report<'a>(report: &'a GraphReport, name: &str) -> &'a ModuleReport {
    report
        .modules
        .iter()
        .find(|m| m.name == name)
        .unwrap_or_else(|| panic!("module `{name}` in {report:?}"))
}

/// Short provider type names (`module::Repo` → `Repo`).
fn short(keys: impl IntoIterator<Item = impl AsRef<str>>) -> Vec<String> {
    keys.into_iter()
        .map(|k| {
            k.as_ref()
                .rsplit("::")
                .next()
                .unwrap_or_default()
                .to_owned()
        })
        .collect()
}

mod users {
    use super::*;

    #[injectable]
    pub struct Repo;

    #[module(providers = [Repo], exports = [Repo])]
    pub struct DataModule;

    #[injectable]
    pub struct UserService {
        pub repo: Arc<Repo>,
    }

    #[injectable]
    pub struct PasswordHasher;

    #[module(
        imports = [DataModule],
        providers = [UserService, PasswordHasher],
        exports = [UserService],
    )]
    pub struct UsersModule;
}

#[tokio::test]
async fn module_registers_imports_providers_exports() {
    use users::{PasswordHasher, Repo, UserService, UsersModule};

    let report = Navis::app().module(UsersModule).validate().expect("valid");
    let users = module_report(&report, "UsersModule");
    assert_eq!(users.imports, vec!["DataModule"]);
    assert_eq!(
        short(users.providers.iter().map(|p| &p.key)),
        vec!["UserService", "PasswordHasher"]
    );
    assert_eq!(short(&users.exports), vec!["UserService"]);
    assert!(!users.global);
    assert_eq!(
        short(&module_report(&report, "DataModule").exports),
        vec!["Repo"]
    );

    let app = Navis::app().module(UsersModule).boot().await.expect("boot");
    let service = app.ctx().get::<UserService>().await.expect("UserService");
    let repo = app.ctx().get::<Repo>().await.expect("Repo");
    assert!(Arc::ptr_eq(&service.repo, &repo));
    app.ctx()
        .get::<PasswordHasher>()
        .await
        .expect("PasswordHasher");
}

mod private {
    use super::*;
    use users::{PasswordHasher, UsersModule};

    #[injectable]
    pub struct Signup {
        #[allow(dead_code)] // never built: the module fails validation
        pub hasher: Arc<PasswordHasher>,
    }

    /// Needs `PasswordHasher`, which `UsersModule` keeps private.
    #[module(imports = [UsersModule], providers = [Signup])]
    pub struct SignupModule;
}

#[tokio::test]
async fn module_without_export_keeps_provider_private() {
    let errors = Navis::app()
        .module(private::SignupModule)
        .validate()
        .expect_err("PasswordHasher is not exported");
    assert!(
        matches!(
            errors.0.as_slice(),
            [KernelError::NotExported {
                module: "SignupModule",
                owner: "UsersModule",
                ..
            }]
        ),
        "{errors:?}"
    );
}

mod global {
    use super::*;

    #[injectable]
    pub struct Clock;

    #[module(providers = [Clock], exports = [Clock], global)]
    pub struct ClockModule;

    #[injectable]
    pub struct Billing {
        pub clock: Arc<Clock>,
    }

    /// Uses `Clock` without importing `ClockModule`.
    #[module(providers = [Billing])]
    pub struct BillingModule;
}

#[tokio::test]
async fn module_global_flag_shares_exports_without_import() {
    use global::{Billing, BillingModule, Clock, ClockModule};

    let builder = || Navis::app().module(ClockModule).module(BillingModule);
    let report = builder().validate().expect("valid");
    assert!(module_report(&report, "ClockModule").global);
    assert!(!module_report(&report, "BillingModule").global);

    let app = builder().boot().await.expect("boot");
    let billing = app.ctx().get::<Billing>().await.expect("Billing");
    let clock = app.ctx().get::<Clock>().await.expect("Clock");
    assert!(Arc::ptr_eq(&billing.clock, &clock));
}

mod configured {
    use super::*;
    use navis_kernel::{Module, ModuleBuilder, ModuleId};

    pub struct Greeting(pub String);

    /// A configurable module: `for_root` returns a configured instance.
    pub struct GreetingModule;

    impl GreetingModule {
        pub fn for_root(text: &str) -> ConfiguredGreeting {
            ConfiguredGreeting(text.to_owned())
        }
    }

    impl Module for GreetingModule {
        fn register(&self, m: &mut ModuleBuilder) {
            m.value(Greeting("hello".to_owned())).exports::<Greeting>();
        }
    }

    pub struct ConfiguredGreeting(String);

    impl Module for ConfiguredGreeting {
        fn id(&self) -> ModuleId {
            ModuleId::of::<GreetingModule>()
        }

        fn is_configured(&self) -> bool {
            true
        }

        fn register(&self, m: &mut ModuleBuilder) {
            m.value(Greeting(self.0.clone())).exports::<Greeting>();
        }
    }

    #[injectable]
    pub struct Welcome {
        pub greeting: Arc<Greeting>,
    }

    #[module(
        imports = [GreetingModule::for_root("bonjour"), users::DataModule],
        providers = [Welcome],
    )]
    pub struct WelcomeModule;
}

#[tokio::test]
async fn module_imports_accept_expressions() {
    use configured::{Welcome, WelcomeModule};

    let report = Navis::app()
        .module(WelcomeModule)
        .validate()
        .expect("valid");
    assert_eq!(
        module_report(&report, "WelcomeModule").imports,
        vec!["GreetingModule", "DataModule"]
    );

    let app = Navis::app()
        .module(WelcomeModule)
        .boot()
        .await
        .expect("boot");
    let welcome = app.ctx().get::<Welcome>().await.expect("Welcome");
    assert_eq!(welcome.greeting.0, "bonjour");
}

mod hooks {
    use super::*;
    use navis_kernel::{Factory, ModuleBuilder};

    pub struct Label(pub &'static str);

    #[injectable]
    pub struct Banner {
        pub label: Arc<Label>,
        #[inject(name = "motto")]
        pub motto: Arc<Label>,
    }

    /// A module with configuration: `register` adds the providers the
    /// attribute cannot declare (values, factories, named providers).
    #[module(providers = [Banner], exports = [Banner, Label], register = Self::extra)]
    pub struct BannerModule {
        pub label: &'static str,
    }

    impl BannerModule {
        fn extra(&self, m: &mut ModuleBuilder) {
            m.value(Label(self.label)).factory(
                Factory::<Label>::new(|_| async { Ok(Arc::new(Label("ship it"))) }).named("motto"),
            );
        }
    }

    pub struct Counter(pub u32);

    #[module(register = |_, m| { m.value(Counter(7)).exports::<Counter>(); })]
    pub struct ClosureModule;
}

#[tokio::test]
async fn module_register_hook_runs_after_the_declared_keys() {
    use hooks::{Banner, BannerModule, ClosureModule, Counter, Label};

    let root = || BannerModule { label: "navis" };
    let report = Navis::app().module(root()).validate().expect("valid");
    assert_eq!(
        short(&module_report(&report, "BannerModule").exports),
        vec!["Banner", "Label"]
    );

    let app = Navis::app()
        .module(root())
        .module(ClosureModule)
        .boot()
        .await
        .expect("boot");
    let banner = app.ctx().get::<Banner>().await.expect("Banner");
    assert_eq!(banner.label.0, "navis");
    assert_eq!(banner.motto.0, "ship it");
    let label = app.ctx().get::<Label>().await.expect("Label");
    assert!(Arc::ptr_eq(&banner.label, &label));
    assert_eq!(app.ctx().get::<Counter>().await.expect("Counter").0, 7);
}

/// A module with nothing to declare (a grouping root).
#[module]
pub struct EmptyModule;

#[tokio::test]
async fn module_without_keys_is_an_empty_module() {
    let report = Navis::app().module(EmptyModule).validate().expect("valid");
    let empty = module_report(&report, "EmptyModule");
    assert!(empty.imports.is_empty() && empty.providers.is_empty() && empty.exports.is_empty());
}

mod controllers {
    use super::*;
    use http::Method;
    use navis_http::{Controller, RouteDef};

    #[injectable]
    pub struct DealController;

    impl Controller for DealController {
        fn prefix(&self) -> &'static str {
            "/deals"
        }

        fn version(&self) -> Option<&'static str> {
            Some("1")
        }

        fn routes(self: Arc<Self>) -> Vec<RouteDef> {
            vec![
                RouteDef::on(Method::GET, "/", || async { "deals" })
                    .name("deals.index")
                    .public(),
            ]
        }
    }

    #[module(controllers = [DealController])]
    pub struct DealsModule;
}

#[tokio::test]
async fn module_registers_controllers() {
    use controllers::{DealController, DealsModule};
    use navis_http::{HttpModule, HttpOptions, route_table};

    let app = Navis::app()
        .module(HttpModule::for_root(HttpOptions::default()))
        .module(DealsModule)
        .boot()
        .await
        .expect("boot");

    let routes = route_table(app.ctx()).await.expect("route table");
    assert_eq!(routes.len(), 1, "{routes:?}");
    assert_eq!(routes[0].full_path, "/api/v1/deals");
    assert_eq!(routes[0].name.as_deref(), Some("deals.index"));
    assert_eq!(routes[0].module, Some("DealsModule"));
    app.ctx()
        .get::<DealController>()
        .await
        .expect("the controller is a provider of DealsModule");
}
