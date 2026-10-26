//! Provider kinds: global exports, named providers, trait objects,
//! multi-bindings and app-level overrides.

use std::sync::Arc;

use navree_kernel::{BoxError, Factory, KernelError, Navree, ProviderKey};

use crate::support::{Counter, boot_errors, injectable, module};

mod global {
    use super::*;

    pub struct Config(pub &'static str);

    module!(ConfigModule, |m| {
        m.global().value(Config("global")).exports::<Config>();
    });

    injectable!(UserService { config: Config });

    module!(UsersModule, |m| {
        m.provider::<UserService>();
    });

    #[tokio::test]
    async fn global_module_exports_visible_everywhere() {
        let app = Navree::app()
            .module(UsersModule)
            .module(ConfigModule)
            .boot()
            .await
            .expect("boot");
        let service = app.ctx().get::<UserService>().await.expect("UserService");
        assert_eq!(service.config.0, "global");
    }
}

mod optional {
    use super::*;

    pub struct Present;
    pub struct Absent;

    module!(PresentModule, |m| {
        m.value(Present);
    });

    #[tokio::test]
    async fn try_get_is_none_only_when_no_provider_exists() {
        let app = Navree::app()
            .module(PresentModule)
            .boot()
            .await
            .expect("boot");
        assert!(app.ctx().try_get::<Present>().await.is_some());
        assert!(app.ctx().try_get::<Absent>().await.is_none());
    }
}

mod named {
    use super::*;

    #[derive(Debug)]
    pub struct Db(pub &'static str);

    /// Depends on both the default and the "analytics" database.
    pub struct Reports {
        pub main: Arc<Db>,
        pub analytics: Arc<Db>,
    }

    impl navree_kernel::Injectable for Reports {
        fn dependencies() -> Vec<ProviderKey> {
            vec![
                ProviderKey::of::<Db>(),
                ProviderKey::named::<Db>("analytics"),
            ]
        }

        fn construct(
            r: navree_kernel::Resolver,
        ) -> navree_kernel::BoxFuture<'static, Result<Self, KernelError>> {
            Box::pin(async move {
                Ok(Reports {
                    main: r.get::<Db>().await?,
                    analytics: r.get_named::<Db>("analytics").await?,
                })
            })
        }
    }

    module!(DbModule, |m| {
        m.value(Db("main"))
            .factory(
                Factory::<Db>::new(|_| async { Ok::<_, BoxError>(Arc::new(Db("analytics"))) })
                    .named("analytics"),
            )
            .provider::<Reports>();
    });

    module!(OnlyMainDb, |m| {
        m.value(Db("main")).provider::<Reports>();
    });

    #[tokio::test]
    async fn named_providers_coexist() {
        let app = Navree::app().module(DbModule).boot().await.expect("boot");
        let reports = app.ctx().get::<Reports>().await.expect("Reports");
        assert_eq!(reports.main.0, "main");
        assert_eq!(reports.analytics.0, "analytics");
        let analytics = app
            .ctx()
            .get_named::<Db>("analytics")
            .await
            .expect("named Db");
        assert!(Arc::ptr_eq(&analytics, &reports.analytics));
    }

    #[tokio::test]
    async fn missing_named_provider_is_reported_with_its_name() {
        let errors = boot_errors(OnlyMainDb).await;
        assert!(
            matches!(
                errors.0.as_slice(),
                [KernelError::MissingProvider { requested, .. }] if requested.ends_with("Db(\"analytics\")")
            ),
            "{errors:?}"
        );
    }
}

mod trait_objects {
    use super::*;

    pub trait Greeter: Send + Sync {
        fn greet(&self) -> String;
    }

    struct English;

    impl Greeter for English {
        fn greet(&self) -> String {
            "hello".into()
        }
    }

    module!(GreeterModule, |m| {
        m.factory(Factory::<dyn Greeter>::new(|_| async {
            Ok::<_, BoxError>(Arc::new(English) as Arc<dyn Greeter>)
        }))
        .exports::<dyn Greeter>();
    });

    injectable!(Welcome { greeter: dyn Greeter });

    module!(WelcomeModule, |m| {
        m.import(GreeterModule).provider::<Welcome>();
    });

    #[tokio::test]
    async fn trait_object_provider_via_factory() {
        let app = Navree::app()
            .module(WelcomeModule)
            .boot()
            .await
            .expect("boot");
        let welcome = app.ctx().get::<Welcome>().await.expect("Welcome");
        assert_eq!(welcome.greeter.greet(), "hello");
        let greeter = app.ctx().get::<dyn Greeter>().await.expect("dyn Greeter");
        assert!(Arc::ptr_eq(&greeter, &welcome.greeter));
    }
}

mod multi {
    use super::*;

    pub trait Check: Send + Sync {
        fn name(&self) -> &'static str;
    }

    struct Named(&'static str);

    impl Check for Named {
        fn name(&self) -> &'static str {
            self.0
        }
    }

    fn check(name: &'static str) -> Factory<dyn Check> {
        Factory::<dyn Check>::new(move |_| async move {
            Ok::<_, BoxError>(Arc::new(Named(name)) as Arc<dyn Check>)
        })
    }

    module!(DbChecks, |m| {
        m.multi(check("db")).multi(check("db-replica"));
    });

    module!(CacheChecks, |m| {
        m.import(DbChecks).multi(check("cache"));
    });

    #[tokio::test]
    async fn multi_binding_get_all_returns_every_registration() {
        let app = Navree::app().module(CacheChecks).boot().await.expect("boot");
        let names =
            |checks: Vec<Arc<dyn Check>>| checks.iter().map(|c| c.name()).collect::<Vec<_>>();

        let all = app.ctx().get_all::<dyn Check>().await.expect("get_all");
        assert_eq!(
            names(all),
            ["db", "db-replica", "cache"],
            "module dependency order"
        );

        let from_resolver = app
            .ctx()
            .resolver_for(navree_kernel::ModuleId::of::<CacheChecks>())
            .get_all::<dyn Check>()
            .await
            .expect("resolver get_all");
        assert_eq!(names(from_resolver), ["db", "db-replica", "cache"]);

        assert!(
            matches!(
                app.ctx().get::<dyn Check>().await,
                Err(KernelError::MissingProvider { .. })
            ),
            "multi-bindings are only reachable through get_all"
        );
    }

    #[tokio::test]
    async fn get_all_without_registrations_is_empty() {
        let app = Navree::app().boot().await.expect("boot");
        assert!(
            app.ctx()
                .get_all::<dyn Check>()
                .await
                .expect("get_all")
                .is_empty()
        );
    }
}

mod overrides {
    use super::*;

    pub struct Mailer(pub &'static str);
    pub struct Clock(pub u64);

    pub struct MailModule(pub Arc<Counter>);

    impl navree_kernel::Module for MailModule {
        fn register(&self, m: &mut navree_kernel::ModuleBuilder) {
            let built = self.0.clone();
            m.factory(Factory::<Mailer>::new(move |_| {
                built.hit();
                async { Ok::<_, BoxError>(Arc::new(Mailer("smtp"))) }
            }))
            .exports::<Mailer>();
        }
    }

    injectable!(Signup {
        mailer: Mailer,
        clock: Clock
    });

    pub struct SignupModule(pub Arc<Counter>);

    impl navree_kernel::Module for SignupModule {
        fn register(&self, m: &mut navree_kernel::ModuleBuilder) {
            m.import(MailModule(self.0.clone())).provider::<Signup>();
        }
    }

    #[tokio::test]
    async fn override_value_replaces_provider() {
        let built = Arc::new(Counter::default());
        let fake = Arc::new(Mailer("fake"));
        let app = Navree::app()
            .module(SignupModule(built.clone()))
            .override_value(fake.clone())
            .override_value(Arc::new(Clock(42)))
            .boot()
            .await
            .expect("boot");

        let signup = app.ctx().get::<Signup>().await.expect("Signup");
        assert!(
            Arc::ptr_eq(&signup.mailer, &fake),
            "the override replaces the registered Mailer"
        );
        assert_eq!(app.ctx().get::<Mailer>().await.expect("Mailer").0, "fake");
        assert_eq!(
            signup.clock.0, 42,
            "an override for an unregistered type is provided globally"
        );
        assert_eq!(built.get(), 0, "the replaced factory never runs");
    }
}
